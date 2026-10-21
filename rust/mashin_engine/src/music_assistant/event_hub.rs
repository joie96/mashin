//! One authenticated Music Assistant WebSocket owned by native session consumers.

use super::models::identifiers::{AccessToken, MediaId, PlayerId, QueueId};
use super::models::player::MusicAssistantPlayer;
use super::models::queue::MusicAssistantQueue;
use super::models::session::MusicAssistantServerUrl;
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
    UserData,
    AndroidAutoLiveState,
    ExplicitUserRefresh,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MusicAssistantEvent {
    Connected,
    Disconnected,
    FavoritesChanged,
    PlaylistsChanged,
    PlaylistChanged(MediaId),
    QueueUpdated(MusicAssistantQueue),
    QueueItemsUpdated(QueueId),
    QueueRemoved(QueueId),
    QueueTimeUpdated {
        queue_id: QueueId,
        elapsed_seconds: f64,
    },
    PlayerUpdated(MusicAssistantPlayer),
    PlayerRemoved(PlayerId),
}

/// Owns one authenticated WebSocket while at least one live-state feature has a lease.
pub(crate) struct MusicAssistantEventHub {
    websocket_url: String,
    access_token: String,
    leases: HashMap<EventHubLeaseReason, usize>,
    event_sender: broadcast::Sender<MusicAssistantEvent>,
    worker: Option<JoinHandle<()>>,
    is_connected: Arc<AtomicBool>,
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
            is_connected: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<MusicAssistantEvent> {
        self.event_sender.subscribe()
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    // Feature lease lifecycle.
    pub(crate) fn acquire(&mut self, reason: EventHubLeaseReason) {
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

    pub(crate) fn shutdown(&mut self) {
        self.leases.clear();
        self.stop_worker();
    }

    fn start_worker(&mut self) {
        let websocket_url = self.websocket_url.clone();
        let access_token = self.access_token.clone();
        let event_sender = self.event_sender.clone();
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
    event_sender: broadcast::Sender<MusicAssistantEvent>,
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
        let _ = event_sender.send(MusicAssistantEvent::Disconnected);
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
    event_sender: &broadcast::Sender<MusicAssistantEvent>,
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
    let _ = event_sender.send(MusicAssistantEvent::Connected);

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

fn parse_event(payload: Value) -> Option<MusicAssistantEvent> {
    let event_name = payload.get("event")?.as_str()?.trim().to_ascii_lowercase();
    let object_id = payload.get("object_id")?.as_str()?.trim();
    if object_id.is_empty() {
        return None;
    }
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    match event_name.as_str() {
        "favorite_updated" => Some(MusicAssistantEvent::FavoritesChanged),
        "media_item_updated" if object_id.starts_with("library://playlist/") => {
            Some(MusicAssistantEvent::PlaylistChanged(MediaId(
                object_id
                    .trim_start_matches("library://playlist/")
                    .to_owned(),
            )))
        }
        "media_item_added" | "media_item_updated" | "media_item_deleted"
            if object_id.starts_with("library://playlist/")
                || data.get("media_type").and_then(Value::as_str) == Some("playlist") =>
        {
            Some(MusicAssistantEvent::PlaylistsChanged)
        }
        "queue_added" | "queue_updated" | "queue_settings_updated" => serde_json::from_value(data)
            .ok()
            .map(MusicAssistantEvent::QueueUpdated),
        "queue_items_updated" => Some(MusicAssistantEvent::QueueItemsUpdated(QueueId(
            object_id.into(),
        ))),
        "queue_removed" => Some(MusicAssistantEvent::QueueRemoved(QueueId(object_id.into()))),
        "queue_time_updated" => Some(MusicAssistantEvent::QueueTimeUpdated {
            queue_id: QueueId(object_id.into()),
            elapsed_seconds: data.as_f64()?,
        }),
        "player_added" | "player_updated" => serde_json::from_value(data)
            .ok()
            .map(MusicAssistantEvent::PlayerUpdated),
        "player_removed" => Some(MusicAssistantEvent::PlayerRemoved(PlayerId(
            object_id.into(),
        ))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_authenticated_response, parse_event, websocket_url_for_music_assistant,
        MusicAssistantEvent,
    };
    use serde_json::json;

    #[test]
    fn user_data_events_are_forwarded_without_playback_consumers() {
        assert_eq!(
            parse_event(
                json!({"event":"favorite_updated", "object_id":"library://track/1", "data":{"favorite":true}})
            ),
            Some(MusicAssistantEvent::FavoritesChanged)
        );
        assert_eq!(
            parse_event(
                json!({"event":"media_item_updated", "object_id":"library://playlist/42", "data":{"media_type":"playlist"}})
            ),
            Some(MusicAssistantEvent::PlaylistChanged(super::MediaId(
                "42".into()
            )))
        );
    }

    #[test]
    fn builds_websocket_urls_without_query_or_fragment() {
        assert_eq!(
            websocket_url_for_music_assistant("https://music.example/base/?token=ignored#ignored")
                .expect("build websocket URL"),
            "wss://music.example/base/ws"
        );
    }

    #[test]
    fn filters_and_normalizes_queue_events() {
        let event = parse_event(json!({
            "event": "QUEUE_TIME_UPDATED",
            "object_id": " queue-1 ",
            "data": 42.5,
        }))
        .expect("queue event");

        assert!(
            matches!(event, MusicAssistantEvent::QueueTimeUpdated { queue_id, elapsed_seconds }
            if queue_id.0 == "queue-1" && elapsed_seconds == 42.5)
        );
        assert!(parse_event(json!({ "event": "player_added" })).is_none());
        assert!(
            matches!(parse_event(json!({ "event": "player_removed", "object_id": "remote" })),
            Some(MusicAssistantEvent::PlayerRemoved(player_id)) if player_id.0 == "remote")
        );
    }

    #[tokio::test]
    async fn authenticates_after_server_info_and_forwards_queue_events() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"server_id": "test", "schema_version": 1})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let request = socket.next().await.unwrap().unwrap();
            assert!(matches!(request, Message::Text(_)));
            socket
                .send(Message::Text(
                    json!({"message_id":"mashin-event-hub-auth", "result":{"authenticated":true}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    json!({"event":"queue_items_updated", "object_id":"player"})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            socket.close(None).await.unwrap();
        });
        let (sender, mut receiver) = tokio::sync::broadcast::channel(8);
        let connected = std::sync::atomic::AtomicBool::new(false);
        let _ = super::connect_and_forward_events(
            &format!("ws://{address}"),
            "test-token",
            &sender,
            &connected,
        )
        .await;
        assert_eq!(
            receiver.recv().await.unwrap(),
            MusicAssistantEvent::Connected
        );
        assert!(
            matches!(receiver.recv().await.unwrap(), MusicAssistantEvent::QueueItemsUpdated(queue_id) if queue_id.0 == "player")
        );
        server.await.unwrap();
    }

    #[test]
    fn accepts_only_matching_successful_authentication_responses() {
        assert!(is_authenticated_response(&json!({
            "message_id": "mashin-event-hub-auth",
            "result": { "authenticated": true },
        })));
        assert!(!is_authenticated_response(&json!({
            "message_id": "mashin-event-hub-auth",
            "result": { "authenticated": false },
        })));
    }

    #[tokio::test]
    async fn detects_unresponsive_authenticated_connection() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, hold_connection) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            socket
                .send(Message::Text(
                    json!({
                        "message_id":"mashin-event-hub-auth", "result":{"authenticated":true}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            let _ = hold_connection.await;
            drop(socket);
        });
        let (sender, _) = tokio::sync::broadcast::channel(8);
        let connected = std::sync::atomic::AtomicBool::new(false);
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(25),
            super::connect_and_forward_events(
                &format!("ws://{address}"),
                "test-token",
                &sender,
                &connected,
            ),
        )
        .await;
        let _ = release.send(());
        server.await.unwrap();
        assert_eq!(
            result.expect("unresponsive connection must not hang indefinitely"),
            Err("Music Assistant event hub liveness timed out".to_string()),
        );
    }

    #[tokio::test]
    async fn reconnects_after_stalled_connection_and_resumes_events() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, hold_connection) = tokio::sync::oneshot::channel::<()>();
        let server =
            tokio::spawn(async move {
                let mut retained_sockets = Vec::new();
                for generation in 0..2 {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    assert!(matches!(
                        socket.next().await.unwrap().unwrap(),
                        Message::Text(_)
                    ));
                    socket.send(Message::Text(json!({
                    "message_id":"mashin-event-hub-auth", "result":{"authenticated":true}
                }).to_string().into())).await.unwrap();
                    if generation == 1 {
                        for _ in 0..2 {
                            let Message::Ping(payload) = socket.next().await.unwrap().unwrap()
                            else {
                                panic!("expected event hub heartbeat");
                            };
                            socket.send(Message::Pong(payload)).await.unwrap();
                        }
                        socket
                            .send(Message::Text(
                                json!({
                                    "event":"queue_items_updated", "object_id":"reconnected-player"
                                })
                                .to_string()
                                .into(),
                            ))
                            .await
                            .unwrap();
                    }
                    retained_sockets.push(socket);
                }
                let _ = hold_connection.await;
                drop(retained_sockets);
            });
        let (sender, mut events) = tokio::sync::broadcast::channel(8);
        let connected = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = tokio::spawn(super::run_event_worker(
            format!("ws://{address}"),
            "test-token".into(),
            sender,
            connected.clone(),
        ));
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(40), async {
            assert_eq!(events.recv().await.unwrap(), MusicAssistantEvent::Connected);
            assert_eq!(events.recv().await.unwrap(), MusicAssistantEvent::Disconnected);
            assert!(!connected.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(events.recv().await.unwrap(), MusicAssistantEvent::Connected);
            assert!(matches!(events.recv().await.unwrap(), MusicAssistantEvent::QueueItemsUpdated(queue_id)
                if queue_id.0 == "reconnected-player"));
            assert!(connected.load(std::sync::atomic::Ordering::Acquire));
        }).await;
        worker.abort();
        let _ = worker.await;
        let _ = release.send(());
        server.await.unwrap();
        result.expect("event hub must reconnect and resume live events");
    }
}
