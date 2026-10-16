//! Server-authoritative queue projection shared by local and remote playback adapters.
//! The worker owns snapshot ordering; callers can only issue MA commands or read snapshots.

use super::client::MusicAssistantClient;
use super::event_hub::MusicAssistantEvent;
use super::models::identifiers::{PlayerId, QueueId, QueueItemId};
use super::models::player::MusicAssistantPlayer;
use super::models::queue::{MusicAssistantQueue, PlayState};
use super::queue_projection::{QueueProjection, QueueSnapshotRequest};
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{PlaybackConnectionState, PlayerState, Queue};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

struct QueueState {
    projection: QueueProjection,
    player: Option<MusicAssistantPlayer>,
    player_revision: u64,
    is_connected: bool,
}

struct QueueSnapshot {
    queue: Queue,
    player: MusicAssistantPlayer,
}

pub(crate) struct MusicAssistantQueueController {
    state: Arc<Mutex<QueueState>>,
    worker: JoinHandle<()>,
}

impl MusicAssistantQueueController {
    // Construction and native lifetime.
    pub(crate) fn new(
        runtime: &tokio::runtime::Runtime,
        client: Arc<MusicAssistantClient>,
        events: broadcast::Receiver<MusicAssistantEvent>,
        player_id: PlayerId,
        is_connected: bool,
    ) -> Self {
        let state = Arc::new(Mutex::new(QueueState {
            projection: QueueProjection::new(QueueId(player_id.0.clone())),
            player: None,
            player_revision: 0,
            is_connected,
        }));
        let worker = runtime.spawn(run_projection(client, events, player_id, state.clone()));
        Self { state, worker }
    }

    // Read-only projections.
    pub(crate) fn queue(&self) -> Queue {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .projection
            .queue()
            .clone()
    }

    pub(crate) fn remote_player_state(&self) -> PlayerState {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        project_remote_player(&state)
    }

    pub(crate) fn queue_item_index(&self, item_id: &QueueItemId) -> Option<usize> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state
            .projection
            .queue()
            .items
            .iter()
            .position(|item| &item.queue_item_id == item_id)
    }

    pub(crate) fn is_synchronized(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .projection
            .is_synchronized()
    }

    pub(crate) fn revision(&self) -> (QueueSnapshotRequest, bool) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        (
            state.projection.begin_snapshot(),
            state.projection.is_synchronized(),
        )
    }
}

