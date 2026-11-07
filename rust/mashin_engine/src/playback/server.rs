//! Playback capabilities; transport implementation and authentication stay behind this port.

use crate::domain::identifiers::{MediaId, PlayerId, QueueId, QueueItemId};
use crate::domain::media::MediaItem;
use crate::playback::models::{PlayState, QueueItem, QueueOption, RepeatMode};
use futures_util::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub(crate) struct ServerPlayerMedia {
    pub(crate) queue_item_id: Option<QueueItemId>,
    pub(crate) duration: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub(crate) struct ServerPlayer {
    pub(crate) player_id: PlayerId,
    #[serde(default)]
    pub(crate) playback_state: PlayState,
    pub(crate) elapsed_time: Option<f64>,
    pub(crate) volume_level: Option<u8>,
    pub(crate) volume_muted: Option<bool>,
    pub(crate) active_source: Option<String>,
    pub(crate) current_media: Option<ServerPlayerMedia>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub(crate) struct ServerQueue {
    pub(crate) queue_id: QueueId,
    #[serde(default)]
    pub(crate) current_index: Option<usize>,
    #[serde(default)]
    pub(crate) shuffle_enabled: bool,
    #[serde(default)]
    pub(crate) repeat_mode: RepeatMode,
    #[serde(default)]
    pub(crate) autoplay_enabled: bool,
    #[serde(default)]
    pub(crate) current_item: Option<QueueItem>,
}

pub(crate) enum PlayerCommand {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Seek(f64),
    Volume(u8),
    Mute(bool),
}

pub(crate) enum QueueCommand {
    Clear,
    PlayMedia {
        media: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    },
    PlayIndex(usize),
    PlayItem {
        item_id: QueueItemId,
        position_seconds: u64,
    },
    MoveItem {
        item_id: QueueItemId,
        shift: i64,
    },
    DeleteItem(QueueItemId),
    MoveItemToEnd(QueueItemId),
    Shuffle(bool),
    Repeat(RepeatMode),
    Autoplay(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ServerEvent {
    Connected,
    Disconnected,
    FavoritesChanged,
    PlaylistsChanged,
    PlaylistChanged(MediaId),
    QueueUpdated(ServerQueue),
    QueueItemsUpdated(QueueId),
    QueueRemoved(QueueId),
    QueueTimeUpdated {
        queue_id: QueueId,
        elapsed_seconds: f64,
    },
    PlayerUpdated(ServerPlayer),
    PlayerRemoved(PlayerId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlaybackServerError {
    Denied(String),
    Failed(String),
}

impl std::fmt::Display for PlaybackServerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(message) | Self::Failed(message) => formatter.write_str(message),
        }
    }
}

impl From<PlaybackServerError> for String {
    fn from(error: PlaybackServerError) -> Self {
        error.to_string()
    }
}

pub(crate) trait PlaybackServer: Send + Sync {
    fn execute_player_command<'a>(
        &'a self,
        player: &'a PlayerId,
        command: PlayerCommand,
    ) -> BoxFuture<'a, Result<(), String>>;
    fn execute_queue_command<'a>(
        &'a self,
        queue: &'a QueueId,
        command: QueueCommand,
    ) -> BoxFuture<'a, Result<(), PlaybackServerError>>;
    fn get_player<'a>(
        &'a self,
        player: &'a PlayerId,
    ) -> BoxFuture<'a, Result<ServerPlayer, String>>;
    fn get_active_queue<'a>(
        &'a self,
        player: &'a PlayerId,
    ) -> BoxFuture<'a, Result<Option<ServerQueue>, String>>;
    fn get_player_queues(&self) -> BoxFuture<'_, Result<Vec<ServerQueue>, PlaybackServerError>>;
    fn get_queue_items<'a>(
        &'a self,
        queue: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Vec<QueueItem>, String>>;
}

pub(crate) trait PlaybackConnection: Send + Sync {
    fn server_url(&self) -> String;
    fn client(&self) -> Arc<dyn PlaybackServer>;
    fn acquire_playback(
        &self,
        runtime: &tokio::runtime::Handle,
    ) -> Result<(broadcast::Receiver<ServerEvent>, bool), String>;
    fn release_playback(&self);
}
