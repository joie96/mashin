//! Engine-neutral queue and player-state contracts for platform adapters.

use crate::music_assistant::models::identifiers::{PlayerId, QueueId, QueueItemId};
use crate::music_assistant::models::queue::QueueItem;
use crate::music_assistant::models::queue::{PlayState, RepeatMode};
use serde::Serialize;

use crate::playback::mode::PlaybackMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlaybackTarget {
    Local,
    Remote(PlayerId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaybackSessionConfiguration {
    pub(crate) local: crate::playback::local::session::LocalSessionConfiguration,
    pub(crate) target: PlaybackTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlaybackConnectionState {
    Offline,
    Connecting,
    Online,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Queue {
    pub(crate) queue_id: QueueId,
    pub(crate) items: Vec<QueueItem>,
    pub(crate) current_index: Option<usize>,
    pub(crate) shuffle_enabled: bool,
    pub(crate) repeat_mode: RepeatMode,
    pub(crate) autoplay_enabled: bool,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            queue_id: QueueId(String::new()),
            items: Vec::new(),
            current_index: None,
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
            autoplay_enabled: false,
        }
    }
}

impl Queue {
    pub(crate) fn has_same_structure_as(&self, other: &Self) -> bool {
        self.queue_id == other.queue_id
            && self.current_index == other.current_index
            && self.shuffle_enabled == other.shuffle_enabled
            && self.repeat_mode == other.repeat_mode
            && self.autoplay_enabled == other.autoplay_enabled
            && self.items == other.items
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerState {
    pub(crate) active_mode: PlaybackMode,
    pub(crate) connection_state: PlaybackConnectionState,
    pub(crate) playback_state: PlayState,
    pub(crate) current_queue_item_id: Option<QueueItemId>,
    pub(crate) current_index: Option<usize>,
    pub(crate) position_millis: Option<i64>,
    pub(crate) duration_millis: Option<i64>,
    pub(crate) buffered_position_millis: Option<i64>,
    pub(crate) volume: u8,
    pub(crate) muted: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            active_mode: PlaybackMode::Local,
            connection_state: PlaybackConnectionState::Offline,
            playback_state: PlayState::Uninitialized,
            current_queue_item_id: None,
            current_index: None,
            position_millis: None,
            duration_millis: None,
            buffered_position_millis: None,
            volume: 50,
            muted: false,
        }
    }
}

impl PlayerState {
    pub(crate) fn has_same_non_position_as(&self, other: &Self) -> bool {
        self.active_mode == other.active_mode
            && self.connection_state == other.connection_state
            && self.playback_state == other.playback_state
            && self.current_queue_item_id == other.current_queue_item_id
            && self.current_index == other.current_index
            && self.duration_millis == other.duration_millis
            && self.buffered_position_millis == other.buffered_position_millis
            && self.volume == other.volume
            && self.muted == other.muted
    }
}

/// Complete read projection of the active media session without a second state owner.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaSessionState {
    pub(crate) queue: Queue,
    pub(crate) player_state: PlayerState,
}
