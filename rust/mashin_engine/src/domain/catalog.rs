//! Typed catalog identities and stored observations, independent of transport and storage.

use super::identifiers::MediaId;
use super::media::{
    Album, Artist, MediaItem, MediaType, MusicAssistantSearchResults, Playlist, Track,
};
use super::recommendations::RecommendationFolder;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum CatalogQuery {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum CatalogValue {
    Item(MediaItem),
    RecommendationFolders(Vec<RecommendationFolder>),
    RecommendationItems(Vec<MediaItem>),
    Search(MusicAssistantSearchResults),
    Artist(Artist),
    Album(Album),
    Track(Track),
    Playlist(Playlist),
    Tracks(Vec<Track>),
    Albums(Vec<Album>),
    Artists(Vec<Artist>),
}
