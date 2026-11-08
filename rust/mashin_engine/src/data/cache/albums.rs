//! Canonical albums retain provider identities and field knowledge independently of selections.
//! The caller owns the transaction; nested observations do not imply library membership.

use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{Map, Value};

use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{Album, FieldKnowledge, MediaItem};

use super::album_artist_relations::write_album_relations;
use super::database::CacheError;

// Schema extension of the existing canonical album and relationship tables.
pub(super) fn initialize_album_details_schema(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "ALTER TABLE albums ADD COLUMN version TEXT;
         ALTER TABLE albums ADD COLUMN sort_name TEXT;
         ALTER TABLE albums ADD COLUMN sort_name_known INTEGER NOT NULL DEFAULT 0 CHECK(sort_name_known IN (0,1));
         ALTER TABLE albums ADD COLUMN uri TEXT;
         ALTER TABLE albums ADD COLUMN uri_known INTEGER NOT NULL DEFAULT 0 CHECK(uri_known IN (0,1));
         ALTER TABLE albums ADD COLUMN is_playable INTEGER CHECK(is_playable IN (0,1));
         ALTER TABLE albums ADD COLUMN available INTEGER CHECK(available IN (0,1));
         ALTER TABLE albums ADD COLUMN available_known INTEGER NOT NULL DEFAULT 0 CHECK(available_known IN (0,1));
         ALTER TABLE albums ADD COLUMN metadata TEXT CHECK(metadata IS NULL OR json_type(metadata)='object');
         ALTER TABLE albums ADD COLUMN metadata_known INTEGER NOT NULL DEFAULT 0 CHECK(metadata_known IN (0,1));
         ALTER TABLE albums ADD COLUMN position INTEGER CHECK(position>=0);
         ALTER TABLE albums ADD COLUMN position_known INTEGER NOT NULL DEFAULT 0 CHECK(position_known IN (0,1));
         ALTER TABLE albums ADD COLUMN date_added TEXT;
         ALTER TABLE albums ADD COLUMN date_added_known INTEGER NOT NULL DEFAULT 0 CHECK(date_added_known IN (0,1));
         ALTER TABLE albums ADD COLUMN year INTEGER;
         ALTER TABLE albums ADD COLUMN year_known INTEGER NOT NULL DEFAULT 0 CHECK(year_known IN (0,1));
         ALTER TABLE albums ADD COLUMN album_type TEXT;
         ALTER TABLE albums ADD COLUMN external_ids_known INTEGER NOT NULL DEFAULT 0 CHECK(external_ids_known IN (0,1));
         CREATE INDEX albums_sort_name_idx ON albums(sort_name,provider,item_id);
         CREATE INDEX albums_year_idx ON albums(year,provider,item_id);
         ALTER TABLE album_provider_mappings ADD COLUMN available INTEGER CHECK(available IN (0,1));
         ALTER TABLE album_provider_mappings ADD COLUMN is_unique INTEGER CHECK(is_unique IN (0,1));
         ALTER TABLE album_provider_mappings ADD COLUMN is_unique_known INTEGER NOT NULL DEFAULT 0 CHECK(is_unique_known IN (0,1));
         ALTER TABLE album_provider_mappings ADD COLUMN audio_format TEXT CHECK(audio_format IS NULL OR json_type(audio_format)='object');
         ALTER TABLE album_provider_mappings ADD COLUMN audio_format_known INTEGER NOT NULL DEFAULT 0 CHECK(audio_format_known IN (0,1));
         ALTER TABLE album_provider_mappings ADD COLUMN url TEXT;
         ALTER TABLE album_provider_mappings ADD COLUMN url_known INTEGER NOT NULL DEFAULT 0 CHECK(url_known IN (0,1));
         ALTER TABLE album_provider_mappings ADD COLUMN details TEXT;
         ALTER TABLE album_provider_mappings ADD COLUMN details_known INTEGER NOT NULL DEFAULT 0 CHECK(details_known IN (0,1));
         CREATE INDEX album_provider_lookup_idx ON album_provider_mappings(provider_instance,item_id,album_provider,album_item_id);
         CREATE TABLE album_external_ids (
            album_provider TEXT NOT NULL,album_item_id TEXT NOT NULL,position INTEGER NOT NULL CHECK(position>=0),
            id_type TEXT NOT NULL,external_id TEXT NOT NULL,
            PRIMARY KEY(album_provider,album_item_id,position),
            FOREIGN KEY(album_provider,album_item_id) REFERENCES albums(provider,item_id) ON DELETE CASCADE
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX album_external_lookup_idx ON album_external_ids(id_type,external_id,album_provider,album_item_id);
         CREATE TABLE album_favorites (
            user_id TEXT NOT NULL REFERENCES users(user_id),provider TEXT NOT NULL,item_id TEXT NOT NULL,
            rating INTEGER CHECK(rating IN (0,1)),PRIMARY KEY(user_id,provider,item_id),
            FOREIGN KEY(provider,item_id) REFERENCES albums(provider,item_id) ON DELETE CASCADE
         ) STRICT, WITHOUT ROWID;",
    )?;
    Ok(())
}

