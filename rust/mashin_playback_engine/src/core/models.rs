//! Typed application-core configuration, event, and error contracts.

use crate::music_assistant::models::identifiers::{AccessToken, PlayerId, UserId};
use crate::music_assistant::models::session::MusicAssistantServerUrl;
use crate::playback::models::{PlayerState, Queue};
use crate::sendspin::models::SendspinAudioCodec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CoreErrorCode {
    InvalidArgument,
    CommandFailed,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CoreErrorSource {
    Ffi,
    Core,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoreError {
    pub(crate) code: CoreErrorCode,
    pub(crate) message: String,
    pub(crate) is_retryable: bool,
    pub(crate) source: CoreErrorSource,
}

impl CoreError {
    // Error construction.
    pub(crate) fn command_failed(message: impl Into<String>) -> Self {
        Self {
            code: CoreErrorCode::CommandFailed,
            message: message.into(),
            is_retryable: false,
            source: CoreErrorSource::Core,
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            code: CoreErrorCode::Internal,
            message: message.into(),
            is_retryable: false,
            source: CoreErrorSource::Core,
        }
    }
}

impl From<String> for CoreError {
    fn from(message: String) -> Self {
        Self {
            code: CoreErrorCode::InvalidArgument,
            message,
            is_retryable: false,
            source: CoreErrorSource::Ffi,
        }
    }
}

/// A validated Music Assistant read request received at the FFI boundary.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MusicAssistantQueryRequest {
    pub(crate) request_id: u64,
    #[serde(flatten)]
    pub(crate) query: MusicAssistantQuery,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum MusicAssistantQuery {
    Recommendations,
    ProviderManifests,
    ProviderIconData {
        provider: String,
    },
    RecentlyPlayedTracks {
        limit: u32,
    },
    SearchMusic {
        query: String,
        limit: u32,
    },
    Artist {
        item_id: String,
        provider: String,
    },
    Album {
        item_id: String,
        provider: String,
    },
    AlbumTracks {
        item_id: String,
        provider: String,
    },
    Playlist {
        item_id: String,
        provider: String,
    },
    PlaylistTracks {
        item_id: String,
        provider: String,
        force_refresh: bool,
        allow_dynamic_tracks: bool,
    },
    ArtistAlbums {
        item_id: String,
        provider: String,
    },
    ArtistTopTracks {
        item_id: String,
        provider: String,
    },
    SimilarArtists {
        item_id: String,
        provider: String,
        limit: u32,
    },
    SimilarTracks {
        item_id: String,
        provider: String,
        limit: u32,
        allow_lookup: bool,
    },
    TrackVersions {
        item_id: String,
        provider: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SelectedOutput {
    Offline(OfflineOutputConfig),
    Sendspin(SendspinOutputConfig),
    Remote(RemoteOutputConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OfflineOutputConfig {
    pub(crate) library_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendspinOutputConfig {
    pub(crate) sendspin_url: String,
    pub(crate) client_id: String,
    pub(crate) client_name: String,
    pub(crate) downloader_buffer_bytes: u64,
    pub(crate) cache_bytes: usize,
    pub(crate) preferred_codec: SendspinAudioCodec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteOutputConfig {
    pub(crate) player_id: PlayerId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionConfig {
    pub(crate) music_assistant_url: MusicAssistantServerUrl,
    pub(crate) access_token: Option<AccessToken>,
    pub(crate) user_id: Option<UserId>,
    pub(crate) username: String,
    pub(crate) selected_output: SelectedOutput,
    pub(crate) volume: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginConfig {
    pub(crate) music_assistant_url: MusicAssistantServerUrl,
    pub(crate) username: String,
    pub(crate) password: String,
}

/// A change to one projection of the active media session.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum MediaSessionEvent {
    QueueChanged { queue: Queue },
    PlayerStateChanged { player_state: PlayerState },
}

#[cfg(test)]
mod tests {
    use super::{MediaSessionEvent, MusicAssistantQuery, MusicAssistantQueryRequest};
    use crate::playback::models::PlayerState;

    #[test]
    fn serializes_player_state_events_with_the_dart_contract_key() {
        let event = MediaSessionEvent::PlayerStateChanged {
            player_state: PlayerState::default(),
        };

        let payload = serde_json::to_value(event).expect("serialize player state event");

        assert_eq!(payload["type"], "player_state_changed");
        assert!(payload.get("player_state").is_some());
        assert!(payload.get("playerState").is_none());
    }

    #[test]
    fn decodes_playlist_track_query_from_the_dart_contract() {
        let request = serde_json::from_str::<MusicAssistantQueryRequest>(
            r#"{
                "requestId": 42,
                "kind": "playlistTracks",
                "itemId": "playlist-1",
                "provider": "library",
                "forceRefresh": true,
                "allowDynamicTracks": false
            }"#,
        )
        .expect("decode playlist track query");

        assert_eq!(request.request_id, 42);
        assert!(matches!(
            request.query,
            MusicAssistantQuery::PlaylistTracks {
                item_id,
                provider,
                force_refresh: true,
                allow_dynamic_tracks: false,
            } if item_id == "playlist-1" && provider == "library"
        ));
    }
}
