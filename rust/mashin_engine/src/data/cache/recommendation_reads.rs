//! Observed recommendation sections reference canonical media, not a second feed inventory.
//! User/query coverage and occurrence context are committed in the enclosing writer transaction.

use super::database::CacheError;
use super::reads::CatalogReadObservation;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{MediaItem, MediaItemImage};
use crate::domain::recommendations::{RecommendationFolder, RecommendationKnowledge};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
struct OccurrenceContext {
    position: Option<u32>,
    position_known: bool,
    album_known: bool,
    disc_number: u16,
    disc_number_known: bool,
    track_number: u16,
    track_number_known: bool,
    image: Option<MediaItemImage>,
    image_known: bool,
}

// Schema and atomic observations.
pub(super) fn initialize_recommendation_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch("CREATE TABLE recommendation_read_states (
        user_id TEXT NOT NULL REFERENCES users(user_id),query TEXT NOT NULL,observed_at INTEGER NOT NULL CHECK(observed_at>=0),
        PRIMARY KEY(user_id,query)) STRICT, WITHOUT ROWID;
        CREATE TABLE recommendation_sections (
        user_id TEXT NOT NULL,query TEXT NOT NULL,section INTEGER NOT NULL CHECK(section>=0),provider TEXT,item_id TEXT,title TEXT,subtitle TEXT,
        title_known INTEGER NOT NULL CHECK(title_known IN (0,1)),subtitle_known INTEGER NOT NULL CHECK(subtitle_known IN (0,1)),
        items_known INTEGER NOT NULL CHECK(items_known IN (0,1)),playlist_tracks_known INTEGER NOT NULL CHECK(playlist_tracks_known IN (0,1)),
        PRIMARY KEY(user_id,query,section),CHECK((provider IS NULL)=(item_id IS NULL)),
        FOREIGN KEY(user_id,query) REFERENCES recommendation_read_states(user_id,query) ON DELETE CASCADE) STRICT, WITHOUT ROWID;
        CREATE TABLE recommendation_items (
        user_id TEXT NOT NULL,query TEXT NOT NULL,section INTEGER NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),
        media_type TEXT NOT NULL,provider TEXT NOT NULL,item_id TEXT NOT NULL,artist_id TEXT,album_id TEXT,track_id TEXT,playlist_id TEXT,
        context_album_provider TEXT,context_album_id TEXT,context TEXT NOT NULL CHECK(json_type(context)='object'),
        PRIMARY KEY(user_id,query,section,occurrence),
          CHECK((media_type='artist' AND artist_id IS NOT NULL AND artist_id=item_id AND album_id IS NULL AND track_id IS NULL AND playlist_id IS NULL)
              OR (media_type='album' AND album_id IS NOT NULL AND album_id=item_id AND artist_id IS NULL AND track_id IS NULL AND playlist_id IS NULL)
              OR (media_type='track' AND track_id IS NOT NULL AND track_id=item_id AND artist_id IS NULL AND album_id IS NULL AND playlist_id IS NULL)
              OR (media_type='playlist' AND playlist_id IS NOT NULL AND playlist_id=item_id AND artist_id IS NULL AND album_id IS NULL AND track_id IS NULL)),
          CHECK((context_album_provider IS NULL)=(context_album_id IS NULL)),
        FOREIGN KEY(user_id,query,section) REFERENCES recommendation_sections(user_id,query,section) ON DELETE CASCADE,
        FOREIGN KEY(provider,artist_id) REFERENCES artists(provider,item_id),FOREIGN KEY(provider,album_id) REFERENCES albums(provider,item_id),
        FOREIGN KEY(provider,track_id) REFERENCES tracks(provider,item_id),FOREIGN KEY(provider,playlist_id) REFERENCES playlists(provider,item_id),
        FOREIGN KEY(context_album_provider,context_album_id,provider,track_id) REFERENCES album_tracks(album_provider,album_item_id,track_provider,track_item_id)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX recommendation_items_identity_idx ON recommendation_items(user_id,media_type,provider,item_id,query);")?;
    Ok(())
}

pub(super) fn supports_recommendation_read(query: &CatalogQuery) -> bool {
    matches!(
        query,
        CatalogQuery::RecommendationFolders | CatalogQuery::RecommendationItems { .. }
    )
}

