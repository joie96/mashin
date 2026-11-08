//! Preliminary selections from observed artists, never complete server result pages.
//! SQL selects identities before detail reconstruction; an empty selection proves no absence.

use std::num::NonZeroU32;

use rusqlite::{functions::FunctionFlags, params, Connection, Transaction};

use crate::domain::catalog::LibraryArtistType;
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::Artist;

use super::artists::{read_artist, read_artist_summary, ArtistReadProjection};
use super::database::CacheError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ArtistCandidateDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum ArtistCandidateSort {
    #[default]
    SortName,
    Name,
}

pub(super) struct ArtistCandidateQuery {
    pub(super) user_id: UserId,
    pub(super) favorite: Option<bool>,
    pub(super) search: Option<String>,
    pub(super) sort: ArtistCandidateSort,
    /// Resolved provider subset; an explicitly empty subset must match nothing.
    pub(super) provider_instances: Option<Vec<ProviderInstanceId>>,
    pub(super) artist_type: Option<LibraryArtistType>,
    pub(super) album_artists_only: bool,
    /// User-visible album providers, independent of the explicit artist provider filter.
    pub(super) visible_album_provider_instances: Option<Vec<ProviderInstanceId>>,
    pub(super) projection: ArtistReadProjection,
    pub(super) requires_user_library_observation: bool,
    pub(super) direction: ArtistCandidateDirection,
    pub(super) limit: NonZeroU32,
    pub(super) offset: u32,
}

impl ArtistCandidateQuery {
    // Defaults for a bounded preliminary selection, not a server API parameter decoder.
    pub(super) fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            favorite: None,
            search: None,
            sort: ArtistCandidateSort::SortName,
            provider_instances: None,
            artist_type: None,
            album_artists_only: false,
            visible_album_provider_instances: None,
            projection: ArtistReadProjection::Summary,
            requires_user_library_observation: false,
            direction: ArtistCandidateDirection::Ascending,
            limit: NonZeroU32::new(500).unwrap(),
            offset: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArtistCandidateKnowledge {
    Unknown,
    Partial,
}

pub(super) struct ArtistCandidates {
    pub(super) items: Vec<Artist>,
    pub(super) knowledge: ArtistCandidateKnowledge,
}

// Connection-local search support does not modify persisted observations.
pub(super) fn initialize_artist_query_functions(connection: &Connection) -> Result<(), CacheError> {
    connection.create_scalar_function(
        "mashin_artist_search_key",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            Ok(context
                .get::<Option<String>>(0)?
                .as_deref()
                .map(artist_search_key))
        },
    )?;
    Ok(())
}

