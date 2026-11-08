//! Exact album responses retain user/query coverage independently of canonical media.
//! The caller owns the snapshot or write transaction; selections store only identities.

use rusqlite::{params, OptionalExtension, Transaction};

use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{Album, MediaItem, MediaType, MusicAssistantSearchResults};

use super::albums::{read_album, read_album_summary, write_album_observation};
use super::catalog_sections::{read_catalog_section, write_catalog_section};
use super::database::CacheError;
use super::reads::CatalogReadObservation;

const ALBUM_IDENTITIES_SQL: &str =
    "SELECT provider,item_id FROM album_read_items WHERE user_id=?1 AND query=?2 ORDER BY position";

// Schema and atomic response publication.
pub(super) fn initialize_album_read_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE album_read_states (
            user_id TEXT NOT NULL REFERENCES users(user_id), query TEXT NOT NULL,
            observed_at INTEGER NOT NULL CHECK(observed_at>=0),
            PRIMARY KEY(user_id,query)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE album_read_items (
            user_id TEXT NOT NULL, query TEXT NOT NULL, position INTEGER NOT NULL CHECK(position>=0),
            provider TEXT NOT NULL, item_id TEXT NOT NULL,
            PRIMARY KEY(user_id,query,position),
            FOREIGN KEY(user_id,query) REFERENCES album_read_states(user_id,query) ON DELETE CASCADE,
            FOREIGN KEY(provider,item_id) REFERENCES albums(provider,item_id)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX album_read_items_identity_idx ON album_read_items(user_id,provider,item_id,query);
        CREATE INDEX album_domain_lookup_idx ON album_provider_mappings(provider_domain,item_id,album_provider,album_item_id);",
    )?;
    Ok(())
}

pub(super) fn initialize_album_track_read_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE album_track_read_items (
            user_id TEXT NOT NULL,query TEXT NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),
            provider TEXT NOT NULL,item_id TEXT NOT NULL,album_provider TEXT,album_item_id TEXT,
            album_known INTEGER NOT NULL CHECK(album_known IN (0,1)),
            disc_number INTEGER CHECK(disc_number>=0),track_number INTEGER CHECK(track_number>=0),
            position INTEGER CHECK(position>=0),position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),
            PRIMARY KEY(user_id,query,occurrence),CHECK((album_provider IS NULL)=(album_item_id IS NULL)),
            CHECK(album_known=1 OR album_provider IS NULL),
            FOREIGN KEY(user_id,query) REFERENCES album_read_states(user_id,query) ON DELETE CASCADE,
            FOREIGN KEY(provider,item_id) REFERENCES tracks(provider,item_id),
            FOREIGN KEY(album_provider,album_item_id,provider,item_id)
                REFERENCES album_tracks(album_provider,album_item_id,track_provider,track_item_id)
         ) STRICT, WITHOUT ROWID;",
    )?;
    Ok(())
}

pub(crate) fn write_album_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    let albums: &[Album] = match (query, value) {
        (CatalogQuery::LibraryAlbums { .. }, CatalogValue::Albums(albums)) => {
            if albums
                .iter()
                .any(|album| album.media.provider.as_str() != "library")
            {
                return Err(CacheError::InvalidMediaObservation(
                    "library album pages require library identities",
                ));
            }
            albums
        }
        (CatalogQuery::AlbumTracks { .. }, CatalogValue::Tracks(_)) => &[],
        (CatalogQuery::Album { .. }, CatalogValue::Album(album)) => std::slice::from_ref(album),
        (
            CatalogQuery::ProviderItem {
                media_type: MediaType::Album,
                ..
            },
            CatalogValue::Item(MediaItem::Album(album)),
        ) => std::slice::from_ref(album),
        (
            CatalogQuery::Search {
                media_type: MediaType::Album,
                ..
            },
            CatalogValue::Search(results),
        ) if results.artists.is_empty()
            && results.tracks.is_empty()
            && results.playlists.is_empty()
            && results.radio.is_empty() =>
        {
            &results.albums
        }
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "album query and response do not match",
            ))
        }
    };
    let query_key = serde_json::to_string(query)?;
    transaction.execute(
        "INSERT INTO users(user_id) VALUES (?1) ON CONFLICT DO NOTHING",
        [user_id.as_str()],
    )?;
    transaction.execute(
        "INSERT INTO album_read_states(user_id,query,observed_at) VALUES (?1,?2,?3)
         ON CONFLICT(user_id,query) DO UPDATE SET observed_at=excluded.observed_at",
        params![user_id.as_str(), query_key, observed_at],
    )?;
    transaction.execute(
        "DELETE FROM album_read_items WHERE user_id=?1 AND query=?2",
        params![user_id.as_str(), query_key],
    )?;
    if matches!(query, CatalogQuery::AlbumTracks { .. }) {
        return write_catalog_section(transaction, query, user_id, value);
    }
    for (position, album) in albums.iter().enumerate() {
        write_album_observation(transaction, album, user_id)?;
        transaction.execute(
            "INSERT INTO album_read_items(user_id,query,position,provider,item_id) VALUES (?1,?2,?3,?4,?5)",
            params![user_id.as_str(),query_key,position as i64,album.media.provider.as_str(),album.media.item_id.as_str()],
        )?;
    }
    Ok(())
}

