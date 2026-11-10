//! Canonical track observations and ordered relationships use server-provided identities.
//! The caller owns the transaction; album associations never prove a complete album listing.

use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{Map, Value};

use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
use crate::domain::media::{FieldKnowledge, MediaItem, Track};

use super::albums::{read_album, write_album_observation};
use super::artists::{read_artist, write_artist_observation};
use super::database::CacheError;

// Canonical media and relationship schema within the cache writer transaction.
pub(super) fn initialize_track_schema(transaction: &Transaction<'_>) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE tracks (
            provider TEXT NOT NULL,item_id TEXT NOT NULL,name TEXT NOT NULL,
            version TEXT,sort_name TEXT,sort_name_known INTEGER NOT NULL CHECK(sort_name_known IN (0,1)),
            uri TEXT,uri_known INTEGER NOT NULL CHECK(uri_known IN (0,1)),
            is_playable INTEGER CHECK(is_playable IN (0,1)),available INTEGER CHECK(available IN (0,1)),
            available_known INTEGER NOT NULL CHECK(available_known IN (0,1)),
            metadata TEXT CHECK(metadata IS NULL OR json_type(metadata)='object'),metadata_known INTEGER NOT NULL CHECK(metadata_known IN (0,1)),
            position INTEGER CHECK(position>=0),position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),
            date_added TEXT,date_added_known INTEGER NOT NULL CHECK(date_added_known IN (0,1)),
            duration INTEGER CHECK(duration>=0),duration_known INTEGER NOT NULL CHECK(duration_known IN (0,1)),
            artists_known INTEGER NOT NULL CHECK(artists_known IN (0,1)),
            album_provider TEXT,album_item_id TEXT,album_known INTEGER NOT NULL CHECK(album_known IN (0,1)),
            image TEXT CHECK(image IS NULL OR json_type(image)='object'),image_known INTEGER NOT NULL CHECK(image_known IN (0,1)),
            disc_number INTEGER CHECK(disc_number>=0),disc_number_known INTEGER NOT NULL CHECK(disc_number_known IN (0,1)),
            track_number INTEGER CHECK(track_number>=0),track_number_known INTEGER NOT NULL CHECK(track_number_known IN (0,1)),
            external_ids_known INTEGER NOT NULL CHECK(external_ids_known IN (0,1)),
            provider_mappings_known INTEGER NOT NULL CHECK(provider_mappings_known IN (0,1)),
            PRIMARY KEY(provider,item_id),CHECK((album_provider IS NULL)=(album_item_id IS NULL)),
            FOREIGN KEY(album_provider,album_item_id) REFERENCES albums(provider,item_id)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX tracks_sort_name_idx ON tracks(sort_name,provider,item_id);
         CREATE TABLE track_artists (
            track_provider TEXT NOT NULL,track_item_id TEXT NOT NULL,position INTEGER NOT NULL CHECK(position>=0),
            artist_provider TEXT NOT NULL,artist_item_id TEXT NOT NULL,
            PRIMARY KEY(track_provider,track_item_id,position),
            FOREIGN KEY(track_provider,track_item_id) REFERENCES tracks(provider,item_id) ON DELETE CASCADE,
            FOREIGN KEY(artist_provider,artist_item_id) REFERENCES artists(provider,item_id)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX track_artist_lookup_idx ON track_artists(artist_provider,artist_item_id,track_provider,track_item_id);
         CREATE TABLE album_tracks (
            album_provider TEXT NOT NULL,album_item_id TEXT NOT NULL,track_provider TEXT NOT NULL,track_item_id TEXT NOT NULL,
            disc_number INTEGER CHECK(disc_number>=0),disc_number_known INTEGER NOT NULL CHECK(disc_number_known IN (0,1)),
            track_number INTEGER CHECK(track_number>=0),track_number_known INTEGER NOT NULL CHECK(track_number_known IN (0,1)),
            PRIMARY KEY(album_provider,album_item_id,track_provider,track_item_id),
            FOREIGN KEY(album_provider,album_item_id) REFERENCES albums(provider,item_id) ON DELETE CASCADE,
            FOREIGN KEY(track_provider,track_item_id) REFERENCES tracks(provider,item_id) ON DELETE CASCADE
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX album_track_order_idx ON album_tracks(album_provider,album_item_id,disc_number,track_number,track_provider,track_item_id);
         CREATE INDEX track_album_lookup_idx ON album_tracks(track_provider,track_item_id,album_provider,album_item_id);
         CREATE TABLE track_provider_mappings (
            track_provider TEXT NOT NULL,track_item_id TEXT NOT NULL,position INTEGER NOT NULL CHECK(position>=0),
            provider_domain TEXT NOT NULL,provider_instance TEXT NOT NULL,item_id TEXT NOT NULL,
            available INTEGER CHECK(available IN (0,1)),in_library INTEGER CHECK(in_library IN (0,1)),
            in_library_known INTEGER NOT NULL CHECK(in_library_known IN (0,1)),
            is_unique INTEGER CHECK(is_unique IN (0,1)),is_unique_known INTEGER NOT NULL CHECK(is_unique_known IN (0,1)),
            audio_format TEXT CHECK(audio_format IS NULL OR json_type(audio_format)='object'),audio_format_known INTEGER NOT NULL CHECK(audio_format_known IN (0,1)),
            url TEXT,url_known INTEGER NOT NULL CHECK(url_known IN (0,1)),details TEXT,details_known INTEGER NOT NULL CHECK(details_known IN (0,1)),
            PRIMARY KEY(track_provider,track_item_id,position),
            UNIQUE(track_provider,track_item_id,provider_domain,provider_instance,item_id),
            FOREIGN KEY(track_provider,track_item_id) REFERENCES tracks(provider,item_id) ON DELETE CASCADE
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX track_provider_lookup_idx ON track_provider_mappings(provider_instance,item_id,track_provider,track_item_id);
         CREATE TABLE track_external_ids (
            track_provider TEXT NOT NULL,track_item_id TEXT NOT NULL,position INTEGER NOT NULL CHECK(position>=0),
            id_type TEXT NOT NULL,external_id TEXT NOT NULL,PRIMARY KEY(track_provider,track_item_id,position),
            FOREIGN KEY(track_provider,track_item_id) REFERENCES tracks(provider,item_id) ON DELETE CASCADE
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX track_external_lookup_idx ON track_external_ids(id_type,external_id,track_provider,track_item_id);
         CREATE TABLE track_favorites (
            user_id TEXT NOT NULL REFERENCES users(user_id),provider TEXT NOT NULL,item_id TEXT NOT NULL,
            rating INTEGER CHECK(rating IN (0,1)),PRIMARY KEY(user_id,provider,item_id),
            FOREIGN KEY(provider,item_id) REFERENCES tracks(provider,item_id) ON DELETE CASCADE
         ) STRICT, WITHOUT ROWID;",
    )?;
    Ok(())
}

// Observations merge against canonical data, never against a previous result occurrence.
pub(super) fn write_track_observation(
    transaction: &Transaction<'_>,
    incoming: &Track,
    user_id: &UserId,
) -> Result<(), CacheError> {
    if incoming
        .media
        .external_ids
        .iter()
        .any(|identity| identity.len() != 2)
    {
        return Err(CacheError::InvalidMediaObservation(
            "external IDs require type and value",
        ));
    }
    if incoming.knowledge.artists {
        for artist in &incoming.artists {
            write_artist_observation(transaction, artist, user_id, FieldKnowledge::Unknown)?;
        }
    }
    if incoming.knowledge.album {
        if let Some(album) = &incoming.album {
            write_album_observation(transaction, album, user_id)?;
        }
    }
    let track = if let Some(mut previous) = read_track(
        transaction,
        &incoming.media.provider,
        &incoming.media.item_id,
        user_id,
    )? {
        let identity = |track: &Track| {
            track
                .album
                .as_ref()
                .map(|album| (album.media.provider.clone(), album.media.item_id.clone()))
        };
        if incoming.knowledge.album && identity(&previous) != identity(incoming) {
            previous.disc_number = 0;
            previous.knowledge.disc_number = false;
            previous.track_number = 0;
            previous.knowledge.track_number = false;
            if let Some(album) = &incoming.album {
                if let Some((disc_number, track_number)) = read_album_track_numbers(
                    transaction,
                    &album.media.provider,
                    &album.media.item_id,
                    &incoming.media.provider,
                    &incoming.media.item_id,
                )? {
                    if let Some(value) = disc_number {
                        previous.disc_number = value;
                        previous.knowledge.disc_number = true;
                    }
                    if let Some(value) = track_number {
                        previous.track_number = value;
                        previous.knowledge.track_number = true;
                    }
                }
            }
        }
        let mut merged = MediaItem::Track(previous);
        merged.merge_known(MediaItem::Track(incoming.clone()));
        let MediaItem::Track(track) = merged else {
            unreachable!()
        };
        track
    } else {
        incoming.clone()
    };
    let media = &track.media;
    let known = &media.knowledge;
    let album_identity = track
        .album
        .as_ref()
        .filter(|_| track.knowledge.album)
        .map(|album| (&album.media.provider, &album.media.item_id));
    transaction.execute(
        "INSERT INTO tracks(provider,item_id,name,version,sort_name,sort_name_known,uri,uri_known,is_playable,
         available,available_known,metadata,metadata_known,position,position_known,date_added,date_added_known,
         duration,duration_known,artists_known,album_provider,album_item_id,album_known,image,image_known,
         disc_number,disc_number_known,track_number,track_number_known,external_ids_known,provider_mappings_known)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31)
         ON CONFLICT(provider,item_id) DO UPDATE SET name=excluded.name,version=excluded.version,sort_name=excluded.sort_name,
         sort_name_known=excluded.sort_name_known,uri=excluded.uri,uri_known=excluded.uri_known,is_playable=excluded.is_playable,
         available=excluded.available,available_known=excluded.available_known,metadata=excluded.metadata,metadata_known=excluded.metadata_known,
         position=excluded.position,position_known=excluded.position_known,date_added=excluded.date_added,date_added_known=excluded.date_added_known,
         duration=excluded.duration,duration_known=excluded.duration_known,artists_known=excluded.artists_known,
         album_provider=excluded.album_provider,album_item_id=excluded.album_item_id,album_known=excluded.album_known,
         image=excluded.image,image_known=excluded.image_known,disc_number=excluded.disc_number,disc_number_known=excluded.disc_number_known,
         track_number=excluded.track_number,track_number_known=excluded.track_number_known,
         external_ids_known=excluded.external_ids_known,provider_mappings_known=excluded.provider_mappings_known",
        params![media.provider.as_str(),media.item_id.as_str(),media.name,known.version.then_some(&media.version),media.sort_name,known.sort_name,
            media.uri.as_ref().map(|uri|uri.as_str()),known.uri,known.is_playable.then_some(media.is_playable),media.available,known.available,
            media.metadata.as_ref().map(serde_json::to_string).transpose()?,known.metadata,media.position,known.position,media.date_added,known.date_added,
            track.knowledge.duration.then_some(track.duration),track.knowledge.duration,track.knowledge.artists,
            album_identity.map(|(provider,_)|provider.as_str()),album_identity.map(|(_,item_id)|item_id.as_str()),track.knowledge.album,
            track.image.as_ref().map(serde_json::to_string).transpose()?,track.knowledge.image,
            track.knowledge.disc_number.then_some(track.disc_number),track.knowledge.disc_number,
            track.knowledge.track_number.then_some(track.track_number),track.knowledge.track_number,known.external_ids,known.provider_mappings],
    )?;
    if track.knowledge.artists {
        transaction.execute(
            "DELETE FROM track_artists WHERE track_provider=?1 AND track_item_id=?2",
            params![media.provider.as_str(), media.item_id.as_str()],
        )?;
        for (position, artist) in track.artists.iter().enumerate() {
            transaction.execute("INSERT INTO track_artists(track_provider,track_item_id,position,artist_provider,artist_item_id) VALUES(?1,?2,?3,?4,?5)",params![media.provider.as_str(),media.item_id.as_str(),position as i64,artist.media.provider.as_str(),artist.media.item_id.as_str()])?;
        }
    }
    if let Some((provider, item_id)) = album_identity {
        transaction.execute("INSERT INTO album_tracks(album_provider,album_item_id,track_provider,track_item_id,disc_number,disc_number_known,track_number,track_number_known)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(album_provider,album_item_id,track_provider,track_item_id) DO UPDATE SET
            disc_number=CASE WHEN excluded.disc_number_known THEN excluded.disc_number ELSE album_tracks.disc_number END,
            disc_number_known=MAX(album_tracks.disc_number_known,excluded.disc_number_known),
            track_number=CASE WHEN excluded.track_number_known THEN excluded.track_number ELSE album_tracks.track_number END,
            track_number_known=MAX(album_tracks.track_number_known,excluded.track_number_known)",params![provider.as_str(),item_id.as_str(),media.provider.as_str(),media.item_id.as_str(),
                track.knowledge.disc_number.then_some(track.disc_number),track.knowledge.disc_number,track.knowledge.track_number.then_some(track.track_number),track.knowledge.track_number])?;
    }
    write_track_properties(transaction, &track, user_id)?;
    Ok(())
}

