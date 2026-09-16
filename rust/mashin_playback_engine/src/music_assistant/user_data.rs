//! Canonical Mashin favorites and local playlists persisted in Music Assistant user preferences.

use super::client::MusicAssistantClient;
use super::models::identifiers::{MediaId, MediaUri, ProviderInstanceId};
use super::models::media::{Album, Artist, MediaItem, MediaItemFields, Playlist, Track};
use super::models::session::MusicAssistantUser;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::{Duration, Instant};

const FAVORITES_PREFERENCE_KEY: &str = "mashin.favorites";
const PLAYLISTS_PREFERENCE_KEY: &str = "mashin.playlists";
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
        let favorites = parse_preference(&mut preferences, FAVORITES_PREFERENCE_KEY)?;
        let mut playlists: Vec<LocalPlaylist> = parse_playlist_preference(&mut preferences)?;
        playlists.sort_by(compare_local_playlists);
        Ok(Self {
            additional_preferences: preferences,
            snapshot: UserDataSnapshot {
                favorites,
                playlists,
            },
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
            FAVORITES_PREFERENCE_KEY.to_string(),
            serialize_reduced_music_assistant_value(&self.snapshot.favorites)?,
        );
        preferences.insert(
            PLAYLISTS_PREFERENCE_KEY.to_string(),
            serde_json::json!({
                "playlists": serialize_reduced_music_assistant_value(&self.snapshot.playlists)?,
            }),
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

fn parse_preference<T: for<'de> Deserialize<'de> + Default>(
    preferences: &mut Map<String, Value>,
    key: &str,
) -> Result<T, String> {
    let value = preferences
        .remove(key)
        .unwrap_or_else(|| serde_json::json!({}));
    serde_json::from_value(value)
        .map_err(|error| format!("decode {key} preferences failed: {error}"))
}

fn parse_playlist_preference(
    preferences: &mut Map<String, Value>,
) -> Result<Vec<LocalPlaylist>, String> {
    let value = preferences
        .remove(PLAYLISTS_PREFERENCE_KEY)
        .unwrap_or_else(|| serde_json::json!({}));
    #[derive(Deserialize, Default)]
    struct PlaylistPreference {
        #[serde(default)]
        playlists: Vec<LocalPlaylist>,
    }
    serde_json::from_value::<PlaylistPreference>(value)
        .map(|value| value.playlists)
        .map_err(|error| format!("decode local playlist preferences failed: {error}"))
}

// Converts typed Music Assistant models to compact preference data without local UI state.
fn serialize_reduced_music_assistant_value<T: Serialize>(value: &T) -> Result<Value, String> {
    let mut value = serde_json::to_value(value)
        .map_err(|error| format!("serialize Music Assistant preference failed: {error}"))?;
    reduce_music_assistant_value(&mut value);
    Ok(value)
}

fn reduce_music_assistant_value(value: &mut Value) {
    const DEFAULT_TRUE_FIELDS: &[&str] = &["available", "is_playable"];
    const DEFAULT_FALSE_FIELDS: &[&str] = &["is_dynamic", "is_editable", "remotely_accessible"];
    const DEFAULT_ZERO_FIELDS: &[&str] = &["disc_number", "duration", "track_number"];

    match value {
        Value::Array(values) => values.iter_mut().for_each(reduce_music_assistant_value),
        Value::Object(values) => {
            values.values_mut().for_each(reduce_music_assistant_value);
            values.retain(|key, value| {
                if key == "favorite"
                    || value.is_null()
                    || value.as_array().is_some_and(Vec::is_empty)
                {
                    return false;
                }
                if DEFAULT_TRUE_FIELDS.contains(&key.as_str()) && value == &Value::Bool(true) {
                    return false;
                }
                if DEFAULT_FALSE_FIELDS.contains(&key.as_str()) && value == &Value::Bool(false) {
                    return false;
                }
                !DEFAULT_ZERO_FIELDS.contains(&key.as_str()) || value.as_u64() != Some(0)
            });
        }
        _ => {}
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
        assert!(preferences.contains_key(FAVORITES_PREFERENCE_KEY));
        assert!(preferences.contains_key(PLAYLISTS_PREFERENCE_KEY));
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
    fn preferences_store_compact_tracks_with_images_and_references() {
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
        let favorite_track = &preferences[FAVORITES_PREFERENCE_KEY]["tracks"][0];
        let playlist_track = &preferences[PLAYLISTS_PREFERENCE_KEY]["playlists"][0]["items"][0];

        for stored_track in [favorite_track, playlist_track] {
            assert!(stored_track.get("favorite").is_none());
            assert_eq!(stored_track["image"]["proxy_id"], "track-image");
            assert_eq!(
                stored_track["artists"][0]["image"]["proxy_id"],
                "artist-image"
            );
            assert_eq!(stored_track["album"]["image"]["proxy_id"], "album-image");
            assert!(stored_track.get("duration").is_none());
        }
    }
}
