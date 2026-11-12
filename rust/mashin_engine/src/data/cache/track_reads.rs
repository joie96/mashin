//! Track details and lists retain observed context without copying canonical media.
//! The caller owns the snapshot/write transaction; partial aliases never prove exact coverage.

use rusqlite::{params, OptionalExtension, Transaction};

use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{MediaItem, MediaType, Track};

use super::albums::read_album;
use super::database::CacheError;
use super::reads::CatalogReadObservation;
use super::tracks::{read_track, write_track_observation};

// Schema and atomic track observations.
pub(super) fn initialize_track_read_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE track_read_states (
            user_id TEXT NOT NULL REFERENCES users(user_id),query TEXT NOT NULL,
            observed_at INTEGER NOT NULL CHECK(observed_at>=0),provider TEXT NOT NULL,item_id TEXT NOT NULL,
            album_provider TEXT,album_item_id TEXT,album_known INTEGER NOT NULL CHECK(album_known IN (0,1)),
            disc_number INTEGER CHECK(disc_number>=0),track_number INTEGER CHECK(track_number>=0),
            position INTEGER CHECK(position>=0),position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),
            image TEXT CHECK(image IS NULL OR json_type(image)='object'),image_known INTEGER NOT NULL CHECK(image_known IN (0,1)),
            PRIMARY KEY(user_id,query),CHECK((album_provider IS NULL)=(album_item_id IS NULL)),
            CHECK(album_known=1 OR album_provider IS NULL),
            FOREIGN KEY(provider,item_id) REFERENCES tracks(provider,item_id),
            FOREIGN KEY(album_provider,album_item_id,provider,item_id)
                REFERENCES album_tracks(album_provider,album_item_id,track_provider,track_item_id)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX track_read_states_identity_idx ON track_read_states(user_id,provider,item_id,query);
         CREATE INDEX track_domain_lookup_idx ON track_provider_mappings(provider_domain,item_id,track_provider,track_item_id);",
    )?;
    Ok(())
}

pub(super) fn initialize_track_list_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE track_list_states (
            user_id TEXT NOT NULL REFERENCES users(user_id),query TEXT NOT NULL,
            observed_at INTEGER NOT NULL CHECK(observed_at>=0),PRIMARY KEY(user_id,query)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE track_list_items (
            user_id TEXT NOT NULL,query TEXT NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),
            provider TEXT NOT NULL,item_id TEXT NOT NULL,album_provider TEXT,album_item_id TEXT,
            album_known INTEGER NOT NULL CHECK(album_known IN (0,1)),disc_number INTEGER CHECK(disc_number>=0),
            track_number INTEGER CHECK(track_number>=0),position INTEGER CHECK(position>=0),
            position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),
            image TEXT CHECK(image IS NULL OR json_type(image)='object'),image_known INTEGER NOT NULL DEFAULT 0 CHECK(image_known IN (0,1)),
            PRIMARY KEY(user_id,query,occurrence),CHECK((album_provider IS NULL)=(album_item_id IS NULL)),
            CHECK(album_known=1 OR album_provider IS NULL),
            FOREIGN KEY(user_id,query) REFERENCES track_list_states(user_id,query) ON DELETE CASCADE,
            FOREIGN KEY(provider,item_id) REFERENCES tracks(provider,item_id),
            FOREIGN KEY(album_provider,album_item_id,provider,item_id) REFERENCES album_tracks(album_provider,album_item_id,track_provider,track_item_id)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX track_list_items_identity_idx ON track_list_items(user_id,provider,item_id,query);"
    )?;
    Ok(())
}

// Mutations retire exact content coverage for the canonical identity and observed aliases.
pub(super) fn invalidate_confirmed_playlist_content_reads(
    transaction: &Transaction<'_>,
    playlist: &crate::domain::media::Playlist,
) -> Result<(), CacheError> {
    transaction.execute("WITH identities(provider,item_id) AS (
        SELECT ?1,?2 UNION SELECT provider_instance,item_id FROM playlist_provider_mappings WHERE provider=?1 AND playlist_id=?2
        UNION SELECT provider_domain,item_id FROM playlist_provider_mappings WHERE provider=?1 AND playlist_id=?2)
        DELETE FROM track_list_states AS state WHERE EXISTS(SELECT 1 FROM identities identity WHERE
            json_extract(state.query,'$.PlaylistTracks.provider')=identity.provider AND
            json_extract(state.query,'$.PlaylistTracks.item_id')=identity.item_id)",
        params![playlist.media.provider.as_str(),playlist.media.item_id.as_str()])?;
    Ok(())
}

// Removal retires the playlist's content knowledge, not its canonical tracks.
pub(super) fn invalidate_removed_playlist_content_reads(
    transaction: &Transaction<'_>,
    provider: &str,
    item_id: &str,
) -> Result<(), CacheError> {
    transaction.execute("DELETE FROM track_list_states WHERE json_extract(query,'$.PlaylistTracks.provider')=?1 AND json_extract(query,'$.PlaylistTracks.item_id')=?2", params![provider,item_id])?;
    Ok(())
}

pub(crate) fn supports_track_read(query: &CatalogQuery) -> bool {
    matches!(
        query,
        CatalogQuery::Track { .. }
            | CatalogQuery::TrackVersions { .. }
            | CatalogQuery::SimilarTracks { .. }
            | CatalogQuery::RecentlyPlayed { .. }
            | CatalogQuery::LibraryTracks { .. }
            | CatalogQuery::PlaylistTracks { .. }
            | CatalogQuery::Search {
                media_type: MediaType::Track,
                ..
            }
            | CatalogQuery::ProviderItem {
                media_type: MediaType::Track,
                ..
            }
    )
}

