//! Catalog endpoint mapping; query identities and results remain typed outside this boundary.

use crate::data::music_assistant::client::MusicAssistantClient;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, MediaUri, ProviderInstanceId};
use crate::domain::media::{FieldKnowledge, MediaItemImage, MediaType, Track};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub(crate) async fn load_catalog(
    client: &MusicAssistantClient,
    query: &CatalogQuery,
    force_refresh: bool,
) -> Result<CatalogValue, String> {
    match query {
        CatalogQuery::ProviderManifests {} => super::session::load_provider_manifests(client).await.map(CatalogValue::ProviderManifests),
        CatalogQuery::ProviderInstances => super::session::load_provider_instances(client)
            .await
            .map(CatalogValue::ProviderInstances),
        CatalogQuery::LibraryTracks { favorite, limit, offset, summary } => client
            .execute_command("music/tracks/library_items", json!({
                "favorite": favorite, "limit": limit, "offset": offset, "summary": summary,
                "sort_field": "sort_name", "sort_direction": "asc",
            }))
            .await
            .map(CatalogValue::Tracks),
        CatalogQuery::LibraryPlaylists { favorite, limit, offset, summary } => client
            .execute_command("music/playlists/library_items", json!({
                "favorite": favorite, "limit": limit, "offset": offset, "summary": summary,
                "sort_field": "sort_name", "sort_direction": "asc",
            }))
            .await
            .map(CatalogValue::Playlists),
        CatalogQuery::LibraryAlbums {favorite,limit,offset,summary,filters} => {
            let mut arguments=serde_json::to_value(filters).map_err(|error|error.to_string())?;
            arguments["favorite"]=json!(favorite);
            arguments["limit"]=json!(limit);
            arguments["offset"]=json!(offset);
            arguments["summary"]=json!(summary);
            if filters.sort_field.is_none() {
                arguments["sort_field"]=json!("sort_name");
                if filters.sort_direction.is_none() {arguments["sort_direction"]=json!("asc");}
            }
            client.execute_command("music/albums/library_items", arguments).await.map(CatalogValue::Albums)
        }
        CatalogQuery::LibraryArtists {favorite,limit,offset,summary,filters}=> {
            let mut arguments=serde_json::to_value(filters).map_err(|error|error.to_string())?;
            arguments["favorite"]=json!(favorite);
            arguments["limit"]=json!(limit);
            arguments["offset"]=json!(offset);
            arguments["summary"]=json!(summary);
            if filters.sort_field.is_none() {
                arguments["sort_field"]=json!("sort_name");
                if filters.sort_direction.is_none() {arguments["sort_direction"]=json!("asc");}
            }
            client.execute_command("music/artists/library_items",arguments).await.map(CatalogValue::Artists)
        }
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
        CatalogQuery::RecentlyPlayed { limit } => {
            let mappings: Vec<RecentlyPlayedTrackMapping> = client.execute_command(
                "music/recently_played_items",
                json!({"limit": limit, "media_types": ["track"]}),
            ).await?;
            tokio::task::spawn_blocking(move || {
                mappings.into_iter().map(RecentlyPlayedTrackMapping::into_track)
                    .collect::<Result<Vec<_>, _>>()
            }).await.map_err(|error| format!("recently played projection failed: {error}"))?
                .map(CatalogValue::Tracks)
        }
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
                CatalogQuery::ProviderManifests {}
                | CatalogQuery::ProviderInstances
                | CatalogQuery::RecentlyPlayed { .. }
                | CatalogQuery::LibraryArtists { .. }
                | CatalogQuery::LibraryAlbums { .. }
                | CatalogQuery::LibraryTracks { .. }
                | CatalogQuery::LibraryPlaylists { .. }
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
                CatalogQuery::PlaylistTracks { .. } => {
                    let items: Vec<crate::domain::media::MediaItem> = client.execute_command(command, arguments).await?;
                    items.into_iter().map(|item| match item {
                        crate::domain::media::MediaItem::Track(track) => Ok(track),
                        _ => Err("playlist contains unsupported media type for track-only query".to_owned()),
                    }).collect::<Result<Vec<_>, _>>().map(CatalogValue::Tracks)
                }
                _ => client
                    .execute_command(command, arguments)
                    .await
                    .map(CatalogValue::Tracks),
            }
        }
    }
}

