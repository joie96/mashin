//! Shared player commands and server-owned queue projection; never owns local audio.

use crate::domain::identifiers::{PlayerId, QueueItemId};
use crate::domain::media::MediaItem;
use crate::playback::backend::PlaybackCommand;
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{MediaSessionState, PlayerState, Queue};
use crate::playback::models::{QueueOption, RepeatMode};
use crate::playback::queue_controller::QueueController;
use crate::playback::queue_projection::QueueSnapshotRequest;
use crate::playback::server::PlaybackConnection;
use crate::playback::server::{PlayerCommand, QueueCommand};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub(crate) struct ServerTargetConfiguration {
    pub(crate) player_id: PlayerId,
    pub(crate) connection: Arc<dyn PlaybackConnection>,
    pub(crate) runtime: Handle,
}

pub(crate) struct ServerTarget {
    configuration: Option<ServerTargetConfiguration>,
    projection: Option<QueueController>,
    retired_workers: Vec<JoinHandle<()>>,
    pending_commands: Vec<PlaybackCommand>,
}

impl ServerTarget {
    // Construction and configuration.
    pub(crate) fn new() -> Self {
        Self {
            configuration: None,
            projection: None,
            retired_workers: Vec::new(),
            pending_commands: Vec::new(),
        }
    }

    pub(crate) fn configure(
        &mut self,
        configuration: ServerTargetConfiguration,
    ) -> Result<(), String> {
        self.shutdown();
        self.configuration = Some(configuration);
        Ok(())
    }

    // Projection lifecycle and EventHub lease ownership.
    pub(crate) fn activate(&mut self) -> Result<(), String> {
        if self.projection.is_some() {
            return Ok(());
        }
        let configuration = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?;
        let (events, is_connected) = configuration
            .connection
            .acquire_playback(&configuration.runtime)?;
        self.projection = Some(QueueController::new(
            &configuration.runtime,
            configuration.connection.client(),
            events,
            configuration.player_id.clone(),
            is_connected,
        ));
        Ok(())
    }

    pub(crate) fn deactivate(&mut self) {
        if let Some(projection) = self.projection.take() {
            self.retired_workers.push(projection.shutdown());
            if let Some(configuration) = &self.configuration {
                configuration.connection.release_playback();
            }
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.deactivate();
        self.configuration = None;
        self.pending_commands.clear();
    }

    // Deactivation and configuration changes keep ownership until the application can join.
    pub(crate) fn take_retired_workers(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.retired_workers)
    }

    // MA queue commands.
    pub(crate) fn queue_play_media(
        &mut self,
        media: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    ) -> Result<(), String> {
        if media.is_empty() && option == QueueOption::Replace {
            return self.queue_command(QueueCommand::Clear);
        }
        self.queue_command(QueueCommand::PlayMedia {
            media,
            option,
            start_item,
            shuffle,
        })
    }

    pub(crate) fn move_queue_item(
        &mut self,
        item_id: QueueItemId,
        position_shift: i32,
    ) -> Result<(), String> {
        self.queue_command(QueueCommand::MoveItem {
            item_id,
            shift: position_shift.into(),
        })
    }

    pub(crate) fn move_queue_item_to_end(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.queue_command(QueueCommand::MoveItemToEnd(item_id))
    }