/// Confirmation retires album library pages and exact responses containing this identity.
pub(crate) fn commit_confirmed_album(
    transaction: &Transaction<'_>,
    album: &Album,
    user_id: &UserId,
) -> Result<(), CacheError> {
    write_album_observation(transaction, album, user_id)?;
    transaction.execute(
        "DELETE FROM album_read_states AS state WHERE state.user_id=?1 AND (
            json_type(state.query,'$.LibraryAlbums') IS NOT NULL OR EXISTS(
                SELECT 1 FROM album_read_items item WHERE item.user_id=state.user_id AND item.query=state.query
                AND ((item.provider=?2 AND item.item_id=?3) OR EXISTS(
                    SELECT 1 FROM album_provider_mappings mapping
                    WHERE mapping.album_provider=?2 AND mapping.album_item_id=?3
                    AND mapping.item_id=item.item_id
                    AND (mapping.provider_instance=item.provider OR mapping.provider_domain=item.provider)))))",
        params![user_id.as_str(),album.media.provider.as_str(),album.media.item_id.as_str()],
    )?;
    Ok(())
}

// Snapshot reads select identities before hydrating any album or artist details.
pub(crate) fn supports_album_read(query: &CatalogQuery) -> bool {
    matches!(
        query,
        CatalogQuery::Album { .. }
            | CatalogQuery::LibraryAlbums { .. }
            | CatalogQuery::AlbumTracks { .. }
            | CatalogQuery::ProviderItem {
                media_type: MediaType::Album,
                ..
            }
            | CatalogQuery::Search {
                media_type: MediaType::Album,
                ..
            }
    )
}

