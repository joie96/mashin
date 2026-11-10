//! Library read projections and pure replacement rules; no storage or network ownership.

use super::favorites::{has_same_media_identity, FavoriteOverride};
use super::media::{Album, Artist, FieldKnowledge, MediaItem, Playlist, Track};
use serde::{Deserialize, Serialize};

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
pub(crate) struct PrivatePlaylist {
    #[serde(flatten)]
    pub(crate) playlist: Playlist,
    #[serde(default)]
    pub(crate) items: Vec<Track>,
    #[serde(default)]
    pub(crate) is_loading_tracks: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct UserDataSnapshot {
    /// Orders session-local UI publications, not server data or persisted cache entries.
    #[serde(default)]
    pub(crate) revision: u64,
    #[serde(default)]
    pub(crate) favorites: Favorites,
    #[serde(default)]
    pub(crate) playlists: Vec<PrivatePlaylist>,
    #[serde(default)]
    pub(crate) is_loading_favorites: bool,
    #[serde(default)]
    pub(crate) is_loading_playlists: bool,
    #[serde(default)]
    pub(crate) has_loaded_favorites: bool,
    #[serde(default)]
    pub(crate) favorite_overrides: Vec<FavoriteOverride>,
}

impl Favorites {
    // Favorite projection updates never modify the caller's media values.
    pub(crate) fn set_item(&mut self, item: MediaItem, is_favorite: bool) {
        macro_rules! update {
            ($items:expr, $variant:ident, $value:expr) => {{
                $items.retain(|existing| {
                    !has_same_media_identity(&MediaItem::$variant(existing.clone()), &item)
                });
                if is_favorite {
                    let mut value = $value.clone();
                    value.media.favorite = FieldKnowledge::Known(Some(true));
                    $items.push(value);
                }
            }};
        }
        match &item {
            MediaItem::Track(value) => update!(self.tracks, Track, value),
            MediaItem::Album(value) => update!(self.albums, Album, value),
            MediaItem::Artist(value) => update!(self.artists, Artist, value),
            MediaItem::Playlist(value) => update!(self.playlists, Playlist, value),
            _ => {}
        }
    }

    // Canonical projection ordering matches the existing native snapshot contract.
    pub(crate) fn items(&self) -> Vec<MediaItem> {
        self.tracks
            .iter()
            .cloned()
            .map(MediaItem::Track)
            .chain(self.albums.iter().cloned().map(MediaItem::Album))
            .chain(self.artists.iter().cloned().map(MediaItem::Artist))
            .chain(self.playlists.iter().cloned().map(MediaItem::Playlist))
            .collect()
    }
}

impl PrivatePlaylist {
    // Track order and duplicates matter; metadata-only events must not regenerate artwork.
    pub(crate) fn has_same_tracks(&self, other: &Self) -> bool {
        self.items.len() == other.items.len()
            && self.items.iter().zip(&other.items).all(|(left, right)| {
                match (&left.media.uri, &right.media.uri) {
                    (Some(left), Some(right)) => left == right,
                    _ => {
                        left.media.provider == right.media.provider
                            && left.media.item_id == right.media.item_id
                    }
                }
            })
    }
}

impl UserDataSnapshot {
    // Confirmed removal clears only the deleted library identity from visible projections.
    pub(crate) fn record_playlist_removal(&mut self, playlist: &Playlist) {
        let matches = |item: &Playlist| {
            item.media.provider == playlist.media.provider
                && item.media.item_id == playlist.media.item_id
        };
        self.playlists.retain(|entry| !matches(&entry.playlist));
        self.favorites.playlists.retain(|entry| !matches(entry));
        self.favorite_overrides
            .retain(|entry| !matches!(&entry.item, MediaItem::Playlist(item) if matches(item)));
    }

