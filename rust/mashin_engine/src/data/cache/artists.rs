//! Artist observations retain server identities and explicit field knowledge in SQLite.
//! Callers own the transaction; this module does no network I/O and keeps no RAM catalog.

use rusqlite::{params, types::Type, OptionalExtension, Row, Transaction};
use serde::de::DeserializeOwned;

use crate::domain::identifiers::{MediaId, MediaUri, ProviderId, ProviderInstanceId, UserId};
use crate::domain::media::{
    Artist, ArtistKnowledge, FieldKnowledge, MediaFieldsKnowledge, MediaItem, MediaItemFields,
    ProviderMapping, ProviderMappingKnowledge,
};

use super::database::CacheError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ArtistReadProjection {
    #[default]
    Summary,
    Detail,
}

pub(super) enum ArtistLookupKey {
    Library(MediaId),
    ProviderInstance {
        provider_instance: ProviderInstanceId,
        item_id: MediaId,
    },
    ProviderDomain {
        provider_domain: ProviderId,
        item_id: MediaId,
    },
    ExternalId {
        id_type: ArtistExternalIdType,
        value: String,
    },
    ExternalValue(String),
}

#[derive(Clone)]
pub(super) enum ArtistExternalIdType {
    MusicBrainzArtist,
    Discogs,
    AudioDb,
    Barcode,
    Isrc,
    Other(String),
}

pub(super) enum ArtistLookupResult {
    Unknown,
    Observed(Artist),
    AmbiguousLibraryMapping,
}

pub(super) struct ArtistObservation {
    pub(super) artist: Artist,
    pub(super) library_membership: FieldKnowledge<bool>,
}

// Schema initialization within the media writer's transaction.
pub(super) fn initialize_artist_schema(transaction: &Transaction<'_>) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE TABLE artists (
            provider TEXT NOT NULL,
            item_id TEXT NOT NULL,
            name TEXT NOT NULL,
            version TEXT,
            sort_name TEXT,
            sort_name_known INTEGER NOT NULL CHECK (sort_name_known IN (0, 1)),
            uri TEXT,
            uri_known INTEGER NOT NULL CHECK (uri_known IN (0, 1)),
            is_playable INTEGER CHECK (is_playable IN (0, 1)),
            available INTEGER CHECK (available IN (0, 1)),
            available_known INTEGER NOT NULL CHECK (available_known IN (0, 1)),
            metadata TEXT CHECK (metadata IS NULL OR json_type(metadata) = 'object'),
            metadata_known INTEGER NOT NULL CHECK (metadata_known IN (0, 1)),
            position INTEGER CHECK (position >= 0),
            position_known INTEGER NOT NULL CHECK (position_known IN (0, 1)),
            date_added TEXT,
            date_added_known INTEGER NOT NULL CHECK (date_added_known IN (0, 1)),
            artist_type TEXT,
            artist_type_known INTEGER NOT NULL CHECK (artist_type_known IN (0, 1)),
            provider_mappings_known INTEGER NOT NULL CHECK (provider_mappings_known IN (0, 1)),
            external_ids_known INTEGER NOT NULL CHECK (external_ids_known IN (0, 1)),
            PRIMARY KEY (provider, item_id)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX artists_sort_name_idx ON artists (sort_name, provider, item_id);
        CREATE TABLE artist_provider_mappings (
            artist_provider TEXT NOT NULL,
            artist_item_id TEXT NOT NULL,
            position INTEGER NOT NULL CHECK (position >= 0),
            provider_domain TEXT NOT NULL,
            provider_instance TEXT NOT NULL,
            item_id TEXT NOT NULL,
            available INTEGER CHECK (available IN (0, 1)),
            in_library INTEGER CHECK (in_library IN (0, 1)),
            in_library_known INTEGER NOT NULL CHECK (in_library_known IN (0, 1)),
            is_unique INTEGER CHECK (is_unique IN (0, 1)),
            is_unique_known INTEGER NOT NULL CHECK (is_unique_known IN (0, 1)),
            audio_format TEXT CHECK (audio_format IS NULL OR json_type(audio_format) = 'object'),
            audio_format_known INTEGER NOT NULL CHECK (audio_format_known IN (0, 1)),
            url TEXT,
            url_known INTEGER NOT NULL CHECK (url_known IN (0, 1)),
            details TEXT,
            details_known INTEGER NOT NULL CHECK (details_known IN (0, 1)),
            PRIMARY KEY (artist_provider, artist_item_id, position),
            UNIQUE (artist_provider, artist_item_id, provider_domain, provider_instance, item_id),
            FOREIGN KEY (artist_provider, artist_item_id) REFERENCES artists (provider, item_id)
                ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX artist_provider_lookup_idx ON artist_provider_mappings
            (provider_instance, item_id, artist_provider, artist_item_id);
        CREATE TABLE artist_external_ids (
            artist_provider TEXT NOT NULL,
            artist_item_id TEXT NOT NULL,
            position INTEGER NOT NULL CHECK (position >= 0),
            id_type TEXT NOT NULL,
            external_id TEXT NOT NULL,
            PRIMARY KEY (artist_provider, artist_item_id, position),
            FOREIGN KEY (artist_provider, artist_item_id) REFERENCES artists (provider, item_id)
                ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX artist_external_lookup_idx ON artist_external_ids (id_type, external_id);
        CREATE TABLE users (user_id TEXT PRIMARY KEY NOT NULL) STRICT, WITHOUT ROWID;
        CREATE TABLE artist_favorites (
            user_id TEXT NOT NULL REFERENCES users (user_id),
            provider TEXT NOT NULL,
            item_id TEXT NOT NULL,
            rating INTEGER CHECK (rating IN (0, 1)),
            PRIMARY KEY (user_id, provider, item_id),
            FOREIGN KEY (provider, item_id) REFERENCES artists (provider, item_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX artist_favorite_lookup_idx ON artist_favorites (user_id, rating, provider, item_id);
        CREATE TABLE artist_library_entries (
            provider TEXT NOT NULL,
            item_id TEXT NOT NULL,
            is_in_library INTEGER NOT NULL CHECK (is_in_library IN (0, 1)),
            PRIMARY KEY (provider, item_id),
            FOREIGN KEY (provider, item_id) REFERENCES artists (provider, item_id) ON DELETE CASCADE
        ) STRICT, WITHOUT ROWID;",
    )?;
    initialize_artist_lookup_indexes(transaction)?;
    Ok(())
}

/// Adds derived lookup indexes without changing observed data or its schema version.
pub(super) fn initialize_artist_lookup_indexes(
    transaction: &Transaction<'_>,
) -> Result<(), CacheError> {
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS artist_domain_lookup_idx ON artist_provider_mappings
            (provider_domain, item_id, artist_provider, artist_item_id);
         CREATE INDEX IF NOT EXISTS artist_external_nocase_lookup_idx ON artist_external_ids
                (id_type, external_id COLLATE NOCASE, artist_provider, artist_item_id);
            CREATE INDEX IF NOT EXISTS artist_external_value_lookup_idx ON artist_external_ids
                (external_id COLLATE NOCASE, artist_provider, artist_item_id, id_type);",
    )?;
    Ok(())
}

// Atomic observation writes; the caller propagates errors and commits or rolls back.
/// Writes one observation batch in the caller's transaction; any error must abort that transaction.
pub(super) fn write_artist_observations(
    transaction: &Transaction<'_>,
    observations: &[ArtistObservation],
    user_id: &UserId,
) -> Result<(), CacheError> {
    for observation in observations {
        write_artist_observation(
            transaction,
            &observation.artist,
            user_id,
            observation.library_membership.clone(),
        )?;
    }
    Ok(())
}

/// Persists one observation; unknown fields and membership cannot erase previous knowledge.
pub(super) fn write_artist_observation(
    transaction: &Transaction<'_>,
    incoming: &Artist,
    user_id: &UserId,
    membership: FieldKnowledge<bool>,
) -> Result<(), CacheError> {
    let previous = read_artist(
        transaction,
        &incoming.media.provider,
        &incoming.media.item_id,
        user_id,
    )?;
    let artist = if let Some(previous) = previous.clone() {
        let mut merged = MediaItem::Artist(previous);
        merged.merge_known(MediaItem::Artist(incoming.clone()));
        let MediaItem::Artist(artist) = merged else {
            unreachable!()
        };
        artist
    } else {
        incoming.clone()
    };
    if artist
        .media
        .external_ids
        .iter()
        .any(|identity| identity.len() != 2)
    {
        return Err(CacheError::InvalidMediaObservation(
            "external IDs require type and value",
        ));
    }
    if previous.as_ref() == Some(&artist) {
        if let FieldKnowledge::Known(is_in_library) = membership {
            transaction.execute(
                "INSERT INTO artist_library_entries(provider,item_id,is_in_library) VALUES(?1,?2,?3)
                 ON CONFLICT(provider,item_id) DO UPDATE SET is_in_library=excluded.is_in_library
                 WHERE is_in_library IS NOT excluded.is_in_library",
                params![artist.media.provider.as_str(), artist.media.item_id.as_str(), is_in_library],
            )?;
        }
        return Ok(());
    }
    let media = &artist.media;
    let knowledge = &media.knowledge;
    let metadata = media
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    transaction.execute(
        "INSERT INTO artists (provider, item_id, name, version, sort_name, sort_name_known,
         uri, uri_known, is_playable, available, available_known, metadata, metadata_known,
         position, position_known, date_added, date_added_known, artist_type, artist_type_known,
         provider_mappings_known, external_ids_known)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20, ?21)
         ON CONFLICT (provider, item_id) DO UPDATE SET name=excluded.name, version=excluded.version,
         sort_name=excluded.sort_name, sort_name_known=excluded.sort_name_known,
         uri=excluded.uri, uri_known=excluded.uri_known, is_playable=excluded.is_playable,
         available=excluded.available, available_known=excluded.available_known,
         metadata=excluded.metadata, metadata_known=excluded.metadata_known, position=excluded.position,
         position_known=excluded.position_known, date_added=excluded.date_added,
         date_added_known=excluded.date_added_known, artist_type=excluded.artist_type,
         artist_type_known=excluded.artist_type_known, provider_mappings_known=excluded.provider_mappings_known,
         external_ids_known=excluded.external_ids_known",
        params![media.provider.as_str(), media.item_id.as_str(), media.name,
            knowledge.version.then_some(&media.version), media.sort_name, knowledge.sort_name,
            media.uri.as_ref().map(MediaUri::as_str), knowledge.uri,
            knowledge.is_playable.then_some(media.is_playable), media.available, knowledge.available,
            metadata, knowledge.metadata, media.position, knowledge.position,
            media.date_added, knowledge.date_added, artist.artist_type, artist.knowledge.artist_type,
            knowledge.provider_mappings, knowledge.external_ids],
    )?;
    write_artist_relations(transaction, &artist)?;
    transaction.execute(
        "INSERT INTO users (user_id) VALUES (?1) ON CONFLICT DO NOTHING",
        [user_id.as_str()],
    )?;
    if let FieldKnowledge::Known(rating) = incoming.media.favorite {
        transaction.execute(
            "INSERT INTO artist_favorites (user_id, provider, item_id, rating) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (user_id, provider, item_id) DO UPDATE SET rating=excluded.rating",
            params![user_id.as_str(), media.provider.as_str(), media.item_id.as_str(), rating],
        )?;
    }
    if let FieldKnowledge::Known(is_in_library) = membership {
        transaction.execute(
            "INSERT INTO artist_library_entries (provider, item_id, is_in_library) VALUES (?1, ?2, ?3)
             ON CONFLICT (provider, item_id) DO UPDATE SET is_in_library=excluded.is_in_library",
            params![media.provider.as_str(), media.item_id.as_str(), is_in_library],
        )?;
    }
    Ok(())
}

