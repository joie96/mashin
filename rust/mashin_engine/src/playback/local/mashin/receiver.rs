//! Token-free Mashin sockets share one ordered control writer and one native output.
//! Socket loss retains authorized audio; only explicit server rejection resets playback.

use super::audio_adapter::MashinAudioAdapter;
use super::models::{AudioProfileId, HEADER_BYTES, MAX_INTEGER, MAX_PAYLOAD_BYTES};
use super::protocol::{
    self, AckStatus, Capabilities, ClientMessage, CommandAck, ControlCommand, ProtocolError,
    ServerMessage,
};
use crate::audio::models::AudioEvent;
use crate::logging::Logger;
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type ConnectingAudio = Pin<Box<dyn Future<Output = Result<Socket, String>> + Send>>;

const LOGGER: Logger = Logger::new("mashin");

#[derive(Debug, Clone)]
pub(crate) struct ReceiverConfiguration {
    pub(crate) music_assistant_url: String,
    pub(crate) device_id: String,
    pub(crate) client_name: String,
    pub(crate) audio_profile: AudioProfileId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceiverSnapshot {
    pub(crate) control_connection: ConnectionState,
    pub(crate) audio_connection: ConnectionState,
    pub(crate) player_id: Option<String>,
    pub(crate) audio_profile: AudioProfileId,
}

struct ProfileRequest {
    profile: AudioProfileId,
    response: oneshot::Sender<Result<(), String>>,
}

pub(crate) struct MashinReceiver {
    worker: JoinHandle<()>,
    snapshot: watch::Receiver<ReceiverSnapshot>,
    requests: mpsc::Sender<ProfileRequest>,
}

impl MashinReceiver {
    // Configuration and lifecycle; ownership belongs to the native session.
    pub(crate) fn new(
        runtime: &tokio::runtime::Runtime,
        configuration: ReceiverConfiguration,
        output: MashinAudioAdapter,
    ) -> Result<Self, String> {
        endpoints(&configuration.music_assistant_url)?;
        if configuration.device_id.is_empty()
            || configuration.device_id.len() > 128
            || !configuration.device_id.is_ascii()
            || configuration.client_name.is_empty()
            || configuration.client_name.len() > 512
        {
            return Err("invalid Mashin receiver identity".into());
        }
        let (snapshots, snapshot) = watch::channel(ReceiverSnapshot {
            control_connection: ConnectionState::Connecting,
            audio_connection: ConnectionState::Disconnected,
            player_id: None,
            audio_profile: configuration.audio_profile,
        });
        let (requests, receiver) = mpsc::channel(1);
        let worker = runtime.spawn(run(configuration, output, snapshots, receiver));
        Ok(Self {
            worker,
            snapshot,
            requests,
        })
    }

    // Profile changes are committed only after the server's authoritative response.
    pub(crate) async fn set_audio_profile(&self, profile: AudioProfileId) -> Result<(), String> {
        let (response, receiver) = oneshot::channel();
        self.requests
            .send(ProfileRequest { profile, response })
            .await
            .map_err(|_| "Mashin receiver stopped")?;
        tokio::time::timeout(Duration::from_secs(10), receiver)
            .await
            .map_err(|_| "Mashin profile request timed out")?
            .map_err(|_| "Mashin connection interrupted during profile change")?
    }

    // Connection state is independent of local output state.
    pub(crate) fn snapshot(&self) -> ReceiverSnapshot {
        self.snapshot.borrow().clone()
    }
}

impl Drop for MashinReceiver {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

struct Acknowledgments {
    history: VecDeque<(ControlCommand, CommandAck)>,
    last_command_id: u64,
}

impl Acknowledgments {
    // Idempotence is connection-scoped and bounded by the V1 tombstone budget.
    fn new() -> Self {
        Self {
            history: VecDeque::new(),
            last_command_id: 0,
        }
    }

    fn find(&self, command: &ControlCommand) -> Result<Option<CommandAck>, String> {
        if let Some((previous, ack)) = self
            .history
            .iter()
            .find(|(previous, _)| previous.command_id() == command.command_id())
        {
            return if previous == command {
                Ok(Some(ack.clone()))
            } else {
                Err("Mashin command ID reused with different contents".into())
            };
        }
        if command.command_id() <= self.last_command_id {
            return Err("expired Mashin command ID replayed".into());
        }
        Ok(None)
    }

