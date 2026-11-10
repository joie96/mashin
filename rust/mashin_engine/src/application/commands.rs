//! Application commands independent of native calling conventions.

use crate::domain::identifiers::{PlayerId, QueueItemId};
use crate::domain::media::{MediaItem, Track};
use crate::domain::settings::{PreferredProvider, SessionSettingsUpdate, UpdateChannel};
use crate::playback::models::QueueOption;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ApplicationCommand {
    Login {
        music_assistant_url: String,
        username: String,
        password: String,
    },
    Logout {},
    RestorePersistedSession {},
    ActivateEngine {},
    DeactivateEngine {},
    Shutdown {},
    ClearDatabaseCache {},
    ClearImageCache {},
    SelectLocalOutput {},
    SelectRemotePlayer {
        player_id: PlayerId,
    },
    TransportPlay {},
    TransportPause {},
    TransportStop {},
    TransportNext {},
    TransportPrevious {},
    TransportSeek {
        seconds: f64,
    },
    TransportRepeatMode {
        mode: u8,
    },
    TransportShuffle {
        enabled: bool,
    },
    SetAutoplay {
        enabled: bool,
    },
    SetVolume {
        volume: u8,
    },
    SetMuted {
        muted: bool,
    },
    SaveSessionSettings {
        #[serde(flatten)]
        settings: SessionSettingsUpdate,
    },
    SaveUpdateChannel {
        channel: UpdateChannel,
    },
    SetPreferredProvider {
        #[serde(deserialize_with = "Option::deserialize")]
        preferred_provider: Option<PreferredProvider>,
    },
    SetUserDataFavorites {
        media_items: Vec<MediaItem>,
        favorite: bool,
    },
    QueuePlayMedia {
        request: QueuePlayMediaRequest,
    },
    StartRadio {
        media_items: Vec<MediaItem>,
    },
    MoveQueueItem {
        item_id: QueueItemId,
        position_shift: i32,
    },
    MoveQueueItemToEnd {
        item_id: QueueItemId,
    },
    DeleteQueueItems {
        item_ids: Vec<QueueItemId>,
    },
    SelectQueueItem {
        item_id: QueueItemId,
    },
    PlayQueueIndex {
        index: usize,
    },
    SetAudioProfile {
        audio_profile: String,
    },
    AddPlaylistTracks {
        playlist_id: String,
        uris: Vec<String>,
    },
    RemovePlaylistTracks {
        playlist_id: String,
        positions: Vec<u32>,
    },
    CreatePrivatePlaylist {
        name: String,
    },
    RenamePrivatePlaylist {
        playlist_id: String,
        name: String,
    },
    RefreshPrivatePlaylistCover {
        playlist_id: String,
    },
    RemovePrivatePlaylist {
        playlist_id: String,
    },
    AddPrivatePlaylistTracks {
        playlist_id: String,
        tracks: Vec<Track>,
    },
    RemovePrivatePlaylistTracks {
        playlist_id: String,
        uris: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct QueuePlayMediaRequest {
    #[serde(default)]
    pub(crate) media_items: Vec<MediaItem>,
    pub(crate) option: QueueOption,
    #[serde(default)]
    pub(crate) start_item: Option<usize>,
    #[serde(default)]
    pub(crate) shuffle: bool,
}
