use base64::prelude::*;
use once_cell::sync::Lazy;
use sendspin::audio::decode::{Decoder, FlacDecoder, OpusDecoder, PcmDecoder, PcmEndian};
use sendspin::audio::{AudioBuffer, AudioFormat, Codec, SyncedPlayer, SyncedPlayerConfig};
use sendspin::protocol::client::{ConnectionGuard, Controller, WsSender};
use sendspin::protocol::messages::{
    ArtworkFormatRequest, AudioFormatSpec, ClientSyncState, GoodbyeReason, Message,
    PlaybackState, PlayerCommandType, PlayerFormatRequest, PlayerState, PlayerV1Support,
    RepeatMode, VisualizerFormatRequest,
};
use sendspin::ProtocolClientBuilder;
use serde::{Deserialize, Serialize};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

// Android JNI types are needed to receive application context from MainActivity.
#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JObject};
#[cfg(target_os = "android")]
use jni::JNIEnv;
#[cfg(target_os = "android")]
use std::ffi::c_void;

static ENGINE: Lazy<Mutex<SendspinEngine>> = Lazy::new(|| Mutex::new(SendspinEngine::new()));
static LAST_ERROR: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
type EventCallback = extern "C" fn(*mut c_char);
static EVENT_CALLBACK: Lazy<Mutex<Option<EventCallback>>> = Lazy::new(|| Mutex::new(None));
static LAST_SNAPSHOT_EVENT_JSON: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));
// Keep a global ref so Android context stays alive for native audio backend usage.
#[cfg(target_os = "android")]
static ANDROID_APP_CONTEXT: Lazy<Mutex<Option<GlobalRef>>> = Lazy::new(|| Mutex::new(None));

fn log_error(message: &str) {
    eprintln!("[mashin_sendspin_bridge][E] {}", message);
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "unknown panic payload".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfig {
    sendspin_url: String,
    client_id: String,
    client_name: String,
    buffer_capacity_bytes: u64,
    preferred_codec: String,
    initial_volume: u8,
    initial_muted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    connection_state: String,
    playback_state: String,
    metadata_timestamp_micros: Option<i64>,
    metadata_title: Option<String>,
    metadata_artist: Option<String>,
    metadata_album_artist: Option<String>,
    metadata_album: Option<String>,
    metadata_artwork_url: Option<String>,
    metadata_year: Option<u32>,
    metadata_track_number: Option<u32>,
    metadata_track_progress_millis: Option<i64>,
    metadata_track_duration_millis: Option<i64>,
    metadata_playback_speed_millis: Option<i32>,
    volume: u8,
    muted: bool,
    repeat_mode: Option<String>,
    shuffle_enabled: Option<bool>,
    seek_max_millis: Option<u64>,
    active_codec: Option<String>,
    active_sample_rate_hz: Option<u32>,
    active_channels: Option<u8>,
    active_bit_depth: Option<u8>,
    group_id: Option<String>,
    group_name: Option<String>,
    server_name: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            connection_state: "offline".to_string(),
            playback_state: "uninitialized".to_string(),
            metadata_timestamp_micros: None,
            metadata_title: None,
            metadata_artist: None,
            metadata_album_artist: None,
            metadata_album: None,
            metadata_artwork_url: None,
            metadata_year: None,
            metadata_track_number: None,
            metadata_track_progress_millis: None,
            metadata_track_duration_millis: None,
            metadata_playback_speed_millis: None,
            volume: 50,
            muted: false,
            repeat_mode: None,
            shuffle_enabled: None,
            seek_max_millis: None,
            active_codec: None,
            active_sample_rate_hz: None,
            active_channels: None,
            active_bit_depth: None,
            group_id: None,
            group_name: None,
            server_name: None,
        }
    }
}

struct SendspinEngine {
    runtime: Runtime,
    config: Option<RuntimeConfig>,
    snapshot: Arc<Mutex<Snapshot>>,
    controller: Option<Controller>,
    sender: Option<WsSender>,
    guard: Option<ConnectionGuard>,
    io_task: Option<JoinHandle<()>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamFormatsRequest {
    player: Option<PlayerFormatRequest>,
    artwork: Option<ArtworkFormatRequest>,
    visualizer: Option<VisualizerFormatRequest>,
}

enum ActiveDecoder {
    Pcm(PcmDecoder),
    Opus(OpusDecoder),
    Flac(FlacDecoder),
}

impl ActiveDecoder {
    fn decode(&self, data: &[u8]) -> Result<Arc<[i32]>, String> {
        match self {
            ActiveDecoder::Pcm(decoder) => decoder.decode(data).map_err(|err| err.to_string()),
            ActiveDecoder::Opus(decoder) => decoder.decode(data).map_err(|err| err.to_string()),
            ActiveDecoder::Flac(decoder) => decoder.decode(data).map_err(|err| err.to_string()),
        }
    }
}

impl SendspinEngine {
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime could not be created");