// Identity reads hydrate canonical endpoints, not nested media copies.
pub(super) fn read_track(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
) -> Result<Option<Track>, CacheError> {
    let row=transaction.query_row("SELECT provider,item_id,name,version,sort_name,sort_name_known,uri,uri_known,is_playable,available,available_known,
        metadata,metadata_known,position,position_known,date_added,date_added_known,duration,duration_known,artists_known,
        album_provider,album_item_id,album_known,image,image_known,disc_number,disc_number_known,track_number,track_number_known,
        external_ids_known,provider_mappings_known FROM tracks WHERE provider=?1 AND item_id=?2",params![provider.as_str(),item_id.as_str()],|row|{
        let mut object=Map::new();
        for (index,field) in ["provider","item_id","name"].iter().enumerate() {object.insert((*field).into(),Value::String(row.get(index)?));}
        if let Some(value)=row.get::<_,Option<String>>(3)? {object.insert("version".into(),Value::String(value));}
        for (field,index,known) in [("sort_name",4,5),("uri",6,7),("date_added",15,16)] {
            if row.get::<_,bool>(known)? {object.insert(field.into(),row.get::<_,Option<String>>(index)?.map(Value::String).unwrap_or(Value::Null));}
        }
        if let Some(value)=row.get::<_,Option<bool>>(8)? {object.insert("is_playable".into(),Value::Bool(value));}
        if row.get::<_,bool>(10)? {object.insert("available".into(),row.get::<_,Option<bool>>(9)?.map(Value::Bool).unwrap_or(Value::Null));}
        for (field,index,known) in [("position",13,14),("duration",17,18),("disc_number",25,26),("track_number",27,28)] {
            if row.get::<_,bool>(known)? {object.insert(field.into(),row.get::<_,Option<i64>>(index)?.map(|value|Value::Number(value.into())).unwrap_or(Value::Null));}
        }
        Ok((object,row.get::<_,Option<String>>(11)?,row.get::<_,bool>(12)?,row.get::<_,bool>(19)?,
            row.get::<_,Option<String>>(20)?,row.get::<_,Option<String>>(21)?,row.get::<_,bool>(22)?,
            row.get::<_,Option<String>>(23)?,row.get::<_,bool>(24)?,row.get::<_,bool>(29)?,row.get::<_,bool>(30)?))
    }).optional()?;
    let Some((
        mut object,
        metadata,
        is_metadata_known,
        are_artists_known,
        album_provider,
        album_item_id,
        is_album_known,
        image,
        is_image_known,
        are_external_ids_known,
        are_mappings_known,
    )) = row
    else {
        return Ok(None);
    };
    for (field, is_known, encoded) in [
        ("metadata", is_metadata_known, metadata),
        ("image", is_image_known, image),
    ] {
        if is_known {
            object.insert(
                field.into(),
                encoded
                    .map(|value| serde_json::from_str(&value))
                    .transpose()?
                    .unwrap_or(Value::Null),
            );
        }
    }
    if are_artists_known {
        let identities=transaction.prepare("SELECT artist_provider,artist_item_id FROM track_artists WHERE track_provider=?1 AND track_item_id=?2 ORDER BY position")?
            .query_map(params![provider.as_str(),item_id.as_str()],|row|Ok((ProviderInstanceId(row.get(0)?),MediaId(row.get(1)?))))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let artists = identities
            .into_iter()
            .map(|(provider, item_id)| {
                read_artist(transaction, &provider, &item_id, user_id)?.ok_or(
                    CacheError::InvalidMediaObservation("track artist is missing"),
                )
            })
            .collect::<Result<Vec<_>, CacheError>>()?;
        object.insert("artists".into(), serde_json::to_value(artists)?);
    }
    if is_album_known {
        let album = match (album_provider, album_item_id) {
            (Some(provider), Some(item_id)) => serde_json::to_value(
                read_album(
                    transaction,
                    &ProviderInstanceId(provider),
                    &MediaId(item_id),
                    user_id,
                )?
                .ok_or(CacheError::InvalidMediaObservation(
                    "track album is missing",
                ))?,
            )?,
            (None, None) => Value::Null,
            _ => {
                return Err(CacheError::InvalidMediaObservation(
                    "track album identity is incomplete",
                ))
            }
        };
        object.insert("album".into(), album);
    }
    if are_external_ids_known {
        let identities=transaction.prepare("SELECT id_type,external_id FROM track_external_ids WHERE track_provider=?1 AND track_item_id=?2 ORDER BY position")?
            .query_map(params![provider.as_str(),item_id.as_str()],|row|Ok(vec![row.get::<_,String>(0)?,row.get::<_,String>(1)?]))?.collect::<rusqlite::Result<Vec<_>>>()?;
        object.insert("external_ids".into(), serde_json::to_value(identities)?);
    }
    if are_mappings_known {
        object.insert(
            "provider_mappings".into(),
            Value::Array(read_track_mappings(transaction, provider, item_id)?),
        );
    }
    let rating: Option<Option<bool>> = transaction
        .query_row(
            "SELECT rating FROM track_favorites WHERE user_id=?1 AND provider=?2 AND item_id=?3",
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

// Private codecs keep serialization at the SQLite boundary.
fn write_track_properties(
    transaction: &Transaction<'_>,
    track: &Track,
    user_id: &UserId,
) -> Result<(), CacheError> {
    let media = &track.media;
    if media.knowledge.provider_mappings {
        transaction.execute(
            "DELETE FROM track_provider_mappings WHERE track_provider=?1 AND track_item_id=?2",
            params![media.provider.as_str(), media.item_id.as_str()],
        )?;
        for (position, mapping) in media.provider_mappings.iter().enumerate() {
            let known = &mapping.knowledge;
            transaction.execute("INSERT INTO track_provider_mappings(track_provider,track_item_id,position,provider_domain,provider_instance,item_id,
                available,in_library,in_library_known,is_unique,is_unique_known,audio_format,audio_format_known,url,url_known,details,details_known)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",params![media.provider.as_str(),media.item_id.as_str(),position as i64,mapping.provider_domain.as_str(),mapping.provider_instance.as_str(),mapping.item_id.as_str(),
                    known.available.then_some(mapping.available),known.in_library.then_some(mapping.in_library).flatten(),known.in_library,
                    mapping.is_unique,known.is_unique,mapping.audio_format.as_ref().map(serde_json::to_string).transpose()?,known.audio_format,mapping.url,known.url,mapping.details,known.details])?;
        }
    }
    if media.knowledge.external_ids {
        transaction.execute(
            "DELETE FROM track_external_ids WHERE track_provider=?1 AND track_item_id=?2",
            params![media.provider.as_str(), media.item_id.as_str()],
        )?;
        for (position, identity) in media.external_ids.iter().enumerate() {
            transaction.execute("INSERT INTO track_external_ids(track_provider,track_item_id,position,id_type,external_id) VALUES(?1,?2,?3,?4,?5)",params![media.provider.as_str(),media.item_id.as_str(),position as i64,identity[0],identity[1]])?;
        }
    }
    if let FieldKnowledge::Known(rating) = media.favorite {
        transaction.execute(
            "INSERT INTO users(user_id) VALUES(?1) ON CONFLICT DO NOTHING",
            [user_id.as_str()],
        )?;
        transaction.execute("INSERT INTO track_favorites(user_id,provider,item_id,rating) VALUES(?1,?2,?3,?4) ON CONFLICT(user_id,provider,item_id) DO UPDATE SET rating=excluded.rating",params![user_id.as_str(),media.provider.as_str(),media.item_id.as_str(),rating])?;
    }
    Ok(())
}

