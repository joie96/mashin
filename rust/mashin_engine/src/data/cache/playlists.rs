//! Canonical playlist metadata is independent of user ratings and content selections.
//! The enclosing writer transaction owns observations; readers reconstruct only known fields.

use rusqlite::{params, OptionalExtension, Row, Transaction};
use serde_json::{Map, Value};

use super::database::CacheError;
use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{FieldKnowledge, MediaItem, Playlist};

// Schema and canonical observations.
pub(super) fn initialize_playlist_schema(transaction: &Transaction<'_>) -> Result<(), CacheError> {
    transaction.execute_batch("CREATE TABLE playlists (
        provider TEXT NOT NULL,item_id TEXT NOT NULL,name TEXT NOT NULL,
        version TEXT,sort_name TEXT,uri TEXT,is_playable INTEGER CHECK(is_playable IN (0,1)),
        available INTEGER CHECK(available IN (0,1)),metadata TEXT CHECK(metadata IS NULL OR json_type(metadata)='object'),
        position INTEGER CHECK(position>=0),date_added TEXT,owner TEXT,
        is_editable INTEGER CHECK(is_editable IN (0,1)),is_dynamic INTEGER CHECK(is_dynamic IN (0,1)),
        access TEXT CHECK(access IS NULL OR json_type(access)='object'),
        known_fields TEXT NOT NULL CHECK(json_type(known_fields)='array'),PRIMARY KEY(provider,item_id)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX playlists_sort_name_idx ON playlists(sort_name,provider,item_id);
        CREATE TABLE playlist_provider_mappings (
        provider TEXT NOT NULL,playlist_id TEXT NOT NULL,position INTEGER NOT NULL CHECK(position>=0),
        provider_domain TEXT NOT NULL,provider_instance TEXT NOT NULL,item_id TEXT NOT NULL,
        available INTEGER CHECK(available IN (0,1)),in_library INTEGER CHECK(in_library IN (0,1)),
        is_unique INTEGER CHECK(is_unique IN (0,1)),audio_format TEXT CHECK(audio_format IS NULL OR json_type(audio_format)='object'),
        url TEXT,details TEXT,known_fields TEXT NOT NULL CHECK(json_type(known_fields)='array'),
        PRIMARY KEY(provider,playlist_id,position),FOREIGN KEY(provider,playlist_id) REFERENCES playlists(provider,item_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX playlist_provider_lookup_idx ON playlist_provider_mappings(provider_instance,item_id,provider,playlist_id);
        CREATE INDEX playlist_domain_lookup_idx ON playlist_provider_mappings(provider_domain,item_id,provider,playlist_id);
        CREATE TABLE playlist_external_ids (
        provider TEXT NOT NULL,playlist_id TEXT NOT NULL,position INTEGER NOT NULL CHECK(position>=0),id_type TEXT NOT NULL,external_id TEXT NOT NULL,
        PRIMARY KEY(provider,playlist_id,position),FOREIGN KEY(provider,playlist_id) REFERENCES playlists(provider,item_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX playlist_external_lookup_idx ON playlist_external_ids(id_type,external_id,provider,playlist_id);
        CREATE TABLE playlist_favorites (
        user_id TEXT NOT NULL REFERENCES users(user_id),provider TEXT NOT NULL,item_id TEXT NOT NULL,rating INTEGER CHECK(rating IN (0,1)),
        PRIMARY KEY(user_id,provider,item_id),FOREIGN KEY(provider,item_id) REFERENCES playlists(provider,item_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;")?;
    Ok(())
}

pub(super) fn write_playlist_observation(
    transaction: &Transaction<'_>,
    incoming: &Playlist,
    user_id: &UserId,
) -> Result<(), CacheError> {
    let previous = read_playlist(
        transaction,
        &incoming.media.provider,
        &incoming.media.item_id,
        user_id,
    )?;
    let playlist = if let Some(previous) = previous.clone() {
        let mut merged = MediaItem::Playlist(previous);
        merged.merge_known(MediaItem::Playlist(incoming.clone()));
        let MediaItem::Playlist(playlist) = merged else {
            unreachable!()
        };
        playlist
    } else {
        incoming.clone()
    };
    if playlist.media.metadata.as_ref().is_some_and(|metadata| {
        metadata
            .images
            .iter()
            .any(crate::domain::artwork::is_inline_image)
    }) {
        return Err(CacheError::InvalidMediaObservation(
            "inline playlist images require persistent image references",
        ));
    }
    if playlist
        .media
        .external_ids
        .iter()
        .any(|identity| identity.len() != 2)
    {
        return Err(CacheError::InvalidMediaObservation(
            "external IDs require type and value",
        ));
    }
    if previous.as_ref() == Some(&playlist) {
        return Ok(());
    }
    let media = &playlist.media;
    let known = &media.knowledge;
    let fields: Vec<_> = [
        ("version", known.version),
        ("sort_name", known.sort_name),
        ("uri", known.uri),
        ("is_playable", known.is_playable),
        ("available", known.available),
        ("metadata", known.metadata),
        ("position", known.position),
        ("date_added", known.date_added),
        ("external_ids", known.external_ids),
        ("provider_mappings", known.provider_mappings),
        ("owner", playlist.knowledge.owner),
        ("is_editable", playlist.knowledge.is_editable),
        ("is_dynamic", playlist.knowledge.is_dynamic),
        ("access", playlist.knowledge.access),
    ]
    .into_iter()
    .filter_map(|(field, is_known)| is_known.then_some(field))
    .collect();
    let observation = serde_json::to_string(&playlist)?;
    transaction.execute("INSERT INTO playlists(provider,item_id,name,version,sort_name,uri,is_playable,available,metadata,position,date_added,owner,is_editable,is_dynamic,access,known_fields)
        VALUES(?1,?2,?3,json_extract(?4,'$.version'),json_extract(?4,'$.sort_name'),json_extract(?4,'$.uri'),json_extract(?4,'$.is_playable'),
        json_extract(?4,'$.available'),json_extract(?4,'$.metadata'),json_extract(?4,'$.position'),json_extract(?4,'$.date_added'),json_extract(?4,'$.owner'),
        json_extract(?4,'$.is_editable'),json_extract(?4,'$.is_dynamic'),json_extract(?4,'$.access'),?5)
        ON CONFLICT(provider,item_id) DO UPDATE SET name=excluded.name,version=excluded.version,sort_name=excluded.sort_name,uri=excluded.uri,
        is_playable=excluded.is_playable,available=excluded.available,metadata=excluded.metadata,position=excluded.position,date_added=excluded.date_added,
        owner=excluded.owner,is_editable=excluded.is_editable,is_dynamic=excluded.is_dynamic,access=excluded.access,known_fields=excluded.known_fields",
        params![media.provider.as_str(),media.item_id.as_str(),media.name,observation,serde_json::to_string(&fields)?])?;
    if known.provider_mappings {
        transaction.execute(
            "DELETE FROM playlist_provider_mappings WHERE provider=?1 AND playlist_id=?2",
            params![media.provider.as_str(), media.item_id.as_str()],
        )?;
        for (position, mapping) in media.provider_mappings.iter().enumerate() {
            let knowledge = &mapping.knowledge;
            let fields: Vec<_> = [
                ("available", knowledge.available),
                ("in_library", knowledge.in_library),
                ("is_unique", knowledge.is_unique),
                ("audio_format", knowledge.audio_format),
                ("url", knowledge.url),
                ("details", knowledge.details),
            ]
            .into_iter()
            .filter_map(|(field, is_known)| is_known.then_some(field))
            .collect();
            transaction.execute("INSERT INTO playlist_provider_mappings(provider,playlist_id,position,provider_domain,provider_instance,item_id,available,in_library,is_unique,audio_format,url,details,known_fields)
                VALUES(?1,?2,?3,?4,?5,?6,json_extract(?7,'$.available'),json_extract(?7,'$.in_library'),json_extract(?7,'$.is_unique'),json_extract(?7,'$.audio_format'),json_extract(?7,'$.url'),json_extract(?7,'$.details'),?8)",
                params![media.provider.as_str(),media.item_id.as_str(),position as i64,mapping.provider_domain.as_str(),mapping.provider_instance.as_str(),mapping.item_id.as_str(),serde_json::to_string(mapping)?,serde_json::to_string(&fields)?])?;
        }
    }
    if known.external_ids {
        transaction.execute(
            "DELETE FROM playlist_external_ids WHERE provider=?1 AND playlist_id=?2",
            params![media.provider.as_str(), media.item_id.as_str()],
        )?;
        for (position, identity) in media.external_ids.iter().enumerate() {
            transaction.execute("INSERT INTO playlist_external_ids(provider,playlist_id,position,id_type,external_id) VALUES(?1,?2,?3,?4,?5)",params![media.provider.as_str(),media.item_id.as_str(),position as i64,identity[0],identity[1]])?;
        }
    }
    transaction.execute(
        "INSERT INTO users(user_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [user_id.as_str()],
    )?;
    if let FieldKnowledge::Known(rating) = media.favorite {
        transaction.execute("INSERT INTO playlist_favorites(user_id,provider,item_id,rating) VALUES(?1,?2,?3,?4) ON CONFLICT(user_id,provider,item_id) DO UPDATE SET rating=excluded.rating",
            params![user_id.as_str(),media.provider.as_str(),media.item_id.as_str(),rating])?;
    }
    Ok(())
}

// Snapshot reconstruction preserves absence, explicit null and user-specific ratings.
pub(super) fn read_playlist(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
) -> Result<Option<Playlist>, CacheError> {
    read_playlist_projection(transaction, provider, item_id, user_id, false)
}

/// Projects list metadata without hydrating external identities or detail metadata.
pub(super) fn read_playlist_summary(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
) -> Result<Option<Playlist>, CacheError> {
    read_playlist_projection(transaction, provider, item_id, user_id, true)
}

fn read_playlist_projection(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
    is_summary: bool,
) -> Result<Option<Playlist>, CacheError> {
    let object = transaction
        .query_row(
            "SELECT provider,item_id,name,version,sort_name,uri,is_playable,available,
             CASE WHEN ?3 AND metadata IS NOT NULL THEN
                 (SELECT json_group_object(key,CASE WHEN type IN ('array','object') THEN json(value) ELSE value END)
                  FROM json_each(metadata) WHERE key IN ('images','description'))
                 ELSE metadata END AS metadata,
             CASE WHEN ?3 THEN NULL ELSE position END AS position,
             CASE WHEN ?3 THEN NULL ELSE date_added END AS date_added,
             owner,is_editable,is_dynamic,access,
             CASE WHEN ?3 THEN (SELECT json_group_array(value) FROM json_each(known_fields)
                 WHERE value NOT IN ('external_ids','position','date_added')) ELSE known_fields END AS known_fields
             FROM playlists WHERE provider=?1 AND item_id=?2",
            params![provider.as_str(), item_id.as_str(), is_summary],
            |row| {
                observed_object(
                    row,
                    &["provider", "item_id", "name"],
                    &["metadata", "access"],
                    &["is_playable", "available", "is_editable", "is_dynamic"],
                )
            },
        )
        .optional()?;
    let Some(mut object) = object else {
        return Ok(None);
    };
    let known: Vec<String> = serde_json::from_str(&transaction.query_row(
        "SELECT known_fields FROM playlists WHERE provider=?1 AND item_id=?2",
        params![provider.as_str(), item_id.as_str()],
        |row| row.get::<_, String>(0),
    )?)?;
    if known.iter().any(|field| field == "provider_mappings") {
        let mut statement = transaction.prepare_cached("SELECT * FROM playlist_provider_mappings WHERE provider=?1 AND playlist_id=?2 ORDER BY position")?;
        let mappings = statement
            .query_map(params![provider.as_str(), item_id.as_str()], |row| {
                observed_object(
                    row,
                    &["provider_domain", "provider_instance", "item_id"],
                    &["audio_format"],
                    &["available", "in_library", "is_unique"],
                )
            })?
            .collect::<Result<Vec<_>, _>>()?;
        object.insert("provider_mappings".into(), serde_json::to_value(mappings)?);
    }
    if !is_summary && known.iter().any(|field| field == "external_ids") {
        let mut statement = transaction.prepare_cached("SELECT id_type,external_id FROM playlist_external_ids WHERE provider=?1 AND playlist_id=?2 ORDER BY position")?;
        let identities = statement
            .query_map(params![provider.as_str(), item_id.as_str()], |row| {
                Ok(vec![row.get::<_, String>(0)?, row.get::<_, String>(1)?])
            })?
            .collect::<Result<Vec<_>, _>>()?;
        object.insert("external_ids".into(), serde_json::to_value(identities)?);
    }
    let rating = transaction
        .query_row(
            "SELECT rating FROM playlist_favorites WHERE user_id=?1 AND provider=?2 AND item_id=?3",
            params![user_id.as_str(), provider.as_str(), item_id.as_str()],
            |row| row.get::<_, Option<bool>>(0),
        )
        .optional()?;
    if let Some(rating) = rating {
        object.insert("favorite".into(), serde_json::to_value(rating)?);
    }
    serde_json::from_value(Value::Object(object))
        .map(Some)
        .map_err(Into::into)
}

fn observed_object(
    row: &Row<'_>,
    required: &[&str],
    json_fields: &[&str],
    bool_fields: &[&str],
) -> rusqlite::Result<Map<String, Value>> {
    let mut fields: Vec<String> = serde_json::from_str(&row.get::<_, String>("known_fields")?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    fields.extend(required.iter().map(|field| (*field).to_owned()));
    let mut object = Map::new();
    for field in fields {
        if matches!(field.as_str(), "provider_mappings" | "external_ids") {
            continue;
        }
        let value = match row.get::<_, rusqlite::types::Value>(field.as_str())? {
            rusqlite::types::Value::Null => Value::Null,
            rusqlite::types::Value::Integer(value) if bool_fields.contains(&field.as_str()) => {
                Value::Bool(value != 0)
            }
            rusqlite::types::Value::Integer(value) => Value::from(value),
            rusqlite::types::Value::Text(value) if json_fields.contains(&field.as_str()) => {
                serde_json::from_str(&value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?
            }
            rusqlite::types::Value::Text(value) => Value::String(value),
            _ => {
                return Err(rusqlite::Error::InvalidColumnType(
                    0,
                    field,
                    rusqlite::types::Type::Blob,
                ))
            }
        };
        object.insert(field, value);
    }
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn playlist_details_merge_partial_observations_and_partition_ratings() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::database::initialize_media_schema(&transaction).unwrap();
        let user = UserId("user".into());
        let other = UserId("other".into());
        let initial: Playlist = serde_json::from_value(json!({"provider":"source-a","item_id":"007","name":"Original","owner":"Owner",
            "is_editable":true,"is_dynamic":false,"favorite":true,"metadata":{"description":"Retained"},
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote","in_library":false}],
            "external_ids":[["custom","identity"]]})).unwrap();
        write_playlist_observation(&transaction, &initial, &user).unwrap();
        assert_eq!(
            read_playlist(
                &transaction,
                &initial.media.provider,
                &initial.media.item_id,
                &user
            )
            .unwrap(),
            Some(initial.clone())
        );
        let partial: Playlist =
            serde_json::from_value(json!({"provider":"source-a","item_id":"007","name":"Updated"}))
                .unwrap();
        write_playlist_observation(&transaction, &partial, &other).unwrap();
        let observed = read_playlist(
            &transaction,
            &initial.media.provider,
            &initial.media.item_id,
            &other,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observed.owner, "Owner");
        assert!(observed.is_editable);
        assert_eq!(observed.media.favorite, FieldKnowledge::Unknown);
        assert_eq!(
            observed.media.metadata.unwrap().description.as_deref(),
            Some("Retained")
        );
        assert_eq!(
            read_playlist(
                &transaction,
                &initial.media.provider,
                &initial.media.item_id,
                &user
            )
            .unwrap()
            .unwrap()
            .media
            .favorite,
            FieldKnowledge::Known(Some(true))
        );
        let empty: Playlist = serde_json::from_value(json!({"provider":"source-a","item_id":"007","name":"Empty","owner":"","is_editable":false,
            "metadata":null,"access":null,"provider_mappings":[],"external_ids":[],"favorite":null})).unwrap();
        write_playlist_observation(&transaction, &empty, &user).unwrap();
        let observed = read_playlist(
            &transaction,
            &initial.media.provider,
            &initial.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observed.owner, "");
        assert!(!observed.is_editable);
        assert!(observed.media.metadata.is_none());
        assert!(observed.media.provider_mappings.is_empty());
        assert_eq!(observed.media.favorite, FieldKnowledge::Known(None));
    }
}