// Ownership checks request metadata without allowing a server-side metadata update.
pub(crate) async fn load_playlist<Response: serde::de::DeserializeOwned>(
    client: &MusicAssistantClient,
    item_id: &MediaId,
    provider: &str,
    should_update_metadata: bool,
) -> Result<Response, String> {
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

// Private projections separate progress-derived observations from generic mapping defaults.
#[derive(Deserialize, Serialize)]
struct RecentlyPlayedTrackMapping {
    media_type: MediaType,
    item_id: MediaId,
    provider: ProviderInstanceId,
    name: String,
    #[serde(default, skip_serializing_if = "FieldKnowledge::is_unknown")]
    uri: FieldKnowledge<Option<MediaUri>>,
    #[serde(default, skip_serializing_if = "FieldKnowledge::is_unknown")]
    image: FieldKnowledge<Option<MediaItemImage>>,
    #[serde(default, skip_serializing_if = "FieldKnowledge::is_unknown")]
    available: FieldKnowledge<bool>,
}

impl RecentlyPlayedTrackMapping {
    // Mapping defaults cannot confirm or erase detailed track fields.
    fn into_track(self) -> Result<Track, String> {
        if self.media_type != MediaType::Track {
            return Err("Music Assistant recently played item is not a track".into());
        }
        let value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        serde_json::from_value(value).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::music_assistant::client::tests::server;
    use crate::domain::identifiers::MediaId;
    use crate::domain::media::{MediaItem, MediaType};

    #[tokio::test]
    async fn playlist_tracks_preserve_occurrences_flags_and_reject_mixed_media() {
        let (client, worker) = server(vec![
            (
                "music/playlists/playlist_tracks",
                json!([
                    {"media_type":"track","provider":"source-a","item_id":"same","name":"Track","position":1},
                    {"media_type":"track","provider":"source-a","item_id":"same","name":"Track","position":4}
                ]),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([
                    {"media_type":"track","provider":"source-a","item_id":"same","name":"Track"},
                    {"media_type":"radio","provider":"source-a","item_id":"station","name":"Station"}
                ]),
            ),
        ]);
        for (allow_dynamic_tracks, force_refresh) in [(false, false), (true, true)] {
            let result = load_catalog(
                &client,
                &CatalogQuery::PlaylistTracks {
                    provider: "library".into(),
                    item_id: MediaId("playlist".into()),
                    allow_dynamic_tracks,
                },
                force_refresh,
            )
            .await;
            if allow_dynamic_tracks {
                assert!(result.unwrap_err().contains("unsupported media type"));
            } else {
                let CatalogValue::Tracks(tracks) = result.unwrap() else {
                    panic!("playlist tracks missing")
                };
                assert_eq!(tracks.len(), 2);
                assert_eq!(tracks[0].media.position, Some(1));
                assert_eq!(tracks[1].media.position, Some(4));
                assert_eq!(tracks[0].media.item_id, tracks[1].media.item_id);
            }
        }
        let requests = worker.join().unwrap();
        for (request, flag) in requests.iter().zip([false, true]) {
            assert_eq!(
                request["args"],
                json!({"item_id":"playlist","provider_instance_id_or_domain":"library","force_refresh":flag,"allow_dynamic_tracks":flag})
            );
        }
    }

    #[tokio::test]
    async fn recently_played_projects_progress_fields_and_keeps_limit_zero() {
        let mapping = json!([{"media_type":"track","provider":"source-a","item_id":"recent","name":"Recent",
            "version":"","sort_name":"recent","uri":"source-a://track/recent","external_ids":[],"is_playable":true,
            "available":false,"image":null,"year":null}]);
        let (client, worker) = server(vec![
            ("music/recently_played_items", mapping.clone()),
            ("music/recently_played_items", mapping),
            (
                "music/recently_played_items",
                json!([{"media_type":"album","provider":"library","item_id":"wrong","name":"Wrong"}]),
            ),
            ("music/recently_played_items", json!([])),
        ]);
        for limit in [0, 2] {
            let CatalogValue::Tracks(tracks) =
                load_catalog(&client, &CatalogQuery::RecentlyPlayed { limit }, false)
                    .await
                    .unwrap()
            else {
                panic!("recent tracks missing")
            };
            let track = &tracks[0];
            assert_eq!(track.media.name, "Recent");
            assert_eq!(track.media.available, Some(false));
            assert!(track.media.knowledge.available);
            assert_eq!(
                track.media.uri.as_ref().unwrap().as_str(),
                "source-a://track/recent"
            );
            assert!(track.media.knowledge.uri);
            assert_eq!(track.image, None);
            assert!(track.knowledge.image);
            assert!(!track.media.knowledge.version);
            assert!(!track.media.knowledge.sort_name);
            assert!(!track.media.knowledge.external_ids);
            assert!(!track.media.knowledge.is_playable);
            assert!(!track.knowledge.duration);
            assert!(!track.knowledge.artists);
            assert!(!track.knowledge.album);
            assert_eq!(track.media.favorite, FieldKnowledge::Unknown);
            let typed = serde_json::to_value(CatalogValue::Tracks(tracks.clone())).unwrap();
            assert!(typed["Tracks"][0].get("media_type").is_none());
            assert_eq!(typed["Tracks"][0]["item_id"], "recent");
            let tagged = serde_json::to_value(MediaItem::Track(track.clone())).unwrap();
            assert_eq!(tagged["media_type"], "track");
            assert_eq!(tagged["item_id"], typed["Tracks"][0]["item_id"]);
        }
        assert!(
            load_catalog(&client, &CatalogQuery::RecentlyPlayed { limit: 0 }, true)
                .await
                .unwrap_err()
                .contains("not a track")
        );
        assert_eq!(
            load_catalog(&client, &CatalogQuery::RecentlyPlayed { limit: 2 }, true)
                .await
                .unwrap(),
            CatalogValue::Tracks(Vec::new())
        );
        let requests = worker.join().unwrap();
        for (request, limit) in requests.iter().zip([0, 2, 0, 2]) {
            assert_eq!(
                request["args"],
                json!({"limit":limit,"media_types":["track"]})
            );
        }
    }

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
        load_playlist::<crate::domain::media::Playlist>(
            &client,
            &MediaId("one".into()),
            "library",
            false,
        )
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
