//! Shared command and snapshot contract for the two engine-owned playback modes.

use crate::application::events::{ApplicationEvent, EventPublisher};
use crate::domain::identifiers::QueueItemId;
use crate::domain::media::MediaItem;
use crate::playback::models::{MediaSessionState, PlayerState, Queue};
use crate::playback::models::{QueueOption, RepeatMode};
use crate::playback::start::{PlaybackStart, StartObservation};
use std::sync::{Arc, Mutex};

pub(crate) struct PresentationEvents {
    pub(crate) publisher: EventPublisher,
    pub(crate) events: Vec<ApplicationEvent>,
}

impl PresentationEvents {
    // Delivery runs only after the backend lock has been released.
    pub(crate) fn publish(self, should_publish: impl Fn() -> bool) {
        for event in self.events {
            if !should_publish() {
                break;
            }
            self.publisher.publish(event);
        }
    }
}

pub(crate) type PlaybackCommand = futures_util::future::BoxFuture<'static, Result<(), String>>;

pub(crate) trait PlaybackBackend: Send {
    fn start_presentation(&self) -> Arc<Mutex<PlaybackStart>>;
    fn start_observation(&self) -> StartObservation;
    fn activate(&mut self) -> Result<(), String>;
    fn deactivate(&mut self);
    fn shutdown(&mut self);
    fn take_retired_workers(&mut self) -> Vec<tokio::task::JoinHandle<()>>;
    fn queue_play_media(
        &mut self,
        media_items: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    ) -> Result<(), String>;
    fn move_queue_item(&mut self, item_id: QueueItemId, position_shift: i32) -> Result<(), String>;
    fn move_queue_item_to_end(&mut self, item_id: QueueItemId) -> Result<(), String>;
    fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String>;
    fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String>;
    fn play_queue_index(&mut self, index: usize) -> Result<(), String>;
    fn transport_play(&mut self) -> Result<(), String>;
    fn transport_pause(&mut self) -> Result<(), String>;
    fn transport_stop(&mut self);
    fn transport_next(&mut self) -> Result<(), String>;
    fn transport_previous(&mut self) -> Result<(), String>;
    fn transport_seek(&mut self, seconds: f64) -> Result<(), String>;
    fn set_volume(&mut self, volume: u8);
    fn set_muted(&mut self, muted: bool);
    fn set_shuffle(&mut self, enabled: bool);
    fn set_repeat_mode(&mut self, mode: RepeatMode);
    fn queue(&self) -> Queue;
    fn player_state(&self) -> PlayerState;
    fn media_session_state(&self) -> MediaSessionState {
        MediaSessionState {
            queue: self.queue(),
            player_state: self.player_state(),
        }
    }
    fn collect_presentation_events(&mut self) -> PresentationEvents;
    fn set_autoplay(&mut self, is_enabled: bool);
    fn take_pending_commands(&mut self) -> Vec<PlaybackCommand>;
}
