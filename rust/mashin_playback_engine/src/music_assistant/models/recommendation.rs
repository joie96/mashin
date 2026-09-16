//! Typed Music Assistant recommendation response models.

use super::identifiers::{MediaId, ProviderInstanceId};
use super::media::{deserialize_nullable_vec, MediaItem};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MusicAssistantRecommendationFolder {
    pub(crate) item_id: MediaId,
    pub(crate) provider: ProviderInstanceId,
    #[serde(default, alias = "name")]
    pub(crate) title: String,
    #[serde(default, alias = "description")]
    pub(crate) subtitle: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) items: Vec<MediaItem>,
}

#[cfg(test)]
mod tests {
    use super::MusicAssistantRecommendationFolder;
    use crate::music_assistant::models::media::MediaItem;

    #[test]
    fn deserializes_folder_name_and_description_aliases() {
        let folder = serde_json::from_str::<MusicAssistantRecommendationFolder>(
            r#"{
                "item_id": "top_tracks",
                "provider": "listenbrainz_recommendations--instance",
                "name": "Top tracks",
                "description": "A personal selection",
                "items": [{
                    "media_type": "track",
                    "item_id": "track-1",
                    "provider": "library",
                    "name": "Track one"
                }]
            }"#,
        )
        .expect("recommendation folder should deserialize");

        assert_eq!(folder.title, "Top tracks");
        assert_eq!(folder.subtitle.as_deref(), Some("A personal selection"));
        assert_eq!(folder.items.len(), 1);
    }

    #[test]
    fn preserves_playlist_recommendation_items() {
        let folder = serde_json::from_str::<MusicAssistantRecommendationFolder>(
            r#"{
                "item_id": "artist_playlists",
                "provider": "listenbrainz_recommendations--instance",
                "items": [{
                    "media_type": "playlist",
                    "item_id": "playlist-1",
                    "provider": "library",
                    "name": "Playlist one"
                }]
            }"#,
        )
        .expect("playlist recommendation should deserialize");

        assert!(matches!(folder.items.as_slice(), [MediaItem::Playlist(_)]));
    }
}