// Atomic observations merge only known fields through the canonical domain model.
pub(super) fn write_album_observation(
    transaction: &Transaction<'_>,
    incoming: &Album,
    user_id: &UserId,
) -> Result<(), CacheError> {
    let album = if let Some(previous) = read_album(
        transaction,
        &incoming.media.provider,
        &incoming.media.item_id,
        user_id,
    )? {
        let mut merged = MediaItem::Album(previous);
        merged.merge_known(MediaItem::Album(incoming.clone()));
        let MediaItem::Album(album) = merged else {
            unreachable!()
        };
        album
    } else {
        incoming.clone()
    };
    if album
        .media
        .external_ids
        .iter()
        .any(|identity| identity.len() != 2)
    {
        return Err(CacheError::InvalidMediaObservation(
            "external IDs require type and value",
        ));
    }
    let media = &album.media;
    let knowledge = &media.knowledge;
    transaction.execute(
        "INSERT INTO albums(provider,item_id,name,artists_known,provider_mappings_known) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(provider,item_id) DO UPDATE SET name=excluded.name,artists_known=excluded.artists_known,
         provider_mappings_known=excluded.provider_mappings_known",
        params![media.provider.as_str(),media.item_id.as_str(),media.name,album.knowledge.artists,knowledge.provider_mappings],
    )?;
    write_album_relations(transaction, &album, user_id)?;
    transaction.execute(
        "UPDATE albums SET version=?3,sort_name=?4,sort_name_known=?5,uri=?6,uri_known=?7,
         is_playable=?8,available=?9,available_known=?10,metadata=?11,metadata_known=?12,
         position=?13,position_known=?14,date_added=?15,date_added_known=?16,year=?17,year_known=?18,
         album_type=?19,external_ids_known=?20 WHERE provider=?1 AND item_id=?2",
        params![media.provider.as_str(),media.item_id.as_str(),knowledge.version.then_some(&media.version),
            media.sort_name,knowledge.sort_name,media.uri.as_ref().map(|uri| uri.as_str()),knowledge.uri,
            knowledge.is_playable.then_some(media.is_playable),media.available,knowledge.available,
            media.metadata.as_ref().map(serde_json::to_string).transpose()?,knowledge.metadata,
            media.position,knowledge.position,media.date_added,knowledge.date_added,album.year,album.knowledge.year,
            album.knowledge.album_type.then(|| serde_json::to_value(&album.album_type)).transpose()?.and_then(|value| value.as_str().map(str::to_owned)),
            knowledge.external_ids],
    )?;
    if knowledge.provider_mappings {
        for (position, mapping) in media.provider_mappings.iter().enumerate() {
            let known = &mapping.knowledge;
            transaction.execute(
                "UPDATE album_provider_mappings SET available=?4,is_unique=?5,is_unique_known=?6,
                 audio_format=?7,audio_format_known=?8,url=?9,url_known=?10,details=?11,details_known=?12
                 WHERE album_provider=?1 AND album_item_id=?2 AND position=?3",
                params![media.provider.as_str(),media.item_id.as_str(),position as i64,
                    known.available.then_some(mapping.available),mapping.is_unique,known.is_unique,
                    mapping.audio_format.as_ref().map(serde_json::to_string).transpose()?,known.audio_format,
                    mapping.url,known.url,mapping.details,known.details],
            )?;
        }
    }
    if knowledge.external_ids {
        transaction.execute(
            "DELETE FROM album_external_ids WHERE album_provider=?1 AND album_item_id=?2",
            params![media.provider.as_str(), media.item_id.as_str()],
        )?;
        for (position, identity) in media.external_ids.iter().enumerate() {
            transaction.execute("INSERT INTO album_external_ids(album_provider,album_item_id,position,id_type,external_id) VALUES (?1,?2,?3,?4,?5)",params![media.provider.as_str(),media.item_id.as_str(),position as i64,identity[0],identity[1]])?;
        }
    }
    if let FieldKnowledge::Known(rating) = media.favorite {
        transaction.execute(
            "INSERT INTO users(user_id) VALUES (?1) ON CONFLICT DO NOTHING",
            [user_id.as_str()],
        )?;
        transaction.execute("INSERT INTO album_favorites(user_id,provider,item_id,rating) VALUES (?1,?2,?3,?4) ON CONFLICT(user_id,provider,item_id) DO UPDATE SET rating=excluded.rating",params![user_id.as_str(),media.provider.as_str(),media.item_id.as_str(),rating])?;
    }
    Ok(())
}

