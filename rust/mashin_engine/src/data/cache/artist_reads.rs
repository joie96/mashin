//! Exact observed artist responses, partitioned by user and typed catalog query.
//! Ordered occurrences and observation time commit with canonical artist writes.
//! Response coverage never implies coverage of another page or complete artist metadata.

use rusqlite::{params, OptionalExtension, Transaction};

use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{Artist, FieldKnowledge, MediaType};

use super::artists::{read_artist, write_artist_observation};
use super::database::CacheError;
use super::reads::CatalogReadObservation;

// Schema and atomic response publication.
pub(super) fn initialize_artist_read_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE artist_read_states (
            user_id TEXT NOT NULL REFERENCES users(user_id), query TEXT NOT NULL,
            observed_at INTEGER NOT NULL CHECK(observed_at>=0),
            PRIMARY KEY(user_id,query)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE artist_read_items (
            user_id TEXT NOT NULL, query TEXT NOT NULL, position INTEGER NOT NULL CHECK(position>=0),
            provider TEXT NOT NULL, item_id TEXT NOT NULL,
            PRIMARY KEY(user_id,query,position),
            FOREIGN KEY(user_id,query) REFERENCES artist_read_states(user_id,query) ON DELETE CASCADE,
            FOREIGN KEY(provider,item_id) REFERENCES artists(provider,item_id)
        ) STRICT, WITHOUT ROWID;",
    )?;
    Ok(())
}

pub(crate) fn supports_artist_read(query: &CatalogQuery) -> bool {
    matches!(
        query,
        CatalogQuery::Artist { .. }
            | CatalogQuery::SimilarArtists { .. }
            | CatalogQuery::ProviderItem {
                media_type: MediaType::Artist,
                ..
            }
            | CatalogQuery::LibraryArtists { .. }
            | CatalogQuery::ArtistAlbums { .. }
            | CatalogQuery::ArtistTracks { .. }
            | CatalogQuery::ArtistTopTracks { .. }
            | CatalogQuery::Search {
                media_type: MediaType::Artist,
                ..
            }
    )
}

