//! Preliminary selections require positive membership and a user's observed library page.
//! The caller owns the snapshot; SQL bounds identities before reconstructing any details.

use std::num::NonZeroU32;

use rusqlite::{params, Transaction};

use crate::domain::catalog::{CatalogSortDirection, LibraryAlbumType};
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::Album;

use super::albums::{read_album, read_album_summary};
use super::database::CacheError;

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum AlbumCandidateSort {
    #[default]
    SortName,
    Name,
    Year,
}

pub(super) struct AlbumCandidateQuery {
    pub(super) user_id: UserId,
    pub(super) favorite: Option<bool>,
    pub(super) search: Option<String>,
    /// Resolved provider subset; an explicit empty subset matches nothing.
    pub(super) provider_instances: Option<Vec<ProviderInstanceId>>,
    pub(super) album_types: Option<Vec<LibraryAlbumType>>,
    pub(super) sort: AlbumCandidateSort,
    pub(super) direction: CatalogSortDirection,
    pub(super) is_summary: bool,
    pub(super) limit: NonZeroU32,
    pub(super) offset: u32,
}

impl AlbumCandidateQuery {
    // Bounded preliminary defaults, independent of server pagination defaults.
    pub(super) fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            favorite: None,
            search: None,
            provider_instances: None,
            album_types: None,
            sort: AlbumCandidateSort::SortName,
            direction: CatalogSortDirection::Asc,
            is_summary: true,
            limit: NonZeroU32::new(500).unwrap(),
            offset: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AlbumCandidateKnowledge {
    Unknown,
    Partial,
}

pub(super) struct AlbumCandidates {
    pub(super) items: Vec<Album>,
    pub(super) knowledge: AlbumCandidateKnowledge,
}

// Expression indexes reuse the existing connection-local media-name normalization.
pub(super) fn initialize_album_query_indexes(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS albums_candidate_name_idx ON albums(provider,mashin_artist_search_key(name),item_id);
         CREATE INDEX IF NOT EXISTS albums_candidate_sort_name_idx ON albums(provider,mashin_artist_search_key(sort_name),item_id);
         CREATE INDEX IF NOT EXISTS albums_candidate_year_idx ON albums(provider,year,item_id);"
    )?;
    Ok(())
}

