//! Album credit and membership evidence needed by artist selection.
//! The canonical album owner creates the parent before writing these ordered relationships.
//! The caller owns the transaction; nested artist observations never imply library membership.

use rusqlite::{params, Transaction};

use crate::domain::identifiers::UserId;
use crate::domain::media::{Album, FieldKnowledge};

use super::artists::write_artist_observation;
use super::database::CacheError;

// Schema and transactional relationship observations.
pub(super) fn initialize_album_artist_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE albums (
            provider TEXT NOT NULL, item_id TEXT NOT NULL, name TEXT NOT NULL,
            artists_known INTEGER NOT NULL CHECK (artists_known IN (0,1)),
            provider_mappings_known INTEGER NOT NULL CHECK (provider_mappings_known IN (0,1)),
            PRIMARY KEY(provider,item_id)
        ) STRICT, WITHOUT ROWID;
        CREATE TABLE album_artists (
            album_provider TEXT NOT NULL, album_item_id TEXT NOT NULL,
            position INTEGER NOT NULL CHECK (position>=0),
            artist_provider TEXT NOT NULL, artist_item_id TEXT NOT NULL,
            PRIMARY KEY(album_provider,album_item_id,position),
            FOREIGN KEY(album_provider,album_item_id) REFERENCES albums(provider,item_id) ON DELETE CASCADE,
            FOREIGN KEY(artist_provider,artist_item_id) REFERENCES artists(provider,item_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX album_artist_lookup_idx ON album_artists
            (artist_provider,artist_item_id,album_provider,album_item_id);
        CREATE TABLE album_provider_mappings (
            album_provider TEXT NOT NULL, album_item_id TEXT NOT NULL,
            position INTEGER NOT NULL CHECK (position>=0),
            provider_domain TEXT NOT NULL, provider_instance TEXT NOT NULL, item_id TEXT NOT NULL,
            in_library INTEGER CHECK (in_library IN (0,1)),
            in_library_known INTEGER NOT NULL CHECK (in_library_known IN (0,1)),
            PRIMARY KEY(album_provider,album_item_id,position),
            UNIQUE(album_provider,album_item_id,provider_domain,provider_instance,item_id),
            FOREIGN KEY(album_provider,album_item_id) REFERENCES albums(provider,item_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX album_membership_lookup_idx ON album_provider_mappings
            (album_provider,album_item_id,in_library,provider_instance,in_library_known);",
    )?;
    Ok(())
}

/// Updates observed relationships; the canonical album owner must already have written the parent.
pub(super) fn write_album_relations(
    transaction: &Transaction<'_>,
    album: &Album,
    user_id: &UserId,
) -> Result<(), CacheError> {
    let provider = album.media.provider.as_str();
    let item_id = album.media.item_id.as_str();
    let are_artists_known = album.knowledge.artists;
    let are_mappings_known = album.media.knowledge.provider_mappings;
    if are_artists_known {
        transaction.execute(
            "DELETE FROM album_artists WHERE album_provider=?1 AND album_item_id=?2",
            params![provider, item_id],
        )?;
        for (position, artist) in album.artists.iter().enumerate() {
            write_artist_observation(transaction, artist, user_id, FieldKnowledge::Unknown)?;
            transaction.execute(
                "INSERT INTO album_artists(album_provider,album_item_id,position,artist_provider,artist_item_id)
                 VALUES (?1,?2,?3,?4,?5)",
                params![provider,item_id,position as i64,artist.media.provider.as_str(),artist.media.item_id.as_str()],
            )?;
        }
    }
    if are_mappings_known {
        transaction.execute(
            "DELETE FROM album_provider_mappings WHERE album_provider=?1 AND album_item_id=?2",
            params![provider, item_id],
        )?;
        for (position, mapping) in album.media.provider_mappings.iter().enumerate() {
            let (in_library, is_in_library_known) = if mapping.knowledge.in_library {
                (mapping.in_library, true)
            } else {
                (None, false)
            };
            transaction.execute(
                "INSERT INTO album_provider_mappings(album_provider,album_item_id,position,
                    provider_domain,provider_instance,item_id,in_library,in_library_known)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    provider,
                    item_id,
                    position as i64,
                    mapping.provider_domain.as_str(),
                    mapping.provider_instance.as_str(),
                    mapping.item_id.as_str(),
                    in_library,
                    is_in_library_known
                ],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::identifiers::ProviderInstanceId;
    use rusqlite::Connection;

    // Isolated canonical observations and transaction boundaries.
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

    fn album(value: serde_json::Value) -> Album {
        serde_json::from_value(value).unwrap()
    }

    fn observe(connection: &mut Connection, album: &Album) -> Result<(), CacheError> {
        let transaction = connection.transaction()?;
        super::super::albums::write_album_observation(
            &transaction,
            album,
            &UserId("user".to_owned()),
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn complete_album() -> Album {
        album(
            serde_json::json!({"provider":"library","item_id":"007","name":"Album",
            "artists":[{"provider":"library","item_id":"a","name":"Artist"},
                {"provider":"library","item_id":"b","name":"Other"}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a",
                "item_id":"007","in_library":true}]}),
        )
    }

    fn observe_candidate_artist(connection: &mut Connection, item_id: &str, artist_type: &str) {
        let artist =
            serde_json::from_value(serde_json::json!({"provider":"library","item_id":item_id,
            "name":item_id,"sort_name":item_id,"artist_type":artist_type,
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a",
                "item_id":item_id,"in_library":true}]}))
            .unwrap();
        let transaction = connection.transaction().unwrap();
        write_artist_observation(
            &transaction,
            &artist,
            &UserId("user".to_owned()),
            FieldKnowledge::Known(true),
        )
        .unwrap();
        transaction.commit().unwrap();
    }

    fn candidate_ids(
        connection: &mut Connection,
        query: &super::super::artist_candidates::ArtistCandidateQuery,
    ) -> Vec<String> {
        let transaction = connection.transaction().unwrap();
        let result =
            super::super::artist_candidates::read_artist_candidates(&transaction, query).unwrap();
        assert_eq!(
            result.knowledge,
            super::super::artist_candidates::ArtistCandidateKnowledge::Partial
        );
        result
            .items
            .into_iter()
            .map(|artist| artist.media.item_id.0)
            .collect()
    }

    // Artist selection uses credit and same-row visible membership before hydration.
    #[test]
    fn album_artist_relations_candidates_require_same_row_visible_membership_without_fanout() {
        use crate::domain::identifiers::ProviderInstanceId;
        let mut connection = database();
        observe_candidate_artist(&mut connection, "a", "singer");
        observe_candidate_artist(&mut connection, "b", "singer");
        let incoming = album(
            serde_json::json!({"provider":"library","item_id":"007","name":"Album",
            "artists":[{"provider":"library","item_id":"a","name":"a"}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"x","in_library":false},
                {"provider_domain":"source","provider_instance":"source-b","item_id":"x","in_library":true},
                {"provider_domain":"source","provider_instance":"source-c","item_id":"x","in_library":true}]}),
        );
        observe(&mut connection, &incoming).unwrap();
        let mut query =
            super::super::artist_candidates::ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.album_artists_only = true;
        query.provider_instances = Some(vec![ProviderInstanceId("source-a".to_owned())]);
        query.visible_album_provider_instances =
            Some(vec![ProviderInstanceId("source-a".to_owned())]);
        assert!(candidate_ids(&mut connection, &query).is_empty());
        query.visible_album_provider_instances = Some(vec![
            ProviderInstanceId("source-b".to_owned()),
            ProviderInstanceId("source-c".to_owned()),
        ]);
        assert_eq!(candidate_ids(&mut connection, &query), ["a"]);
        query.visible_album_provider_instances = Some(Vec::new());
        assert!(candidate_ids(&mut connection, &query).is_empty());
        query.album_artists_only = false;
        assert_eq!(candidate_ids(&mut connection, &query), ["a", "b"]);
    }

    #[test]
    fn album_artist_relations_candidates_preserve_unknown_and_honor_explicit_removal() {
        let mut connection = database();
        observe_candidate_artist(&mut connection, "a", "singer");
        observe(&mut connection, &complete_album()).unwrap();
        let mut query =
            super::super::artist_candidates::ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.album_artists_only = true;
        assert_eq!(candidate_ids(&mut connection, &query), ["a"]);
        observe(
            &mut connection,
            &album(serde_json::json!({"provider":"library","item_id":"007","name":"Summary"})),
        )
        .unwrap();
        assert_eq!(candidate_ids(&mut connection, &query), ["a"]);
        observe(&mut connection,&album(serde_json::json!({"provider":"library","item_id":"007","name":"Changed",
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007","in_library":null}]}))).unwrap();
        assert!(candidate_ids(&mut connection, &query).is_empty());
        observe(&mut connection, &complete_album()).unwrap();
        observe(&mut connection,&album(serde_json::json!({"provider":"library","item_id":"007","name":"Empty","artists":[]}))).unwrap();
        assert!(candidate_ids(&mut connection, &query).is_empty());
    }

    #[test]
    fn album_artist_relations_candidates_do_not_count_provider_albums_or_unobserved_membership() {
        let mut connection = database();
        observe_candidate_artist(&mut connection, "a", "singer");
        for (provider, is_in_library) in [("source-a", Some(true)), ("library", None)] {
            let mut value = serde_json::json!({"provider":provider,"item_id":"007","name":"Album",
                "artists":[{"provider":"library","item_id":"a","name":"a"}],
                "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007"}]});
            if let Some(is_in_library) = is_in_library {
                value["provider_mappings"][0]["in_library"] = serde_json::json!(is_in_library);
            }
            observe(&mut connection, &album(value)).unwrap();
        }
        let mut query =
            super::super::artist_candidates::ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.album_artists_only = true;
        assert!(candidate_ids(&mut connection, &query).is_empty());
    }

    #[test]
    fn album_artist_relations_candidates_other_roles_skip_album_only_filter_like_server() {
        let mut connection = database();
        observe_candidate_artist(&mut connection, "a", "singer");
        observe_candidate_artist(&mut connection, "b", "author");
        let mut query =
            super::super::artist_candidates::ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.album_artists_only = true;
        query.artist_type = Some(crate::domain::catalog::LibraryArtistType::Author);
        assert_eq!(candidate_ids(&mut connection, &query), ["b"]);
        query.artist_type = Some(crate::domain::catalog::LibraryArtistType::Singer);
        assert!(candidate_ids(&mut connection, &query).is_empty());
    }

    #[test]
    fn album_artist_relations_mapping_summary_preserves_known_value_only_for_same_identity() {
        let mut connection = database();
        observe_candidate_artist(&mut connection, "a", "singer");
        observe(&mut connection, &complete_album()).unwrap();
        let mut query =
            super::super::artist_candidates::ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.album_artists_only = true;
        let mut summary = serde_json::json!({"provider":"library","item_id":"007","name":"Summary",
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007"}]});
        observe(&mut connection, &album(summary.clone())).unwrap();
        assert_eq!(candidate_ids(&mut connection, &query), ["a"]);
        summary["provider_mappings"][0]["provider_instance"] = serde_json::json!("source-b");
        observe(&mut connection, &album(summary)).unwrap();
        assert!(candidate_ids(&mut connection, &query).is_empty());
    }

    #[test]
    fn album_artist_relations_unknown_model_value_never_counts_as_membership() {
        let mut connection = database();
        observe_candidate_artist(&mut connection, "a", "singer");
        let mut incoming = complete_album();
        incoming.media.provider_mappings[0].knowledge.in_library = false;
        observe(&mut connection, &incoming).unwrap();
        let mut query =
            super::super::artist_candidates::ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.album_artists_only = true;
        assert!(candidate_ids(&mut connection, &query).is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn album_artist_relations_filter_survives_worker_restart_with_user_favorite() {
        use super::super::workers::DatabaseWorkers;
        let directory = tempfile::tempdir().unwrap();
        let workers = DatabaseWorkers::initialize(directory.path().to_owned(), "server".to_owned())
            .await
            .unwrap();
        let incoming = complete_album();
        workers
            .write_transaction(move |transaction| {
                let artist =
                    serde_json::from_value(serde_json::json!({"provider":"library","item_id":"a",
                "name":"Artist","favorite":true,"artist_type":"singer"}))?;
                write_artist_observation(
                    transaction,
                    &artist,
                    &UserId("user".to_owned()),
                    FieldKnowledge::Known(true),
                )?;
                super::super::albums::write_album_observation(
                    transaction,
                    &incoming,
                    &UserId("user".to_owned()),
                )
            })
            .await
            .unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "server".to_owned())
                .await
                .unwrap();
        let result = reopened
            .read_snapshot(|snapshot| {
                let mut query = super::super::artist_candidates::ArtistCandidateQuery::new(UserId(
                    "user".to_owned(),
                ));
                query.album_artists_only = true;
                query.favorite = Some(true);
                query.visible_album_provider_instances =
                    Some(vec![ProviderInstanceId("source-a".to_owned())]);
                super::super::artist_candidates::read_artist_candidates(snapshot, &query)
            })
            .await
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].media.item_id.as_str(), "a");
        assert_eq!(
            result.items[0].media.favorite,
            FieldKnowledge::Known(Some(true))
        );
        reopened.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    #[test]
    fn album_artist_relations_filter_uses_covering_credit_and_membership_indexes() {
        let connection = database();
        let plans = connection.prepare(
            "EXPLAIN QUERY PLAN SELECT EXISTS(SELECT 1 FROM album_artists credit
             JOIN album_provider_mappings mapping ON mapping.album_provider=credit.album_provider
                AND mapping.album_item_id=credit.album_item_id AND mapping.in_library=1
                AND mapping.in_library_known=1
             WHERE credit.album_provider='library' AND credit.artist_provider=?1 AND credit.artist_item_id=?2)",
        ).unwrap().query_map(params!["library","a"],|row|row.get::<_,String>(3)).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap();
        for index in ["album_artist_lookup_idx", "album_membership_lookup_idx"] {
            assert!(
                plans
                    .iter()
                    .any(|plan| plan.contains(&format!("USING COVERING INDEX {index}"))),
                "unexpected query plan: {plans:?}"
            );
        }
    }

    #[test]
    fn album_artist_relations_filter_and_pagination_precede_detail_reconstruction() {
        let mut connection = database();
        observe_candidate_artist(&mut connection, "a", "singer");
        observe_candidate_artist(&mut connection, "b", "singer");
        observe(&mut connection,&album(serde_json::json!({"provider":"library","item_id":"album","name":"Album",
            "artists":[{"provider":"library","item_id":"b","name":"b"}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"album","in_library":true}]}))).unwrap();
        connection.execute("UPDATE artists SET metadata_known=1,metadata='{\"description\":42}' WHERE item_id='a'",[]).unwrap();
        let mut query =
            super::super::artist_candidates::ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.album_artists_only = true;
        query.projection = super::super::artists::ArtistReadProjection::Detail;
        query.limit = std::num::NonZeroU32::new(1).unwrap();
        assert_eq!(candidate_ids(&mut connection, &query), ["b"]);
        query.offset = 1;
        assert!(candidate_ids(&mut connection, &query).is_empty());
    }

    // Knowledge preservation, scoped identity and atomic failure.
    #[test]
    fn album_artist_relations_preserve_order_without_implying_artist_membership() {
        let mut connection = database();
        observe(&mut connection, &complete_album()).unwrap();
        let credits = connection
            .prepare("SELECT artist_item_id FROM album_artists ORDER BY position")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(credits, ["a", "b"]);
        let memberships: i64 = connection
            .query_row("SELECT COUNT(*) FROM artist_library_entries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(memberships, 0);
        let in_library: bool = connection
            .query_row(
                "SELECT in_library FROM album_provider_mappings",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(in_library);
    }

    #[test]
    fn album_artist_relations_unknown_summary_preserves_known_and_explicit_empty_clears() {
        let mut connection = database();
        observe(&mut connection, &complete_album()).unwrap();
        observe(
            &mut connection,
            &album(serde_json::json!({"provider":"library","item_id":"007","name":"Summary"})),
        )
        .unwrap();
        let counts: (i64,i64) = connection.query_row(
            "SELECT (SELECT COUNT(*) FROM album_artists),(SELECT COUNT(*) FROM album_provider_mappings)",[],
            |row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(counts, (2, 1));
        observe(
            &mut connection,
            &album(
                serde_json::json!({"provider":"library","item_id":"007","name":"Empty",
            "artists":[],"provider_mappings":[]}),
            ),
        )
        .unwrap();
        let counts: (i64,i64) = connection.query_row(
            "SELECT (SELECT COUNT(*) FROM album_artists),(SELECT COUNT(*) FROM album_provider_mappings)",[],
            |row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(counts, (0, 0));
        let knowledge: (bool, bool) = connection
            .query_row(
                "SELECT artists_known,provider_mappings_known FROM albums",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(knowledge, (true, true));
    }

    #[test]
    fn album_artist_relations_later_invalid_credit_rolls_back_header_and_previous_credits() {
        let mut connection = database();
        let previous = complete_album();
        observe(&mut connection, &previous).unwrap();
        let invalid = album(
            serde_json::json!({"provider":"library","item_id":"007","name":"Changed",
            "artists":[{"provider":"library","item_id":"new","name":"New"},
                {"provider":"library","item_id":"bad","name":"Bad","external_ids":[["discogs"]]}]}),
        );
        assert!(observe(&mut connection, &invalid).is_err());
        let name: String = connection
            .query_row("SELECT name FROM albums", [], |row| row.get(0))
            .unwrap();
        assert_eq!(name, previous.media.name);
        let counts: (i64,i64) = connection.query_row(
            "SELECT (SELECT COUNT(*) FROM album_artists),(SELECT COUNT(*) FROM artists WHERE item_id IN ('new','bad'))",[],
            |row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(counts, (2, 0));
    }

    #[test]
    fn album_artist_relations_provider_and_library_album_identity_remain_separate() {
        let mut connection = database();
        observe(&mut connection, &complete_album()).unwrap();
        let provider_album = album(
            serde_json::json!({"provider":"source-a","item_id":"007","name":"Provider Album",
            "artists":[{"provider":"source-a","item_id":"a","name":"Provider Artist"}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007"}]}),
        );
        observe(&mut connection, &provider_album).unwrap();
        let rows = connection.prepare("SELECT album_provider,artist_provider FROM album_artists WHERE artist_item_id='a' ORDER BY album_provider")
            .unwrap().query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert_eq!(
            rows,
            [
                ("library".to_owned(), "library".to_owned()),
                ("source-a".to_owned(), "source-a".to_owned())
            ]
        );
        let known: bool = connection.query_row("SELECT in_library_known FROM album_provider_mappings WHERE album_provider='source-a'",[],|row|row.get(0)).unwrap();
        assert!(!known);
    }
}