    fn retain(&mut self, command: ControlCommand, ack: CommandAck) {
        self.last_command_id = command.command_id();
        if self.history.len() == 32 {
            self.history.pop_front();
        }
        self.history.push_back((command, ack));
    }
}

async fn run(
    mut configuration: ReceiverConfiguration,
    output: MashinAudioAdapter,
    snapshots: watch::Sender<ReceiverSnapshot>,
    mut requests: mpsc::Receiver<ProfileRequest>,
) {
    let mut delay = Duration::from_millis(500);
    let mut audio_connection_id = 0;
    let mut connection_attempt = 0_u64;
    loop {
        connection_attempt = connection_attempt.saturating_add(1);
        LOGGER.info(format_args!(
            "Control connection attempt #{connection_attempt}"
        ));
        snapshots.send_modify(|snapshot| {
            snapshot.control_connection = ConnectionState::Connecting;
            snapshot.audio_connection = ConnectionState::Disconnected;
        });
        let connected_at = Instant::now();
        let result = connect(
            &mut configuration,
            &output,
            &snapshots,
            &mut requests,
            &mut audio_connection_id,
        )
        .await;
        let was_connected = snapshots.borrow().control_connection == ConnectionState::Connected;
        match result {
            Ok(()) => LOGGER.info("Control connection ended"),
            Err(error) => LOGGER.info(format_args!(
                "{} on attempt #{connection_attempt}: {error}",
                if was_connected {
                    "Control connection lost"
                } else {
                    "Control connection attempt failed"
                },
            )),
        }
        snapshots.send_modify(|snapshot| {
            snapshot.control_connection = ConnectionState::Disconnected;
            snapshot.audio_connection = ConnectionState::Disconnected;
        });
        if connected_at.elapsed() >= Duration::from_secs(15) {
            delay = Duration::from_millis(500);
        }
        let retained = output.snapshot();
        LOGGER.info(format_args!(
            "Control reconnect scheduled in {} ms; retained_stream={:?}, buffered={} ms",
            delay.as_millis(),
            retained.resume.current_stream_id,
            retained.buffered_duration_millis,
        ));
        let retry = tokio::time::sleep(delay);
        tokio::pin!(retry);
        loop {
            tokio::select! {
                _ = &mut retry => break,
                request = requests.recv() => match request {
                    Some(request) => { let _ = request.response.send(Err("Mashin is disconnected".into())); }
                    None => return,
                }
            }
        }
        delay = (delay * 2).min(Duration::from_secs(8));
    }
}

async fn connect(
    configuration: &mut ReceiverConfiguration,
    output: &MashinAudioAdapter,
    snapshots: &watch::Sender<ReceiverSnapshot>,
    requests: &mut mpsc::Receiver<ProfileRequest>,
    audio_connection_id: &mut u64,
) -> Result<(), String> {
    let (control_url, audio_url) = endpoints(&configuration.music_assistant_url)?;
    let mut events = output.subscribe();
    let mut control = open_socket(control_url, protocol::MAX_CONTROL_BYTES).await?;
    LOGGER.debug("Control WebSocket connected; sending hello");
    let initial = output.snapshot();
    let mut last_event_id = initial.audible_events.last().map_or(0, |(id, _)| *id);
    send(
        &mut control,
        ClientMessage::Hello {
            device_id: configuration.device_id.clone(),
            client_name: configuration.client_name.clone(),
            client_version: env!("CARGO_PKG_VERSION").into(),
            resume_state: initial.resume,
            capabilities: Capabilities::default(),
            audio_profile: configuration.audio_profile,
        },
        None,
    )
    .await?;
    let envelope = tokio::time::timeout(Duration::from_secs(30), receive_json(&mut control, None))
        .await
        .map_err(|_| "Mashin welcome timed out")??;
    let welcome = match envelope.message {
        ServerMessage::Welcome(welcome) => welcome,
        ServerMessage::Error { error, .. } => {
            LOGGER.warn(format_args!(
                "Control handshake rejected: code={} retryable={}: {}",
                error.code, error.is_retryable, error.message,
            ));
            if matches!(
                error.code.as_str(),
                "cache_unavailable" | "stale_playback" | "stream_unavailable"
            ) && !(error.code == "stream_unavailable" && error.is_retryable)
            {
                LOGGER.info(format_args!(
                    "Stopping retained playback after handshake rejection: code={}; control reconnect will continue",
                    error.code,
                ));
                output
                    .execute(ControlCommand::Stop {
                        command_id: 0,
                        stream_id: None,
                    })
                    .await?;
            }
            return Err("Mashin hello was rejected".into());
        }
        _ => return Err("expected Mashin welcome".into()),
    };
    let epoch = envelope
        .connection_epoch
        .filter(|epoch| !epoch.is_empty())
        .ok_or("Mashin welcome has no connection epoch")?;
    output.reconcile(welcome.clone()).await?;
    configuration.audio_profile = welcome.selected_audio_profile;
    LOGGER.info(format_args!(
        "Control handshake completed: player={}, audio_profile={:?}",
        welcome.player_id, welcome.selected_audio_profile,
    ));
    snapshots.send_modify(|snapshot| {
        snapshot.control_connection = ConnectionState::Connected;
        snapshot.player_id = Some(welcome.player_id.clone());
        snapshot.audio_profile = welcome.selected_audio_profile;
        snapshot.audio_connection = ConnectionState::Connecting;
    });
    *audio_connection_id = increment(*audio_connection_id)?;
    let mut connecting = Some(connect_audio(
        audio_url.clone(),
        configuration.device_id.clone(),
        epoch.clone(),
        welcome.audio_binding_id,
        *audio_connection_id,
        output.clone(),
    ));
    let mut audio = None;
    let mut acknowledgments = Acknowledgments::new();
    let mut sequence = 0;
    let mut reports = tokio::time::interval(Duration::from_millis(250));
    reports.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ping = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut outstanding_ping: Option<(Vec<u8>, Instant)> = None;
    let mut pending_binding: Option<(String, Instant)> = None;
    let mut pending_profile: Option<(String, Instant, ProfileRequest)> = None;
    loop {
        tokio::select! {
            message = control.next() => {
                match message.ok_or("Mashin control closed")?.map_err(|_| "read Mashin control failed")? {
                    Message::Ping(payload) => send_frame(&mut control, Message::Pong(payload)).await?,
                    Message::Pong(payload) => { if outstanding_ping.as_ref().is_some_and(|(expected, _)| expected.as_slice() == payload.as_ref()) { outstanding_ping = None; } }
                    Message::Text(text) => match protocol::decode(&text, Some(&epoch))?.message {
                        ServerMessage::Command(command) => {
                            let ack = if let Some(ack) = acknowledgments.find(&command)? { ack } else {
                                let result = output.execute(command.clone()).await;
                                let ack = CommandAck { command_id: command.command_id(), status: if result.is_ok() { AckStatus::Accepted } else { AckStatus::Rejected }, error: result.err().map(|_| ProtocolError::command_failed()) };
                                acknowledgments.retain(command, ack.clone());
                                ack
                            };
                            send(&mut control, ClientMessage::CommandAck(ack), Some(&epoch)).await?;
                            report(&mut control, &epoch, output, &mut sequence, &mut last_event_id, audio.is_some(), *audio_connection_id).await?;
                        }
                        ServerMessage::AudioBinding { request_id, audio_binding_id } => {
                            if pending_binding.as_ref().map(|(pending, _)| pending.as_str()) != Some(&request_id) { return Err("unsolicited Mashin audio binding".into()); }
                            pending_binding = None;
                            *audio_connection_id = increment(*audio_connection_id)?;
                            LOGGER.debug(format_args!("Audio reconnect binding received; starting connection #{audio_connection_id}"));
                            connecting = Some(connect_audio(audio_url.clone(), configuration.device_id.clone(), epoch.clone(), audio_binding_id, *audio_connection_id, output.clone()));
                        }
                        ServerMessage::AudioProfileChanged { request_id, status, selected_audio_profile, error, .. } => {
                            let (expected, _, request) = pending_profile.take().ok_or("unsolicited Mashin profile response")?;
                            if request_id != expected { return Err("Mashin profile response ID mismatch".into()); }
                            let result = if status == AckStatus::Accepted && error.is_none() && selected_audio_profile == request.profile {
                                configuration.audio_profile = selected_audio_profile;
                                snapshots.send_modify(|snapshot| snapshot.audio_profile = selected_audio_profile);
                                Ok(())
                            } else { Err("Mashin profile change rejected".into()) };
                            let _ = request.response.send(result);
                        }
                        ServerMessage::Error { error, stream_id, .. } => {
                            LOGGER.error(format_args!(
                                "Server error: code={} stream_id={stream_id:?} retryable={}: {}",
                                error.code, error.is_retryable, error.message
                            ));
                            if matches!(error.code.as_str(), "cache_unavailable" | "stale_playback" | "stream_unavailable") && (stream_id.is_none() || stream_id == output.snapshot().resume.current_stream_id) {
                                if error.code == "stream_unavailable" && error.is_retryable {
                                    return Err("Mashin audio transport interrupted".into());
                                }
                                LOGGER.info(format_args!("Stopping retained playback after server error: code={}; control reconnect will continue", error.code));
                                output.execute(ControlCommand::Stop { command_id: 0, stream_id: None }).await?;
                                return Err("Mashin server revoked retained playback".into());
                            }
                        }
                        _ => return Err("unexpected Mashin control message".into()),
                    },
                    Message::Close(frame) => return Err(format!("Mashin control closed by peer (code={:?})", frame.as_ref().map(|frame| frame.code))),
                    _ => return Err("unexpected Mashin control frame".into()),
                }
            }
            result = finish_connecting(&mut connecting) => {
                connecting = None;
                audio = Some(result.map_err(|error| format!("Mashin audio setup failed: {error}"))?);
                LOGGER.info(format_args!("Audio connection established: connection_id={audio_connection_id}"));
                snapshots.send_modify(|snapshot| snapshot.audio_connection = ConnectionState::Connected);
                report(&mut control, &epoch, output, &mut sequence, &mut last_event_id, true, *audio_connection_id).await?;
            }
            message = receive_audio(&mut audio) => {
                match message {
                    Ok(Message::Binary(bytes)) => {
                        let chunk = crate::playback::local::mashin::streams::parse_chunk(bytes.to_vec().into())?;
                        let stream_id = chunk.stream_id;
                        let chunk_index = chunk.index;
                        if !output.snapshot().resume.received_streams.iter().any(|cursor| cursor.stream_id == stream_id) { continue; }
                        let (snapshot, error) = match output.retain(chunk).await {
                            Ok(snapshot) => (snapshot, None),
                            Err(error) => (output.snapshot(), Some(error)),
                        };
                        let next_chunk_index = snapshot.resume.received_streams.iter().find(|cursor| cursor.stream_id == stream_id).map_or(1, |cursor| cursor.next_chunk_index);
                        sequence = increment(sequence)?;
                        send(&mut control, ClientMessage::ChunkAck { sequence, stream_id, audio_connection_id: *audio_connection_id, chunk_index, next_chunk_index,
                            status: if error.is_none() { AckStatus::Accepted } else { AckStatus::Rejected }, buffer_capacity: snapshot.buffer_capacity,
                            error: error.map(|_| ProtocolError { code: "stream_failed".into(), message: "Audio chunk could not be retained".into(), is_retryable: false, retry_after_millis: None }) }, Some(&epoch)).await?;
                    }
                    Ok(Message::Ping(payload)) => { send_frame(audio.as_mut().unwrap(), Message::Pong(payload)).await?; }
                    Ok(Message::Pong(_)) => {}
                    message => {
                        match message {
                            Err(error) => LOGGER.info(format_args!("Audio connection #{audio_connection_id} lost: {error}")),
                            Ok(Message::Close(frame)) => LOGGER.info(format_args!("Audio connection #{audio_connection_id} closed by peer (code={:?})", frame.as_ref().map(|frame| frame.code))),
                            _ => LOGGER.info(format_args!("Audio connection #{audio_connection_id} interrupted by unexpected frame")),
                        }
                        audio = None;
                        snapshots.send_modify(|snapshot| snapshot.audio_connection = ConnectionState::Connecting);
                        let request_id = Uuid::new_v4().to_string();
                        sequence = increment(sequence)?;
                        send(&mut control, ClientMessage::AudioReconnectRequest { sequence, request_id: request_id.clone() }, Some(&epoch)).await?;
                        LOGGER.info(format_args!("Audio reconnect binding requested after connection #{audio_connection_id}"));
                        pending_binding = Some((request_id, Instant::now()));
                    }
                }
            }
            _ = reports.tick() => {
                if outstanding_ping.as_ref().is_some_and(|(_, sent)| sent.elapsed() >= Duration::from_secs(15)) { return Err("Mashin control liveness timed out".into()); }
                if pending_binding.as_ref().is_some_and(|(_, sent)| sent.elapsed() >= Duration::from_secs(5)) { return Err("Mashin audio binding timed out".into()); }
                if pending_profile.as_ref().is_some_and(|(_, sent, _)| sent.elapsed() >= Duration::from_secs(5)) { return Err("Mashin profile response timed out".into()); }
                report(&mut control, &epoch, output, &mut sequence, &mut last_event_id, audio.is_some(), *audio_connection_id).await?;
            }
            _ = ping.tick(), if outstanding_ping.is_none() => {
                let payload = Uuid::new_v4().as_bytes().to_vec();
                send_frame(&mut control, Message::Ping(payload.clone().into())).await?;
                outstanding_ping = Some((payload, Instant::now()));
            }
            event = events.recv() => {
                if matches!(event, Err(broadcast::error::RecvError::Closed)) { return Err("Mashin audio worker stopped".into()); }
                report(&mut control, &epoch, output, &mut sequence, &mut last_event_id, audio.is_some(), *audio_connection_id).await?;
            }
            request = requests.recv() => {
                let request = request.ok_or("Mashin receiver stopped")?;
                if pending_profile.is_some() { let _ = request.response.send(Err("Mashin profile change already pending".into())); continue; }
                let request_id = Uuid::new_v4().to_string();
                sequence = increment(sequence)?;
                send(&mut control, ClientMessage::SetAudioProfile { sequence, request_id: request_id.clone(), audio_profile: request.profile }, Some(&epoch)).await?;
                pending_profile = Some((request_id, Instant::now(), request));
            }
        }
    }
}

// Every write is bounded so a stalled peer cannot stall output ownership or shutdown.
async fn send(
    socket: &mut Socket,
    message: ClientMessage,
    epoch: Option<&str>,
) -> Result<(), String> {
    send_frame(
        socket,
        Message::Text(protocol::encode(message, epoch)?.into()),
    )
    .await
}

async fn send_frame(socket: &mut Socket, message: Message) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| "Mashin write timed out")?
        .map_err(|_| "Mashin websocket write failed".into())
}