// Exact identity reads reconstruct only observed fields and ordered relationships.
pub(super) fn read_album(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
) -> Result<Option<Album>, CacheError> {
    read_album_projection(transaction, provider, item_id, user_id, false)
}

/// Omits unrequested details while retaining observed images and ordered artist references.
pub(super) fn read_album_summary(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
) -> Result<Option<Album>, CacheError> {
    read_album_projection(transaction, provider, item_id, user_id, true)
}

fn read_album_projection(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
    is_summary: bool,
) -> Result<Option<Album>, CacheError> {
    let object = transaction.query_row(
        "SELECT provider,item_id,name,version,sort_name,sort_name_known,uri,uri_known,is_playable,
         available,available_known,
         CASE WHEN ?3 THEN CASE WHEN metadata IS NULL THEN NULL
              WHEN json_type(metadata,'$.images') IS NULL THEN '{}'
              ELSE json_object('images',json_extract(metadata,'$.images')) END ELSE metadata END,
         metadata_known,CASE WHEN ?3 THEN NULL ELSE position END,CASE WHEN ?3 THEN 0 ELSE position_known END,
         CASE WHEN ?3 THEN NULL ELSE date_added END,CASE WHEN ?3 THEN 0 ELSE date_added_known END,
         year,year_known,album_type,CASE WHEN ?3 THEN 0 ELSE external_ids_known END,artists_known,provider_mappings_known FROM albums
         WHERE provider=?1 AND item_id=?2",params![provider.as_str(),item_id.as_str(),is_summary],|row| {
            let mut object = Map::new();
            for (index, field) in ["provider","item_id","name"].iter().enumerate() {
                object.insert((*field).to_owned(),Value::String(row.get(index)?));
            }
            for (field,index) in [("version",3),("album_type",19)] {
                if let Some(value) = row.get::<_,Option<String>>(index)? { object.insert(field.to_owned(),Value::String(value)); }
            }
            for (field,index,known) in [("sort_name",4,5),("uri",6,7),("date_added",15,16)] {
                if row.get::<_,bool>(known)? { object.insert(field.to_owned(),row.get::<_,Option<String>>(index)?.map(Value::String).unwrap_or(Value::Null)); }
            }
            if let Some(value) = row.get::<_,Option<bool>>(8)? { object.insert("is_playable".into(),Value::Bool(value)); }
            if row.get::<_,bool>(10)? { object.insert("available".into(),row.get::<_,Option<bool>>(9)?.map(Value::Bool).unwrap_or(Value::Null)); }
            for (field,index,known) in [("position",13,14),("year",17,18)] {
                if row.get::<_,bool>(known)? { object.insert(field.to_owned(),row.get::<_,Option<i64>>(index)?.map(|value| Value::Number(value.into())).unwrap_or(Value::Null)); }
            }
            Ok((object,row.get::<_,bool>(12)?,row.get::<_,Option<String>>(11)?,row.get::<_,bool>(20)?,row.get::<_,bool>(21)?,row.get::<_,bool>(22)?))
        },
    ).optional()?;
    let Some((
        mut object,
        is_metadata_known,
        metadata,
        are_external_ids_known,
        are_artists_known,
        are_mappings_known,
    )) = object
    else {
        return Ok(None);
    };
    if is_metadata_known {
        object.insert(
            "metadata".into(),
            metadata
                .map(|value| serde_json::from_str(&value))
                .transpose()?
                .unwrap_or(Value::Null),
        );
    }
    if are_external_ids_known {
        let identities = transaction.prepare("SELECT id_type,external_id FROM album_external_ids WHERE album_provider=?1 AND album_item_id=?2 ORDER BY position")?
            .query_map(params![provider.as_str(),item_id.as_str()],|row| Ok(vec![row.get::<_,String>(0)?,row.get::<_,String>(1)?]))?.collect::<rusqlite::Result<Vec<_>>>()?;
        object.insert("external_ids".into(), serde_json::to_value(identities)?);
    }
    if are_artists_known && is_summary {
        let artists = transaction.prepare(
            "SELECT artist.provider,artist.item_id,artist.name,artist.sort_name,artist.sort_name_known
             FROM album_artists relation JOIN artists artist
             ON artist.provider=relation.artist_provider AND artist.item_id=relation.artist_item_id
             WHERE relation.album_provider=?1 AND relation.album_item_id=?2 ORDER BY relation.position",
        )?.query_map(params![provider.as_str(),item_id.as_str()], |row| {
            let mut artist = Map::new();
            for (index,field) in ["provider","item_id","name"].iter().enumerate() {
                artist.insert((*field).into(), Value::String(row.get(index)?));
            }
            if row.get::<_,bool>(4)? {
                artist.insert("sort_name".into(),row.get::<_,Option<String>>(3)?.map(Value::String).unwrap_or(Value::Null));
            }
            Ok(Value::Object(artist))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        object.insert("artists".into(), Value::Array(artists));
    } else if are_artists_known {
        let identities = transaction.prepare("SELECT artist_provider,artist_item_id FROM album_artists WHERE album_provider=?1 AND album_item_id=?2 ORDER BY position")?
            .query_map(params![provider.as_str(),item_id.as_str()],|row| Ok((ProviderInstanceId(row.get(0)?),MediaId(row.get(1)?))))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let artists = identities
            .into_iter()
            .map(|(provider, item_id)| {
                super::artists::read_artist(transaction, &provider, &item_id, user_id)?.ok_or(
                    CacheError::InvalidMediaObservation("album artist is missing"),
                )
            })
            .collect::<Result<Vec<_>, CacheError>>()?;
        object.insert("artists".into(), serde_json::to_value(artists)?);
    }
    if are_mappings_known {
        object.insert(
            "provider_mappings".into(),
            Value::Array(read_album_mappings(transaction, provider, item_id)?),
        );
    }
    let rating: Option<Option<bool>> = transaction
        .query_row(
            "SELECT rating FROM album_favorites WHERE user_id=?1 AND provider=?2 AND item_id=?3",
            params![user_id.as_str(), provider.as_str(), item_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(rating) = rating {
        object.insert(
            "favorite".into(),
            rating.map(Value::Bool).unwrap_or(Value::Null),
        );
    }
    Ok(Some(serde_json::from_value(Value::Object(object))?))
}

// Mapping hydration preserves explicit nulls separately from absent fields.
fn read_album_mappings(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
) -> Result<Vec<Value>, CacheError> {
    let mut statement = transaction.prepare("SELECT item_id,provider_domain,provider_instance,available,in_library,in_library_known,is_unique,is_unique_known,audio_format,audio_format_known,url,url_known,details,details_known FROM album_provider_mappings WHERE album_provider=?1 AND album_item_id=?2 ORDER BY position")?;
    let rows = statement
        .query_map(params![provider.as_str(), item_id.as_str()], |row| {
            let mut object = Map::new();
            for (index, field) in ["item_id", "provider_domain", "provider_instance"]
                .iter()
                .enumerate()
            {
                object.insert((*field).to_owned(), Value::String(row.get(index)?));
            }
            if let Some(value) = row.get::<_, Option<bool>>(3)? {
                object.insert("available".into(), Value::Bool(value));
            }
            for (field, index, known) in [("in_library", 4, 5), ("is_unique", 6, 7)] {
                if row.get::<_, bool>(known)? {
                    object.insert(
                        field.to_owned(),
                        row.get::<_, Option<bool>>(index)?
                            .map(Value::Bool)
                            .unwrap_or(Value::Null),
                    );
                }
            }
            for (field, index, known) in [("url", 10, 11), ("details", 12, 13)] {
                if row.get::<_, bool>(known)? {
                    object.insert(
                        field.to_owned(),
                        row.get::<_, Option<String>>(index)?
                            .map(Value::String)
                            .unwrap_or(Value::Null),
                    );
                }
            }
            Ok((
                object,
                row.get::<_, bool>(9)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(mut object, is_format_known, format)| {
            if is_format_known {
                object.insert(
                    "audio_format".into(),
                    format
                        .map(|value| serde_json::from_str(&value))
                        .transpose()?
                        .unwrap_or(Value::Null),
                );
            }
            Ok(Value::Object(object))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Isolated canonical albums exercise provider separation, knowledge and rollback.
    fn database() -> rusqlite::Connection {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::artists::initialize_artist_schema(&transaction).unwrap();
        super::super::album_artist_relations::initialize_album_artist_schema(&transaction).unwrap();
        initialize_album_details_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        connection
    }

    #[test]
    fn album_summary_skips_details_and_artist_biographies_but_validates_requested_images() {
        let mut connection = database();
        let user = UserId("user".into());
        let album: Album = serde_json::from_value(serde_json::json!({
            "provider":"library","item_id":"007","name":"Album","year":2024,
            "favorite":true,"position":5,"date_added":"2026-01-01","external_ids":[["barcode","00123"]],
            "metadata":{"description":"Album details","images":[{"type":"thumb","provider":"source","path":"cover"}]},
            "artists":[{"provider":"library","item_id":"artist","name":"Artist","sort_name":"artist","metadata":{"description":"Biography"}}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"provider-id","available":false,"audio_format":{"content_type":"flac","sample_rate":96000}}]
        })).unwrap();
        let transaction = connection.transaction().unwrap();
        write_album_observation(&transaction, &album, &user).unwrap();
        let projected = read_album_summary(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(projected.year, Some(2024));
        assert_eq!(projected.media.favorite, FieldKnowledge::Known(Some(true)));
        assert_eq!(
            projected.media.provider_mappings,
            album.media.provider_mappings
        );
        assert!(
            !projected.media.knowledge.position
                && !projected.media.knowledge.date_added
                && !projected.media.knowledge.external_ids
        );
        assert!(projected
            .media
            .metadata
            .as_ref()
            .unwrap()
            .description
            .is_none());
        assert_eq!(
            projected.media.metadata.as_ref().unwrap().images,
            album.media.metadata.as_ref().unwrap().images
        );
        assert_eq!(projected.artists[0].media.name, "Artist");
        assert!(!projected.artists[0].media.knowledge.metadata);
        assert_eq!(
            read_album(
                &transaction,
                &album.media.provider,
                &album.media.item_id,
                &user
            )
            .unwrap()
            .unwrap(),
            album
        );
        transaction.execute("UPDATE albums SET metadata=json_set(metadata,'$.description',7) WHERE item_id='007'", []).unwrap();
        transaction.execute("UPDATE artists SET metadata=json_object('description',7),metadata_known=1 WHERE item_id='artist'", []).unwrap();
        assert!(read_album_summary(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user
        )
        .unwrap()
        .is_some());
        assert!(read_album(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user
        )
        .is_err());
        transaction
            .execute(
                "UPDATE albums SET metadata=json_set(metadata,'$.images',7) WHERE item_id='007'",
                [],
            )
            .unwrap();
        assert!(read_album_summary(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user
        )
        .is_err());
    }

    #[test]
    fn album_canonical_round_trip_preserves_fields_and_user_scope() {
        let mut connection = database();
        let user = UserId("user".into());
        let album: Album = serde_json::from_value(serde_json::json!({
            "provider":"source-a","item_id":"007","name":"Album","sort_name":null,
            "year":2024,"album_type":"album","uri":"source-a://album/007","version":"Edition",
            "favorite":null,"available":true,"is_playable":false,"date_added":null,"position":3,
            "metadata":{"description":"Keep","genres":["Jazz"]},"external_ids":[["barcode","00123"]],
            "artists":[{"provider":"library","item_id":"7","name":"Artist"}],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007",
                "available":false,"in_library":null,"is_unique":true,"url":null,"details":"Keep",
                "audio_format":{"content_type":"flac","sample_rate":96000}}]
        })).unwrap();
        let transaction = connection.transaction().unwrap();
        write_album_observation(&transaction, &album, &user).unwrap();
        let result = read_album(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result, album);
        assert!(read_album(
            &transaction,
            &ProviderInstanceId("source-b".into()),
            &album.media.item_id,
            &user
        )
        .unwrap()
        .is_none());
        assert!(read_album(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &UserId("other".into())
        )
        .unwrap()
        .unwrap()
        .media
        .favorite
        .is_unknown());
        transaction.commit().unwrap();
        let summary: Album = serde_json::from_value(
            serde_json::json!({"provider":"source-a","item_id":"007","name":"Renamed"}),
        )
        .unwrap();
        let transaction = connection.transaction().unwrap();
        write_album_observation(&transaction, &summary, &user).unwrap();
        let result = read_album(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        let mut expected = album.clone();
        expected.media.name = "Renamed".into();
        assert_eq!(result, expected);
        transaction.commit().unwrap();
        let cleared: Album = serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"007","name":"Renamed","year":null,"metadata":null,"artists":[],"provider_mappings":[],"external_ids":[]})).unwrap();
        let transaction = connection.transaction().unwrap();
        write_album_observation(&transaction, &cleared, &user).unwrap();
        let result = read_album(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        assert!(result.knowledge.year && result.year.is_none());
        assert!(result.media.knowledge.metadata && result.media.metadata.is_none());
        assert!(result.knowledge.artists && result.artists.is_empty());
        assert!(
            result.media.knowledge.provider_mappings && result.media.provider_mappings.is_empty()
        );
        assert!(result.media.knowledge.external_ids && result.media.external_ids.is_empty());
    }

    #[test]
    fn album_summary_preserves_metadata_knowledge_and_rejects_invalid_mapping_formats() {
        let mut connection = database();
        let user = UserId("user".into());
        let transaction = connection.transaction().unwrap();
        for (index, metadata) in [
            None,
            Some(serde_json::Value::Null),
            Some(serde_json::json!({})),
            Some(serde_json::json!({"images":null})),
        ]
        .into_iter()
        .enumerate()
        {
            let mut value = serde_json::json!({"provider":"library","item_id":index.to_string(),"name":"Album"});
            if let Some(metadata) = metadata {
                value["metadata"] = metadata;
            }
            let album: Album = serde_json::from_value(value).unwrap();
            write_album_observation(&transaction, &album, &user).unwrap();
            let projected = read_album_summary(
                &transaction,
                &album.media.provider,
                &album.media.item_id,
                &user,
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                projected.media.knowledge.metadata,
                album.media.knowledge.metadata
            );
            assert_eq!(projected.media.metadata, album.media.metadata);
            assert_eq!(projected.media.favorite, FieldKnowledge::Unknown);
        }
        let album: Album = serde_json::from_value(serde_json::json!({"provider":"library","item_id":"mapping","name":"Album","favorite":true,
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007","audio_format":{"sample_rate":96000}}]})).unwrap();
        write_album_observation(&transaction, &album, &user).unwrap();
        let other = read_album_summary(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &UserId("other".into()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(other.media.favorite, FieldKnowledge::Unknown);
        transaction.execute("UPDATE album_provider_mappings SET audio_format=json_object('sample_rate','invalid') WHERE album_item_id='mapping'", []).unwrap();
        assert!(read_album_summary(
            &transaction,
            &album.media.provider,
            &album.media.item_id,
            &user
        )
        .is_err());
    }

    #[test]
    fn album_canonical_partial_mapping_updates_preserve_only_matching_identity() {
        let mut connection = database();
        let user = UserId("user".into());
        let full: Album = serde_json::from_value(serde_json::json!({"provider":"source","item_id":"007","name":"Album",
            "metadata":{"description":"Keep","genres":["Jazz"]},
            "provider_mappings":[{"provider_domain":"domain","provider_instance":"source-a","item_id":"id",
                "in_library":true,"is_unique":true,"url":"https://example.test/album","details":"Keep",
                "audio_format":{"content_type":"flac","sample_rate":96000}}]})).unwrap();
        let transaction = connection.transaction().unwrap();
        write_album_observation(&transaction, &full, &user).unwrap();
        transaction.commit().unwrap();
        let mut partial = serde_json::json!({"provider":"source","item_id":"007","name":"Album","metadata":{"genres":[]},
            "provider_mappings":[{"provider_domain":"domain","provider_instance":"source-a","item_id":"id","audio_format":{"bit_depth":24}}]});
        let transaction = connection.transaction().unwrap();
        write_album_observation(
            &transaction,
            &serde_json::from_value(partial.clone()).unwrap(),
            &user,
        )
        .unwrap();
        let result = read_album(
            &transaction,
            &full.media.provider,
            &full.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        let mapping = &result.media.provider_mappings[0];
        assert_eq!(mapping.in_library, Some(true));
        assert_eq!(mapping.audio_format.as_ref().unwrap().sample_rate, 96000);
        assert_eq!(mapping.audio_format.as_ref().unwrap().bit_depth, 24);
        assert_eq!(mapping.url.as_deref(), Some("https://example.test/album"));
        assert_eq!(
            result
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Keep")
        );
        assert!(result.media.metadata.as_ref().unwrap().genres.is_empty());
        transaction.commit().unwrap();
        partial["provider_mappings"][0]["provider_instance"] = serde_json::json!("source-b");
        let transaction = connection.transaction().unwrap();
        write_album_observation(
            &transaction,
            &serde_json::from_value(partial).unwrap(),
            &user,
        )
        .unwrap();
        let result = read_album(
            &transaction,
            &full.media.provider,
            &full.media.item_id,
            &user,
        )
        .unwrap()
        .unwrap();
        let mapping = &result.media.provider_mappings[0];
        assert!(!mapping.knowledge.in_library);
        assert!(!mapping.knowledge.url);
        assert!(!mapping.audio_format.as_ref().unwrap().knowledge.sample_rate);
    }

    #[test]
    fn album_canonical_failed_relationship_write_rolls_back() {
        let mut connection = database();
        let user = UserId("user".into());
        let invalid: Album = serde_json::from_value(serde_json::json!({"provider":"source","item_id":"007","name":"Invalid","artists":[{"provider":"source","item_id":"bad","name":"Bad","external_ids":[["discogs"]]}]})).unwrap();
        {
            let transaction = connection.transaction().unwrap();
            assert!(write_album_observation(&transaction, &invalid, &user).is_err());
        }
        let transaction = connection.transaction().unwrap();
        assert!(read_album(
            &transaction,
            &invalid.media.provider,
            &invalid.media.item_id,
            &user
        )
        .unwrap()
        .is_none());
        assert_eq!(
            transaction
                .query_row("SELECT COUNT(*) FROM artists", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
