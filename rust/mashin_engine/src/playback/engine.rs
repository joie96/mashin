//! Playback-engine contract and adapters owned by the playback coordinator.

use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::{MediaItem, Track};
use crate::music_assistant::models::queue::{QueueOption, RepeatMode};
use crate::playback::models::{PlaybackEngineConfiguration, PlayerState, Queue};
use std::sync::{Arc, Mutex};

pub(crate) trait PlaybackEngine: Send {
    fn configure(&mut self, configuration: PlaybackEngineConfiguration) -> Result<(), String>;
    fn activate(&mut self) -> Result<(), String>;
    fn deactivate(&mut self);
    fn shutdown(&mut self);
    fn queue_play_media(
        &mut self,
        media_items: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
        start_download: bool,
    ) -> Result<(), String>;
    fn queue_start_radio(&mut self, started_tracks: Vec<MediaItem>) -> Result<(), String>;
    fn queue_append_radio_tracks(
        &mut self,
        seed_tracks: Vec<Track>,
        radio_tracks: Vec<Track>,
    ) -> Result<(), String>;
    fn insert_queue_items(&mut self, index: usize, media_items: Vec<MediaItem>) -> Result<(), String>;
    fn append_queue_items(&mut self, media_items: Vec<MediaItem>) -> Result<(), String>;
    fn move_queue_items(&mut self, item_ids: Vec<QueueItemId>, target_index: usize) -> Result<(), String>;
    fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String>;
    fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String>;
    fn queue_play(&mut self, index: usize) -> Result<(), String>;
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
    fn preferred_codec_configuration(
        &mut self,
    ) -> Option<&mut dyn PreferredCodecConfigurablePlaybackEngine> {
        None
    }
    fn start_background_work(&mut self, _engine: Arc<Mutex<Box<dyn PlaybackEngine>>>) {}
    fn stop_background_work(&mut self) {}
    fn process_playback_cycle(&mut self) {}
    fn set_autoplay(&mut self, _is_enabled: bool) {}
}

// Local Sendspin playback allows selecting a downloader/audio codec.
pub(crate) trait PreferredCodecConfigurablePlaybackEngine: Send {
    fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String>;
}
