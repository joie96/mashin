//! Shared typed routing for migrated catalog reads; media owners retain their own SQL.
//! All reads use one snapshot and all observations use the enclosing writer transaction.

use super::database::CacheError;
use super::{album_reads, artist_reads, playlist_reads, recommendation_reads, track_reads};
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::UserId;
use rusqlite::Transaction;

pub(crate) struct CatalogReadObservation {
    pub(crate) value: CatalogValue,
    pub(crate) observed_at: i64,
}

// Query routing and atomic response publication.
pub(crate) fn supports_cached_read(query: &CatalogQuery) -> bool {
    matches!(
        query,
        CatalogQuery::ProviderInstances | CatalogQuery::ProviderManifests {}
    ) || artist_reads::supports_artist_read(query)
        || album_reads::supports_album_read(query)
        || track_reads::supports_track_read(query)
        || playlist_reads::supports_playlist_read(query)
        || recommendation_reads::supports_recommendation_read(query)
}

pub(crate) fn write_cached_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    if matches!(query, CatalogQuery::ProviderManifests {}) {
        let CatalogValue::ProviderManifests(manifests) = value else {
            return Err(CacheError::InvalidMediaObservation(
                "provider manifest response required",
            ));
        };
        let stored = manifests
            .iter()
            .cloned()
            .map(super::providers::StoredManifest::from_published_manifest)
            .collect::<Result<Vec<_>, _>>()?;
        super::providers::write_manifests(transaction, user_id, &stored, observed_at)
    } else if matches!(query, CatalogQuery::ProviderInstances) {
        let CatalogValue::ProviderInstances(instances) = value else {
            return Err(CacheError::InvalidMediaObservation(
                "provider instance response required",
            ));
        };
        super::providers::write_instances(transaction, user_id, instances, observed_at)
    } else if recommendation_reads::supports_recommendation_read(query) {
        recommendation_reads::write_recommendation_read(
            transaction,
            query,
            user_id,
            value,
            observed_at,
        )
    } else if playlist_reads::supports_playlist_read(query) {
        playlist_reads::write_playlist_read(transaction, query, user_id, value, observed_at)
    } else if track_reads::supports_track_read(query) {
        track_reads::write_track_read(transaction, query, user_id, value, observed_at)
    } else if album_reads::supports_album_read(query) {
        album_reads::write_album_read(transaction, query, user_id, value, observed_at)
    } else {
        artist_reads::write_artist_read(transaction, query, user_id, value, observed_at)
    }
}

// Snapshot reads preserve the media owner's exact and partial knowledge conventions.
pub(crate) fn read_cached_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    if matches!(query, CatalogQuery::ProviderManifests {}) {
        Ok(
            super::providers::read_manifest_observation(transaction, user_id)?.map(
                |(stored, observed_at)| CatalogReadObservation {
                    value: CatalogValue::ProviderManifests(
                        stored.into_iter().map(|stored| stored.manifest).collect(),
                    ),
                    observed_at,
                },
            ),
        )
    } else if matches!(query, CatalogQuery::ProviderInstances) {
        super::providers::read_instance_observation(transaction, user_id)
    } else if recommendation_reads::supports_recommendation_read(query) {
        recommendation_reads::read_recommendation_read(transaction, query, user_id)
    } else if playlist_reads::supports_playlist_read(query) {
        playlist_reads::read_playlist_read(transaction, query, user_id)
    } else if track_reads::supports_track_read(query) {
        track_reads::read_track_read(transaction, query, user_id)
    } else if album_reads::supports_album_read(query) {
        album_reads::read_album_read(transaction, query, user_id)
    } else {
        artist_reads::read_artist_read(transaction, query, user_id)
    }
}