/// Known headers or bounded library candidates do not prove a fresh exact response.
/// Criteria requiring unobserved server state remain unknown rather than guessed.
pub(crate) fn read_partial_artist(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogValue>, CacheError> {
    use super::artists::{
        read_artist_lookup, ArtistLookupKey, ArtistLookupResult, ArtistReadProjection,
    };
    if let CatalogQuery::LibraryArtists {
        favorite,
        limit,
        offset,
        summary,
        filters,
    } = query
    {
        use super::artist_candidates::{
            read_artist_candidates, ArtistCandidateDirection, ArtistCandidateQuery,
            ArtistCandidateSort,
        };
        use crate::domain::catalog::{
            ArtistLibrarySort, CatalogSortDirection, LibraryGenreFilter, LibraryProviderFilter,
        };
        if filters.reachable_via.is_some()
            || filters.played_only
            || matches!(&filters.genre, Some(LibraryGenreFilter::Genre(_)))
            || matches!(&filters.genre,Some(LibraryGenreFilter::Genres(genres)) if !genres.is_empty())
        {
            return Ok(None);
        }
        let sort = match filters.sort_field {
            Some(ArtistLibrarySort::Name) => ArtistCandidateSort::Name,
            None | Some(ArtistLibrarySort::SortName) => ArtistCandidateSort::SortName,
            _ => return Ok(None),
        };
        let mut selection = ArtistCandidateQuery::new(user_id.clone());
        selection.favorite = *favorite;
        selection.search = filters.search.clone();
        selection.sort = sort;
        selection.artist_type = filters.artist_type;
        selection.album_artists_only = filters.album_artists_only;
        selection.provider_instances = match &filters.provider {
            Some(LibraryProviderFilter::Instance(provider)) => {
                Some(vec![ProviderInstanceId(provider.clone())])
            }
            Some(LibraryProviderFilter::Instances(providers)) if !providers.is_empty() => {
                Some(providers.iter().cloned().map(ProviderInstanceId).collect())
            }
            _ => None,
        };
        selection.direction = if matches!(filters.sort_direction, Some(CatalogSortDirection::Desc))
        {
            ArtistCandidateDirection::Descending
        } else {
            ArtistCandidateDirection::Ascending
        };
        selection.projection = if *summary {
            ArtistReadProjection::Summary
        } else {
            ArtistReadProjection::Detail
        };
        selection.requires_user_library_observation = true;
        if let Some(limit) = std::num::NonZeroU32::new(*limit) {
            selection.limit = limit;
        }
        selection.offset = *offset;
        let candidates = read_artist_candidates(transaction, &selection)?;
        return Ok(
            (!candidates.items.is_empty()).then_some(CatalogValue::Artists(candidates.items))
        );
    }
    let (item_id, provider, is_provider_item) = match query {
        CatalogQuery::Artist { item_id, provider } => (item_id, provider, false),
        CatalogQuery::ProviderItem {
            item_id,
            provider,
            media_type: MediaType::Artist,
        } => (item_id, provider, true),
        _ => return Ok(None),
    };
    let key = if provider == "library" {
        ArtistLookupKey::Library(item_id.clone())
    } else {
        ArtistLookupKey::ProviderInstance {
            provider_instance: ProviderInstanceId(provider.clone()),
            item_id: item_id.clone(),
        }
    };
    let mut result = read_artist_lookup(transaction, &key, user_id, ArtistReadProjection::Detail)?;
    if matches!(result, ArtistLookupResult::Unknown) && provider != "library" {
        result = read_artist_lookup(
            transaction,
            &ArtistLookupKey::ProviderDomain {
                provider_domain: crate::domain::identifiers::ProviderId(provider.clone()),
                item_id: item_id.clone(),
            },
            user_id,
            ArtistReadProjection::Detail,
        )?;
    }
    Ok(match result {
        ArtistLookupResult::Observed(artist) => Some(if is_provider_item {
            CatalogValue::Item(crate::domain::media::MediaItem::Artist(artist))
        } else {
            CatalogValue::Artist(artist)
        }),
        _ => None,
    })
}

/// The caller propagates errors and commits the whole response, never individual occurrences.
pub(crate) fn write_artist_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    if let (CatalogQuery::LibraryArtists { .. }, CatalogValue::Artists(artists)) = (query, value) {
        if artists
            .iter()
            .any(|artist| artist.media.provider.as_str() != "library")
        {
            return Err(CacheError::InvalidMediaObservation(
                "library artist pages require library identities",
            ));
        }
    }
    let artists: &[Artist] = match (query, value) {
        (CatalogQuery::Artist { .. }, CatalogValue::Artist(artist)) => std::slice::from_ref(artist),
        (
            CatalogQuery::ProviderItem {
                media_type: MediaType::Artist,
                ..
            },
            CatalogValue::Item(crate::domain::media::MediaItem::Artist(artist)),
        ) => std::slice::from_ref(artist),
        (CatalogQuery::SimilarArtists { .. }, CatalogValue::Artists(artists)) => artists,
        (CatalogQuery::LibraryArtists { .. }, CatalogValue::Artists(artists)) => artists,
        (
            CatalogQuery::Search {
                media_type: MediaType::Artist,
                ..
            },
            CatalogValue::Search(results),
        ) => &results.artists,
        (CatalogQuery::ArtistAlbums { .. }, CatalogValue::Albums(_))
        | (
            CatalogQuery::ArtistTracks { .. } | CatalogQuery::ArtistTopTracks { .. },
            CatalogValue::Tracks(_),
        ) => &[],
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "artist query and response do not match",
            ))
        }
    };
    let query_key = serde_json::to_string(query)?;
    transaction.execute(
        "INSERT INTO users(user_id) VALUES (?1) ON CONFLICT DO NOTHING",
        [user_id.as_str()],
    )?;
    transaction.execute(
        "INSERT INTO artist_read_states(user_id,query,observed_at) VALUES (?1,?2,?3)
        ON CONFLICT(user_id,query) DO UPDATE SET observed_at=excluded.observed_at
        WHERE observed_at IS NOT excluded.observed_at",
        params![user_id.as_str(), query_key, observed_at],
    )?;
    if matches!(
        query,
        CatalogQuery::ArtistAlbums { .. }
            | CatalogQuery::ArtistTracks { .. }
            | CatalogQuery::ArtistTopTracks { .. }
    ) {
        return super::catalog_sections::write_catalog_section(transaction, query, user_id, value);
    }
    transaction.execute(
        "DELETE FROM artist_read_items WHERE user_id=?1 AND query=?2 AND position>=?3",
        params![user_id.as_str(), query_key, artists.len() as i64],
    )?;
    let mut statement = transaction.prepare_cached(
        "INSERT INTO artist_read_items(user_id,query,position,provider,item_id) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(user_id,query,position) DO UPDATE SET provider=excluded.provider,item_id=excluded.item_id
         WHERE (provider,item_id) IS NOT (excluded.provider,excluded.item_id)",
    )?;
    for (position, artist) in artists.iter().enumerate() {
        let membership = if matches!(query, CatalogQuery::LibraryArtists { .. }) {
            FieldKnowledge::Known(true)
        } else {
            FieldKnowledge::Unknown
        };
        write_artist_observation(transaction, artist, user_id, membership)?;
        statement.execute(
            params![user_id.as_str(),query_key,position as i64,artist.media.provider.as_str(),artist.media.item_id.as_str()])?;
    }
    Ok(())
}

