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
                    "user_artist_recommendations",
                    "user_top_albums_monthly",
                    "user_album_recommendations",
                    "user_rediscovery_tracks",
                ],
                &["user_genre_artists_"],
            ),
            Self::Explore => (
                &[
                    "global_top_tracks",
                    "de_top_tracks",
                    "global_top_artists",
                    "de_top_artists",
                    "global_popular_artist_albums",
                    "de_popular_artist_albums",
                    "global_genre_playlists",
                    "global_artist_radio_playlists",
                    "global_mood_playlists",
                    "global_decade_playlists",
                ],
                &["global_genre_albums_", "global_genre_artists_"],
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
    pub(crate) revision: u64,
    pub(crate) folders: Vec<RecommendationFolder>,
    pub(crate) is_loading: bool,
    pub(crate) has_load_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) delta: Option<RecommendationDelta>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct RecommendationDelta {
    pub(crate) revision: u64,
    pub(crate) sequence: u64,
    pub(crate) order: Vec<RecommendationFolderIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct RecommendationFolderIdentity {
    pub(crate) provider: ProviderInstanceId,
    pub(crate) item_id: MediaId,
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

#[cfg(test)]
mod tests {
    use super::RecommendationView;

    #[test]
    fn lastfm_folders_belong_to_exactly_one_view() {
        for view in [RecommendationView::Home, RecommendationView::Explore] {
            let other = match view {
                RecommendationView::Home => RecommendationView::Explore,
                RecommendationView::Explore => RecommendationView::Home,
            };
            let (identities, prefixes) = view.folder_selection();
            for identity in identities.iter().map(|identity| identity.to_string())
                .chain(prefixes.iter().map(|prefix| format!("{prefix}rock"))) {
                assert!(view.includes(&identity));
                assert!(!other.includes(&identity));
            }
            assert!(!view.includes("sitewide_top_tracks_weekly"));
            assert!(!view.includes("user_similar_artists_rock"));
        }
    }
}
