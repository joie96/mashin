//! Catalog endpoint mapping; query identities and results remain typed outside this boundary.

use crate::data::music_assistant::client::MusicAssistantClient;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::MediaId;
use crate::domain::media::Playlist;
use serde_json::json;

pub(crate) async fn load_catalog(
    client: &MusicAssistantClient,
    query: &CatalogQuery,
    force_refresh: bool,
) -> Result<CatalogValue, String> {
    match query {
        CatalogQuery::ProviderItem { item_id, provider, media_type } => client.execute_command("music/item", json!({"media_type":media_type, "item_id":item_id, "provider_instance_id_or_domain":provider, "allow_update_metadata":false})).await.map(CatalogValue::Item),
        CatalogQuery::RecommendationFolders => client
            .execute_command("music/recommendations", json!({}))
            .await
            .map(CatalogValue::RecommendationFolders),
        CatalogQuery::RecommendationItems { item_id, provider } => client
            .execute_command(
                "music/recommendations/items",
                json!({"provider":provider, "item_id":item_id}),
            )
            .await
            .map(CatalogValue::RecommendationItems),
        CatalogQuery::RecentlyPlayed { limit } => client
            .execute_command(
                "music/recently_played_items",
                json!({"limit": limit, "media_types": ["track"]}),
            )
            .await
            .map(CatalogValue::Tracks),
        CatalogQuery::Search {
            query,
            media_type,
            limit,
        } => client
            .execute_command(
                "music/search",
                json!({"search_query": query, "media_types": [media_type],
                "limit": limit, "library_only": false}),
            )
            .await
            .map(CatalogValue::Search),
        CatalogQuery::Playlist { item_id, provider } => {
            load_playlist(client, item_id, provider, true)
                .await
                .map(CatalogValue::Playlist)
        }
        query => {
            let (command, item_id, provider) = match query {
                CatalogQuery::Artist { item_id, provider } => {
                    ("music/artists/get", item_id, provider)
                }
                CatalogQuery::Album { item_id, provider } => {
                    ("music/albums/get", item_id, provider)
                }
                CatalogQuery::Track { item_id, provider } => {
                    ("music/tracks/get", item_id, provider)
                }
                CatalogQuery::AlbumTracks { item_id, provider } => {
                    ("music/albums/album_tracks", item_id, provider)
                }
                CatalogQuery::PlaylistTracks {
                    item_id, provider, ..
                } => ("music/playlists/playlist_tracks", item_id, provider),
                CatalogQuery::ArtistAlbums { item_id, provider } => {
                    ("music/artists/artist_albums", item_id, provider)
                }
                CatalogQuery::ArtistTopTracks { item_id, provider } => {
                    ("music/artists/top_tracks", item_id, provider)
                }
                CatalogQuery::ArtistTracks { item_id, provider } => {
                    ("music/artists/artist_tracks", item_id, provider)
                }
                CatalogQuery::SimilarArtists {
                    item_id, provider, ..
                } => ("music/artists/similar_artists", item_id, provider),
                CatalogQuery::SimilarTracks {
                    item_id, provider, ..
                } => ("music/tracks/similar_tracks", item_id, provider),
                CatalogQuery::TrackVersions { item_id, provider } => {
                    ("music/tracks/track_versions", item_id, provider)
                }
                CatalogQuery::RecentlyPlayed { .. }
                | CatalogQuery::ProviderItem { .. }
                | CatalogQuery::RecommendationFolders
                | CatalogQuery::RecommendationItems { .. }
                | CatalogQuery::Search { .. }
                | CatalogQuery::Playlist { .. } => unreachable!(),
            };
            let mut arguments =
                json!({"item_id":item_id, "provider_instance_id_or_domain":provider});
            match query {
                CatalogQuery::PlaylistTracks {
                    allow_dynamic_tracks,
                    ..
                } => {
                    arguments["force_refresh"] = json!(force_refresh);
                    arguments["allow_dynamic_tracks"] = json!(allow_dynamic_tracks);
                }
                CatalogQuery::SimilarArtists { limit, .. } => arguments["limit"] = json!(limit),
                CatalogQuery::SimilarTracks {
                    limit,
                    allow_lookup,
                    ..
                } => {
                    arguments["limit"] = json!(limit);
                    arguments["allow_lookup"] = json!(allow_lookup);
                }
                _ => {}
            }
            match query {
                CatalogQuery::Artist { .. } => client
                    .execute_command(command, arguments)
                    .await
                    .map(CatalogValue::Artist),
                CatalogQuery::Album { .. } => client
                    .execute_command(command, arguments)
                    .await
                    .map(CatalogValue::Album),
                CatalogQuery::Track { .. } => client
                    .execute_command(command, arguments)
                    .await
                    .map(CatalogValue::Track),
                CatalogQuery::ArtistAlbums { .. } => client
                    .execute_command(command, arguments)
                    .await
                    .map(CatalogValue::Albums),
                CatalogQuery::SimilarArtists { .. } => client
                    .execute_command(command, arguments)
                    .await
                    .map(CatalogValue::Artists),
                _ => client
                    .execute_command(command, arguments)
                    .await
                    .map(CatalogValue::Tracks),
            }
        }
    }
}

