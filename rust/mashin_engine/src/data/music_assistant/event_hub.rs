//! One authenticated Music Assistant WebSocket owned by native session consumers.

use crate::domain::identifiers::{MediaId, PlayerId, QueueId};
use crate::domain::session::{AccessToken, MusicAssistantServerUrl};
use crate::playback::server::ServerEvent;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

const RECONNECT_DELAY: Duration = Duration::from_secs(2);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const PING_INTERVAL: Duration = Duration::from_secs(5);
const PONG_TIMEOUT: Duration = Duration::from_secs(15);
const EVENT_CHANNEL_CAPACITY: usize = 64;
const EVENT_HUB_LOGGER: crate::logging::Logger = crate::logging::Logger::new("event_hub");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EventHubLeaseReason {
    Playback,
    SessionData,
    AndroidAutoLiveState,
    ExplicitUserRefresh,
}

/// Owns one authenticated WebSocket while at least one live-state feature has a lease.
pub(crate) struct MusicAssistantEventHub {
    websocket_url: String,
    access_token: String,
    leases: HashMap<EventHubLeaseReason, usize>,
    event_sender: broadcast::Sender<ServerEvent>,
    worker: Option<JoinHandle<()>>,
    retired_workers: Vec<JoinHandle<()>>,
    is_connected: Arc<AtomicBool>,
    is_shutdown: bool,
}

impl MusicAssistantEventHub {
    // Construction and subscriptions.
    pub(crate) fn new(
        server_url: &MusicAssistantServerUrl,
        access_token: Option<&AccessToken>,
    ) -> Result<Self, String> {
        let access_token = access_token
            .map(|token| token.as_str().trim())
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                "Music Assistant event hub requires an authenticated session".to_string()
            })?;
        let (event_sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Ok(Self {
            websocket_url: websocket_url_for_music_assistant(&server_url.0)?,
            access_token: access_token.to_string(),
            leases: HashMap::new(),
            event_sender,
            worker: None,
            retired_workers: Vec::new(),
            is_connected: Arc::new(AtomicBool::new(false)),
            is_shutdown: false,
        })
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_sender.subscribe()
    }

    pub(crate) fn is_connected(&self) -> bool {
        !self.is_shutdown && self.worker.is_some() && self.is_connected.load(Ordering::Acquire)
    }

    // Feature lease lifecycle.
    pub(crate) fn acquire(&mut self, reason: EventHubLeaseReason) {
        if self.is_shutdown {
            return;
        }
        *self.leases.entry(reason).or_default() += 1;
        if self.worker.is_none() {
            self.start_worker();
        }
    }

    pub(crate) fn release(&mut self, reason: EventHubLeaseReason) {
        let Some(lease_count) = self.leases.get_mut(&reason) else {
            return;
        };
        *lease_count -= 1;
        if *lease_count == 0 {
            self.leases.remove(&reason);
        }
        if self.leases.is_empty() {
            self.stop_worker();
        }
    }

    // The session owner awaits these handles after releasing application and hub locks.
    pub(crate) fn shutdown(&mut self) -> Vec<JoinHandle<()>> {
        self.is_shutdown = true;
        self.leases.clear();
        self.stop_worker();
        self.take_retired_workers()
    }

    pub(crate) fn take_retired_workers(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.retired_workers)
    }

    fn start_worker(&mut self) {
        let websocket_url = self.websocket_url.clone();
        let access_token = self.access_token.clone();
        let event_sender = self.event_sender.clone();
        self.is_connected = Arc::new(AtomicBool::new(false));
        let is_connected = self.is_connected.clone();
        self.worker = Some(tokio::spawn(async move {
            run_event_worker(websocket_url, access_token, event_sender, is_connected).await;
        }));
    }

    fn stop_worker(&mut self) {
        self.is_connected.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            EVENT_HUB_LOGGER.info("Music Assistant event hub worker stopped");
            worker.abort();
            self.retired_workers.push(worker);
        }
    }
}