    // Metadata confirmations preserve track hydration and separately owned favorite intent.
    pub(crate) fn record_playlist_metadata(&mut self, mut playlist: Playlist, user_id: &str) {
        let mut metadata = playlist.clone();
        metadata.media.favorite = FieldKnowledge::Unknown;
        for favorite in &mut self.favorites.playlists {
            if favorite.media.provider == metadata.media.provider
                && favorite.media.item_id == metadata.media.item_id
            {
                let mut merged = MediaItem::Playlist(favorite.clone());
                merged.merge_known(MediaItem::Playlist(metadata.clone()));
                if let MediaItem::Playlist(updated) = merged {
                    *favorite = updated;
                }
            }
        }
        for entry in &mut self.favorite_overrides {
            if entry.item.fields().provider == metadata.media.provider
                && entry.item.fields().item_id == metadata.media.item_id
            {
                entry
                    .item
                    .merge_known(MediaItem::Playlist(metadata.clone()));
            }
        }
        let existing = self.playlists.iter().position(|entry| {
            entry.playlist.media.provider == metadata.media.provider
                && entry.playlist.media.item_id == metadata.media.item_id
        });
        if let Some(index) = existing {
            let mut merged = MediaItem::Playlist(self.playlists[index].playlist.clone());
            merged.merge_known(MediaItem::Playlist(metadata));
            if let MediaItem::Playlist(updated) = merged {
                playlist = updated;
            }
        }
        let is_owned_private = playlist.access.as_ref().is_some_and(|access| {
            access.owner.as_deref() == Some(user_id)
                && access.sharing == super::media::PlaylistSharing::Private
        });
        match (existing, is_owned_private) {
            (Some(index), true) => self.playlists[index].playlist = playlist,
            (Some(index), false) => {
                self.playlists.remove(index);
            }
            (None, true) => self.playlists.push(PrivatePlaylist {
                playlist,
                items: Vec::new(),
                is_loading_tracks: true,
            }),
            (None, false) => {}
        }
    }

    // Metadata pages preserve hydrated tracks; only a complete read removes unseen playlists.
    pub(crate) fn apply_playlist_metadata(
        &mut self,
        mut playlists: Vec<PrivatePlaylist>,
        has_observed_all_pages: bool,
    ) {
        for playlist in &mut playlists {
            if let Some(previous) = self.playlists.iter().find(|previous| {
                previous.playlist.media.provider == playlist.playlist.media.provider
                    && previous.playlist.media.item_id == playlist.playlist.media.item_id
            }) {
                let mut merged = MediaItem::Playlist(previous.playlist.clone());
                merged.merge_known(MediaItem::Playlist(playlist.playlist.clone()));
                if let MediaItem::Playlist(metadata) = merged {
                    playlist.playlist = metadata;
                }
                if playlist.is_loading_tracks {
                    playlist.items = previous.items.clone();
                    playlist.is_loading_tracks = previous.is_loading_tracks;
                }
            }
        }
        if !has_observed_all_pages {
            for previous in &self.playlists {
                if !playlists.iter().any(|playlist| {
                    playlist.playlist.media.provider == previous.playlist.media.provider
                        && playlist.playlist.media.item_id == previous.playlist.media.item_id
                }) {
                    playlists.push(previous.clone());
                }
            }
        }
        self.playlists = playlists;
        self.is_loading_playlists = !has_observed_all_pages;
    }