    pub(crate) fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        let item_ids = item_ids
            .into_iter()
            .filter(|item_id| seen.insert(item_id.clone()))
            .collect::<Vec<_>>();
        if item_ids.is_empty()
            || item_ids
                .iter()
                .any(|item_id| item_id.as_str().trim().is_empty())
        {
            return Err("queue item IDs must not be empty".into());
        }
        let projection = self
            .projection
            .as_ref()
            .ok_or("remote queue is not active")?;
        let client = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?
            .connection
            .client();
        let mutation = projection.begin_mutation()?;
        self.pending_commands.push(Box::pin(async move {
            let total = item_ids.len();
            for (completed, item_id) in item_ids.into_iter().enumerate() {
                mutation.check_connection().map_err(|error| {
                    format!("Queue deletion stopped after {completed}/{total} requests: {error}")
                })?;
                client
                    .execute_queue_command(mutation.queue_id(), QueueCommand::DeleteItem(item_id))
                    .await
                    .map_err(|error| {
                        format!(
                            "Queue deletion stopped after {completed}/{total} requests: {error}"
                        )
                    })?;
            }
            mutation.check_connection()
        }));
        Ok(())
    }

    pub(crate) fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.queue_command(QueueCommand::PlayItem {
            item_id,
            position_seconds: 0,
        })
    }

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.queue_command(QueueCommand::PlayIndex(index))
    }

    // Player transport and queue preferences.
    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Play)
    }

    pub(crate) fn transport_pause(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Pause)
    }

    pub(crate) fn transport_stop(&mut self) {
        let result = self.player_command(PlayerCommand::Stop);
        self.record_failure(result);
    }

    pub(crate) fn transport_next(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Next)
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Previous)
    }

    pub(crate) fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        self.player_command(PlayerCommand::Seek(seconds))
    }

    pub(crate) fn set_volume(&mut self, volume: u8) {
        let result = self.player_command(PlayerCommand::Volume(volume));
        self.record_failure(result);
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        let result = self.player_command(PlayerCommand::Mute(muted));
        self.record_failure(result);
    }

    pub(crate) fn set_shuffle(&mut self, enabled: bool) {
        let result = self.queue_command(QueueCommand::Shuffle(enabled));
        self.record_failure(result);
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: RepeatMode) {
        let result = self.queue_command(QueueCommand::Repeat(mode));
        self.record_failure(result);
    }

    pub(crate) fn set_autoplay(&mut self, enabled: bool) {
        let result = self.queue_command(QueueCommand::Autoplay(enabled));
        self.record_failure(result);
    }

    pub(crate) fn take_pending_commands(&mut self) -> Vec<PlaybackCommand> {
        std::mem::take(&mut self.pending_commands)
    }

    // Read-only server projections and revision metadata.
    pub(crate) fn queue(&self) -> Queue {
        self.projection
            .as_ref()
            .map(|projection| projection.queue())
            .unwrap_or_default()
    }

    pub(crate) fn player_state(&self) -> PlayerState {
        self.projection
            .as_ref()
            .map(|projection| projection.remote_player_state())
            .unwrap_or_else(|| PlayerState {
                active_mode: PlaybackMode::Remote,
                ..PlayerState::default()
            })
    }

    pub(crate) fn queue_revision(&self) -> Option<(QueueSnapshotRequest, bool)> {
        self.projection.as_ref().map(QueueController::revision)
    }

    pub(crate) fn synchronized_snapshot(&self) -> Option<MediaSessionState> {
        self.projection.as_ref()?.synchronized_snapshot()
    }

    // Command serialization and asynchronous error delivery.
    fn player_command(&mut self, command: PlayerCommand) -> Result<(), String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?;
        let client = configuration.connection.client();
        let player_id = configuration.player_id.clone();
        self.pending_commands.push(Box::pin(async move {
            client.execute_player_command(&player_id, command).await
        }));
        Ok(())
    }

    fn queue_command(&mut self, command: QueueCommand) -> Result<(), String> {
        let projection = self
            .projection
            .as_ref()
            .ok_or("remote queue is not active")?;
        if !projection.is_synchronized() {
            return Err("remote queue is synchronizing".into());
        }
        let client = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?
            .connection
            .client();
        let queue_id = projection.queue().queue_id;
        self.pending_commands.push(Box::pin(async move {
            client
                .execute_queue_command(&queue_id, command)
                .await
                .map_err(String::from)
        }));
        Ok(())
    }

    fn record_failure(&mut self, result: Result<(), String>) {
        if let Err(error) = result {
            self.pending_commands
                .push(Box::pin(async move { Err(error) }));
        }
    }
}

impl Drop for ServerTarget {
    fn drop(&mut self) {
        self.shutdown();
    }
}