fn read_track_mappings(
    transaction: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
) -> Result<Vec<Value>, CacheError> {
    let mut statement=transaction.prepare("SELECT item_id,provider_domain,provider_instance,available,in_library,in_library_known,is_unique,is_unique_known,audio_format,audio_format_known,url,url_known,details,details_known FROM track_provider_mappings WHERE track_provider=?1 AND track_item_id=?2 ORDER BY position")?;
    let rows = statement
        .query_map(params![provider.as_str(), item_id.as_str()], |row| {
            let mut object = Map::new();
            for (index, field) in ["item_id", "provider_domain", "provider_instance"]
                .iter()
                .enumerate()
            {
                object.insert((*field).into(), Value::String(row.get(index)?));
            }
            if let Some(value) = row.get::<_, Option<bool>>(3)? {
                object.insert("available".into(), Value::Bool(value));
            }
            for (field, index, known) in [("in_library", 4, 5), ("is_unique", 6, 7)] {
                if row.get::<_, bool>(known)? {
                    object.insert(
                        field.into(),
                        row.get::<_, Option<bool>>(index)?
                            .map(Value::Bool)
                            .unwrap_or(Value::Null),
                    );
                }
            }
            for (field, index, known) in [("url", 10, 11), ("details", 12, 13)] {
                if row.get::<_, bool>(known)? {
                    object.insert(
                        field.into(),
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
        .map(|(mut object, is_known, format)| {
            if is_known {
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

fn read_album_track_numbers(
    transaction: &Transaction<'_>,
    album_provider: &ProviderInstanceId,
    album_item_id: &MediaId,
    track_provider: &ProviderInstanceId,
    track_item_id: &MediaId,
) -> Result<Option<(Option<u16>, Option<u16>)>, CacheError> {
    Ok(transaction.query_row("SELECT CASE WHEN disc_number_known THEN disc_number END,CASE WHEN track_number_known THEN track_number END FROM album_tracks WHERE album_provider=?1 AND album_item_id=?2 AND track_provider=?3 AND track_item_id=?4",params![album_provider.as_str(),album_item_id.as_str(),track_provider.as_str(),track_item_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?))).optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Isolated schema and observations precede integration with active callers.
    fn database() -> rusqlite::Connection {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::artists::initialize_artist_schema(&transaction).unwrap();
        super::super::album_artist_relations::initialize_album_artist_schema(&transaction).unwrap();
        super::super::albums::initialize_album_details_schema(&transaction).unwrap();
        initialize_track_schema(&transaction).unwrap();
        transaction.commit().unwrap();
        connection
    }

    fn observe(connection: &mut rusqlite::Connection, value: Value) -> Track {
        let incoming: Track = serde_json::from_value(value).unwrap();
        let transaction = connection.transaction().unwrap();
        write_track_observation(&transaction, &incoming, &UserId("user".into())).unwrap();
        let result = read_track(
            &transaction,
            &incoming.media.provider,
            &incoming.media.item_id,
            &UserId("user".into()),
        )
        .unwrap()
        .unwrap();
        transaction.commit().unwrap();
        result
    }

    #[test]
    fn track_canonical_round_trip_and_summary_preserve_known_fields() {
        let mut connection = database();
        let value = serde_json::json!({"provider":"source","item_id":"007","name":"Track","duration":120,
            "version":"Edition","sort_name":null,"uri":"source://track/007","is_playable":false,"available":null,
            "favorite":null,"position":2,"date_added":null,"image":null,"disc_number":1,"track_number":4,
            "metadata":{"description":"Keep"},"external_ids":[["isrc","AA1234567890"]],
            "artists":[{"provider":"library","item_id":"a","name":"Artist"}],
            "album":{"provider":"album-source","item_id":"007","name":"Album","year":2024},
            "provider_mappings":[{"provider_domain":"domain","provider_instance":"source","item_id":"007","available":false,
                "in_library":null,"is_unique":true,"audio_format":{"content_type":"flac","sample_rate":96000},"url":null,"details":"Keep"}]});
        let expected: Track = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(observe(&mut connection, value), expected);
        let result = observe(
            &mut connection,
            serde_json::json!({"provider":"source","item_id":"007","name":"Renamed"}),
        );
        let mut expected = expected;
        expected.media.name = "Renamed".into();
        assert_eq!(result, expected);
        let transaction = connection.transaction().unwrap();
        assert!(read_track(
            &transaction,
            &ProviderInstanceId("other".into()),
            &MediaId("007".into()),
            &UserId("user".into())
        )
        .unwrap()
        .is_none());
        assert!(read_track(
            &transaction,
            &result.media.provider,
            &result.media.item_id,
            &UserId("other".into())
        )
        .unwrap()
        .unwrap()
        .media
        .favorite
        .is_unknown());
        assert_eq!(
            transaction
                .query_row("SELECT artist_provider FROM track_artists", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "library"
        );
        assert_eq!(
            transaction
                .query_row("SELECT COUNT(*) FROM artist_library_entries", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn track_canonical_album_numbers_are_scoped_and_associations_are_not_complete_lists() {
        let mut connection = database();
        observe(
            &mut connection,
            serde_json::json!({"provider":"source","item_id":"007","name":"Track","disc_number":2,"track_number":8,
            "album":{"provider":"album-source","item_id":"a","name":"A"}}),
        );
        let other = observe(
            &mut connection,
            serde_json::json!({"provider":"source","item_id":"007","name":"Track","album":{"provider":"album-source","item_id":"b","name":"B"}}),
        );
        assert!(!other.knowledge.disc_number && !other.knowledge.track_number);
        let first = observe(
            &mut connection,
            serde_json::json!({"provider":"source","item_id":"007","name":"Track","album":{"provider":"album-source","item_id":"a","name":"A"}}),
        );
        assert_eq!((first.disc_number, first.track_number), (2, 8));
        assert!(first.knowledge.disc_number && first.knowledge.track_number);
        let cleared = observe(
            &mut connection,
            serde_json::json!({"provider":"source","item_id":"007","name":"Track","album":null,"artists":[],"metadata":null,"provider_mappings":[],"external_ids":[]}),
        );
        assert!(cleared.knowledge.album && cleared.album.is_none());
        assert!(cleared.knowledge.artists && cleared.artists.is_empty());
        assert!(cleared.media.knowledge.metadata && cleared.media.metadata.is_none());
        assert!(
            cleared.media.knowledge.provider_mappings && cleared.media.provider_mappings.is_empty()
        );
        assert!(cleared.media.knowledge.external_ids && cleared.media.external_ids.is_empty());
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM album_tracks", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn track_canonical_provider_identities_and_relationship_indexes_remain_distinct() {
        let mut connection = database();
        for provider in ["library", "source-a", "source-b"] {
            observe(
                &mut connection,
                serde_json::json!({"provider":provider,"item_id":"007","name":provider,
                "artists":[{"provider":"artist-source","item_id":"a","name":"Artist"}]}),
            );
        }
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM tracks", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM track_artists", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
        let plan=connection.prepare("EXPLAIN QUERY PLAN SELECT track_provider,track_item_id FROM track_artists WHERE artist_provider='artist-source' AND artist_item_id='a'").unwrap()
            .query_map([],|row|row.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap().join(" ");
        assert!(
            plan.contains("COVERING INDEX track_artist_lookup_idx"),
            "{plan}"
        );
        let plan=connection.prepare("EXPLAIN QUERY PLAN SELECT track_provider,track_item_id FROM album_tracks WHERE album_provider='album-source' AND album_item_id='a' ORDER BY disc_number,track_number,track_provider,track_item_id").unwrap()
            .query_map([],|row|row.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap().join(" ");
        assert!(
            plan.contains("COVERING INDEX album_track_order_idx"),
            "{plan}"
        );
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }

    #[test]
    fn track_canonical_mapping_partials_preserve_only_matching_provider_identity() {
        let mut connection = database();
        observe(
            &mut connection,
            serde_json::json!({"provider":"source","item_id":"007","name":"Track",
            "metadata":{"description":"Keep","genres":["Jazz"]},"provider_mappings":[{
                "provider_domain":"domain","provider_instance":"source-a","item_id":"id","in_library":true,
                "url":"https://example.test/track","audio_format":{"content_type":"flac","sample_rate":96000}}]}),
        );
        let mut partial = serde_json::json!({"provider":"source","item_id":"007","name":"Track","metadata":{"genres":[]},
            "provider_mappings":[{"provider_domain":"domain","provider_instance":"source-a","item_id":"id","audio_format":{"bit_depth":24}}]});
        let result = observe(&mut connection, partial.clone());
        assert_eq!(result.media.provider_mappings[0].in_library, Some(true));
        assert_eq!(
            result.media.provider_mappings[0].url.as_deref(),
            Some("https://example.test/track")
        );
        assert_eq!(
            result.media.provider_mappings[0]
                .audio_format
                .as_ref()
                .unwrap()
                .sample_rate,
            96000
        );
        assert_eq!(
            result.media.provider_mappings[0]
                .audio_format
                .as_ref()
                .unwrap()
                .bit_depth,
            24
        );
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
        partial["provider_mappings"][0]["provider_instance"] = serde_json::json!("source-b");
        let result = observe(&mut connection, partial);
        assert!(!result.media.provider_mappings[0].knowledge.in_library);
        assert!(!result.media.provider_mappings[0].knowledge.url);
        assert!(
            !result.media.provider_mappings[0]
                .audio_format
                .as_ref()
                .unwrap()
                .knowledge
                .sample_rate
        );
    }

    #[test]
    fn track_canonical_invalid_credit_rolls_back_nested_observations() {
        let mut connection = database();
        let invalid:Track=serde_json::from_value(serde_json::json!({"provider":"source","item_id":"007","name":"Track",
            "artists":[{"provider":"source","item_id":"a","name":"Valid"},{"provider":"source","item_id":"b","name":"Invalid","external_ids":[["discogs"]]}]})).unwrap();
        {
            let transaction = connection.transaction().unwrap();
            assert!(
                write_track_observation(&transaction, &invalid, &UserId("user".into())).is_err()
            );
        }
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM artists", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM tracks", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