pub(super) fn is_track_list_read(query: &CatalogQuery) -> bool {
    matches!(
        query,
        CatalogQuery::Search {
            media_type: MediaType::Track,
            ..
        } | CatalogQuery::TrackVersions { .. }
            | CatalogQuery::SimilarTracks { .. }
            | CatalogQuery::RecentlyPlayed { .. }
            | CatalogQuery::LibraryTracks { .. }
            | CatalogQuery::PlaylistTracks { .. }
    )
}

pub(crate) fn write_track_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    if is_track_list_read(query) {
        return write_track_list(transaction, query, user_id, value, observed_at);
    }
    let incoming = match (query, value) {
        (CatalogQuery::Track { .. }, CatalogValue::Track(track)) => track,
        (
            CatalogQuery::ProviderItem {
                media_type: MediaType::Track,
                ..
            },
            CatalogValue::Item(MediaItem::Track(track)),
        ) => track,
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "track query and response do not match",
            ))
        }
    };
    write_track_observation(transaction, incoming, user_id)?;
    let track = read_track(
        transaction,
        &incoming.media.provider,
        &incoming.media.item_id,
        user_id,
    )?
    .ok_or(CacheError::InvalidMediaObservation(
        "observed canonical track missing",
    ))?;
    let query_key = serde_json::to_string(query)?;
    let image = track
        .image
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    transaction.execute(
        "INSERT INTO track_read_states(user_id,query,observed_at,provider,item_id,album_provider,album_item_id,album_known,
            disc_number,track_number,position,position_known,image,image_known)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
         ON CONFLICT(user_id,query) DO UPDATE SET observed_at=excluded.observed_at,provider=excluded.provider,item_id=excluded.item_id,
            album_provider=excluded.album_provider,album_item_id=excluded.album_item_id,album_known=excluded.album_known,
            disc_number=excluded.disc_number,track_number=excluded.track_number,position=excluded.position,position_known=excluded.position_known,
            image=excluded.image,image_known=excluded.image_known",
        params![user_id.as_str(),query_key,observed_at,track.media.provider.as_str(),track.media.item_id.as_str(),
            track.album.as_ref().map(|album| album.media.provider.as_str()),track.album.as_ref().map(|album| album.media.item_id.as_str()),
            track.knowledge.album,track.knowledge.disc_number.then_some(track.disc_number),track.knowledge.track_number.then_some(track.track_number),
            track.media.position,track.media.knowledge.position,image,track.knowledge.image],
    )?;
    Ok(())
}

/// Confirmation retires this user's track details and lists, including base and mapping aliases.
pub(crate) fn commit_confirmed_track(
    transaction: &Transaction<'_>,
    track: &Track,
    user_id: &UserId,
) -> Result<(), CacheError> {
    write_track_observation(transaction, track, user_id)?;
    transaction.execute(
        "DELETE FROM track_read_states AS state WHERE state.user_id=?1 AND (
            (state.provider=?2 AND state.item_id=?3) OR EXISTS(
                SELECT 1 FROM track_provider_mappings mapping
                WHERE mapping.track_provider=?2 AND mapping.track_item_id=?3
                AND mapping.item_id=state.item_id
                AND (mapping.provider_instance=state.provider OR mapping.provider_domain=state.provider)))",
        params![user_id.as_str(),track.media.provider.as_str(),track.media.item_id.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM track_list_states AS state WHERE state.user_id=?1 AND (
            json_type(state.query,'$.LibraryTracks') IS NOT NULL OR
            EXISTS(SELECT 1 FROM json_each(state.query) selector WHERE selector.key IN ('TrackVersions','SimilarTracks') AND (
                (json_extract(selector.value,'$.provider')=?2 AND json_extract(selector.value,'$.item_id')=?3)
                OR EXISTS(SELECT 1 FROM track_provider_mappings mapping WHERE mapping.track_provider=?2 AND mapping.track_item_id=?3
                    AND mapping.item_id=json_extract(selector.value,'$.item_id')
                    AND (mapping.provider_instance=json_extract(selector.value,'$.provider')
                        OR mapping.provider_domain=json_extract(selector.value,'$.provider'))))) OR EXISTS(
            SELECT 1 FROM track_list_items item WHERE item.user_id=state.user_id AND item.query=state.query AND (
                (item.provider=?2 AND item.item_id=?3) OR EXISTS(
                    SELECT 1 FROM track_provider_mappings mapping WHERE mapping.track_provider=?2 AND mapping.track_item_id=?3
                    AND mapping.item_id=item.item_id AND (mapping.provider_instance=item.provider OR mapping.provider_domain=item.provider)))))",
        params![user_id.as_str(),track.media.provider.as_str(),track.media.item_id.as_str()],
    )?;
    Ok(())
}

