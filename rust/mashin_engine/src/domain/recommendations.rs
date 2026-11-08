//! Recommendation values and view rules; loading and retry state are session-owned.

use super::identifiers::{MediaId, ProviderInstanceId};
use super::media::{MediaItem, Track};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RecommendationView {
    Home,
    Explore,
}

impl RecommendationView {
    pub(crate) fn folder_selection(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::Home => (
                &[
                    "user_recommendations",
                    "user_genre_playlists_monthly",
                    "user_artist_playlists_monthly",
                    "user_top_tracks_monthly",
                    "user_top_artists_monthly",
                ],
                &["user_genre_artists_", "user_similar_artists_"],
            ),
            Self::Explore => (
                &[
                    "user_exploration_weekly",
                    "similarusers_jam_weekly",
                    "sitewide_top_albums_weekly",
                    "sitewide_top_tracks_weekly",
                    "sitewide_top_artists_weekly",
                ],
                &[],
            ),
        }
    }

    pub(crate) fn includes(self, item_id: &str) -> bool {
        let (identities, prefixes) = self.folder_selection();
        identities.contains(&item_id) || prefixes.iter().any(|prefix| item_id.starts_with(prefix))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct RecommendationSnapshot {
    pub(crate) folders: Vec<RecommendationFolder>,
    pub(crate) is_loading: bool,
    pub(crate) has_load_error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct RecommendationFolder {
    pub(crate) item_id: MediaId,
    pub(crate) provider: ProviderInstanceId,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) items: Vec<MediaItem>,
    pub(crate) is_loading: bool,
    pub(crate) has_load_error: bool,
    pub(crate) playlist_tracks: Vec<Track>,
    #[serde(skip)]
    pub(crate) knowledge: RecommendationKnowledge,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RecommendationKnowledge {
    pub(crate) title: bool,
    pub(crate) subtitle: bool,
    pub(crate) items: bool,
    pub(crate) playlist_tracks: bool,
}