async fn report(
    control: &mut Socket,
    epoch: &str,
    output: &MashinAudioAdapter,
    sequence: &mut u64,
    last_event_id: &mut u64,
    is_audio_ready: bool,
    audio_connection_id: u64,
) -> Result<(), String> {
    let snapshot = output.snapshot();
    if snapshot
        .audible_events
        .first()
        .is_some_and(|(id, _)| *id > last_event_id.saturating_add(1))
    {
        return Err("Mashin audible event history lost".into());
    }
    for (id, event) in &snapshot.audible_events {
        if *id <= *last_event_id {
            continue;
        }
        *sequence = increment(*sequence)?;
        let message = match event {
            AudioEvent::PreparedActivated {
                activation,
                output_revision,
            } => ClientMessage::PreparedActivated {
                sequence: *sequence,
                activation: super::protocol::PreparedActivation {
                    previous_stream_id: activation.previous_stream_id,
                    stream_id: activation.stream_id,
                    media_revision: activation.media_revision,
                    previous_position_frame: activation.previous_position_frame,
                },
                output_revision: *output_revision,
                output_state: super::models::OutputState::Buffering,
                position_frame: 0,
            },
            AudioEvent::TrackStarted {
                stream_id,
                media_revision,
                output_revision,
            } => ClientMessage::TrackStarted {
                sequence: *sequence,
                stream_id: *stream_id,
                media_revision: *media_revision,
                output_revision: *output_revision,
                position_frame: 0,
            },
            AudioEvent::TrackEnded { stream_id } => ClientMessage::TrackEnded {
                sequence: *sequence,
                stream_id: *stream_id,
            },
        };
        send(control, message, Some(epoch)).await?;
        *last_event_id = *id;
    }
    if is_audio_ready {
        *sequence = increment(*sequence)?;
        send(
            control,
            ClientMessage::BufferStatus {
                sequence: *sequence,
                audio_connection_id,
                buffer_capacity: snapshot.buffer_capacity,
            },
            Some(epoch),
        )
        .await?;
    }
    *sequence = increment(*sequence)?;
    let state = snapshot.resume;
    send(
        control,
        ClientMessage::PlayerState {
            sequence: *sequence,
            stream_id: state.current_stream_id,
            output_revision: state.output_revision,
            output_state: state.output_state,
            position_frame: state.position_frame,
            volume_percent: state.volume_percent,
            is_muted: state.is_muted,
            error: snapshot.error,
        },
        Some(epoch),
    )
    .await
}