// Snapshot reads hydrate only identities in the exact observed response.
/// Confirmation retires artist favorite pages and exact responses containing this identity.
pub(crate) fn commit_confirmed_artist(
    transaction: &Transaction<'_>,
    artist: &Artist,
    user_id: &UserId,
) -> Result<(), CacheError> {
    write_artist_observation(transaction, artist, user_id, FieldKnowledge::Unknown)?;
    transaction.execute(
        "DELETE FROM artist_read_states AS state WHERE state.user_id=?1 AND (
            json_type(state.query,'$.LibraryArtists') IS NOT NULL OR EXISTS(
                SELECT 1 FROM artist_read_items item WHERE item.user_id=state.user_id AND item.query=state.query
                AND item.provider=?2 AND item.item_id=?3))",
        params![user_id.as_str(),artist.media.provider.as_str(),artist.media.item_id.as_str()],
    )?;
    Ok(())
}

pub(crate) fn read_artist_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    if !supports_artist_read(query) {
        return Err(CacheError::InvalidMediaObservation(
            "query is not an artist read",
        ));
    }
    let query_key = serde_json::to_string(query)?;
    let observed_at = transaction
        .query_row(
            "SELECT observed_at FROM artist_read_states WHERE user_id=?1 AND query=?2",
            params![user_id.as_str(), query_key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(observed_at) = observed_at else {
        return Ok(None);
    };
    if matches!(
        query,
        CatalogQuery::ArtistAlbums { .. }
            | CatalogQuery::ArtistTracks { .. }
            | CatalogQuery::ArtistTopTracks { .. }
    ) {
        return Ok(Some(CatalogReadObservation {
            value: super::catalog_sections::read_catalog_section(transaction, query, user_id)?,
            observed_at,
        }));
    }
    let identities = transaction.prepare("SELECT provider,item_id FROM artist_read_items WHERE user_id=?1 AND query=?2 ORDER BY position")?
        .query_map(params![user_id.as_str(),query_key],|row|Ok((ProviderInstanceId(row.get(0)?),MediaId(row.get(1)?))))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let artists = identities
        .into_iter()
        .map(|(provider, item_id)| {
            let artist = if matches!(query, CatalogQuery::LibraryArtists { summary: true, .. }) {
                super::artists::read_artist_summary(transaction, &provider, &item_id, user_id)?
            } else {
                read_artist(transaction, &provider, &item_id, user_id)?
            };
            artist.ok_or(CacheError::InvalidMediaObservation(
                "observed artist is missing",
            ))
        })
        .collect::<Result<Vec<_>, CacheError>>()?;
    let value = match query {
        CatalogQuery::Artist { .. }
        | CatalogQuery::ProviderItem {
            media_type: MediaType::Artist,
            ..
        } => {
            if artists.len() != 1 {
                return Err(CacheError::InvalidMediaObservation(
                    "detail response must contain one artist",
                ));
            }
            let artist = artists.into_iter().next().unwrap();
            if matches!(query, CatalogQuery::ProviderItem { .. }) {
                CatalogValue::Item(crate::domain::media::MediaItem::Artist(artist))
            } else {
                CatalogValue::Artist(artist)
            }
        }
        CatalogQuery::SimilarArtists { .. } | CatalogQuery::LibraryArtists { .. } => {
            CatalogValue::Artists(artists)
        }
        CatalogQuery::Search { .. } => {
            let mut results = crate::domain::media::MusicAssistantSearchResults::default();
            results.artists = artists;
            CatalogValue::Search(results)
        }
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "query is not an artist read",
            ))
        }
    };
    Ok(Some(CatalogReadObservation { value, observed_at }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // Isolated canonical responses.
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
    fn query(limit: u32) -> CatalogQuery {
        CatalogQuery::SimilarArtists {
            provider: "source-a".to_owned(),
            item_id: MediaId("007".to_owned()),
            limit,
        }
    }
    fn artist(item_id: &str) -> Artist {
        serde_json::from_value(
            serde_json::json!({"provider":"source-a","item_id":item_id,"name":item_id}),
        )
        .unwrap()
    }

    // Coverage, order, partitioning and atomic failure.
    #[test]
    fn artist_library_partial_filters_preserve_unknown_values_and_supported_parameter_semantics() {
        use crate::domain::catalog::{
            ArtistLibraryFilters, ArtistLibrarySort, LibraryArtistType, LibraryGenreFilter,
            LibraryProviderFilter,
        };
        let mut connection = database();
        let user = UserId("user".into());
        let all = CatalogQuery::LibraryArtists {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
            filters: ArtistLibraryFilters::default(),
        };
        let values = [
            serde_json::json!({"provider":"library","item_id":"liked","name":"Liked","favorite":true,"artist_type":"singer"}),
            serde_json::json!({"provider":"library","item_id":"disliked","name":"Disliked","favorite":false,"artist_type":"singer","provider_mappings":[
                {"provider_domain":"source","provider_instance":"member","item_id":"p","in_library":true},
                {"provider_domain":"source","provider_instance":"playable","item_id":"p","in_library":false,"available":true}]}),
            serde_json::json!({"provider":"library","item_id":"unrated","name":"Unrated","favorite":null,"artist_type":"unknown"}),
            serde_json::json!({"provider":"library","item_id":"unknown","name":"Unknown"}),
        ];
        let transaction = connection.transaction().unwrap();
        write_artist_read(
            &transaction,
            &all,
            &user,
            &CatalogValue::Artists(
                values
                    .into_iter()
                    .map(|value| serde_json::from_value(value).unwrap())
                    .collect(),
            ),
            100,
        )
        .unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let make_query = |favorite, filters| CatalogQuery::LibraryArtists {
            favorite,
            limit: 0,
            offset: 0,
            summary: true,
            filters,
        };
        let supported = ArtistLibraryFilters {
            provider: Some(LibraryProviderFilter::Instance("member".into())),
            artist_type: Some(LibraryArtistType::Singer),
            ..Default::default()
        };
        let CatalogValue::Artists(items) =
            read_partial_artist(&transaction, &make_query(Some(false), supported), &user)
                .unwrap()
                .unwrap()
        else {
            panic!("expected artists")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media.item_id.as_str(), "disliked");
        let CatalogValue::Artists(items) = read_partial_artist(
            &transaction,
            &make_query(Some(false), ArtistLibraryFilters::default()),
            &user,
        )
        .unwrap()
        .unwrap() else {
            panic!("expected artists")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media.item_id.as_str(), "disliked");
        let empty_filters = ArtistLibraryFilters {
            provider: Some(LibraryProviderFilter::Instances(Vec::new())),
            genre: Some(LibraryGenreFilter::Genres(Vec::new())),
            ..Default::default()
        };
        let CatalogValue::Artists(items) =
            read_partial_artist(&transaction, &make_query(None, empty_filters), &user)
                .unwrap()
                .unwrap()
        else {
            panic!("expected artists")
        };
        assert_eq!(items.len(), 4);
        let unknown_type = ArtistLibraryFilters {
            artist_type: Some(LibraryArtistType::Unknown),
            ..Default::default()
        };
        let CatalogValue::Artists(items) =
            read_partial_artist(&transaction, &make_query(None, unknown_type), &user)
                .unwrap()
                .unwrap()
        else {
            panic!("expected artists")
        };
        assert_eq!(items[0].media.item_id.as_str(), "unrated");
        assert_eq!(items.len(), 1);
        for sort in [
            ArtistLibrarySort::TimestampAdded,
            ArtistLibrarySort::TimestampModified,
            ArtistLibrarySort::LastPlayed,
            ArtistLibrarySort::PlayCount,
            ArtistLibrarySort::FavoriteTimestamp,
            ArtistLibrarySort::Random,
            ArtistLibrarySort::RandomPlayCount,
        ] {
            let filters = ArtistLibraryFilters {
                sort_field: Some(sort),
                ..Default::default()
            };
            assert!(
                read_partial_artist(&transaction, &make_query(None, filters), &user)
                    .unwrap()
                    .is_none()
            );
        }
        for filters in [
            ArtistLibraryFilters {
                played_only: true,
                ..Default::default()
            },
            ArtistLibraryFilters {
                genre: Some(LibraryGenreFilter::Genre(7)),
                ..Default::default()
            },
            ArtistLibraryFilters {
                reachable_via: Some(Vec::new()),
                ..Default::default()
            },
            ArtistLibraryFilters {
                reachable_via: Some(vec!["playable".into()]),
                ..Default::default()
            },
            ArtistLibraryFilters {
                provider: Some(LibraryProviderFilter::Instance("playable".into())),
                ..Default::default()
            },
            ArtistLibraryFilters {
                search: Some("missing".into()),
                ..Default::default()
            },
        ] {
            assert!(
                read_partial_artist(&transaction, &make_query(None, filters), &user)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(read_partial_artist(
            &transaction,
            &make_query(None, ArtistLibraryFilters::default()),
            &UserId("other".into())
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn artist_library_partial_album_artists_require_user_observed_library_albums_but_not_other_roles(
    ) {
        use crate::domain::catalog::{ArtistLibraryFilters, LibraryArtistType};
        let mut connection = database();
        let user = UserId("user".into());
        let all = CatalogQuery::LibraryArtists {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
            filters: ArtistLibraryFilters::default(),
        };
        let artists=[("singer","singer"),("author","author"),("external-only","singer")].into_iter().map(|(item_id,role)|
            serde_json::from_value(serde_json::json!({"provider":"library","item_id":item_id,"name":item_id,"artist_type":role})).unwrap()).collect();
        let transaction = connection.transaction().unwrap();
        write_artist_read(
            &transaction,
            &all,
            &user,
            &CatalogValue::Artists(artists),
            100,
        )
        .unwrap();
        let albums_query = CatalogQuery::ArtistAlbums {
            provider: "library".into(),
            item_id: MediaId("singer".into()),
        };
        let album=serde_json::from_value(serde_json::json!({"provider":"library","item_id":"007","name":"Album","artists":[{"provider":"library","item_id":"singer","name":"Singer"}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007","in_library":true}]})).unwrap();
        write_artist_read(
            &transaction,
            &albums_query,
            &UserId("other".into()),
            &CatalogValue::Albums(vec![album]),
            200,
        )
        .unwrap();
        transaction.commit().unwrap();
        let filtered = CatalogQuery::LibraryArtists {
            favorite: None,
            limit: 20,
            offset: 0,
            summary: true,
            filters: ArtistLibraryFilters {
                album_artists_only: true,
                ..Default::default()
            },
        };
        let transaction = connection.transaction().unwrap();
        assert!(read_partial_artist(&transaction, &filtered, &user)
            .unwrap()
            .is_none());
        transaction.commit().unwrap();
        let album = serde_json::from_value(
            serde_json::json!({"provider":"library","item_id":"007","name":"Album"}),
        )
        .unwrap();
        let external=serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"007","name":"External","artists":[{"provider":"library","item_id":"external-only","name":"External Artist"}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007","in_library":true}]})).unwrap();
        let transaction = connection.transaction().unwrap();
        write_artist_read(
            &transaction,
            &albums_query,
            &user,
            &CatalogValue::Albums(vec![album, external]),
            300,
        )
        .unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let CatalogValue::Artists(items) = read_partial_artist(&transaction, &filtered, &user)
            .unwrap()
            .unwrap()
        else {
            panic!("expected artists")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media.item_id.as_str(), "singer");
        let author = CatalogQuery::LibraryArtists {
            favorite: None,
            limit: 20,
            offset: 0,
            summary: true,
            filters: ArtistLibraryFilters {
                album_artists_only: true,
                artist_type: Some(LibraryArtistType::Author),
                ..Default::default()
            },
        };
        let CatalogValue::Artists(items) = read_partial_artist(&transaction, &author, &user)
            .unwrap()
            .unwrap()
        else {
            panic!("expected artists")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media.item_id.as_str(), "author");
    }

    #[test]
    fn artist_library_partial_selects_user_observed_candidates_before_hydration() {
        use crate::domain::catalog::{
            ArtistLibraryFilters, ArtistLibrarySort, CatalogSortDirection,
        };
        let mut connection = database();
        let user = UserId("user".into());
        let all = CatalogQuery::LibraryArtists {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
            filters: ArtistLibraryFilters::default(),
        };
        let items = [("a", "Alpha Needle", "z"), ("b", "Beta Needle", "a"), ("excluded", "Unrelated", "u")]
            .into_iter().map(|(item_id,name,sort_name)|serde_json::from_value(serde_json::json!({"provider":"library","item_id":item_id,"name":name,"sort_name":sort_name,"metadata":{"description":"Detail"}})).unwrap()).collect();
        let transaction = connection.transaction().unwrap();
        write_artist_read(
            &transaction,
            &all,
            &user,
            &CatalogValue::Artists(items),
            100,
        )
        .unwrap();
        let foreign=serde_json::from_value(serde_json::json!({"provider":"library","item_id":"foreign","name":"Z Foreign Needle","metadata":{"description":"Private"}})).unwrap();
        write_artist_read(
            &transaction,
            &all,
            &UserId("other".into()),
            &CatalogValue::Artists(vec![foreign]),
            200,
        )
        .unwrap();
        transaction
            .execute(
                "UPDATE artists SET metadata='{\"description\":[]}' WHERE item_id='excluded'",
                [],
            )
            .unwrap();
        transaction.commit().unwrap();
        let query = CatalogQuery::LibraryArtists {
            favorite: None,
            limit: 1,
            offset: 0,
            summary: false,
            filters: ArtistLibraryFilters {
                search: Some("Needle".into()),
                sort_field: Some(ArtistLibrarySort::Name),
                sort_direction: Some(CatalogSortDirection::Desc),
                ..Default::default()
            },
        };
        let transaction = connection.transaction().unwrap();
        assert!(read_artist_read(&transaction, &query, &user)
            .unwrap()
            .is_none());
        let CatalogValue::Artists(items) = read_partial_artist(&transaction, &query, &user)
            .unwrap()
            .expect("known preliminary artists")
        else {
            panic!("expected artists")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media.item_id.as_str(), "b");
        assert_eq!(
            items[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Detail")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_reads_parameter_matrix_preserves_remote_arguments_and_exact_empty_coverage() {
        use crate::data::music_assistant::client::tests::server_with_request_handler;
        use crate::domain::catalog::ArtistLibraryFilters;
        use serde_json::json;
        let inputs = vec![
            json!({"search":"Beyonce %_","provider":"source-a","genre":7,"played_only":true}),
            json!({"provider":["source-a","source-b"],"genre":[7,8],"album_artists_only":true,"artist_type":"singer"}),
            json!({"provider":[],"genre":[],"reachable_via":[]}),
            json!({"artist_type":"author","reachable_via":["source-b"],"sort_field":"name","sort_direction":"desc"}),
            json!({"artist_type":"narrator","sort_field":"timestamp_added"}),
            json!({"artist_type":"unknown","sort_field":"timestamp_modified"}),
            json!({"sort_field":"last_played","sort_direction":"asc"}),
            json!({"sort_field":"play_count"}),
            json!({"sort_field":"favorite_timestamp"}),
            json!({"sort_field":"random","random_seed":42}),
            json!({"sort_field":"random_play_count","random_seed":43}),
            json!({"sort_field":"sort_name","sort_direction":"asc"}),
        ];
        let expected = inputs.clone();
        let mut position = 0;
        let (client, worker) = server_with_request_handler(
            vec![("music/artists/library_items", json!([])); inputs.len()],
            move |request| {
                for (key, value) in expected[position].as_object().unwrap() {
                    assert_eq!(&request["args"][key], value);
                }
                assert_eq!(request["args"]["limit"], 0);
                assert_eq!(request["args"]["offset"], 42);
                assert_eq!(request["args"]["summary"], false);
                position += 1;
            },
        );
        let mut connection = database();
        let user = UserId("user".into());
        for (position, input) in inputs.into_iter().enumerate() {
            let filters: ArtistLibraryFilters = serde_json::from_value(input).unwrap();
            let query = CatalogQuery::LibraryArtists {
                favorite: Some(false),
                limit: 0,
                offset: 42,
                summary: false,
                filters,
            };
            let response =
                crate::data::music_assistant::catalog::load_catalog(&client, &query, false)
                    .await
                    .unwrap();
            let transaction = connection.transaction().unwrap();
            assert!(read_artist_read(&transaction, &query, &user)
                .unwrap()
                .is_none());
            write_artist_read(&transaction, &query, &user, &response, position as i64).unwrap();
            assert_eq!(
                read_artist_read(&transaction, &query, &user)
                    .unwrap()
                    .unwrap()
                    .value,
                CatalogValue::Artists(vec![])
            );
            assert!(
                read_artist_read(&transaction, &query, &UserId("other".into()))
                    .unwrap()
                    .is_none()
            );
            transaction.commit().unwrap();
        }
        worker.join().unwrap();
        assert!(
            serde_json::from_value::<ArtistLibraryFilters>(json!({"order_by":"random"})).is_err()
        );
        assert!(
            serde_json::from_value::<ArtistLibraryFilters>(json!({"sort_field":"duration"}))
                .is_err()
        );
        let original =
            json!({"LibraryArtists":{"favorite":true,"limit":500,"offset":0,"summary":true}});
        let restored: CatalogQuery = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), original);
    }

    #[test]
    fn artist_reads_library_pages_keep_context_membership_and_summary_projection() {
        let mut connection = database();
        let user = UserId("user".into());
        let query = CatalogQuery::LibraryArtists {
            favorite: Some(true),
            limit: 2,
            offset: 0,
            summary: true,
            filters: Default::default(),
        };
        let rich=serde_json::from_value(serde_json::json!({"provider":"library","item_id":"007","name":"Artist","favorite":true,"metadata":{"description":"Detail","images":[]}})).unwrap();
        let transaction = connection.transaction().unwrap();
        write_artist_observation(&transaction, &rich, &user, FieldKnowledge::Unknown).unwrap();
        let summary = serde_json::from_value(
            serde_json::json!({"provider":"library","item_id":"007","name":"Artist"}),
        )
        .unwrap();
        write_artist_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Artists(vec![summary]),
            100,
        )
        .unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let observed = read_artist_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        let CatalogValue::Artists(artists) = observed.value else {
            panic!("expected artists")
        };
        assert!(artists[0]
            .media
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_ref())
            .is_none());
        assert_eq!(artists[0].media.favorite, FieldKnowledge::Known(Some(true)));
        assert_eq!(
            read_artist(
                &transaction,
                &ProviderInstanceId("library".into()),
                &MediaId("007".into()),
                &user
            )
            .unwrap()
            .unwrap(),
            rich
        );
        let membership: bool = transaction
            .query_row(
                "SELECT is_in_library FROM artist_library_entries WHERE item_id='007'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(membership);
        assert!(
            read_artist_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        let next = CatalogQuery::LibraryArtists {
            favorite: Some(true),
            limit: 2,
            offset: 2,
            summary: true,
            filters: Default::default(),
        };
        assert!(read_artist_read(&transaction, &next, &user)
            .unwrap()
            .is_none());
        write_artist_read(
            &transaction,
            &next,
            &user,
            &CatalogValue::Artists(vec![]),
            200,
        )
        .unwrap();
        assert_eq!(
            read_artist_read(&transaction, &next, &user)
                .unwrap()
                .unwrap()
                .value,
            CatalogValue::Artists(vec![])
        );
    }

    #[test]
    fn artist_reads_distinguish_unknown_and_observed_empty_and_keep_query_user_scope() {
        let mut connection = database();
        let user = UserId("user".to_owned());
        let transaction = connection.transaction().unwrap();
        assert!(read_artist_read(&transaction, &query(5), &user)
            .unwrap()
            .is_none());
        write_artist_read(
            &transaction,
            &query(5),
            &user,
            &CatalogValue::Artists(Vec::new()),
            100,
        )
        .unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let result = read_artist_read(&transaction, &query(5), &user)
            .unwrap()
            .unwrap();
        assert_eq!(result.observed_at, 100);
        assert_eq!(result.value, CatalogValue::Artists(Vec::new()));
        assert!(read_artist_read(&transaction, &query(6), &user)
            .unwrap()
            .is_none());
        assert!(
            read_artist_read(&transaction, &query(5), &UserId("other".to_owned()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn artist_reads_preserve_order_occurrences_and_non_library_objects() {
        let mut connection = database();
        let value = CatalogValue::Artists(vec![artist("b"), artist("a"), artist("b")]);
        let user = UserId("user".to_owned());
        let transaction = connection.transaction().unwrap();
        write_artist_read(&transaction, &query(5), &user, &value, 100).unwrap();
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            read_artist_read(&transaction, &query(5), &user)
                .unwrap()
                .unwrap()
                .value,
            value
        );
        let memberships: i64 = transaction
            .query_row("SELECT COUNT(*) FROM artist_library_entries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(memberships, 0);
    }

    #[test]
    fn artist_reads_failed_later_item_rolls_back_response_time_and_media() {
        let mut connection = database();
        let user = UserId("user".to_owned());
        let previous = CatalogValue::Artists(vec![artist("previous")]);
        let transaction = connection.transaction().unwrap();
        write_artist_read(&transaction, &query(5), &user, &previous, 100).unwrap();
        transaction.commit().unwrap();
        {
            let transaction = connection.transaction().unwrap();
            let invalid:Artist=serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"bad","name":"Bad","external_ids":[["discogs"]]})).unwrap();
            assert!(write_artist_read(
                &transaction,
                &query(5),
                &user,
                &CatalogValue::Artists(vec![artist("new"), invalid]),
                200
            )
            .is_err());
        }
        let transaction = connection.transaction().unwrap();
        let result = read_artist_read(&transaction, &query(5), &user)
            .unwrap()
            .unwrap();
        assert_eq!(result.value, previous);
        assert_eq!(result.observed_at, 100);
        let count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM artists WHERE item_id IN ('new','bad')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