    // Command knowledge remains visible while a complete favorite listing is unavailable.
    pub(crate) fn record_favorite_state(&mut self, item: MediaItem, is_favorite: bool) {
        self.favorites.set_item(item.clone(), is_favorite);
        self.favorite_overrides
            .retain(|entry| !has_same_media_identity(&entry.item, &item));
        self.favorite_overrides
            .push(FavoriteOverride { item, is_favorite });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_playlist_metadata_preserves_hydrated_contents_and_favorite_intent() {
        let mut snapshot: UserDataSnapshot = serde_json::from_value(serde_json::json!({"playlists":[
            {"provider":"library","item_id":"owned","name":"Old","favorite":true,"access":{"owner":"user","sharing":"private"},"items":[{"provider":"library","item_id":"track","name":"Track"}],"is_loading_tracks":false},
            {"provider":"library","item_id":"other","name":"Keep","items":[],"is_loading_tracks":false}
        ]})).unwrap();
        let original = snapshot.playlists[0].clone();
        let other = snapshot.playlists[1].clone();
        snapshot.favorites.playlists.push(original.playlist.clone());
        snapshot.favorite_overrides.push(FavoriteOverride {
            item: MediaItem::Playlist(original.playlist),
            is_favorite: true,
        });
        let confirmed = serde_json::from_value(serde_json::json!({"provider":"library","item_id":"owned","name":"Confirmed","favorite":null,"access":{"owner":"user","sharing":"private"}})).unwrap();
        snapshot.record_playlist_metadata(confirmed, "user");
        assert_eq!(snapshot.playlists[0].playlist.media.name, "Confirmed");
        assert_eq!(snapshot.playlists[0].items, original.items);
        assert!(!snapshot.playlists[0].is_loading_tracks);
        assert_eq!(snapshot.playlists[1], other);
        assert!(!snapshot.is_loading_playlists);
        assert_eq!(snapshot.favorites.playlists[0].media.name, "Confirmed");
        assert_eq!(
            snapshot.favorites.playlists[0].media.favorite,
            FieldKnowledge::Known(Some(true))
        );
        assert!(snapshot.favorite_overrides[0].is_favorite);
        assert_eq!(
            snapshot.favorite_overrides[0].item.fields().name,
            "Confirmed"
        );
    }

    #[test]
    fn private_playlist_metadata_preserves_tracks_and_removes_unseen_only_after_completion() {
        let mut snapshot: UserDataSnapshot = serde_json::from_value(serde_json::json!({"playlists":[
            {"provider":"library","item_id":"owned","name":"Old","metadata":{"description":"Details"},"items":[{"provider":"library","item_id":"track","name":"Track"}],"is_loading_tracks":false},
            {"provider":"library","item_id":"empty","name":"Empty","items":[],"is_loading_tracks":false},
            {"provider":"library","item_id":"unseen","name":"Unseen","items":[],"is_loading_tracks":true}
        ]})).unwrap();
        let page: Vec<PrivatePlaylist> = serde_json::from_value(serde_json::json!([
            {"provider":"library","item_id":"owned","name":"New","is_loading_tracks":true},
            {"provider":"library","item_id":"empty","name":"Empty","is_loading_tracks":true}
        ]))
        .unwrap();
        snapshot.apply_playlist_metadata(page.clone(), false);
        assert_eq!(snapshot.playlists.len(), 3);
        assert!(snapshot.is_loading_playlists);
        assert_eq!(snapshot.playlists[0].items.len(), 1);
        assert_eq!(snapshot.playlists[0].playlist.media.name, "New");
        assert_eq!(
            snapshot.playlists[0]
                .playlist
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Details")
        );
        assert!(!snapshot.playlists[0].is_loading_tracks);
        assert!(!snapshot.playlists[1].is_loading_tracks);
        snapshot.apply_playlist_metadata(Vec::new(), false);
        assert_eq!(snapshot.playlists.len(), 3);
        snapshot.apply_playlist_metadata(page, true);
        assert_eq!(snapshot.playlists.len(), 2);
        assert!(!snapshot.is_loading_playlists);
        assert_eq!(snapshot.playlists[0].items.len(), 1);
        snapshot.apply_playlist_metadata(Vec::new(), true);
        assert!(snapshot.playlists.is_empty());
    }

    fn track(favorite: Option<bool>) -> MediaItem {
        MediaItem::Track(
            serde_json::from_value(serde_json::json!({
                "item_id":"track", "provider":"library", "name":"Track", "favorite":favorite
            }))
            .unwrap(),
        )
    }

    #[test]
    fn favorite_projection_preserves_the_original_three_state_rating() {
        for state in [Some(true), Some(false), None] {
            let item = track(state);
            let mut favorites = Favorites::default();
            favorites.set_item(item.clone(), true);
            assert_eq!(item.fields().favorite, FieldKnowledge::Known(state));
            assert_eq!(
                favorites.tracks[0].media.favorite,
                FieldKnowledge::Known(Some(true))
            );
            favorites.set_item(item.clone(), false);
            assert!(favorites.items().is_empty());
            assert_eq!(item.fields().favorite, FieldKnowledge::Known(state));
        }
    }

    fn playlist(track_ids: &[&str]) -> PrivatePlaylist {
        PrivatePlaylist {
            playlist: serde_json::from_value(serde_json::json!({
                "item_id":"42", "provider":"library", "name":"Playlist"
            }))
            .unwrap(),
            items: track_ids
                .iter()
                .map(|item_id| {
                    serde_json::from_value(serde_json::json!({
                        "item_id":item_id, "provider":"source-one", "name":"Track"
                    }))
                    .unwrap()
                })
                .collect(),
            is_loading_tracks: false,
        }
    }
}
