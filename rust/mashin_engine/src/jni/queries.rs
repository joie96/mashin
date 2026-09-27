//! Synchronous Android JNI query adapter for the Rust-owned playback core.

use crate::core;
use crate::core::models::CoreError;
use crate::music_assistant::models::media::{
    Album, Artist, MusicAssistantSearchResults, Playlist, Track,
};
use crate::music_assistant::models::recommendation::MusicAssistantRecommendationFolder;
use crate::music_assistant::models::session::ProviderManifest;
use crate::music_assistant::user_data::UserDataSnapshot;
use crate::playback::models::{PlayerState, Queue};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemReferenceQuery {
    item_id: String,
    provider: String,
    #[serde(default = "default_query_limit")]
    limit: u32,
    #[serde(default)]
    force_refresh: bool,
    #[serde(default)]
    allow_dynamic_tracks: bool,
    #[serde(default)]
    allow_lookup: bool,
}

// JNI query routing.
pub(crate) fn execute_query(kind: &str, argument: Option<&str>) -> Result<String, String> {
    match kind {
        "sessionSettings" => serialize_query(get_session_settings()),
        "queue" => serialize_query(get_queue()),
        "playerState" => serialize_query(get_player_state()),
        "userData" => serialize_query(core::read_or_load_user_data()),
        "recommendations" => serialize_query(get_recommendations()),
        "providerManifests" => serialize_query(get_provider_manifests()),
        "providerIconData" => {
            serialize_query(get_provider_icon_data(required_text_argument(argument)?))
        }
        "recentlyPlayed" => serialize_query(get_recently_played_tracks(50)),
        "search" => serialize_query(search_music(required_text_argument(argument)?, 50)),
        "artist" => with_item_reference(argument, |item| {
            serialize_query(get_artist(&item.item_id, &item.provider))
        }),
        "album" => with_item_reference(argument, |item| {
            serialize_query(get_album(&item.item_id, &item.provider))
        }),
        "albumTracks" => with_item_reference(argument, |item| {
            serialize_query(get_album_tracks(&item.item_id, &item.provider))
        }),
        "playlist" => with_item_reference(argument, |item| {
            serialize_query(get_playlist(&item.item_id, &item.provider))
        }),
        "playlistTracks" => with_item_reference(argument, |item| {
            serialize_query(get_playlist_tracks(
                &item.item_id,
                &item.provider,
                item.force_refresh,
                item.allow_dynamic_tracks,
            ))
        }),
        "artistAlbums" => with_item_reference(argument, |item| {
            serialize_query(get_artist_albums(&item.item_id, &item.provider))
        }),
        "artistTopTracks" => with_item_reference(argument, |item| {
            serialize_query(get_artist_top_tracks(&item.item_id, &item.provider))
        }),
        "similarArtists" => with_item_reference(argument, |item| {
            serialize_query(get_similar_artists(
                &item.item_id,
                &item.provider,
                item.limit,
            ))
        }),
        "similarTracks" => with_item_reference(argument, |item| {
            serialize_query(get_similar_tracks(
                &item.item_id,
                &item.provider,
                item.limit,
                item.allow_lookup,
            ))
        }),
        "trackVersions" => with_item_reference(argument, |item| {
            serialize_query(get_track_versions(&item.item_id, &item.provider))
        }),
        _ => Err(format!("unsupported Android core query '{kind}'")),
    }
}

fn default_query_limit() -> u32 {
    50
}

fn required_text_argument(argument: Option<&str>) -> Result<&str, String> {
    argument.ok_or_else(|| "missing Android core query argument".to_string())
}

fn with_item_reference<T>(
    argument: Option<&str>,
    operation: impl FnOnce(ItemReferenceQuery) -> Result<T, String>,
) -> Result<T, String> {
    let item = serde_json::from_str(required_text_argument(argument)?)
        .map_err(|error| format!("invalid Android item reference query: {error}"))?;
    operation(item)
}

fn serialize_query<T: Serialize>(result: Result<T, CoreError>) -> Result<String, String> {
    result
        .map_err(|error| error.message)
        .and_then(|response| serde_json::to_string(&response).map_err(|error| error.to_string()))
}

pub(crate) fn get_session_settings() -> Result<core::config::SessionSettings, CoreError> {
    core::read_session_settings()
}

// Playback state queries.
pub(crate) fn get_queue() -> Result<Queue, CoreError> {
    core::read_queue()
}

pub(crate) fn get_player_state() -> Result<PlayerState, CoreError> {
    core::read_player_state()
}

pub(crate) fn get_user_data() -> Result<UserDataSnapshot, CoreError> {
    core::read_user_data()
}

// Music Assistant library queries.
pub(crate) fn get_recommendations() -> Result<Vec<MusicAssistantRecommendationFolder>, CoreError> {
    core::read_music_assistant_recommendations()
}

pub(crate) fn get_recently_played_tracks(limit: u32) -> Result<Vec<Track>, CoreError> {
    core::read_music_assistant_recently_played_tracks(limit)
}

pub(crate) fn search_music(
    query: &str,
    limit: u32,
) -> Result<MusicAssistantSearchResults, CoreError> {
    core::search_music_assistant(query, limit)
}

pub(crate) fn get_provider_manifests() -> Result<Vec<ProviderManifest>, CoreError> {
    core::read_music_assistant_provider_manifests()
}

pub(crate) fn get_provider_icon_data(provider: &str) -> Result<Option<String>, CoreError> {
    core::read_music_assistant_provider_icon_data(provider)
}

pub(crate) fn get_artist(item_id: &str, provider: &str) -> Result<Artist, CoreError> {
    core::read_music_assistant_artist(item_id, provider)
}

pub(crate) fn get_album(item_id: &str, provider: &str) -> Result<Album, CoreError> {
    core::read_music_assistant_album(item_id, provider)
}

pub(crate) fn get_album_tracks(item_id: &str, provider: &str) -> Result<Vec<Track>, CoreError> {
    core::read_music_assistant_album_tracks(item_id, provider)
}

pub(crate) fn get_playlist(item_id: &str, provider: &str) -> Result<Playlist, CoreError> {
    core::read_music_assistant_playlist(item_id, provider)
}

pub(crate) fn get_playlist_tracks(
    item_id: &str,
    provider: &str,
    force_refresh: bool,
    allow_dynamic_tracks: bool,
) -> Result<Vec<Track>, CoreError> {
    core::read_music_assistant_playlist_tracks(
        item_id,
        provider,
        force_refresh,
        allow_dynamic_tracks,
    )
}

pub(crate) fn get_artist_albums(item_id: &str, provider: &str) -> Result<Vec<Album>, CoreError> {
    core::read_music_assistant_artist_albums(item_id, provider)
}

pub(crate) fn get_artist_top_tracks(
    item_id: &str,
    provider: &str,
) -> Result<Vec<Track>, CoreError> {
    core::read_music_assistant_artist_top_tracks(item_id, provider)
}

pub(crate) fn get_similar_artists(
    item_id: &str,
    provider: &str,
    limit: u32,
) -> Result<Vec<Artist>, CoreError> {
    core::read_music_assistant_similar_artists(item_id, provider, limit)
}

pub(crate) fn get_similar_tracks(
    item_id: &str,
    provider: &str,
    limit: u32,
    allow_lookup: bool,
) -> Result<Vec<Track>, CoreError> {
    core::read_music_assistant_similar_tracks(item_id, provider, limit, allow_lookup)
}

pub(crate) fn get_track_versions(item_id: &str, provider: &str) -> Result<Vec<Track>, CoreError> {
    core::read_music_assistant_track_versions(item_id, provider)
}
