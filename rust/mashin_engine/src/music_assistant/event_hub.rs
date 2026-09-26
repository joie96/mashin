//! Authenticated Music Assistant WebSocket lifecycle for opt-in live features.

use crate::core::models::SessionConfig;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::tungstenite::Message;

const RECONNECT_DELAY: Duration = Duration::from_secs(2);
const AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(10);
const EVENT_CHANNEL_CAPACITY: usize = 64;
const EVENT_HUB_LOGGER: crate::logging::Logger = crate::logging::Logger::new("event_hub");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EventHubLeaseReason {
    RemotePlayback,
    AndroidAutoLiveState,
    ExplicitUserRefresh,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MusicAssistantQueueEvent {
    pub(crate) name: String,
    pub(crate) queue_id: Option<String>,
    pub(crate) data: Value,
}

/// Owns one authenticated WebSocket while at least one live-state feature has a lease.
pub(crate) struct MusicAssistantEventHub {
    websocket_url: String,
    access_token: String,
    leases: HashMap<EventHubLeaseReason, usize>,
    event_sender: broadcast::Sender<MusicAssistantQueueEvent>,
    worker: Option<JoinHandle<()>>,
}

impl MusicAssistantEventHub {
    // Construction and subscriptions.
    pub(crate) fn new(session: &SessionConfig) -> Result<Self, String> {
        let access_token = session
            .access_token
            .as_ref()
            .map(|token| token.as_str().trim())
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                "Music Assistant event hub requires an authenticated session".to_string()
            })?;
        let (event_sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Ok(Self {
            websocket_url: websocket_url_for_music_assistant(&session.music_assistant_url.0)?,
            access_token: access_token.to_string(),
            leases: HashMap::new(),
            event_sender,
            worker: None,
        })
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<MusicAssistantQueueEvent> {
        self.event_sender.subscribe()
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
        self.worker = Some(tokio::spawn(async move {
            run_event_worker(websocket_url, access_token, event_sender).await;
        }));
    }

    fn stop_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
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
    event_sender: broadcast::Sender<MusicAssistantQueueEvent>,
) {
    loop {
        let result = connect_and_forward_events(&websocket_url, &access_token, &event_sender).await;
        if let Err(error) = result {
            EVENT_HUB_LOGGER.warn(format!("Music Assistant event hub disconnected: {error}"));
        }
        sleep(RECONNECT_DELAY).await;
    }
}

async fn connect_and_forward_events(
    websocket_url: &str,
    access_token: &str,
    event_sender: &broadcast::Sender<MusicAssistantQueueEvent>,
) -> Result<(), String> {
    let (mut websocket, _) = tokio_tungstenite::connect_async(websocket_url)
        .await
        .map_err(|error| format!("connect Music Assistant event hub failed: {error}"))?;
    websocket
        .send(Message::Text(
            json!({
                "message_id": "mashin-event-hub-auth",
                "command": "auth",
                "args": { "token": access_token },
            })
            .to_string()
            .into(),
        ))
        .await
        .map_err(|error| format!("authenticate Music Assistant event hub failed: {error}"))?;

    let authentication_response = tokio::time::timeout(AUTHENTICATION_TIMEOUT, websocket.next())
        .await
        .map_err(|_| "Music Assistant event hub authentication timed out".to_string())?
        .ok_or_else(|| "Music Assistant event hub closed during authentication".to_string())?
        .map_err(|error| {
            format!("read Music Assistant event hub authentication failed: {error}")
        })?;
    let Message::Text(authentication_payload) = authentication_response else {
        return Err(
            "Music Assistant event hub returned a non-text authentication response".to_string(),
        );
    };
    let authentication_payload: Value =
        serde_json::from_str(&authentication_payload).map_err(|error| {
            format!("decode Music Assistant event hub authentication failed: {error}")
        })?;
    if !is_authenticated_response(&authentication_payload) {
        return Err("Music Assistant event hub authentication was rejected".to_string());
    }

    while let Some(message) = websocket.next().await {
        let message =
            message.map_err(|error| format!("read Music Assistant event failed: {error}"))?;
        let Message::Text(payload) = message else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        let Some(event) = parse_queue_event(payload) else {
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

fn parse_queue_event(payload: Value) -> Option<MusicAssistantQueueEvent> {
    let event_name = payload.get("event")?.as_str()?.trim().to_ascii_lowercase();
    if !matches!(
        event_name.as_str(),
        "queue_added"
            | "queue_removed"
            | "queue_updated"
            | "queue_items_updated"
            | "queue_time_updated"
            | "queue_settings_updated"
    ) {
        return None;
    }
    Some(MusicAssistantQueueEvent {
        name: event_name,
        queue_id: payload
            .get("object_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        data: payload.get("data").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::{is_authenticated_response, parse_queue_event, websocket_url_for_music_assistant};
    use serde_json::json;

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
        let event = parse_queue_event(json!({
            "event": "QUEUE_TIME_UPDATED",
            "object_id": " queue-1 ",
            "data": 42.5,
        }))
        .expect("queue event");

        assert_eq!(event.name, "queue_time_updated");
        assert_eq!(event.queue_id.as_deref(), Some("queue-1"));
        assert_eq!(event.data, json!(42.5));
        assert!(parse_queue_event(json!({ "event": "player_added" })).is_none());
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
}
