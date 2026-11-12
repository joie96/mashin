//! Typed catalog identities and stored observations, independent of transport and storage.

use super::identifiers::MediaId;
use super::media::{
    Album, Artist, MediaItem, MediaType, MusicAssistantSearchResults, Playlist, Track,
};
use super::recommendations::RecommendationFolder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ArtistLibraryFilters {
    pub(crate) search: Option<String>,
    pub(crate) provider: Option<LibraryProviderFilter>,
    pub(crate) genre: Option<LibraryGenreFilter>,
    pub(crate) played_only: bool,
    pub(crate) album_artists_only: bool,
    pub(crate) artist_type: Option<LibraryArtistType>,
    pub(crate) sort_field: Option<ArtistLibrarySort>,
    pub(crate) sort_direction: Option<CatalogSortDirection>,
    pub(crate) reachable_via: Option<Vec<String>>,
    pub(crate) random_seed: Option<i64>,
}

// Refresh sharing and persistent query identity.
impl ArtistLibraryFilters {
    pub(crate) fn requires_new_random_sample(&self) -> bool {
        self.random_seed.is_none()
            && matches!(
                self.sort_field,
                Some(ArtistLibrarySort::Random | ArtistLibrarySort::RandomPlayCount)
            )
    }

    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum LibraryProviderFilter {
    Instance(String),
    Instances(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum LibraryGenreFilter {
    Genre(i64),
    Genres(Vec<i64>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LibraryArtistType {
    Singer,
    Author,
    Narrator,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArtistLibrarySort {
    SortName,
    Name,
    TimestampAdded,
    TimestampModified,
    LastPlayed,
    PlayCount,
    FavoriteTimestamp,
    Random,
    RandomPlayCount,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AlbumLibraryFilters {
    pub(crate) search: Option<String>,
    pub(crate) provider: Option<LibraryProviderFilter>,
    pub(crate) genre: Option<LibraryGenreFilter>,
    pub(crate) played_only: bool,
    pub(crate) album_types: Option<Vec<LibraryAlbumType>>,
    pub(crate) sort_field: Option<AlbumLibrarySort>,
    pub(crate) sort_direction: Option<CatalogSortDirection>,
    pub(crate) reachable_via: Option<Vec<String>>,
    pub(crate) random_seed: Option<i64>,
}

// Refresh sharing and persistent query identity.
impl AlbumLibraryFilters {
    pub(crate) fn requires_new_random_sample(&self) -> bool {
        self.random_seed.is_none()
            && matches!(
                self.sort_field,
                Some(AlbumLibrarySort::Random | AlbumLibrarySort::RandomPlayCount)
            )
    }

    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LibraryAlbumType {
    Album,
    Single,
    Live,
    Soundtrack,
    Compilation,
    Ep,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AlbumLibrarySort {
    SortName,
    Name,
    TimestampAdded,
    TimestampModified,
    LastPlayed,
    PlayCount,
    FavoriteTimestamp,
    Year,
    ArtistName,
    Random,
    RandomPlayCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogSortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum CatalogQuery {
    ProviderManifests {},
    #[serde(skip)]
    ProviderInstances,
    ProviderItem {
        item_id: MediaId,
        provider: String,
        media_type: MediaType,
    },
    RecommendationFolders,
    RecommendationItems {
        item_id: MediaId,
        provider: String,
    },
    RecentlyPlayed {
        limit: u32,
    },
    Search {
        query: String,
        media_type: MediaType,
        limit: u32,
    },
    Artist {
        item_id: MediaId,
        provider: String,
    },
    LibraryArtists {
        favorite: Option<bool>,
        limit: u32,
        offset: u32,
        summary: bool,
        #[serde(default, skip_serializing_if = "ArtistLibraryFilters::is_default")]
        filters: ArtistLibraryFilters,
    },
    LibraryAlbums {
        favorite: Option<bool>,
        limit: u32,
        offset: u32,
        summary: bool,
        #[serde(default, skip_serializing_if = "AlbumLibraryFilters::is_default")]
        filters: AlbumLibraryFilters,
    },
    LibraryTracks {
        favorite: Option<bool>,
        limit: u32,
        offset: u32,
        summary: bool,
    },
    LibraryPlaylists {
        favorite: Option<bool>,
        limit: u32,
        offset: u32,
        summary: bool,
    },
    Album {
        item_id: MediaId,
        provider: String,
    },
    Track {
        item_id: MediaId,
        provider: String,
    },
    Playlist {
        item_id: MediaId,
        provider: String,
    },
    AlbumTracks {
        item_id: MediaId,
        provider: String,
    },
    PlaylistTracks {
        item_id: MediaId,
        provider: String,
        allow_dynamic_tracks: bool,
    },
    ArtistAlbums {
        item_id: MediaId,
        provider: String,
    },
    ArtistTopTracks {
        item_id: MediaId,
        provider: String,
    },
    ArtistTracks {
        item_id: MediaId,
        provider: String,
    },
    SimilarArtists {
        item_id: MediaId,
        provider: String,
        limit: u32,
    },
    SimilarTracks {
        item_id: MediaId,
        provider: String,
        limit: u32,
        allow_lookup: bool,
    },
    TrackVersions {
        item_id: MediaId,
        provider: String,
    },
}

// Payload-free query classification for diagnostics.
impl CatalogQuery {
    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            Self::ProviderManifests { .. } => "ProviderManifests",
            Self::ProviderInstances => "ProviderInstances",
            Self::ProviderItem { .. } => "ProviderItem",
            Self::RecommendationFolders => "RecommendationFolders",
            Self::RecommendationItems { .. } => "RecommendationItems",
            Self::RecentlyPlayed { .. } => "RecentlyPlayed",
            Self::Search { .. } => "Search",
            Self::Artist { .. } => "Artist",
            Self::LibraryArtists { .. } => "LibraryArtists",
            Self::LibraryAlbums { .. } => "LibraryAlbums",
            Self::LibraryTracks { .. } => "LibraryTracks",
            Self::LibraryPlaylists { .. } => "LibraryPlaylists",
            Self::Album { .. } => "Album",
            Self::Track { .. } => "Track",
            Self::Playlist { .. } => "Playlist",
            Self::AlbumTracks { .. } => "AlbumTracks",
            Self::PlaylistTracks { .. } => "PlaylistTracks",
            Self::ArtistAlbums { .. } => "ArtistAlbums",
            Self::ArtistTopTracks { .. } => "ArtistTopTracks",
            Self::ArtistTracks { .. } => "ArtistTracks",
            Self::SimilarArtists { .. } => "SimilarArtists",
            Self::SimilarTracks { .. } => "SimilarTracks",
            Self::TrackVersions { .. } => "TrackVersions",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum CatalogValue {
    ProviderManifests(Vec<super::providers::ProviderManifest>),
    #[serde(skip)]
    ProviderInstances(Vec<super::providers::ProviderInstance>),
    Item(MediaItem),
    RecommendationFolders(Vec<RecommendationFolder>),
    RecommendationItems(Vec<MediaItem>),
    Search(MusicAssistantSearchResults),
    Artist(Artist),
    Album(Album),
    Track(Track),
    Playlist(Playlist),
    Playlists(Vec<Playlist>),
    Tracks(Vec<Track>),
    Albums(Vec<Album>),
    Artists(Vec<Artist>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogLoadPhase {
    ReadingLocal,
    Refreshing,
    Ready,
    Failed,
    Invalidated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogReadKnowledge {
    Unknown,
    Partial,
    ObservedResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogFreshness {
    Unknown,
    Stale,
    Fresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CatalogLoadSnapshot {
    #[serde(skip)]
    pub(crate) publication_revision: u64,
    pub(crate) value: Option<std::sync::Arc<CatalogValue>>,
    #[serde(default)]
    pub(crate) source: Option<CatalogDataSource>,
    pub(crate) phase: CatalogLoadPhase,
    pub(crate) knowledge: CatalogReadKnowledge,
    pub(crate) freshness: CatalogFreshness,
    pub(crate) observed_at: Option<i64>,
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) cache_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogDataSource {
    Cache,
    Server,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_snapshot_clones_share_media_and_preserve_wire_shape() {
        let encoded = serde_json::json!({
            "value": {"Tracks": []},
            "source": "server",
            "phase": "ready",
            "knowledge": "observed_response",
            "freshness": "fresh",
            "observed_at": null,
            "error": null,
            "cache_error": null
        });
        let snapshot: CatalogLoadSnapshot = serde_json::from_value(encoded.clone()).unwrap();
        let mut status = snapshot.clone();
        status.phase = CatalogLoadPhase::Refreshing;
        assert!(std::sync::Arc::ptr_eq(
            snapshot.value.as_ref().unwrap(),
            status.value.as_ref().unwrap(),
        ));
        assert_eq!(snapshot.phase, CatalogLoadPhase::Ready);
        assert_eq!(serde_json::to_value(snapshot).unwrap(), encoded);
    }
}
