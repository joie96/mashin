//! Shared command and snapshot contract for the two engine-owned playback modes.

use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{QueueOption, RepeatMode};
use crate::playback::models::{MediaSessionState, PlayerState, Queue};

pub(crate) type PlaybackCommand = futures_util::future::BoxFuture<'static, Result<(), String>>;

pub(crate) trait PlaybackBackend: Send {
    fn activate(&mut self) -> Result<(), String>;
    fn deactivate(&mut self);
    fn shutdown(&mut self);
    fn queue_play_media(
        &mut self,
        media_items: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    ) -> Result<(), String>;
    fn start_radio(&mut self, started_tracks: Vec<MediaItem>) -> Result<(), String>;
    fn insert_queue_items(
        &mut self,
        index: usize,
        media_items: Vec<MediaItem>,
    ) -> Result<(), String>;
    fn append_queue_items(&mut self, media_items: Vec<MediaItem>) -> Result<(), String>;
    fn move_queue_items(
        &mut self,
        item_ids: Vec<QueueItemId>,
        target_index: usize,
    ) -> Result<(), String>;
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
    fn process_playback_cycle(&mut self);
    fn set_autoplay(&mut self, is_enabled: bool);
    fn take_pending_commands(&mut self) -> Vec<PlaybackCommand>;
}
