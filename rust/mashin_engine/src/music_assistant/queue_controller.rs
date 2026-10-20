//! Server-authoritative queue projection shared by local and remote playback adapters.
//! The worker owns snapshot ordering; callers can only issue MA commands or read snapshots.

use super::client::MusicAssistantClient;
use super::event_hub::MusicAssistantEvent;
use super::models::identifiers::{PlayerId, QueueId};
use super::models::player::MusicAssistantPlayer;
use super::models::queue::PlayState;
use super::queue_projection::{QueueProjection, QueueSnapshotRequest};
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{MediaSessionState, PlaybackConnectionState, PlayerState, Queue};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, Notify};
use tokio::task::JoinHandle;

struct QueueState {
    projection: QueueProjection,
    player: Option<MusicAssistantPlayer>,
    player_revision: u64,
    is_connected: bool,
    connection_revision: u64,
    is_mutating: bool,
}

struct QueueSnapshot {
    queue: Queue,
    player: MusicAssistantPlayer,
}

pub(crate) struct MusicAssistantQueueController {
    state: Arc<Mutex<QueueState>>,
    refresh_requested: Arc<Notify>,
    worker: JoinHandle<()>,
}

pub(crate) struct QueueMutation {
    state: Arc<Mutex<QueueState>>,
    refresh_requested: Arc<Notify>,
    connection_revision: u64,
    queue_id: QueueId,
}

impl QueueMutation {
    // Commands remain bound to the connection and queue selected when the batch began.
    pub(crate) fn check_connection(&self) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "queue state lock poisoned")?;
        if !state.is_connected
            || state.connection_revision != self.connection_revision
            || state.projection.queue().queue_id != self.queue_id
        {
            return Err("queue deletion interrupted by a connection or queue change".into());
        }
        Ok(())
    }

    pub(crate) fn queue_id(&self) -> &QueueId {
        &self.queue_id
    }
}

impl Drop for QueueMutation {
    // Cancellation and partial failures must also resume snapshot loading.
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.is_mutating = false;
        state.projection.invalidate();
        self.refresh_requested.notify_one();
    }
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
            connection_revision: 0,
            is_mutating: false,
        }));
        let refresh_requested = Arc::new(Notify::new());
        let worker = runtime.spawn(run_projection(
            client,
            events,
            player_id,
            state.clone(),
            refresh_requested.clone(),
        ));
        Self {
            state,
            refresh_requested,
            worker,
        }
    }

    // Batch mutations defer server snapshots without changing local playback authority.
    pub(crate) fn begin_mutation(&self) -> Result<QueueMutation, String> {
        let mut state = self.state.lock().map_err(|_| "queue state lock poisoned")?;
        if !state.is_connected || !state.projection.is_synchronized() || state.is_mutating {
            return Err("remote queue is synchronizing".into());
        }
        let mutation = QueueMutation {
            state: self.state.clone(),
            refresh_requested: self.refresh_requested.clone(),
            connection_revision: state.connection_revision,
            queue_id: state.projection.queue().queue_id.clone(),
        };
        state.is_mutating = true;
        state.projection.invalidate();
        Ok(mutation)
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

    pub(crate) fn synchronized_snapshot(&self) -> Option<MediaSessionState> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        (state.is_connected && state.projection.is_synchronized()).then(|| MediaSessionState {
            queue: state.projection.queue().clone(),
            player_state: project_remote_player(&state),
        })
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
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.is_connected = false;
        state.connection_revision += 1;
        state.projection.invalidate();
        self.worker.abort();
    }
}

// Events continue to invalidate the request while pagination is in flight.
async fn run_projection(
    client: Arc<MusicAssistantClient>,
    mut events: broadcast::Receiver<MusicAssistantEvent>,
    player_id: PlayerId,
    state: Arc<Mutex<QueueState>>,
    refresh_requested: Arc<Notify>,
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
            state.is_connected && !state.is_mutating && !state.projection.is_synchronized()
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
                        state.connection_revision += 1;
                        state.is_connected = true;
                        state.projection.invalidate();
                    }
                    Ok(MusicAssistantEvent::Disconnected) => {
                        state.connection_revision += 1;
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
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        state.connection_revision += 1;
                        state.projection.invalidate();
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
            result = async { snapshot.as_mut().unwrap().await }, if snapshot.is_some() => {
                snapshot = None;
                match result {
                    (request, player_revision, Ok(response)) => {
                        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                        if !state.is_connected || request != state.projection.begin_snapshot() {
                            continue;
                        }
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
            _ = refresh_requested.notified() => {}
        }
    }
}

async fn load_snapshot(
    client: &MusicAssistantClient,
    player_id: &PlayerId,
) -> Result<QueueSnapshot, String> {
    let metadata = client.get_active_queue(player_id).await?;
    let metadata = metadata.ok_or_else(|| "MA player has no active queue".to_string())?;
    let player = client.get_player(player_id).await?;
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
            connection_revision: 0,
            is_mutating: false,
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
            connection_revision: 0,
            is_mutating: false,
        };
        assert_eq!(
            project_remote_player(&state).connection_state,
            PlaybackConnectionState::Reconnecting
        );
    }

    // Batch lifetime and reconnect isolation.
    #[test]
    fn mutation_survives_item_events_but_not_reconnects() {
        let state = Arc::new(Mutex::new(QueueState {
            projection: QueueProjection::new(QueueId("queue".into())),
            player: None,
            player_revision: 0,
            is_connected: true,
            connection_revision: 4,
            is_mutating: true,
        }));
        let mutation = QueueMutation {
            state: state.clone(),
            refresh_requested: Arc::new(Notify::new()),
            connection_revision: 4,
            queue_id: QueueId("queue".into()),
        };
        state.lock().unwrap().projection.invalidate();
        assert!(mutation.check_connection().is_ok());
        state.lock().unwrap().connection_revision += 2;
        assert!(mutation.check_connection().is_err());
        drop(mutation);
        let state = state.lock().unwrap();
        assert!(!state.is_mutating);
        assert!(!state.projection.is_synchronized());
    }

    #[tokio::test]
    async fn dropping_mutation_wakes_the_snapshot_worker() {
        let state = Arc::new(Mutex::new(QueueState {
            projection: QueueProjection::new(QueueId("queue".into())),
            player: None,
            player_revision: 0,
            is_connected: true,
            connection_revision: 0,
            is_mutating: true,
        }));
        let refresh_requested = Arc::new(Notify::new());
        let mutation = QueueMutation {
            state,
            refresh_requested: refresh_requested.clone(),
            connection_revision: 0,
            queue_id: QueueId("queue".into()),
        };
        drop(mutation);
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            refresh_requested.notified(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dropping_the_controller_interrupts_its_active_mutation() {
        let mut projection = QueueProjection::new(QueueId("queue".into()));
        assert!(projection.apply_snapshot(projection.begin_snapshot(), projection.queue().clone()));
        let controller = MusicAssistantQueueController {
            state: Arc::new(Mutex::new(QueueState {
                projection,
                player: None,
                player_revision: 0,
                is_connected: true,
                connection_revision: 0,
                is_mutating: false,
            })),
            refresh_requested: Arc::new(Notify::new()),
            worker: tokio::spawn(std::future::pending()),
        };
        let mutation = controller.begin_mutation().unwrap();
        assert!(controller.begin_mutation().is_err());
        drop(controller);
        assert!(mutation.check_connection().is_err());
    }
}