impl Drop for MusicAssistantEventHub {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

async fn run_event_worker(
    websocket_url: String,
    access_token: String,
    event_sender: broadcast::Sender<ServerEvent>,
    is_connected: Arc<AtomicBool>,
) {
    let mut connection_attempt = 0_u64;
    loop {
        connection_attempt = connection_attempt.saturating_add(1);
        EVENT_HUB_LOGGER.info(format_args!(
            "Music Assistant event hub connection attempt #{connection_attempt}"
        ));
        let result =
            connect_and_forward_events(&websocket_url, &access_token, &event_sender, &is_connected)
                .await;
        is_connected.store(false, Ordering::Release);
        if let Err(error) = result {
            EVENT_HUB_LOGGER.warn(format!("Music Assistant event hub disconnected: {error}"));
        }
        let _ = event_sender.send(ServerEvent::Disconnected);
        EVENT_HUB_LOGGER.info(format_args!(
            "Music Assistant event hub reconnect scheduled in {} ms",
            RECONNECT_DELAY.as_millis(),
        ));
        sleep(RECONNECT_DELAY).await;
    }
}

async fn connect_and_forward_events(
    websocket_url: &str,
    access_token: &str,
    event_sender: &broadcast::Sender<ServerEvent>,
    is_connected: &AtomicBool,
) -> Result<(), String> {
    let (mut websocket, _) = tokio::time::timeout(
        CONNECTION_TIMEOUT,
        tokio_tungstenite::connect_async(websocket_url),
    )
    .await
    .map_err(|_| "Music Assistant event hub connection timed out".to_string())?
    .map_err(|error| format!("connect Music Assistant event hub failed: {error}"))?;
    tokio::time::timeout(
        WRITE_TIMEOUT,
        websocket.send(Message::Text(
            json!({
                "message_id": "mashin-event-hub-auth",
                "command": "auth",
                "args": { "token": access_token },
            })
            .to_string()
            .into(),
        )),
    )
    .await
    .map_err(|_| "Music Assistant event hub authentication write timed out".to_string())?
    .map_err(|error| format!("authenticate Music Assistant event hub failed: {error}"))?;

    tokio::time::timeout(AUTHENTICATION_TIMEOUT, async {
        while let Some(message) = websocket.next().await {
            match message.map_err(|error| format!("read MA authentication failed: {error}"))? {
                Message::Text(payload) => {
                    let payload: Value = serde_json::from_str(&payload)
                        .map_err(|error| format!("decode MA authentication failed: {error}"))?;
                    if payload.get("message_id").and_then(Value::as_str)
                        != Some("mashin-event-hub-auth")
                    {
                        continue;
                    }
                    return if is_authenticated_response(&payload) {
                        Ok(())
                    } else {
                        Err("Music Assistant authentication was rejected".to_string())
                    };
                }
                Message::Ping(payload) => websocket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| format!("MA authentication pong failed: {error}"))?,
                Message::Close(_) => break,
                _ => {}
            }
        }
        Err("Music Assistant closed during authentication".to_string())
    })
    .await
    .map_err(|_| "Music Assistant authentication timed out".to_string())??;
    is_connected.store(true, Ordering::Release);
    EVENT_HUB_LOGGER.info("Music Assistant event hub connected and authenticated");
    let _ = event_sender.send(ServerEvent::Connected);

