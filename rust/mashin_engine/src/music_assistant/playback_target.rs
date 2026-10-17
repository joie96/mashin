//! Shared player commands and server-owned queue projection; never owns local audio.

use crate::core::core::CORE_NETWORK_RUNTIME;
use crate::music_assistant::commands::{PlayerCommand, QueueCommand};
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::event_hub::EventHubLeaseReason;
use crate::music_assistant::models::identifiers::{PlayerId, QueueItemId};
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{QueueOption, RepeatMode};
use crate::music_assistant::queue_controller::MusicAssistantQueueController;
use crate::music_assistant::queue_projection::QueueSnapshotRequest;
use crate::playback::backend::PlaybackCommand;
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{PlayerState, Queue};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct MusicAssistantPlaybackTargetConfiguration {
    pub(crate) player_id: PlayerId,
    pub(crate) connection: Arc<MusicAssistantConnection>,
}

pub(crate) struct MusicAssistantPlaybackTarget {
    configuration: Option<MusicAssistantPlaybackTargetConfiguration>,
    projection: Option<MusicAssistantQueueController>,
    pending_commands: Vec<PlaybackCommand>,
}

impl MusicAssistantPlaybackTarget {
    // Construction and configuration.
    pub(crate) fn new() -> Self {
        Self {
            configuration: None,
            projection: None,
            pending_commands: Vec::new(),
        }
    }

    pub(crate) fn configure(
        &mut self,
        configuration: MusicAssistantPlaybackTargetConfiguration,
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
        let mut hub = configuration
            .connection
            .event_hub
            .lock()
            .map_err(|_| "MA event hub lock poisoned")?;
        let events = hub.subscribe();
        let _runtime = CORE_NETWORK_RUNTIME.enter();
        hub.acquire(EventHubLeaseReason::Playback);
        self.projection = Some(MusicAssistantQueueController::new(
            &CORE_NETWORK_RUNTIME,
            configuration.connection.client.clone(),
            events,
            configuration.player_id.clone(),
            hub.is_connected(),
        ));
        Ok(())
    }

    pub(crate) fn deactivate(&mut self) {
        if self.projection.take().is_some() {
            if let Some(configuration) = &self.configuration {
                if let Ok(mut hub) = configuration.connection.event_hub.lock() {
                    hub.release(EventHubLeaseReason::Playback);
                }
            }
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.deactivate();
        self.configuration = None;
        self.pending_commands.clear();
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
            radio_mode: false,
        })
    }

    pub(crate) fn start_radio(&mut self, media: Vec<MediaItem>) -> Result<(), String> {
        self.queue_command(QueueCommand::PlayMedia {
            media,
            option: QueueOption::Play,
            start_item: None,
            shuffle: false,
            radio_mode: true,
        })
    }

    pub(crate) fn insert_queue_items(
        &mut self,
        index: usize,
        media: Vec<MediaItem>,
    ) -> Result<(), String> {
        let queue = self.queue();
        if index == queue.items.len() {
            return self.append_queue_items(media);
        }
        if Some(index) == queue.current_index.map(|current| current + 1) {
            return self.queue_play_media(media, QueueOption::Next, None, false);
        }
        Err("MA supports inserting next or appending; use move for existing items".into())
    }

    pub(crate) fn append_queue_items(&mut self, media: Vec<MediaItem>) -> Result<(), String> {
        self.queue_play_media(media, QueueOption::Add, None, false)
    }

    pub(crate) fn move_queue_items(
        &mut self,
        item_ids: Vec<QueueItemId>,
        target_index: usize,
    ) -> Result<(), String> {
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
            .client
            .clone();
        let queue_id = projection.queue().queue_id;
        self.pending_commands.push(Box::pin(async move {
            client
                .move_queue_items_to(&queue_id, item_ids, target_index)
                .await
        }));
        Ok(())
    }

    pub(crate) fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String> {
        for item_id in item_ids {
            self.queue_command(QueueCommand::DeleteItem(item_id))?;
        }
        Ok(())
    }

    pub(crate) fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.queue_command(QueueCommand::PlayItem(item_id))
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
        self.projection
            .as_ref()
            .map(MusicAssistantQueueController::revision)
    }

    // Command serialization and asynchronous error delivery.
    fn player_command(&mut self, command: PlayerCommand) -> Result<(), String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?;
        let client = configuration.connection.client.clone();
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
            .client
            .clone();
        let queue_id = projection.queue().queue_id;
        self.pending_commands.push(Box::pin(async move {
            client.execute_queue_command(&queue_id, command).await
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

impl Drop for MusicAssistantPlaybackTarget {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(crate) fn unconfigured_remote_has_no_local_audio_and_rejects_commands() {
        let mut engine = MusicAssistantPlaybackTarget::new();
        assert_eq!(engine.player_state().active_mode, PlaybackMode::Remote);
        assert_eq!(engine.player_state().buffered_position_millis, None);
        assert!(engine.transport_play().is_err());
        assert!(engine.play_queue_index(0).is_err());
        engine.shutdown();
    }
}
