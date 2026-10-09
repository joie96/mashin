//! Compact, display-ready user preferences; domain media remain full Rust models.

use super::models::identifiers::{MediaId, MediaUri, ProviderInstanceId};
use super::models::media::{
    Album, AlbumType, Artist, MediaItemFields, MediaItemImage, MediaItemMetadata, Playlist, Track,
};
use super::user_data::{Favorites, LocalPlaylist, UserDataSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredUserData {
    tracks: HashMap<String, StoredTrack>,
    albums: HashMap<String, StoredAlbum>,
    artists: HashMap<String, StoredArtist>,
    playlists: HashMap<String, StoredPlaylist>,
    favorites: StoredFavorites,
    local_playlists: Vec<StoredLocalPlaylist>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMedia {
    item_id: String,
    provider: String,
    name: String,
    uri: Option<String>,
    sort_name: Option<String>,
    image: Option<MediaItemImage>,
    cache_checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredTrack {
    #[serde(flatten)]
    media: StoredMedia,
    duration: u32,
    disc_number: u16,
    track_number: u16,
    artists: Vec<String>,
    album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAlbum {
    #[serde(flatten)]
    media: StoredMedia,
    year: Option<i32>,
    album_type: AlbumType,
    artists: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredArtist {
    #[serde(flatten)]
    media: StoredMedia,
    artist_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPlaylist {
    #[serde(flatten)]
    media: StoredMedia,
    owner: String,
    is_editable: bool,
    is_dynamic: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredFavorites {
    tracks: Vec<String>,
    albums: Vec<String>,
    artists: Vec<String>,
    playlists: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredLocalPlaylist {
    playlist: String,
    items: Vec<String>,
}

fn key(media: &MediaItemFields) -> String {
    format!("{}:{}", media.provider.as_str(), media.item_id.as_str())
}

impl StoredMedia {
    // Capture only fields needed to render cached library media.
    fn from_media(media: &MediaItemFields) -> Self {
        Self {
            item_id: media.item_id.as_str().to_string(),
            provider: media.provider.as_str().to_string(),
            name: media.name.clone(),
            uri: media.uri.as_ref().map(|uri| uri.as_str().to_string()),
            sort_name: media.sort_name.clone(),
            image: media
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.images.first())
                .cloned(),
            cache_checksum: media
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.cache_checksum.clone()),
        }
    }

    fn merge(&mut self, media: &MediaItemFields) {
        if self.name.is_empty() && !media.name.is_empty() {
            self.name.clone_from(&media.name);
        }
        if self.uri.is_none() {
            self.uri = media.uri.as_ref().map(|uri| uri.as_str().to_string());
        }
        if self.sort_name.is_none() {
            self.sort_name.clone_from(&media.sort_name);
        }
        if self.image.is_none() {
            self.image = media
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.images.first())
                .cloned();
        }
        if self.cache_checksum.is_none() {
            self.cache_checksum = media
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.cache_checksum.clone());
        }
    }

    // Rebuild the stable, typed domain shape expected by Dart snapshots.
    fn to_media(&self, favorite: bool) -> MediaItemFields {
        MediaItemFields {
            item_id: MediaId(self.item_id.clone()),
            provider: ProviderInstanceId(self.provider.clone()),
            name: self.name.clone(),
            version: String::new(),
            sort_name: self.sort_name.clone(),
            uri: self.uri.clone().map(MediaUri),
            external_ids: Vec::new(),
            is_playable: true,
            available: None,
            provider_mappings: Vec::new(),
            metadata: self.image.clone().map(|image| MediaItemMetadata {
                images: vec![image],
                cache_checksum: self.cache_checksum.clone(),
                ..MediaItemMetadata::default()
            }),
            favorite,
            position: None,
            date_added: None,
        }
    }
}

impl StoredUserData {
    // Projection and deduplication.
    pub(crate) fn from_snapshot(snapshot: &UserDataSnapshot) -> Self {
        let mut stored = Self {
            tracks: HashMap::new(),
            albums: HashMap::new(),
            artists: HashMap::new(),
            playlists: HashMap::new(),
            favorites: StoredFavorites::default(),
            local_playlists: Vec::new(),
        };
        for track in &snapshot.favorites.tracks {
            let id = stored.add_track(track);
            stored.favorites.tracks.push(id);
        }
        for album in &snapshot.favorites.albums {
            let id = stored.add_album(album);
            stored.favorites.albums.push(id);
        }
        for artist in &snapshot.favorites.artists {
            let id = stored.add_artist(artist);
            stored.favorites.artists.push(id);
        }
        for playlist in &snapshot.favorites.playlists {
            let id = stored.add_playlist(playlist);
            stored.favorites.playlists.push(id);
        }
        for playlist in &snapshot.playlists {
            let playlist_key = stored.add_playlist(&playlist.playlist);
            let items = playlist
                .items
                .iter()
                .map(|track| stored.add_track(track))
                .collect();
            stored.local_playlists.push(StoredLocalPlaylist {
                playlist: playlist_key,
                items,
            });
        }
        stored
    }

    fn add_artist(&mut self, artist: &Artist) -> String {
        let id = key(&artist.media);
        let entry = self
            .artists
            .entry(id.clone())
            .or_insert_with(|| StoredArtist {
                media: StoredMedia::from_media(&artist.media),
                artist_type: artist.artist_type.clone(),
            });
        entry.media.merge(&artist.media);
        if entry.artist_type.is_none() {
            entry.artist_type.clone_from(&artist.artist_type);
        }
        id
    }

    fn add_album(&mut self, album: &Album) -> String {
        let id = key(&album.media);
        let artists: Vec<_> = album
            .artists
            .iter()
            .map(|artist| self.add_artist(artist))
            .collect();
        if !self.albums.contains_key(&id) {
            self.albums.insert(
                id.clone(),
                StoredAlbum {
                    media: StoredMedia::from_media(&album.media),
                    year: album.year,
                    album_type: album.album_type,
                    artists,
                },
            );
        } else if let Some(entry) = self.albums.get_mut(&id) {
            entry.media.merge(&album.media);
            if entry.year.is_none() {
                entry.year = album.year;
            }
            if entry.artists.is_empty() {
                entry.artists = artists;
            }
        }
        id
    }

    fn add_track(&mut self, track: &Track) -> String {
        let id = key(&track.media);
        let artists: Vec<_> = track
            .artists
            .iter()
            .map(|artist| self.add_artist(artist))
            .collect();
        let album = track.album.as_ref().map(|album| self.add_album(album));
        if !self.tracks.contains_key(&id) {
            let mut media = StoredMedia::from_media(&track.media);
            media.image = track.image.clone().or(media.image);
            self.tracks.insert(
                id.clone(),
                StoredTrack {
                    media,
                    duration: track.duration,
                    disc_number: track.disc_number,
                    track_number: track.track_number,
                    artists,
                    album,
                },
            );
        } else if let Some(entry) = self.tracks.get_mut(&id) {
            if entry.media.image.is_none() {
                entry.media.image.clone_from(&track.image);
            }
            entry.media.merge(&track.media);
            if entry.duration == 0 {
                entry.duration = track.duration;
            }
            if entry.artists.is_empty() {
                entry.artists = artists;
            }
            if entry.album.is_none() {
                entry.album = album;
            }
        }
        id
    }

    fn add_playlist(&mut self, playlist: &Playlist) -> String {
        let id = key(&playlist.media);
        let entry = self
            .playlists
            .entry(id.clone())
            .or_insert_with(|| StoredPlaylist {
                media: StoredMedia::from_media(&playlist.media),
                owner: playlist.owner.clone(),
                is_editable: playlist.is_editable,
                is_dynamic: playlist.is_dynamic,
            });
        entry.media.merge(&playlist.media);
        id
    }

    // Full snapshots for the active application model.
    fn artist(&self, id: &str) -> Result<Artist, String> {
        let artist = self
            .artists
            .get(id)
            .ok_or_else(|| format!("missing artist {id}"))?;
        Ok(Artist {
            media: artist.media.to_media(false),
            artist_type: artist.artist_type.clone(),
        })
    }

    fn album(&self, id: &str, favorite: bool) -> Result<Album, String> {
        let album = self
            .albums
            .get(id)
            .ok_or_else(|| format!("missing album {id}"))?;
        Ok(Album {
            media: album.media.to_media(favorite),
            year: album.year,
            album_type: album.album_type,
            artists: album
                .artists
                .iter()
                .map(|id| self.artist(id))
                .collect::<Result<_, _>>()?,
        })
    }

    fn track(&self, id: &str, favorite: bool) -> Result<Track, String> {
        let track = self
            .tracks
            .get(id)
            .ok_or_else(|| format!("missing track {id}"))?;
        Ok(Track {
            media: track.media.to_media(favorite),
            duration: track.duration,
            disc_number: track.disc_number,
            track_number: track.track_number,
            artists: track
                .artists
                .iter()
                .map(|id| self.artist(id))
                .collect::<Result<_, _>>()?,
            album: track
                .album
                .as_deref()
                .map(|id| self.album(id, false))
                .transpose()?,
            image: track.media.image.clone(),
        })
    }

    fn playlist(&self, id: &str, favorite: bool) -> Result<Playlist, String> {
        let playlist = self
            .playlists
            .get(id)
            .ok_or_else(|| format!("missing playlist {id}"))?;
        Ok(Playlist {
            media: playlist.media.to_media(favorite),
            owner: playlist.owner.clone(),
            is_editable: playlist.is_editable,
            is_dynamic: playlist.is_dynamic,
        })
    }

    pub(crate) fn to_snapshot(&self) -> Result<UserDataSnapshot, String> {
        Ok(UserDataSnapshot {
            favorites: Favorites {
                tracks: self
                    .favorites
                    .tracks
                    .iter()
                    .map(|id| self.track(id, true))
                    .collect::<Result<_, _>>()?,
                albums: self
                    .favorites
                    .albums
                    .iter()
                    .map(|id| self.album(id, true))
                    .collect::<Result<_, _>>()?,
                artists: self
                    .favorites
                    .artists
                    .iter()
                    .map(|id| {
                        let mut artist = self.artist(id)?;
                        artist.media.favorite = true;
                        Ok(artist)
                    })
                    .collect::<Result<_, String>>()?,
                playlists: self
                    .favorites
                    .playlists
                    .iter()
                    .map(|id| self.playlist(id, true))
                    .collect::<Result<_, _>>()?,
            },
            playlists: self
                .local_playlists
                .iter()
                .map(|playlist| {
                    Ok(LocalPlaylist {
                        playlist: self.playlist(&playlist.playlist, false)?,
                        items: playlist
                            .items
                            .iter()
                            .map(|id| self.track(id, false))
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::StoredUserData;
    use crate::music_assistant::models::media::{Album, Artist, Playlist, Track};
    use crate::music_assistant::user_data::{Favorites, LocalPlaylist, UserDataSnapshot};

    #[test]
    fn track_artwork_uses_one_version_four_field_and_survives_round_trip() {
        let metadata_image = serde_json::json!({
            "type": "thumb", "path": "https://yt3.googleusercontent.com/cover",
            "provider": "ytmusic--test", "remotely_accessible": true,
            "proxy_id": "youtube-cover"
        });
        let direct_image = serde_json::json!({
            "type": "thumb", "path": "https://i.ytimg.com/track.jpg",
            "provider": "url", "remotely_accessible": true,
            "proxy_id": "direct-cover"
        });
        for (metadata, direct, expected) in [
            (Some(metadata_image.clone()), None, Some(metadata_image.clone())),
            (None, Some(direct_image.clone()), Some(direct_image.clone())),
            (Some(metadata_image), Some(direct_image.clone()), Some(direct_image)),
            (None, None, None),
        ] {
            let track: Track = serde_json::from_value(serde_json::json!({
                "item_id": "video", "provider": "ytmusic--test", "name": "Track",
                "uri": "ytmusic--test://track/video",
                "metadata": { "images": metadata.into_iter().collect::<Vec<_>>() },
                "image": direct
            }))
            .unwrap();
            let snapshot = UserDataSnapshot {
                favorites: Favorites { tracks: vec![track], ..Favorites::default() },
                playlists: Vec::new(),
            };
            let stored = StoredUserData::from_snapshot(&snapshot);
            let encoded = serde_json::to_string(&stored).unwrap();
            assert_eq!(encoded.matches("\"image\":").count(), 1);
            let json = serde_json::to_value(&stored).unwrap();
            assert_eq!(json["tracks"]["ytmusic--test:video"]["image"], serde_json::json!(expected));
            let restored = serde_json::from_str::<StoredUserData>(&encoded)
                .unwrap()
                .to_snapshot()
                .unwrap();
            let restored_track = &restored.favorites.tracks[0];
            assert_eq!(serde_json::to_value(&restored_track.image).unwrap(), serde_json::json!(expected));
            assert_eq!(
                restored_track.media.metadata.as_ref().and_then(|metadata| metadata.images.first()),
                restored_track.image.as_ref()
            );
        }
    }

    #[test]
    fn existing_version_four_track_image_is_read_without_a_schema_change() {
        for image in [serde_json::json!({
            "type": "thumb", "path": "cover.jpg", "provider": "library",
            "remotely_accessible": false, "proxy_id": "existing-cover"
        }), serde_json::Value::Null] {
            let stored: StoredUserData = serde_json::from_value(serde_json::json!({
                "tracks": { "library:track": {
                    "item_id": "track", "provider": "library", "name": "Track",
                    "uri": "library://track/track", "sort_name": null,
                    "image": image, "cache_checksum": null,
                    "duration": 210, "disc_number": 1, "track_number": 2,
                    "artists": [], "album": null
                } },
                "albums": {}, "artists": {}, "playlists": {},
                "favorites": { "tracks": ["library:track"], "albums": [], "artists": [], "playlists": [] },
                "local_playlists": []
            }))
            .unwrap();
            let restored = stored.to_snapshot().unwrap();
            assert_eq!(serde_json::to_value(&restored.favorites.tracks[0].image).unwrap(), image);
            assert!(restored.favorites.tracks[0].media.favorite);
        }
    }

    #[test]
    fn repeated_tracks_keep_order_and_display_details_without_duplicating_metadata() {
        let artist: Artist = serde_json::from_value(serde_json::json!({
            "item_id": "artist", "provider": "library", "name": "Artist",
            "metadata": { "description": "large biography" }
        }))
        .unwrap();
        let album: Album = serde_json::from_value(serde_json::json!({
            "item_id": "album", "provider": "library", "name": "Album", "year": 2014,
            "artists": [{ "item_id": "artist", "provider": "library", "name": "Artist" }]
        }))
        .unwrap();
        let track: Track = serde_json::from_value(serde_json::json!({
            "item_id": "track", "provider": "library", "name": "Track", "duration": 210,
            "metadata": { "lyrics": "long lyrics" }
        }))
        .unwrap();
        let mut track = track;
        track.artists = vec![artist];
        track.album = Some(album);
        let playlist: Playlist = serde_json::from_value(serde_json::json!({
            "item_id": "1", "provider": "mashin", "name": "Local"
        }))
        .unwrap();
        let snapshot = UserDataSnapshot {
            favorites: Favorites {
                tracks: vec![track.clone()],
                ..Favorites::default()
            },
            playlists: vec![LocalPlaylist {
                playlist,
                items: vec![track.clone(), track],
            }],
        };
        let stored = StoredUserData::from_snapshot(&snapshot);
        let json = serde_json::to_value(&stored).unwrap();
        assert_eq!(json["tracks"].as_object().unwrap().len(), 1);
        assert_eq!(json["albums"].as_object().unwrap().len(), 1);
        assert_eq!(json["artists"].as_object().unwrap().len(), 1);
        assert!(!json.to_string().contains("long lyrics"));
        assert!(!json.to_string().contains("large biography"));
        let restored = stored.to_snapshot().unwrap();
        assert_eq!(restored.playlists[0].items.len(), 2);
        assert_eq!(
            restored.playlists[0].items[0].album.as_ref().unwrap().year,
            Some(2014)
        );
        assert_eq!(
            restored.playlists[0].items[0].artists[0].media.name,
            "Artist"
        );
    }

    #[test]
    fn sparse_favorite_does_not_hide_richer_local_playlist_details() {
        let sparse: Track = serde_json::from_value(serde_json::json!({
            "item_id": "t", "provider": "library", "name": "Track"
        }))
        .unwrap();
        let rich: Track = serde_json::from_value(serde_json::json!({
            "item_id": "t", "provider": "library", "name": "Track", "duration": 233,
            "album": { "item_id": "a", "provider": "library", "name": "Album", "year": 2018 },
            "artists": [{ "item_id": "r", "provider": "library", "name": "Singer" }]
        }))
        .unwrap();
        let playlist: Playlist = serde_json::from_value(serde_json::json!({
            "item_id": "1", "provider": "mashin", "name": "Local"
        }))
        .unwrap();
        let snapshot = UserDataSnapshot {
            favorites: Favorites {
                tracks: vec![sparse],
                ..Favorites::default()
            },
            playlists: vec![LocalPlaylist {
                playlist,
                items: vec![rich],
            }],
        };
        let restored = StoredUserData::from_snapshot(&snapshot)
            .to_snapshot()
            .unwrap();
        assert_eq!(restored.favorites.tracks[0].duration, 233);
        assert_eq!(
            restored.playlists[0].items[0].album.as_ref().unwrap().year,
            Some(2018)
        );
        assert_eq!(
            restored.playlists[0].items[0].artists[0].media.name,
            "Singer"
        );
    }
}