// Snapshot selection; SQL pagination is over the observed subset only.
/// Requires confirmed membership; unknown favorites never count as negative ratings.
pub(super) fn read_artist_candidates(
    snapshot: &Transaction<'_>,
    query: &ArtistCandidateQuery,
) -> Result<ArtistCandidates, CacheError> {
    let has_members: bool = snapshot.query_row(
        "SELECT EXISTS(SELECT 1 FROM artist_library_entries WHERE is_in_library=1)",
        [],
        |row| row.get(0),
    )?;
    let providers = query
        .provider_instances
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let should_filter_album_artists = query.album_artists_only
        && matches!(query.artist_type, None | Some(LibraryArtistType::Singer));
    let album_providers = query
        .visible_album_provider_instances
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let search = query
        .search
        .as_deref()
        .map(artist_search_key)
        .filter(|value| !value.is_empty());
    let artist_type = query.artist_type.map(|artist_type| match artist_type {
        LibraryArtistType::Singer => "singer",
        LibraryArtistType::Author => "author",
        LibraryArtistType::Narrator => "narrator",
        LibraryArtistType::Unknown => "unknown",
    });
    let sql = artist_candidate_sql(query);
    let mut statement = snapshot.prepare(&sql)?;
    let identities = statement
        .query_map(
            params![
                query.user_id.as_str(),
                query.favorite,
                providers,
                artist_type,
                query.limit.get(),
                query.offset,
                should_filter_album_artists,
                album_providers,
                search,
                query.requires_user_library_observation
            ],
            |row| Ok((ProviderInstanceId(row.get(0)?), MediaId(row.get(1)?))),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let items = identities
        .into_iter()
        .map(|(provider, item_id)| {
            let artist = match query.projection {
                ArtistReadProjection::Summary => {
                    read_artist_summary(snapshot, &provider, &item_id, &query.user_id)?
                }
                ArtistReadProjection::Detail => {
                    read_artist(snapshot, &provider, &item_id, &query.user_id)?
                }
            };
            artist.ok_or(CacheError::InvalidMediaObservation(
                "selected artist is missing",
            ))
        })
        .collect::<Result<Vec<_>, CacheError>>()?;
    Ok(ArtistCandidates {
        items,
        knowledge: if has_members {
            ArtistCandidateKnowledge::Partial
        } else {
            ArtistCandidateKnowledge::Unknown
        },
    })
}

// Private query construction and server-compatible name normalization.
fn artist_search_key(value: &str) -> String {
    let value = match value {
        "P!nk" | "p!nk" => "pink",
        "Wh\u{2642}" | "wh\u{2642}" => "who",
        "Ko\u{42f}n" | "ko\u{42f}n" => "korn",
        "$hort" => "short",
        _ => value,
    };
    any_ascii::any_ascii(&value.to_lowercase())
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn artist_candidate_sql(query: &ArtistCandidateQuery) -> String {
    let direction = match query.direction {
        ArtistCandidateDirection::Ascending => "ASC",
        ArtistCandidateDirection::Descending => "DESC",
    };
    let order_column = match query.sort {
        ArtistCandidateSort::SortName => "mashin_artist_search_key(artist.sort_name)",
        ArtistCandidateSort::Name => "mashin_artist_search_key(artist.name)",
    };
    format!(
        "SELECT artist.provider, artist.item_id FROM artists artist
         JOIN artist_library_entries membership
             ON membership.provider=artist.provider AND membership.item_id=artist.item_id
         WHERE membership.is_in_library=1
         AND (?10=0 OR EXISTS(SELECT 1 FROM artist_read_items observed
             WHERE observed.user_id=?1 AND observed.provider=artist.provider AND observed.item_id=artist.item_id
             AND json_type(observed.query,'$.LibraryArtists') IS NOT NULL))
         AND (?2 IS NULL OR EXISTS(SELECT 1 FROM artist_favorites favorite
             WHERE favorite.user_id=?1 AND favorite.provider=artist.provider
             AND favorite.item_id=artist.item_id AND favorite.rating=?2))
         AND (?3 IS NULL OR EXISTS(SELECT 1 FROM artist_provider_mappings mapping
             WHERE mapping.artist_provider=artist.provider AND mapping.artist_item_id=artist.item_id
             AND mapping.in_library=1 AND mapping.provider_instance IN (SELECT value FROM json_each(?3))))
         AND (?4 IS NULL OR (artist.artist_type_known=1 AND artist.artist_type=?4))
         AND (?7=0 OR EXISTS(SELECT 1 FROM album_artists credit
             JOIN album_provider_mappings album_mapping
                 ON album_mapping.album_provider=credit.album_provider
                 AND album_mapping.album_item_id=credit.album_item_id
                 AND album_mapping.in_library=1
                 AND album_mapping.in_library_known=1
                 AND (?8 IS NULL OR album_mapping.provider_instance IN (SELECT value FROM json_each(?8)))
             WHERE credit.album_provider='library' AND credit.artist_provider=artist.provider
                 AND credit.artist_item_id=artist.item_id
                 AND (?10=0 OR EXISTS(SELECT 1 FROM artist_album_read_items observed_album
                    WHERE observed_album.user_id=?1 AND observed_album.provider=credit.album_provider
                        AND observed_album.item_id=credit.album_item_id))))
         AND (?9 IS NULL OR instr(mashin_artist_search_key(artist.name),?9)>0)
         ORDER BY {order_column} {direction}, artist.provider ASC, artist.item_id ASC LIMIT ?5 OFFSET ?6"
    )
}

#[cfg(test)]
mod tests {
    use super::super::artists::write_artist_observation;
    use super::*;
    use crate::domain::media::FieldKnowledge;

    // Invalid excluded details prove that SQL filters and bounds precede hydration.
    #[test]
    fn artist_candidates_name_and_sort_name_order_follow_normalized_server_keys() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        for (item_id, name, sort_name) in [
            ("z", "Zulu", "Zulu"),
            ("a", "alpha", "alpha"),
            ("e", "\u{c9}clair", "\u{c9}clair"),
            ("u", "\u{dc}ber", "\u{dc}ber"),
            ("c", "The Cure", "Cure, The"),
        ] {
            let artist=serde_json::from_value(serde_json::json!({"provider":"library","item_id":item_id,"name":name,"sort_name":sort_name})).unwrap();
            write_artist_observation(&transaction, &artist, &user, FieldKnowledge::Known(true))
                .unwrap();
        }
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(user);
        for (sort, expected) in [
            (ArtistCandidateSort::Name, vec!["a", "e", "c", "u", "z"]),
            (ArtistCandidateSort::SortName, vec!["a", "c", "e", "u", "z"]),
        ] {
            query.sort = sort;
            query.direction = ArtistCandidateDirection::Ascending;
            let result = read_artist_candidates(&transaction, &query).unwrap();
            assert_eq!(
                result
                    .items
                    .iter()
                    .map(|artist| artist.media.item_id.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            query.direction = ArtistCandidateDirection::Descending;
            let result = read_artist_candidates(&transaction, &query).unwrap();
            assert_eq!(
                result
                    .items
                    .iter()
                    .map(|artist| artist.media.item_id.as_str())
                    .collect::<Vec<_>>(),
                expected.into_iter().rev().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn artist_candidates_order_and_user_observation_use_indexes_before_hydration() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        for (sort, index) in [
            (ArtistCandidateSort::Name, "artists_search_name_idx"),
            (
                ArtistCandidateSort::SortName,
                "artists_search_sort_name_idx",
            ),
        ] {
            let mut query = ArtistCandidateQuery::new(user.clone());
            query.sort = sort;
            query.requires_user_library_observation = true;
            let mut statement = transaction
                .prepare(&format!(
                    "EXPLAIN QUERY PLAN {}",
                    artist_candidate_sql(&query)
                ))
                .unwrap();
            let plan = statement
                .query_map(
                    params![
                        user.as_str(),
                        None::<bool>,
                        None::<String>,
                        None::<String>,
                        10,
                        0,
                        false,
                        None::<String>,
                        None::<String>,
                        true
                    ],
                    |row| row.get::<_, String>(3),
                )
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
                .join(" ");
            assert!(plan.contains(index), "{plan}");
            assert!(plan.contains("artist_read_items_identity_idx"), "{plan}");
            assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        }
    }

    #[test]
    fn artist_candidates_search_uses_server_unicode_and_special_name_rules() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        for (item_id, name) in [
            ("beyonce", "Beyonc\u{e9}"),
            ("pink", "P!nk"),
            ("who", "Wh\u{2642}"),
            ("korn", "Ko\u{42f}n"),
            ("short", "$hort"),
            ("symbol", "Artist\u{2122}"),
            ("cjk", "\u{6771}\u{4eac}"),
            ("spaces", "Depeche Mode"),
        ] {
            let artist=serde_json::from_value(serde_json::json!({"provider":"library","item_id":item_id,"name":name,"sort_name":"unrelated_sort_key"})).unwrap();
            write_artist_observation(&transaction, &artist, &user, FieldKnowledge::Known(true))
                .unwrap();
        }
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(user);
        for (search, item_id) in [
            ("BEYONCE", "beyonce"),
            ("pink", "pink"),
            ("who", "who"),
            ("korn", "korn"),
            ("short", "short"),
            ("artisttm", "symbol"),
            ("dongjing", "cjk"),
            ("depeche-mode", "spaces"),
        ] {
            query.search = Some(search.into());
            let result = read_artist_candidates(&transaction, &query).unwrap();
            assert_eq!(result.items.len(), 1, "{search}");
            assert_eq!(result.items[0].media.item_id.as_str(), item_id, "{search}");
        }
        query.search = Some("unrelated_sort_key".into());
        assert!(read_artist_candidates(&transaction, &query)
            .unwrap()
            .items
            .is_empty());
        query.search = Some("%_ ' OR 1=1 --".into());
        assert!(read_artist_candidates(&transaction, &query)
            .unwrap()
            .items
            .is_empty());
        query.search = Some("%_".into());
        assert_eq!(
            read_artist_candidates(&transaction, &query)
                .unwrap()
                .items
                .len(),
            8
        );
    }

    #[test]
    fn artist_candidates_search_and_sort_select_before_unrelated_details() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        for index in 0..1000 {
            let artist=serde_json::from_value(serde_json::json!({"provider":"library","item_id":format!("irrelevant-{index}"),"name":"Unrelated"})).unwrap();
            write_artist_observation(&transaction, &artist, &user, FieldKnowledge::Known(true))
                .unwrap();
        }
        transaction
            .execute(
                "UPDATE artists SET metadata='{\"description\":[]}',metadata_known=1",
                [],
            )
            .unwrap();
        for (item_id, name, sort_name) in [
            ("a", "Alpha Needle", "z"),
            ("b", "Beta Needle", "a"),
            ("literal", "Literal %_", "literal"),
        ] {
            let artist=serde_json::from_value(serde_json::json!({"provider":"library","item_id":item_id,"name":name,"sort_name":sort_name})).unwrap();
            write_artist_observation(&transaction, &artist, &user, FieldKnowledge::Known(true))
                .unwrap();
        }
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(user);
        query.search = Some("NEEDLE".into());
        query.limit = NonZeroU32::new(1).unwrap();
        query.projection = ArtistReadProjection::Detail;
        assert_eq!(
            read_artist_candidates(&transaction, &query).unwrap().items[0]
                .media
                .item_id
                .as_str(),
            "b"
        );
        query.sort = ArtistCandidateSort::Name;
        assert_eq!(
            read_artist_candidates(&transaction, &query).unwrap().items[0]
                .media
                .item_id
                .as_str(),
            "a"
        );
        query.direction = ArtistCandidateDirection::Descending;
        assert_eq!(
            read_artist_candidates(&transaction, &query).unwrap().items[0]
                .media
                .item_id
                .as_str(),
            "b"
        );
        query.offset = 1;
        assert_eq!(
            read_artist_candidates(&transaction, &query).unwrap().items[0]
                .media
                .item_id
                .as_str(),
            "a"
        );
        query.offset = 0;
        query.search = Some("Literal %_".into());
        let result = read_artist_candidates(&transaction, &query).unwrap();
        assert_eq!(result.items[0].media.item_id.as_str(), "literal");
        assert_eq!(result.knowledge, ArtistCandidateKnowledge::Partial);
    }
}