pub(crate) fn read_partial_cached(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogValue>, CacheError> {
    if matches!(
        query,
        CatalogQuery::ProviderInstances | CatalogQuery::ProviderManifests {}
    ) {
        Ok(None)
    } else if recommendation_reads::supports_recommendation_read(query) {
        Ok(None)
    } else if playlist_reads::supports_playlist_read(query) {
        playlist_reads::read_partial_playlist(transaction, query, user_id)
    } else if track_reads::supports_track_read(query) {
        track_reads::read_partial_track(transaction, query, user_id)
    } else if album_reads::supports_album_read(query) {
        album_reads::read_partial_album(transaction, query, user_id)
    } else {
        artist_reads::read_partial_artist(transaction, query, user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identifiers::MediaId;
    use crate::domain::media::{MediaItem, MediaType, MusicAssistantSearchResults};
    use serde_json::json;

    // Repeated responses preserve occurrence rows while freshness and membership remain authoritative.
    #[test]
    fn result_lists_write_only_changes_and_preserve_order_duplicates_and_empty_results() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        for kind in ["artist", "album", "track", "playlist"] {
            let media_type: MediaType = serde_json::from_value(json!(kind)).unwrap();
            let mut query = CatalogQuery::Search {
                query: "test".into(),
                media_type,
                limit: 50,
            };
            let value = |identities: &[&str], is_library: bool| {
                let mut results = MusicAssistantSearchResults::default();
                for identity in identities {
                    let item: MediaItem = serde_json::from_value(json!({
                        "media_type":kind,"provider":"library","item_id":identity,"name":identity
                    }))
                    .unwrap();
                    match item {
                        MediaItem::Artist(item) => results.artists.push(item),
                        MediaItem::Album(item) => results.albums.push(item),
                        MediaItem::Track(item) => results.tracks.push(item),
                        MediaItem::Playlist(item) => results.playlists.push(item),
                        _ => panic!("unsupported result fixture"),
                    }
                }
                if is_library {
                    match media_type {
                        MediaType::Artist => CatalogValue::Artists(results.artists),
                        MediaType::Album => CatalogValue::Albums(results.albums),
                        MediaType::Track => CatalogValue::Tracks(results.tracks),
                        MediaType::Playlist => CatalogValue::Playlists(results.playlists),
                        _ => panic!("unsupported library fixture"),
                    }
                } else {
                    CatalogValue::Search(results)
                }
            };
            let library_kind = match media_type {
                MediaType::Artist => "LibraryArtists",
                MediaType::Album => "LibraryAlbums",
                MediaType::Track => "LibraryTracks",
                MediaType::Playlist => "LibraryPlaylists",
                _ => panic!("unsupported library fixture"),
            };
            let library_query = serde_json::from_value(json!({(library_kind): {
                "favorite":null,"limit":50,"offset":0,"summary":false
            }}))
            .unwrap();
            for (selected_query, is_library) in [(query.clone(), false), (library_query, true)] {
                query = selected_query;
                for identities in [vec!["a", "b", "a"], vec!["b", "a"], vec!["b", "c"], vec![]] {
                    let expected = value(&identities, is_library);
                    write_cached_read(&transaction, &query, &user, &expected, 100).unwrap();
                    let before = transaction.total_changes();
                    write_cached_read(&transaction, &query, &user, &expected, 100).unwrap();
                    assert_eq!(transaction.total_changes(), before, "identical {kind}");
                    write_cached_read(&transaction, &query, &user, &expected, 101).unwrap();
                    assert_eq!(
                        transaction.total_changes(),
                        before + 1,
                        "freshness only {kind}"
                    );
                    let observed = read_cached_read(&transaction, &query, &user)
                        .unwrap()
                        .unwrap();
                    assert_eq!(observed.value, expected);
                    assert_eq!(observed.observed_at, 101);
                }
            }
            query = CatalogQuery::Search {
                query: "other".into(),
                media_type,
                limit: 50,
            };
            assert!(read_cached_read(&transaction, &query, &user)
                .unwrap()
                .is_none());
        }
        let query = CatalogQuery::RecommendationItems {
            provider: "source".into(),
            item_id: MediaId("folder".into()),
        };
        for identities in [vec!["a", "b", "a"], vec!["b", "a"], vec![]] {
            let expected = CatalogValue::RecommendationItems(identities.into_iter().map(|identity| {
                serde_json::from_value(json!({"media_type":"track","provider":"library","item_id":identity,"name":identity})).unwrap()
            }).collect());
            write_cached_read(&transaction, &query, &user, &expected, 100).unwrap();
            let before = transaction.total_changes();
            write_cached_read(&transaction, &query, &user, &expected, 100).unwrap();
            assert_eq!(transaction.total_changes(), before);
            write_cached_read(&transaction, &query, &user, &expected, 101).unwrap();
            assert_eq!(transaction.total_changes(), before + 1);
            assert_eq!(
                read_cached_read(&transaction, &query, &user)
                    .unwrap()
                    .unwrap()
                    .value,
                expected
            );
        }
        transaction.commit().unwrap();
    }

    #[test]
    fn recommendation_folders_preserve_unchanged_sections_and_replace_changed_occurrences() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let query = CatalogQuery::RecommendationFolders;
        let user = UserId("user".into());
        for identities in [vec!["a", "b", "a"], vec!["b", "a"], vec!["b"], vec![]] {
            let folders = identities.into_iter().map(|identity| {
                serde_json::from_value(json!({"provider":"source","item_id":identity,"name":identity,
                    "subtitle":null,"items":[{"media_type":"track","provider":"library","item_id":identity,"name":identity}]
                })).unwrap()
            }).collect();
            let expected = CatalogValue::RecommendationFolders(folders);
            write_cached_read(&transaction, &query, &user, &expected, 100).unwrap();
            let before = transaction.total_changes();
            write_cached_read(&transaction, &query, &user, &expected, 100).unwrap();
            assert_eq!(transaction.total_changes(), before);
            write_cached_read(&transaction, &query, &user, &expected, 101).unwrap();
            assert_eq!(transaction.total_changes(), before + 1);
            let observed = read_cached_read(&transaction, &query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(observed.value, expected);
            assert_eq!(observed.observed_at, 101);
        }
        transaction.commit().unwrap();
    }
}
