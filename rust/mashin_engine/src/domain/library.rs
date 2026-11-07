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