// Targeted snapshot reads; no list completeness or remote equivalence is implied.
pub(super) fn read_artist(
    snapshot: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
) -> Result<Option<Artist>, CacheError> {
    let artist = snapshot
        .query_row(
            "SELECT a.provider, a.item_id, a.name, a.version, a.sort_name, a.sort_name_known,
         a.uri, a.uri_known, a.is_playable, a.available, a.available_known, a.metadata,
         a.metadata_known, a.position, a.position_known, a.date_added, a.date_added_known,
         a.artist_type, a.artist_type_known, a.provider_mappings_known, a.external_ids_known,
         f.user_id, f.rating FROM artists a LEFT JOIN artist_favorites f
         ON f.provider=a.provider AND f.item_id=a.item_id AND f.user_id=?3
         WHERE a.provider=?1 AND a.item_id=?2",
            params![provider.as_str(), item_id.as_str(), user_id.as_str()],
            read_artist_row,
        )
        .optional()?;
    let Some(mut artist) = artist else {
        return Ok(None);
    };
    artist.media.provider_mappings = read_artist_provider_mappings(snapshot, provider, item_id)?;
    let mut external_ids = snapshot.prepare_cached(
        "SELECT id_type, external_id FROM artist_external_ids
         WHERE artist_provider=?1 AND artist_item_id=?2 ORDER BY position",
    )?;
    artist.media.external_ids = external_ids
        .query_map(params![provider.as_str(), item_id.as_str()], |row| {
            Ok(vec![row.get::<_, String>(0)?, row.get::<_, String>(1)?])
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(artist))
}

/// Keeps observed image lists intact; omitted details remain unknown rather than empty.
pub(super) fn read_artist_summary(
    snapshot: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
    user_id: &UserId,
) -> Result<Option<Artist>, CacheError> {
    let artist = snapshot
        .query_row(
            "SELECT artist.provider, artist.item_id, artist.name, artist.version,
             artist.sort_name, artist.sort_name_known, artist.uri, artist.uri_known,
             artist.is_playable, artist.available, artist.available_known,
             CASE WHEN artist.metadata IS NULL THEN NULL
                  WHEN json_type(artist.metadata, '$.images') IS NULL THEN '{}'
                  ELSE json_object('images', json_extract(artist.metadata, '$.images')) END,
             artist.metadata_known, NULL, 0, NULL, 0,
             artist.artist_type, artist.artist_type_known, artist.provider_mappings_known, 0,
             favorite.user_id, favorite.rating
             FROM artists artist LEFT JOIN artist_favorites favorite
             ON favorite.provider=artist.provider AND favorite.item_id=artist.item_id
             AND favorite.user_id=?3 WHERE artist.provider=?1 AND artist.item_id=?2",
            params![provider.as_str(), item_id.as_str(), user_id.as_str()],
            read_artist_row,
        )
        .optional()?;
    let Some(mut artist) = artist else {
        return Ok(None);
    };
    artist.media.provider_mappings = read_artist_provider_mappings(snapshot, provider, item_id)?;
    Ok(Some(artist))
}

/// Prefers an observed library mapping; a miss proves neither absence nor detail completeness.
pub(super) fn read_artist_lookup(
    snapshot: &Transaction<'_>,
    key: &ArtistLookupKey,
    user_id: &UserId,
    projection: ArtistReadProjection,
) -> Result<ArtistLookupResult, CacheError> {
    let library_provider = ProviderInstanceId("library".to_owned());
    let (provider, item_id) = match key {
        ArtistLookupKey::Library(item_id) => (library_provider, item_id.clone()),
        ArtistLookupKey::ProviderDomain {
            provider_domain,
            item_id,
        } => {
            let mut mappings = snapshot.prepare_cached(
                "SELECT DISTINCT artist_item_id FROM artist_provider_mappings
                 WHERE provider_domain=?1 AND item_id=?2 AND artist_provider='library' LIMIT 2",
            )?;
            let identities = mappings
                .query_map(params![provider_domain.as_str(), item_id.as_str()], |row| {
                    Ok(MediaId(row.get(0)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            match identities.as_slice() {
                [] => return Ok(ArtistLookupResult::Unknown),
                [library_item_id] => (library_provider, library_item_id.clone()),
                _ => return Ok(ArtistLookupResult::AmbiguousLibraryMapping),
            }
        }
        ArtistLookupKey::ExternalId { id_type, value } => {
            let (id_type, values) = artist_external_lookup_values(id_type, value);
            let values = serde_json::to_string(&values)?;
            let mut external_ids = snapshot.prepare_cached(
                "SELECT DISTINCT ids.artist_item_id FROM json_each(?2) AS lookup
                 CROSS JOIN artist_external_ids AS ids
                 WHERE ids.id_type=?1 AND ids.external_id COLLATE NOCASE=lookup.value
                 AND ids.artist_provider='library' LIMIT 2",
            )?;
            let identities = external_ids
                .query_map(params![id_type, values], |row| Ok(MediaId(row.get(0)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            match identities.as_slice() {
                [] => return Ok(ArtistLookupResult::Unknown),
                [library_item_id] => (library_provider, library_item_id.clone()),
                _ => return Ok(ArtistLookupResult::AmbiguousLibraryMapping),
            }
        }
        ArtistLookupKey::ProviderInstance {
            provider_instance,
            item_id,
        } => {
            let mut mappings = snapshot.prepare_cached(
                "SELECT DISTINCT artist_item_id FROM artist_provider_mappings
                 WHERE provider_instance=?1 AND item_id=?2 AND artist_provider='library' LIMIT 2",
            )?;
            let identities = mappings
                .query_map(
                    params![provider_instance.as_str(), item_id.as_str()],
                    |row| Ok(MediaId(row.get(0)?)),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            match identities.as_slice() {
                [] => (provider_instance.clone(), item_id.clone()),
                [library_item_id] => (library_provider, library_item_id.clone()),
                _ => return Ok(ArtistLookupResult::AmbiguousLibraryMapping),
            }
        }
        ArtistLookupKey::ExternalValue(value) => {
            let values = serde_json::to_string(&external_lookup_values(None, value))?;
            let identities=snapshot.prepare_cached(
                "SELECT DISTINCT ids.artist_item_id FROM json_each(?1) AS lookup
                 CROSS JOIN artist_external_ids AS ids
                 WHERE ids.external_id COLLATE NOCASE=lookup.value AND ids.artist_provider='library' LIMIT 2"
            )?.query_map([values],|row|Ok(MediaId(row.get(0)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            match identities.as_slice() {
                [] => return Ok(ArtistLookupResult::Unknown),
                [library_item_id] => (library_provider, library_item_id.clone()),
                _ => return Ok(ArtistLookupResult::AmbiguousLibraryMapping),
            }
        }
    };
    let artist = match projection {
        ArtistReadProjection::Summary => {
            read_artist_summary(snapshot, &provider, &item_id, user_id)?
        }
        ArtistReadProjection::Detail => read_artist(snapshot, &provider, &item_id, user_id)?,
    };
    Ok(match artist {
        Some(artist) => ArtistLookupResult::Observed(artist),
        None => ArtistLookupResult::Unknown,
    })
}

pub(super) fn read_artist_membership(
    snapshot: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
) -> Result<FieldKnowledge<bool>, CacheError> {
    let membership: Option<bool> = snapshot
        .query_row(
            "SELECT is_in_library FROM artist_library_entries WHERE provider=?1 AND item_id=?2",
            params![provider.as_str(), item_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(membership.map(FieldKnowledge::Known).unwrap_or_default())
}

// Relational mapping and model reconstruction on the owning SQLite worker.
fn artist_external_lookup_values<'identity>(
    id_type: &'identity ArtistExternalIdType,
    value: &str,
) -> (&'identity str, Vec<String>) {
    let id_type = match id_type {
        ArtistExternalIdType::MusicBrainzArtist => "musicbrainz_artistid",
        ArtistExternalIdType::Discogs => "discogs",
        ArtistExternalIdType::AudioDb => "tadb",
        ArtistExternalIdType::Barcode => "barcode",
        ArtistExternalIdType::Isrc => "isrc",
        ArtistExternalIdType::Other(id_type) => id_type.as_str(),
    };
    (id_type, external_lookup_values(Some(id_type), value))
}

// Server-compatible aliases never compare media names or change stored identities.
fn external_lookup_values(id_type: Option<&str>, value: &str) -> Vec<String> {
    let value = value.trim();
    let mut values = vec![value.to_owned()];
    if id_type.is_none() || id_type.is_some_and(|id_type| id_type.starts_with("musicbrainz_")) {
        let normalized = uuid::Uuid::parse_str(value.trim_matches(&['{', '}'][..]))
            .map(|id| id.hyphenated().to_string())
            .unwrap_or_else(|_| value.to_owned());
        values.push(format!("{{{normalized}}}"));
        values.push(normalized);
    }
    if id_type.is_none() || id_type == Some("isrc") {
        let compact = value.replace(['-', ' '], "").to_uppercase();
        let bytes = compact.as_bytes();
        if bytes.len() == 12
            && bytes[..2].iter().all(u8::is_ascii_uppercase)
            && bytes[2..5].iter().all(u8::is_ascii_alphanumeric)
            && bytes[5..].iter().all(u8::is_ascii_digit)
        {
            values.push(format!(
                "{}-{}-{}-{}",
                &compact[..2],
                &compact[2..5],
                &compact[5..7],
                &compact[7..]
            ));
            values.push(compact);
        }
    }
    if id_type.is_none() || id_type == Some("barcode") {
        let compact = value.replace(['-', ' '], "");
        if matches!(compact.len(), 8 | 12 | 13 | 14 | 15)
            && compact.bytes().all(|digit| digit.is_ascii_digit())
        {
            let payload = compact.trim_start_matches('0');
            if !payload.is_empty() && payload.len() <= 14 {
                let mut normalized = format!("{payload:0>14}");
                if normalized.as_bytes()[13] != gtin_check_digit(&normalized[..13]) {
                    // MA also recovers GTIN bodies delivered without their final check digit.
                    if compact.len() == 13 {
                        normalized = format!("{compact}{}", char::from(gtin_check_digit(&compact)));
                    } else {
                        normalized = value.to_owned();
                    }
                }
                if normalized.len() == 14
                    && normalized.bytes().all(|digit| digit.is_ascii_digit())
                    && normalized.as_bytes()[13] == gtin_check_digit(&normalized[..13])
                {
                    let payload = normalized.trim_start_matches('0');
                    for length in [8, 12, 13, 14, 15] {
                        if payload.len() <= length {
                            values.push(format!("{payload:0>length$}"));
                        }
                    }
                    values.push(normalized);
                }
            }
        }
    }
    values.sort();
    values.dedup();
    values
}

fn gtin_check_digit(body: &str) -> u8 {
    let checksum = body
        .bytes()
        .rev()
        .enumerate()
        .map(|(index, digit)| u32::from(digit - b'0') * if index % 2 == 0 { 3 } else { 1 })
        .sum::<u32>();
    b'0' + ((10 - checksum % 10) % 10) as u8
}

fn write_artist_relations(
    transaction: &Transaction<'_>,
    artist: &Artist,
) -> Result<(), CacheError> {
    let media = &artist.media;
    transaction.execute(
        "DELETE FROM artist_provider_mappings WHERE artist_provider=?1 AND artist_item_id=?2",
        params![media.provider.as_str(), media.item_id.as_str()],
    )?;
    for (position, mapping) in media.provider_mappings.iter().enumerate() {
        let knowledge = &mapping.knowledge;
        let audio_format = mapping
            .audio_format
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        transaction.execute(
            "INSERT INTO artist_provider_mappings (artist_provider, artist_item_id, position,
             provider_domain, provider_instance, item_id, available, in_library, in_library_known,
             is_unique, is_unique_known, audio_format, audio_format_known, url, url_known, details,
             details_known) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                                    ?14, ?15, ?16, ?17)",
            params![
                media.provider.as_str(),
                media.item_id.as_str(),
                position as i64,
                mapping.provider_domain.as_str(),
                mapping.provider_instance.as_str(),
                mapping.item_id.as_str(),
                knowledge.available.then_some(mapping.available),
                mapping.in_library,
                knowledge.in_library,
                mapping.is_unique,
                knowledge.is_unique,
                audio_format,
                knowledge.audio_format,
                mapping.url,
                knowledge.url,
                mapping.details,
                knowledge.details
            ],
        )?;
    }
    transaction.execute(
        "DELETE FROM artist_external_ids WHERE artist_provider=?1 AND artist_item_id=?2",
        params![media.provider.as_str(), media.item_id.as_str()],
    )?;
    for (position, identity) in media.external_ids.iter().enumerate() {
        transaction.execute(
            "INSERT INTO artist_external_ids (artist_provider, artist_item_id, position, id_type,
             external_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                media.provider.as_str(),
                media.item_id.as_str(),
                position as i64,
                identity[0],
                identity[1]
            ],
        )?;
    }
    Ok(())
}

fn read_artist_row(row: &Row<'_>) -> rusqlite::Result<Artist> {
    let version: Option<String> = row.get(3)?;
    let is_playable: Option<bool> = row.get(8)?;
    let favorite_user: Option<String> = row.get(21)?;
    Ok(Artist {
        media: MediaItemFields {
            provider: ProviderInstanceId(row.get(0)?),
            item_id: MediaId(row.get(1)?),
            name: row.get(2)?,
            favorite: if favorite_user.is_some() {
                FieldKnowledge::Known(row.get(22)?)
            } else {
                FieldKnowledge::Unknown
            },
            version: version.clone().unwrap_or_default(),
            sort_name: row.get(4)?,
            uri: row.get::<_, Option<String>>(6)?.map(MediaUri),
            is_playable: is_playable.unwrap_or(true),
            available: row.get(9)?,
            metadata: read_json_column(row, 11)?,
            position: row.get(13)?,
            date_added: row.get(15)?,
            provider_mappings: Vec::new(),
            external_ids: Vec::new(),
            knowledge: MediaFieldsKnowledge {
                version: version.is_some(),
                sort_name: row.get(5)?,
                uri: row.get(7)?,
                is_playable: is_playable.is_some(),
                available: row.get(10)?,
                metadata: row.get(12)?,
                position: row.get(14)?,
                date_added: row.get(16)?,
                provider_mappings: row.get(19)?,
                external_ids: row.get(20)?,
            },
        },
        artist_type: row.get(17)?,
        knowledge: ArtistKnowledge {
            artist_type: row.get(18)?,
        },
    })
}

fn read_artist_provider_mappings(
    snapshot: &Transaction<'_>,
    provider: &ProviderInstanceId,
    item_id: &MediaId,
) -> rusqlite::Result<Vec<ProviderMapping>> {
    let mut mappings = snapshot.prepare_cached(
        "SELECT item_id, provider_domain, provider_instance, available, in_library, in_library_known,
         is_unique, is_unique_known, audio_format, audio_format_known, url, url_known, details,
         details_known FROM artist_provider_mappings
         WHERE artist_provider=?1 AND artist_item_id=?2 ORDER BY position",
    )?;
    let result = mappings
        .query_map(
            params![provider.as_str(), item_id.as_str()],
            read_mapping_row,
        )?
        .collect();
    result
}

fn read_mapping_row(row: &Row<'_>) -> rusqlite::Result<ProviderMapping> {
    let available: Option<bool> = row.get(3)?;
    Ok(ProviderMapping {
        item_id: MediaId(row.get(0)?),
        provider_domain: ProviderId(row.get(1)?),
        provider_instance: ProviderInstanceId(row.get(2)?),
        available: available.unwrap_or(true),
        in_library: row.get(4)?,
        is_unique: row.get(6)?,
        audio_format: read_json_column(row, 8)?,
        url: row.get(10)?,
        details: row.get(12)?,
        knowledge: ProviderMappingKnowledge {
            available: available.is_some(),
            in_library: row.get(5)?,
            is_unique: row.get(7)?,
            audio_format: row.get(9)?,
            url: row.get(11)?,
            details: row.get(13)?,
        },
    })
}

fn read_json_column<Value: DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<Value>> {
    row.get::<_, Option<String>>(index)?
        .map(|json| {
            serde_json::from_str(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::super::artist_candidates::{
        read_artist_candidates, ArtistCandidateDirection, ArtistCandidateKnowledge,
        ArtistCandidateQuery,
    };
    use super::*;
    use rusqlite::Connection;

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

    fn insert_artist(
        connection: &Connection,
        provider: &str,
        item_id: &str,
    ) -> rusqlite::Result<usize> {
        connection.execute(
            "INSERT INTO artists (provider, item_id, name, sort_name_known, uri_known,
             available_known, metadata_known, position_known, date_added_known,
             artist_type_known, provider_mappings_known, external_ids_known)
             VALUES (?1, ?2, 'synthetic artist', 0, 0, 0, 0, 0, 0, 0, 0, 0)",
            [provider, item_id],
        )
    }

    fn artist(json: &str) -> Artist {
        serde_json::from_str(json).unwrap()
    }

    fn observe(
        connection: &mut Connection,
        artist: &Artist,
        user: &str,
        membership: FieldKnowledge<bool>,
    ) -> Result<(), CacheError> {
        let transaction = connection.transaction()?;
        write_artist_observation(&transaction, artist, &UserId(user.to_owned()), membership)?;
        transaction.commit()?;
        Ok(())
    }

    fn load(
        connection: &mut Connection,
        provider: &str,
        item_id: &str,
        user: &str,
    ) -> Option<Artist> {
        let snapshot = connection.transaction().unwrap();
        read_artist(
            &snapshot,
            &ProviderInstanceId(provider.to_owned()),
            &MediaId(item_id.to_owned()),
            &UserId(user.to_owned()),
        )
        .unwrap()
    }

    // Domain and typed external lookup normalization.
    #[test]
    fn artist_identity_untyped_aliases_and_opaque_types_use_scoped_covering_lookup() {
        let mut connection = database();
        let user = UserId("user".into());
        let transaction = connection.transaction().unwrap();
        for (item_id, id_type, value) in [
            (
                "mb",
                "musicbrainz_albumid",
                "{550E8400-E29B-41D4-A716-446655440000}",
            ),
            ("barcode", "barcode", "00036000291452"),
            ("qobuz", "barcode", "00602557393958"),
            ("isrc", "isrc", "USRC12312345"),
            ("opaque", "isni", "0007"),
        ] {
            let artist=serde_json::from_value(serde_json::json!({"provider":"library","item_id":item_id,"name":item_id,"external_ids":[[id_type,value]]})).unwrap();
            write_artist_observation(&transaction, &artist, &user, FieldKnowledge::Unknown)
                .unwrap();
        }
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        for (value, expected) in [
            ("550e8400e29b41d4a716446655440000", "mb"),
            ("036000291452", "barcode"),
            ("0060255739395", "qobuz"),
            ("us-rc1-23-12345", "isrc"),
            ("0007", "opaque"),
        ] {
            let ArtistLookupResult::Observed(artist) = read_artist_lookup(
                &transaction,
                &ArtistLookupKey::ExternalValue(value.into()),
                &user,
                ArtistReadProjection::Detail,
            )
            .unwrap() else {
                panic!("missing external identity {value}")
            };
            assert_eq!(artist.media.item_id.as_str(), expected);
        }
        for (id_type, value) in [
            (ArtistExternalIdType::Other("isni".into()), " 0007 "),
            (ArtistExternalIdType::Barcode, "036000291452"),
            (ArtistExternalIdType::Isrc, "us-rc1-23-12345"),
        ] {
            let key = ArtistLookupKey::ExternalId {
                id_type,
                value: value.into(),
            };
            assert!(matches!(
                read_artist_lookup(&transaction, &key, &user, ArtistReadProjection::Summary)
                    .unwrap(),
                ArtistLookupResult::Observed(_)
            ));
        }
        let plan=transaction.prepare("EXPLAIN QUERY PLAN SELECT DISTINCT ids.artist_item_id FROM json_each(?1) AS lookup CROSS JOIN artist_external_ids AS ids WHERE ids.external_id COLLATE NOCASE=lookup.value AND ids.artist_provider='library' LIMIT 2").unwrap()
            .query_map(["[\"0007\"]"],|row|row.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert!(
            plan.iter()
                .any(|part| part.contains("COVERING INDEX artist_external_value_lookup_idx")),
            "{plan:?}"
        );
        transaction.commit().unwrap();
        let transaction = connection.transaction().unwrap();
        let other=serde_json::from_value(serde_json::json!({"provider":"library","item_id":"competing","name":"Competing","external_ids":[["discogs","0007"]]})).unwrap();
        write_artist_observation(&transaction, &other, &user, FieldKnowledge::Unknown).unwrap();
        transaction.execute("UPDATE artists SET metadata='{\"description\":[]}',metadata_known=1 WHERE item_id='competing'",[]).unwrap();
        assert!(matches!(
            read_artist_lookup(
                &transaction,
                &ArtistLookupKey::ExternalValue("0007".into()),
                &user,
                ArtistReadProjection::Detail
            )
            .unwrap(),
            ArtistLookupResult::AmbiguousLibraryMapping
        ));
    }

    #[test]
    fn artist_identity_lookup_normalizes_musicbrainz_and_separates_domain_and_type() {
        let mut connection = database();
        let incoming = artist(
            r#"{"provider":"library","item_id":"007","name":"Artist",
            "external_ids":[["musicbrainz_artistid","83D91898-7763-47D7-B03B-B92132375C47"]],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"mapped"},
                {"provider_domain":"source","provider_instance":"source-b","item_id":"mapped"}]}"#,
        );
        observe(&mut connection, &incoming, "user", FieldKnowledge::Unknown).unwrap();
        let snapshot = connection.transaction().unwrap();
        let keys = [
            ArtistLookupKey::ProviderDomain {
                provider_domain: ProviderId("source".to_owned()),
                item_id: MediaId("mapped".to_owned()),
            },
            ArtistLookupKey::ExternalId {
                id_type: ArtistExternalIdType::MusicBrainzArtist,
                value: " {83d91898-7763-47d7-b03b-b92132375c47} ".to_owned(),
            },
        ];
        for key in keys {
            let result = read_artist_lookup(
                &snapshot,
                &key,
                &UserId("user".to_owned()),
                ArtistReadProjection::Detail,
            )
            .unwrap();
            let ArtistLookupResult::Observed(observed) = result else {
                panic!("expected library match")
            };
            assert_eq!(observed, incoming);
        }
        let wrong_type = ArtistLookupKey::ExternalId {
            id_type: ArtistExternalIdType::Discogs,
            value: "83d91898-7763-47d7-b03b-b92132375c47".to_owned(),
        };
        assert!(matches!(
            read_artist_lookup(
                &snapshot,
                &wrong_type,
                &UserId("user".to_owned()),
                ArtistReadProjection::Detail
            )
            .unwrap(),
            ArtistLookupResult::Unknown
        ));
    }

    #[test]
    fn artist_identity_lookup_domain_ambiguity_is_not_hidden_by_projection_or_membership() {
        let mut connection = database();
        for (item_id, instance) in [("1", "source-a"), ("2", "source-b")] {
            let incoming = artist(
                &serde_json::json!({
                    "provider":"library", "item_id":item_id, "name":"Artist",
                    "provider_mappings":[{"provider_domain":"source", "provider_instance":instance,
                        "item_id":"007", "in_library":false}],
                    "metadata":{"description":"detail"}
                })
                .to_string(),
            );
            observe(
                &mut connection,
                &incoming,
                "user",
                FieldKnowledge::Known(false),
            )
            .unwrap();
        }
        connection
            .execute("UPDATE artists SET metadata='{\"description\":42}'", [])
            .unwrap();
        let snapshot = connection.transaction().unwrap();
        let key = ArtistLookupKey::ProviderDomain {
            provider_domain: ProviderId("source".to_owned()),
            item_id: MediaId("007".to_owned()),
        };
        for projection in [ArtistReadProjection::Summary, ArtistReadProjection::Detail] {
            assert!(matches!(
                read_artist_lookup(&snapshot, &key, &UserId("user".to_owned()), projection)
                    .unwrap(),
                ArtistLookupResult::AmbiguousLibraryMapping
            ));
        }
        for (domain, item_id) in [("source-a", "007"), ("source", "7"), ("other", "007")] {
            let key = ArtistLookupKey::ProviderDomain {
                provider_domain: ProviderId(domain.to_owned()),
                item_id: MediaId(item_id.to_owned()),
            };
            assert!(matches!(
                read_artist_lookup(
                    &snapshot,
                    &key,
                    &UserId("user".to_owned()),
                    ArtistReadProjection::Detail
                )
                .unwrap(),
                ArtistLookupResult::Unknown
            ));
        }
    }

    #[test]
    fn artist_identity_lookup_external_matches_are_exact_typed_and_library_scoped() {
        let mut connection = database();
        for (provider, item_id, id_type, value) in [
            ("library", "discogs", "discogs", "AbC007"),
            ("library", "audio-db", "tadb", "AbC007"),
            ("source-a", "provider-only", "discogs", "provider-id"),
            (
                "library",
                "invalid-mbid",
                "musicbrainz_artistid",
                "not-a-uuid",
            ),
        ] {
            let incoming = artist(
                &serde_json::json!({"provider":provider, "item_id":item_id,
                "name":"Artist", "external_ids":[[id_type,value]]})
                .to_string(),
            );
            observe(&mut connection, &incoming, "user", FieldKnowledge::Unknown).unwrap();
        }
        let snapshot = connection.transaction().unwrap();
        for (id_type, value, expected) in [
            (ArtistExternalIdType::Discogs, " abc007 ", Some("discogs")),
            (ArtistExternalIdType::AudioDb, "ABC007", Some("audio-db")),
            (
                ArtistExternalIdType::MusicBrainzArtist,
                " not-a-uuid ",
                Some("invalid-mbid"),
            ),
            (ArtistExternalIdType::Discogs, "abc", None),
            (ArtistExternalIdType::Discogs, "provider-id", None),
            (ArtistExternalIdType::Discogs, "", None),
        ] {
            let key = ArtistLookupKey::ExternalId {
                id_type,
                value: value.to_owned(),
            };
            match (
                read_artist_lookup(
                    &snapshot,
                    &key,
                    &UserId("user".to_owned()),
                    ArtistReadProjection::Summary,
                )
                .unwrap(),
                expected,
            ) {
                (ArtistLookupResult::Observed(observed), Some(expected)) => {
                    assert_eq!(observed.media.item_id.as_str(), expected)
                }
                (ArtistLookupResult::Unknown, None) => {}
                _ => panic!("unexpected external lookup result"),
            }
        }
    }

    #[test]
    fn artist_identity_lookup_external_ambiguity_and_model_errors_are_not_fallbacks() {
        let mut connection = database();
        for (item_id, external_id) in [
            ("1", "{83d91898-7763-47d7-b03b-b92132375c47}"),
            ("2", "83D91898-7763-47D7-B03B-B92132375C47"),
        ] {
            let incoming = artist(
                &serde_json::json!({"provider":"library", "item_id":item_id,
                "name":"Artist", "external_ids":[["musicbrainz_artistid",external_id]],
                "metadata":{"description":"detail"}})
                .to_string(),
            );
            observe(&mut connection, &incoming, "user", FieldKnowledge::Unknown).unwrap();
        }
        connection
            .execute("UPDATE artists SET metadata='{\"description\":42}'", [])
            .unwrap();
        let key = ArtistLookupKey::ExternalId {
            id_type: ArtistExternalIdType::MusicBrainzArtist,
            value: "83d91898776347d7b03bb92132375c47".to_owned(),
        };
        {
            let snapshot = connection.transaction().unwrap();
            for projection in [ArtistReadProjection::Summary, ArtistReadProjection::Detail] {
                assert!(matches!(
                    read_artist_lookup(&snapshot, &key, &UserId("user".to_owned()), projection)
                        .unwrap(),
                    ArtistLookupResult::AmbiguousLibraryMapping
                ));
            }
        }
        connection
            .execute("DELETE FROM artists WHERE item_id='2'", [])
            .unwrap();
        let snapshot = connection.transaction().unwrap();
        assert!(read_artist_lookup(
            &snapshot,
            &key,
            &UserId("user".to_owned()),
            ArtistReadProjection::Detail
        )
        .is_err());
    }

    #[test]
    fn artist_identity_lookup_queries_use_covering_indexes() {
        let connection = database();
        for (sql, expected_index) in [
            (
                "EXPLAIN QUERY PLAN SELECT DISTINCT artist_item_id FROM artist_provider_mappings
              WHERE provider_domain=?1 AND item_id=?2 AND artist_provider='library' LIMIT 2",
                "artist_domain_lookup_idx",
            ),
            (
                "EXPLAIN QUERY PLAN SELECT DISTINCT ids.artist_item_id FROM json_each(?2) AS lookup
                            CROSS JOIN artist_external_ids AS ids
                            WHERE ids.id_type=?1 AND ids.external_id COLLATE NOCASE=lookup.value
                            AND ids.artist_provider='library' LIMIT 2",
                "artist_external_nocase_lookup_idx",
            ),
        ] {
            let plans = connection
                .prepare(sql)
                .unwrap()
                .query_map(params!["source", "[\"007\"]"], |row| {
                    row.get::<_, String>(3)
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert!(
                plans
                    .iter()
                    .any(|plan| plan.contains(&format!("USING COVERING INDEX {expected_index}"))),
                "unexpected query plan: {plans:?}"
            );
        }
    }

    // Atomic provider observation batches without implicit library membership.
    #[test]
    fn provider_batch_rolls_back_all_observations_when_a_later_item_fails() {
        let mut connection = database();
        let original =
            artist(r#"{"provider":"source-a","item_id":"007","name":"Keep","favorite":true}"#);
        observe(&mut connection, &original, "user", FieldKnowledge::Unknown).unwrap();
        let observations = [
            ArtistObservation {
                artist: artist(
                    r#"{"provider":"source-a","item_id":"007","name":"Do not commit","favorite":false}"#,
                ),
                library_membership: FieldKnowledge::Unknown,
            },
            ArtistObservation {
                artist: artist(
                    r#"{"provider":"source-b","item_id":"new","name":"Also roll back"}"#,
                ),
                library_membership: FieldKnowledge::Unknown,
            },
            ArtistObservation {
                artist: artist(
                    r#"{"provider":"source-c","item_id":"invalid","name":"Invalid",
                    "external_ids":[["missing-value"]]}"#,
                ),
                library_membership: FieldKnowledge::Unknown,
            },
        ];
        let result = (|| {
            let transaction = connection.transaction()?;
            write_artist_observations(&transaction, &observations, &UserId("user".to_owned()))?;
            transaction.commit()?;
            Ok::<_, CacheError>(())
        })();
        assert!(result.is_err());
        assert_eq!(
            load(&mut connection, "source-a", "007", "user"),
            Some(original)
        );
        assert!(load(&mut connection, "source-b", "new", "user").is_none());
        assert!(load(&mut connection, "source-c", "invalid", "user").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_batch_retains_non_library_objects_and_instance_identity_after_restart() {
        use super::super::workers::DatabaseWorkers;

        let directory = tempfile::tempdir().unwrap();
        let workers =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let expected = ["source-a", "source-b"].map(|provider| {
            serde_json::from_value::<Artist>(serde_json::json!({
                "provider":provider, "item_id":"007", "name":provider, "favorite":false,
                "artist_type":"singer", "metadata":{"description":"Observed provider detail",
                    "images":[{"type":"thumb", "path":format!("{provider}-image"), "provider":provider}]},
                "external_ids":[["musicbrainz_artist", format!("{provider}-external")]],
                "provider_mappings":[{"provider_domain":"source", "provider_instance":provider,
                    "item_id":"007", "in_library":false, "available":true}]
            })).unwrap()
        });
        let observations = expected
            .iter()
            .cloned()
            .map(|artist| ArtistObservation {
                artist,
                library_membership: FieldKnowledge::Unknown,
            })
            .collect::<Vec<_>>();
        workers
            .write_transaction(move |transaction| {
                write_artist_observations(transaction, &observations, &UserId("user".to_owned()))
            })
            .await
            .unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let (observed, memberships, candidates) = reopened
            .read_snapshot(|snapshot| {
                let user = UserId("user".to_owned());
                let mut observed = Vec::new();
                let mut memberships = Vec::new();
                for provider in ["source-a", "source-b"] {
                    let provider = ProviderInstanceId(provider.to_owned());
                    let item_id = MediaId("007".to_owned());
                    let key = ArtistLookupKey::ProviderInstance {
                        provider_instance: provider.clone(),
                        item_id: item_id.clone(),
                    };
                    observed.push(read_artist_lookup(
                        snapshot,
                        &key,
                        &user,
                        ArtistReadProjection::Detail,
                    )?);
                    memberships.push(read_artist_membership(snapshot, &provider, &item_id)?);
                }
                let candidates =
                    read_artist_candidates(snapshot, &ArtistCandidateQuery::new(user))?;
                Ok((observed, memberships, candidates))
            })
            .await
            .unwrap();
        for (result, expected) in observed.into_iter().zip(expected) {
            let ArtistLookupResult::Observed(artist) = result else {
                panic!("expected provider observation")
            };
            assert_eq!(artist, expected);
        }
        assert_eq!(
            memberships,
            [FieldKnowledge::Unknown, FieldKnowledge::Unknown]
        );
        assert!(candidates.items.is_empty());
        assert_eq!(candidates.knowledge, ArtistCandidateKnowledge::Unknown);
        reopened.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    #[test]
    fn provider_batch_keeps_explicit_membership_and_repeated_summary_detail_knowledge() {
        let mut connection = database();
        let detail = artist(
            r#"{"provider":"source-a","item_id":"007","name":"Provider",
            "favorite":true,"metadata":{"description":"Keep detail","images":[]},
            "external_ids":[["musicbrainz_artist","observed-id"]]}"#,
        );
        let observations = [
            ArtistObservation {
                artist: detail.clone(),
                library_membership: FieldKnowledge::Unknown,
            },
            ArtistObservation {
                artist: artist(
                    r#"{"provider":"source-a","item_id":"007","name":"Summary","metadata":{}}"#,
                ),
                library_membership: FieldKnowledge::Unknown,
            },
            ArtistObservation {
                artist: artist(r#"{"provider":"library","item_id":"007","name":"Library"}"#),
                library_membership: FieldKnowledge::Known(true),
            },
        ];
        let transaction = connection.transaction().unwrap();
        write_artist_observations(&transaction, &observations, &UserId("user".to_owned())).unwrap();
        transaction.commit().unwrap();
        let stored = load(&mut connection, "source-a", "007", "user").unwrap();
        let mut expected = detail;
        expected.media.name = "Summary".to_owned();
        assert_eq!(stored, expected);
        let snapshot = connection.transaction().unwrap();
        let candidates = read_artist_candidates(
            &snapshot,
            &ArtistCandidateQuery::new(UserId("user".to_owned())),
        )
        .unwrap();
        assert_eq!(candidates.items.len(), 1);
        assert_eq!(candidates.items[0].media.provider.as_str(), "library");
        assert_eq!(
            read_artist_membership(
                &snapshot,
                &ProviderInstanceId("source-a".to_owned()),
                &MediaId("007".to_owned())
            )
            .unwrap(),
            FieldKnowledge::Unknown
        );
    }

    #[test]
    fn provider_batch_empty_input_is_a_noop_and_user_favorites_remain_partitioned() {
        let mut connection = database();
        let original =
            artist(r#"{"provider":"source-a","item_id":"007","name":"Provider","favorite":true}"#);
        observe(
            &mut connection,
            &original,
            "user-a",
            FieldKnowledge::Unknown,
        )
        .unwrap();
        let transaction = connection.transaction().unwrap();
        write_artist_observations(&transaction, &[], &UserId("unused".to_owned())).unwrap();
        let observations = [ArtistObservation {
            artist: artist(
                r#"{"provider":"source-a","item_id":"007","name":"Provider","favorite":false}"#,
            ),
            library_membership: FieldKnowledge::Unknown,
        }];
        write_artist_observations(&transaction, &observations, &UserId("user-b".to_owned()))
            .unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            load(&mut connection, "source-a", "007", "user-a")
                .unwrap()
                .media
                .favorite,
            FieldKnowledge::Known(Some(true))
        );
        assert_eq!(
            load(&mut connection, "source-a", "007", "user-b")
                .unwrap()
                .media
                .favorite,
            FieldKnowledge::Known(Some(false))
        );
        let unused_user_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE user_id='unused')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!unused_user_exists);
    }

    // Library-first identity resolution without provider or domain guessing.
    #[test]
    fn artist_lookup_prefers_exact_instance_library_mapping_without_membership_filter() {
        let mut connection = database();
        let library_artist = artist(
            r#"{"provider":"library","item_id":"007","name":"Library",
            "favorite":false,"metadata":{"description":"Known detail"},
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a",
            "item_id":"same","in_library":false}]}"#,
        );
        observe(
            &mut connection,
            &library_artist,
            "user",
            FieldKnowledge::Unknown,
        )
        .unwrap();
        let direct = artist(r#"{"provider":"source-a","item_id":"same","name":"Provider"}"#);
        observe(&mut connection, &direct, "user", FieldKnowledge::Unknown).unwrap();
        let other = artist(
            r#"{"provider":"library","item_id":"other","name":"Other instance",
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-b",
            "item_id":"same","in_library":true}]}"#,
        );
        observe(&mut connection, &other, "user", FieldKnowledge::Known(true)).unwrap();
        let snapshot = connection.transaction().unwrap();
        let key = ArtistLookupKey::ProviderInstance {
            provider_instance: ProviderInstanceId("source-a".to_owned()),
            item_id: MediaId("same".to_owned()),
        };
        let result = read_artist_lookup(
            &snapshot,
            &key,
            &UserId("user".to_owned()),
            ArtistReadProjection::Detail,
        )
        .unwrap();
        let ArtistLookupResult::Observed(observed) = result else {
            panic!("expected observed library artist")
        };
        assert_eq!(observed, library_artist);
        assert_eq!(
            read_artist_membership(&snapshot, &observed.media.provider, &observed.media.item_id)
                .unwrap(),
            FieldKnowledge::Unknown
        );
    }

    #[test]
    fn artist_lookup_preserves_library_ids_and_does_not_guess_provider_domains() {
        let mut connection = database();
        for item_id in ["007", "7"] {
            let incoming: Artist = serde_json::from_value(serde_json::json!({
                "provider":"library", "item_id":item_id, "name":item_id,
                "provider_mappings":[{"provider_domain":"domain-only",
                    "provider_instance":"source-b", "item_id":item_id}]
            }))
            .unwrap();
            observe(&mut connection, &incoming, "user", FieldKnowledge::Unknown).unwrap();
        }
        let snapshot = connection.transaction().unwrap();
        for item_id in ["007", "7"] {
            let key = ArtistLookupKey::Library(MediaId(item_id.to_owned()));
            let result = read_artist_lookup(
                &snapshot,
                &key,
                &UserId("user".to_owned()),
                ArtistReadProjection::Detail,
            )
            .unwrap();
            let ArtistLookupResult::Observed(observed) = result else {
                panic!("expected exact library identity")
            };
            assert_eq!(observed.media.item_id.as_str(), item_id);
        }
        let domain_as_instance = ArtistLookupKey::ProviderInstance {
            provider_instance: ProviderInstanceId("domain-only".to_owned()),
            item_id: MediaId("007".to_owned()),
        };
        assert!(matches!(
            read_artist_lookup(
                &snapshot,
                &domain_as_instance,
                &UserId("user".to_owned()),
                ArtistReadProjection::Detail
            )
            .unwrap(),
            ArtistLookupResult::Unknown
        ));
        assert!(matches!(
            read_artist_lookup(
                &snapshot,
                &ArtistLookupKey::Library(MediaId("0007".to_owned())),
                &UserId("user".to_owned()),
                ArtistReadProjection::Detail
            )
            .unwrap(),
            ArtistLookupResult::Unknown
        ));
    }

    #[test]
    fn artist_lookup_returns_exact_provider_fallback_and_unknown_misses() {
        let mut connection = database();
        let incoming =
            artist(r#"{"provider":"source-a","item_id":"007","name":"Provider","favorite":true}"#);
        observe(&mut connection, &incoming, "user", FieldKnowledge::Unknown).unwrap();
        let snapshot = connection.transaction().unwrap();
        let key = ArtistLookupKey::ProviderInstance {
            provider_instance: incoming.media.provider.clone(),
            item_id: incoming.media.item_id.clone(),
        };
        let result = read_artist_lookup(
            &snapshot,
            &key,
            &UserId("user".to_owned()),
            ArtistReadProjection::Detail,
        )
        .unwrap();
        let ArtistLookupResult::Observed(observed) = result else {
            panic!("expected exact provider identity")
        };
        assert_eq!(observed, incoming);
        for key in [
            ArtistLookupKey::Library(MediaId("007".to_owned())),
            ArtistLookupKey::ProviderInstance {
                provider_instance: ProviderInstanceId("source-b".to_owned()),
                item_id: MediaId("007".to_owned()),
            },
        ] {
            assert!(matches!(
                read_artist_lookup(
                    &snapshot,
                    &key,
                    &UserId("user".to_owned()),
                    ArtistReadProjection::Detail
                )
                .unwrap(),
                ArtistLookupResult::Unknown
            ));
        }
    }

    #[test]
    fn artist_lookup_reports_competing_library_mappings_before_model_reconstruction() {
        let mut connection = database();
        for item_id in ["a", "b"] {
            let incoming: Artist = serde_json::from_value(serde_json::json!({
                "provider":"library", "item_id":item_id, "name":"Artist",
                "provider_mappings":[{"provider_domain":"source", "provider_instance":"source-a", "item_id":"same"}]
            })).unwrap();
            observe(&mut connection, &incoming, "user", FieldKnowledge::Unknown).unwrap();
        }
        let direct =
            artist(r#"{"provider":"source-a","item_id":"same","name":"Provider fallback"}"#);
        observe(&mut connection, &direct, "user", FieldKnowledge::Unknown).unwrap();
        connection
            .execute(
                "UPDATE artists SET metadata=?1, metadata_known=1 WHERE provider='library'",
                [r#"{"images":[42],"description":42}"#],
            )
            .unwrap();
        let snapshot = connection.transaction().unwrap();
        let key = ArtistLookupKey::ProviderInstance {
            provider_instance: ProviderInstanceId("source-a".to_owned()),
            item_id: MediaId("same".to_owned()),
        };
        for projection in [ArtistReadProjection::Summary, ArtistReadProjection::Detail] {
            assert!(matches!(
                read_artist_lookup(&snapshot, &key, &UserId("user".to_owned()), projection)
                    .unwrap(),
                ArtistLookupResult::AmbiguousLibraryMapping
            ));
        }
    }

    #[test]
    fn artist_lookup_retains_projection_and_requested_user_without_masking_detail_errors() {
        let mut connection = database();
        let incoming = artist(
            r#"{"provider":"library","item_id":"007","name":"Library","favorite":true,
            "metadata":{"description":"Detail","images":[]},"external_ids":[["musicbrainz","external"]],
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"same"}]}"#,
        );
        observe(
            &mut connection,
            &incoming,
            "user-a",
            FieldKnowledge::Unknown,
        )
        .unwrap();
        let rating =
            artist(r#"{"provider":"library","item_id":"007","name":"Library","favorite":false}"#);
        observe(&mut connection, &rating, "user-b", FieldKnowledge::Unknown).unwrap();
        let snapshot = connection.transaction().unwrap();
        let key = ArtistLookupKey::ProviderInstance {
            provider_instance: ProviderInstanceId("source-a".to_owned()),
            item_id: MediaId("same".to_owned()),
        };
        for projection in [ArtistReadProjection::Summary, ArtistReadProjection::Detail] {
            let result =
                read_artist_lookup(&snapshot, &key, &UserId("user-b".to_owned()), projection)
                    .unwrap();
            let ArtistLookupResult::Observed(observed) = result else {
                panic!("expected observed library artist")
            };
            assert_eq!(observed.media.favorite, FieldKnowledge::Known(Some(false)));
            assert_eq!(
                observed.media.knowledge.external_ids,
                projection == ArtistReadProjection::Detail
            );
            assert_eq!(
                observed
                    .media
                    .metadata
                    .as_ref()
                    .unwrap()
                    .knowledge
                    .description,
                projection == ArtistReadProjection::Detail
            );
        }
        let result = read_artist_lookup(
            &snapshot,
            &key,
            &UserId("unobserved".to_owned()),
            ArtistReadProjection::Detail,
        )
        .unwrap();
        let ArtistLookupResult::Observed(observed) = result else {
            panic!("expected observed library artist")
        };
        assert_eq!(observed.media.favorite, FieldKnowledge::Unknown);
        snapshot.rollback().unwrap();
        let direct =
            artist(r#"{"provider":"source-a","item_id":"same","name":"Do not hide error"}"#);
        observe(&mut connection, &direct, "user-b", FieldKnowledge::Unknown).unwrap();
        connection
            .execute(
                "UPDATE artists SET metadata=?1 WHERE provider='library'",
                [r#"{"description":42}"#],
            )
            .unwrap();
        let snapshot = connection.transaction().unwrap();
        assert!(read_artist_lookup(
            &snapshot,
            &key,
            &UserId("user-b".to_owned()),
            ArtistReadProjection::Detail
        )
        .is_err());
    }

    #[test]
    fn artist_lookup_uses_covering_index_and_skips_large_unrelated_detail_trees() {
        let mut connection = database();
        let incoming = artist(
            r#"{"provider":"library","item_id":"007","name":"Selected",
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"selected"}]}"#,
        );
        observe(&mut connection, &incoming, "user", FieldKnowledge::Unknown).unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..2_000 {
            let item_id = format!("unrelated-{index:04}");
            insert_artist(&transaction, "library", &item_id).unwrap();
            transaction
                .execute(
                    "UPDATE artists SET metadata=?2, metadata_known=1 WHERE item_id=?1",
                    params![item_id, r#"{"images":[42],"description":42}"#],
                )
                .unwrap();
            transaction.execute(
                "INSERT INTO artist_provider_mappings (artist_provider, artist_item_id, position,
                 provider_domain, provider_instance, item_id, in_library_known, is_unique_known,
                 audio_format_known, url_known, details_known)
                 VALUES ('library', ?1, 0, 'source', 'source-a', ?1, 0, 0, 0, 0, 0)", [&item_id],
            ).unwrap();
        }
        transaction.commit().unwrap();
        let snapshot = connection.transaction().unwrap();
        let mut plan = snapshot
            .prepare(
                "EXPLAIN QUERY PLAN SELECT DISTINCT artist_item_id FROM artist_provider_mappings
             WHERE provider_instance=?1 AND item_id=?2 AND artist_provider='library' LIMIT 2",
            )
            .unwrap();
        let details = plan
            .query_map(["source-a", "selected"], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(
            details.contains("SEARCH")
                && details.contains("COVERING INDEX artist_provider_lookup_idx"),
            "{details}"
        );
        let key = ArtistLookupKey::ProviderInstance {
            provider_instance: ProviderInstanceId("source-a".to_owned()),
            item_id: MediaId("selected".to_owned()),
        };
        let result = read_artist_lookup(
            &snapshot,
            &key,
            &UserId("user".to_owned()),
            ArtistReadProjection::Detail,
        )
        .unwrap();
        let ArtistLookupResult::Observed(observed) = result else {
            panic!("expected selected artist")
        };
        assert_eq!(observed, incoming);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_lookup_survives_worker_restart() {
        use super::super::workers::DatabaseWorkers;

        let directory = tempfile::tempdir().unwrap();
        let workers =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let incoming = artist(
            r#"{"provider":"library","item_id":"007","name":"Selected","favorite":false,
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"selected"}]}"#,
        );
        let expected = incoming.clone();
        workers
            .write_transaction(move |transaction| {
                write_artist_observation(
                    transaction,
                    &incoming,
                    &UserId("user".to_owned()),
                    FieldKnowledge::Unknown,
                )
            })
            .await
            .unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let result = reopened
            .read_snapshot(|snapshot| {
                let key = ArtistLookupKey::ProviderInstance {
                    provider_instance: ProviderInstanceId("source-a".to_owned()),
                    item_id: MediaId("selected".to_owned()),
                };
                read_artist_lookup(
                    snapshot,
                    &key,
                    &UserId("user".to_owned()),
                    ArtistReadProjection::Detail,
                )
            })
            .await
            .unwrap();
        let ArtistLookupResult::Observed(observed) = result else {
            panic!("expected persisted mapping")
        };
        assert_eq!(observed, expected);
        reopened.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    // Summary projections and retained detail knowledge.
    #[test]
    fn summary_projection_skips_unrequested_details_and_preserves_mapping_fields() {
        let mut connection = database();
        let incoming = artist(
            r#"{"provider":"library","item_id":"summary","name":"Artist","favorite":false,
            "artist_type":"singer","metadata":{"images":[{"type":"thumb","path":"image-a",
            "provider":"source-a"},{"type":"fanart","path":"image-b","provider":"source-b"}],
            "description":"Detail"},"external_ids":[["musicbrainz","external"]],
            "provider_mappings":[{"item_id":"mapped","provider_domain":"source",
            "provider_instance":"source-a","available":false,"in_library":true,
            "audio_format":{"sample_rate":96000},"details":"retained"}]}"#,
        );
        observe(
            &mut connection,
            &incoming,
            "user",
            FieldKnowledge::Known(true),
        )
        .unwrap();
        connection
            .execute(
                "UPDATE artists SET metadata=json_set(metadata, '$.description', 42,
             '$.genres', json('[42]')) WHERE item_id='summary'",
                [],
            )
            .unwrap();
        let snapshot = connection.transaction().unwrap();
        let provider = ProviderInstanceId("library".to_owned());
        let item_id = MediaId("summary".to_owned());
        let user = UserId("user".to_owned());
        assert!(read_artist(&snapshot, &provider, &item_id, &user).is_err());
        let summary = read_artist_summary(&snapshot, &provider, &item_id, &user)
            .unwrap()
            .unwrap();
        assert_eq!(summary.media.favorite, incoming.media.favorite);
        assert_eq!(summary.artist_type, incoming.artist_type);
        assert_eq!(
            summary.media.provider_mappings,
            incoming.media.provider_mappings
        );
        assert!(summary.media.external_ids.is_empty());
        assert!(!summary.media.knowledge.external_ids);
        let metadata = summary.media.metadata.as_ref().unwrap();
        assert_eq!(
            metadata.images,
            incoming.media.metadata.as_ref().unwrap().images
        );
        assert!(metadata.knowledge.images);
        assert!(!metadata.knowledge.description);
        assert!(!metadata.knowledge.genres);
        let mut merged = MediaItem::Artist(incoming.clone());
        merged.merge_known(MediaItem::Artist(summary));
        assert_eq!(merged, MediaItem::Artist(incoming));
    }

    #[test]
    fn summary_projection_retains_unknown_null_and_empty_metadata_knowledge() {
        let mut connection = database();
        for (item_id, metadata) in [
            ("unknown", None),
            ("null", Some(serde_json::Value::Null)),
            ("missing-images", Some(serde_json::json!({}))),
            ("empty-images", Some(serde_json::json!({"images": []}))),
            ("null-images", Some(serde_json::json!({"images": null}))),
        ] {
            let mut input =
                serde_json::json!({"provider":"library", "item_id":item_id, "name":"Artist"});
            if let Some(metadata) = metadata {
                input["metadata"] = metadata;
            }
            let incoming: Artist = serde_json::from_value(input).unwrap();
            observe(
                &mut connection,
                &incoming,
                "user",
                FieldKnowledge::Known(true),
            )
            .unwrap();
            let snapshot = connection.transaction().unwrap();
            let summary = read_artist_summary(
                &snapshot,
                &incoming.media.provider,
                &incoming.media.item_id,
                &UserId("user".to_owned()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                summary.media.knowledge.metadata, incoming.media.knowledge.metadata,
                "{item_id}"
            );
            assert_eq!(summary.media.metadata, incoming.media.metadata, "{item_id}");
        }
    }

    #[test]
    fn summary_projection_does_not_read_external_ids_or_other_users_favorites() {
        let mut connection = database();
        let incoming = artist(
            r#"{"provider":"library","item_id":"summary","name":"Artist",
            "favorite":true,"position":7,"date_added":"2026-11-07",
            "external_ids":[["musicbrainz","external"]]}"#,
        );
        observe(
            &mut connection,
            &incoming,
            "user-a",
            FieldKnowledge::Known(true),
        )
        .unwrap();
        connection
            .execute("DROP TABLE artist_external_ids", [])
            .unwrap();
        let snapshot = connection.transaction().unwrap();
        let summary = read_artist_summary(
            &snapshot,
            &incoming.media.provider,
            &incoming.media.item_id,
            &UserId("user-b".to_owned()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(summary.media.favorite, FieldKnowledge::Unknown);
        assert!(!summary.media.knowledge.external_ids);
        assert!(!summary.media.knowledge.position);
        assert!(!summary.media.knowledge.date_added);
        assert_eq!(summary.media.position, None);
        assert_eq!(summary.media.date_added, None);
        assert!(read_artist(
            &snapshot,
            &incoming.media.provider,
            &incoming.media.item_id,
            &UserId("user-a".to_owned()),
        )
        .is_err());
    }

    #[test]
    fn summary_projection_candidate_pages_use_explicit_requested_projection() {
        let mut connection = database();
        for (item_id, sort_name) in [("a", "Alpha"), ("b", "Beta")] {
            let input = serde_json::json!({"provider":"library", "item_id":item_id, "name":"Artist",
                "sort_name":sort_name, "metadata":{"description":"detail", "images":[]},
                "external_ids":[["musicbrainz",item_id]]});
            let incoming: Artist = serde_json::from_value(input).unwrap();
            observe(
                &mut connection,
                &incoming,
                "user",
                FieldKnowledge::Known(true),
            )
            .unwrap();
        }
        let snapshot = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.limit = std::num::NonZeroU32::new(1).unwrap();
        query.offset = 1;
        assert_eq!(query.projection, ArtistReadProjection::Summary);
        let summary = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(summary.items.len(), 1);
        assert_eq!(summary.items[0].media.item_id.as_str(), "b");
        assert!(!summary.items[0].media.knowledge.external_ids);
        assert!(
            !summary.items[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .knowledge
                .description
        );
        query.projection = ArtistReadProjection::Detail;
        let detail = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(detail.items.len(), 1);
        assert_eq!(
            detail.items[0].media.item_id,
            summary.items[0].media.item_id
        );
        assert_eq!(detail.knowledge, summary.knowledge);
        assert!(detail.items[0].media.knowledge.external_ids);
        assert_eq!(
            detail.items[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("detail")
        );
    }

    #[test]
    fn summary_projection_rejects_corrupt_requested_images_and_mapping_formats() {
        for (field, statement, malformed) in [
            (
                "images",
                "UPDATE artists SET metadata=?1",
                r#"{"images":[42]}"#,
            ),
            (
                "audio_format",
                "UPDATE artist_provider_mappings SET audio_format=?1",
                r#"{"sample_rate":"invalid"}"#,
            ),
        ] {
            let mut connection = database();
            let incoming = artist(
                r#"{"provider":"library","item_id":"summary","name":"Artist",
                "metadata":{"images":[]},"provider_mappings":[{"item_id":"mapped",
                "provider_domain":"source","provider_instance":"source-a","audio_format":{}}]}"#,
            );
            observe(
                &mut connection,
                &incoming,
                "user",
                FieldKnowledge::Known(true),
            )
            .unwrap();
            assert_eq!(connection.execute(statement, [malformed]).unwrap(), 1);
            let snapshot = connection.transaction().unwrap();
            assert!(
                read_artist_summary(
                    &snapshot,
                    &incoming.media.provider,
                    &incoming.media.item_id,
                    &UserId("user".to_owned()),
                )
                .is_err(),
                "{field}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn summary_projection_survives_worker_restart_without_changing_stored_details() {
        use super::super::workers::DatabaseWorkers;

        let directory = tempfile::tempdir().unwrap();
        let workers =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let incoming = artist(
            r#"{"provider":"library","item_id":"summary","name":"Artist",
            "favorite":false,"artist_type":"singer","metadata":{"description":"Keep detail",
            "images":[{"type":"thumb","path":"image","provider":"source-a"}]},
            "external_ids":[["musicbrainz","external"]],"provider_mappings":[{"item_id":"mapped",
            "provider_domain":"source","provider_instance":"source-a","audio_format":{"sample_rate":96000}}]}"#,
        );
        let expected = incoming.clone();
        workers
            .write_transaction(move |transaction| {
                write_artist_observation(
                    transaction,
                    &incoming,
                    &UserId("user".to_owned()),
                    FieldKnowledge::Known(true),
                )
            })
            .await
            .unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let (candidates, detail) = reopened
            .read_snapshot(|snapshot| {
                let mut query = ArtistCandidateQuery::new(UserId("user".to_owned()));
                query.favorite = Some(false);
                let candidates = read_artist_candidates(snapshot, &query)?;
                let detail = read_artist(
                    snapshot,
                    &ProviderInstanceId("library".to_owned()),
                    &MediaId("summary".to_owned()),
                    &query.user_id,
                )?;
                Ok((candidates, detail))
            })
            .await
            .unwrap();
        assert_eq!(detail, Some(expected.clone()));
        assert_eq!(candidates.knowledge, ArtistCandidateKnowledge::Partial);
        assert_eq!(candidates.items.len(), 1);
        let summary = &candidates.items[0];
        assert_eq!(
            summary.media.provider_mappings,
            expected.media.provider_mappings
        );
        assert_eq!(
            summary.media.metadata.as_ref().unwrap().images,
            expected.media.metadata.as_ref().unwrap().images
        );
        assert!(!summary.media.knowledge.external_ids);
        assert!(
            !summary
                .media
                .metadata
                .as_ref()
                .unwrap()
                .knowledge
                .description
        );
        reopened.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    // Preliminary selections, filter isolation and SQL pagination.
    #[test]
    fn candidate_pages_filter_before_reconstruction_and_never_claim_completeness() {
        let mut connection = database();
        for (item_id, sort_name, membership) in [
            ("b", "Beta", true),
            ("a", "Alpha", true),
            ("hidden", "Aardvark", false),
        ] {
            let incoming = artist(&format!(
                r#"{{"provider":"library","item_id":"{item_id}","name":"Artist","sort_name":"{sort_name}"}}"#
            ));
            observe(
                &mut connection,
                &incoming,
                "user",
                FieldKnowledge::Known(membership),
            )
            .unwrap();
        }
        connection
            .execute(
                "UPDATE artists SET metadata=?1 WHERE item_id='hidden'",
                [r#"{"description":42}"#],
            )
            .unwrap();
        let snapshot = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.limit = std::num::NonZeroU32::new(1).unwrap();
        query.offset = 1;
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(result.knowledge, ArtistCandidateKnowledge::Partial);
        assert_eq!(result.items[0].media.item_id.as_str(), "b");
        query.direction = ArtistCandidateDirection::Descending;
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(result.items[0].media.item_id.as_str(), "a");
        query.offset = 20;
        let empty = read_artist_candidates(&snapshot, &query).unwrap();
        assert!(empty.items.is_empty());
        assert_eq!(empty.knowledge, ArtistCandidateKnowledge::Partial);
    }

    #[test]
    fn candidate_miss_is_unknown_not_a_confirmed_empty_library() {
        let mut connection = database();
        let snapshot = connection.transaction().unwrap();
        let result = read_artist_candidates(
            &snapshot,
            &ArtistCandidateQuery::new(UserId("user".to_owned())),
        )
        .unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.knowledge, ArtistCandidateKnowledge::Unknown);
    }

    #[test]
    fn candidate_favorite_filter_does_not_treat_unknown_or_unrated_as_disliked() {
        let mut connection = database();
        for (item_id, rating) in [
            ("liked", "true"),
            ("disliked", "false"),
            ("unrated", "null"),
        ] {
            let incoming = artist(&format!(
                r#"{{"provider":"library","item_id":"{item_id}","name":"Artist","favorite":{rating}}}"#
            ));
            observe(
                &mut connection,
                &incoming,
                "user-a",
                FieldKnowledge::Known(true),
            )
            .unwrap();
        }
        let unknown = artist(r#"{"provider":"library","item_id":"unknown","name":"Artist"}"#);
        observe(
            &mut connection,
            &unknown,
            "user-a",
            FieldKnowledge::Known(true),
        )
        .unwrap();
        let other_user =
            artist(r#"{"provider":"library","item_id":"liked","name":"Artist","favorite":false}"#);
        observe(
            &mut connection,
            &other_user,
            "user-b",
            FieldKnowledge::Unknown,
        )
        .unwrap();
        let snapshot = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(UserId("user-a".to_owned()));
        query.favorite = Some(false);
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].media.item_id.as_str(), "disliked");
        query.user_id = UserId("user-b".to_owned());
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].media.item_id.as_str(), "liked");
    }

    #[test]
    fn candidate_provider_filter_requires_same_in_library_mapping_and_no_duplicates() {
        let mut connection = database();
        let mismatched = artist(
            r#"{"provider":"library","item_id":"mismatched","name":"Artist",
            "provider_mappings":[{"item_id":"p","provider_domain":"source","provider_instance":"selected","in_library":false},
                {"item_id":"p","provider_domain":"source","provider_instance":"other","in_library":true}]}"#,
        );
        let matched = artist(
            r#"{"provider":"library","item_id":"matched","name":"Artist",
            "provider_mappings":[{"item_id":"p","provider_domain":"source","provider_instance":"selected","in_library":true},
                {"item_id":"q","provider_domain":"source","provider_instance":"selected","in_library":true}]}"#,
        );
        for incoming in [&mismatched, &matched] {
            observe(
                &mut connection,
                incoming,
                "user",
                FieldKnowledge::Known(true),
            )
            .unwrap();
        }
        let snapshot = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.provider_instances = Some(vec![ProviderInstanceId("selected".to_owned())]);
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].media.item_id.as_str(), "matched");
        query.provider_instances = Some(Vec::new());
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.knowledge, ArtistCandidateKnowledge::Partial);
    }

    #[test]
    fn candidate_artist_type_filter_and_sort_ties_are_stable() {
        let mut connection = database();
        for (item_id, artist_type) in [
            ("b", Some("singer")),
            ("a", Some("singer")),
            ("narrator", Some("narrator")),
            ("unknown", None),
        ] {
            let mut incoming = artist(&format!(
                r#"{{"provider":"library","item_id":"{item_id}","name":"Artist","sort_name":"Same"}}"#
            ));
            if let Some(artist_type) = artist_type {
                incoming.artist_type = Some(artist_type.to_owned());
                incoming.knowledge.artist_type = true;
            }
            observe(
                &mut connection,
                &incoming,
                "user",
                FieldKnowledge::Known(true),
            )
            .unwrap();
        }
        let snapshot = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.artist_type = Some(crate::domain::catalog::LibraryArtistType::Singer);
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(
            result
                .items
                .iter()
                .map(|artist| artist.media.item_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        query.direction = ArtistCandidateDirection::Descending;
        let result = read_artist_candidates(&snapshot, &query).unwrap();
        assert_eq!(
            result
                .items
                .iter()
                .map(|artist| artist.media.item_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn candidate_page_does_not_reconstruct_large_unselected_member_details() {
        let mut connection = database();
        let selected = artist(
            r#"{"provider":"library","item_id":"selected","name":"Selected","sort_name":"Alpha"}"#,
        );
        observe(
            &mut connection,
            &selected,
            "user",
            FieldKnowledge::Known(true),
        )
        .unwrap();
        let invalid_metadata =
            serde_json::to_string(&serde_json::json!({"description":42})).unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..2_000 {
            let item_id = format!("unselected-{index:04}");
            insert_artist(&transaction, "library", &item_id).unwrap();
            transaction
                .execute(
                    "UPDATE artists SET sort_name='Zulu', sort_name_known=1, metadata=?2,
                 metadata_known=1 WHERE provider='library' AND item_id=?1",
                    params![item_id, invalid_metadata],
                )
                .unwrap();
            transaction
                .execute(
                    "INSERT INTO artist_library_entries VALUES ('library', ?1, 1)",
                    [&item_id],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        let snapshot = connection.transaction().unwrap();
        let mut query = ArtistCandidateQuery::new(UserId("user".to_owned()));
        query.limit = std::num::NonZeroU32::new(1).unwrap();
        for projection in [ArtistReadProjection::Summary, ArtistReadProjection::Detail] {
            query.projection = projection;
            let result = read_artist_candidates(&snapshot, &query).unwrap();
            assert_eq!(result.items, [selected.clone()]);
            assert_eq!(result.knowledge, ArtistCandidateKnowledge::Partial);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn candidate_selection_survives_worker_restart() {
        use super::super::workers::DatabaseWorkers;

        let directory = tempfile::tempdir().unwrap();
        let workers =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let incoming = artist(
            r#"{"provider":"library","item_id":"selected","name":"Selected","sort_name":"Alpha","favorite":false}"#,
        );
        let expected = incoming.clone();
        workers
            .write_transaction(move |transaction| {
                write_artist_observation(
                    transaction,
                    &incoming,
                    &UserId("user".to_owned()),
                    FieldKnowledge::Known(true),
                )
            })
            .await
            .unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let result = reopened
            .read_snapshot(|snapshot| {
                let mut query = ArtistCandidateQuery::new(UserId("user".to_owned()));
                query.favorite = Some(false);
                read_artist_candidates(snapshot, &query)
            })
            .await
            .unwrap();
        assert_eq!(result.items, [expected]);
        assert_eq!(result.knowledge, ArtistCandidateKnowledge::Partial);
        reopened.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    // Observation roundtrips and authoritative field knowledge.
    #[test]
    fn detail_roundtrip_and_summary_keep_known_details_and_relations() {
        let mut connection = database();
        let detail = artist(
            r#"{"provider":"library","item_id":"007","name":"Detail",
            "favorite":true,"sort_name":"Artist","version":"Original","artist_type":"singer",
            "metadata":{"description":"Known description","images":[]},
            "external_ids":[["musicbrainz","original-id"]],
            "provider_mappings":[{"item_id":"provider-007","provider_domain":"source",
                "provider_instance":"source-a","available":false,"in_library":null,
                "audio_format":{"sample_rate":48000}}]}"#,
        );
        observe(&mut connection, &detail, "user-a", FieldKnowledge::Unknown).unwrap();
        assert_eq!(
            load(&mut connection, "library", "007", "user-a").unwrap(),
            detail
        );
        let summary =
            artist(r#"{"provider":"library","item_id":"007","name":"Renamed","metadata":{}}"#);
        observe(&mut connection, &summary, "user-a", FieldKnowledge::Unknown).unwrap();
        let mut expected = MediaItem::Artist(detail);
        expected.merge_known(MediaItem::Artist(summary));
        let MediaItem::Artist(expected) = expected else {
            unreachable!()
        };
        assert_eq!(
            load(&mut connection, "library", "007", "user-a").unwrap(),
            expected
        );
        let snapshot = connection.transaction().unwrap();
        assert_eq!(
            read_artist_membership(
                &snapshot,
                &ProviderInstanceId("library".to_owned()),
                &MediaId("007".to_owned())
            )
            .unwrap(),
            FieldKnowledge::Unknown
        );
    }

    #[test]
    fn explicit_null_and_empty_values_clear_only_observed_fields() {
        let mut connection = database();
        let detail = artist(
            r#"{"provider":"library","item_id":"a","name":"Artist","favorite":true,
            "sort_name":"Known","artist_type":"singer","metadata":{"description":"Known"},
            "external_ids":[["musicbrainz","id"]],"provider_mappings":[{"item_id":"p",
            "provider_domain":"source","provider_instance":"source-a"}]}"#,
        );
        observe(
            &mut connection,
            &detail,
            "user",
            FieldKnowledge::Known(true),
        )
        .unwrap();
        let clear = artist(
            r#"{"provider":"library","item_id":"a","name":"Artist","favorite":null,
            "sort_name":null,"metadata":{"description":null},"external_ids":[],"provider_mappings":[]}"#,
        );
        observe(
            &mut connection,
            &clear,
            "user",
            FieldKnowledge::Known(false),
        )
        .unwrap();
        let stored = load(&mut connection, "library", "a", "user").unwrap();
        assert_eq!(stored.media.favorite, FieldKnowledge::Known(None));
        assert!(stored.media.knowledge.sort_name);
        assert_eq!(stored.media.sort_name, None);
        assert!(stored.media.provider_mappings.is_empty());
        assert!(stored.media.external_ids.is_empty());
        assert_eq!(stored.artist_type.as_deref(), Some("singer"));
        assert_eq!(stored.media.metadata.unwrap().description, None);
        let snapshot = connection.transaction().unwrap();
        assert_eq!(
            read_artist_membership(
                &snapshot,
                &ProviderInstanceId("library".to_owned()),
                &MediaId("a".to_owned())
            )
            .unwrap(),
            FieldKnowledge::Known(false)
        );
    }

    #[test]
    fn favorites_do_not_leak_between_users_or_provider_identities() {
        let mut connection = database();
        for (user, rating) in [("user-a", "true"), ("user-b", "false"), ("user-c", "null")] {
            let incoming = artist(&format!(
                r#"{{"provider":"library","item_id":"007","name":"Artist","favorite":{rating}}}"#
            ));
            observe(&mut connection, &incoming, user, FieldKnowledge::Unknown).unwrap();
        }
        for (user, expected) in [
            ("user-a", FieldKnowledge::Known(Some(true))),
            ("user-b", FieldKnowledge::Known(Some(false))),
            ("user-c", FieldKnowledge::Known(None)),
            ("unobserved", FieldKnowledge::Unknown),
        ] {
            assert_eq!(
                load(&mut connection, "library", "007", user)
                    .unwrap()
                    .media
                    .favorite,
                expected
            );
        }
        let provider_artist =
            artist(r#"{"provider":"source-a","item_id":"007","name":"Other artist"}"#);
        observe(
            &mut connection,
            &provider_artist,
            "user-a",
            FieldKnowledge::Unknown,
        )
        .unwrap();
        assert_eq!(
            load(&mut connection, "source-a", "007", "user-a")
                .unwrap()
                .media
                .favorite,
            FieldKnowledge::Unknown
        );
        assert!(load(&mut connection, "library", "missing", "user-a").is_none());
    }

    #[test]
    fn failed_relation_write_rolls_back_name_favorite_and_membership() {
        let mut connection = database();
        let initial =
            artist(r#"{"provider":"library","item_id":"007","name":"Keep","favorite":true}"#);
        observe(
            &mut connection,
            &initial,
            "user",
            FieldKnowledge::Known(true),
        )
        .unwrap();
        let invalid = artist(
            r#"{"provider":"library","item_id":"007","name":"Do not publish","favorite":false,
            "provider_mappings":[{"item_id":"p","provider_domain":"source","provider_instance":"source-a"},
             {"item_id":"p","provider_domain":"source","provider_instance":"source-a"}]}"#,
        );
        assert!(observe(
            &mut connection,
            &invalid,
            "user",
            FieldKnowledge::Known(false)
        )
        .is_err());
        assert_eq!(
            load(&mut connection, "library", "007", "user").unwrap(),
            initial
        );
        let snapshot = connection.transaction().unwrap();
        assert_eq!(
            read_artist_membership(
                &snapshot,
                &ProviderInstanceId("library".to_owned()),
                &MediaId("007".to_owned())
            )
            .unwrap(),
            FieldKnowledge::Known(true)
        );
    }

    #[test]
    fn targeted_read_uses_identity_and_relation_indexes() {
        let connection = database();
        for query in [
            "EXPLAIN QUERY PLAN SELECT name FROM artists WHERE provider='library' AND item_id='007'",
            "EXPLAIN QUERY PLAN SELECT item_id FROM artist_provider_mappings WHERE artist_provider='library' AND artist_item_id='007' ORDER BY position",
        ] {
            let plan: String = connection.query_row(query, [], |row| row.get(3)).unwrap();
            assert!(plan.contains("SEARCH") && plan.contains("PRIMARY KEY"), "{plan}");
        }
    }

    #[test]
    fn targeted_read_does_not_reconstruct_large_unrelated_detail_trees() {
        let mut connection = database();
        let expected = artist(
            r#"{"provider":"library","item_id":"selected","name":"Selected","favorite":false}"#,
        );
        observe(&mut connection, &expected, "user", FieldKnowledge::Unknown).unwrap();
        let invalid_metadata =
            serde_json::to_string(&serde_json::json!({"description": 42})).unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..2_000 {
            let item_id = format!("unrelated-{index}");
            insert_artist(&transaction, "library", &item_id).unwrap();
            transaction.execute(
                "UPDATE artists SET metadata=?2, metadata_known=1 WHERE provider='library' AND item_id=?1",
                params![item_id, invalid_metadata],
            ).unwrap();
        }
        transaction.commit().unwrap();
        assert_eq!(
            load(&mut connection, "library", "selected", "user").unwrap(),
            expected
        );
    }

    #[test]
    fn invalid_external_identity_is_reported_without_partial_observation() {
        let mut connection = database();
        let invalid = artist(
            r#"{"provider":"library","item_id":"invalid","name":"Invalid","external_ids":[["musicbrainz"]]}"#,
        );
        assert!(matches!(
            observe(
                &mut connection,
                &invalid,
                "user",
                FieldKnowledge::Known(true)
            ),
            Err(CacheError::InvalidMediaObservation(_))
        ));
        assert!(load(&mut connection, "library", "invalid", "user").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn worker_commit_survives_restart_and_retains_field_knowledge() {
        use super::super::workers::DatabaseWorkers;

        let directory = tempfile::tempdir().unwrap();
        let workers =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let incoming = artist(
            r#"{"provider":"library","item_id":"007","name":"Persisted",
            "favorite":null,"metadata":{"description":null},"sort_name":null,
            "provider_mappings":[],"external_ids":[]}"#,
        );
        let expected = incoming.clone();
        workers
            .write_transaction(move |transaction| {
                write_artist_observation(
                    transaction,
                    &incoming,
                    &UserId("user".to_owned()),
                    FieldKnowledge::Known(true),
                )
            })
            .await
            .unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "observed-server".to_owned())
                .await
                .unwrap();
        let (stored, membership) = reopened
            .read_snapshot(|snapshot| {
                let provider = ProviderInstanceId("library".to_owned());
                let item_id = MediaId("007".to_owned());
                Ok((
                    read_artist(snapshot, &provider, &item_id, &UserId("user".to_owned()))?,
                    read_artist_membership(snapshot, &provider, &item_id)?,
                ))
            })
            .await
            .unwrap();
        assert_eq!(stored.unwrap(), expected);
        assert_eq!(membership, FieldKnowledge::Known(true));
        reopened.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    #[test]
    fn identities_keep_provider_context_and_original_text() {
        let connection = database();
        for (provider, item_id) in [("library", "007"), ("provider-a", "007"), ("library", "7")] {
            insert_artist(&connection, provider, item_id).unwrap();
        }
        assert!(insert_artist(&connection, "library", "007").is_err());
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
        let members: i64 = connection
            .query_row("SELECT COUNT(*) FROM artist_library_entries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(members, 0);
    }

    #[test]
    fn favorites_require_observed_user_and_artist_and_allow_unrated() {
        let connection = database();
        insert_artist(&connection, "library", "007").unwrap();
        assert!(connection
            .execute(
                "INSERT INTO artist_favorites VALUES ('missing', 'library', '007', 1)",
                []
            )
            .is_err());
        connection
            .execute("INSERT INTO users VALUES ('user-a'), ('user-b')", [])
            .unwrap();
        connection.execute("INSERT INTO artist_favorites VALUES ('user-a', 'library', '007', 1), ('user-b', 'library', '007', NULL)", []).unwrap();
        assert!(connection
            .execute(
                "INSERT INTO artist_favorites VALUES ('user-a', 'library', 'unknown', 0)",
                []
            )
            .is_err());
        let rating: Option<bool> = connection
            .query_row(
                "SELECT rating FROM artist_favorites WHERE user_id = 'user-b'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rating, None);
    }

    #[test]
    fn malformed_metadata_and_invalid_field_knowledge_are_rejected() {
        let connection = database();
        insert_artist(&connection, "library", "007").unwrap();
        assert!(connection
            .execute("UPDATE artists SET metadata = '[]'", [])
            .is_err());
        assert!(connection
            .execute("UPDATE artists SET sort_name_known = 2", [])
            .is_err());
    }
}