pub(super) fn write_recommendation_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user: &UserId,
    value: &CatalogValue,
    observed_at: i64,
) -> Result<(), CacheError> {
    if !matches!(
        (query, value),
        (
            CatalogQuery::RecommendationFolders,
            CatalogValue::RecommendationFolders(_)
        ) | (
            CatalogQuery::RecommendationItems { .. },
            CatalogValue::RecommendationItems(_)
        )
    ) {
        return Err(CacheError::InvalidMediaObservation(
            "recommendation query and response do not match",
        ));
    }
    let key = serde_json::to_string(query)?;
    transaction.execute(
        "INSERT INTO users(user_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [user.as_str()],
    )?;
    transaction.execute("INSERT INTO recommendation_read_states(user_id,query,observed_at) VALUES(?1,?2,?3) ON CONFLICT(user_id,query) DO UPDATE SET observed_at=excluded.observed_at WHERE observed_at IS NOT excluded.observed_at",params![user.as_str(),key,observed_at])?;
    let section_count = match value {
        CatalogValue::RecommendationFolders(folders) => folders.len(),
        _ => 1,
    };
    transaction.execute(
        "DELETE FROM recommendation_sections WHERE user_id=?1 AND query=?2 AND section>=?3",
        params![user.as_str(), key, section_count as i64],
    )?;
    let mut statement = transaction.prepare_cached(
        "INSERT INTO recommendation_sections(user_id,query,section,provider,item_id,title,subtitle,title_known,subtitle_known,items_known,playlist_tracks_known) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(user_id,query,section) DO UPDATE SET provider=excluded.provider,item_id=excluded.item_id,title=excluded.title,subtitle=excluded.subtitle,title_known=excluded.title_known,subtitle_known=excluded.subtitle_known,items_known=excluded.items_known,playlist_tracks_known=excluded.playlist_tracks_known
         WHERE (provider,item_id,title,subtitle,title_known,subtitle_known,items_known,playlist_tracks_known) IS NOT (excluded.provider,excluded.item_id,excluded.title,excluded.subtitle,excluded.title_known,excluded.subtitle_known,excluded.items_known,excluded.playlist_tracks_known)",
    )?;
    match value {
        CatalogValue::RecommendationFolders(folders) => {
            for (section, folder) in folders.iter().enumerate() {
                if !folder.playlist_tracks.is_empty() {
                    return Err(CacheError::InvalidMediaObservation(
                        "derived playlist tracks belong to their own selection",
                    ));
                }
                statement.execute(
                    params![user.as_str(),key,section as i64,folder.provider.as_str(),folder.item_id.as_str(),folder.title,folder.subtitle,folder.knowledge.title,folder.knowledge.subtitle,folder.knowledge.items,folder.knowledge.playlist_tracks])?;
                write_items(transaction, user, &key, section as i64, &folder.items)?;
            }
        }
        CatalogValue::RecommendationItems(items) => {
            statement.execute(params![user.as_str(),key,0,Option::<String>::None,Option::<String>::None,Option::<String>::None,Option::<String>::None,false,false,true,false])?;
            write_items(transaction, user, &key, 0, items)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

// A confirmed library mutation retires dynamic selections, including known empty answers.
pub(super) fn invalidate_user_recommendation_reads(
    transaction: &Transaction<'_>,
    user: &UserId,
) -> Result<(), CacheError> {
    transaction.execute(
        "DELETE FROM recommendation_read_states WHERE user_id=?1",
        [user.as_str()],
    )?;
    Ok(())
}

// Other users lose only selections referencing the removed playlist or its source context.
pub(super) fn invalidate_removed_playlist_recommendation_reads(
    transaction: &Transaction<'_>,
    user: &UserId,
    provider: &str,
    item_id: &str,
) -> Result<(), CacheError> {
    transaction.execute("DELETE FROM recommendation_read_states AS state WHERE state.user_id=?1 OR
        (json_extract(state.query,'$.RecommendationItems.provider')=?2 AND json_extract(state.query,'$.RecommendationItems.item_id')=?3) OR
        EXISTS(SELECT 1 FROM recommendation_sections section WHERE section.user_id=state.user_id AND section.query=state.query AND section.provider=?2 AND section.item_id=?3) OR
        EXISTS(SELECT 1 FROM recommendation_items item WHERE item.user_id=state.user_id AND item.query=state.query AND item.provider=?2 AND item.playlist_id=?3)", params![user.as_str(),provider,item_id])?;
    Ok(())
}

// Exact cached answers never infer unseen folders or membership.
pub(super) fn read_recommendation_read(
    transaction: &Transaction<'_>,
    query: &CatalogQuery,
    user: &UserId,
) -> Result<Option<CatalogReadObservation>, CacheError> {
    let key = serde_json::to_string(query)?;
    let time = transaction
        .query_row(
            "SELECT observed_at FROM recommendation_read_states WHERE user_id=?1 AND query=?2",
            params![user.as_str(), key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(observed_at) = time else {
        return Ok(None);
    };
    let value = match query {
        CatalogQuery::RecommendationItems { .. } => {
            CatalogValue::RecommendationItems(read_items(transaction, user, &key, 0)?)
        }
        CatalogQuery::RecommendationFolders => {
            let mut statement = transaction.prepare("SELECT section,provider,item_id,title,subtitle,title_known,subtitle_known,items_known,playlist_tracks_known FROM recommendation_sections WHERE user_id=?1 AND query=?2 ORDER BY section")?;
            let rows = statement
                .query_map(params![user.as_str(), key], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        RecommendationFolder {
                            provider: ProviderInstanceId(row.get(1)?),
                            item_id: MediaId(row.get(2)?),
                            title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                            subtitle: row.get(4)?,
                            knowledge: RecommendationKnowledge {
                                title: row.get(5)?,
                                subtitle: row.get(6)?,
                                items: row.get(7)?,
                                playlist_tracks: row.get(8)?,
                            },
                            items: Vec::new(),
                            playlist_tracks: Vec::new(),
                            is_loading: false,
                            has_load_error: false,
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut folders = Vec::new();
            for (section, mut folder) in rows {
                folder.items = read_items(transaction, user, &key, section)?;
                folders.push(folder);
            }
            CatalogValue::RecommendationFolders(folders)
        }
        _ => {
            return Err(CacheError::InvalidMediaObservation(
                "query is not a recommendation read",
            ))
        }
    };
    Ok(Some(CatalogReadObservation { value, observed_at }))
}

// Canonical writers and occurrence-specific reconstruction share one transaction.
fn write_items(
    transaction: &Transaction<'_>,
    user: &UserId,
    query: &str,
    section: i64,
    items: &[MediaItem],
) -> Result<(), CacheError> {
    transaction.execute(
        "DELETE FROM recommendation_items WHERE user_id=?1 AND query=?2 AND section=?3 AND occurrence>=?4",
        params![user.as_str(), query, section, items.len() as i64],
    )?;
    let mut statement = transaction.prepare_cached(
        "INSERT INTO recommendation_items(user_id,query,section,occurrence,media_type,provider,item_id,artist_id,album_id,track_id,playlist_id,context_album_provider,context_album_id,context)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
         ON CONFLICT(user_id,query,section,occurrence) DO UPDATE SET media_type=excluded.media_type,provider=excluded.provider,item_id=excluded.item_id,artist_id=excluded.artist_id,album_id=excluded.album_id,track_id=excluded.track_id,playlist_id=excluded.playlist_id,context_album_provider=excluded.context_album_provider,context_album_id=excluded.context_album_id,context=excluded.context
         WHERE (media_type,provider,item_id,artist_id,album_id,track_id,playlist_id,context_album_provider,context_album_id,context) IS NOT (excluded.media_type,excluded.provider,excluded.item_id,excluded.artist_id,excluded.album_id,excluded.track_id,excluded.playlist_id,excluded.context_album_provider,excluded.context_album_id,excluded.context)",
    )?;
    for (occurrence, item) in items.iter().enumerate() {
        let media = item.fields();
        let identity = media.item_id.as_str();
        let mut context = OccurrenceContext {
            position: media.position,
            position_known: media.knowledge.position,
            ..Default::default()
        };
        let mut album_context = None;
        let (kind, artist, album, track, playlist) = match item {
            MediaItem::Artist(item) => {
                super::artists::write_artist_observation(
                    transaction,
                    item,
                    user,
                    crate::domain::media::FieldKnowledge::Unknown,
                )?;
                ("artist", Some(identity), None, None, None)
            }
            MediaItem::Album(item) => {
                super::albums::write_album_observation(transaction, item, user)?;
                ("album", None, Some(identity), None, None)
            }
            MediaItem::Playlist(item) => {
                super::playlists::write_playlist_observation(transaction, item, user)?;
                ("playlist", None, None, None, Some(identity))
            }
            MediaItem::Track(item) => {
                super::tracks::write_track_observation(transaction, item, user)?;
                context.album_known = item.knowledge.album;
                context.disc_number = item.disc_number;
                context.disc_number_known = item.knowledge.disc_number;
                context.track_number = item.track_number;
                context.track_number_known = item.knowledge.track_number;
                context.image = item.image.clone();
                context.image_known = item.knowledge.image;
                album_context = item
                    .album
                    .as_ref()
                    .map(|album| (&album.media.provider, &album.media.item_id));
                ("track", None, None, Some(identity), None)
            }
            _ => {
                return Err(CacheError::InvalidMediaObservation(
                    "recommendation contains a media type outside the cache scope",
                ))
            }
        };
        statement.execute(params![user.as_str(),query,section,occurrence as i64,kind,media.provider.as_str(),identity,artist,album,track,playlist,
                album_context.map(|(provider,_)|provider.as_str()),album_context.map(|(_,item_id)|item_id.as_str()),serde_json::to_string(&context)?])?;
    }
    Ok(())
}

fn read_items(
    transaction: &Transaction<'_>,
    user: &UserId,
    query: &str,
    section: i64,
) -> Result<Vec<MediaItem>, CacheError> {
    let mut statement=transaction.prepare("SELECT media_type,provider,item_id,context_album_provider,context_album_id,context FROM recommendation_items WHERE user_id=?1 AND query=?2 AND section=?3 ORDER BY occurrence")?;
    let references = statement
        .query_map(params![user.as_str(), query, section], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ProviderInstanceId(row.get(1)?),
                MediaId(row.get(2)?),
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut items = Vec::new();
    for (kind, provider, item_id, album_provider, album_id, context) in references {
        let mut item = match kind.as_str() {
            "artist" => super::artists::read_artist(transaction, &provider, &item_id, user)?
                .map(MediaItem::Artist),
            "album" => super::albums::read_album(transaction, &provider, &item_id, user)?
                .map(MediaItem::Album),
            "track" => super::tracks::read_track(transaction, &provider, &item_id, user)?
                .map(MediaItem::Track),
            "playlist" => super::playlists::read_playlist(transaction, &provider, &item_id, user)?
                .map(MediaItem::Playlist),
            _ => None,
        }
        .ok_or(CacheError::InvalidMediaObservation(
            "canonical recommendation item missing",
        ))?;
        let context: OccurrenceContext = serde_json::from_str(&context)?;
        let media = item.fields_mut();
        media.position = context.position;
        media.knowledge.position = context.position_known;
        if let MediaItem::Track(ref mut track) = item {
            track.album = match (album_provider, album_id) {
                (Some(provider), Some(item_id)) => super::albums::read_album(
                    transaction,
                    &ProviderInstanceId(provider),
                    &MediaId(item_id),
                    user,
                )?,
                _ => None,
            };
            track.knowledge.album = context.album_known;
            track.disc_number = context.disc_number;
            track.knowledge.disc_number = context.disc_number_known;
            track.track_number = context.track_number;
            track.knowledge.track_number = context.track_number_known;
            track.image = context.image;
            track.knowledge.image = context.image_known;
        }
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recommendation_references_preserve_mixed_supported_media_context_scope_and_rollback() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let query = CatalogQuery::RecommendationItems {
            provider: "recommendations".into(),
            item_id: MediaId("row".into()),
        };
        let value=CatalogValue::RecommendationItems(serde_json::from_value(json!([
            {"media_type":"artist","provider":"source-a","item_id":"artist","name":"Artist"},
            {"media_type":"album","provider":"source-a","item_id":"album","name":"Album"},
            {"media_type":"playlist","provider":"source-a","item_id":"playlist","name":"Playlist","owner":"Owner"},
            {"media_type":"track","provider":"source-a","item_id":"track","name":"Track","position":1,"album":{"provider":"source-a","item_id":"album","name":"Album"},"disc_number":1,"track_number":4},
            {"media_type":"track","provider":"source-a","item_id":"track","name":"Track","position":9,"album":{"provider":"source-b","item_id":"album-b","name":"Other album"},"disc_number":2,"track_number":8}
        ])).unwrap());
        write_recommendation_read(&transaction, &query, &user, &value, 100).unwrap();
        assert_eq!(
            read_recommendation_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            value
        );
        assert!(
            read_recommendation_read(&transaction, &query, &UserId("other".into()))
                .unwrap()
                .is_none()
        );
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let invalid=CatalogValue::RecommendationItems(serde_json::from_value(json!([
            {"media_type":"track","provider":"source-a","item_id":"intruder","name":"Must roll back"},
            {"media_type":"radio","provider":"source-a","item_id":"radio","name":"Outside scope"}
        ])).unwrap());
        assert!(write_recommendation_read(&transaction, &query, &user, &invalid, 200).is_err());
        drop(transaction);
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            read_recommendation_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            value
        );
        assert!(super::super::tracks::read_track(
            &transaction,
            &ProviderInstanceId("source-a".into()),
            &MediaId("intruder".into()),
            &user
        )
        .unwrap()
        .is_none());
        let empty = CatalogValue::RecommendationItems(Vec::new());
        write_recommendation_read(&transaction, &query, &user, &empty, 300).unwrap();
        assert_eq!(
            read_recommendation_read(&transaction, &query, &user)
                .unwrap()
                .unwrap()
                .value,
            empty
        );
        let folders = CatalogValue::RecommendationFolders(
            serde_json::from_value(json!([
                {"provider":"recommendations","item_id":"row","name":"First","items":[]},
                {"provider":"source","item_id":"row","name":"Second"}
            ]))
            .unwrap(),
        );
        write_recommendation_read(
            &transaction,
            &CatalogQuery::RecommendationFolders,
            &user,
            &folders,
            400,
        )
        .unwrap();
        assert_eq!(
            read_recommendation_read(&transaction, &CatalogQuery::RecommendationFolders, &user)
                .unwrap()
                .unwrap()
                .value,
            folders
        );
    }
}