    let mut ping = tokio::time::interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut outstanding_ping: Option<(Vec<u8>, Instant)> = None;
    loop {
        let message = tokio::select! {
            message = websocket.next() => match message {
                Some(message) => message,
                None => break,
            },
            _ = async {
                match &outstanding_ping {
                    Some((_, sent)) => tokio::time::sleep_until(*sent + PONG_TIMEOUT).await,
                    None => std::future::pending().await,
                }
            } => return Err("Music Assistant event hub liveness timed out".to_string()),
            _ = ping.tick() => {
                if outstanding_ping.is_none() {
                    let payload = uuid::Uuid::new_v4().as_bytes().to_vec();
                    tokio::time::timeout(WRITE_TIMEOUT, websocket.send(Message::Ping(payload.clone().into())))
                        .await
                        .map_err(|_| "Music Assistant event hub ping write timed out".to_string())?
                        .map_err(|error| format!("Music Assistant event hub ping failed: {error}"))?;
                    outstanding_ping = Some((payload, Instant::now()));
                }
                continue;
            }
        };
        let message =
            message.map_err(|error| format!("read Music Assistant event failed: {error}"))?;
        let payload = match message {
            Message::Text(payload) => payload,
            Message::Ping(payload) => {
                tokio::time::timeout(WRITE_TIMEOUT, websocket.send(Message::Pong(payload)))
                    .await
                    .map_err(|_| "Music Assistant event hub pong write timed out".to_string())?
                    .map_err(|error| format!("MA event pong failed: {error}"))?;
                continue;
            }
            Message::Pong(payload) => {
                if outstanding_ping
                    .as_ref()
                    .is_some_and(|(expected, _)| expected.as_slice() == payload.as_ref())
                {
                    outstanding_ping = None;
                }
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(payload) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        let Some(event) = parse_event(payload) else {
            continue;
        };
        let _ = event_sender.send(event);
    }
    Err("Music Assistant event hub connection closed".to_string())
}

fn is_authenticated_response(payload: &Value) -> bool {
    payload.get("message_id").and_then(Value::as_str) == Some("mashin-event-hub-auth")
        && payload
            .get("result")
            .and_then(Value::as_object)
            .and_then(|result| result.get("authenticated"))
            .and_then(Value::as_bool)
            == Some(true)
}

fn websocket_url_for_music_assistant(base_url: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|error| format!("invalid Music Assistant URL for event hub: {error}"))?;
    let websocket_scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        scheme => {
            return Err(format!(
                "unsupported Music Assistant URL scheme for event hub: {scheme}"
            ))
        }
    };
    url.set_scheme(websocket_scheme)
        .map_err(|_| "set Music Assistant event hub websocket scheme failed".to_string())?;
    let path = format!("{}/ws", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.into())
}

fn parse_event(payload: Value) -> Option<ServerEvent> {
    let event_name = payload.get("event")?.as_str()?.trim().to_ascii_lowercase();
    let object_id = payload.get("object_id")?.as_str()?.trim();
    if object_id.is_empty() {
        return None;
    }
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    match event_name.as_str() {
        "favorite_updated" => Some(ServerEvent::FavoritesChanged),
        "media_item_updated" if object_id.starts_with("library://playlist/") => {
            Some(ServerEvent::PlaylistChanged(MediaId(
                object_id
                    .trim_start_matches("library://playlist/")
                    .to_owned(),
            )))
        }
        "media_item_added" | "media_item_updated" | "media_item_deleted"
            if object_id.starts_with("library://playlist/")
                || data.get("media_type").and_then(Value::as_str) == Some("playlist") =>
        {
            Some(ServerEvent::PlaylistsChanged)
        }
        "queue_added" | "queue_updated" | "queue_settings_updated" => serde_json::from_value::<
            crate::data::music_assistant::dto::queue::MusicAssistantQueue,
        >(data)
        .ok()
        .map(Into::into)
        .map(ServerEvent::QueueUpdated),
        "queue_items_updated" => Some(ServerEvent::QueueItemsUpdated(QueueId(object_id.into()))),
        "queue_removed" => Some(ServerEvent::QueueRemoved(QueueId(object_id.into()))),
        "queue_time_updated" => Some(ServerEvent::QueueTimeUpdated {
            queue_id: QueueId(object_id.into()),
            elapsed_seconds: data.as_f64()?,
        }),
        "player_added" | "player_updated" => serde_json::from_value::<
            crate::data::music_assistant::dto::player::MusicAssistantPlayer,
        >(data)
        .ok()
        .map(Into::into)
        .map(ServerEvent::PlayerUpdated),
        "player_removed" => Some(ServerEvent::PlayerRemoved(PlayerId(object_id.into()))),
        _ => None,
    }
}
