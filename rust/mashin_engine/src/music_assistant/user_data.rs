//! Canonical Mashin favorites and local playlists persisted in Music Assistant user preferences.

use super::client::MusicAssistantClient;
use super::models::identifiers::{MediaId, MediaUri, ProviderInstanceId};
use super::models::media::{
    Album, Artist, MediaItem, MediaItemFields, MediaItemImage, MediaItemMetadata, Playlist, Track,
};
use super::models::session::MusicAssistantUser;
use super::user_data_store::StoredUserData;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::{Duration, Instant};

const VERSION_PREFERENCE_KEY: &str = "mashin.user_data_version";
const USER_DATA_PREFERENCE_KEY: &str = "mashin.user_data";
const USER_DATA_VERSION: u64 = 4;
pub(crate) const LOCAL_PLAYLIST_PROVIDER: &str = "mashin";
pub(crate) const WRITE_DEBOUNCE: Duration = Duration::from_secs(2);

pub(crate) struct UserDataWriteRequest {
    pub(crate) revision: u64,
    pub(crate) username: String,
    pub(crate) preferences: Map<String, Value>,
    pub(crate) client: MusicAssistantClient,
}

pub(crate) async fn persist_write_request(
    request: UserDataWriteRequest,
) -> Result<MusicAssistantUser, String> {
    request
        .client
        .update_user(serde_json::json!({
            "username": request.username,
            "preferences": request.preferences,
        }))
        .await
}