// Ownership checks request metadata without allowing a server-side metadata update.
pub(crate) async fn load_playlist(
    client: &MusicAssistantClient,
    item_id: &MediaId,
    provider: &str,
    should_update_metadata: bool,
) -> Result<Playlist, String> {
    let mut arguments = json!({"item_id":item_id, "provider_instance_id_or_domain":provider});
    if !should_update_metadata {
        arguments["allow_update_metadata"] = json!(false);
    }
    client
        .execute_command("music/playlists/get", arguments)
        .await
}

impl crate::application::repositories::CatalogClient for MusicAssistantClient {
    // Scheduling reservations and catalog reads share the authenticated transport.
    fn begin_startup(&self) -> Box<dyn Send> {
        MusicAssistantClient::begin_startup(self)
    }
    fn load_catalog(
        &self,
        query: CatalogQuery,
        force_refresh: bool,
    ) -> futures_util::future::BoxFuture<'_, Result<CatalogValue, String>> {
        Box::pin(async move {
            let group = self.create_read_group();
            group.scope(load_catalog(self, &query, force_refresh)).await
        })
    }
}

impl crate::application::repositories::ArtworkClient for MusicAssistantClient {
    // Existing artwork wins; missing artwork is resolved without retaining provider results.
    fn get_media_artwork<'request>(
        &'request self,
        item: &'request crate::domain::media::MediaItem,
    ) -> futures_util::future::BoxFuture<'request, Option<crate::domain::media::MediaItemImage>>
    {
        Box::pin(async move {
            if let Some(image) = crate::domain::artwork::select_artwork(item) {
                return Some(image);
            }
            for mapping in &item.fields().provider_mappings {
                let media_type = serde_json::to_value(item).ok()?.get("media_type")?.clone();
                let value: crate::domain::media::MediaItem = match self.execute_command("music/item", json!({"media_type":media_type,"item_id":mapping.item_id,"provider_instance_id_or_domain":mapping.provider_instance,"allow_update_metadata":false})).await {
                    Ok(value) => value, Err(_) => continue,
                };
                if let Some(image) = crate::domain::artwork::select_artwork(&value) {
                    return Some(image);
                }
            }
            None
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::music_assistant::client::tests::server;
    use crate::domain::identifiers::MediaId;
    use crate::domain::media::MediaType;

    #[tokio::test]
    async fn playlist_metadata_preserves_normal_reads_and_non_updating_ownership_reads() {
        let playlist = json!({"item_id":"one", "provider":"library", "name":"Playlist"});
        let (client, worker) = server(vec![
            ("music/playlists/get", playlist.clone()),
            ("music/playlists/get", playlist),
        ]);
        let query = CatalogQuery::Playlist {
            item_id: MediaId("one".into()),
            provider: "library".into(),
        };
        assert!(matches!(
            load_catalog(&client, &query, false).await.unwrap(),
            CatalogValue::Playlist(_)
        ));
        load_playlist(&client, &MediaId("one".into()), "library", false)
            .await
            .unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(
            requests[0]["args"],
            json!({"item_id":"one", "provider_instance_id_or_domain":"library"})
        );
        assert_eq!(requests[1]["args"]["allow_update_metadata"], false);
    }

    #[tokio::test]
    async fn search_preserves_typed_filters_and_returns_metadata_without_provider_reads() {
        let (client, worker) = server(vec![(
            "music/search",
            json!({"tracks":[{
                "item_id":"one", "provider":"test", "name":"Track"
            }]}),
        )]);
        let value = load_catalog(
            &client,
            &CatalogQuery::Search {
                query: "jazz".into(),
                media_type: MediaType::Track,
                limit: 12,
            },
            false,
        )
        .await
        .unwrap();
        assert!(matches!(value, CatalogValue::Search(items) if items.tracks.len() == 1));
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["args"],
            json!({"search_query":"jazz", "media_types":["track"],
            "limit":12, "library_only":false})
        );
    }
}