// Snapshot reads hydrate canonical metadata and restore only the observed query context.
pub(crate) fn read_track_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    if is_track_list_read(query) {
        return read_track_list(transaction, query, user_id);
    }
    if !supports_track_read(query) {
        return Err(CacheError::InvalidMediaObservation(
            "query is not a track single-item read",
        ));
    }
    let query_key = serde_json::to_string(query)?;
    let reference = transaction.query_row(
        "SELECT observed_at,provider,item_id,album_provider,album_item_id,album_known,disc_number,track_number,
            position,position_known,image,image_known FROM track_read_states WHERE user_id=?1 AND query=?2",
        params![user_id.as_str(),query_key],
        |row| Ok(TrackReadReference {
            observed_at: row.get(0)?, provider: ProviderInstanceId(row.get(1)?), item_id: MediaId(row.get(2)?),
            album_provider: row.get::<_, Option<String>>(3)?.map(ProviderInstanceId),
            album_item_id: row.get::<_, Option<String>>(4)?.map(MediaId), has_known_album: row.get(5)?,
            disc_number: row.get(6)?, track_number: row.get(7)?, position: row.get(8)?,
            has_known_position: row.get(9)?, image_json: row.get(10)?, has_known_image: row.get(11)?,
        }),
    ).optional()?;
    let Some(reference) = reference else {
        return Ok(None);
    };
    let mut track = read_track(
        transaction,
        &reference.provider,
        &reference.item_id,
        user_id,
    )?
    .ok_or(CacheError::InvalidMediaObservation(
        "observed canonical track missing",
    ))?;
    track.album = match reference.album_provider.zip(reference.album_item_id) {
        Some((provider, item_id)) => Some(
            read_album(transaction, &provider, &item_id, user_id)?.ok_or(
                CacheError::InvalidMediaObservation("observed track album missing"),
            )?,
        ),
        None => None,
    };
    track.knowledge.album = reference.has_known_album;
    track.disc_number = reference.disc_number.unwrap_or_default();
    track.knowledge.disc_number = reference.disc_number.is_some();
    track.track_number = reference.track_number.unwrap_or_default();
    track.knowledge.track_number = reference.track_number.is_some();
    track.media.position = reference.position;
    track.media.knowledge.position = reference.has_known_position;
    track.image = reference
        .image_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    track.knowledge.image = reference.has_known_image;
    Ok(Some(CatalogReadObservation {
        value: track_value(query, track),
        observed_at: reference.observed_at,
    }))
}