impl Drop for MusicAssistantQueueController {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

// Events continue to invalidate the request while pagination is in flight.
async fn run_projection(
    client: Arc<MusicAssistantClient>,
    mut events: broadcast::Receiver<MusicAssistantEvent>,
    player_id: PlayerId,
    state: Arc<Mutex<QueueState>>,
) {
    let mut snapshot: Option<
        futures_util::future::BoxFuture<
            'static,
            (QueueSnapshotRequest, u64, Result<QueueSnapshot, String>),
        >,
    > = None;
    let mut retry_at = tokio::time::Instant::now();
    loop {
        let should_refresh = {
            let state = state.lock().unwrap_or_else(|error| error.into_inner());
            state.is_connected && !state.projection.is_synchronized()
        };
        if snapshot.is_none() && should_refresh && tokio::time::Instant::now() >= retry_at {
            let (request, player_revision) = {
                let state = state.lock().unwrap_or_else(|error| error.into_inner());
                (state.projection.begin_snapshot(), state.player_revision)
            };
            let client = client.clone();
            let player_id = player_id.clone();
            snapshot = Some(Box::pin(async move {
                (
                    request,
                    player_revision,
                    load_snapshot(&client, &player_id).await,
                )
            }));
        }
        tokio::select! {
            biased;
            event = events.recv() => {
                let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                match event {
                    Ok(MusicAssistantEvent::Connected) => {
                        state.is_connected = true;
                        state.projection.invalidate();
                    }
                    Ok(MusicAssistantEvent::Disconnected) => {
                        state.is_connected = false;
                        state.projection.invalidate();
                    }
                    Ok(MusicAssistantEvent::QueueItemsUpdated(queue_id) | MusicAssistantEvent::QueueRemoved(queue_id))
                        if queue_id == state.projection.queue().queue_id => state.projection.invalidate(),
                    Ok(MusicAssistantEvent::QueueUpdated(queue))
                        if queue.queue_id == state.projection.queue().queue_id => state.projection.invalidate(),
                    Ok(MusicAssistantEvent::PlayerUpdated(player)) if player.player_id == player_id => {
                        state.player_revision += 1;
                        let has_changed_source = state.player.as_ref().map(|previous| &previous.active_source) != Some(&player.active_source);
                        state.player = Some(player);
                        if has_changed_source { state.projection.invalidate(); }
                    }
                    Ok(MusicAssistantEvent::PlayerRemoved(removed)) if removed == player_id => {
                        state.player_revision += 1;
                        state.player = None;
                        state.projection.invalidate();
                    }
                    Ok(MusicAssistantEvent::QueueTimeUpdated { queue_id, elapsed_seconds })
                        if queue_id == state.projection.queue().queue_id => {
                            state.player_revision += 1;
                            if let Some(player) = state.player.as_mut() { player.elapsed_time = Some(elapsed_seconds); }
                        }
                    Err(broadcast::error::RecvError::Lagged(_)) => state.projection.invalidate(),
                    Err(broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
            result = async { snapshot.as_mut().unwrap().await }, if snapshot.is_some() => {
                snapshot = None;
                match result {
                    (request, player_revision, Ok(response)) => {
                        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                        if response.queue.queue_id != state.projection.queue().queue_id {
                            state.projection.select_queue(response.queue.queue_id.clone());
                        } else if state.is_connected && state.projection.apply_snapshot(request, response.queue) {
                            if state.player_revision == player_revision {
                                state.player = Some(response.player);
                            }
                        }
                    }
                    _ => {
                        retry_at = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
                    }
                }
            }
            _ = tokio::time::sleep_until(retry_at), if snapshot.is_none() && should_refresh => {}
        }
    }
}

async fn load_snapshot(
    client: &MusicAssistantClient,
    player_id: &PlayerId,
) -> Result<QueueSnapshot, String> {
    let metadata: Option<MusicAssistantQueue> = client
        .execute_command(
            "player_queues/get_active_queue",
            serde_json::json!({"player_id": player_id}),
        )
        .await?;
    let metadata = metadata.ok_or_else(|| "MA player has no active queue".to_string())?;
    let player = client
        .execute_command("players/get", serde_json::json!({"player_id": player_id}))
        .await?;
    let mut items = Vec::new();
    loop {
        let page = client
            .get_queue_items(&metadata.queue_id.0, 500, items.len() as u32)
            .await?;
        let has_more = page.len() == 500;
        items.extend(page);
        if !has_more {
            break;
        }
    }
    Ok(QueueSnapshot {
        queue: Queue {
            queue_id: metadata.queue_id,
            items,
            current_index: metadata.current_index,
            shuffle_enabled: metadata.shuffle_enabled,
            repeat_mode: metadata.repeat_mode,
            autoplay_enabled: metadata.autoplay_enabled,
        },
        player,
    })
}

fn project_remote_player(state: &QueueState) -> PlayerState {
    let mut result = PlayerState {
        active_mode: PlaybackMode::Remote,
        connection_state: if state.is_connected {
            PlaybackConnectionState::Online
        } else {
            PlaybackConnectionState::Reconnecting
        },
        current_index: state.projection.queue().current_index,
        ..PlayerState::default()
    };
    if let Some(player) = &state.player {
        result.playback_state = player.playback_state;
        result.position_millis = player
            .elapsed_time
            .filter(|time| time.is_finite())
            .map(|time| (time.max(0.0) * 1000.0) as i64);
        result.volume = player.volume_level.unwrap_or(0);
        result.muted = player.volume_muted.unwrap_or(false);
        if let Some(media) = &player.current_media {
            result.current_queue_item_id = media.queue_item_id.clone();
            result.duration_millis = media
                .duration
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .map(|seconds| (seconds * 1000.0) as i64);
        }
    } else {
        result.playback_state = PlayState::Uninitialized;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_state_uses_ma_seconds_and_never_invents_a_local_buffer() {
        let player = serde_json::from_value(serde_json::json!({
            "player_id":"remote", "provider":"test", "type":"player", "name":"Remote", "available":true,
            "device_info":{"model":"test", "manufacturer":"test"}, "playback_state":"playing",
            "elapsed_time":12.5, "volume_level":37, "current_media":{"uri":"test", "duration":180.25, "queue_item_id":"item"}
        })).unwrap();
        let state = QueueState {
            projection: QueueProjection::new(QueueId("remote".into())),
            player: Some(player),
            player_revision: 0,
            is_connected: true,
        };
        let result = project_remote_player(&state);
        assert_eq!(result.position_millis, Some(12500));
        assert_eq!(result.duration_millis, Some(180250));
        assert_eq!(result.volume, 37);
        assert_eq!(result.buffered_position_millis, None);
        assert_eq!(result.current_queue_item_id.unwrap().0, "item");
    }

    #[test]
    fn disconnection_does_not_turn_playing_into_idle() {
        let state = QueueState {
            projection: QueueProjection::new(QueueId("remote".into())),
            player: None,
            player_revision: 0,
            is_connected: false,
        };
        assert_eq!(
            project_remote_player(&state).connection_state,
            PlaybackConnectionState::Reconnecting
        );
    }
}
