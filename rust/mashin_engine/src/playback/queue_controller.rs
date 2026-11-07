//! Server-authoritative queue projection shared by local and remote playback adapters.
//! The worker owns snapshot ordering; callers can only issue MA commands or read snapshots.

use crate::domain::identifiers::{PlayerId, QueueId};
use crate::playback::mode::PlaybackMode;
use crate::playback::models::PlayState;
use crate::playback::models::{MediaSessionState, PlaybackConnectionState, PlayerState, Queue};
use crate::playback::queue_projection::{QueueProjection, QueueSnapshotRequest};
use crate::playback::server::PlaybackServer;
use crate::playback::server::ServerEvent;
use crate::playback::server::ServerPlayer;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, Notify};
use tokio::task::JoinHandle;

struct QueueState {
    projection: QueueProjection,
    player: Option<ServerPlayer>,
    player_revision: u64,
    is_connected: bool,
    connection_revision: u64,
    is_mutating: bool,
}

struct QueueSnapshot {
    queue: Queue,
    player: ServerPlayer,
}

pub(crate) struct QueueController {
    state: Arc<Mutex<QueueState>>,
    refresh_requested: Arc<Notify>,
    worker: Option<JoinHandle<()>>,
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

impl QueueController {
    // Construction and native lifetime.
    pub(crate) fn new(
        runtime: &tokio::runtime::Handle,
        client: Arc<dyn PlaybackServer>,
        events: broadcast::Receiver<ServerEvent>,
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
            worker: Some(worker),
        }
    }

    // The target retains the handle until the application joins outside playback locks.
    pub(crate) fn shutdown(mut self) -> JoinHandle<()> {
        self.stop_worker()
            .expect("queue worker is owned until shutdown")
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

    // Invalidate outstanding mutations before requesting worker cancellation.
    fn stop_worker(&mut self) -> Option<JoinHandle<()>> {
        let worker = self.worker.take()?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.is_connected = false;
        state.connection_revision = state.connection_revision.wrapping_add(1);
        state.projection.invalidate();
        worker.abort();
        Some(worker)
    }
}

impl Drop for QueueController {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

// Events continue to invalidate the request while pagination is in flight.
async fn run_projection(
    client: Arc<dyn PlaybackServer>,
    mut events: broadcast::Receiver<ServerEvent>,
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
                    load_snapshot(client.as_ref(), &player_id).await,
                )
            }));
        }
        tokio::select! {
            biased;
            event = events.recv() => {
                let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                match event {
                    Ok(ServerEvent::Connected) => {
                        state.connection_revision += 1;
                        state.is_connected = true;
                        state.projection.invalidate();
                    }
                    Ok(ServerEvent::Disconnected) => {
                        state.connection_revision += 1;
                        state.is_connected = false;
                        state.projection.invalidate();
                    }
                    Ok(ServerEvent::QueueItemsUpdated(queue_id) | ServerEvent::QueueRemoved(queue_id))
                        if queue_id == state.projection.queue().queue_id => state.projection.invalidate(),
                    Ok(ServerEvent::QueueUpdated(queue))
                        if queue.queue_id == state.projection.queue().queue_id => state.projection.invalidate(),
                    Ok(ServerEvent::PlayerUpdated(player)) if player.player_id == player_id => {
                        state.player_revision += 1;
                        let has_changed_source = state.player.as_ref().map(|previous| &previous.active_source) != Some(&player.active_source);
                        state.player = Some(player);
                        if has_changed_source { state.projection.invalidate(); }
                    }
                    Ok(ServerEvent::PlayerRemoved(removed)) if removed == player_id => {
                        state.player_revision += 1;
                        state.player = None;
                        state.projection.invalidate();
                    }
                    Ok(ServerEvent::QueueTimeUpdated { queue_id, elapsed_seconds })
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
    client: &dyn PlaybackServer,
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
