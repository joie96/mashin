//! Decodes server recommendation aliases without adding wire policy to view rules.

use crate::domain::identifiers::{MediaId, ProviderInstanceId};
use crate::domain::media::{FieldKnowledge, MediaItem, Track};
use crate::domain::recommendations::{RecommendationFolder, RecommendationKnowledge};
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
struct RecommendationFolderInput {
    item_id: MediaId,
    provider: ProviderInstanceId,
    #[serde(default, alias = "name")]
    title: FieldKnowledge<String>,
    #[serde(default, alias = "description")]
    subtitle: FieldKnowledge<Option<String>>,
    #[serde(default)]
    items: FieldKnowledge<Option<Vec<MediaItem>>>,
    #[serde(default)]
    is_loading: bool,
    #[serde(default)]
    has_load_error: bool,
    #[serde(default)]
    playlist_tracks: FieldKnowledge<Option<Vec<Track>>>,
}

impl<'de> Deserialize<'de> for RecommendationFolder {
    fn deserialize<Decoder: Deserializer<'de>>(decoder: Decoder) -> Result<Self, Decoder::Error> {
        let input = RecommendationFolderInput::deserialize(decoder)?;
        let knowledge = RecommendationKnowledge {
            title: !input.title.is_unknown(),
            subtitle: !input.subtitle.is_unknown(), items: !input.items.is_unknown(),
            playlist_tracks: !input.playlist_tracks.is_unknown(),
        };
        Ok(Self {
            item_id: input.item_id, provider: input.provider, title: input.title.known_value().cloned().unwrap_or_default(),
            subtitle: input.subtitle.known_value().cloned().flatten(),
            items: input.items.known_value().cloned().flatten().unwrap_or_default(),
            playlist_tracks: input.playlist_tracks.known_value().cloned().flatten().unwrap_or_default(),
            is_loading: input.is_loading, has_load_error: input.has_load_error, knowledge,
        })
    }
}
