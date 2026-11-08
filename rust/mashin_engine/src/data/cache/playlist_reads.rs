//! Exact playlist selections retain user/query coverage separately from canonical metadata.
//! The caller owns the snapshot or writer transaction and its atomic completion.

use super::database::CacheError;
use super::playlists::{read_playlist, write_playlist_observation};
use super::reads::CatalogReadObservation;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{MediaItem, MediaType, Playlist};
use rusqlite::{params, OptionalExtension, Transaction};

// Schema and selection writes.
pub(super) fn initialize_playlist_read_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch("CREATE TABLE playlist_read_states (
        user_id TEXT NOT NULL REFERENCES users(user_id),query TEXT NOT NULL,observed_at INTEGER NOT NULL CHECK(observed_at>=0),
        PRIMARY KEY(user_id,query)) STRICT, WITHOUT ROWID;
        CREATE TABLE playlist_read_items (
        user_id TEXT NOT NULL,query TEXT NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),provider TEXT NOT NULL,item_id TEXT NOT NULL,
        position INTEGER CHECK(position>=0),position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),PRIMARY KEY(user_id,query,occurrence),
        FOREIGN KEY(user_id,query) REFERENCES playlist_read_states(user_id,query) ON DELETE CASCADE,
        FOREIGN KEY(provider,item_id) REFERENCES playlists(provider,item_id)) STRICT, WITHOUT ROWID;
        CREATE INDEX playlist_read_items_identity_idx ON playlist_read_items(user_id,provider,item_id,query);")?;
    Ok(())
}

pub(super) fn supports_playlist_read(query: &CatalogQuery) -> bool {
    matches!(
        query,
        CatalogQuery::Playlist { .. }
            | CatalogQuery::LibraryPlaylists { .. }
            | CatalogQuery::ProviderItem {
                media_type: MediaType::Playlist,
                ..
            }
            | CatalogQuery::Search {
                media_type: MediaType::Playlist,
                ..
            }
    )
}

pub(super) fn write_playlist_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    let items: Vec<&Playlist> = match (query, value) {
        (CatalogQuery::LibraryPlaylists { .. }, CatalogValue::Playlists(items))
            if items
                .iter()
                .all(|item| item.media.provider.as_str() == "library") =>
        {
            items.iter().collect()
        }
        (CatalogQuery::Playlist { .. }, CatalogValue::Playlist(item)) => vec![item],
        (
            CatalogQuery::ProviderItem {
                media_type: MediaType::Playlist,
                ..
            },
            CatalogValue::Item(MediaItem::Playlist(item)),
        ) => vec![item],
        (
            CatalogQuery::Search {
                media_type: MediaType::Playlist,
                ..
            },
            CatalogValue::Search(result),
        ) if result.artists.is_empty()
            && result.albums.is_empty()
            && result.tracks.is_empty()
            && result.radio.is_empty() =>
        {
            result.playlists.iter().collect()
        }
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "playlist query and response do not match",
            ))
        }
    };
    let key = serde_json::to_string(query)?;
    transaction.execute(
        "INSERT INTO users(user_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [user.as_str()],
    )?;
    transaction.execute("INSERT INTO playlist_read_states(user_id,query,observed_at) VALUES(?1,?2,?3) ON CONFLICT(user_id,query) DO UPDATE SET observed_at=excluded.observed_at",params![user.as_str(),key,observed_at])?;
    transaction.execute(
        "DELETE FROM playlist_read_items WHERE user_id=?1 AND query=?2",
        params![user.as_str(), key],
    )?;
    for (occurrence, item) in items.into_iter().enumerate() {
        write_playlist_observation(transaction, item, user)?;
        transaction.execute("INSERT INTO playlist_read_items(user_id,query,occurrence,provider,item_id,position,position_known) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![user.as_str(),key,occurrence as i64,item.media.provider.as_str(),item.media.item_id.as_str(),item.media.position,item.media.knowledge.position])?;
    }
    Ok(())
}

// Confirmed metadata replaces canonical observations, not playlist contents.
pub(super) fn commit_confirmed_playlist(
    transaction: &Transaction<'_>,
    playlist: &Playlist,
    user: &UserId,
) -> Result<(), CacheError> {
    invalidate_playlist_metadata_reads(transaction, playlist, user)?;
    write_playlist_observation(transaction, playlist, user)?;
    invalidate_playlist_metadata_reads(transaction, playlist, user)?;
    super::recommendation_reads::invalidate_user_recommendation_reads(transaction, user)?;
    Ok(())
}