async fn open_socket(url: String, message_limit: usize) -> Result<Socket, String> {
    let configuration = WebSocketConfig::default()
        .max_message_size(Some(message_limit))
        .max_frame_size(Some(message_limit));
    tokio::time::timeout(
        Duration::from_secs(30),
        tokio_tungstenite::connect_async_with_config(url, Some(configuration), false),
    )
    .await
    .map_err(|_| "Mashin connection timed out")?
    .map(|(socket, _)| socket)
    .map_err(|_| "Mashin websocket connection failed".into())
}

async fn receive_json(
    socket: &mut Socket,
    epoch: Option<&str>,
) -> Result<protocol::ServerEnvelope, String> {
    loop {
        match socket
            .next()
            .await
            .ok_or("Mashin websocket closed")?
            .map_err(|_| "read Mashin handshake failed")?
        {
            Message::Text(text) => return protocol::decode(&text, epoch),
            Message::Ping(payload) => send_frame(socket, Message::Pong(payload)).await?,
            Message::Pong(_) => {}
            _ => return Err("unexpected Mashin handshake frame".into()),
        }
    }
}

fn connect_audio(
    url: String,
    device_id: String,
    epoch: String,
    binding: String,
    audio_connection_id: u64,
    output: MashinAudioAdapter,
) -> ConnectingAudio {
    Box::pin(async move {
        LOGGER.info(format_args!(
            "Audio connection attempt: connection_id={audio_connection_id}"
        ));
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut socket = open_socket(url, HEADER_BYTES + MAX_PAYLOAD_BYTES).await?;
            send(
                &mut socket,
                ClientMessage::AudioHello {
                    device_id,
                    audio_binding_id: binding,
                    audio_connection_id,
                    received_streams: output.snapshot().resume.received_streams,
                },
                Some(&epoch),
            )
            .await?;
            if !matches!(
                receive_json(&mut socket, Some(&epoch)).await?.message,
                ServerMessage::AudioReady {}
            ) {
                return Err("expected Mashin audio_ready".into());
            }
            Ok(socket)
        })
        .await
        .map_err(|_| "Mashin audio setup timed out")?
    })
}