/// Runs one coalescing write worker for a session-owned UserData instance.
pub(crate) async fn flush_scheduled_write_after_debounce<ReadDelay, ReadRequest, FinishWrite>(
    mut read_delay: ReadDelay,
    mut read_request: ReadRequest,
    mut finish_write: FinishWrite,
) where
    ReadDelay: FnMut() -> Option<Duration>,
    ReadRequest: FnMut() -> Option<UserDataWriteRequest>,
    FinishWrite: FnMut(u64, Result<MusicAssistantUser, String>) -> bool,
{
    loop {
        let Some(delay) = read_delay() else {
            return;
        };
        tokio::time::sleep(delay).await;

        let Some(remaining_delay) = read_delay() else {
            return;
        };
        if !remaining_delay.is_zero() {
            continue;
        }

        let Some(request) = read_request() else {
            return;
        };
        let revision = request.revision;
        let response = persist_write_request(request).await;
        if finish_write(revision, response) {
            return;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Favorites {
    #[serde(default)]
    pub(crate) tracks: Vec<Track>,
    #[serde(default)]
    pub(crate) albums: Vec<Album>,
    #[serde(default)]
    pub(crate) playlists: Vec<Playlist>,
    #[serde(default)]
    pub(crate) artists: Vec<Artist>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct LocalPlaylist {
    #[serde(flatten)]
    pub(crate) playlist: Playlist,
    #[serde(default)]
    pub(crate) items: Vec<Track>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct UserDataSnapshot {
    #[serde(default)]
    pub(crate) favorites: Favorites,
    #[serde(default)]
    pub(crate) playlists: Vec<LocalPlaylist>,
}

#[derive(Debug, Clone)]
pub(crate) struct UserDataController {
    additional_preferences: Map<String, Value>,
    snapshot: UserDataSnapshot,
    mutation_revision: u64,
    last_mutated_at: Option<Instant>,
    write_is_scheduled: bool,
}

impl UserDataController {
    // Construction and snapshot reads.
    pub(crate) fn from_preferences(mut preferences: Map<String, Value>) -> Result<Self, String> {
        let version = preferences.remove(VERSION_PREFERENCE_KEY);
        let stored_data = preferences.remove(USER_DATA_PREFERENCE_KEY);
        if preferences.contains_key("mashin.favorites")
            || preferences.contains_key("mashin.playlists")
        {
            return Err("unsupported legacy user data preferences".to_string());
        }
        let snapshot = match (version, stored_data) {
            (None, None) => UserDataSnapshot::default(),
            (Some(version), stored_data) => {
                let version = version.as_u64().ok_or("invalid user data version")?;
                if version != USER_DATA_VERSION {
                    return Err(format!("unsupported user data version {version}"));
                }
                let stored_data = stored_data.ok_or("missing version four user data")?;
                serde_json::from_value::<StoredUserData>(stored_data)
                    .map_err(|error| format!("decode version four user data failed: {error}"))?
                    .to_snapshot()?
            }
            (None, Some(_)) => return Err("missing user data version".to_string()),
        };
        Ok(Self {
            additional_preferences: preferences,
            snapshot,
            mutation_revision: 0,
            last_mutated_at: None,
            write_is_scheduled: false,
        })
    }

    pub(crate) fn snapshot(&self) -> UserDataSnapshot {
        self.snapshot.clone()
    }

    pub(crate) fn preferences(&self) -> Result<Map<String, Value>, String> {
        let mut preferences = self.additional_preferences.clone();
        preferences.insert(
            VERSION_PREFERENCE_KEY.to_string(),
            Value::from(USER_DATA_VERSION),
        );
        preferences.insert(
            USER_DATA_PREFERENCE_KEY.to_string(),
            serde_json::to_value(StoredUserData::from_snapshot(&self.snapshot))
                .map_err(|error| format!("serialize user data failed: {error}"))?,
        );
        Ok(preferences)
    }

    // Delayed preference persistence.
    pub(crate) fn schedule_write(&mut self) -> bool {
        self.mutation_revision = self.mutation_revision.wrapping_add(1);
        self.last_mutated_at = Some(Instant::now());
        let should_schedule = !self.write_is_scheduled;
        self.write_is_scheduled = true;
        should_schedule
    }

    pub(crate) fn write_delay(&self, now: Instant) -> Option<Duration> {
        self.write_is_scheduled.then(|| {
            WRITE_DEBOUNCE
                .saturating_sub(now.saturating_duration_since(self.last_mutated_at.unwrap_or(now)))
        })
    }

    pub(crate) fn pending_write(&self) -> Option<(u64, Map<String, Value>)> {
        self.write_is_scheduled
            .then(|| {
                self.preferences()
                    .ok()
                    .map(|preferences| (self.mutation_revision, preferences))
            })
            .flatten()
    }

    pub(crate) fn finish_write(
        &mut self,
        written_revision: u64,
        preferences: Option<Map<String, Value>>,
    ) -> bool {
        if self.mutation_revision != written_revision {
            return false;
        }
        self.write_is_scheduled = false;
        if let Some(preferences) = preferences {
            if let Ok(reloaded) = Self::from_preferences(preferences) {
                self.additional_preferences = reloaded.additional_preferences;
                self.snapshot = reloaded.snapshot;
            }
        }
        true
    }

    // Favorite and local-playlist commands.
    pub(crate) fn set_favorites(&mut self, media_items: Vec<MediaItem>, is_favorite: bool) {
        for media_item in media_items {
            match media_item {
                MediaItem::Track(track) => {
                    update_favorite(&mut self.snapshot.favorites.tracks, track, is_favorite)
                }
                MediaItem::Album(album) => {
                    update_favorite(&mut self.snapshot.favorites.albums, album, is_favorite)
                }
                MediaItem::Playlist(playlist) => update_favorite(
                    &mut self.snapshot.favorites.playlists,
                    playlist,
                    is_favorite,
                ),
                MediaItem::Artist(artist) => {
                    update_favorite(&mut self.snapshot.favorites.artists, artist, is_favorite)
                }
                _ => {}
            }
        }
    }

    pub(crate) fn create_local_playlist(&mut self, name: String) -> Result<Playlist, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("local playlist name is required".to_string());
        }
        let playlist = Playlist {
            media: MediaItemFields {
                item_id: MediaId(self.next_local_playlist_id()),
                provider: ProviderInstanceId(LOCAL_PLAYLIST_PROVIDER.to_string()),
                name: name.to_string(),
                sort_name: Some(name.to_string()),
                uri: Some(MediaUri(format!("mashin://playlist/{}", name))),
                is_playable: true,
                ..default_media_fields()
            },
            owner: String::new(),
            is_editable: true,
            is_dynamic: false,
        };
        self.snapshot.playlists.push(LocalPlaylist {
            playlist: playlist.clone(),
            items: Vec::new(),
        });
        self.snapshot.playlists.sort_by(compare_local_playlists);
        Ok(playlist)
    }

    pub(crate) fn rename_local_playlist(
        &mut self,
        playlist_id: &str,
        name: String,
    ) -> Result<(), String> {
        let playlist = self.local_playlist_mut(playlist_id)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("local playlist name is required".to_string());
        }
        playlist.playlist.media.name = name.to_string();
        playlist.playlist.media.sort_name = Some(name.to_string());
        self.snapshot.playlists.sort_by(compare_local_playlists);
        Ok(())
    }

    pub(crate) fn remove_local_playlist(&mut self, playlist_id: &str) -> Result<(), String> {
        let playlist_id = playlist_id.trim();
        let initial_count = self.snapshot.playlists.len();
        self.snapshot.playlists.retain(|playlist| {
            !playlist
                .playlist
                .media
                .item_id
                .as_str()
                .eq_ignore_ascii_case(playlist_id)
        });
        if self.snapshot.playlists.len() == initial_count {
            return Err("local playlist was not found".to_string());
        }
        Ok(())
    }

    pub(crate) fn add_local_playlist_tracks(
        &mut self,
        playlist_id: &str,
        tracks: Vec<Track>,
    ) -> Result<(), String> {
        self.local_playlist_mut(playlist_id)?.items.extend(tracks);
        Ok(())
    }

    pub(crate) fn remove_local_playlist_tracks(
        &mut self,
        playlist_id: &str,
        track_uris: Vec<String>,
    ) -> Result<(), String> {
        let track_uris: Vec<String> = track_uris
            .into_iter()
            .map(|uri| uri.trim().to_ascii_lowercase())
            .filter(|uri| !uri.is_empty())
            .collect();
        if track_uris.is_empty() {
            return Ok(());
        }
        self.local_playlist_mut(playlist_id)?.items.retain(|track| {
            track.media.uri.as_ref().is_none_or(|uri| {
                !track_uris
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(uri.as_str()))
            })
        });
        Ok(())
    }

    pub(crate) fn set_local_playlist_cover(
        &mut self,
        playlist_id: &str,
        data_uri: Option<String>,
        clear_when_empty: bool,
    ) -> Result<(), String> {
        let playlist = self.local_playlist_mut(playlist_id)?;
        if let Some(data_uri) = data_uri {
            let metadata = playlist
                .playlist
                .media
                .metadata
                .get_or_insert_with(MediaItemMetadata::default);
            metadata.images = vec![MediaItemImage {
                image_type: "thumb".to_string(),
                path: data_uri,
                provider: ProviderInstanceId(LOCAL_PLAYLIST_PROVIDER.to_string()),
                remotely_accessible: false,
                proxy_id: None,
            }];
        } else if clear_when_empty {
            if let Some(metadata) = playlist.playlist.media.metadata.as_mut() {
                metadata.images.clear();
            }
        }
        Ok(())
    }

    fn local_playlist_mut(&mut self, playlist_id: &str) -> Result<&mut LocalPlaylist, String> {
        self.snapshot
            .playlists
            .iter_mut()
            .find(|playlist| {
                playlist
                    .playlist
                    .media
                    .item_id
                    .as_str()
                    .eq_ignore_ascii_case(playlist_id.trim())
            })
            .ok_or_else(|| "local playlist was not found".to_string())
    }

    fn next_local_playlist_id(&self) -> String {
        self.snapshot
            .playlists
            .iter()
            .filter_map(|playlist| playlist.playlist.media.item_id.as_str().parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .to_string()
    }
}

fn update_favorite<T: FavoriteMedia>(favorites: &mut Vec<T>, mut item: T, is_favorite: bool) {
    let Some(uri) = item.media_uri() else {
        return;
    };
    if let Some(index) = favorites.iter().position(|favorite| {
        favorite
            .media_uri()
            .is_some_and(|favorite_uri| favorite_uri.eq_ignore_ascii_case(uri))
    }) {
        if !is_favorite {
            favorites.remove(index);
        }
        return;
    }
    if is_favorite {
        item.set_favorite();
        favorites.push(item);
    }
}

trait FavoriteMedia {
    fn media_uri(&self) -> Option<&str>;
    fn set_favorite(&mut self);
}

macro_rules! favorite_media {
    ($type:ty) => {
        impl FavoriteMedia for $type {
            fn media_uri(&self) -> Option<&str> {
                self.media.uri.as_ref().map(|uri| uri.as_str())
            }
            fn set_favorite(&mut self) {
                self.media.favorite = true;
            }
        }
    };
}

favorite_media!(Track);
favorite_media!(Album);
favorite_media!(Playlist);
favorite_media!(Artist);

fn default_media_fields() -> MediaItemFields {
    MediaItemFields {
        item_id: MediaId(String::new()),
        provider: ProviderInstanceId(String::new()),
        name: String::new(),
        version: String::new(),
        sort_name: None,
        uri: None,
        external_ids: Vec::new(),
        is_playable: true,
        provider_mappings: Vec::new(),
        metadata: None,
        favorite: false,
        position: None,
        date_added: None,
    }
}

fn compare_local_playlists(left: &LocalPlaylist, right: &LocalPlaylist) -> std::cmp::Ordering {
    left.playlist
        .media
        .sort_name
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .cmp(
            &right
                .playlist
                .media
                .sort_name
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
        )
        .then_with(|| {
            left.playlist
                .media
                .name
                .to_ascii_lowercase()
                .cmp(&right.playlist.media.name.to_ascii_lowercase())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn track(uri: &str) -> Track {
        serde_json::from_value(serde_json::json!({
            "item_id": "track-id",
            "provider": "library",
            "name": "Track",
            "uri": uri,
        }))
        .expect("test track must deserialize")
    }

    #[test]
    fn write_waits_for_the_full_debounce_after_the_latest_mutation() {
        let mut controller =
            UserDataController::from_preferences(Map::new()).expect("preferences must load");
        let mutation_time = Instant::now();
        controller.schedule_write();
        controller.last_mutated_at = Some(mutation_time);

        assert_eq!(controller.write_delay(mutation_time), Some(WRITE_DEBOUNCE));
        assert_eq!(
            controller.write_delay(mutation_time + Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            controller.write_delay(mutation_time + Duration::from_secs(3)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn preferences_preserve_unrelated_music_assistant_values() {
        let mut preferences = Map::new();
        preferences.insert(
            "music_assistant.theme".to_string(),
            Value::String("dark".to_string()),
        );
        let controller =
            UserDataController::from_preferences(preferences).expect("preferences must load");

        let preferences = controller
            .preferences()
            .expect("preferences must serialize");

        assert_eq!(preferences["music_assistant.theme"], "dark");
        assert_eq!(preferences[VERSION_PREFERENCE_KEY], USER_DATA_VERSION);
        assert!(preferences.contains_key(USER_DATA_PREFERENCE_KEY));
    }

    #[test]
    fn favorite_updates_are_case_insensitive() {
        let mut controller =
            UserDataController::from_preferences(Map::new()).expect("preferences must load");
        let media_item = MediaItem::Track(track("library://track/UPPER"));

        controller.set_favorites(vec![media_item], true);
        controller.set_favorites(
            vec![MediaItem::Track(track("LIBRARY://TRACK/upper"))],
            false,
        );

        assert!(controller.snapshot().favorites.tracks.is_empty());
    }

    #[test]
    fn playlist_track_removal_is_case_insensitive() {
        let mut controller =
            UserDataController::from_preferences(Map::new()).expect("preferences must load");
        let playlist = controller
            .create_local_playlist("Local".to_string())
            .expect("playlist must be created");
        controller
            .add_local_playlist_tracks(
                playlist.media.item_id.as_str(),
                vec![track("library://track/UPPER")],
            )
            .expect("track must be added");

        controller
            .remove_local_playlist_tracks(
                playlist.media.item_id.as_str(),
                vec!["LIBRARY://TRACK/upper".to_string()],
            )
            .expect("track must be removed");

        assert!(controller.snapshot().playlists[0].items.is_empty());
    }

    #[test]
    fn preferences_store_compact_tracks_with_nested_display_media() {
        let mut controller =
            UserDataController::from_preferences(Map::new()).expect("preferences must load");
        let track: Track = serde_json::from_value(serde_json::json!({
            "media_type": "track",
            "item_id": "track-id",
            "provider": "library",
            "name": "Track",
            "uri": "library://track/1",
            "favorite": true,
            "image": { "type": "thumb", "path": "track.jpg", "provider": "library", "proxy_id": "track-image" },
            "artists": [{
                "media_type": "artist",
                "item_id": "artist-id",
                "provider": "library",
                "name": "Artist",
                "image": { "type": "thumb", "path": "artist.jpg", "provider": "library", "proxy_id": "artist-image" }
            }],
            "album": {
                "media_type": "album",
                "item_id": "album-id",
                "provider": "library",
                "name": "Album",
                "image": { "type": "thumb", "path": "album.jpg", "provider": "library", "proxy_id": "album-image" }
            }
        }))
        .expect("test track must deserialize");

        controller.set_favorites(vec![MediaItem::Track(track.clone())], true);
        let playlist = controller
            .create_local_playlist("Local".to_string())
            .expect("playlist must be created");
        controller
            .add_local_playlist_tracks(playlist.media.item_id.as_str(), vec![track])
            .expect("track must be added");

        let preferences = controller
            .preferences()
            .expect("preferences must serialize");
        assert_eq!(preferences[VERSION_PREFERENCE_KEY], USER_DATA_VERSION);
        let data = &preferences[USER_DATA_PREFERENCE_KEY];
        assert_eq!(data["tracks"].as_object().unwrap().len(), 1);
        assert_eq!(data["albums"].as_object().unwrap().len(), 1);
        assert_eq!(data["artists"].as_object().unwrap().len(), 1);
        assert!(!preferences.contains_key("mashin.favorites"));
        assert!(!preferences.contains_key("mashin.playlists"));

        let restored = UserDataController::from_preferences(preferences)
            .expect("compact preferences must deserialize");
        let snapshot = restored.snapshot();
        assert_eq!(
            snapshot.favorites.tracks[0]
                .image
                .as_ref()
                .unwrap()
                .proxy_id
                .as_deref(),
            Some("track-image")
        );
        assert_eq!(
            snapshot.playlists[0].items[0].artists[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images[0]
                .proxy_id
                .as_deref(),
            Some("artist-image")
        );
        assert_eq!(
            snapshot.playlists[0].items[0]
                .album
                .as_ref()
                .unwrap()
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images[0]
                .proxy_id
                .as_deref(),
            Some("album-image")
        );
    }

    #[test]
    fn version_four_playlist_jpeg_survives_roundtrip() {
        let mut controller = UserDataController::from_preferences(Map::new()).unwrap();
        let playlist = controller
            .create_local_playlist("Local".to_string())
            .unwrap();
        let id = playlist.media.item_id.as_str();
        controller
            .add_local_playlist_tracks(id, vec![track("library://track/1")])
            .unwrap();
        controller
            .set_local_playlist_cover(id, Some("data:image/jpeg;base64,/9j/".to_string()), false)
            .unwrap();

        let restored =
            UserDataController::from_preferences(controller.preferences().unwrap()).unwrap();
        assert_eq!(
            restored.snapshot().playlists[0]
                .playlist
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images[0]
                .path,
            "data:image/jpeg;base64,/9j/"
        );
        assert_eq!(restored.snapshot().playlists[0].items.len(), 1);
    }

    #[test]
    fn unsupported_or_incomplete_user_data_is_not_overwritten() {
        let mut preferences = Map::new();
        preferences.insert(VERSION_PREFERENCE_KEY.into(), Value::from(3));
        assert!(UserDataController::from_preferences(preferences)
            .unwrap_err()
            .contains("unsupported user data version 3"));

        let mut preferences = Map::new();
        preferences.insert("mashin.favorites".into(), serde_json::json!({}));
        assert!(UserDataController::from_preferences(preferences)
            .unwrap_err()
            .contains("unsupported legacy user data"));

        let mut preferences = Map::new();
        preferences.insert(
            VERSION_PREFERENCE_KEY.into(),
            Value::from(USER_DATA_VERSION),
        );
        assert!(UserDataController::from_preferences(preferences)
            .unwrap_err()
            .contains("missing version four user data"));

        let mut preferences = Map::new();
        preferences.insert(USER_DATA_PREFERENCE_KEY.into(), serde_json::json!({}));
        assert!(UserDataController::from_preferences(preferences)
            .unwrap_err()
            .contains("missing user data version"));
    }

    #[test]
    fn removing_last_cover_source_clears_playlist_image_in_version_four() {
        let mut controller = UserDataController::from_preferences(Map::new()).unwrap();
        let playlist = controller
            .create_local_playlist("Local".to_string())
            .unwrap();
        let id = playlist.media.item_id.as_str();
        controller
            .add_local_playlist_tracks(id, vec![track("library://track/1")])
            .unwrap();
        controller
            .set_local_playlist_cover(id, Some("data:image/jpeg;base64,/9j/".to_string()), false)
            .unwrap();
        assert_eq!(
            controller.snapshot().playlists[0]
                .playlist
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images
                .len(),
            1
        );
        controller
            .remove_local_playlist_tracks(id, vec!["library://track/1".to_string()])
            .unwrap();
        controller.set_local_playlist_cover(id, None, true).unwrap();
        let restored =
            UserDataController::from_preferences(controller.preferences().unwrap()).unwrap();
        assert!(restored.snapshot().playlists[0].items.is_empty());
        assert!(restored.snapshot().playlists[0]
            .playlist
            .media
            .metadata
            .as_ref()
            .is_none_or(|metadata| metadata.images.is_empty()));
    }
}