        Self {
            runtime,
            config: None,
            snapshot: Arc::new(Mutex::new(Snapshot::default())),
            controller: None,
            sender: None,
            guard: None,
            io_task: None,
        }
    }

    fn init(&mut self, config_json: &str) -> Result<(), String> {
        let parsed: RuntimeConfig =
            serde_json::from_str(config_json).map_err(|err| format!("invalid config: {err}"))?;

        self.config = Some(parsed.clone());
        self.set_snapshot(|snapshot| {
            snapshot.connection_state = "offline".to_string();
            snapshot.playback_state = "idle".to_string();
            snapshot.metadata_timestamp_micros = None;
            snapshot.metadata_title = None;
            snapshot.metadata_artist = None;
            snapshot.metadata_album_artist = None;
            snapshot.metadata_album = None;
            snapshot.metadata_artwork_url = None;
            snapshot.metadata_year = None;
            snapshot.metadata_track_number = None;
            snapshot.metadata_track_progress_millis = None;
            snapshot.metadata_track_duration_millis = None;
            snapshot.metadata_playback_speed_millis = None;
            snapshot.volume = parsed.initial_volume.clamp(0, 100);
            snapshot.muted = parsed.initial_muted;
            snapshot.repeat_mode = None;
            snapshot.shuffle_enabled = None;
            snapshot.seek_max_millis = None;
            snapshot.active_codec = Some(parsed.preferred_codec.clone());
            snapshot.active_sample_rate_hz = None;
            snapshot.active_channels = None;
            snapshot.active_bit_depth = None;
            snapshot.group_id = None;
            snapshot.group_name = None;
            snapshot.server_name = None;
        });

        Ok(())
    }

