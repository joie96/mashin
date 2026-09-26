//! Engine-neutral queue and player-state contracts for platform adapters.

use crate::music_assistant::models::identifiers::{AccessToken, PlayerId, QueueId, QueueItemId};
use crate::music_assistant::models::queue::QueueItem;
use crate::music_assistant::models::queue::{PlayState, RepeatMode};
use crate::music_assistant::models::session::MusicAssistantServerUrl;
use crate::sendspin::models::SendspinEngineConfiguration;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PlaybackEngineKind {
    Offline,
    Sendspin,
    Remote,
}

impl PlaybackEngineKind {
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Sendspin => "Sendspin",
            Self::Remote => "remote",
        }
    }
}

#[derive(Clone)]
pub(crate) enum PlaybackEngineConfiguration {
    Offline(OfflinePlaybackConfiguration),
    Sendspin(SendspinEngineConfiguration),
    Remote(RemotePlaybackConfiguration),
}

#[derive(Clone)]
pub(crate) struct OfflinePlaybackConfiguration {
    pub(crate) library_root: PathBuf,
}

#[derive(Clone)]
pub(crate) struct RemotePlaybackConfiguration {
    pub(crate) music_assistant_url: MusicAssistantServerUrl,
    pub(crate) access_token: Option<AccessToken>,
    pub(crate) player_id: PlayerId,
}

impl PlaybackEngineConfiguration {
    pub(crate) fn kind(&self) -> PlaybackEngineKind {
        match self {
            Self::Offline(_) => PlaybackEngineKind::Offline,
            Self::Sendspin(_) => PlaybackEngineKind::Sendspin,
            Self::Remote(_) => PlaybackEngineKind::Remote,
        }
    }
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
            queue_id: QueueId("local-sendspin".to_string()),
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
            && self.items.len() == other.items.len()
            && self
                .items
                .iter()
                .zip(&other.items)
                .all(|(item, other_item)| queue_items_have_same_structure(item, other_item))
    }
}

fn queue_items_have_same_structure(item: &QueueItem, other: &QueueItem) -> bool {
    let mut item_without_cache = item.clone();
    let mut other_without_cache = other.clone();
    item_without_cache.cache = None;
    other_without_cache.cache = None;
    item_without_cache == other_without_cache
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerState {
    pub(crate) active_engine_kind: PlaybackEngineKind,
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
            active_engine_kind: PlaybackEngineKind::Sendspin,
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
        self.active_engine_kind == other.active_engine_kind
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