async fn finish_connecting(connecting: &mut Option<ConnectingAudio>) -> Result<Socket, String> {
    match connecting {
        Some(connecting) => connecting.await,
        None => std::future::pending().await,
    }
}

async fn receive_audio(audio: &mut Option<Socket>) -> Result<Message, String> {
    match audio {
        Some(socket) => socket
            .next()
            .await
            .ok_or("Mashin audio closed")?
            .map_err(|_| "read Mashin audio failed".into()),
        None => std::future::pending().await,
    }
}

fn endpoints(base_url: &str) -> Result<(String, String), String> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| "invalid Music Assistant URL")?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        _ => return Err("invalid Music Assistant URL scheme".into()),
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Mashin transport URLs must not contain credentials".into());
    }
    url.set_scheme(scheme)
        .map_err(|_| "invalid Mashin websocket URL")?;
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&format!("{path}/mashin-stream/v3/control"));
    let control = url.to_string();
    url.set_path(&format!("{path}/mashin-stream/v3/audio"));
    Ok((control, url.to_string()))
}

fn increment(value: u64) -> Result<u64, String> {
    value
        .checked_add(1)
        .filter(|value| *value <= MAX_INTEGER)
        .ok_or("Mashin counter exhausted".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_message(kind: &str, epoch: &str, payload: serde_json::Value) -> Message {
        let text = serde_json::json!({"protocol_version":3,"type":kind,"message_id":Uuid::new_v4().to_string(),"connection_epoch":epoch,"payload":payload}).to_string();
        protocol::decode(&text, Some(epoch)).expect("valid server test message");
        Message::Text(text.into())
    }

    async fn next_json(socket: &mut WebSocketStream<TcpStream>, kind: &str) -> serde_json::Value {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => {
                    let message: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if message["type"] == kind {
                        return message;
                    }
                }
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                _ => panic!("unexpected client frame"),
            }
        }
    }

    #[test]
    fn live_handshake_replay_and_reconnect_preserve_native_output() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
                let server = tokio::spawn(async move {
                    for generation in 0..2 {
                        let (stream, _) = listener.accept().await.unwrap();
                        let mut control = tokio_tungstenite::accept_async(stream).await.unwrap();
                        let hello = next_json(&mut control, "hello").await;
                        assert!(hello["connection_epoch"].is_null());
                        assert_eq!(hello["payload"]["resume_state"]["volume_percent"], if generation == 0 { 30 } else { 42 });
                        assert_eq!(hello["payload"]["resume_state"]["output_revision"], 0);
                        assert!(hello["payload"]["resume_state"]["current_stream_id"].is_null());
                        assert_eq!(hello["payload"]["resume_state"]["output_state"], "idle");
                        assert_eq!(hello["payload"]["resume_state"]["position_frame"], 0);
                        assert_eq!(hello["payload"]["resume_state"]["received_streams"], serde_json::json!([]));
                        let epoch = format!("epoch-{generation}");
                        control.send(server_message("welcome", &epoch, serde_json::json!({
                            "player_id":"mashin-test", "provider_version":"1", "server_revision":"test",
                            "limits":protocol::ProtocolLimits::default(), "audio_profiles":AudioProfileId::ALL,
                            "selected_audio_profile":"flac", "audio_binding_id":format!("binding-{generation}"),
                            "current_stream":null, "desired_output_state":"idle", "media_revision":0, "prepared_next":null
                        }))).await.unwrap();
                        let (stream, _) = listener.accept().await.unwrap();
                        let mut audio = tokio_tungstenite::accept_async(stream).await.unwrap();
                        let hello = next_json(&mut audio, "audio_hello").await;
                        assert_eq!(hello["connection_epoch"], epoch);
                        assert_eq!(hello["payload"]["audio_connection_id"], generation + 1);
                        audio.send(server_message("audio_ready", &epoch, serde_json::json!({}))).await.unwrap();
                        let grant = next_json(&mut control, "buffer_status").await;
                        assert_eq!(grant["payload"]["buffer_capacity"], MAX_PAYLOAD_BYTES);
                        let command = server_message("set_volume", &epoch, serde_json::json!({"command_id":1,"volume_percent":42 + generation}));
                        control.send(command.clone()).await.unwrap();
                        let ack = next_json(&mut control, "command_ack").await;
                        assert_eq!(ack["payload"]["status"], "accepted");
                        control.send(command).await.unwrap();
                        assert_eq!(next_json(&mut control, "command_ack").await["payload"], ack["payload"]);
                        let state = next_json(&mut control, "player_state").await;
                        assert_eq!(state["payload"]["output_revision"], 0);
                        assert_eq!(state["payload"]["volume_percent"], 42 + generation);
                        let current = super::super::models::test_stream();
                        let prepared = super::super::models::test_stream();
                        control.send(server_message("set_media", &epoch, serde_json::json!({
                            "command_id":2, "media_revision":1, "current":current,
                            "prepared":prepared, "initial_output_state":"paused"
                        }))).await.unwrap();
                        assert_eq!(next_json(&mut control, "command_ack").await["payload"]["status"], "accepted");
                        let state = next_json(&mut control, "player_state").await;
                        assert_eq!(state["payload"]["stream_id"], current.stream_id.to_string());
                        assert_eq!(state["payload"]["output_state"], "paused");
                        control.send(server_message("stop", &epoch, serde_json::json!({
                            "command_id":3, "stream_id":current.stream_id
                        }))).await.unwrap();
                        assert_eq!(next_json(&mut control, "command_ack").await["payload"]["status"], "accepted");
                        let state = next_json(&mut control, "player_state").await;
                        assert!(state["payload"]["stream_id"].is_null());
                        assert_eq!(state["payload"]["output_state"], "idle");
                        assert_eq!(state["payload"]["output_revision"], 0);
                        assert_eq!(state["payload"]["position_frame"], 0);
                        if generation == 0 {
                            control.send(server_message("error", &epoch, serde_json::json!({
                                "correlation_id":null, "stream_id":null, "audio_connection_id":1,
                                "error":{"code":"stream_unavailable", "message":"Audio response deadline expired", "is_retryable":true, "retry_after_millis":null}
                            }))).await.unwrap();
                        } else {
                            control.close(None).await.unwrap();
                            audio.close(None).await.unwrap();
                        }
                    }
                });
                let receiver = MashinReceiver::new(&runtime, ReceiverConfiguration { music_assistant_url: format!("http://{address}"), device_id:"test-device".into(), client_name:"Test".into(), audio_profile:AudioProfileId::Flac }, output.clone()).unwrap();
                server.await.unwrap();
                assert_eq!(output.snapshot().resume.volume_percent, 43);
                assert_eq!(output.snapshot().resume.output_revision, 0);
                drop(receiver);
            }).await.unwrap();
        });
    }

    #[test]
    fn retryable_handshake_error_preserves_registered_media() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
                output.execute(ControlCommand::SetMedia {
                    command_id: 1,
                    current: Some(super::super::models::test_stream()),
                    prepared: Some(super::super::models::test_stream()),
                    media_revision: 1,
                    initial_output_state: super::super::models::OutputState::Paused,
                }).await.unwrap();
                let expected = serde_json::to_value(output.snapshot().resume).unwrap();
                let server = tokio::spawn(async move {
                    for _ in 0..2 {
                        let (stream, _) = listener.accept().await.unwrap();
                        let mut control = tokio_tungstenite::accept_async(stream).await.unwrap();
                        let hello = next_json(&mut control, "hello").await;
                        assert_eq!(hello["payload"]["resume_state"], expected);
                        control.send(server_message("error", "handshake-error", serde_json::json!({
                            "correlation_id":null, "stream_id":null, "audio_connection_id":null,
                            "error":{"code":"stream_unavailable", "message":"Temporarily unavailable", "is_retryable":true, "retry_after_millis":null}
                        }))).await.unwrap();
                    }
                });
                let receiver = MashinReceiver::new(&runtime, ReceiverConfiguration {
                    music_assistant_url: format!("http://{address}"), device_id: "test-device".into(),
                    client_name: "Test".into(), audio_profile: AudioProfileId::Flac,
                }, output).unwrap();
                server.await.unwrap();
                drop(receiver);
            }).await.unwrap();
        });
    }

    #[test]
    fn endpoints_use_ma_origin_without_authentication_or_sendspin_port() {
        assert_eq!(
            endpoints("https://ma.example:8095/proxy/").unwrap(),
            (
                "wss://ma.example:8095/proxy/mashin-stream/v3/control".into(),
                "wss://ma.example:8095/proxy/mashin-stream/v3/audio".into()
            )
        );
        assert_eq!(
            endpoints("http://ma.example:8095").unwrap(),
            (
                "ws://ma.example:8095/mashin-stream/v3/control".into(),
                "ws://ma.example:8095/mashin-stream/v3/audio".into()
            )
        );
        assert!(endpoints("http://ma.example/?token=secret").is_err());
        assert!(endpoints("http://user:secret@ma.example").is_err());
    }

    #[test]
    fn command_replays_are_exact_and_history_is_bounded() {
        let mut acks = Acknowledgments::new();
        for command_id in 1..=33 {
            let command = ControlCommand::SetVolume {
                command_id,
                volume_percent: 30,
            };
            assert!(acks.find(&command).unwrap().is_none());
            acks.retain(
                command,
                CommandAck {
                    command_id,
                    status: AckStatus::Accepted,
                    error: None,
                },
            );
        }
        assert_eq!(acks.history.len(), 32);
        assert!(acks
            .find(&ControlCommand::SetVolume {
                command_id: 33,
                volume_percent: 30
            })
            .unwrap()
            .is_some());
        assert!(acks
            .find(&ControlCommand::SetVolume {
                command_id: 33,
                volume_percent: 31
            })
            .is_err());
        assert!(acks
            .find(&ControlCommand::SetVolume {
                command_id: 1,
                volume_percent: 30
            })
            .is_err());
    }
}