pub(crate) fn read_partial_track(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogValue>, CacheError> {
    let (provider, item_id) = match query {
        CatalogQuery::Track { provider, item_id }
        | CatalogQuery::ProviderItem {
            media_type: MediaType::Track,
            provider,
            item_id,
        } => (provider, item_id),
        _ => return Ok(None),
    };
    let mut identity = (ProviderInstanceId(provider.clone()), item_id.clone());
    if provider != "library" {
        let identities = transaction
            .prepare(
                "SELECT track_provider,track_item_id FROM track_provider_mappings
                WHERE track_provider='library' AND provider_instance=?1 AND item_id=?2
             UNION SELECT track_provider,track_item_id FROM track_provider_mappings
                WHERE track_provider='library' AND provider_domain=?1 AND item_id=?2 LIMIT 2",
            )?
            .query_map(params![provider, item_id.as_str()], |row| {
                Ok((ProviderInstanceId(row.get(0)?), MediaId(row.get(1)?)))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if identities.len() > 1 {
            return Ok(None);
        }
        if let Some(library) = identities.into_iter().next() {
            identity = library;
        }
    }
    Ok(read_track(transaction, &identity.0, &identity.1, user_id)?
        .map(|track| track_value(query, track)))
}

// Private list coverage, typed reference decoding and catalog projection.
fn write_track_list(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    if !matches!((query,value), (CatalogQuery::Search { media_type:MediaType::Track,.. },CatalogValue::Search(results))
        if results.artists.is_empty() && results.albums.is_empty() && results.playlists.is_empty() && results.radio.is_empty())
        && !matches!(
            (query, value),
            (
                CatalogQuery::TrackVersions { .. }
                    | CatalogQuery::SimilarTracks { .. }
                    | CatalogQuery::RecentlyPlayed { .. }
                    | CatalogQuery::LibraryTracks { .. }
                    | CatalogQuery::PlaylistTracks { .. },
                CatalogValue::Tracks(_)
            )
        )
    {
        return Err(CacheError::InvalidMediaObservation(
            "track list query and response do not match",
        ));
    }
    transaction.execute(
        "INSERT INTO users(user_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [user_id.as_str()],
    )?;
    transaction.execute(
        "INSERT INTO track_list_states(user_id,query,observed_at) VALUES(?1,?2,?3)
        ON CONFLICT(user_id,query) DO UPDATE SET observed_at=excluded.observed_at
        WHERE observed_at IS NOT excluded.observed_at",
        params![user_id.as_str(), serde_json::to_string(query)?, observed_at],
    )?;
    super::catalog_sections::write_catalog_section(transaction, query, user_id, value)
}

fn read_track_list(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user_id: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    let observed_at = transaction
        .query_row(
            "SELECT observed_at FROM track_list_states WHERE user_id=?1 AND query=?2",
            params![user_id.as_str(), serde_json::to_string(query)?],
            |row| row.get(0),
        )
        .optional()?;
    observed_at
        .map(|observed_at| {
            Ok(CatalogReadObservation {
                value: super::catalog_sections::read_catalog_section(transaction, query, user_id)?,
                observed_at,
            })
        })
        .transpose()
}

struct TrackReadReference {
    observed_at: i64,
    provider: ProviderInstanceId,
    item_id: MediaId,
    album_provider: Option<ProviderInstanceId>,
    album_item_id: Option<MediaId>,
    has_known_album: bool,
    disc_number: Option<u16>,
    track_number: Option<u16>,
    position: Option<u32>,
    has_known_position: bool,
    image_json: Option<String>,
    has_known_image: bool,
}

fn track_value(query: &CatalogQuery, track: Track) -> CatalogValue {
    if matches!(query, CatalogQuery::ProviderItem { .. }) {
        CatalogValue::Item(MediaItem::Track(track))
    } else {
        CatalogValue::Track(track)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::media::FieldKnowledge;
    use rusqlite::Connection;
    use serde_json::json;

    // Isolated schemas and single-item fixtures precede active routing.
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
    fn query() -> CatalogQuery {
        CatalogQuery::Track {
            provider: "library".into(),
            item_id: MediaId("007".into()),
        }
    }
    fn alias_query() -> CatalogQuery {
        CatalogQuery::ProviderItem {
            media_type: MediaType::Track,
            provider: "source-a".into(),
            item_id: MediaId("remote".into()),
        }
    }
    fn track(album_id: &str, number: u16) -> Track {
        serde_json::from_value(json!({"provider":"library","item_id":"007","name":"Observed","favorite":true,
            "duration":240,"metadata":{"description":"Retained"},"position":3,
            "album":{"provider":"library","item_id":album_id,"name":album_id},"disc_number":1,"track_number":number,
            "image":{"type":"thumb","provider":"library","path":format!("https://example.com/{album_id}.jpg")},
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote","in_library":false}]
        })).unwrap()
    }

    // Exact context, canonical merges and per-user coverage.
    #[test]
    fn library_tracks_preserve_page_scope_reopen_empty_and_mutation_invalidation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracks.db");
        let user = UserId("user".into());
        let query: CatalogQuery = serde_json::from_value(json!({"LibraryTracks": {
            "favorite": true, "limit": 2, "offset": 0, "summary": false
        }}))
        .unwrap();
        let empty_page: CatalogQuery = serde_json::from_value(json!({"LibraryTracks": {
            "favorite": true, "limit": 2, "offset": 2, "summary": false
        }}))
        .unwrap();
        let value = CatalogValue::Tracks(vec![track("album-a", 4), track("album-b", 8)]);
        {
            let mut connection = Connection::open(&path).unwrap();
            connection
                .pragma_update(None, "foreign_keys", true)
                .unwrap();
            let transaction = connection.transaction().unwrap();
            super::super::database::initialize_media_schema(&transaction).unwrap();
            assert!(supports_track_read(&query));
            write_track_read(&transaction, &query, &user, &value, 100).unwrap();
            write_track_read(
                &transaction,
                &empty_page,
                &user,
                &CatalogValue::Tracks(Vec::new()),
                101,
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        let mut connection = Connection::open(&path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        let observed = read_track_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observed.value, value);
        assert_eq!(observed.observed_at, 100);
        assert_eq!(
            read_track_read(&transaction, &empty_page, &user)
                .unwrap()
                .unwrap()
                .value,
            CatalogValue::Tracks(Vec::new())
        );
        assert!(
            read_track_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        for (favorite, limit, offset, summary) in [
            (None, 2, 0, false),
            (Some(false), 2, 0, false),
            (Some(true), 1, 0, false),
            (Some(true), 2, 1, false),
            (Some(true), 2, 0, true),
        ] {
            let different: CatalogQuery = serde_json::from_value(json!({"LibraryTracks": {
                "favorite": favorite, "limit": limit, "offset": offset, "summary": summary
            }}))
            .unwrap();
            assert!(read_track_read(&transaction, &different, &user)
                .unwrap()
                .is_none());
            assert!(read_partial_track(&transaction, &different, &user)
                .unwrap()
                .is_none());
        }
        let mut added = track("album-c", 1);
        added.media.item_id = MediaId("new-favorite".into());
        commit_confirmed_track(&transaction, &added, &user).unwrap();
        for page in [query, empty_page] {
            assert!(read_track_read(&transaction, &page, &user)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn playlist_tracks_preserve_occurrences_context_scope_and_known_empty() {
        for allow_dynamic_tracks in [false, true] {
            let mut connection = database();
            let transaction = connection.transaction().unwrap();
            let user = UserId("user".into());
            let query = CatalogQuery::PlaylistTracks {
                provider: "library".into(),
                item_id: MediaId("playlist".into()),
                allow_dynamic_tracks,
            };
            let mut first = track("album-a", 4);
            first.media.position = Some(1);
            let mut second = track("album-b", 8);
            second.media.position = Some(4);
            let value = CatalogValue::Tracks(vec![first, second]);
            assert!(supports_track_read(&query));
            write_track_read(&transaction, &query, &user, &value, 100).unwrap();
            let observed = read_track_read(&transaction, &query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(observed.observed_at, 100);
            assert_eq!(observed.value, value);
            assert!(
                read_track_read(&transaction, &query, &UserId("other".into()))
                    .unwrap()
                    .is_none()
            );
            for (provider, item_id, dynamic) in [
                ("source-a", "playlist", allow_dynamic_tracks),
                ("library", "other", allow_dynamic_tracks),
                ("library", "playlist", !allow_dynamic_tracks),
            ] {
                let different = CatalogQuery::PlaylistTracks {
                    provider: provider.into(),
                    item_id: MediaId(item_id.into()),
                    allow_dynamic_tracks: dynamic,
                };
                assert!(read_track_read(&transaction, &different, &user)
                    .unwrap()
                    .is_none());
                assert!(read_partial_track(&transaction, &different, &user)
                    .unwrap()
                    .is_none());
            }
            let plan = transaction
                .prepare(&format!(
                    "EXPLAIN QUERY PLAN {}",
                    super::super::catalog_sections::section_occurrences_sql(&query).unwrap()
                ))
                .unwrap()
                .query_map(
                    params![user.as_str(), serde_json::to_string(&query).unwrap()],
                    |row| row.get::<_, String>(3),
                )
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
                .join(" ");
            assert!(
                plan.contains("SEARCH track_list_items USING PRIMARY KEY"),
                "{plan}"
            );
            assert!(!plan.contains("TEMP B-TREE"), "{plan}");
            assert!(write_track_read(
                &transaction,
                &query,
                &user,
                &CatalogValue::Search(Default::default()),
                200
            )
            .is_err());
            assert_eq!(
                read_track_read(&transaction, &query, &user)
                    .unwrap()
                    .unwrap()
                    .observed_at,
                100
            );
            write_track_read(
                &transaction,
                &query,
                &user,
                &CatalogValue::Tracks(Vec::new()),
                300,
            )
            .unwrap();
            assert_eq!(
                read_track_read(&transaction, &query, &user)
                    .unwrap()
                    .unwrap()
                    .value,
                CatalogValue::Tracks(Vec::new())
            );
            assert!(read_partial_track(&transaction, &query, &user)
                .unwrap()
                .is_none());
            assert_eq!(
                transaction
                    .query_row("SELECT COUNT(*) FROM tracks", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }

    #[test]
    fn recently_played_preserves_sparse_mappings_order_limits_and_known_empty() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        write_track_observation(&transaction, &track("album-a", 4), &user).unwrap();
        let query = CatalogQuery::RecentlyPlayed { limit: 2 };
        let value = CatalogValue::Tracks(serde_json::from_value(json!([
            {"media_type":"track","provider":"source-b","item_id":"007","name":"Provider recent","image":null,"available":true},
            {"media_type":"track","provider":"library","item_id":"007","name":"Library recent","image":null,"available":false}
        ])).unwrap());
        assert!(supports_track_read(&query));
        write_track_read(&transaction, &query, &user, &value, 100).unwrap();
        let observed = read_track_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observed.observed_at, 100);
        let CatalogValue::Tracks(tracks) = observed.value else {
            panic!("recent tracks missing")
        };
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].media.provider.as_str(), "source-b");
        assert_eq!(tracks[0].media.name, "Provider recent");
        assert!(!tracks[0].knowledge.duration);
        assert!(!tracks[0].knowledge.artists);
        assert!(!tracks[0].knowledge.album);
        assert_eq!(tracks[1].media.provider.as_str(), "library");
        assert_eq!(tracks[1].media.name, "Library recent");
        assert_eq!(tracks[1].duration, 240);
        assert!(tracks[1].knowledge.duration);
        assert_eq!(tracks[1].media.favorite, FieldKnowledge::Known(Some(true)));
        assert_eq!(tracks[1].media.available, Some(false));
        assert_eq!(tracks[1].media.provider_mappings.len(), 1);
        assert_eq!(
            tracks[1].album.as_ref().unwrap().media.item_id.as_str(),
            "album-a"
        );
        assert_eq!(tracks[1].image, None);
        assert!(tracks[1].knowledge.image);
        assert_eq!(
            tracks[1]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Retained")
        );
        assert!(
            read_track_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        let all = CatalogQuery::RecentlyPlayed { limit: 0 };
        assert!(read_track_read(&transaction, &all, &user)
            .unwrap()
            .is_none());
        assert!(read_partial_track(&transaction, &all, &user)
            .unwrap()
            .is_none());
        assert!(write_track_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Search(Default::default()),
            200
        )
        .is_err());
        assert_eq!(
            read_track_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            100
        );
        write_track_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Tracks(Vec::new()),
            300,
        )
        .unwrap();
        assert_eq!(
            read_track_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            CatalogValue::Tracks(Vec::new())
        );
        assert!(read_partial_track(
            &transaction,
            &CatalogQuery::Track {
                provider: "library".into(),
                item_id: MediaId("007".into())
            },
            &user
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn similar_tracks_preserve_occurrences_parameters_and_known_empty() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        let similar = CatalogQuery::SimilarTracks {
            provider: "source-a".into(),
            item_id: MediaId("base".into()),
            limit: 2,
            allow_lookup: false,
        };
        let value = CatalogValue::Tracks(vec![track("album-a", 4), track("album-b", 8)]);
        assert!(supports_track_read(&similar));
        write_track_read(&transaction, &similar, &user, &value, 100).unwrap();
        let observed = read_track_read(&transaction, &similar, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observed.value, value);
        assert_eq!(observed.observed_at, 100);
        let plan = transaction
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                super::super::catalog_sections::section_occurrences_sql(&similar).unwrap()
            ))
            .unwrap()
            .query_map(
                params![user.as_str(), serde_json::to_string(&similar).unwrap()],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join(" ");
        assert!(
            plan.contains("SEARCH track_list_items USING PRIMARY KEY"),
            "{plan}"
        );
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        assert!(
            read_track_read(&transaction, &similar, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        for (provider, limit, allow_lookup) in [
            ("source-b", 2, false),
            ("source-a", 0, false),
            ("source-a", 2, true),
        ] {
            let different = CatalogQuery::SimilarTracks {
                provider: provider.into(),
                item_id: MediaId("base".into()),
                limit,
                allow_lookup,
            };
            assert!(read_track_read(&transaction, &different, &user)
                .unwrap()
                .is_none());
            assert!(read_partial_track(&transaction, &different, &user)
                .unwrap()
                .is_none());
        }
        assert!(write_track_read(
            &transaction,
            &similar,
            &user,
            &CatalogValue::Search(Default::default()),
            200
        )
        .is_err());
        assert_eq!(
            read_track_read(&transaction, &similar, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            100
        );
        write_track_read(
            &transaction,
            &similar,
            &user,
            &CatalogValue::Tracks(Vec::new()),
            300,
        )
        .unwrap();
        assert_eq!(
            read_track_read(&transaction, &similar, &user)
                .unwrap()
                .unwrap()
                .value,
            CatalogValue::Tracks(Vec::new())
        );
        assert!(read_partial_track(&transaction, &query(), &user)
            .unwrap()
            .is_some());
    }

    #[test]
    fn track_versions_preserve_order_context_query_scope_and_known_empty() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        let versions = CatalogQuery::TrackVersions {
            provider: "source-a".into(),
            item_id: MediaId("base".into()),
        };
        let first = track("album-a", 4);
        let second = track("album-b", 8);
        let value = CatalogValue::Tracks(vec![first, second]);
        write_track_read(&transaction, &versions, &user, &value, 100).unwrap();
        let observed = read_track_read(&transaction, &versions, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observed.observed_at, 100);
        assert_eq!(observed.value, value);
        assert!(
            read_track_read(&transaction, &versions, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        let other_provider = CatalogQuery::TrackVersions {
            provider: "source-b".into(),
            item_id: MediaId("base".into()),
        };
        assert!(read_track_read(&transaction, &other_provider, &user)
            .unwrap()
            .is_none());
        assert!(read_partial_track(&transaction, &versions, &user)
            .unwrap()
            .is_none());
        assert!(write_track_read(
            &transaction,
            &versions,
            &user,
            &CatalogValue::Search(Default::default()),
            200
        )
        .is_err());
        assert_eq!(
            read_track_read(&transaction, &versions, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            100
        );
        write_track_read(
            &transaction,
            &versions,
            &user,
            &CatalogValue::Tracks(Vec::new()),
            300,
        )
        .unwrap();
        assert_eq!(
            read_track_read(&transaction, &versions, &user)
                .unwrap()
                .unwrap()
                .value,
            CatalogValue::Tracks(Vec::new())
        );
        assert!(read_partial_track(&transaction, &query(), &user)
            .unwrap()
            .is_some());
    }

    #[test]
    fn track_lists_confirmation_retires_base_aliases_empty_lists_and_contained_tracks() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        let other = UserId("other".into());
        let base = serde_json::from_value(json!({"provider":"library","item_id":"base","name":"Base","favorite":true,
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"base","in_library":false}]})).unwrap();
        write_track_observation(&transaction, &base, &user).unwrap();
        let selectors = ["library", "source-a", "source"]
            .into_iter()
            .flat_map(|provider| {
                std::iter::once(CatalogQuery::TrackVersions {
                    provider: provider.into(),
                    item_id: MediaId("base".into()),
                })
                .chain([(0, false), (2, false), (2, true)].map(
                    |(limit, allow_lookup)| CatalogQuery::SimilarTracks {
                        provider: provider.into(),
                        item_id: MediaId("base".into()),
                        limit,
                        allow_lookup,
                    },
                ))
            })
            .collect::<Vec<_>>();
        let value = CatalogValue::Tracks(vec![track("album-a", 4)]);
        for owner in [&user, &other] {
            for (index, selector) in selectors.iter().enumerate() {
                let observed = if index % 3 == 0 {
                    CatalogValue::Tracks(Vec::new())
                } else {
                    value.clone()
                };
                write_track_read(&transaction, selector, owner, &observed, 100).unwrap();
            }
        }
        let unrelated = CatalogQuery::TrackVersions {
            provider: "source-a".into(),
            item_id: MediaId("unrelated".into()),
        };
        write_track_read(
            &transaction,
            &unrelated,
            &user,
            &CatalogValue::Tracks(Vec::new()),
            101,
        )
        .unwrap();
        commit_confirmed_track(&transaction, &base, &user).unwrap();
        for selector in &selectors {
            assert!(read_track_read(&transaction, selector, &user)
                .unwrap()
                .is_none());
            assert!(read_track_read(&transaction, selector, &other)
                .unwrap()
                .is_some());
            write_track_read(&transaction, selector, &user, &value, 200).unwrap();
        }
        commit_confirmed_track(&transaction, &track("album-a", 4), &user).unwrap();
        for selector in &selectors {
            assert!(read_track_read(&transaction, selector, &user)
                .unwrap()
                .is_none());
            assert!(read_track_read(&transaction, selector, &other)
                .unwrap()
                .is_some());
        }
        assert_eq!(
            read_track_read(&transaction, &unrelated, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            101
        );
    }

    #[test]
    fn track_search_preserves_occurrences_context_and_exact_empty_knowledge() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        let search = CatalogQuery::Search {
            query: "Observed".into(),
            media_type: MediaType::Track,
            limit: 12,
        };
        let first = track("album-a", 4);
        let mut second = track("album-b", 8);
        second.media.position = Some(9);
        let results = crate::domain::media::MusicAssistantSearchResults {
            tracks: vec![first.clone(), second.clone()],
            ..Default::default()
        };
        write_track_read(
            &transaction,
            &search,
            &user,
            &CatalogValue::Search(results),
            100,
        )
        .unwrap();
        let observed = read_track_read(&transaction, &search, &user)
            .unwrap()
            .unwrap();
        assert_eq!(observed.observed_at, 100);
        let CatalogValue::Search(results) = observed.value else {
            panic!("search result missing")
        };
        assert_eq!(results.tracks, vec![first, second]);
        assert!(
            read_track_read(&transaction, &search, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        let different = CatalogQuery::Search {
            query: "Observed".into(),
            media_type: MediaType::Track,
            limit: 13,
        };
        assert!(read_track_read(&transaction, &different, &user)
            .unwrap()
            .is_none());
        let empty = CatalogValue::Search(Default::default());
        write_track_read(&transaction, &search, &user, &empty, 200).unwrap();
        assert_eq!(
            read_track_read(&transaction, &search, &user)
                .unwrap()
                .unwrap()
                .value,
            empty
        );
        assert!(read_partial_track(&transaction, &query(), &user)
            .unwrap()
            .is_some());
        let plan = transaction
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                super::super::catalog_sections::section_occurrences_sql(&search).unwrap()
            ))
            .unwrap()
            .query_map(
                params![user.as_str(), serde_json::to_string(&search).unwrap()],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join(" ");
        assert!(
            plan.contains("SEARCH track_list_items USING PRIMARY KEY"),
            "{plan}"
        );
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        assert!(write_track_read(
            &transaction,
            &search,
            &user,
            &CatalogValue::Tracks(Vec::new()),
            300
        )
        .is_err());
        assert_eq!(
            read_track_read(&transaction, &search, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            200
        );
    }

    #[test]
    fn track_lists_rollback_and_confirmation_preserve_other_user_and_unrelated_coverage() {
        for (query, unrelated) in [
            (
                CatalogQuery::Search {
                    query: "Observed".into(),
                    media_type: MediaType::Track,
                    limit: 25,
                },
                CatalogQuery::Search {
                    query: "unrelated".into(),
                    media_type: MediaType::Track,
                    limit: 25,
                },
            ),
            (
                CatalogQuery::SimilarTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("base".into()),
                    limit: 2,
                    allow_lookup: false,
                },
                CatalogQuery::SimilarTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("unrelated".into()),
                    limit: 2,
                    allow_lookup: false,
                },
            ),
            (
                CatalogQuery::SimilarTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("base".into()),
                    limit: 0,
                    allow_lookup: true,
                },
                CatalogQuery::SimilarTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("unrelated".into()),
                    limit: 0,
                    allow_lookup: true,
                },
            ),
            (
                CatalogQuery::RecentlyPlayed { limit: 0 },
                CatalogQuery::RecentlyPlayed { limit: 1 },
            ),
            (
                CatalogQuery::RecentlyPlayed { limit: 25 },
                CatalogQuery::RecentlyPlayed { limit: 26 },
            ),
            (
                CatalogQuery::PlaylistTracks {
                    provider: "library".into(),
                    item_id: MediaId("playlist".into()),
                    allow_dynamic_tracks: false,
                },
                CatalogQuery::PlaylistTracks {
                    provider: "library".into(),
                    item_id: MediaId("unrelated".into()),
                    allow_dynamic_tracks: false,
                },
            ),
            (
                CatalogQuery::PlaylistTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("playlist".into()),
                    allow_dynamic_tracks: true,
                },
                CatalogQuery::PlaylistTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("unrelated".into()),
                    allow_dynamic_tracks: true,
                },
            ),
        ] {
            let mut connection = database();
            let user = UserId("user".into());
            let other = UserId("other".into());
            let response = |tracks: Vec<Track>| {
                if matches!(&query, CatalogQuery::Search { .. }) {
                    CatalogValue::Search(crate::domain::media::MusicAssistantSearchResults {
                        tracks,
                        ..Default::default()
                    })
                } else {
                    CatalogValue::Tracks(tracks)
                }
            };
            let original = track("album-a", 4);
            let mut alias = original.clone();
            alias.media.provider = ProviderInstanceId("source-a".into());
            alias.media.item_id = MediaId("remote".into());
            let value = response(vec![original.clone(), alias]);
            let transaction = connection.transaction().unwrap();
            for owner in [&user, &other] {
                write_track_read(&transaction, &query, owner, &value, 100).unwrap();
            }
            write_track_read(&transaction, &unrelated, &user, &response(Vec::new()), 101).unwrap();
            transaction.commit().unwrap();
            let transaction = connection.transaction().unwrap();
            let mut changed = original.clone();
            changed.media.name = "Must roll back".into();
            let mut invalid = original;
            invalid.media.external_ids = vec![vec!["invalid".into()]];
            invalid.media.knowledge.external_ids = true;
            let invalid_value = response(vec![changed, invalid]);
            assert!(write_track_read(&transaction, &query, &user, &invalid_value, 200).is_err());
            drop(transaction);
            let transaction = connection.transaction().unwrap();
            let retained = read_track_read(&transaction, &query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(retained.observed_at, 100);
            assert_eq!(retained.value, value);
            let confirmed = serde_json::from_value(
                json!({"provider":"library","item_id":"007","name":"Confirmed","favorite":null}),
            )
            .unwrap();
            commit_confirmed_track(&transaction, &confirmed, &user).unwrap();
            assert!(read_track_read(&transaction, &query, &user)
                .unwrap()
                .is_none());
            assert!(read_track_read(&transaction, &query, &other)
                .unwrap()
                .is_some());
            assert_eq!(
                read_track_read(&transaction, &unrelated, &user)
                    .unwrap()
                    .unwrap()
                    .observed_at,
                101
            );
            assert!(read_partial_track(&transaction, &query, &user)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn track_confirmed_mutation_retires_only_own_identity_and_alias_reads() {
        for rating in [Some(true), Some(false), None] {
            let mut connection = database();
            let transaction = connection.transaction().unwrap();
            let user = UserId("user".into());
            let other = UserId("other".into());
            for owner in [&user, &other] {
                let mut observed = track("album-a", 4);
                observed.media.favorite = FieldKnowledge::Known(Some(owner == &user));
                write_track_read(
                    &transaction,
                    &query(),
                    owner,
                    &CatalogValue::Track(observed.clone()),
                    100,
                )
                .unwrap();
                observed.media.provider = ProviderInstanceId("source-a".into());
                observed.media.item_id = MediaId("remote".into());
                write_track_read(
                    &transaction,
                    &alias_query(),
                    owner,
                    &CatalogValue::Item(MediaItem::Track(observed)),
                    101,
                )
                .unwrap();
            }
            let unrelated_query = CatalogQuery::Track {
                provider: "library".into(),
                item_id: MediaId("008".into()),
            };
            let mut unrelated = track("album-a", 8);
            unrelated.media.item_id = MediaId("008".into());
            unrelated.media.provider_mappings.clear();
            write_track_read(
                &transaction,
                &unrelated_query,
                &user,
                &CatalogValue::Track(unrelated.clone()),
                102,
            )
            .unwrap();
            let confirmed: Track = serde_json::from_value(json!({
                "provider":"library","item_id":"007","name":"Confirmed","favorite":rating
            }))
            .unwrap();
            commit_confirmed_track(&transaction, &confirmed, &user).unwrap();
            for read_query in [query(), alias_query()] {
                assert!(read_track_read(&transaction, &read_query, &user)
                    .unwrap()
                    .is_none());
                assert!(read_track_read(&transaction, &read_query, &other)
                    .unwrap()
                    .is_some());
            }
            let Some(CatalogValue::Track(persisted)) =
                read_partial_track(&transaction, &query(), &user).unwrap()
            else {
                panic!("confirmed canonical track missing")
            };
            assert_eq!(persisted.media.name, "Confirmed");
            assert_eq!(persisted.media.favorite, FieldKnowledge::Known(rating));
            assert_eq!(persisted.duration, 240);
            assert_eq!(
                persisted.media.metadata.unwrap().description.as_deref(),
                Some("Retained")
            );
            assert_eq!(persisted.album.unwrap().media.item_id.as_str(), "album-a");
            assert_eq!((persisted.disc_number, persisted.track_number), (1, 4));
            let Some(CatalogValue::Item(MediaItem::Track(alias))) =
                read_partial_track(&transaction, &alias_query(), &user).unwrap()
            else {
                panic!("confirmed provider alias missing")
            };
            assert_eq!(alias.media.provider.as_str(), "library");
            assert_eq!(alias.media.favorite, FieldKnowledge::Known(rating));
            let Some(CatalogValue::Track(other_track)) =
                read_partial_track(&transaction, &query(), &other).unwrap()
            else {
                panic!("other user's track missing")
            };
            assert_eq!(
                other_track.media.favorite,
                FieldKnowledge::Known(Some(false))
            );
            let retained = read_track_read(&transaction, &unrelated_query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(retained.observed_at, 102);
            assert_eq!(retained.value, CatalogValue::Track(unrelated));
        }
    }

    #[test]
    fn track_single_reads_preserve_context_and_user_knowledge() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        write_track_read(
            &transaction,
            &query(),
            &user,
            &CatalogValue::Track(track("album-a", 4)),
            100,
        )
        .unwrap();
        let mut second = track("album-b", 9);
        second.media.name = "Updated".into();
        write_track_read(
            &transaction,
            &alias_query(),
            &user,
            &CatalogValue::Item(MediaItem::Track(second)),
            101,
        )
        .unwrap();
        let observed = read_track_read(&transaction, &query(), &user)
            .unwrap()
            .unwrap();
        assert_eq!(observed.observed_at, 100);
        let CatalogValue::Track(first) = observed.value else {
            panic!("track missing")
        };
        assert_eq!(first.media.name, "Updated");
        assert_eq!(first.album.unwrap().media.item_id.as_str(), "album-a");
        assert_eq!((first.disc_number, first.track_number), (1, 4));
        assert_eq!(first.image.unwrap().path, "https://example.com/album-a.jpg");
        let other = UserId("other".into());
        assert!(read_track_read(&transaction, &query(), &other)
            .unwrap()
            .is_none());
        let Some(CatalogValue::Item(MediaItem::Track(partial))) =
            read_partial_track(&transaction, &alias_query(), &other).unwrap()
        else {
            panic!("partial track missing")
        };
        assert_eq!(partial.media.provider.as_str(), "library");
        assert_eq!(partial.album.unwrap().media.item_id.as_str(), "album-b");
        assert_eq!(partial.media.favorite, FieldKnowledge::Unknown);
        assert!(supports_track_read(&CatalogQuery::Search {
            query: "needle".into(),
            media_type: MediaType::Track,
            limit: 25
        }));
    }

    #[test]
    fn track_single_read_merge_preserves_details_and_known_null_context() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        write_track_read(
            &transaction,
            &query(),
            &user,
            &CatalogValue::Track(track("album-a", 4)),
            100,
        )
        .unwrap();
        let incoming: Track = serde_json::from_value(json!({"provider":"library","item_id":"007","name":"Partial","favorite":null,"album":null,"image":null,"position":null})).unwrap();
        write_track_read(
            &transaction,
            &alias_query(),
            &user,
            &CatalogValue::Item(MediaItem::Track(incoming)),
            101,
        )
        .unwrap();
        let CatalogValue::Item(MediaItem::Track(current)) =
            read_track_read(&transaction, &alias_query(), &user)
                .unwrap()
                .unwrap()
                .value
        else {
            panic!("track missing")
        };
        assert_eq!(current.duration, 240);
        assert!(current.knowledge.duration);
        assert_eq!(
            current.media.metadata.unwrap().description.as_deref(),
            Some("Retained")
        );
        assert!(current.album.is_none());
        assert!(current.knowledge.album);
        assert!(current.image.is_none());
        assert!(current.knowledge.image);
        assert_eq!(current.media.favorite, FieldKnowledge::Known(None));
        assert_eq!(current.media.position, None);
        assert!(current.media.knowledge.position);
        let CatalogValue::Track(previous) = read_track_read(&transaction, &query(), &user)
            .unwrap()
            .unwrap()
            .value
        else {
            panic!("track missing")
        };
        assert_eq!(previous.album.unwrap().media.item_id.as_str(), "album-a");
        assert_eq!(previous.track_number, 4);
    }

    #[test]
    fn track_single_reads_reject_wrong_values_and_ambiguous_aliases() {
        let mut connection = database();
        let transaction = connection.transaction().unwrap();
        let user = UserId("user".into());
        assert!(write_track_read(
            &transaction,
            &query(),
            &user,
            &CatalogValue::Tracks(vec![track("album-a", 4)]),
            100
        )
        .is_err());
        assert!(read_track_read(&transaction, &query(), &user)
            .unwrap()
            .is_none());
        let observed = track("album-a", 4);
        write_track_read(
            &transaction,
            &query(),
            &user,
            &CatalogValue::Track(observed.clone()),
            100,
        )
        .unwrap();
        let mut duplicate = observed;
        duplicate.media.item_id = MediaId("008".into());
        write_track_observation(&transaction, &duplicate, &user).unwrap();
        assert!(read_partial_track(&transaction, &alias_query(), &user)
            .unwrap()
            .is_none());
        assert!(transaction
            .execute("UPDATE track_read_states SET album_item_id='missing'", [])
            .is_err());
    }
}