pub(crate) fn read_album_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    if !supports_album_read(query) {
        return Err(CacheError::InvalidMediaObservation(
            "query is not an album read",
        ));
    }
    let query_key = serde_json::to_string(query)?;
    let observed_at = transaction
        .query_row(
            "SELECT observed_at FROM album_read_states WHERE user_id=?1 AND query=?2",
            params![user_id.as_str(), query_key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(observed_at) = observed_at else {
        return Ok(None);
    };
    if matches!(query, CatalogQuery::AlbumTracks { .. }) {
        return Ok(Some(CatalogReadObservation {
            value: read_catalog_section(transaction, query, user_id)?,
            observed_at,
        }));
    }
    let identities = transaction
        .prepare(ALBUM_IDENTITIES_SQL)?
        .query_map(params![user_id.as_str(), query_key], |row| {
            Ok((ProviderInstanceId(row.get(0)?), MediaId(row.get(1)?)))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let albums = identities
        .into_iter()
        .map(|(provider, item_id)| {
            let album = if matches!(query, CatalogQuery::LibraryAlbums { summary: true, .. }) {
                read_album_summary(transaction, &provider, &item_id, user_id)?
            } else {
                read_album(transaction, &provider, &item_id, user_id)?
            };
            album.ok_or(CacheError::InvalidMediaObservation(
                "observed album is missing",
            ))
        })
        .collect::<Result<Vec<_>, CacheError>>()?;
    let value = match query {
        CatalogQuery::LibraryAlbums { .. } => CatalogValue::Albums(albums),
        CatalogQuery::Search { .. } => CatalogValue::Search(MusicAssistantSearchResults {
            albums,
            ..Default::default()
        }),
        _ => {
            if albums.len() != 1 {
                return Err(CacheError::InvalidMediaObservation(
                    "album detail response must contain one item",
                ));
            }
            let album = albums.into_iter().next().unwrap();
            if matches!(query, CatalogQuery::ProviderItem { .. }) {
                CatalogValue::Item(MediaItem::Album(album))
            } else {
                CatalogValue::Album(album)
            }
        }
    };
    Ok(Some(CatalogReadObservation { value, observed_at }))
}

/// Known canonical metadata does not prove an exact detail response or fresh provider state.
pub(crate) fn read_partial_album(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogValue>, CacheError> {
    if let CatalogQuery::LibraryAlbums {
        favorite,
        limit,
        offset,
        summary,
        filters,
    } = query
    {
        use super::album_candidates::{
            read_album_candidates, AlbumCandidateKnowledge, AlbumCandidateQuery, AlbumCandidateSort,
        };
        use crate::domain::catalog::{
            AlbumLibrarySort, CatalogSortDirection, LibraryGenreFilter, LibraryProviderFilter,
        };
        if filters.reachable_via.is_some()
            || filters.played_only
            || matches!(&filters.genre, Some(LibraryGenreFilter::Genre(_)))
            || matches!(&filters.genre,Some(LibraryGenreFilter::Genres(genres)) if !genres.is_empty())
        {
            return Ok(None);
        }
        let sort = match filters.sort_field {
            None | Some(AlbumLibrarySort::SortName) => AlbumCandidateSort::SortName,
            Some(AlbumLibrarySort::Name) => AlbumCandidateSort::Name,
            Some(AlbumLibrarySort::Year) => AlbumCandidateSort::Year,
            _ => return Ok(None),
        };
        let mut selection = AlbumCandidateQuery::new(user_id.clone());
        selection.favorite = *favorite;
        selection.search = filters.search.clone();
        selection.album_types = filters.album_types.clone();
        selection.provider_instances = match &filters.provider {
            Some(LibraryProviderFilter::Instance(provider)) => {
                Some(vec![ProviderInstanceId(provider.clone())])
            }
            Some(LibraryProviderFilter::Instances(providers)) if !providers.is_empty() => {
                Some(providers.iter().cloned().map(ProviderInstanceId).collect())
            }
            _ => None,
        };
        selection.sort = sort;
        selection.direction =
            filters
                .sort_direction
                .unwrap_or(if matches!(sort, AlbumCandidateSort::Year) {
                    CatalogSortDirection::Desc
                } else {
                    CatalogSortDirection::Asc
                });
        selection.is_summary = *summary;
        if let Some(limit) = std::num::NonZeroU32::new(*limit) {
            selection.limit = limit;
        }
        selection.offset = *offset;
        let candidates = read_album_candidates(transaction, &selection)?;
        return Ok(
            matches!(candidates.knowledge, AlbumCandidateKnowledge::Partial)
                .then_some(CatalogValue::Albums(candidates.items)),
        );
    }
    let (item_id, provider) = match query {
        CatalogQuery::Album { item_id, provider }
        | CatalogQuery::ProviderItem {
            item_id,
            provider,
            media_type: MediaType::Album,
        } => (item_id, provider),
        _ => return Ok(None),
    };
    let mut identity = (ProviderInstanceId(provider.clone()), item_id.clone());
    if provider != "library" {
        let libraries = transaction.prepare(
            "SELECT DISTINCT album_provider,album_item_id FROM album_provider_mappings
             WHERE album_provider='library' AND item_id=?2 AND (provider_instance=?1 OR provider_domain=?1) LIMIT 2",
        )?.query_map(params![provider,item_id.as_str()], |row| Ok((ProviderInstanceId(row.get(0)?),MediaId(row.get(1)?))))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if libraries.len() > 1 {
            return Ok(None);
        }
        if let Some(library) = libraries.into_iter().next() {
            identity = library;
        }
    }
    Ok(
        read_album(transaction, &identity.0, &identity.1, user_id)?.map(|album| {
            if matches!(query, CatalogQuery::ProviderItem { .. }) {
                CatalogValue::Item(MediaItem::Album(album))
            } else {
                CatalogValue::Album(album)
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::media::FieldKnowledge;
    use rusqlite::Connection;
    use serde_json::json;

    // Isolated response and canonical-media fixtures.
    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        connection
    }
    fn search(limit: u32) -> CatalogQuery {
        CatalogQuery::Search {
            query: "needle".into(),
            media_type: MediaType::Album,
            limit,
        }
    }
    #[test]
    fn album_tracks_without_an_observed_response_are_unknown() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let query = CatalogQuery::AlbumTracks {
            provider: "source-a".into(),
            item_id: MediaId("007".into()),
        };
        let user = UserId("user".into());
        assert!(supports_album_read(&query));
        assert!(read_album_read(&transaction, &query, &user)
            .unwrap()
            .is_none());
        assert!(read_partial_album(&transaction, &query, &user)
            .unwrap()
            .is_none());
    }
    fn album(provider: &str, name: &str) -> Album {
        serde_json::from_value(json!({"provider":provider,"item_id":"007","name":name})).unwrap()
    }
    fn response(albums: Vec<Album>) -> CatalogValue {
        CatalogValue::Search(MusicAssistantSearchResults {
            albums,
            ..Default::default()
        })
    }

    // Exact coverage, canonical hydration and transactional failure.
    #[test]
    fn album_confirmed_mutation_retires_only_user_coverage_and_preserves_details() {
        for rating in [Some(true), Some(false), None] {
            let mut connection = database();
            let transaction = connection.transaction().unwrap();
            let user = UserId("user".into());
            let other = UserId("other".into());
            let detail = CatalogQuery::Album {
                provider: "library".into(),
                item_id: MediaId("007".into()),
            };
            let alias_query = CatalogQuery::ProviderItem {
                media_type: MediaType::Album,
                provider: "source-a".into(),
                item_id: MediaId("remote".into()),
            };
            let page = CatalogQuery::LibraryAlbums {
                favorite: Some(true),
                limit: 25,
                offset: 0,
                summary: false,
                filters: Default::default(),
            };
            let observed: Album = serde_json::from_value(json!({
                "provider":"library","item_id":"007","name":"Observed","favorite":true,
                "metadata":{"description":"Retained"},"year":2024,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote","in_library":true}]
            })).unwrap();
            for owner in [&user, &other] {
                let mut item = observed.clone();
                item.media.favorite = FieldKnowledge::Known(Some(owner == &user));
                write_album_read(
                    &transaction,
                    &detail,
                    owner,
                    &CatalogValue::Album(item.clone()),
                    100,
                )
                .unwrap();
                write_album_read(
                    &transaction,
                    &page,
                    owner,
                    &CatalogValue::Albums(vec![item.clone()]),
                    101,
                )
                .unwrap();
                let mut alias = item.clone();
                alias.media.provider = ProviderInstanceId("source-a".into());
                alias.media.item_id = MediaId("remote".into());
                write_album_read(
                    &transaction,
                    &alias_query,
                    owner,
                    &CatalogValue::Item(MediaItem::Album(alias)),
                    102,
                )
                .unwrap();
                write_album_read(&transaction, &search(25), owner, &response(vec![item]), 102)
                    .unwrap();
            }
            let unrelated_query = CatalogQuery::Album {
                provider: "library".into(),
                item_id: MediaId("008".into()),
            };
            let unrelated: Album = serde_json::from_value(json!({
                "provider":"library","item_id":"008","name":"Unrelated","favorite":true
            }))
            .unwrap();
            write_album_read(
                &transaction,
                &unrelated_query,
                &user,
                &CatalogValue::Album(unrelated.clone()),
                103,
            )
            .unwrap();
            let confirmed: Album = serde_json::from_value(json!({
                "provider":"library","item_id":"007","name":"Confirmed","favorite":rating
            }))
            .unwrap();
            commit_confirmed_album(&transaction, &confirmed, &user).unwrap();
            for query in [&detail, &alias_query, &page, &search(25)] {
                assert!(read_album_read(&transaction, query, &user)
                    .unwrap()
                    .is_none());
                assert!(read_album_read(&transaction, query, &other)
                    .unwrap()
                    .is_some());
            }
            let Some(CatalogValue::Album(persisted)) =
                read_partial_album(&transaction, &detail, &user).unwrap()
            else {
                panic!("confirmed canonical album missing")
            };
            assert_eq!(persisted.media.name, "Confirmed");
            assert_eq!(persisted.media.favorite, FieldKnowledge::Known(rating));
            assert_eq!(persisted.year, Some(2024));
            assert_eq!(
                persisted.media.metadata.unwrap().description.as_deref(),
                Some("Retained")
            );
            assert_eq!(persisted.media.provider_mappings.len(), 1);
            let Some(CatalogValue::Item(MediaItem::Album(alias))) =
                read_partial_album(&transaction, &alias_query, &user).unwrap()
            else {
                panic!("confirmed library identity not resolved for provider alias")
            };
            assert_eq!(alias.media.provider.as_str(), "library");
            assert_eq!(alias.media.favorite, FieldKnowledge::Known(rating));
            let Some(CatalogValue::Album(other_album)) =
                read_partial_album(&transaction, &detail, &other).unwrap()
            else {
                panic!("other user's canonical album missing")
            };
            assert_eq!(
                other_album.media.favorite,
                FieldKnowledge::Known(Some(false))
            );
            let retained = read_album_read(&transaction, &unrelated_query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(retained.observed_at, 103);
            assert_eq!(retained.value, CatalogValue::Album(unrelated));
        }
    }

    #[test]
    fn album_library_partial_uses_year_defaults_and_preserves_unobserved_filter_knowledge() {
        use crate::domain::catalog::AlbumLibraryFilters;
        let mut connection = database();
        let snapshot = connection.transaction().unwrap();
        let user = UserId("user".into());
        let page = CatalogQuery::LibraryAlbums {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
            filters: Default::default(),
        };
        let albums:Vec<Album>=serde_json::from_value(json!([
            {"provider":"library","item_id":"old","name":"Old","sort_name":"Old","year":2020,"album_type":"ep","favorite":false,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"old","in_library":true}]},
            {"provider":"library","item_id":"new","name":"New","sort_name":"New","year":2024,"album_type":"ep","favorite":false,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"new","in_library":true}]},
            {"provider":"library","item_id":"null","name":"Null","sort_name":"Null","year":null,"album_type":"ep","favorite":null,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"null","in_library":true}]},
            {"provider":"library","item_id":"unknown","name":"Unknown","sort_name":"Unknown","album_type":"ep",
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"unknown","in_library":true}]}
        ])).unwrap();
        let value = CatalogValue::Albums(albums);
        write_album_read(&snapshot, &page, &user, &value, 100).unwrap();
        let query = CatalogQuery::LibraryAlbums {
            favorite: Some(false),
            limit: 1,
            offset: 0,
            summary: true,
            filters: serde_json::from_value(
                json!({"sort_field":"year","provider":[],"genre":[],"album_types":[]}),
            )
            .unwrap(),
        };
        let Some(CatalogValue::Albums(selected)) =
            read_partial_album(&snapshot, &query, &user).unwrap()
        else {
            panic!("expected partial albums")
        };
        assert_eq!(selected[0].media.item_id.as_str(), "new");
        assert!(read_album_read(&snapshot, &query, &user).unwrap().is_none());
        for input in [
            json!({"genre":7}),
            json!({"genre":[7]}),
            json!({"played_only":true}),
            json!({"reachable_via":[]}),
            json!({"reachable_via":["member"]}),
            json!({"sort_field":"artist_name"}),
            json!({"sort_field":"random","random_seed":42}),
            json!({"sort_field":"last_played"}),
        ] {
            let filters: AlbumLibraryFilters = serde_json::from_value(input).unwrap();
            let query = CatalogQuery::LibraryAlbums {
                favorite: None,
                limit: 25,
                offset: 0,
                summary: false,
                filters,
            };
            assert!(read_partial_album(&snapshot, &query, &user)
                .unwrap()
                .is_none());
            write_album_read(&snapshot, &query, &user, &value, 101).unwrap();
            assert!(read_album_read(&snapshot, &query, &user).unwrap().is_some());
        }
    }

    #[test]
    fn album_library_filters_preserve_default_keys_and_exact_observation_scope() {
        use crate::domain::catalog::AlbumLibraryFilters;
        let old_key =
            json!({"LibraryAlbums":{"favorite":null,"limit":25,"offset":0,"summary":true}});
        let default_query: CatalogQuery = serde_json::from_value(old_key.clone()).unwrap();
        assert_eq!(serde_json::to_value(&default_query).unwrap(), old_key);
        assert_eq!(
            serde_json::to_string(&default_query).unwrap(),
            r#"{"LibraryAlbums":{"favorite":null,"limit":25,"offset":0,"summary":true}}"#
        );
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        let value = CatalogValue::Albums(vec![album("library", "Observed")]);
        write_album_read(&transaction, &default_query, &user, &value, 100).unwrap();
        for (index, input) in [
            json!({"search":"Artist - Album%"}),
            json!({"provider":"source-a"}),
            json!({"provider":["source-a","source-b"]}),
            json!({"provider":[]}),
            json!({"genre":7}),
            json!({"genre":[7,9]}),
            json!({"genre":[]}),
            json!({"played_only":true}),
            json!({"album_types":["album","ep"]}),
            json!({"album_types":[]}),
            json!({"sort_field":"artist_name"}),
            json!({"sort_field":"year"}),
            json!({"sort_direction":"desc"}),
            json!({"reachable_via":[]}),
            json!({"reachable_via":["source-b"]}),
            json!({"random_seed":0}),
        ]
        .into_iter()
        .enumerate()
        {
            let filters: AlbumLibraryFilters = serde_json::from_value(input).unwrap();
            let query = CatalogQuery::LibraryAlbums {
                favorite: None,
                limit: 25,
                offset: 0,
                summary: true,
                filters,
            };
            assert_ne!(query, default_query);
            assert!(read_album_read(&transaction, &query, &user)
                .unwrap()
                .is_none());
            assert!(read_partial_album(&transaction, &query, &user)
                .unwrap()
                .is_none());
            write_album_read(&transaction, &query, &user, &value, 101 + index as i64).unwrap();
            assert_eq!(
                read_album_read(&transaction, &query, &user)
                    .unwrap()
                    .unwrap()
                    .observed_at,
                101 + index as i64
            );
            assert!(
                read_album_read(&transaction, &query, &UserId("other".into()))
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                read_album_read(&transaction, &default_query, &user)
                    .unwrap()
                    .unwrap()
                    .observed_at,
                100
            );
        }
    }

    #[test]
    fn album_library_filters_validate_server_values_and_random_sampling() {
        use crate::domain::catalog::AlbumLibraryFilters;
        for sort in [
            "sort_name",
            "name",
            "timestamp_added",
            "timestamp_modified",
            "last_played",
            "play_count",
            "favorite_timestamp",
            "year",
            "artist_name",
            "random",
            "random_play_count",
        ] {
            let mut filters: AlbumLibraryFilters =
                serde_json::from_value(json!({"sort_field":sort})).unwrap();
            assert_eq!(serde_json::to_value(&filters).unwrap()["sort_field"], sort);
            assert_eq!(
                filters.requires_new_random_sample(),
                sort == "random" || sort == "random_play_count"
            );
            filters.random_seed = Some(-42);
            assert!(!filters.requires_new_random_sample());
        }
        for input in [
            json!({"order_by":"random"}),
            json!({"artist_type":"singer"}),
            json!({"sort_field":"duration"}),
            json!({"album_types":["studio"]}),
            json!({"sort_direction":"sideways"}),
        ] {
            assert!(serde_json::from_value::<AlbumLibraryFilters>(input).is_err());
        }
    }

    #[test]
    fn album_summary_pages_keep_details_and_separate_projection_coverage() {
        let mut connection = database();
        let user = UserId("user".into());
        let summary = CatalogQuery::LibraryAlbums {
            favorite: None,
            limit: 25,
            offset: 0,
            summary: true,
            filters: Default::default(),
        };
        let detail = CatalogQuery::LibraryAlbums {
            favorite: None,
            limit: 25,
            offset: 0,
            summary: false,
            filters: Default::default(),
        };
        let original: Album = serde_json::from_value(json!({"provider":"library","item_id":"007","name":"Detail","year":2024,
            "metadata":{"description":"Keep","images":[{"type":"thumb","provider":"source","path":"cover"}]},
            "external_ids":[["barcode","00123"]],"favorite":true,
            "artists":[{"provider":"library","item_id":"artist","name":"Artist","metadata":{"description":"Biography"}}]})).unwrap();
        let transaction = connection.transaction().unwrap();
        write_album_observation(&transaction, &original, &user).unwrap();
        let incoming: Album = serde_json::from_value(json!({"provider":"library","item_id":"007","name":"Summary","year":null,"metadata":{"images":null}})).unwrap();
        write_album_read(
            &transaction,
            &summary,
            &user,
            &CatalogValue::Albums(vec![incoming]),
            100,
        )
        .unwrap();
        assert!(read_album_read(&transaction, &detail, &user)
            .unwrap()
            .is_none());
        assert!(
            read_album_read(&transaction, &summary, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        let observed = read_album_read(&transaction, &summary, &user)
            .unwrap()
            .unwrap();
        let CatalogValue::Albums(albums) = observed.value else {
            panic!("expected albums")
        };
        let projected = &albums[0];
        assert_eq!(projected.media.name, "Summary");
        assert!(projected.knowledge.year && projected.year.is_none());
        assert_eq!(projected.media.favorite, FieldKnowledge::Known(Some(true)));
        assert!(!projected.media.knowledge.external_ids);
        let metadata = projected.media.metadata.as_ref().unwrap();
        assert!(metadata.knowledge.images && metadata.images.is_empty());
        assert!(!metadata.knowledge.description);
        assert!(!projected.artists[0].media.knowledge.metadata);
        let retained = read_album(
            &transaction,
            &original.media.provider,
            &original.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            retained
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Keep")
        );
        assert_eq!(retained.media.external_ids, original.media.external_ids);
        assert_eq!(
            retained.artists[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Biography")
        );
        write_album_read(
            &transaction,
            &summary,
            &user,
            &CatalogValue::Albums(vec![]),
            101,
        )
        .unwrap();
        assert_eq!(
            read_album_read(&transaction, &summary, &user)
                .unwrap()
                .unwrap()
                .value,
            CatalogValue::Albums(vec![])
        );
        assert!(read_album(
            &transaction,
            &original.media.provider,
            &original.media.item_id,
            &user
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn library_album_pages_preserve_occurrences_user_and_exact_page_coverage() {
        let mut connection = database();
        let user = UserId("user".into());
        let query = CatalogQuery::LibraryAlbums {
            favorite: Some(true),
            limit: 3,
            offset: 0,
            summary: false,
            filters: Default::default(),
        };
        let first = album("library", "First");
        let mut second = album("library", "Second");
        second.media.item_id = MediaId("008".into());
        let value = CatalogValue::Albums(vec![second.clone(), first, second]);
        let transaction = connection.transaction().unwrap();
        assert!(read_album_read(&transaction, &query, &user)
            .unwrap()
            .is_none());
        write_album_read(&transaction, &query, &user, &value, 100).unwrap();
        assert_eq!(
            read_album_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            value
        );
        assert!(
            read_album_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        let next = CatalogQuery::LibraryAlbums {
            favorite: Some(true),
            limit: 3,
            offset: 3,
            summary: false,
            filters: Default::default(),
        };
        assert!(read_album_read(&transaction, &next, &user)
            .unwrap()
            .is_none());
        assert!(read_partial_album(&transaction, &next, &user)
            .unwrap()
            .is_none());
        for favorite in [None, Some(false)] {
            let other = CatalogQuery::LibraryAlbums {
                favorite,
                limit: 3,
                offset: 0,
                summary: false,
                filters: Default::default(),
            };
            assert!(read_album_read(&transaction, &other, &user)
                .unwrap()
                .is_none());
        }
        write_album_read(
            &transaction,
            &next,
            &user,
            &CatalogValue::Albums(vec![]),
            101,
        )
        .unwrap();
        let empty = read_album_read(&transaction, &next, &user)
            .unwrap()
            .unwrap();
        assert_eq!(empty.observed_at, 101);
        assert_eq!(empty.value, CatalogValue::Albums(vec![]));
        assert_eq!(
            read_album_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            value
        );
        let summary = CatalogQuery::LibraryAlbums {
            favorite: Some(true),
            limit: 3,
            offset: 0,
            summary: true,
            filters: Default::default(),
        };
        assert!(supports_album_read(&summary));
        assert!(read_album_read(&transaction, &summary, &user)
            .unwrap()
            .is_none());
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            read_album_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            value
        );
    }

    #[test]
    fn library_album_page_rejects_external_identities_and_rolls_back_failed_updates() {
        let mut connection = database();
        let user = UserId("user".into());
        let query = CatalogQuery::LibraryAlbums {
            favorite: Some(true),
            limit: 500,
            offset: 0,
            summary: false,
            filters: Default::default(),
        };
        let original = CatalogValue::Albums(vec![album("library", "Original")]);
        let transaction = connection.transaction().unwrap();
        write_album_read(&transaction, &query, &user, &original, 100).unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        assert!(write_album_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Albums(vec![album("external", "Wrong")]),
            101
        )
        .is_err());
        let mut invalid = album("library", "Invalid");
        invalid.media.item_id = MediaId("invalid".into());
        invalid.media.external_ids = vec![vec!["malformed".into()]];
        assert!(write_album_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Albums(vec![album("library", "Changed"), invalid]),
            102
        )
        .is_err());
        drop(transaction);
        let transaction = connection.transaction().unwrap();
        let unchanged = read_album_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.observed_at, 100);
        assert_eq!(unchanged.value, original);
    }

    #[test]
    fn album_tracks_preserve_context_duplicates_and_canonical_updates_without_scanning() {
        use super::super::tracks::write_track_observation;
        use crate::domain::media::Track;
        let mut connection = database();
        let user = UserId("user".into());
        let transaction = connection.transaction().unwrap();
        let query = CatalogQuery::AlbumTracks {
            provider: "source-a".into(),
            item_id: MediaId("007".into()),
        };
        let track: Track = serde_json::from_value(
            json!({"provider":"source-b","item_id":"track","name":"Original","duration":125,
            "artists":[{"provider":"source-c","item_id":"artist","name":"Credit"}],
            "album":{"provider":"source-a","item_id":"007","name":"Album A","year":2020},
            "disc_number":1,"track_number":4,"position":null}),
        )
        .unwrap();
        let mut duplicate = track.clone();
        duplicate.disc_number = 2;
        duplicate.track_number = 9;
        duplicate.media.position = Some(17);
        write_album_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(vec![track.clone(), duplicate]),
            100,
        )
        .unwrap();
        let latest: Track = serde_json::from_value(json!({"provider":"source-b","item_id":"track","name":"Latest","duration":321,
            "album":{"provider":"source-d","item_id":"007","name":"Album B"},"disc_number":8,"track_number":20})).unwrap();
        write_track_observation(&transaction, &latest, &user).unwrap();
        let sparse: Track = serde_json::from_value(
            json!({"provider":"source-b","item_id":"track","name":"Refreshed"}),
        )
        .unwrap();
        write_album_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(vec![sparse.clone(), sparse]),
            101,
        )
        .unwrap();
        for position in 0..1000 {
            transaction.execute("INSERT INTO tracks(provider,item_id,name,sort_name_known,uri_known,available_known,metadata,metadata_known,
                position_known,date_added_known,duration_known,artists_known,album_known,image_known,disc_number_known,track_number_known,external_ids_known,provider_mappings_known)
                VALUES('excluded',?1,'Irrelevant',0,0,0,'{\"description\":5}',1,0,0,0,0,0,0,0,0,0,0)", [position.to_string()]).unwrap();
        }
        let observation = read_album_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observation.observed_at, 101);
        let CatalogValue::Tracks(tracks) = observation.value else {
            panic!("tracks expected")
        };
        assert_eq!(tracks.len(), 2);
        assert!(tracks.iter().all(|track| track.media.name == "Refreshed"
            && track.duration == 321
            && track.artists[0].media.provider.as_str() == "source-c"));
        assert!(tracks
            .iter()
            .all(|track| track.album.as_ref().unwrap().media.provider.as_str() == "source-a"));
        assert_eq!((tracks[0].disc_number, tracks[0].track_number), (1, 4));
        assert_eq!((tracks[1].disc_number, tracks[1].track_number), (2, 9));
        assert!(tracks[0].media.knowledge.position && tracks[0].media.position.is_none());
        assert_eq!(tracks[1].media.position, Some(17));
        assert!(
            read_album_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        let other = CatalogQuery::AlbumTracks {
            provider: "source-d".into(),
            item_id: MediaId("007".into()),
        };
        assert!(read_album_read(&transaction, &other, &user)
            .unwrap()
            .is_none());
        assert!(read_partial_album(&transaction, &other, &user)
            .unwrap()
            .is_none());
        let query_key = serde_json::to_string(&query).unwrap();
        let sql = super::super::catalog_sections::section_occurrences_sql(&query).unwrap();
        let plan = transaction
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(params![user.as_str(), query_key], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join(" ");
        assert!(
            plan.contains("PRIMARY KEY") && !plan.contains("TEMP B-TREE"),
            "{plan}"
        );
        write_album_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(vec![]),
            102,
        )
        .unwrap();
        assert_eq!(
            read_album_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            CatalogValue::Tracks(vec![])
        );
    }

    #[test]
    fn album_tracks_failed_response_rolls_back_references_time_and_media() {
        use crate::domain::media::Track;
        let mut connection = database();
        let user = UserId("user".into());
        let query = CatalogQuery::AlbumTracks {
            provider: "source-a".into(),
            item_id: MediaId("007".into()),
        };
        let track: Track = serde_json::from_value(json!({"provider":"source-a","item_id":"track","name":"Old","album":{"provider":"source-a","item_id":"007","name":"Album"}})).unwrap();
        let previous = CatalogValue::Tracks(vec![track.clone()]);
        let transaction = connection.transaction().unwrap();
        write_album_read(&transaction, &query, &user, &previous, 100).unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut updated = track.clone();
        updated.media.name = "New".into();
        let mut invalid = track;
        invalid.media.external_ids = vec![vec!["missing value".into()]];
        assert!(write_album_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(vec![updated, invalid]),
            200
        )
        .is_err());
        transaction.rollback().unwrap();
        let transaction = connection.transaction().unwrap();
        let observation = read_album_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observation.observed_at, 100);
        assert_eq!(observation.value, previous);
    }

    #[test]
    fn album_reads_select_before_unrelated_details_and_use_response_primary_key() {
        let mut connection = database();
        let user = UserId("user".into());
        let transaction = connection.transaction().unwrap();
        let expected = response(vec![album("source-a", "Selected")]);
        write_album_read(&transaction, &search(25), &user, &expected, 100).unwrap();
        for position in 0..1000 {
            transaction.execute("INSERT INTO albums(provider,item_id,name,artists_known,provider_mappings_known,metadata,metadata_known)
                VALUES ('excluded',?1,'Irrelevant',0,0,'{\"description\":5}',1)", [position.to_string()]).unwrap();
        }
        assert_eq!(
            read_album_read(&transaction, &search(25), &user)
                .unwrap()
                .unwrap()
                .value,
            expected
        );
        let plan = transaction
            .prepare(&format!("EXPLAIN QUERY PLAN {ALBUM_IDENTITIES_SQL}"))
            .unwrap()
            .query_map(
                params![user.as_str(), serde_json::to_string(&search(25)).unwrap()],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join(" ");
        assert!(plan.contains("PRIMARY KEY"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }

    #[test]
    fn album_reads_preserve_occurrences_canonical_updates_and_exact_user_scope() {
        let mut connection = database();
        let user = UserId("user".into());
        let transaction = connection.transaction().unwrap();
        assert!(read_album_read(&transaction, &search(25), &user)
            .unwrap()
            .is_none());
        let albums = vec![
            album("source-a", "A"),
            album("source-b", "B"),
            album("source-a", "A"),
        ];
        write_album_read(&transaction, &search(25), &user, &response(albums), 100).unwrap();
        let updated: Album = serde_json::from_value(json!({"provider":"source-a","item_id":"007","name":"Updated","year":2026,"favorite":false,"artists":[{"provider":"source-c","item_id":"007","name":"Credit"}]})).unwrap();
        write_album_observation(&transaction, &updated, &user).unwrap();
        let read = read_album_read(&transaction, &search(25), &user)
            .unwrap()
            .unwrap();
        assert_eq!(read.observed_at, 100);
        let CatalogValue::Search(results) = read.value else {
            panic!("album search expected");
        };
        assert_eq!(
            results
                .albums
                .iter()
                .map(|album| album.media.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Updated", "B", "Updated"]
        );
        assert_eq!(
            results.albums[0].artists[0].media.provider.as_str(),
            "source-c"
        );
        assert_eq!(results.albums[0].year, Some(2026));
        assert!(read_album_read(&transaction, &search(24), &user)
            .unwrap()
            .is_none());
        assert!(
            read_album_read(&transaction, &search(25), &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        write_album_read(&transaction, &search(0), &user, &response(vec![]), 101).unwrap();
        assert_eq!(
            read_album_read(&transaction, &search(0), &user)
                .unwrap()
                .unwrap()
                .value,
            response(vec![])
        );
        let memberships: i64 = transaction
            .query_row("SELECT COUNT(*) FROM artist_library_entries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(memberships, 0);
        transaction.commit().unwrap();
    }

    #[test]
    fn album_reads_partial_lookup_prefers_unique_library_mapping_without_membership_filter() {
        let mut connection = database();
        let user = UserId("user".into());
        let transaction = connection.transaction().unwrap();
        write_album_observation(&transaction, &album("source-a", "External"), &user).unwrap();
        let library: Album = serde_json::from_value(json!({"provider":"library","item_id":"44","name":"Library","provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007","in_library":false}]})).unwrap();
        write_album_observation(&transaction, &library, &user).unwrap();
        let query = CatalogQuery::ProviderItem {
            provider: "source-a".into(),
            item_id: MediaId("007".into()),
            media_type: MediaType::Album,
        };
        assert!(read_album_read(&transaction, &query, &user)
            .unwrap()
            .is_none());
        let Some(CatalogValue::Item(MediaItem::Album(result))) =
            read_partial_album(&transaction, &query, &user).unwrap()
        else {
            panic!("known library album expected");
        };
        assert_eq!(result.media.item_id.as_str(), "44");
        assert_eq!(result.media.provider.as_str(), "library");
        write_album_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Item(MediaItem::Album(library.clone())),
            100,
        )
        .unwrap();
        assert!(read_album_read(&transaction, &query, &user)
            .unwrap()
            .is_some());
        let mut ambiguous = library;
        ambiguous.media.item_id = MediaId("45".into());
        write_album_observation(&transaction, &ambiguous, &user).unwrap();
        assert!(read_partial_album(&transaction, &query, &user)
            .unwrap()
            .is_none());
        transaction.commit().unwrap();
    }

    #[test]
    fn album_reads_failure_rolls_back_media_references_and_observation_time() {
        let mut connection = database();
        let user = UserId("user".into());
        let previous = response(vec![album("source-a", "Old")]);
        let transaction = connection.transaction().unwrap();
        write_album_read(&transaction, &search(25), &user, &previous, 100).unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut invalid = album("source-b", "Invalid");
        invalid.media.external_ids = vec![vec!["missing value".into()]];
        assert!(write_album_read(
            &transaction,
            &search(25),
            &user,
            &response(vec![album("source-a", "Changed"), invalid]),
            200
        )
        .is_err());
        drop(transaction);
        let transaction = connection.transaction().unwrap();
        let stored = read_album_read(&transaction, &search(25), &user)
            .unwrap()
            .unwrap();
        assert_eq!(stored.observed_at, 100);
        assert_eq!(stored.value, previous);
        assert!(read_album(
            &transaction,
            &ProviderInstanceId("source-b".into()),
            &MediaId("007".into()),
            &user
        )
        .unwrap()
        .is_none());
    }
}