// The server removed the library identity; separately observed provider objects remain intact.
pub(super) fn commit_confirmed_playlist_removal(
    transaction: &Transaction<'_>,
    playlist: &Playlist,
    user: &UserId,
) -> Result<(), CacheError> {
    if playlist.media.provider.as_str() != "library" || playlist.media.item_id.as_str().is_empty() {
        return Err(CacheError::InvalidMediaObservation(
            "playlist removal requires a library identity",
        ));
    }
    let mut identities = vec![(
        playlist.media.provider.as_str().to_owned(),
        playlist.media.item_id.as_str().to_owned(),
    )];
    let cached = read_playlist(
        transaction,
        &playlist.media.provider,
        &playlist.media.item_id,
        user,
    )?;
    for observation in std::iter::once(playlist).chain(cached.as_ref()) {
        for mapping in &observation.media.provider_mappings {
            identities.push((
                mapping.provider_instance.as_str().to_owned(),
                mapping.item_id.as_str().to_owned(),
            ));
            identities.push((
                mapping.provider_domain.as_str().to_owned(),
                mapping.item_id.as_str().to_owned(),
            ));
        }
    }
    identities.sort_unstable();
    identities.dedup();
    invalidate_playlist_metadata_reads(transaction, playlist, user)?;
    for (provider, item_id) in identities {
        transaction.execute("DELETE FROM playlist_read_states AS state WHERE
            (json_extract(state.query,'$.Playlist.provider')=?1 AND json_extract(state.query,'$.Playlist.item_id')=?2) OR
            (json_extract(state.query,'$.ProviderItem.provider')=?1 AND json_extract(state.query,'$.ProviderItem.item_id')=?2) OR
            EXISTS(SELECT 1 FROM playlist_read_items item WHERE item.user_id=state.user_id AND item.query=state.query AND item.provider=?1 AND item.item_id=?2)", params![provider,item_id])?;
        super::track_reads::invalidate_removed_playlist_content_reads(
            transaction,
            &provider,
            &item_id,
        )?;
        super::recommendation_reads::invalidate_removed_playlist_recommendation_reads(
            transaction,
            user,
            &provider,
            &item_id,
        )?;
    }
    transaction.execute(
        "DELETE FROM playlists WHERE provider=?1 AND item_id=?2",
        params![
            playlist.media.provider.as_str(),
            playlist.media.item_id.as_str()
        ],
    )?;
    Ok(())
}

// Exact selections and conservative identity-only partial reads.
pub(super) fn read_playlist_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    let key = serde_json::to_string(query)?;
    let time = transaction
        .query_row(
            "SELECT observed_at FROM playlist_read_states WHERE user_id=?1 AND query=?2",
            params![user.as_str(), key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(observed_at) = time else {
        return Ok(None);
    };
    let mut statement = transaction.prepare("SELECT provider,item_id,position,position_known FROM playlist_read_items WHERE user_id=?1 AND query=?2 ORDER BY occurrence")?;
    let references = statement
        .query_map(params![user.as_str(), key], |row| {
            Ok((
                ProviderInstanceId(row.get(0)?),
                MediaId(row.get(1)?),
                row.get::<_, Option<u32>>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut items = Vec::new();
    let is_summary = matches!(query, CatalogQuery::LibraryPlaylists { summary: true, .. });
    for (provider, item_id, position, is_known) in references {
        let mut item = if is_summary {
            super::playlists::read_playlist_summary(transaction, &provider, &item_id, user)
        } else {
            read_playlist(transaction, &provider, &item_id, user)
        }?
        .ok_or(CacheError::InvalidMediaObservation(
            "canonical playlist missing",
        ))?;
        if !is_summary {
            item.media.position = position;
            item.media.knowledge.position = is_known;
        }
        items.push(item);
    }
    let value = match query {
        CatalogQuery::LibraryPlaylists { .. } => CatalogValue::Playlists(items),
        CatalogQuery::Playlist { .. } if items.len() == 1 => {
            CatalogValue::Playlist(items.remove(0))
        }
        CatalogQuery::ProviderItem {
            media_type: MediaType::Playlist,
            ..
        } if items.len() == 1 => CatalogValue::Item(MediaItem::Playlist(items.remove(0))),
        CatalogQuery::Search {
            media_type: MediaType::Playlist,
            ..
        } => CatalogValue::Search(crate::domain::media::MusicAssistantSearchResults {
            playlists: items,
            ..Default::default()
        }),
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "playlist selection has invalid shape",
            ))
        }
    };
    Ok(Some(CatalogReadObservation { value, observed_at }))
}

pub(super) fn read_partial_playlist(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user: &UserId,
) -> Result<Option<CatalogValue>, CacheError> {
    let (provider, item_id) = match query {
        CatalogQuery::Playlist { provider, item_id }
        | CatalogQuery::ProviderItem {
            provider,
            item_id,
            media_type: MediaType::Playlist,
        } => (provider, item_id),
        _ => return Ok(None),
    };
    let mut statement = transaction.prepare("SELECT provider,item_id FROM playlists WHERE provider=?1 AND item_id=?2
        UNION SELECT provider,playlist_id FROM playlist_provider_mappings WHERE provider='library' AND item_id=?2 AND (provider_instance=?1 OR provider_domain=?1) LIMIT 2")?;
    let identities = statement
        .query_map(params![provider, item_id.as_str()], |row| {
            Ok((ProviderInstanceId(row.get(0)?), MediaId(row.get(1)?)))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if identities.len() != 1 {
        return Ok(None);
    }
    let item = read_playlist(transaction, &identities[0].0, &identities[0].1, user)?;
    Ok(item.map(|item| {
        if matches!(query, CatalogQuery::Playlist { .. }) {
            CatalogValue::Playlist(item)
        } else {
            CatalogValue::Item(MediaItem::Playlist(item))
        }
    }))
}

// Both old and newly observed mapping identities may have their own query coverage.
fn invalidate_playlist_metadata_reads(
    transaction: &Transaction<'_>,
    playlist: &Playlist,
    user: &UserId,
) -> Result<(), CacheError> {
    transaction.execute("WITH identities(provider,item_id) AS (
        SELECT ?2,?3 UNION SELECT provider_instance,item_id FROM playlist_provider_mappings WHERE provider=?2 AND playlist_id=?3
        UNION SELECT provider_domain,item_id FROM playlist_provider_mappings WHERE provider=?2 AND playlist_id=?3)
        DELETE FROM playlist_read_states AS state WHERE state.user_id=?1 AND (
        json_type(state.query,'$.LibraryPlaylists') IS NOT NULL OR json_type(state.query,'$.Search') IS NOT NULL OR
        EXISTS(SELECT 1 FROM identities identity WHERE
            (json_extract(state.query,'$.Playlist.provider')=identity.provider AND json_extract(state.query,'$.Playlist.item_id')=identity.item_id) OR
            (json_extract(state.query,'$.ProviderItem.provider')=identity.provider AND json_extract(state.query,'$.ProviderItem.item_id')=identity.item_id) OR
            EXISTS(SELECT 1 FROM playlist_read_items item WHERE item.user_id=state.user_id AND item.query=state.query AND item.provider=identity.provider AND item.item_id=identity.item_id)))",
        params![user.as_str(),playlist.media.provider.as_str(),playlist.media.item_id.as_str()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn confirmed_playlist_removal_is_atomic_preserves_unrelated_data_and_survives_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mashin-media-cache.sqlite3");
        let mut connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let other = UserId("other".into());
        let unaffected = UserId("unaffected".into());
        let original: Playlist = serde_json::from_value(json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Removed","favorite":true,
            "access":{"owner":"user","sharing":"private"},"external_ids":[["custom","identity"]],
            "provider_mappings":[{"provider_domain":"old-domain","provider_instance":"old-a","item_id":"old"}]})).unwrap();
        let library = CatalogQuery::LibraryPlaylists {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
        };
        let contents = CatalogQuery::PlaylistTracks {
            provider: "library".into(),
            item_id: MediaId("42".into()),
            allow_dynamic_tracks: false,
        };
        let recommendations = CatalogQuery::RecommendationFolders;
        let folders = CatalogValue::RecommendationFolders(serde_json::from_value(json!([{"provider":"recommendations","item_id":"folder","name":"Folder","items":[MediaItem::Playlist(original.clone())]}])).unwrap());
        let tracks = CatalogValue::Tracks(
            serde_json::from_value(json!([{"provider":"library","item_id":"track","name":"Kept"}]))
                .unwrap(),
        );
        for owner in [&user, &other] {
            for (query, value) in [
                (&library, CatalogValue::Playlists(vec![original.clone()])),
                (&contents, tracks.clone()),
                (&recommendations, folders.clone()),
            ] {
                super::super::reads::write_cached_read(&transaction, query, owner, &value, 100)
                    .unwrap();
            }
        }
        for (query, value) in [
            (&library, CatalogValue::Playlists(Vec::new())),
            (
                &recommendations,
                CatalogValue::RecommendationFolders(Vec::new()),
            ),
        ] {
            super::super::reads::write_cached_read(&transaction, query, &unaffected, &value, 100)
                .unwrap();
        }
        let aliases = [
            ("old-a", "old"),
            ("old-domain", "old"),
            ("new-a", "new"),
            ("new-domain", "new"),
        ];
        for (provider, item_id) in aliases {
            let mut alias = original.clone();
            alias.media.provider = ProviderInstanceId(provider.into());
            alias.media.item_id = MediaId(item_id.into());
            write_playlist_read(
                &transaction,
                &CatalogQuery::Playlist {
                    provider: provider.into(),
                    item_id: MediaId(item_id.into()),
                },
                &other,
                &CatalogValue::Playlist(alias),
                100,
            )
            .unwrap();
            super::super::reads::write_cached_read(
                &transaction,
                &CatalogQuery::PlaylistTracks {
                    provider: provider.into(),
                    item_id: MediaId(item_id.into()),
                    allow_dynamic_tracks: false,
                },
                &other,
                &tracks,
                100,
            )
            .unwrap();
        }
        transaction.commit().unwrap();
        let mut removed = original.clone();
        removed.media.provider_mappings = serde_json::from_value(
            json!([{"provider_domain":"new-domain","provider_instance":"new-a","item_id":"new"}]),
        )
        .unwrap();
        {
            let failed = connection.transaction().unwrap();
            failed.execute_batch("CREATE TRIGGER fail_removal BEFORE DELETE ON playlists WHEN OLD.provider='library' AND OLD.item_id='42' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
            assert!(commit_confirmed_playlist_removal(&failed, &removed, &user).is_err());
        }
        let transaction = connection.transaction().unwrap();
        assert!(read_playlist_read(&transaction, &library, &user)
            .unwrap()
            .is_some());
        assert!(
            super::super::reads::read_cached_read(&transaction, &recommendations, &other)
                .unwrap()
                .is_some()
        );
        commit_confirmed_playlist_removal(&transaction, &removed, &user).unwrap();
        commit_confirmed_playlist_removal(&transaction, &removed, &user).unwrap();
        for owner in [&user, &other] {
            for query in [&library, &contents, &recommendations] {
                assert!(
                    super::super::reads::read_cached_read(&transaction, query, owner)
                        .unwrap()
                        .is_none()
                );
            }
        }
        assert!(read_playlist_read(&transaction, &library, &unaffected)
            .unwrap()
            .is_some());
        assert!(
            super::super::reads::read_cached_read(&transaction, &recommendations, &unaffected)
                .unwrap()
                .is_some()
        );
        for (provider, item_id) in aliases {
            assert!(read_playlist_read(
                &transaction,
                &CatalogQuery::Playlist {
                    provider: provider.into(),
                    item_id: MediaId(item_id.into())
                },
                &other
            )
            .unwrap()
            .is_none());
            assert!(super::super::reads::read_cached_read(
                &transaction,
                &CatalogQuery::PlaylistTracks {
                    provider: provider.into(),
                    item_id: MediaId(item_id.into()),
                    allow_dynamic_tracks: false
                },
                &other
            )
            .unwrap()
            .is_none());
            assert!(read_playlist(
                &transaction,
                &ProviderInstanceId(provider.into()),
                &MediaId(item_id.into()),
                &other
            )
            .unwrap()
            .is_some());
        }
        transaction.commit().unwrap();
        drop(connection);
        let mut reopened = rusqlite::Connection::open(&path).unwrap();
        reopened.pragma_update(None, "foreign_keys", true).unwrap();
        let snapshot = reopened.transaction().unwrap();
        assert!(read_playlist(
            &snapshot,
            &original.media.provider,
            &original.media.item_id,
            &user
        )
        .unwrap()
        .is_none());
        assert!(super::super::tracks::read_track(
            &snapshot,
            &ProviderInstanceId("library".into()),
            &MediaId("track".into()),
            &user
        )
        .unwrap()
        .is_some());
        assert_eq!(snapshot.query_row("SELECT COUNT(*) FROM playlist_favorites WHERE provider='library' AND item_id='42'", [], |row|row.get::<_,i64>(0)).unwrap(), 0);
        assert!(snapshot
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none());
    }

    #[test]
    fn confirmed_playlist_creation_invalidates_empty_selections_and_survives_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mashin-media-cache.sqlite3");
        let mut connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let other = UserId("other".into());
        let library = CatalogQuery::LibraryPlaylists {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: true,
        };
        let search = CatalogQuery::Search {
            query: "New".into(),
            media_type: MediaType::Playlist,
            limit: 25,
        };
        let recommendations = CatalogQuery::RecommendationFolders;
        for owner in [&user, &other] {
            for (query, value) in [
                (&library, CatalogValue::Playlists(Vec::new())),
                (&search, CatalogValue::Search(Default::default())),
                (
                    &recommendations,
                    CatalogValue::RecommendationFolders(Vec::new()),
                ),
            ] {
                super::super::reads::write_cached_read(&transaction, query, owner, &value, 100)
                    .unwrap();
            }
        }
        let confirmed: Playlist = serde_json::from_value(json!({"provider":"library","item_id":"new","name":"Server name",
            "favorite":null,"access":{"owner":"user","sharing":"private"},
            "metadata":{"description":"Confirmed details"},
            "provider_mappings":[{"provider_domain":"builtin","provider_instance":"builtin","item_id":"new.m3u","in_library":true}]})).unwrap();
        assert!(read_playlist(
            &transaction,
            &confirmed.media.provider,
            &confirmed.media.item_id,
            &user
        )
        .unwrap()
        .is_none());
        commit_confirmed_playlist(&transaction, &confirmed, &user).unwrap();
        for query in [&library, &search, &recommendations] {
            assert!(
                super::super::reads::read_cached_read(&transaction, query, &user)
                    .unwrap()
                    .is_none()
            );
            assert!(
                super::super::reads::read_cached_read(&transaction, query, &other)
                    .unwrap()
                    .is_some()
            );
        }
        let contents = CatalogQuery::PlaylistTracks {
            provider: "library".into(),
            item_id: confirmed.media.item_id.clone(),
            allow_dynamic_tracks: false,
        };
        assert!(
            super::super::reads::read_cached_read(&transaction, &contents, &user)
                .unwrap()
                .is_none()
        );
        transaction.commit().unwrap();
        drop(connection);
        let mut reopened = rusqlite::Connection::open(&path).unwrap();
        reopened.pragma_update(None, "foreign_keys", true).unwrap();
        let snapshot = reopened.transaction().unwrap();
        let stored = read_playlist(
            &snapshot,
            &confirmed.media.provider,
            &confirmed.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(stored.media.name, "Server name");
        assert_eq!(stored.access, confirmed.access);
        assert_eq!(
            stored.media.metadata.unwrap().description.as_deref(),
            Some("Confirmed details")
        );
        assert_eq!(
            stored.media.favorite,
            crate::domain::media::FieldKnowledge::Known(None)
        );
        assert!(
            super::super::reads::read_cached_read(&snapshot, &contents, &user)
                .unwrap()
                .is_none()
        );
        assert!(snapshot
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none());
    }

    #[test]
    fn confirmed_playlist_preserves_other_user_and_contents_and_retires_old_and_new_aliases() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let other = UserId("other".into());
        let original: Playlist = serde_json::from_value(json!({"provider":"library","item_id":"42","name":"Playlist","favorite":true,"metadata":{"description":"Details"},"provider_mappings":[{"provider_domain":"source","provider_instance":"old-a","item_id":"remote"}]})).unwrap();
        let library = CatalogQuery::LibraryPlaylists {
            favorite: Some(true),
            limit: 500,
            offset: 0,
            summary: false,
        };
        for owner in [&user, &other] {
            write_playlist_read(
                &transaction,
                &library,
                owner,
                &CatalogValue::Playlists(vec![original.clone()]),
                100,
            )
            .unwrap();
        }
        let search = CatalogQuery::Search {
            query: "Nothing".into(),
            media_type: MediaType::Playlist,
            limit: 25,
        };
        write_playlist_read(
            &transaction,
            &search,
            &user,
            &CatalogValue::Search(Default::default()),
            100,
        )
        .unwrap();
        let aliases = ["old-a", "source", "new-a"].map(|provider| CatalogQuery::Playlist {
            provider: provider.into(),
            item_id: MediaId("remote".into()),
        });
        for (query, provider) in aliases.iter().zip(["old-a", "source", "new-a"]) {
            let mut alias = original.clone();
            alias.media.provider = ProviderInstanceId(provider.into());
            alias.media.item_id = MediaId("remote".into());
            write_playlist_read(
                &transaction,
                query,
                &user,
                &CatalogValue::Playlist(alias),
                100,
            )
            .unwrap();
        }
        let content_query = CatalogQuery::PlaylistTracks {
            provider: "library".into(),
            item_id: MediaId("42".into()),
            allow_dynamic_tracks: false,
        };
        super::super::reads::write_cached_read(
            &transaction,
            &content_query,
            &user,
            &CatalogValue::Tracks(Vec::new()),
            100,
        )
        .unwrap();
        let recommendations = CatalogQuery::RecommendationFolders;
        for owner in [&user, &other] {
            super::super::reads::write_cached_read(
                &transaction,
                &recommendations,
                owner,
                &CatalogValue::RecommendationFolders(Vec::new()),
                100,
            )
            .unwrap();
        }
        let confirmed: Playlist = serde_json::from_value(json!({"provider":"library","item_id":"42","name":"Playlist","favorite":null,"provider_mappings":[{"provider_domain":"source","provider_instance":"new-a","item_id":"remote"}]})).unwrap();
        commit_confirmed_playlist(&transaction, &confirmed, &user).unwrap();
        for query in aliases.iter().chain([&library, &search]) {
            assert!(read_playlist_read(&transaction, query, &user)
                .unwrap()
                .is_none());
        }
        assert!(read_playlist_read(&transaction, &library, &other)
            .unwrap()
            .is_some());
        assert!(
            super::super::reads::read_cached_read(&transaction, &content_query, &user)
                .unwrap()
                .is_some()
        );
        assert!(
            super::super::reads::read_cached_read(&transaction, &recommendations, &user)
                .unwrap()
                .is_none()
        );
        assert!(
            super::super::reads::read_cached_read(&transaction, &recommendations, &other)
                .unwrap()
                .is_some()
        );
        let updated = read_playlist(
            &transaction,
            &original.media.provider,
            &original.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            updated.media.favorite,
            crate::domain::media::FieldKnowledge::Known(None)
        );
        assert_eq!(
            updated.media.metadata.unwrap().description.as_deref(),
            Some("Details")
        );
        let other_item = read_playlist(
            &transaction,
            &original.media.provider,
            &original.media.item_id,
            &other,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            other_item.media.favorite,
            crate::domain::media::FieldKnowledge::Known(Some(true))
        );
    }

    #[test]
    fn playlist_library_pages_preserve_scope_details_and_known_empty() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let details: Playlist = serde_json::from_value(json!({
            "provider":"library","item_id":"playlist","name":"Details","favorite":true,
            "owner":"Display owner","access":{"owner":"user","sharing":"private"},
            "metadata":{"description":"Keep details","images":[{"type":"thumb","path":"cover.jpg","provider":"builtin"}],"genres":["Detail genre"]},"is_editable":true,
            "external_ids":[["custom","identity"]],"position":7,"date_added":"2026-11-08"
        })).unwrap();
        write_playlist_observation(&transaction, &details, &user).unwrap();
        let query = CatalogQuery::LibraryPlaylists {
            favorite: Some(true),
            limit: 500,
            offset: 0,
            summary: true,
        };
        let response = CatalogValue::Playlists(vec![serde_json::from_value(json!({
            "provider":"library","item_id":"playlist","name":"Summary"
        }))
        .unwrap()]);
        write_playlist_read(&transaction, &query, &user, &response, 100).unwrap();
        let observed = read_playlist_read(&transaction, &query, &user)
            .unwrap()
            .unwrap();
        let CatalogValue::Playlists(items) = observed.value else {
            panic!("playlist page missing")
        };
        assert_eq!(items[0].media.name, "Summary");
        assert_eq!(items[0].owner, "Display owner");
        assert_eq!(
            items[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Keep details")
        );
        assert_eq!(
            items[0].media.metadata.as_ref().unwrap().images[0].path,
            "cover.jpg"
        );
        assert!(items[0].media.metadata.as_ref().unwrap().genres.is_empty());
        assert!(items[0].media.external_ids.is_empty());
        assert!(!items[0].media.knowledge.external_ids);
        assert!(!items[0].media.knowledge.position);
        assert!(!items[0].media.knowledge.date_added);
        let canonical = read_playlist(
            &transaction,
            &details.media.provider,
            &details.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            canonical.media.metadata.as_ref().unwrap().genres,
            details.media.metadata.as_ref().unwrap().genres
        );
        assert_eq!(canonical.media.external_ids, details.media.external_ids);
        assert_eq!(canonical.media.position, Some(7));
        assert_eq!(
            items[0].media.favorite,
            crate::domain::media::FieldKnowledge::Known(Some(true))
        );
        assert!(read_partial_playlist(&transaction, &query, &user)
            .unwrap()
            .is_none());
        assert!(
            read_playlist_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        for other in [
            CatalogQuery::LibraryPlaylists {
                favorite: None,
                limit: 500,
                offset: 0,
                summary: true,
            },
            CatalogQuery::LibraryPlaylists {
                favorite: Some(false),
                limit: 500,
                offset: 0,
                summary: true,
            },
            CatalogQuery::LibraryPlaylists {
                favorite: Some(true),
                limit: 500,
                offset: 500,
                summary: true,
            },
            CatalogQuery::LibraryPlaylists {
                favorite: Some(true),
                limit: 0,
                offset: 0,
                summary: true,
            },
            CatalogQuery::LibraryPlaylists {
                favorite: Some(true),
                limit: 500,
                offset: 0,
                summary: false,
            },
        ] {
            assert!(read_playlist_read(&transaction, &other, &user)
                .unwrap()
                .is_none());
        }
        let empty = CatalogValue::Playlists(Vec::new());
        write_playlist_read(&transaction, &query, &user, &empty, 200).unwrap();
        assert_eq!(
            read_playlist_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            empty
        );
        assert!(read_playlist(
            &transaction,
            &ProviderInstanceId("library".into()),
            &MediaId("playlist".into()),
            &user
        )
        .unwrap()
        .is_some());
        let external = CatalogValue::Playlists(vec![serde_json::from_value(
            json!({"provider":"source","item_id":"external","name":"Not a library row"}),
        )
        .unwrap()]);
        assert!(write_playlist_read(&transaction, &query, &user, &external, 300).is_err());
        assert_eq!(
            read_playlist_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            200
        );
    }

    #[test]
    fn playlist_selections_preserve_duplicates_scope_empty_and_rollback() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let query = CatalogQuery::Search {
            query: "Playlist".into(),
            media_type: MediaType::Playlist,
            limit: 25,
        };
        let response = CatalogValue::Search(crate::domain::media::MusicAssistantSearchResults{
            playlists:[1,4].into_iter().map(|position|serde_json::from_value(json!({"provider":"source-a","item_id":"007","name":"Playlist","position":position})).unwrap()).collect(),..Default::default()
        });
        write_playlist_read(&transaction, &query, &user, &response, 100).unwrap();
        assert_eq!(
            read_playlist_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            response
        );
        assert!(
            read_playlist_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        assert!(read_partial_playlist(&transaction, &query, &user)
            .unwrap()
            .is_none());
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut invalid = response.clone();
        let CatalogValue::Search(ref mut result) = invalid else {
            unreachable!()
        };
        result.playlists[1].media.external_ids = vec![vec!["invalid".into()]];
        result.playlists[1].media.knowledge.external_ids = true;
        assert!(write_playlist_read(&transaction, &query, &user, &invalid, 200).is_err());
        drop(transaction);
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            read_playlist_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .observed_at,
            100
        );
        let empty = CatalogValue::Search(Default::default());
        write_playlist_read(&transaction, &query, &user, &empty, 300).unwrap();
        assert_eq!(
            read_playlist_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            empty
        );
    }
}