    fn connect(&mut self) -> Result<(), String> {
        if self.controller.is_some() {
            return Ok(());
        }

        let config = self
            .config
            .clone()
            .ok_or_else(|| "engine is not initialized".to_string())?;

        self.set_snapshot(|snapshot| {
            snapshot.connection_state = "connecting".to_string();
        });

        let snapshot = Arc::clone(&self.snapshot);

        let (controller, sender, guard, io_task, server_name) = self.runtime.block_on(async {
            let builder = ProtocolClientBuilder::builder()
                .client_id(config.client_id.clone())
                .name(config.client_name.clone())
                .player_v1_support(PlayerV1Support {
                    supported_formats: vec![AudioFormatSpec {
                        codec: config.preferred_codec.clone(),
                        channels: 2,
                        sample_rate: 48_000,
                        bit_depth: 24,
                    }],
                    buffer_capacity: config
                        .buffer_capacity_bytes
                        .min(u32::MAX as u64) as u32,
                    supported_commands: vec!["volume".to_string(), "mute".to_string()],
                })
                .initial_player_state(PlayerState {
                    volume: Some(config.initial_volume.clamp(0, 100)),
                    muted: Some(config.initial_muted),
                    ..Default::default()
                })
                .metadata()
                .controller()
                .build();

            let client = builder
                .connect(&config.sendspin_url)
                .await
                .map_err(|err| format!("connect failed: {err}"))?;

            let server_name = client.server_hello().name.clone();
            let mut connection = client.split();

            let controller = connection
                .controller
                .take()
                .ok_or_else(|| "server did not grant controller@v1 role".to_string())?;
            let sender = connection.sender.clone();

            let mut messages = connection.messages;
            let mut audio = connection.audio;
            let clock_sync = connection.clock_sync;
            let snapshot_for_task = Arc::clone(&snapshot);

            let io_task = tokio::spawn(async move {
                let mut decoder: Option<ActiveDecoder> = None;
                let mut audio_format: Option<AudioFormat> = None;
                let mut endian_locked: Option<PcmEndian> = None;
                let mut synced_player: Option<SyncedPlayer> = None;
                let mut static_delay_ms: u16 = 0;
                let mut audio_output_broken = false;

                loop {
                    tokio::select! {
                        message = messages.recv() => {
                            let Some(message) = message else {
                                break;
                            };

                            match &message {
                                Message::ServerTime(server_time) => {
                                    let t4 = {
                                        let sync = clock_sync.lock();
                                        sync.clock().now_micros()
                                    };
                                    clock_sync.lock().update(
                                        server_time.client_transmitted,
                                        server_time.server_received,
                                        server_time.server_transmitted,
                                        t4,
                                    );
                                }
                                Message::StreamStart(stream_start) => {
                                    if let Some(player_config) = &stream_start.player {
                                        decoder = None;
                                        audio_format = None;
                                        endian_locked = None;
                                        audio_output_broken = false;

                                        let codec = match player_config.codec.as_str() {
                                            "pcm" => Codec::Pcm,
                                            "opus" => Codec::Opus,
                                            "flac" => Codec::Flac,
                                            _ => {
                                                write_last_error(format!(
                                                    "unsupported codec: {}",
                                                    player_config.codec
                                                ));
                                                continue;
                                            }
                                        };

                                        if player_config.bit_depth != 16 && player_config.bit_depth != 24 {
                                            write_last_error(format!(
                                                "unsupported bit depth: {}",
                                                player_config.bit_depth
                                            ));
                                            continue;
                                        }

                                        let codec_header = match player_config.codec_header.as_deref() {
                                            Some(encoded) => match BASE64_STANDARD.decode(encoded) {
                                                Ok(decoded) => Some(decoded),
                                                Err(err) => {
                                                    write_last_error(format!("invalid codec header: {err}"));
                                                    continue;
                                                }
                                            },
                                            None => None,
                                        };

                                        decoder = match codec {
                                            Codec::Pcm => None,
                                            Codec::Opus => match OpusDecoder::new(
                                                player_config.sample_rate,
                                                player_config.channels,
                                            ) {
                                                Ok(value) => Some(ActiveDecoder::Opus(value)),
                                                Err(err) => {
                                                    write_last_error(format!("opus decoder init failed: {err}"));
                                                    continue;
                                                }
                                            },
                                            Codec::Flac => {
                                                let value = match codec_header.as_deref() {
                                                    Some(header) => match FlacDecoder::with_header(header) {
                                                        Ok(value) => value,
                                                        Err(err) => {
                                                            write_last_error(format!("flac decoder init failed: {err}"));
                                                            continue;
                                                        }
                                                    },
                                                    None => FlacDecoder::new(),
                                                };
                                                Some(ActiveDecoder::Flac(value))
                                            }
                                            _ => None,
                                        };

                                        audio_format = Some(AudioFormat {
                                            codec,
                                            sample_rate: player_config.sample_rate,
                                            channels: player_config.channels,
                                            bit_depth: player_config.bit_depth,
                                            codec_header,
                                        });

                                        if let Some(player) = synced_player.as_ref() {
                                            player.clear();
                                            player.set_static_delay(static_delay_ms);
                                        }
                                    }
                                }
                                Message::StreamEnd(_) | Message::StreamClear(_) => {
                                    decoder = None;
                                    audio_format = None;
                                    endian_locked = None;
                                    audio_output_broken = false;
                                    if let Some(player) = synced_player.as_ref() {
                                        player.clear();
                                    }
                                }
                                Message::ServerCommand(command) => {
                                    if let Some(player_command) = &command.player {
                                        match player_command.command {
                                            PlayerCommandType::Volume => {
                                                if let Some(volume) = player_command.volume {
                                                    if let Some(player) = synced_player.as_ref() {
                                                        player.set_volume(volume);
                                                    }
                                                }
                                            }
                                            PlayerCommandType::Mute => {
                                                if let Some(mute) = player_command.mute {
                                                    if let Some(player) = synced_player.as_ref() {
                                                        player.set_mute(mute);
                                                    }
                                                }
                                            }
                                            PlayerCommandType::SetStaticDelay => {
                                                if let Some(delay) = player_command.static_delay_ms {
                                                    static_delay_ms = delay;
                                                    if let Some(player) = synced_player.as_ref() {
                                                        player.set_static_delay(delay);
                                                    }
                                                }
                                            }
                                            PlayerCommandType::Unknown => {}
                                        }
                                    }
                                }
                                _ => {}
                            }

                            if apply_message_to_snapshot(&snapshot_for_task, &message) {
                                emit_snapshot_event_from_shared(&snapshot_for_task);
                            }
                        }
                        chunk = audio.recv() => {
                            let Some(chunk) = chunk else {
                                break;
                            };

                            let Some(format) = audio_format.clone() else {
                                continue;
                            };

                            if format.codec == Codec::Pcm {
                                let bytes_per_sample = match format.bit_depth {
                                    16 => 2,
                                    24 => 3,
                                    _ => {
                                        write_last_error(format!(
                                            "unsupported pcm bit depth: {}",
                                            format.bit_depth
                                        ));
                                        continue;
                                    }
                                } as usize;
                                let frame_size = bytes_per_sample * format.channels as usize;
                                if chunk.data.len() % frame_size != 0 {
                                    write_last_error("invalid pcm frame size".to_string());
                                    continue;
                                }

                                if endian_locked.is_none() {
                                    let endian = PcmEndian::Little;
                                    decoder = Some(ActiveDecoder::Pcm(PcmDecoder::with_endian(format.bit_depth, endian)));
                                    endian_locked = Some(endian);
                                }
                            }

                            if audio_output_broken {
                                continue;
                            }

                            let Some(active_decoder) = decoder.as_ref() else {
                                continue;
                            };

                            match active_decoder.decode(&chunk.data) {
                                Ok(samples) => {
                                    if synced_player.is_none() {
                                        let (volume, muted) = match snapshot_for_task.lock() {
                                            Ok(snapshot) => (snapshot.volume, snapshot.muted),
                                            Err(_) => (100, false),
                                        };
                                        let config = SyncedPlayerConfig {
                                            device: None,
                                            volume,
                                            muted,
                                            buffer_size: None,
                                        };
                                        let player_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            SyncedPlayer::new(format.clone(), Arc::clone(&clock_sync), config)
                                        }));

                                        match player_result {
                                            Ok(Ok(player)) => {
                                                player.set_static_delay(static_delay_ms);
                                                synced_player = Some(player);
                                            }
                                            Ok(Err(err)) => {
                                                write_last_error(format!("audio output init failed: {err}"));
                                                audio_output_broken = true;
                                                continue;
                                            }
                                            Err(payload) => {
                                                let panic_text = panic_payload_to_string(payload);
                                                write_last_error(format!(
                                                    "audio output init panicked: {}",
                                                    panic_text
                                                ));
                                                audio_output_broken = true;
                                                continue;
                                            }
                                        }
                                    }

                                    if let Some(player) = synced_player.as_ref() {
                                        let buffer = AudioBuffer {
                                            timestamp: chunk.timestamp,
                                            samples,
                                            format: format.clone(),
                                        };
                                        let enqueue_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            player.enqueue(buffer);
                                        }));
                                        if enqueue_result.is_err() {
                                            write_last_error("audio enqueue panicked".to_string());
                                            continue;
                                        }
                                        if let Some(stream_err) = player.take_error() {
                                            log_error(&format!("audio stream runtime error: {stream_err}"));
                                            write_last_error(format!("audio stream error: {stream_err}"));
                                        }
                                    }

                                }
                                Err(err) => {
                                    log_error(&format!("decode failed: {err}"));
                                    write_last_error(format!("decode failed: {err}"));
                                }
                            }
                        }
                        else => {
                            break;
                        }
                    }
                }

                if let Ok(mut state) = snapshot_for_task.lock() {
                    state.connection_state = "offline".to_string();
                }
                emit_snapshot_event_from_shared(&snapshot_for_task);
            });

            Ok::<_, String>((controller, sender, connection.guard, io_task, server_name))
        })?;

        self.controller = Some(controller);
        self.sender = Some(sender);
        self.guard = Some(guard);
        self.io_task = Some(io_task);

        self.set_snapshot(|state| {
            state.connection_state = "online".to_string();
            state.playback_state = "idle".to_string();
            state.server_name = Some(server_name);
        });

        Ok(())
    }

    fn reconnect(&mut self) -> Result<(), String> {
        self.set_snapshot(|snapshot| {
            snapshot.connection_state = "reconnecting".to_string();
        });

        self.disconnect("reconnect")?;
        self.connect()
    }

    fn disconnect(&mut self, _reason: &str) -> Result<(), String> {
        let previous_guard = self.guard.take();
        let previous_task = self.io_task.take();

        self.controller = None;
        self.sender = None;

        self.runtime.block_on(async {
            if let Some(task) = previous_task {
                task.abort();
            }

            if let Some(guard) = previous_guard {
                guard
                    .disconnect(GoodbyeReason::Shutdown)
                    .await
                    .map_err(|err| format!("disconnect failed: {err}"))?;
            }

            Ok::<(), String>(())
        })?;

        self.set_snapshot(|snapshot| {
            snapshot.connection_state = "offline".to_string();
            snapshot.playback_state = "idle".to_string();
            snapshot.metadata_timestamp_micros = None;
            snapshot.metadata_title = None;
            snapshot.metadata_artist = None;
            snapshot.metadata_album_artist = None;
            snapshot.metadata_album = None;
            snapshot.metadata_artwork_url = None;
            snapshot.metadata_year = None;
            snapshot.metadata_track_number = None;
            snapshot.metadata_track_progress_millis = None;
            snapshot.metadata_track_duration_millis = None;
            snapshot.metadata_playback_speed_millis = None;
            snapshot.repeat_mode = None;
            snapshot.shuffle_enabled = None;
            snapshot.seek_max_millis = None;
            snapshot.active_codec = None;
            snapshot.active_sample_rate_hz = None;
            snapshot.active_channels = None;
            snapshot.active_bit_depth = None;
            snapshot.group_id = None;
            snapshot.group_name = None;
            snapshot.server_name = None;
        });

        Ok(())
    }

    fn terminate(&mut self) -> Result<(), String> {
        self.disconnect("terminate")
    }

    fn transport_play(&mut self) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime
            .block_on(async { controller.play().await.map_err(|err| err.to_string()) })?;

        Ok(())
    }

    fn transport_pause(&mut self) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime
            .block_on(async { controller.pause().await.map_err(|err| err.to_string()) })?;

        Ok(())
    }

    fn transport_next(&mut self) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime
            .block_on(async { controller.next().await.map_err(|err| err.to_string()) })?;

        Ok(())
    }

    fn transport_stop(&mut self) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime
            .block_on(async { controller.stop().await.map_err(|err| err.to_string()) })?;

        Ok(())
    }

    fn transport_previous(&mut self) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime
            .block_on(async { controller.previous().await.map_err(|err| err.to_string()) })?;

        Ok(())
    }

    fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        let position_ms = if seconds.is_finite() {
            seconds.max(0.0) * 1000.0
        } else {
            0.0
        };
        let position_ms = position_ms.round() as u64;

        self.runtime.block_on(async {
            controller
                .seek(position_ms)
                .await
                .map_err(|err| err.to_string())
        })?;

        Ok(())
    }

    fn transport_seek_relative(&mut self, offset_ms: i64) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime.block_on(async {
            controller
                .seek_relative(offset_ms)
                .await
                .map_err(|err| err.to_string())
        })?;

        Ok(())
    }

    fn transport_repeat(&mut self, mode: i32) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        let repeat_mode = match mode {
            0 => RepeatMode::Off,
            1 => RepeatMode::One,
            2 => RepeatMode::All,
            _ => {
                return Err("invalid repeat mode (expected 0=off, 1=one, 2=all)".to_string());
            }
        };

        self.runtime.block_on(async {
            controller
                .repeat(repeat_mode)
                .await
                .map_err(|err| err.to_string())
        })?;

        Ok(())
    }

    fn transport_shuffle(&mut self, enabled: bool) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime.block_on(async {
            controller
                .shuffle(enabled)
                .await
                .map_err(|err| err.to_string())
        })?;

        Ok(())
    }

    fn transport_switch_group(&mut self) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime
            .block_on(async { controller.switch().await.map_err(|err| err.to_string()) })?;

        Ok(())
    }

    fn set_volume(&mut self, volume: i32) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;
        let clamped = volume.clamp(0, 100) as u8;

        self.runtime.block_on(async {
            controller
                .set_volume(clamped)
                .await
                .map_err(|err| err.to_string())
        })?;

        self.set_snapshot(|snapshot| {
            snapshot.volume = clamped;
        });

        Ok(())
    }

    fn set_muted(&mut self, muted: bool) -> Result<(), String> {
        let controller = self
            .controller
            .clone()
            .ok_or_else(|| "controller is not connected".to_string())?;

        self.runtime
            .block_on(async { controller.set_mute(muted).await.map_err(|err| err.to_string()) })?;

        self.set_snapshot(|snapshot| {
            snapshot.muted = muted;
        });

        Ok(())
    }

    fn sender_send_sync_state(&mut self, state: &str) -> Result<(), String> {
        let sender = self
            .sender
            .clone()
            .ok_or_else(|| "sender is not connected".to_string())?;

        let normalized = state.trim().to_lowercase();
        let sync_state = match normalized.as_str() {
            "synchronized" => ClientSyncState::Synchronized,
            "external_source" => ClientSyncState::ExternalSource,
            _ => {
                return Err(
                    "invalid sync state (expected 'synchronized' or 'external_source')"
                        .to_string(),
                );
            }
        };

        self.runtime.block_on(async {
            sender
                .send_sync_state(sync_state)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn sender_enter_external_source(&mut self) -> Result<(), String> {
        let sender = self
            .sender
            .clone()
            .ok_or_else(|| "sender is not connected".to_string())?;

        self.runtime.block_on(async {
            sender
                .enter_external_source()
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn sender_exit_external_source(&mut self, player_state_json: Option<&str>) -> Result<(), String> {
        let sender = self
            .sender
            .clone()
            .ok_or_else(|| "sender is not connected".to_string())?;

        let player_state = if let Some(json) = player_state_json {
            let trimmed = json.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str::<PlayerState>(trimmed)
                        .map_err(|err| format!("invalid player state json: {err}"))?,
                )
            }
        } else {
            None
        };

        self.runtime.block_on(async {
            sender
                .exit_external_source(player_state)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn sender_request_stream_formats(&mut self, request_json: &str) -> Result<(), String> {
        let sender = self
            .sender
            .clone()
            .ok_or_else(|| "sender is not connected".to_string())?;

        let request: StreamFormatsRequest = serde_json::from_str(request_json)
            .map_err(|err| format!("invalid stream format request json: {err}"))?;

        self.runtime.block_on(async {
            sender
                .request_stream_formats(request.player, request.artwork, request.visualizer)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn sender_send_message_json(&mut self, message_json: &str) -> Result<(), String> {
        let sender = self
            .sender
            .clone()
            .ok_or_else(|| "sender is not connected".to_string())?;

        let message: Message = serde_json::from_str(message_json)
            .map_err(|err| format!("invalid message json: {err}"))?;

        self.runtime.block_on(async {
            sender
                .send_message(message)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn snapshot_json(&self) -> Result<String, String> {
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|_| "snapshot lock poisoned".to_string())?
            .clone();

        serde_json::to_string(&snapshot).map_err(|err| format!("serialize snapshot failed: {err}"))
    }

    fn set_snapshot<F>(&self, mutator: F)
    where
        F: FnOnce(&mut Snapshot),
    {
        let updated = if let Ok(mut snapshot) = self.snapshot.lock() {
            let before = snapshot.clone();
            mutator(&mut snapshot);
            if *snapshot != before {
                Some(snapshot.clone())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(snapshot) = updated {
            emit_snapshot_event(&snapshot);
        }
    }
}

fn apply_message_to_snapshot(snapshot: &Arc<Mutex<Snapshot>>, message: &Message) -> bool {
    let Ok(mut state) = snapshot.lock() else {
        return false;
    };

    let mut changed = false;

    match message {
        Message::ServerState(server_state) => {
            if let Some(controller) = &server_state.controller {
                if state.volume != controller.volume {
                    state.volume = controller.volume;
                    changed = true;
                }
                if state.muted != controller.muted {
                    state.muted = controller.muted;
                    changed = true;
                }

                let next_repeat = controller.repeat.as_ref().map(repeat_mode_to_string);
                if state.repeat_mode != next_repeat {
                    state.repeat_mode = next_repeat;
                    changed = true;
                }
                if state.shuffle_enabled != controller.shuffle {
                    state.shuffle_enabled = controller.shuffle;
                    changed = true;
                }
                if state.seek_max_millis != controller.seek_max_ms {
                    state.seek_max_millis = controller.seek_max_ms;
                    changed = true;
                }
            }

            if let Some(metadata) = &server_state.metadata {
                let next_timestamp = Some(metadata.timestamp);
                if state.metadata_timestamp_micros != next_timestamp {
                    state.metadata_timestamp_micros = next_timestamp;
                    changed = true;
                }

                if state.metadata_title != metadata.title {
                    state.metadata_title = metadata.title.clone();
                    changed = true;
                }
                if state.metadata_artist != metadata.artist {
                    state.metadata_artist = metadata.artist.clone();
                    changed = true;
                }
                if state.metadata_album_artist != metadata.album_artist {
                    state.metadata_album_artist = metadata.album_artist.clone();
                    changed = true;
                }
                if state.metadata_album != metadata.album {
                    state.metadata_album = metadata.album.clone();
                    changed = true;
                }
                if state.metadata_artwork_url != metadata.artwork_url {
                    state.metadata_artwork_url = metadata.artwork_url.clone();
                    changed = true;
                }
                if state.metadata_year != metadata.year {
                    state.metadata_year = metadata.year;
                    changed = true;
                }
                if state.metadata_track_number != metadata.track {
                    state.metadata_track_number = metadata.track;
                    changed = true;
                }

                if let Some(progress) = &metadata.progress {
                    let next_progress = Some(progress.track_progress.max(0));
                    let next_duration = Some(progress.track_duration.max(0));
                    let next_speed = Some(progress.playback_speed);

                    if state.metadata_track_progress_millis != next_progress {
                        state.metadata_track_progress_millis = next_progress;
                        changed = true;
                    }
                    if state.metadata_track_duration_millis != next_duration {
                        state.metadata_track_duration_millis = next_duration;
                        changed = true;
                    }
                    if state.metadata_playback_speed_millis != next_speed {
                        state.metadata_playback_speed_millis = next_speed;
                        changed = true;
                    }
                } else {
                    if state.metadata_track_progress_millis.is_some() {
                        state.metadata_track_progress_millis = None;
                        changed = true;
                    }
                    if state.metadata_track_duration_millis.is_some() {
                        state.metadata_track_duration_millis = None;
                        changed = true;
                    }
                    if state.metadata_playback_speed_millis.is_some() {
                        state.metadata_playback_speed_millis = None;
                        changed = true;
                    }
                }
            }
        }
        Message::ServerCommand(command) => {
            if let Some(player_command) = &command.player {
                match player_command.command {
                    PlayerCommandType::Volume => {
                        if let Some(volume) = player_command.volume {
                            if state.volume != volume {
                                state.volume = volume;
                                changed = true;
                            }
                        }
                    }
                    PlayerCommandType::Mute => {
                        if let Some(mute) = player_command.mute {
                            if state.muted != mute {
                                state.muted = mute;
                                changed = true;
                            }
                        }
                    }
                    PlayerCommandType::SetStaticDelay | PlayerCommandType::Unknown => {}
                }
            }
        }
        Message::GroupUpdate(group_update) => {
            if state.group_id != group_update.group_id {
                state.group_id = group_update.group_id.clone();
                changed = true;
            }
            if state.group_name != group_update.group_name {
                state.group_name = group_update.group_name.clone();
                changed = true;
            }

            let next_playback_state = match group_update.playback_state {
                Some(PlaybackState::Playing) => "playing".to_string(),
                Some(PlaybackState::Stopped) => "paused".to_string(),
                None => state.playback_state.clone(),
            };

            if state.playback_state != next_playback_state {
                state.playback_state = next_playback_state;
                changed = true;
            }
        }
        Message::StreamStart(stream_start) => {
            if let Some(player) = &stream_start.player {
                let next_codec = Some(player.codec.clone());
                let next_rate = Some(player.sample_rate);
                let next_channels = Some(player.channels);
                let next_bit_depth = Some(player.bit_depth);

                if state.active_codec != next_codec {
                    state.active_codec = next_codec;
                    changed = true;
                }
                if state.active_sample_rate_hz != next_rate {
                    state.active_sample_rate_hz = next_rate;
                    changed = true;
                }
                if state.active_channels != next_channels {
                    state.active_channels = next_channels;
                    changed = true;
                }
                if state.active_bit_depth != next_bit_depth {
                    state.active_bit_depth = next_bit_depth;
                    changed = true;
                }
                if state.playback_state != "buffering" {
                    state.playback_state = "buffering".to_string();
                    changed = true;
                }
            }
        }
        Message::StreamEnd(_) | Message::StreamClear(_) => {
            if state.active_sample_rate_hz.is_some() {
                state.active_sample_rate_hz = None;
                changed = true;
            }
            if state.active_channels.is_some() {
                state.active_channels = None;
                changed = true;
            }
            if state.active_bit_depth.is_some() {
                state.active_bit_depth = None;
                changed = true;
            }
            if state.playback_state != "idle" {
                state.playback_state = "idle".to_string();
                changed = true;
            }
        }
        _ => {}
    }

    changed
}

fn repeat_mode_to_string(mode: &RepeatMode) -> String {
    match mode {
        RepeatMode::Off => "off".to_string(),
        RepeatMode::One => "one".to_string(),
        RepeatMode::All => "all".to_string(),
    }
}

fn write_last_error(message: String) {
    log_error(&message);
    if let Ok(mut error) = LAST_ERROR.lock() {
        *error = message;
    }
}

fn emit_event_json(value: &serde_json::Value) {
    let callback = match EVENT_CALLBACK.lock() {
        Ok(lock) => *lock,
        Err(_) => None,
    };

    let Some(callback) = callback else {
        return;
    };

    let Ok(text) = serde_json::to_string(value) else {
        return;
    };

    let Ok(c_string) = CString::new(text) else {
        return;
    };

    callback(c_string.into_raw());
}

fn emit_snapshot_event(snapshot: &Snapshot) {
    let Ok(snapshot_json) = serde_json::to_string(snapshot) else {
        return;
    };

    let should_emit = {
        let mut last = match LAST_SNAPSHOT_EVENT_JSON.lock() {
            Ok(lock) => lock,
            Err(_) => return,
        };

        if *last == snapshot_json {
            false
        } else {
            *last = snapshot_json;
            true
        }
    };

    if !should_emit {
        return;
    }

    let Ok(value) = serde_json::to_value(snapshot) else {
        return;
    };
    emit_event_json(&value);
}

fn emit_snapshot_event_from_shared(snapshot: &Arc<Mutex<Snapshot>>) {
    let cloned = match snapshot.lock() {
        Ok(state) => state.clone(),
        Err(_) => return,
    };
    emit_snapshot_event(&cloned);
}

fn c_string_from_result(result: Result<String, String>) -> *mut c_char {
    match result {
        Ok(text) => match CString::new(text) {
            Ok(c_string) => c_string.into_raw(),
            Err(err) => {
                write_last_error(format!("invalid CString: {err}"));
                ptr::null_mut()
            }
        },
        Err(err) => {
            write_last_error(err);
            ptr::null_mut()
        }
    }
}

fn as_i32_result(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(err) => {
            write_last_error(err);
            -1
        }
    }
}

fn read_utf8(pointer: *const c_char, field_name: &str) -> Result<String, String> {
    if pointer.is_null() {
        return Err(format!("{field_name} pointer is null"));
    }

    let text = unsafe {
        CStr::from_ptr(pointer)
            .to_str()
            .map_err(|err| format!("{field_name} is not valid utf8: {err}"))?
            .to_string()
    };

    Ok(text)
}

fn read_optional_utf8(pointer: *const c_char, field_name: &str) -> Result<Option<String>, String> {
    if pointer.is_null() {
        return Ok(None);
    }

    read_utf8(pointer, field_name).map(Some)
}

#[cfg(target_os = "android")]
#[no_mangle]
// Android-only JNI entrypoint: receives app context so native audio can bootstrap safely.
pub extern "system" fn Java_com_example_mashin_1flutter_MainActivity_nativeSetAndroidContext(
    env: JNIEnv,
    _thiz: JObject,
    context: JObject,
) {
    let vm = match env.get_java_vm() {
        Ok(vm) => vm,
        Err(err) => {
            log_error(&format!("android context init failed: get_java_vm error: {err}"));
            return;
        }
    };

    let global_context = match env.new_global_ref(context) {
        Ok(value) => value,
        Err(err) => {
            log_error(&format!("android context init failed: new_global_ref error: {err}"));
            return;
        }
    };

    let context_ptr = global_context.as_obj().as_raw() as *mut c_void;
    let vm_ptr = vm.get_java_vm_pointer() as *mut c_void;
    // ndk_context needs VM + app context pointers once before any AAudio usage.
    unsafe {
        ndk_context::initialize_android_context(vm_ptr, context_ptr);
    }

    if let Ok(mut slot) = ANDROID_APP_CONTEXT.lock() {
        *slot = Some(global_context);
    }
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_register_event_callback(callback: Option<EventCallback>) -> i32 {
    let mut lock = match EVENT_CALLBACK.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("event callback lock poisoned".to_string())),
    };

    *lock = callback;
    0
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_engine_init(config_json: *const c_char) -> i32 {
    let config_json = match read_utf8(config_json, "config_json") {
        Ok(value) => value,
        Err(err) => return as_i32_result(Err(err)),
    };

    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.init(&config_json))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_engine_connect() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.connect())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_engine_reconnect() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.reconnect())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_engine_disconnect(reason: *const c_char) -> i32 {
    let reason = match read_utf8(reason, "reason") {
        Ok(value) => value,
        Err(err) => return as_i32_result(Err(err)),
    };

    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.disconnect(&reason))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_engine_terminate() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.terminate())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_play() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_play())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_pause() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_pause())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_next() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_next())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_stop() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_stop())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_previous() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_previous())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_seek(seconds: f64) -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_seek(seconds))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_seek_relative_ms(offset_ms: i64) -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_seek_relative(offset_ms))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_repeat_mode(mode: i32) -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_repeat(mode))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_shuffle(enabled: i32) -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_shuffle(enabled != 0))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_transport_switch_group() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.transport_switch_group())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_set_volume(volume: i32) -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.set_volume(volume))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_set_muted(muted: i32) -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.set_muted(muted != 0))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_sender_send_sync_state(state: *const c_char) -> i32 {
    let state = match read_utf8(state, "state") {
        Ok(value) => value,
        Err(err) => return as_i32_result(Err(err)),
    };

    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.sender_send_sync_state(&state))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_sender_enter_external_source() -> i32 {
    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.sender_enter_external_source())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_sender_exit_external_source(player_state_json: *const c_char) -> i32 {
    let player_state_json = match read_optional_utf8(player_state_json, "player_state_json") {
        Ok(value) => value,
        Err(err) => return as_i32_result(Err(err)),
    };

    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.sender_exit_external_source(player_state_json.as_deref()))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_sender_request_stream_formats(request_json: *const c_char) -> i32 {
    let request_json = match read_utf8(request_json, "request_json") {
        Ok(value) => value,
        Err(err) => return as_i32_result(Err(err)),
    };

    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.sender_request_stream_formats(&request_json))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_sender_send_message_json(message_json: *const c_char) -> i32 {
    let message_json = match read_utf8(message_json, "message_json") {
        Ok(value) => value,
        Err(err) => return as_i32_result(Err(err)),
    };

    let mut engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => return as_i32_result(Err("engine lock poisoned".to_string())),
    };

    as_i32_result(engine.sender_send_message_json(&message_json))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_get_snapshot_json() -> *mut c_char {
    let engine = match ENGINE.lock() {
        Ok(lock) => lock,
        Err(_) => {
            write_last_error("engine lock poisoned".to_string());
            return ptr::null_mut();
        }
    };

    c_string_from_result(engine.snapshot_json())
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_get_last_error() -> *mut c_char {
    let message = match LAST_ERROR.lock() {
        Ok(error) => error.clone(),
        Err(_) => "unable to read last error".to_string(),
    };

    c_string_from_result(Ok(message))
}

#[no_mangle]
pub extern "C" fn mashin_sendspin_free_string(pointer: *mut c_char) {
    if pointer.is_null() {
        return;
    }

    unsafe {
        let _ = CString::from_raw(pointer);
    }
}