// Snapshot selection never interprets an empty observed subset as a complete server page.
pub(super) fn read_album_candidates(
    snapshot: &Transaction<'_>,
    query: &AlbumCandidateQuery,
) -> Result<AlbumCandidates, CacheError> {
    let providers = query
        .provider_instances
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let types = query
        .album_types
        .as_ref()
        .filter(|types| !types.is_empty())
        .map(serde_json::to_string)
        .transpose()?;
    let (artist_search, title_search) =
        match query.search.as_deref().filter(|search| !search.is_empty()) {
            Some(search) => match search.split_once(" - ") {
                Some((artist, title)) => (Some(artist), Some(title)),
                None => (None, Some(search)),
            },
            None => (None, None),
        };
    let identities = snapshot
        .prepare(&album_candidate_sql(query))?
        .query_map(
            params![
                query.user_id.as_str(),
                query.favorite,
                providers,
                types,
                query.limit.get(),
                query.offset,
                title_search,
                artist_search
            ],
            |row| Ok((ProviderInstanceId(row.get(0)?), MediaId(row.get(1)?))),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let items = identities
        .into_iter()
        .map(|(provider, item_id)| {
            let album = if query.is_summary {
                read_album_summary(snapshot, &provider, &item_id, &query.user_id)?
            } else {
                read_album(snapshot, &provider, &item_id, &query.user_id)?
            };
            album.ok_or(CacheError::InvalidMediaObservation(
                "selected album is missing",
            ))
        })
        .collect::<Result<Vec<_>, CacheError>>()?;
    let knowledge = if items.is_empty() {
        AlbumCandidateKnowledge::Unknown
    } else {
        AlbumCandidateKnowledge::Partial
    };
    Ok(AlbumCandidates { items, knowledge })
}

// Only typed sort options determine SQL identifiers; all user values remain bound parameters.
fn album_candidate_sql(query: &AlbumCandidateQuery) -> String {
    let (index, column, known) = match query.sort {
        AlbumCandidateSort::Name => (
            "albums_candidate_name_idx",
            "mashin_artist_search_key(album.name)",
            "1",
        ),
        AlbumCandidateSort::SortName => (
            "albums_candidate_sort_name_idx",
            "mashin_artist_search_key(album.sort_name)",
            "album.sort_name_known=1",
        ),
        AlbumCandidateSort::Year => (
            "albums_candidate_year_idx",
            "album.year",
            "album.year_known=1",
        ),
    };
    let direction = match query.direction {
        CatalogSortDirection::Asc => "ASC",
        CatalogSortDirection::Desc => "DESC",
    };
    format!("SELECT album.provider,album.item_id FROM albums album INDEXED BY {index}
        WHERE album.provider='library' AND {known}
        AND EXISTS(SELECT 1 FROM album_read_items observed
            WHERE observed.user_id=?1 AND observed.provider=album.provider AND observed.item_id=album.item_id
            AND json_type(observed.query,'$.LibraryAlbums') IS NOT NULL)
        AND EXISTS(SELECT 1 FROM album_provider_mappings mapping
            WHERE mapping.album_provider=album.provider AND mapping.album_item_id=album.item_id
            AND mapping.in_library=1 AND mapping.in_library_known=1
            AND (?3 IS NULL OR mapping.provider_instance IN (SELECT value FROM json_each(?3))))
        AND (?2 IS NULL OR EXISTS(SELECT 1 FROM album_favorites favorite
            WHERE favorite.user_id=?1 AND favorite.provider=album.provider AND favorite.item_id=album.item_id AND favorite.rating=?2))
        AND (?4 IS NULL OR album.album_type IN (SELECT value FROM json_each(?4)))
        AND (?7 IS NULL OR instr(mashin_artist_search_key(album.name),mashin_artist_search_key(?7))>0)
        AND (?8 IS NULL OR EXISTS(SELECT 1 FROM album_artists credit JOIN artists artist
            ON artist.provider=credit.artist_provider AND artist.item_id=credit.artist_item_id
            WHERE credit.album_provider=album.provider AND credit.album_item_id=album.item_id
            AND instr(mashin_artist_search_key(artist.name),mashin_artist_search_key(?8))>0))
        ORDER BY {column} {direction},album.item_id {direction} LIMIT ?5 OFFSET ?6")
}

#[cfg(test)]
mod tests {
    use super::super::album_reads::write_album_read;
    use super::*;
    use crate::domain::catalog::{CatalogQuery, CatalogValue};
    use serde_json::json;

    // Isolated snapshot fixtures and identity assertions.
    fn database() -> rusqlite::Connection {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        connection
    }

    fn identities(candidates: AlbumCandidates) -> Vec<String> {
        candidates
            .items
            .into_iter()
            .map(|album| album.media.item_id.as_str().to_owned())
            .collect()
    }

    #[test]
    fn album_candidates_normalize_names_and_search_without_guessing_unknown_sort_fields() {
        let mut connection = database();
        let snapshot = connection.transaction().unwrap();
        let user = UserId("user".into());
        let mut albums = Vec::new();
        for (item_id, name, sort_name, year) in [
            ("a", "alpha", Some("Zulu"), Some(Some(2020))),
            ("e", "\u{c9}clair", Some("Eclair"), Some(Some(2024))),
            ("p", "P!nk", Some("P!nk"), Some(Some(2023))),
            ("u", "Uber", None, None),
            ("z", "Zulu", Some("Zulu"), Some(None)),
        ] {
            let mut value = json!({"provider":"library","item_id":item_id,"name":name,"album_type":"ep",
                "artists":[{"provider":"library","item_id":"artist","name":"Artist","metadata":{"description":"Biography"}}],
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":item_id,"in_library":true}]});
            if let Some(sort_name) = sort_name {
                value["sort_name"] = json!(sort_name);
            }
            if let Some(year) = year {
                value["year"] = json!(year);
            }
            if item_id == "u" {
                value.as_object_mut().unwrap().remove("album_type");
            }
            if item_id == "z" {
                value["album_type"] = json!("unknown");
            }
            albums.push(serde_json::from_value(value).unwrap());
        }
        let page = CatalogQuery::LibraryAlbums {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
            filters: Default::default(),
        };
        write_album_read(&snapshot, &page, &user, &CatalogValue::Albums(albums), 100).unwrap();
        snapshot
            .execute(
                "UPDATE artists SET metadata=json_object('description',7),metadata_known=1",
                [],
            )
            .unwrap();
        let mut query = AlbumCandidateQuery::new(user);
        query.sort = AlbumCandidateSort::Name;
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["a", "e", "p", "u", "z"]
        );
        query.sort = AlbumCandidateSort::SortName;
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["e", "p", "a", "z"]
        );
        query.direction = CatalogSortDirection::Desc;
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["z", "a", "p", "e"]
        );
        query.sort = AlbumCandidateSort::Year;
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["e", "p", "a", "z"]
        );
        query.direction = CatalogSortDirection::Asc;
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["z", "a", "p", "e"]
        );
        query.sort = AlbumCandidateSort::Name;
        query.album_types = Some(vec![LibraryAlbumType::Ep]);
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["a", "e", "p"]
        );
        query.album_types = Some(vec![LibraryAlbumType::Unknown]);
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["z"]
        );
        query.album_types = None;
        query.search = Some("\u{c9}CLAIR%".into());
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["e"]
        );
        query.search = Some("ART!IST - P!nk".into());
        assert_eq!(
            identities(read_album_candidates(&snapshot, &query).unwrap()),
            ["p"]
        );
        query.search = Some("Artist".into());
        assert_eq!(
            read_album_candidates(&snapshot, &query).unwrap().knowledge,
            AlbumCandidateKnowledge::Unknown
        );
    }

    #[test]
    fn album_candidates_query_plans_use_sort_membership_user_and_credit_indexes() {
        let mut connection = database();
        let snapshot = connection.transaction().unwrap();
        for (sort, index) in [
            (AlbumCandidateSort::Name, "albums_candidate_name_idx"),
            (
                AlbumCandidateSort::SortName,
                "albums_candidate_sort_name_idx",
            ),
            (AlbumCandidateSort::Year, "albums_candidate_year_idx"),
        ] {
            for direction in [CatalogSortDirection::Asc, CatalogSortDirection::Desc] {
                let mut query = AlbumCandidateQuery::new(UserId("user".into()));
                query.sort = sort;
                query.direction = direction;
                let plan = snapshot
                    .prepare(&format!(
                        "EXPLAIN QUERY PLAN {}",
                        album_candidate_sql(&query)
                    ))
                    .unwrap()
                    .query_map(
                        params![
                            "user",
                            false,
                            "[\"member\"]",
                            "[\"ep\"]",
                            1,
                            0,
                            "title",
                            "artist"
                        ],
                        |row| row.get::<_, String>(3),
                    )
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
                    .join("\n");
                assert!(plan.contains(index), "{plan}");
                assert!(plan.contains("album_read_items_identity_idx"), "{plan}");
                assert!(plan.contains("album_membership_lookup_idx"), "{plan}");
                assert!(plan.contains("SEARCH favorite USING PRIMARY KEY"), "{plan}");
                assert!(plan.contains("SEARCH credit USING PRIMARY KEY"), "{plan}");
                assert!(!plan.contains("TEMP B-TREE"), "{plan}");
            }
        }
    }

    // Isolated canonical and response observations.
    #[test]
    fn album_candidates_require_same_membership_and_user_observation_before_hydration() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        initialize_album_query_indexes(&transaction).unwrap();
        let user = UserId("user".into());
        let page = CatalogQuery::LibraryAlbums {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
            filters: Default::default(),
        };
        let albums:Vec<Album> = serde_json::from_value(json!([
            {"provider":"library","item_id":"a","name":"Alpha","sort_name":"Alpha","album_type":"ep","favorite":false,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"a","in_library":true}]},
            {"provider":"library","item_id":"b","name":"Beta","sort_name":"Beta","album_type":"ep","favorite":false,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"b","in_library":true}]},
            {"provider":"library","item_id":"wrong","name":"Wrong","sort_name":"Wrong","album_type":"ep","favorite":false,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"wrong","in_library":false},
                    {"provider_domain":"source","provider_instance":"other","item_id":"wrong","in_library":true}]},
            {"provider":"library","item_id":"unknown","name":"Unknown","sort_name":"Unknown","album_type":"ep","favorite":false,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"unknown"}]}
        ])).unwrap();
        write_album_read(
            &transaction,
            &page,
            &user,
            &CatalogValue::Albums(albums),
            100,
        )
        .unwrap();
        let unseen:Album = serde_json::from_value(json!({"provider":"library","item_id":"unseen","name":"Unseen","sort_name":"Unseen","album_type":"ep","favorite":false,
            "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"unseen","in_library":true}]})).unwrap();
        super::super::albums::write_album_observation(&transaction, &unseen, &user).unwrap();
        transaction.execute("UPDATE albums SET metadata=json_object('description',7),metadata_known=1 WHERE item_id IN ('a','wrong','unknown','unseen')",[]).unwrap();
        let mut query = AlbumCandidateQuery::new(user);
        query.favorite = Some(false);
        query.provider_instances = Some(vec![ProviderInstanceId("member".into())]);
        query.album_types = Some(vec![LibraryAlbumType::Ep]);
        query.is_summary = false;
        query.limit = NonZeroU32::new(1).unwrap();
        query.offset = 1;
        let candidates = read_album_candidates(&transaction, &query).unwrap();
        assert_eq!(candidates.knowledge, AlbumCandidateKnowledge::Partial);
        assert_eq!(candidates.items[0].media.item_id.as_str(), "b");
        query.user_id = UserId("other".into());
        assert_eq!(
            read_album_candidates(&transaction, &query)
                .unwrap()
                .knowledge,
            AlbumCandidateKnowledge::Unknown
        );
        query.user_id = UserId("user".into());
        query.offset = 0;
        query.limit = NonZeroU32::new(500).unwrap();
        query.is_summary = true;
        assert_eq!(
            identities(read_album_candidates(&transaction, &query).unwrap()),
            ["a", "b"]
        );
        query.provider_instances = Some(vec![]);
        assert_eq!(
            read_album_candidates(&transaction, &query)
                .unwrap()
                .knowledge,
            AlbumCandidateKnowledge::Unknown
        );
        query.provider_instances = Some(vec![ProviderInstanceId("member".into())]);
        transaction
            .execute(
                "UPDATE album_provider_mappings SET in_library=0 WHERE album_item_id='b'",
                [],
            )
            .unwrap();
        assert_eq!(
            identities(read_album_candidates(&transaction, &query).unwrap()),
            ["a"]
        );
    }
}
