//! Blocking SQLite connection setup for the media writer and independent WAL readers.
//! Opening validates the cache format and server identity before modifying existing files.
//! Recognized schemas 1 through 9 are upgraded atomically; unknown data stays untouched.
//! This module neither creates application directories nor deletes existing files.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, Transaction};

use super::album_artist_relations::initialize_album_artist_schema;
use super::album_reads::{initialize_album_read_schema, initialize_album_track_read_schema};
use super::albums::initialize_album_details_schema;
use super::artist_reads::initialize_artist_read_schema;
use super::artists::{initialize_artist_lookup_indexes, initialize_artist_schema};
use super::catalog_sections::{
    initialize_artist_section_schema, migrate_artist_section_references,
};
use super::tracks::{initialize_track_schema, migrate_section_observations};

pub(super) const DATABASE_FILE_NAME: &str = "mashin-media-cache.sqlite3";
const APPLICATION_ID: i32 = 0x4D534843;
const SCHEMA_VERSION: i32 = 16;

#[derive(Clone, Debug)]
pub(crate) enum CacheError {
    Io(Arc<std::io::Error>),
    Sqlite(Arc<rusqlite::Error>),
    InvalidServerId,
    UnsafePath(PathBuf),
    ForeignFormat,
    UnsupportedSchema(i32),
    ServerMismatch,
    ShuttingDown,
    WorkerUnavailable,
    WorkerPanicked,
    InvalidCacheOwnership(PathBuf),
    ForeignCacheFile(PathBuf),
    Json(Arc<serde_json::Error>),
    CacheAlreadyOwned,
    CacheInactive,
    Invalidated,
    SupersededObservation,
    InvalidMediaObservation(&'static str),
    InvalidImageObservation(&'static str),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Cache I/O failed: {error}"),
            Self::Sqlite(error) => write!(formatter, "Cache database SQL failed: {error}"),
            Self::InvalidServerId => formatter.write_str("Cache requires an observed server ID"),
            Self::UnsafePath(path) => write!(formatter, "Unsafe cache path: {}", path.display()),
            Self::ForeignFormat => formatter.write_str("File is not a Mashin media cache"),
            Self::UnsupportedSchema(version) => {
                write!(
                    formatter,
                    "Unsupported Mashin media cache schema: {version}"
                )
            }
            Self::ServerMismatch => formatter.write_str("Cache belongs to a different server"),
            Self::ShuttingDown => formatter.write_str("Cache database is shutting down"),
            Self::WorkerUnavailable => formatter.write_str("Cache database worker is unavailable"),
            Self::WorkerPanicked => formatter.write_str("Cache database worker operation panicked"),
            Self::InvalidCacheOwnership(path) => {
                write!(formatter, "Unverified cache ownership: {}", path.display())
            }
            Self::ForeignCacheFile(path) => {
                write!(formatter, "Unexpected file in cache: {}", path.display())
            }
            Self::Json(error) => write!(formatter, "Invalid cache metadata: {error}"),
            Self::CacheAlreadyOwned => formatter.write_str("Cache already has an active owner"),
            Self::CacheInactive => formatter.write_str("No database cache is active"),
            Self::Invalidated => {
                formatter.write_str("Cache operation belongs to an old generation")
            }
            Self::SupersededObservation => {
                formatter.write_str("Cache observation predates a confirmed mutation")
            }
            Self::InvalidMediaObservation(reason) => {
                write!(formatter, "Invalid media observation: {reason}")
            }
            Self::InvalidImageObservation(reason) => {
                write!(formatter, "Invalid image observation: {reason}")
            }
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error.as_ref()),
            Self::Sqlite(error) => Some(error.as_ref()),
            Self::Json(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CacheError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(Arc::new(error))
    }
}

impl From<rusqlite::Error> for CacheError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(Arc::new(error))
    }
}

impl From<serde_json::Error> for CacheError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(Arc::new(error))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CacheFileMetadata {
    pub(crate) server_id: String,
    pub(crate) schema_version: i32,
}

pub(crate) struct MediaDatabase {
    writer: Connection,
    path: PathBuf,
    server_id: String,
}

pub(super) struct WorkerConnections {
    pub(super) writer: Connection,
    pub(super) readers: [Connection; 2],
}

impl MediaDatabase {
    // Connection initialization and ownership.
    /// Blocking setup for the dedicated writer; the directory must already exist.
    pub(crate) fn open(cache_directory: &Path, server_id: &str) -> Result<Self, CacheError> {
        if server_id.trim().is_empty() {
            return Err(CacheError::InvalidServerId);
        }
        if cache_directory.as_os_str().is_empty() {
            return Err(CacheError::UnsafePath(cache_directory.to_owned()));
        }
        let directory = if cache_directory.is_absolute() {
            cache_directory.to_owned()
        } else {
            std::env::current_dir()?.join(cache_directory)
        };
        validate_directory(&directory)?;
        let path = directory.canonicalize()?.join(DATABASE_FILE_NAME);
        for suffix in ["", "-wal", "-shm", "-journal"] {
            validate_file(&path.with_file_name(format!("{DATABASE_FILE_NAME}{suffix}")))?;
        }
        let is_new = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error.into()),
        };
        let mut writer = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        writer.busy_timeout(Duration::from_secs(5))?;
        if is_new {
            let transaction = writer.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE cache_file_metadata (
                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                    server_id TEXT NOT NULL CHECK (length(trim(server_id)) > 0)
                ) STRICT;",
            )?;
            transaction.execute(
                "INSERT INTO cache_file_metadata (singleton, server_id) VALUES (1, ?1)",
                [server_id],
            )?;
            initialize_media_schema(&transaction)?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        } else {
            upgrade_media_schema(&mut writer, server_id)?;
        }
        validate_identity(&writer, server_id)?;
        super::artist_candidates::initialize_artist_query_functions(&writer)?;
        let transaction = writer.transaction()?;
        initialize_artist_lookup_indexes(&transaction)?;
        initialize_artist_query_indexes(&transaction)?;
        transaction.commit()?;
        writer.pragma_update(None, "foreign_keys", true)?;
        writer.pragma_update(None, "locking_mode", "NORMAL")?;
        let journal_mode: String =
            writer.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(CacheError::ForeignFormat);
        }
        writer.pragma_update(None, "synchronous", "FULL")?;
        Ok(Self {
            writer,
            path,
            server_id: server_id.to_owned(),
        })
    }

    /// Closes the writer explicitly so lifecycle owners can surface close failures.
    pub(crate) fn shutdown(self) -> Result<(), CacheError> {
        self.writer.close().map_err(|(_, error)| error.into())
    }

    /// Transfers exclusive connection ownership to the writer and two reader workers.
    pub(super) fn into_worker_connections(self) -> Result<WorkerConnections, CacheError> {
        let readers = [self.open_reader()?, self.open_reader()?];
        Ok(WorkerConnections {
            writer: self.writer,
            readers,
        })
    }

    // Independent reads and file metadata.
    /// Blocking reader setup; its owning worker must release it before cache clearing.
    pub(crate) fn open_reader(&self) -> Result<Connection, CacheError> {
        validate_file(&self.path)?;
        let reader = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        reader.busy_timeout(Duration::from_secs(5))?;
        reader.pragma_update(None, "query_only", true)?;
        reader.pragma_update(None, "foreign_keys", true)?;
        validate_identity(&reader, &self.server_id)?;
        super::artist_candidates::initialize_artist_query_functions(&reader)?;
        Ok(reader)
    }

    pub(crate) fn metadata(&self) -> Result<CacheFileMetadata, CacheError> {
        read_metadata(&self.writer)
    }
}

// Schema construction and format validation.
pub(super) fn initialize_media_schema(transaction: &Transaction<'_>) -> Result<(), CacheError> {
    super::artist_candidates::initialize_artist_query_functions(transaction)?;
    initialize_artist_schema(transaction)?;
    initialize_album_artist_schema(transaction)?;
    initialize_album_details_schema(transaction)?;
    initialize_track_schema(transaction)?;
    initialize_artist_read_schema(transaction)?;
    initialize_artist_section_schema(transaction)?;
    initialize_album_read_schema(transaction)?;
    initialize_album_track_read_schema(transaction)?;
    super::track_reads::initialize_track_read_schema(transaction)?;
    super::track_reads::initialize_track_list_schema(transaction)?;
    super::playlists::initialize_playlist_schema(transaction)?;
    super::playlist_reads::initialize_playlist_read_schema(transaction)?;
    super::recommendation_reads::initialize_recommendation_schema(transaction)?;
    initialize_artist_query_indexes(transaction)?;
    super::album_candidates::initialize_album_query_indexes(transaction)?;
    Ok(())
}

fn initialize_artist_query_indexes(transaction: &Transaction<'_>) -> Result<(), CacheError> {
    transaction.execute_batch("CREATE INDEX IF NOT EXISTS artists_search_name_idx ON artists(mashin_artist_search_key(name),provider,item_id);
        CREATE INDEX IF NOT EXISTS artists_search_sort_name_idx ON artists(mashin_artist_search_key(sort_name),provider,item_id);
        CREATE INDEX IF NOT EXISTS artist_read_items_identity_idx ON artist_read_items(user_id,provider,item_id,query);
        CREATE INDEX IF NOT EXISTS artist_album_read_items_identity_idx ON artist_album_read_items(user_id,provider,item_id);")?;
    Ok(())
}

/// Extends recognized older schemas only after server and object preflight; failure rolls back.
fn upgrade_media_schema(connection: &mut Connection, server_id: &str) -> Result<(), CacheError> {
    let application_id: i32 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(CacheError::ForeignFormat);
    }
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if !(1..=15).contains(&version) {
        return Ok(());
    }
    if read_metadata(connection)?.server_id != server_id {
        return Err(CacheError::ServerMismatch);
    }
    let allowed_objects = if version == 1 {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name != 'cache_file_metadata'"
    } else if version == 2 {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name NOT IN (
            'cache_file_metadata','artists','artist_provider_mappings','artist_external_ids','users',
            'artist_favorites','artist_library_entries','artists_sort_name_idx','artist_provider_lookup_idx',
            'artist_external_lookup_idx','artist_favorite_lookup_idx','artist_domain_lookup_idx',
            'artist_external_nocase_lookup_idx','artist_external_value_lookup_idx')"
    } else if version == 3 {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name NOT IN (
            'cache_file_metadata','artists','artist_provider_mappings','artist_external_ids','users',
            'artist_favorites','artist_library_entries','artists_sort_name_idx','artist_provider_lookup_idx',
            'artist_external_lookup_idx','artist_favorite_lookup_idx','artist_domain_lookup_idx',
            'artist_external_nocase_lookup_idx','artist_external_value_lookup_idx','albums','album_artists','album_provider_mappings',
            'album_artist_lookup_idx','album_membership_lookup_idx')"
    } else if version == 4 {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name NOT IN (
            'cache_file_metadata','artists','artist_provider_mappings','artist_external_ids','users',
            'artist_favorites','artist_library_entries','artists_sort_name_idx','artist_provider_lookup_idx',
            'artist_external_lookup_idx','artist_favorite_lookup_idx','artist_domain_lookup_idx',
            'artist_external_nocase_lookup_idx','artist_external_value_lookup_idx','albums','album_artists','album_provider_mappings',
            'album_artist_lookup_idx','album_membership_lookup_idx','artist_read_states','artist_read_items')"
    } else if version == 5 {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name NOT IN (
            'cache_file_metadata','artists','artist_provider_mappings','artist_external_ids','users',
            'artist_favorites','artist_library_entries','artists_sort_name_idx','artist_provider_lookup_idx',
            'artist_external_lookup_idx','artist_favorite_lookup_idx','artist_domain_lookup_idx',
            'artist_external_nocase_lookup_idx','artist_external_value_lookup_idx','albums','album_artists','album_provider_mappings',
            'album_artist_lookup_idx','album_membership_lookup_idx','artist_read_states','artist_read_items',
            'artist_album_read_items','artist_track_read_items','artist_album_read_credits','artist_track_read_credits')"
    } else if version == 6 {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name NOT IN (
            'cache_file_metadata','artists','artist_provider_mappings','artist_external_ids','users',
            'artist_favorites','artist_library_entries','artists_sort_name_idx','artist_provider_lookup_idx',
            'artist_external_lookup_idx','artist_favorite_lookup_idx','artist_domain_lookup_idx',
            'artist_external_nocase_lookup_idx','artist_external_value_lookup_idx','albums','album_artists','album_provider_mappings',
            'album_artist_lookup_idx','album_membership_lookup_idx','artist_read_states','artist_read_items',
            'artist_album_read_items','artist_track_read_items','artist_album_read_credits','artist_track_read_credits',
            'album_external_ids','album_favorites','albums_sort_name_idx','albums_year_idx','album_provider_lookup_idx','album_external_lookup_idx')"
    } else if version >= 8 {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND NOT (
            (type='table' AND name IN ('cache_file_metadata','artists','artist_provider_mappings','artist_external_ids','users',
            'artist_favorites','artist_library_entries','albums','album_artists','album_provider_mappings','artist_read_states','artist_read_items',
            'artist_album_read_items','artist_track_read_items','album_external_ids','album_favorites',
            'tracks','track_artists','album_tracks','track_provider_mappings','track_external_ids','track_favorites'))
            OR (type='index' AND name IN ('artists_sort_name_idx','artist_provider_lookup_idx','artist_external_lookup_idx',
            'artist_favorite_lookup_idx','artist_domain_lookup_idx','artist_external_nocase_lookup_idx','artist_external_value_lookup_idx',
            'album_artist_lookup_idx','album_membership_lookup_idx','albums_sort_name_idx','albums_year_idx','album_provider_lookup_idx','album_external_lookup_idx',
            'tracks_sort_name_idx','track_artist_lookup_idx','album_track_order_idx','track_album_lookup_idx','track_provider_lookup_idx','track_external_lookup_idx',
            'artists_search_name_idx','artists_search_sort_name_idx','artist_read_items_identity_idx','artist_album_read_items_identity_idx'))
            OR (?1>=9 AND type='table' AND name IN ('album_read_states','album_read_items'))
            OR (?1>=9 AND type='index' AND name IN ('album_read_items_identity_idx','album_domain_lookup_idx'))
            OR (?1>=10 AND type='table' AND name='album_track_read_items')
            OR (?1>=11 AND type='index' AND name IN ('albums_candidate_name_idx','albums_candidate_sort_name_idx','albums_candidate_year_idx'))
            OR (?1>=12 AND type='table' AND name='track_read_states')
            OR (?1>=12 AND type='index' AND name IN ('track_read_states_identity_idx','track_domain_lookup_idx'))
            OR (?1=13 AND type='table' AND name IN ('track_search_states','track_search_items'))
            OR (?1=13 AND type='index' AND name='track_search_items_identity_idx')
            OR (?1>=14 AND type='table' AND name IN ('track_list_states','track_list_items'))
            OR (?1>=14 AND type='index' AND name='track_list_items_identity_idx')
            OR (?1>=15 AND type='table' AND name IN ('playlists','playlist_provider_mappings','playlist_external_ids','playlist_favorites','playlist_read_states','playlist_read_items'))
            OR (?1>=15 AND type='index' AND name IN ('playlists_sort_name_idx','playlist_provider_lookup_idx','playlist_domain_lookup_idx','playlist_external_lookup_idx','playlist_read_items_identity_idx')))"
    } else {
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND name NOT IN (
            'cache_file_metadata','artists','artist_provider_mappings','artist_external_ids','users',
            'artist_favorites','artist_library_entries','artists_sort_name_idx','artist_provider_lookup_idx',
            'artist_external_lookup_idx','artist_favorite_lookup_idx','artist_domain_lookup_idx',
            'artist_external_nocase_lookup_idx','artist_external_value_lookup_idx','albums','album_artists','album_provider_mappings',
            'album_artist_lookup_idx','album_membership_lookup_idx','artist_read_states','artist_read_items',
            'artist_album_read_items','artist_track_read_items','artist_album_read_credits','artist_track_read_credits',
            'album_external_ids','album_favorites','albums_sort_name_idx','albums_year_idx','album_provider_lookup_idx','album_external_lookup_idx',
            'tracks','track_artists','album_tracks','track_provider_mappings','track_external_ids','track_favorites',
            'tracks_sort_name_idx','track_artist_lookup_idx','album_track_order_idx','track_album_lookup_idx','track_provider_lookup_idx','track_external_lookup_idx')"
    };
    let other_objects: i64 = if version >= 8 {
        connection.query_row(allowed_objects, [version], |row| row.get(0))?
    } else {
        connection.query_row(allowed_objects, [], |row| row.get(0))?
    };
    if other_objects != 0 {
        return Err(CacheError::ForeignFormat);
    }
    if version >= 2 {
        let tables: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN (
                'cache_file_metadata','artists','artist_provider_mappings','artist_external_ids',
                'users','artist_favorites','artist_library_entries')",
            [],
            |row| row.get(0),
        )?;
        if tables != 7 {
            return Err(CacheError::ForeignFormat);
        }
        for query in [
            "SELECT provider,item_id,name,version,sort_name,sort_name_known,uri,uri_known,is_playable,
                available,available_known,metadata,metadata_known,position,position_known,date_added,
                date_added_known,artist_type,artist_type_known,provider_mappings_known,external_ids_known
             FROM artists LIMIT 0",
            "SELECT artist_provider,artist_item_id,position,provider_domain,provider_instance,item_id,
                available,in_library,in_library_known,is_unique,is_unique_known,audio_format,
                audio_format_known,url,url_known,details,details_known FROM artist_provider_mappings LIMIT 0",
            "SELECT artist_provider,artist_item_id,position,id_type,external_id FROM artist_external_ids LIMIT 0",
            "SELECT user_id,provider,item_id,rating FROM artist_favorites LIMIT 0",
            "SELECT provider,item_id,is_in_library FROM artist_library_entries LIMIT 0",
            "SELECT user_id FROM users LIMIT 0",
        ] {
            connection.prepare(query).map_err(|_|CacheError::ForeignFormat)?;
        }
        let keys = connection
            .prepare("SELECT name FROM pragma_table_info('artists') WHERE pk>0 ORDER BY pk")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if keys != ["provider", "item_id"] {
            return Err(CacheError::ForeignFormat);
        }
    }
    if version >= 3 {
        for query in [
            "SELECT provider,item_id,name,artists_known,provider_mappings_known FROM albums LIMIT 0",
            "SELECT album_provider,album_item_id,position,artist_provider,artist_item_id FROM album_artists LIMIT 0",
            "SELECT album_provider,album_item_id,position,provider_domain,provider_instance,item_id,
                in_library,in_library_known FROM album_provider_mappings LIMIT 0",
        ] {
            connection.prepare(query).map_err(|_|CacheError::ForeignFormat)?;
        }
    }
    if version >= 4 {
        for query in [
            "SELECT user_id,query,observed_at FROM artist_read_states LIMIT 0",
            "SELECT user_id,query,position,provider,item_id FROM artist_read_items LIMIT 0",
        ] {
            connection
                .prepare(query)
                .map_err(|_| CacheError::ForeignFormat)?;
        }
        for (table, expected) in [
            ("artist_read_states", vec!["user_id", "query"]),
            ("artist_read_items", vec!["user_id", "query", "position"]),
        ] {
            let keys = connection
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{table}') WHERE pk>0 ORDER BY pk"
                ))?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if keys != expected {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if (5..=7).contains(&version) {
        for query in [
            "SELECT user_id,query,occurrence,provider,item_id,name,sort_name,sort_name_known,year,year_known,
                version,uri,favorite,external_ids,is_playable,available,provider_mappings,metadata,position,date_added,artists_known,album_type
             FROM artist_album_read_items LIMIT 0",
            "SELECT user_id,query,occurrence,provider,item_id,name,sort_name,sort_name_known,duration,duration_known,
                version,uri,favorite,external_ids,is_playable,available,provider_mappings,metadata,position,date_added,artists_known,
                album,image,disc_number,track_number FROM artist_track_read_items LIMIT 0",
            "SELECT user_id,query,occurrence,position,artist_provider,artist_item_id FROM artist_album_read_credits LIMIT 0",
            "SELECT user_id,query,occurrence,position,artist_provider,artist_item_id FROM artist_track_read_credits LIMIT 0",
        ] { connection.prepare(query).map_err(|_| CacheError::ForeignFormat)?; }
        for (table, expected) in [
            (
                "artist_album_read_items",
                vec!["user_id", "query", "occurrence"],
            ),
            (
                "artist_track_read_items",
                vec!["user_id", "query", "occurrence"],
            ),
            (
                "artist_album_read_credits",
                vec!["user_id", "query", "occurrence", "position"],
            ),
            (
                "artist_track_read_credits",
                vec!["user_id", "query", "occurrence", "position"],
            ),
        ] {
            let keys = connection
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{table}') WHERE pk>0 ORDER BY pk"
                ))?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if keys != expected {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if version >= 6 {
        for query in [
            "SELECT version,sort_name,sort_name_known,uri,uri_known,is_playable,available,available_known,
                metadata,metadata_known,position,position_known,date_added,date_added_known,year,year_known,album_type,external_ids_known FROM albums LIMIT 0",
            "SELECT available,is_unique,is_unique_known,audio_format,audio_format_known,url,url_known,details,details_known FROM album_provider_mappings LIMIT 0",
            "SELECT album_provider,album_item_id,position,id_type,external_id FROM album_external_ids LIMIT 0",
            "SELECT user_id,provider,item_id,rating FROM album_favorites LIMIT 0",
        ] {connection.prepare(query).map_err(|_|CacheError::ForeignFormat)?;}
        for (table, expected) in [
            ("albums", vec!["provider", "item_id"]),
            (
                "album_artists",
                vec!["album_provider", "album_item_id", "position"],
            ),
            (
                "album_provider_mappings",
                vec!["album_provider", "album_item_id", "position"],
            ),
            (
                "album_external_ids",
                vec!["album_provider", "album_item_id", "position"],
            ),
            ("album_favorites", vec!["user_id", "provider", "item_id"]),
        ] {
            let keys = connection
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{table}') WHERE pk>0 ORDER BY pk"
                ))?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if keys != expected {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if version >= 7 {
        for query in [
            "SELECT provider,item_id,name,version,sort_name,sort_name_known,uri,uri_known,is_playable,available,available_known,
                metadata,metadata_known,position,position_known,date_added,date_added_known,duration,duration_known,artists_known,
                album_provider,album_item_id,album_known,image,image_known,disc_number,disc_number_known,track_number,track_number_known,
                external_ids_known,provider_mappings_known FROM tracks LIMIT 0",
            "SELECT track_provider,track_item_id,position,artist_provider,artist_item_id FROM track_artists LIMIT 0",
            "SELECT album_provider,album_item_id,track_provider,track_item_id,disc_number,disc_number_known,track_number,track_number_known FROM album_tracks LIMIT 0",
            "SELECT track_provider,track_item_id,position,provider_domain,provider_instance,item_id,available,
                in_library,in_library_known,is_unique,is_unique_known,audio_format,audio_format_known,url,url_known,details,details_known FROM track_provider_mappings LIMIT 0",
            "SELECT track_provider,track_item_id,position,id_type,external_id FROM track_external_ids LIMIT 0",
            "SELECT user_id,provider,item_id,rating FROM track_favorites LIMIT 0",
        ] {connection.prepare(query).map_err(|_|CacheError::ForeignFormat)?;}
        for (table, expected) in [
            ("tracks", vec!["provider", "item_id"]),
            (
                "track_artists",
                vec!["track_provider", "track_item_id", "position"],
            ),
            (
                "album_tracks",
                vec![
                    "album_provider",
                    "album_item_id",
                    "track_provider",
                    "track_item_id",
                ],
            ),
            (
                "track_provider_mappings",
                vec!["track_provider", "track_item_id", "position"],
            ),
            (
                "track_external_ids",
                vec!["track_provider", "track_item_id", "position"],
            ),
            ("track_favorites", vec!["user_id", "provider", "item_id"]),
        ] {
            let keys = connection
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{table}') WHERE pk>0 ORDER BY pk"
                ))?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if keys != expected {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if version >= 8 {
        let tables: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name NOT GLOB 'sqlite_*'",
            [],
            |row| row.get(0),
        )?;
        if tables
            != match version {
                8 => 22,
                9 => 24,
                12 => 26,
                13 | 14 => 28,
                15 => 34,
                _ => 25,
            }
        {
            return Err(CacheError::ForeignFormat);
        }
        for (table, columns, foreign_keys, query) in [
            ("artist_album_read_items", 7, 4, "SELECT user_id,query,occurrence,provider,item_id,position,position_known FROM artist_album_read_items LIMIT 0"),
            ("artist_track_read_items", 12, 8, "SELECT user_id,query,occurrence,provider,item_id,album_provider,album_item_id,album_known,disc_number,track_number,position,position_known FROM artist_track_read_items LIMIT 0"),
        ] {
            connection.prepare(query).map_err(|_| CacheError::ForeignFormat)?;
            let count: i64 = connection.query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}')"), [], |row|row.get(0))?;
            let keys = connection.prepare(&format!("SELECT name FROM pragma_table_info('{table}') WHERE pk>0 ORDER BY pk"))?
                .query_map([],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let relations: i64 = connection.query_row(&format!("SELECT COUNT(*) FROM pragma_foreign_key_list('{table}')"),[],|row|row.get(0))?;
            if count != columns || keys != ["user_id","query","occurrence"] || relations != foreign_keys { return Err(CacheError::ForeignFormat); }
        }
    }
    if version >= 9 {
        for (table, columns, foreign_keys, keys, query) in [
            (
                "album_read_states",
                3,
                1,
                vec!["user_id", "query"],
                "SELECT user_id,query,observed_at FROM album_read_states LIMIT 0",
            ),
            (
                "album_read_items",
                5,
                4,
                vec!["user_id", "query", "position"],
                "SELECT user_id,query,position,provider,item_id FROM album_read_items LIMIT 0",
            ),
        ] {
            connection
                .prepare(query)
                .map_err(|_| CacheError::ForeignFormat)?;
            let count: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}')"),
                [],
                |row| row.get(0),
            )?;
            let actual_keys = connection
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{table}') WHERE pk>0 ORDER BY pk"
                ))?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let relations: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM pragma_foreign_key_list('{table}')"),
                [],
                |row| row.get(0),
            )?;
            if count != columns || actual_keys != keys || relations != foreign_keys {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if version >= 10 {
        connection.prepare("SELECT user_id,query,occurrence,provider,item_id,album_provider,album_item_id,album_known,disc_number,track_number,position,position_known FROM album_track_read_items LIMIT 0")
            .map_err(|_|CacheError::ForeignFormat)?;
        let columns: i64 = connection.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('album_track_read_items')",
            [],
            |row| row.get(0),
        )?;
        let keys=connection.prepare("SELECT name FROM pragma_table_info('album_track_read_items') WHERE pk>0 ORDER BY pk")?
            .query_map([],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let relations: i64 = connection.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_list('album_track_read_items')",
            [],
            |row| row.get(0),
        )?;
        if columns != 12 || keys != ["user_id", "query", "occurrence"] || relations != 8 {
            return Err(CacheError::ForeignFormat);
        }
    }
    if version >= 11 {
        for (index, column) in [
            ("albums_candidate_name_idx", None),
            ("albums_candidate_sort_name_idx", None),
            ("albums_candidate_year_idx", Some("year")),
        ] {
            let owners: i64 = connection.query_row(
                "SELECT COUNT(*) FROM pragma_index_list('albums') WHERE name=?1 AND origin='c' AND partial=0",
                [index], |row| row.get(0),
            )?;
            let columns = connection
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?
                .query_map([index], |row| row.get::<_, Option<String>>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if owners != 1
                || columns
                    != [
                        Some("provider".into()),
                        column.map(str::to_owned),
                        Some("item_id".into()),
                    ]
            {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if version >= 12 {
        connection.prepare("SELECT user_id,query,observed_at,provider,item_id,album_provider,album_item_id,album_known,disc_number,track_number,position,position_known,image,image_known FROM track_read_states LIMIT 0").map_err(|_|CacheError::ForeignFormat)?;
        let columns: i64 = connection.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('track_read_states')",
            [],
            |row| row.get(0),
        )?;
        let keys = connection
            .prepare(
                "SELECT name FROM pragma_table_info('track_read_states') WHERE pk>0 ORDER BY pk",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let relations: i64 = connection.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_list('track_read_states')",
            [],
            |row| row.get(0),
        )?;
        if columns != 14 || keys != ["user_id", "query"] || relations != 7 {
            return Err(CacheError::ForeignFormat);
        }
        for (table, index, expected) in [
            (
                "track_read_states",
                "track_read_states_identity_idx",
                ["user_id", "provider", "item_id", "query"],
            ),
            (
                "track_provider_mappings",
                "track_domain_lookup_idx",
                [
                    "provider_domain",
                    "item_id",
                    "track_provider",
                    "track_item_id",
                ],
            ),
        ] {
            let owners:i64 = connection.query_row("SELECT COUNT(*) FROM pragma_index_list(?1) WHERE name=?2 AND origin='c' AND partial=0",rusqlite::params![table,index],|row|row.get(0))?;
            let columns = connection
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?
                .query_map([index], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if owners != 1 || columns != expected {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if version >= 13 {
        let (states, items, index) = if version == 13 {
            (
                "track_search_states",
                "track_search_items",
                "track_search_items_identity_idx",
            )
        } else {
            (
                "track_list_states",
                "track_list_items",
                "track_list_items_identity_idx",
            )
        };
        for (table,columns,foreign_keys,keys,query) in [
            (states,3,1,vec!["user_id","query"],format!("SELECT user_id,query,observed_at FROM {states} LIMIT 0")),
            (items,14,8,vec!["user_id","query","occurrence"],format!("SELECT user_id,query,occurrence,provider,item_id,album_provider,album_item_id,album_known,disc_number,track_number,position,position_known,image,image_known FROM {items} LIMIT 0")),
        ] {
            connection.prepare(&query).map_err(|_|CacheError::ForeignFormat)?;
            let count:i64 = connection.query_row("SELECT COUNT(*) FROM pragma_table_info(?1)",[table],|row|row.get(0))?;
            let actual_keys = connection.prepare("SELECT name FROM pragma_table_info(?1) WHERE pk>0 ORDER BY pk")?.query_map([table],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let relations:i64 = connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_list(?1)",[table],|row|row.get(0))?;
            if count!=columns || actual_keys!=keys || relations!=foreign_keys { return Err(CacheError::ForeignFormat); }
        }
        let owners: i64 = connection.query_row(
            "SELECT COUNT(*) FROM pragma_index_list(?1) WHERE name=?2 AND origin='c' AND partial=0",
            rusqlite::params![items, index],
            |row| row.get(0),
        )?;
        let columns = connection
            .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?
            .query_map([index], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if owners != 1 || columns != ["user_id", "provider", "item_id", "query"] {
            return Err(CacheError::ForeignFormat);
        }
    }
    if version >= 15 {
        for (table,columns,relations,keys,query) in [
            ("playlists",16,0,vec!["provider","item_id"],"SELECT provider,item_id,name,version,sort_name,uri,is_playable,available,metadata,position,date_added,owner,is_editable,is_dynamic,access,known_fields FROM playlists LIMIT 0"),
            ("playlist_provider_mappings",13,2,vec!["provider","playlist_id","position"],"SELECT provider,playlist_id,position,provider_domain,provider_instance,item_id,available,in_library,is_unique,audio_format,url,details,known_fields FROM playlist_provider_mappings LIMIT 0"),
            ("playlist_external_ids",5,2,vec!["provider","playlist_id","position"],"SELECT provider,playlist_id,position,id_type,external_id FROM playlist_external_ids LIMIT 0"),
            ("playlist_favorites",4,3,vec!["user_id","provider","item_id"],"SELECT user_id,provider,item_id,rating FROM playlist_favorites LIMIT 0"),
            ("playlist_read_states",3,1,vec!["user_id","query"],"SELECT user_id,query,observed_at FROM playlist_read_states LIMIT 0"),
            ("playlist_read_items",7,4,vec!["user_id","query","occurrence"],"SELECT user_id,query,occurrence,provider,item_id,position,position_known FROM playlist_read_items LIMIT 0"),
        ] {
            connection.prepare(query).map_err(|_|CacheError::ForeignFormat)?;
            let count:i64=connection.query_row("SELECT COUNT(*) FROM pragma_table_info(?1)",[table],|row|row.get(0))?;
            let foreign_keys:i64=connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_list(?1)",[table],|row|row.get(0))?;
            let primary=connection.prepare("SELECT name FROM pragma_table_info(?1) WHERE pk>0 ORDER BY pk")?.query_map([table],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            if count!=columns || foreign_keys!=relations || primary!=keys { return Err(CacheError::ForeignFormat) }
        }
        for (table, index, expected) in [
            (
                "playlists",
                "playlists_sort_name_idx",
                vec!["sort_name", "provider", "item_id"],
            ),
            (
                "playlist_provider_mappings",
                "playlist_provider_lookup_idx",
                vec!["provider_instance", "item_id", "provider", "playlist_id"],
            ),
            (
                "playlist_provider_mappings",
                "playlist_domain_lookup_idx",
                vec!["provider_domain", "item_id", "provider", "playlist_id"],
            ),
            (
                "playlist_external_ids",
                "playlist_external_lookup_idx",
                vec!["id_type", "external_id", "provider", "playlist_id"],
            ),
            (
                "playlist_read_items",
                "playlist_read_items_identity_idx",
                vec!["user_id", "provider", "item_id", "query"],
            ),
        ] {
            let owners:i64=connection.query_row("SELECT COUNT(*) FROM pragma_index_list(?1) WHERE name=?2 AND origin='c' AND partial=0",rusqlite::params![table,index],|row|row.get(0))?;
            let columns = connection
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?
                .query_map([index], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if owners != 1 || columns != expected {
                return Err(CacheError::ForeignFormat);
            }
        }
    }
    if connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
        row.get::<_, i64>(0)
    })? != 0
    {
        return Err(CacheError::ForeignFormat);
    }
    super::artist_candidates::initialize_artist_query_functions(connection)?;
    let transaction = connection.transaction()?;
    if version == 1 {
        initialize_media_schema(&transaction)?;
    } else {
        if version == 2 {
            initialize_album_artist_schema(&transaction)?;
        }
        if version < 4 {
            initialize_artist_read_schema(&transaction)?;
        }
        if version < 6 {
            initialize_album_details_schema(&transaction)?;
        }
        if version < 7 {
            initialize_track_schema(&transaction)?;
        }
        if (5..=7).contains(&version) {
            if version < 7 {
                migrate_section_observations(&transaction, version == 6)?;
            }
            migrate_artist_section_references(&transaction)?;
        } else if version < 5 {
            initialize_artist_section_schema(&transaction)?;
        }
        if version < 9 {
            initialize_album_read_schema(&transaction)?;
        }
        if version < 10 {
            initialize_album_track_read_schema(&transaction)?;
        }
        if version < 11 {
            super::album_candidates::initialize_album_query_indexes(&transaction)?;
        }
        if version < 12 {
            super::track_reads::initialize_track_read_schema(&transaction)?;
        }
        if version == 13 {
            transaction.execute_batch("ALTER TABLE track_search_states RENAME TO track_list_states;
                ALTER TABLE track_search_items RENAME TO track_list_items;
                DROP INDEX track_search_items_identity_idx;
                CREATE INDEX track_list_items_identity_idx ON track_list_items(user_id,provider,item_id,query);")?;
        } else if version < 13 {
            super::track_reads::initialize_track_list_schema(&transaction)?;
        }
        if version < 15 {
            super::playlists::initialize_playlist_schema(&transaction)?;
            super::playlist_reads::initialize_playlist_read_schema(&transaction)?;
        }
        super::recommendation_reads::initialize_recommendation_schema(&transaction)?;
    }
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn read_metadata(connection: &Connection) -> Result<CacheFileMetadata, CacheError> {
    let schema_version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let server_id = connection.query_row(
        "SELECT server_id FROM cache_file_metadata WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(CacheFileMetadata {
        server_id,
        schema_version,
    })
}

fn validate_identity(connection: &Connection, server_id: &str) -> Result<(), CacheError> {
    let application_id: i32 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(CacheError::ForeignFormat);
    }
    let schema_version: i32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if schema_version != SCHEMA_VERSION {
        return Err(CacheError::UnsupportedSchema(schema_version));
    }
    if read_metadata(connection)?.server_id != server_id {
        return Err(CacheError::ServerMismatch);
    }
    Ok(())
}

pub(super) fn validate_directory(path: &Path) -> Result<(), CacheError> {
    for ancestor in path
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
    {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if is_redirect(&metadata) || !metadata.is_dir() {
            return Err(CacheError::UnsafePath(ancestor.to_owned()));
        }
    }
    Ok(())
}

pub(super) fn validate_file(path: &Path) -> Result<(), CacheError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if is_redirect(&metadata) || !metadata.is_file() => {
            Err(CacheError::UnsafePath(path.to_owned()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn is_redirect(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;

    // Historical fixtures are built with their original schemas, not downgraded current tables.
    fn create_schema_five_database(path: &Path) {
        let mut connection = Connection::open(path).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch("CREATE TABLE cache_file_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),server_id TEXT NOT NULL) STRICT; INSERT INTO cache_file_metadata VALUES(1,'server');").unwrap();
        initialize_artist_schema(&transaction).unwrap();
        initialize_album_artist_schema(&transaction).unwrap();
        initialize_artist_read_schema(&transaction).unwrap();
        super::super::catalog_sections::initialize_legacy_artist_section_schema(&transaction)
            .unwrap();
        transaction
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        transaction.pragma_update(None, "user_version", 5).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
    }

    fn seed_schema_five_album_sections(path: &Path) {
        use crate::domain::catalog::CatalogQuery;
        use crate::domain::identifiers::MediaId;
        let mut connection = Connection::open(path).unwrap();
        let transaction = connection.transaction().unwrap();
        let query = serde_json::to_string(&CatalogQuery::ArtistAlbums {
            item_id: MediaId("artist".into()),
            provider: "source-a".into(),
        })
        .unwrap();
        let tracks = serde_json::to_string(&CatalogQuery::ArtistTopTracks {
            item_id: MediaId("artist".into()),
            provider: "source-a".into(),
        })
        .unwrap();
        transaction
            .execute_batch("INSERT INTO users(user_id) VALUES('user-a'),('user-b');")
            .unwrap();
        for (user, time) in [("user-a", 100), ("user-b", 200)] {
            transaction
                .execute(
                    "INSERT INTO artist_read_states(user_id,query,observed_at) VALUES(?1,?2,?3)",
                    rusqlite::params![user, query, time],
                )
                .unwrap();
            transaction.execute("INSERT INTO artist_album_read_items(user_id,query,occurrence,provider,item_id,name,sort_name_known,year,year_known,artists_known,metadata,favorite,provider_mappings)
                VALUES(?1,?2,0,'source-a','007','Album',0,?3,?4,0,?5,?6,?7)",rusqlite::params![user,query,
                    (user=="user-a").then_some(2024),user=="user-a",
                    (user=="user-a").then_some("{\"description\":\"Keep\"}"),
                    if user=="user-a" {"true"} else {"false"},
                    (user=="user-a").then_some("[{\"provider_domain\":\"source\",\"provider_instance\":\"source-a\",\"item_id\":\"007\",\"in_library\":null}]")]).unwrap();
        }
        transaction.execute("INSERT INTO artist_album_read_items(user_id,query,occurrence,provider,item_id,name,sort_name_known,year,year_known,artists_known)
            VALUES('user-a',?1,1,'source-b','007','Other provider',0,1999,1,0),('user-a',?1,2,'source-a','007','Album',0,NULL,0,0)",[&query]).unwrap();
        transaction
            .execute(
                "INSERT INTO artist_read_states(user_id,query,observed_at) VALUES('user-a',?1,150)",
                [&tracks],
            )
            .unwrap();
        transaction.execute("INSERT INTO artist_track_read_items(user_id,query,occurrence,provider,item_id,name,sort_name_known,duration_known,artists_known,album)
            VALUES('user-a',?1,0,'source-a','track','Track',0,0,0,?2)",rusqlite::params![tracks,"{\"provider\":\"source-c\",\"item_id\":\"007\",\"name\":\"Embedded\",\"year\":null,\"artists\":[]}"]).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
    }

    fn create_schema_six_database(path: &Path) {
        use crate::domain::identifiers::UserId;
        create_schema_five_database(path);
        seed_schema_five_album_sections(path);
        let mut connection = Connection::open(path).unwrap();
        let transaction = connection.transaction().unwrap();
        initialize_album_details_schema(&transaction).unwrap();
        for (user, value) in [
            (
                "user-a",
                serde_json::json!({"provider":"source-a","item_id":"007","name":"Album","year":2024,"favorite":true}),
            ),
            (
                "user-b",
                serde_json::json!({"provider":"source-a","item_id":"007","name":"Album","favorite":false}),
            ),
            (
                "user-a",
                serde_json::json!({"provider":"source-b","item_id":"007","name":"Other provider","year":1999}),
            ),
            (
                "user-a",
                serde_json::json!({"provider":"source-c","item_id":"007","name":"Latest Album","year":2025,"metadata":{"description":"Latest"}}),
            ),
        ] {
            super::super::albums::write_album_observation(
                &transaction,
                &serde_json::from_value(value).unwrap(),
                &UserId(user.into()),
            )
            .unwrap();
        }
        transaction.execute_batch("UPDATE artist_track_read_items SET duration=125,duration_known=1,favorite='true',image='null',metadata='{\"description\":\"Track detail\"}',
            album='{\"provider\":\"source-c\",\"item_id\":\"007\",\"name\":\"Old Album\",\"year\":2020,\"metadata\":{\"description\":\"Old\"}}';
            INSERT INTO artist_read_states(user_id,query,observed_at) SELECT 'user-b',query,250 FROM artist_track_read_items WHERE occurrence=0;
            INSERT INTO artist_track_read_items(user_id,query,occurrence,provider,item_id,name,sort_name_known,duration,duration_known,artists_known,favorite)
                SELECT 'user-b',query,0,'source-b',item_id,'Other track',0,73,1,0,'false' FROM artist_track_read_items WHERE user_id='user-a' AND occurrence=0;").unwrap();
        transaction.pragma_update(None, "user_version", 6).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
    }

    fn create_schema_seven_database(path: &Path) {
        use crate::domain::catalog::CatalogQuery;
        use crate::domain::identifiers::{MediaId, UserId};
        create_schema_six_database(path);
        let mut connection = Connection::open(path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        initialize_track_schema(&transaction).unwrap();
        migrate_section_observations(&transaction, true).unwrap();
        let user = UserId("user-a".into());
        for (provider, name, occurrence, disc, number) in
            [("album-a", "A", 3, 1, 4), ("album-b", "B", 7, 2, 9)]
        {
            let track=serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"track","name":"Intermediate",
                "album":{"provider":provider,"item_id":"007","name":name,"year":2026},"disc_number":disc,"track_number":number})).unwrap();
            super::super::tracks::write_track_observation(&transaction, &track, &user).unwrap();
            let album=serde_json::to_string(&serde_json::json!({"provider":provider,"item_id":"007","name":"Old copied album","year":1999})).unwrap();
            transaction.execute("INSERT INTO artist_track_read_items(user_id,query,occurrence,provider,item_id,name,sort_name_known,duration,duration_known,artists_known,album,disc_number,track_number,position)
                SELECT user_id,query,?1,provider,item_id,'Old copied track',0,125,1,0,?2,?3,?4,?5 FROM artist_track_read_items WHERE user_id='user-a' AND occurrence=0",
                rusqlite::params![occurrence,album,disc.to_string(),number.to_string(),if occurrence==3 {"null"} else {"17"}]).unwrap();
        }
        let latest=serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"track","name":"Latest track","duration":321,"favorite":false,
            "metadata":{"description":"Latest track metadata"},"artists":[{"provider":"artist-source","item_id":"artist","name":"Latest artist"}],
            "album":{"provider":"source-c","item_id":"007","name":"Latest Album","year":2027},"disc_number":8,"track_number":20})).unwrap();
        super::super::tracks::write_track_observation(&transaction, &latest, &user).unwrap();
        let empty = serde_json::to_string(&CatalogQuery::ArtistTracks {
            provider: "source-a".into(),
            item_id: MediaId("empty".into()),
        })
        .unwrap();
        transaction
            .execute(
                "INSERT INTO artist_read_states(user_id,query,observed_at) VALUES('user-a',?1,333)",
                [empty],
            )
            .unwrap();
        transaction.pragma_update(None, "user_version", 7).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
    }

    // Reference replacement preserves current canonical data, exact occurrence keys and failures.
    #[test]
    fn artist_section_references_schema_seven_preserves_current_media_and_context_after_reopen() {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::{MediaId, UserId};
        use crate::domain::media::FieldKnowledge;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        create_schema_seven_database(&path);
        let query = CatalogQuery::ArtistTopTracks {
            provider: "source-a".into(),
            item_id: MediaId("artist".into()),
        };
        let user = UserId("user-a".into());
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            let observed = super::super::artist_reads::read_artist_read(&snapshot, &query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(observed.observed_at, 150);
            let CatalogValue::Tracks(tracks) = observed.value else {
                panic!("expected tracks")
            };
            assert_eq!(tracks.len(), 3);
            for (track, (provider, year)) in
                tracks
                    .iter()
                    .zip([("source-c", 2027), ("album-a", 2026), ("album-b", 2026)])
            {
                assert_eq!(track.media.name, "Latest track");
                assert_eq!(track.duration, 321);
                assert_eq!(
                    track
                        .media
                        .metadata
                        .as_ref()
                        .unwrap()
                        .description
                        .as_deref(),
                    Some("Latest track metadata")
                );
                assert_eq!(track.media.favorite, FieldKnowledge::Known(Some(false)));
                assert_eq!(track.artists[0].media.name, "Latest artist");
                assert_eq!(
                    track.album.as_ref().unwrap().media.provider.as_str(),
                    provider
                );
                assert_eq!(track.album.as_ref().unwrap().year, Some(year));
            }
            assert!(!tracks[0].knowledge.disc_number && !tracks[0].knowledge.track_number);
            assert_eq!((tracks[1].disc_number, tracks[1].track_number), (1, 4));
            assert_eq!((tracks[2].disc_number, tracks[2].track_number), (2, 9));
            assert!(tracks[1].media.knowledge.position && tracks[1].media.position.is_none());
            assert_eq!(tracks[2].media.position, Some(17));
            let canonical = super::super::tracks::read_track(
                &snapshot,
                &tracks[0].media.provider,
                &tracks[0].media.item_id,
                &user,
            )
            .unwrap()
            .unwrap();
            assert_eq!((canonical.disc_number, canonical.track_number), (8, 20));
            let occurrences=snapshot.prepare("SELECT occurrence FROM artist_track_read_items WHERE user_id='user-a' ORDER BY occurrence").unwrap().query_map([],|row|row.get::<_,i64>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
            assert_eq!(occurrences, [0, 3, 7]);
            assert_eq!(snapshot.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE name IN ('artist_album_read_credits','artist_track_read_credits') OR name GLOB 'retired_*'",[],|row|row.get::<_,i64>(0)).unwrap(),0);
            for (table, count) in [
                ("artist_album_read_items", 7),
                ("artist_track_read_items", 12),
            ] {
                assert_eq!(
                    snapshot
                        .query_row(
                            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}')"),
                            [],
                            |row| row.get::<_, i64>(0)
                        )
                        .unwrap(),
                    count
                );
                let query_key = serde_json::to_string(&query).unwrap();
                let mut statement=snapshot.prepare(&format!("EXPLAIN QUERY PLAN SELECT provider,item_id FROM {table} WHERE user_id='user-a' AND query=?1 ORDER BY occurrence")).unwrap();
                let plan = statement
                    .query_map([&query_key], |row| row.get::<_, String>(3))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
                    .join(" ");
                assert!(plan.contains("USING PRIMARY KEY"), "{plan}");
                assert!(!plan.contains("TEMP B-TREE"), "{plan}");
            }
            let empty = CatalogQuery::ArtistTracks {
                provider: "source-a".into(),
                item_id: MediaId("empty".into()),
            };
            let observed = super::super::artist_reads::read_artist_read(&snapshot, &empty, &user)
                .unwrap()
                .unwrap();
            assert_eq!(observed.observed_at, 333);
            assert_eq!(observed.value, CatalogValue::Tracks(Vec::new()));
            let other = super::super::artist_reads::read_artist_read(
                &snapshot,
                &query,
                &UserId("user-b".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(other.observed_at, 250);
            let CatalogValue::Tracks(tracks) = other.value else {
                panic!("expected tracks")
            };
            assert_eq!(tracks[0].media.provider.as_str(), "source-b");
            assert_eq!(tracks[0].duration, 73);
            assert_eq!(tracks[0].media.favorite, FieldKnowledge::Known(Some(false)));
            assert!(snapshot
                .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                .optional()
                .unwrap()
                .is_none());
            snapshot.commit().unwrap();
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn artist_section_references_schema_seven_defects_leave_original_file_unchanged() {
        for defect in [
            "other_server",
            "foreign_object",
            "malformed_track_key",
            "malformed_relation",
            "invalid_context",
            "missing_canonical_album",
            "wrong_section",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_seven_database(&path);
            let connection = Connection::open(&path).unwrap();
            match defect {
                "foreign_object"=>connection.execute_batch("CREATE TABLE protected_settings(value TEXT);INSERT INTO protected_settings VALUES('keep');").unwrap(),
                "malformed_track_key"=>connection.execute_batch("ALTER TABLE tracks RENAME TO old_tracks;CREATE TABLE tracks AS SELECT * FROM old_tracks;DROP TABLE old_tracks;").unwrap(),
                "malformed_relation"=>connection.execute_batch("ALTER TABLE album_tracks RENAME COLUMN disc_number TO invalid_number;").unwrap(),
                "invalid_context"=>connection.execute_batch("UPDATE artist_track_read_items SET album='invalid json';").unwrap(),
                "missing_canonical_album"=>connection.execute_batch("DELETE FROM albums WHERE provider='album-a';").unwrap(),
                "wrong_section"=>connection.execute_batch("INSERT INTO artist_album_read_items(user_id,query,occurrence,provider,item_id,name,sort_name_known,year_known,artists_known) SELECT user_id,query,0,'source-a','007','Wrong',0,0,0 FROM artist_read_states WHERE user_id='user-a' AND observed_at=333;").unwrap(),
                _=>{},
            }
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(
                MediaDatabase::open(
                    directory.path(),
                    if defect == "other_server" {
                        "other"
                    } else {
                        "server"
                    }
                )
                .is_err(),
                "{defect}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before, "{defect}");
            let connection = Connection::open(&path).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
                    .unwrap(),
                7
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_schema WHERE name GLOB 'retired_*'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
            connection.close().unwrap();
        }
    }

    #[test]
    fn track_canonical_schema_six_migration_preserves_current_albums_and_provider_users_after_reopen(
    ) {
        use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
        use crate::domain::media::FieldKnowledge;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        create_schema_six_database(&path);
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            let track = super::super::tracks::read_track(
                &snapshot,
                &ProviderInstanceId("source-a".into()),
                &MediaId("track".into()),
                &UserId("user-a".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(track.duration, 125);
            assert!(track.knowledge.duration);
            assert_eq!(
                track
                    .media
                    .metadata
                    .as_ref()
                    .unwrap()
                    .description
                    .as_deref(),
                Some("Track detail")
            );
            assert!(track.knowledge.image && track.image.is_none());
            assert_eq!(track.media.favorite, FieldKnowledge::Known(Some(true)));
            let album = track.album.as_ref().unwrap();
            assert_eq!(album.year, Some(2025));
            assert_eq!(album.media.name, "Latest Album");
            assert_eq!(
                album
                    .media
                    .metadata
                    .as_ref()
                    .unwrap()
                    .description
                    .as_deref(),
                Some("Latest")
            );
            let other = super::super::tracks::read_track(
                &snapshot,
                &ProviderInstanceId("source-b".into()),
                &track.media.item_id,
                &UserId("user-b".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(other.duration, 73);
            assert_eq!(other.media.favorite, FieldKnowledge::Known(Some(false)));
            assert!(!other.knowledge.album);
            assert!(super::super::tracks::read_track(
                &snapshot,
                &track.media.provider,
                &track.media.item_id,
                &UserId("user-b".into())
            )
            .unwrap()
            .unwrap()
            .media
            .favorite
            .is_unknown());
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM tracks", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM album_tracks", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM artist_album_read_items", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                4
            );
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM artist_track_read_items", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                2
            );
            assert_eq!(snapshot.query_row("SELECT observed_at FROM artist_read_states WHERE user_id='user-b' AND query LIKE '%ArtistTopTracks%'",[],|row|row.get::<_,i64>(0)).unwrap(),250);
            assert!(snapshot
                .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                .optional()
                .unwrap()
                .is_none());
            snapshot.commit().unwrap();
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn track_canonical_schema_six_defects_do_not_mutate_existing_data() {
        for defect in [
            "other_server",
            "foreign_object",
            "malformed_section",
            "invalid_metadata",
            "malformed_album_key",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_six_database(&path);
            let connection = Connection::open(&path).unwrap();
            match defect {
                "foreign_object" => {
                    connection.execute_batch("CREATE TABLE protected_settings(value TEXT);INSERT INTO protected_settings VALUES('keep');").unwrap();
                }
                "malformed_section" => {
                    connection.execute_batch("ALTER TABLE artist_track_read_items RENAME COLUMN duration TO broken_duration;").unwrap();
                }
                "invalid_metadata" => {
                    connection
                        .execute_batch(
                            "UPDATE artist_track_read_items SET metadata='invalid json';",
                        )
                        .unwrap();
                }
                "malformed_album_key" => {
                    connection.execute_batch("ALTER TABLE album_favorites RENAME TO old_favorites;CREATE TABLE album_favorites AS SELECT * FROM old_favorites;DROP TABLE old_favorites;").unwrap();
                }
                _ => {}
            }
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(
                MediaDatabase::open(
                    directory.path(),
                    if defect == "other_server" {
                        "other"
                    } else {
                        "server"
                    }
                )
                .is_err(),
                "{defect}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before, "{defect}");
            let connection = Connection::open(&path).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
                    .unwrap(),
                6
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_schema WHERE name='tracks'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
            connection.close().unwrap();
        }
    }

    #[test]
    fn album_canonical_schema_five_migration_preserves_sections_users_and_embedded_albums() {
        use crate::domain::identifiers::{MediaId, ProviderInstanceId, UserId};
        use crate::domain::media::FieldKnowledge;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        create_schema_five_database(&path);
        seed_schema_five_album_sections(&path);
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            let album = super::super::albums::read_album(
                &snapshot,
                &ProviderInstanceId("source-a".into()),
                &MediaId("007".into()),
                &UserId("user-a".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(album.year, Some(2024));
            assert_eq!(
                album
                    .media
                    .metadata
                    .as_ref()
                    .unwrap()
                    .description
                    .as_deref(),
                Some("Keep")
            );
            assert_eq!(album.media.favorite, FieldKnowledge::Known(Some(true)));
            assert!(album.media.provider_mappings[0].knowledge.in_library);
            assert_eq!(album.media.provider_mappings[0].in_library, None);
            let other_user = super::super::albums::read_album(
                &snapshot,
                &album.media.provider,
                &album.media.item_id,
                &UserId("user-b".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                other_user.media.favorite,
                FieldKnowledge::Known(Some(false))
            );
            let other_provider = super::super::albums::read_album(
                &snapshot,
                &ProviderInstanceId("source-b".into()),
                &album.media.item_id,
                &UserId("user-a".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(other_provider.year, Some(1999));
            let embedded = super::super::albums::read_album(
                &snapshot,
                &ProviderInstanceId("source-c".into()),
                &album.media.item_id,
                &UserId("user-a".into()),
            )
            .unwrap()
            .unwrap();
            assert!(embedded.knowledge.year && embedded.year.is_none());
            assert!(embedded.knowledge.artists && embedded.artists.is_empty());
            assert!(embedded.media.favorite.is_unknown());
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM albums", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                3
            );
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM artist_album_read_items", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                4
            );
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM artist_track_read_items", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                1
            );
            assert_eq!(
                snapshot
                    .query_row(
                        "SELECT observed_at FROM artist_read_states WHERE user_id='user-b'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                200
            );
            assert_eq!(
                snapshot
                    .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                    .optional()
                    .unwrap(),
                None
            );
            snapshot.commit().unwrap();
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn album_canonical_schema_five_errors_leave_original_database_unchanged() {
        for defect in [
            "foreign_object",
            "malformed_section",
            "invalid_metadata",
            "other_server",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_five_database(&path);
            seed_schema_five_album_sections(&path);
            let connection = Connection::open(&path).unwrap();
            match defect {
                "foreign_object" => {
                    connection.execute_batch("CREATE TABLE protected_settings(value TEXT);INSERT INTO protected_settings VALUES('keep');").unwrap();
                }
                "malformed_section" => {
                    connection.execute_batch("ALTER TABLE artist_album_read_items RENAME COLUMN year TO broken_year;").unwrap();
                }
                "invalid_metadata" => {
                    connection
                        .execute_batch(
                            "UPDATE artist_album_read_items SET metadata='invalid json';",
                        )
                        .unwrap();
                }
                _ => {}
            }
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            let server = if defect == "other_server" {
                "other"
            } else {
                "server"
            };
            assert!(
                MediaDatabase::open(directory.path(), server).is_err(),
                "{defect}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before, "{defect}");
            let connection = Connection::open(&path).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
                    .unwrap(),
                5
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM pragma_table_info('albums') WHERE name='metadata'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
            connection.close().unwrap();
        }
    }

    #[test]
    fn artist_schema_four_and_three_upgrade_preserve_observations_after_reopen() {
        use crate::data::cache::artist_reads::{read_artist_read, write_artist_read};
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::{MediaId, UserId};
        for version in [3, 4] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_five_database(&path);
            let mut connection = Connection::open(&path).unwrap();
            let query = CatalogQuery::Artist {
                provider: "library".into(),
                item_id: MediaId("007".into()),
            };
            let user = UserId("user".into());
            let artist=serde_json::from_value(serde_json::json!({"provider":"library","item_id":"007","name":"Preserved","favorite":null})).unwrap();
            let transaction = connection.transaction().unwrap();
            write_artist_read(
                &transaction,
                &query,
                &user,
                &CatalogValue::Artist(artist),
                123,
            )
            .unwrap();
            transaction.commit().unwrap();
            connection
                .execute_batch(
                    "DROP TABLE artist_album_read_credits;DROP TABLE artist_track_read_credits;
                DROP TABLE artist_album_read_items;DROP TABLE artist_track_read_items;",
                )
                .unwrap();
            if version == 3 {
                connection
                    .execute_batch("DROP TABLE artist_read_items;DROP TABLE artist_read_states;")
                    .unwrap();
            }
            connection
                .pragma_update(None, "user_version", version)
                .unwrap();
            connection.close().unwrap();
            MediaDatabase::open(directory.path(), "server")
                .unwrap()
                .shutdown()
                .unwrap();
            let reopened = MediaDatabase::open(directory.path(), "server").unwrap();
            let mut reader = reopened.open_reader().unwrap();
            assert_eq!(
                reader
                    .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
                    .unwrap(),
                SCHEMA_VERSION
            );
            let snapshot = reader.transaction().unwrap();
            let artist = crate::data::cache::artists::read_artist(
                &snapshot,
                &crate::domain::identifiers::ProviderInstanceId("library".into()),
                &MediaId("007".into()),
                &user,
            )
            .unwrap()
            .unwrap();
            assert_eq!(artist.media.name, "Preserved");
            assert_eq!(
                artist.media.favorite,
                crate::domain::media::FieldKnowledge::Known(None)
            );
            let read = read_artist_read(&snapshot, &query, &user).unwrap();
            if version == 4 {
                assert_eq!(read.unwrap().observed_at, 123);
            } else {
                assert!(read.is_none());
            }
            snapshot.commit().unwrap();
            reader.close().unwrap();
            reopened.shutdown().unwrap();
        }
    }

    #[test]
    fn artist_schema_four_malformed_read_state_is_rejected_before_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        create_schema_five_database(&path);
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("DROP TABLE artist_album_read_credits;DROP TABLE artist_track_read_credits;
            DROP TABLE artist_album_read_items;DROP TABLE artist_track_read_items;
            DROP TABLE artist_read_items;DROP TABLE artist_read_states;
            CREATE TABLE artist_read_states(user_id TEXT,query TEXT,PRIMARY KEY(user_id,query));
            CREATE TABLE artist_read_items(user_id TEXT,query TEXT,position INTEGER,provider TEXT,item_id TEXT,PRIMARY KEY(user_id,query,position));").unwrap();
        connection.pragma_update(None, "user_version", 4).unwrap();
        connection.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server"),
            Err(CacheError::ForeignFormat)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    // Historical schema eight has no independent album response tables.
    fn create_schema_eight_database(
        path: &Path,
    ) -> (
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
    ) {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::{MediaId, UserId};
        let mut connection = Connection::open(path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch("CREATE TABLE cache_file_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),server_id TEXT NOT NULL CHECK(length(trim(server_id))>0)) STRICT;
            INSERT INTO cache_file_metadata VALUES(1,'server');").unwrap();
        initialize_artist_schema(&transaction).unwrap();
        initialize_album_artist_schema(&transaction).unwrap();
        initialize_album_details_schema(&transaction).unwrap();
        initialize_track_schema(&transaction).unwrap();
        initialize_artist_read_schema(&transaction).unwrap();
        initialize_artist_section_schema(&transaction).unwrap();
        initialize_artist_query_indexes(&transaction).unwrap();
        let query = CatalogQuery::ArtistAlbums {
            provider: "source-a".into(),
            item_id: MediaId("artist".into()),
        };
        let album = serde_json::from_value(serde_json::json!({"provider":"source-a","item_id":"007","name":"Preserved","year":2020,"favorite":false,"metadata":{"description":"Original"},"artists":[{"provider":"source-b","item_id":"007","name":"Credit"}]})).unwrap();
        let value = CatalogValue::Albums(vec![album]);
        super::super::artist_reads::write_artist_read(
            &transaction,
            &query,
            &UserId("user".into()),
            &value,
            100,
        )
        .unwrap();
        transaction
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        transaction.pragma_update(None, "user_version", 8).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
        (query, value)
    }

    fn create_schema_nine_database(
        path: &Path,
    ) -> (
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
    ) {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::{MediaId, UserId};
        let (_, value) = create_schema_eight_database(path);
        let CatalogValue::Albums(albums) = value else {
            unreachable!()
        };
        let query = CatalogQuery::Album {
            provider: "source-a".into(),
            item_id: MediaId("007".into()),
        };
        let value = CatalogValue::Album(albums.into_iter().next().unwrap());
        let mut connection = Connection::open(path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        initialize_album_read_schema(&transaction).unwrap();
        super::super::album_reads::write_album_read(
            &transaction,
            &query,
            &UserId("user".into()),
            &value,
            200,
        )
        .unwrap();
        transaction.pragma_update(None, "user_version", 9).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
        (query, value)
    }

    // Album response extensions preserve observations and reject malformed files before writing.
    fn create_schema_ten_database(
        path: &Path,
    ) -> (
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
    ) {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::UserId;
        create_schema_nine_database(path);
        let mut connection = Connection::open(path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        initialize_album_track_read_schema(&transaction).unwrap();
        let query = CatalogQuery::LibraryAlbums {
            favorite: None,
            limit: 500,
            offset: 0,
            summary: false,
            filters: Default::default(),
        };
        let value:CatalogValue=CatalogValue::Albums(serde_json::from_value(serde_json::json!([
            {"provider":"library","item_id":"library-album","name":"Library album","sort_name":"Library album","year":2024,"album_type":"album","favorite":true,
                "metadata":{"description":"Keep"},"external_ids":[["barcode","00123"]],
                "artists":[{"provider":"library","item_id":"artist","name":"Artist"}],
                "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"007","in_library":true}]}
        ])).unwrap());
        super::super::album_reads::write_album_read(
            &transaction,
            &query,
            &UserId("user".into()),
            &value,
            300,
        )
        .unwrap();
        let tracks = CatalogQuery::AlbumTracks {
            provider: "library".into(),
            item_id: crate::domain::identifiers::MediaId("library-album".into()),
        };
        let track = serde_json::json!({"provider":"source-a","item_id":"track","name":"Track","album":{"provider":"library","item_id":"library-album","name":"Library album"},"disc_number":1,"track_number":4});
        let track_value = CatalogValue::Tracks(
            serde_json::from_value(serde_json::json!([track.clone(), track])).unwrap(),
        );
        super::super::album_reads::write_album_read(
            &transaction,
            &tracks,
            &UserId("user".into()),
            &track_value,
            400,
        )
        .unwrap();
        transaction.pragma_update(None, "user_version", 10).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
        (query, value)
    }

    #[test]
    fn album_candidates_schema_ten_upgrade_preserves_details_favorites_and_read_coverage_after_reopen(
    ) {
        use crate::domain::identifiers::UserId;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let (query, expected) = create_schema_ten_database(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "other-server"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            let observed =
                super::super::reads::read_cached_read(&snapshot, &query, &UserId("user".into()))
                    .unwrap()
                    .unwrap();
            assert_eq!(observed.value, expected);
            assert_eq!(observed.observed_at, 300);
            let occurrences=snapshot.prepare("SELECT disc_number,track_number FROM album_track_read_items ORDER BY occurrence").unwrap()
                .query_map([],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,i64>(1)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
            assert_eq!(occurrences, [(1, 4), (1, 4)]);
            assert_eq!(snapshot.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='index' AND name LIKE 'albums_candidate_%'",[],|row|row.get::<_,i64>(0)).unwrap(),3);
            let candidates = super::super::album_candidates::read_album_candidates(
                &snapshot,
                &super::super::album_candidates::AlbumCandidateQuery::new(UserId("user".into())),
            )
            .unwrap();
            assert_eq!(candidates.items.len(), 1);
            drop(snapshot);
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    // Historical schema 11 adds only its three candidate indexes to a genuine schema 10.
    fn create_schema_eleven_database(
        path: &Path,
    ) -> (
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
    ) {
        let previous = create_schema_ten_database(path);
        let mut connection = Connection::open(path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::album_candidates::initialize_album_query_indexes(&transaction).unwrap();
        transaction.pragma_update(None, "user_version", 11).unwrap();
        transaction.commit().unwrap();
        connection.execute_batch("ANALYZE;").unwrap();
        connection.close().unwrap();
        previous
    }

    // Historical schema 12 adds its real single-read table, not a downgraded current schema.
    fn create_schema_twelve_database(
        path: &Path,
    ) -> (
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
    ) {
        create_schema_eleven_database(path);
        let mut connection = Connection::open(path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        super::super::track_reads::initialize_track_read_schema(&transaction).unwrap();
        let query = crate::domain::catalog::CatalogQuery::Track {
            provider: "source-a".into(),
            item_id: crate::domain::identifiers::MediaId("legacy".into()),
        };
        let value = crate::domain::catalog::CatalogValue::Track(serde_json::from_value(serde_json::json!({
            "provider":"source-a","item_id":"legacy","name":"Legacy","favorite":null,"duration":321,"album":null,"image":null
        })).unwrap());
        super::super::track_reads::write_track_read(
            &transaction,
            &query,
            &crate::domain::identifiers::UserId("user".into()),
            &value,
            500,
        )
        .unwrap();
        transaction.pragma_update(None, "user_version", 12).unwrap();
        transaction.commit().unwrap();
        connection.execute_batch("ANALYZE;").unwrap();
        connection.close().unwrap();
        (query, value)
    }

    #[test]
    fn track_lists_schema_thirteen_upgrade_preserves_searches_and_reopens_lists() {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::{MediaId, UserId};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let (single_query, single_value, search_query, search_value) =
            create_schema_thirteen_database(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "other"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut database = MediaDatabase::open(directory.path(), "server").unwrap();
        let versions = CatalogQuery::TrackVersions {
            provider: "source-a".into(),
            item_id: MediaId("base".into()),
        };
        let value = CatalogValue::Tracks(vec![serde_json::from_value(serde_json::json!({
            "provider":"source-b","item_id":"version","name":"Version","duration":240,"album":null,"image":null
        })).unwrap()]);
        let similar = CatalogQuery::SimilarTracks {
            provider: "source-a".into(),
            item_id: MediaId("base".into()),
            limit: 0,
            allow_lookup: false,
        };
        let lookup = CatalogQuery::SimilarTracks {
            provider: "source-a".into(),
            item_id: MediaId("base".into()),
            limit: 2,
            allow_lookup: true,
        };
        let empty = CatalogValue::Tracks(Vec::new());
        let recent = CatalogQuery::RecentlyPlayed { limit: 2 };
        let recent_all = CatalogQuery::RecentlyPlayed { limit: 0 };
        let playlist = CatalogQuery::PlaylistTracks {
            provider: "library".into(),
            item_id: MediaId("playlist".into()),
            allow_dynamic_tracks: false,
        };
        let dynamic_playlist = CatalogQuery::PlaylistTracks {
            provider: "library".into(),
            item_id: MediaId("playlist".into()),
            allow_dynamic_tracks: true,
        };
        let empty_playlist = CatalogQuery::PlaylistTracks {
            provider: "source-a".into(),
            item_id: MediaId("empty-playlist".into()),
            allow_dynamic_tracks: false,
        };
        let playlist_value = CatalogValue::Tracks(
            [1, 4]
                .into_iter()
                .map(|position| {
                    serde_json::from_value(serde_json::json!({
                        "media_type":"track","provider":"source-c","item_id":"playlist-track",
                        "name":"Occurrence","position":position,"album":null,"image":null
                    }))
                    .unwrap()
                })
                .collect(),
        );
        let recent_value = CatalogValue::Tracks(vec![serde_json::from_value(serde_json::json!({
            "media_type":"track","provider":"source-b","item_id":"recent","name":"Recent","image":null,"available":true
        })).unwrap()]);
        let transaction = database.writer.transaction().unwrap();
        for (query, observed, time) in [
            (&versions, &value, 700),
            (&similar, &empty, 701),
            (&lookup, &value, 702),
            (&recent, &recent_value, 703),
            (&recent_all, &empty, 704),
            (&playlist, &playlist_value, 705),
            (&dynamic_playlist, &playlist_value, 706),
            (&empty_playlist, &empty, 707),
        ] {
            super::super::reads::write_cached_read(
                &transaction,
                query,
                &UserId("user".into()),
                observed,
                time,
            )
            .unwrap();
        }
        transaction.commit().unwrap();
        database.shutdown().unwrap();
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            for (query, expected, time) in [
                (&single_query, &single_value, 500),
                (&search_query, &search_value, 600),
                (&versions, &value, 700),
                (&similar, &empty, 701),
                (&lookup, &value, 702),
                (&recent, &recent_value, 703),
                (&recent_all, &empty, 704),
                (&playlist, &playlist_value, 705),
                (&dynamic_playlist, &playlist_value, 706),
                (&empty_playlist, &empty, 707),
            ] {
                let observed =
                    super::super::reads::read_cached_read(&snapshot, query, &UserId("user".into()))
                        .unwrap()
                        .unwrap();
                assert_eq!(observed.value, *expected);
                assert_eq!(observed.observed_at, time);
            }
            for query in [
                &versions,
                &similar,
                &lookup,
                &recent,
                &recent_all,
                &playlist,
                &dynamic_playlist,
                &empty_playlist,
            ] {
                assert!(super::super::reads::read_cached_read(
                    &snapshot,
                    query,
                    &UserId("other".into())
                )
                .unwrap()
                .is_none());
            }
            assert_eq!(snapshot.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE name IN ('track_search_states','track_search_items','track_search_items_identity_idx')",[],|row|row.get::<_,i64>(0)).unwrap(),0);
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                0
            );
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM album_track_read_items", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            drop(snapshot);
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn track_versions_schema_thirteen_defects_preserve_original_file() {
        for change in [
            "CREATE TABLE sqlitex_protected(value TEXT);",
            "ALTER TABLE track_search_items ADD COLUMN payload TEXT;",
            "DROP INDEX track_search_items_identity_idx;",
            "DROP INDEX track_search_items_identity_idx;CREATE VIEW track_search_items_identity_idx AS SELECT 1;",
            "UPDATE track_search_items SET item_id='missing';",
        ] {
            let directory = tempfile::tempdir().unwrap(); let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_thirteen_database(&path);
            let connection = Connection::open(&path).unwrap(); connection.pragma_update(None,"foreign_keys",false).unwrap();
            connection.execute_batch(change).unwrap(); connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(MediaDatabase::open(directory.path(),"server"),Err(CacheError::ForeignFormat)));
            assert_eq!(std::fs::read(&path).unwrap(),before);
        }
    }

    #[test]
    fn playlist_schema_fourteen_and_fifteen_upgrade_preserves_reads_and_reopens_recommendations() {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::UserId;
        for version in [14, 15] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            let expected = create_playlist_schema_database(&path, version);
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(
                MediaDatabase::open(directory.path(), "other"),
                Err(CacheError::ServerMismatch)
            ));
            assert_eq!(std::fs::read(&path).unwrap(), before);
            let mut database = MediaDatabase::open(directory.path(), "server").unwrap();
            let recommendation = CatalogQuery::RecommendationItems {
                provider: "recommendations".into(),
                item_id: crate::domain::identifiers::MediaId("row".into()),
            };
            let empty = CatalogValue::RecommendationItems(Vec::new());
            let transaction = database.writer.transaction().unwrap();
            super::super::reads::write_cached_read(
                &transaction,
                &recommendation,
                &UserId("user".into()),
                &empty,
                900,
            )
            .unwrap();
            transaction.commit().unwrap();
            database.shutdown().unwrap();
            for _ in 0..2 {
                let database = MediaDatabase::open(directory.path(), "server").unwrap();
                assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
                let mut reader = database.open_reader().unwrap();
                let snapshot = reader.transaction().unwrap();
                for (query, value, time) in &expected {
                    let observed = super::super::reads::read_cached_read(
                        &snapshot,
                        query,
                        &UserId("user".into()),
                    )
                    .unwrap()
                    .unwrap();
                    assert_eq!(observed.value, *value);
                    assert_eq!(observed.observed_at, *time);
                    assert!(super::super::reads::read_cached_read(
                        &snapshot,
                        query,
                        &UserId("other".into())
                    )
                    .unwrap()
                    .is_none());
                }
                let observed = super::super::reads::read_cached_read(
                    &snapshot,
                    &recommendation,
                    &UserId("user".into()),
                )
                .unwrap()
                .unwrap();
                assert_eq!(observed.value, empty);
                assert_eq!(observed.observed_at, 900);
                assert_eq!(snapshot.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name NOT GLOB 'sqlite_*'", [], |row| row.get::<_,i64>(0)).unwrap(), 37);
                assert_eq!(
                    snapshot
                        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .unwrap(),
                    0
                );
                drop(snapshot);
                reader.close().unwrap();
                database.shutdown().unwrap();
            }
        }
    }

    #[test]
    fn playlist_schema_fifteen_defects_preserve_original_file() {
        for change in [
            "ALTER TABLE playlist_read_items ADD COLUMN payload TEXT;",
            "DROP INDEX playlist_provider_lookup_idx;",
            "DROP INDEX playlist_read_items_identity_idx; CREATE VIEW playlist_read_items_identity_idx AS SELECT 1;",
            "UPDATE playlist_provider_mappings SET playlist_id='missing';",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_playlist_schema_database(&path, 15);
            let connection = Connection::open(&path).unwrap();
            connection.pragma_update(None, "foreign_keys", false).unwrap();
            connection.execute_batch(change).unwrap();
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(MediaDatabase::open(directory.path(), "server"), Err(CacheError::ForeignFormat)));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    // Historical schema 14 renames search references; schema 15 adds only the frozen playlist DDL.
    fn create_playlist_schema_database(
        path: &Path,
        version: i32,
    ) -> Vec<(
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
        i64,
    )> {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::{MediaId, UserId};
        let (single_query, single_value, search_query, search_value) =
            create_schema_thirteen_database(path);
        let mut expected = vec![
            (single_query, single_value, 500),
            (search_query, search_value, 600),
        ];
        let mut connection = Connection::open(path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch("ALTER TABLE track_search_states RENAME TO track_list_states;
            ALTER TABLE track_search_items RENAME TO track_list_items;
            DROP INDEX track_search_items_identity_idx;
            CREATE INDEX track_list_items_identity_idx ON track_list_items(user_id,provider,item_id,query);").unwrap();
        if version == 15 {
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
                ) STRICT, WITHOUT ROWID;
                CREATE TABLE playlist_read_states (
                user_id TEXT NOT NULL REFERENCES users(user_id),query TEXT NOT NULL,observed_at INTEGER NOT NULL CHECK(observed_at>=0),PRIMARY KEY(user_id,query)
                ) STRICT, WITHOUT ROWID;
                CREATE TABLE playlist_read_items (
                user_id TEXT NOT NULL,query TEXT NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),provider TEXT NOT NULL,item_id TEXT NOT NULL,
                position INTEGER CHECK(position>=0),position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),
                PRIMARY KEY(user_id,query,occurrence),FOREIGN KEY(user_id,query) REFERENCES playlist_read_states(user_id,query) ON DELETE CASCADE,
                FOREIGN KEY(provider,item_id) REFERENCES playlists(provider,item_id)
                ) STRICT, WITHOUT ROWID;
                CREATE INDEX playlist_read_items_identity_idx ON playlist_read_items(user_id,provider,item_id,query);").unwrap();
            let query = CatalogQuery::Playlist {
                provider: "library".into(),
                item_id: MediaId("playlist".into()),
            };
            let value = CatalogValue::Playlist(serde_json::from_value(serde_json::json!({
                "media_type":"playlist","provider":"library","item_id":"playlist","name":"Legacy Playlist",
                "favorite":true,"owner":"user","is_editable":true,"is_dynamic":false,
                "external_ids":[["barcode","123"]],"provider_mappings":[{"provider_domain":"builtin","provider_instance":"builtin","item_id":"playlist"}],
                "metadata":{"description":"Keep details"}
            })).unwrap());
            super::super::reads::write_cached_read(
                &transaction,
                &query,
                &UserId("user".into()),
                &value,
                700,
            )
            .unwrap();
            expected.push((query, value, 700));
        }
        let query = CatalogQuery::PlaylistTracks {
            provider: "library".into(),
            item_id: MediaId("playlist".into()),
            allow_dynamic_tracks: false,
        };
        let value = CatalogValue::Tracks([1,4].into_iter().map(|position| serde_json::from_value(serde_json::json!({
            "provider":"source","item_id":"duplicate","name":"Occurrence","position":position,"album":null,"image":null
        })).unwrap()).collect());
        super::super::reads::write_cached_read(
            &transaction,
            &query,
            &UserId("user".into()),
            &value,
            800,
        )
        .unwrap();
        expected.push((query, value, 800));
        transaction
            .pragma_update(None, "user_version", version)
            .unwrap();
        transaction.commit().unwrap();
        connection.execute_batch("ANALYZE;").unwrap();
        connection.close().unwrap();
        expected
    }

    // Genuine schema 13 retains the historical search names and inserts reference-only rows.
    fn create_schema_thirteen_database(
        path: &Path,
    ) -> (
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
        crate::domain::catalog::CatalogQuery,
        crate::domain::catalog::CatalogValue,
    ) {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        let (single_query, single_value) = create_schema_twelve_database(path);
        let mut connection = Connection::open(path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch("CREATE TABLE track_search_states (
            user_id TEXT NOT NULL REFERENCES users(user_id),query TEXT NOT NULL,observed_at INTEGER NOT NULL CHECK(observed_at>=0),PRIMARY KEY(user_id,query)
            ) STRICT, WITHOUT ROWID;
            CREATE TABLE track_search_items (
                user_id TEXT NOT NULL,query TEXT NOT NULL,occurrence INTEGER NOT NULL CHECK(occurrence>=0),provider TEXT NOT NULL,item_id TEXT NOT NULL,
                album_provider TEXT,album_item_id TEXT,album_known INTEGER NOT NULL CHECK(album_known IN (0,1)),disc_number INTEGER CHECK(disc_number>=0),
                track_number INTEGER CHECK(track_number>=0),position INTEGER CHECK(position>=0),position_known INTEGER NOT NULL CHECK(position_known IN (0,1)),
                image TEXT CHECK(image IS NULL OR json_type(image)='object'),image_known INTEGER NOT NULL DEFAULT 0 CHECK(image_known IN (0,1)),
                PRIMARY KEY(user_id,query,occurrence),CHECK((album_provider IS NULL)=(album_item_id IS NULL)),CHECK(album_known=1 OR album_provider IS NULL),
                FOREIGN KEY(user_id,query) REFERENCES track_search_states(user_id,query) ON DELETE CASCADE,
                FOREIGN KEY(provider,item_id) REFERENCES tracks(provider,item_id),
                FOREIGN KEY(album_provider,album_item_id,provider,item_id) REFERENCES album_tracks(album_provider,album_item_id,track_provider,track_item_id)
            ) STRICT, WITHOUT ROWID;
            CREATE INDEX track_search_items_identity_idx ON track_search_items(user_id,provider,item_id,query);") .unwrap();
        let search_query = CatalogQuery::Search {
            query: "Legacy".into(),
            media_type: crate::domain::media::MediaType::Track,
            limit: 25,
        };
        let query_key = serde_json::to_string(&search_query).unwrap();
        transaction
            .execute(
                "INSERT INTO track_search_states(user_id,query,observed_at) VALUES('user',?1,600)",
                [&query_key],
            )
            .unwrap();
        for occurrence in 0..2 {
            transaction.execute("INSERT INTO track_search_items(user_id,query,occurrence,provider,item_id,album_known,position_known,image_known)
                VALUES('user',?1,?2,'source-a','legacy',1,0,1)",rusqlite::params![query_key,occurrence]).unwrap();
        }
        transaction.pragma_update(None, "user_version", 13).unwrap();
        transaction.commit().unwrap();
        connection.execute_batch("ANALYZE;").unwrap();
        connection.close().unwrap();
        let CatalogValue::Track(track) = single_value.clone() else {
            unreachable!()
        };
        let search_value =
            CatalogValue::Search(crate::domain::media::MusicAssistantSearchResults {
                tracks: vec![track.clone(), track],
                ..Default::default()
            });
        (single_query, single_value, search_query, search_value)
    }

    #[test]
    fn track_search_schema_twelve_preserves_single_reads_and_reopens_searches() {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::UserId;
        use crate::domain::media::MediaType;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let (old_query, old_value) = create_schema_twelve_database(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "other"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut database = MediaDatabase::open(directory.path(), "server").unwrap();
        let query = CatalogQuery::Search {
            query: "Legacy".into(),
            media_type: MediaType::Track,
            limit: 25,
        };
        let CatalogValue::Track(track) = old_value.clone() else {
            unreachable!()
        };
        let value = CatalogValue::Search(crate::domain::media::MusicAssistantSearchResults {
            tracks: vec![track.clone(), track],
            ..Default::default()
        });
        let transaction = database.writer.transaction().unwrap();
        super::super::reads::write_cached_read(
            &transaction,
            &query,
            &UserId("user".into()),
            &value,
            600,
        )
        .unwrap();
        transaction.commit().unwrap();
        database.shutdown().unwrap();
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            let old = super::super::reads::read_cached_read(
                &snapshot,
                &old_query,
                &UserId("user".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(old.observed_at, 500);
            assert_eq!(old.value, old_value);
            let search =
                super::super::reads::read_cached_read(&snapshot, &query, &UserId("user".into()))
                    .unwrap()
                    .unwrap();
            assert_eq!(search.observed_at, 600);
            assert_eq!(search.value, value);
            assert_eq!(
                snapshot
                    .query_row("SELECT COUNT(*) FROM album_track_read_items", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert!(super::super::reads::read_cached_read(
                &snapshot,
                &query,
                &UserId("other".into())
            )
            .unwrap()
            .is_none());
            drop(snapshot);
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn track_search_schema_twelve_defects_preserve_original_file() {
        for change in [
            "CREATE TABLE sqlitex_protected(value TEXT);",
            "ALTER TABLE track_read_states ADD COLUMN payload TEXT;",
            "DROP INDEX track_read_states_identity_idx;",
            "DROP INDEX track_domain_lookup_idx;CREATE VIEW track_domain_lookup_idx AS SELECT 1;",
            "UPDATE track_read_states SET item_id='missing';",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_twelve_database(&path);
            let connection = Connection::open(&path).unwrap();
            connection
                .pragma_update(None, "foreign_keys", false)
                .unwrap();
            connection.execute_batch(change).unwrap();
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(
                MediaDatabase::open(directory.path(), "server"),
                Err(CacheError::ForeignFormat)
            ));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn track_single_reads_schema_eleven_upgrade_preserves_media_context_and_reopens_new_reads() {
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        use crate::domain::identifiers::{MediaId, UserId};
        use crate::domain::media::FieldKnowledge;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let (album_query, expected) = create_schema_eleven_database(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "different-server"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut database = MediaDatabase::open(directory.path(), "server").unwrap();
        assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
        let user = UserId("user".into());
        let query = CatalogQuery::Track {
            provider: "source-a".into(),
            item_id: MediaId("track".into()),
        };
        let track = serde_json::from_value(serde_json::json!({
            "provider":"source-a","item_id":"track","name":"Single item","favorite":null,"duration":321,"album":null
        })).unwrap();
        let transaction = database.writer.transaction().unwrap();
        super::super::reads::write_cached_read(
            &transaction,
            &query,
            &user,
            &CatalogValue::Track(track),
            500,
        )
        .unwrap();
        transaction.commit().unwrap();
        database.shutdown().unwrap();
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            let old = super::super::reads::read_cached_read(&snapshot, &album_query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(old.observed_at, 300);
            assert_eq!(old.value, expected);
            let contexts = snapshot.prepare("SELECT disc_number,track_number FROM album_track_read_items ORDER BY occurrence").unwrap()
                .query_map([],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,i64>(1)?))).unwrap()
                .collect::<rusqlite::Result<Vec<_>>>().unwrap();
            assert_eq!(contexts, [(1, 4), (1, 4)]);
            let new = super::super::reads::read_cached_read(&snapshot, &query, &user)
                .unwrap()
                .unwrap();
            assert_eq!(new.observed_at, 500);
            let CatalogValue::Track(track) = new.value else {
                panic!("track missing")
            };
            assert_eq!(track.media.name, "Single item");
            assert_eq!(track.duration, 321);
            assert_eq!(track.media.favorite, FieldKnowledge::Known(None));
            assert!(track.album.is_none());
            assert!(track.knowledge.album);
            drop(snapshot);
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn track_single_reads_schema_eleven_defects_leave_original_file_unchanged() {
        for change in [
            "CREATE TABLE sqlitex_protected(value TEXT);",
            "ALTER TABLE album_track_read_items ADD COLUMN payload TEXT;",
            "DROP INDEX albums_candidate_year_idx;",
            "DROP INDEX albums_candidate_name_idx;CREATE VIEW albums_candidate_name_idx AS SELECT 1;",
            "UPDATE album_read_items SET item_id='missing' WHERE provider='library';",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_eleven_database(&path);
            let connection = Connection::open(&path).unwrap();
            connection.pragma_update(None, "foreign_keys", false).unwrap();
            connection.execute_batch(change).unwrap();
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(MediaDatabase::open(directory.path(), "server"), Err(CacheError::ForeignFormat)));
            assert_eq!(std::fs::read(&path).unwrap(), before);
            let connection = Connection::open(&path).unwrap();
            assert_eq!(connection.pragma_query_value::<i32,_>(None,"user_version",|row|row.get(0)).unwrap(),11);
        }
    }

    #[test]
    fn album_candidates_schema_ten_defects_leave_original_file_unchanged() {
        for change in ["CREATE TABLE sqlitex_protected(value TEXT);",
            "ALTER TABLE album_track_read_items ADD COLUMN payload TEXT;",
            "DROP INDEX album_read_items_identity_idx;CREATE VIEW album_read_items_identity_idx AS SELECT 1;",
            "UPDATE album_read_items SET item_id='missing' WHERE provider='library';"] {
            let directory=tempfile::tempdir().unwrap();
            let path=directory.path().join(DATABASE_FILE_NAME);
            create_schema_ten_database(&path);
            let connection=Connection::open(&path).unwrap();
            connection.pragma_update(None,"foreign_keys",false).unwrap();
            connection.execute_batch(change).unwrap();
            connection.close().unwrap();
            let before=std::fs::read(&path).unwrap();
            assert!(matches!(MediaDatabase::open(directory.path(),"server"),Err(CacheError::ForeignFormat)));
            assert_eq!(std::fs::read(&path).unwrap(),before);
            let connection=Connection::open(&path).unwrap();
            assert_eq!(connection.pragma_query_value::<i32,_>(None,"user_version",|row|row.get(0)).unwrap(),10);
        }
    }

    #[test]
    fn album_tracks_schema_eight_and_nine_accept_statistics_and_preserve_reads_after_reopen() {
        use crate::domain::identifiers::UserId;
        for version in [8, 9] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            let (query, expected) = if version == 8 {
                create_schema_eight_database(&path)
            } else {
                create_schema_nine_database(&path)
            };
            let connection = Connection::open(&path).unwrap();
            super::super::artist_candidates::initialize_artist_query_functions(&connection)
                .unwrap();
            connection.execute_batch("ANALYZE;").unwrap();
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_schema WHERE name='sqlite_stat1'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                1
            );
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(
                MediaDatabase::open(directory.path(), "other-server"),
                Err(CacheError::ServerMismatch)
            ));
            assert_eq!(std::fs::read(&path).unwrap(), before);
            for _ in 0..2 {
                let database = MediaDatabase::open(directory.path(), "server").unwrap();
                let mut reader = database.open_reader().unwrap();
                let snapshot = reader.transaction().unwrap();
                let observed = super::super::reads::read_cached_read(
                    &snapshot,
                    &query,
                    &UserId("user".into()),
                )
                .unwrap()
                .unwrap();
                assert_eq!(observed.value, expected);
                assert_eq!(observed.observed_at, if version == 8 { 100 } else { 200 });
                assert_eq!(
                    snapshot
                        .query_row("SELECT COUNT(*) FROM album_track_read_items", [], |row| row
                            .get::<_, i64>(0))
                        .unwrap(),
                    0
                );
                assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
                drop(snapshot);
                reader.close().unwrap();
                database.shutdown().unwrap();
            }
        }
    }

    #[test]
    fn album_tracks_schema_nine_defects_leave_original_file_unchanged() {
        for change in [
            "CREATE TABLE sqlitex_protected(value TEXT);",
            "ALTER TABLE album_read_items ADD COLUMN payload TEXT;",
            "DROP INDEX album_read_items_identity_idx;CREATE VIEW album_read_items_identity_idx AS SELECT 1;",
            "DROP TABLE album_read_items;CREATE TABLE album_read_items(user_id TEXT,query TEXT,position INTEGER,provider TEXT,item_id TEXT,PRIMARY KEY(user_id,query,position));",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_nine_database(&path);
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(change).unwrap();
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(MediaDatabase::open(directory.path(), "server"), Err(CacheError::ForeignFormat)), "{change}");
            assert_eq!(std::fs::read(&path).unwrap(), before, "{change}");
        }
    }

    #[test]
    fn album_reads_schema_eight_preserves_canonical_media_and_artist_context_after_reopen() {
        use crate::domain::catalog::CatalogQuery;
        use crate::domain::identifiers::{MediaId, UserId};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let (query, expected) = create_schema_eight_database(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "other-server"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let mut reader = database.open_reader().unwrap();
            let snapshot = reader.transaction().unwrap();
            let observation = super::super::artist_reads::read_artist_read(
                &snapshot,
                &query,
                &UserId("user".into()),
            )
            .unwrap()
            .unwrap();
            assert_eq!(observation.observed_at, 100);
            assert_eq!(observation.value, expected);
            let album_query = CatalogQuery::Album {
                provider: "source-a".into(),
                item_id: MediaId("007".into()),
            };
            assert!(super::super::album_reads::read_album_read(
                &snapshot,
                &album_query,
                &UserId("user".into())
            )
            .unwrap()
            .is_none());
            assert!(super::super::album_reads::read_partial_album(
                &snapshot,
                &album_query,
                &UserId("user".into())
            )
            .unwrap()
            .is_some());
            let states: i64 = snapshot
                .query_row("SELECT COUNT(*) FROM album_read_states", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(states, 0);
            drop(snapshot);
            reader.close().unwrap();
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn album_reads_schema_eight_defects_leave_original_file_unchanged() {
        for change in [
            "CREATE TABLE sqlitex_protected(value TEXT);",
            "DROP TABLE album_artists;",
            "ALTER TABLE artist_album_read_items ADD COLUMN payload TEXT;",
            "DROP INDEX artists_search_name_idx;CREATE VIEW artists_search_name_idx AS SELECT 1;",
            "DROP TABLE artist_track_read_items;CREATE TABLE artist_track_read_items(user_id TEXT,query TEXT,occurrence INTEGER,provider TEXT,item_id TEXT,album_provider TEXT,album_item_id TEXT,album_known INTEGER,disc_number INTEGER,track_number INTEGER,position INTEGER,position_known INTEGER,PRIMARY KEY(user_id,query,occurrence));",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_schema_eight_database(&path);
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(change).unwrap();
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(MediaDatabase::open(directory.path(),"server"),Err(CacheError::ForeignFormat)),"{change}");
            assert_eq!(std::fs::read(&path).unwrap(),before,"{change}");
        }
    }

    #[test]
    fn metadata_survives_reopening_without_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let database = MediaDatabase::open(directory.path(), "observed-server-id").unwrap();
        assert_eq!(
            database.metadata().unwrap(),
            CacheFileMetadata {
                server_id: "observed-server-id".to_owned(),
                schema_version: SCHEMA_VERSION,
            }
        );
        let reader = database.open_reader().unwrap();
        let tables: Vec<String> = reader
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            tables,
            [
                "album_artists",
                "album_external_ids",
                "album_favorites",
                "album_provider_mappings",
                "album_read_items",
                "album_read_states",
                "album_track_read_items",
                "album_tracks",
                "albums",
                "artist_album_read_items",
                "artist_external_ids",
                "artist_favorites",
                "artist_library_entries",
                "artist_provider_mappings",
                "artist_read_items",
                "artist_read_states",
                "artist_track_read_items",
                "artists",
                "cache_file_metadata",
                "playlist_external_ids",
                "playlist_favorites",
                "playlist_provider_mappings",
                "playlist_read_items",
                "playlist_read_states",
                "playlists",
                "recommendation_items",
                "recommendation_read_states",
                "recommendation_sections",
                "track_artists",
                "track_external_ids",
                "track_favorites",
                "track_list_items",
                "track_list_states",
                "track_provider_mappings",
                "track_read_states",
                "tracks",
                "users"
            ]
        );
        reader.close().unwrap();
        database.shutdown().unwrap();
        let reopened = MediaDatabase::open(directory.path(), "observed-server-id").unwrap();
        assert_eq!(reopened.metadata().unwrap().server_id, "observed-server-id");
        reopened.shutdown().unwrap();
    }

    #[test]
    fn another_server_is_rejected_without_rewriting_the_cache() {
        let directory = tempfile::tempdir().unwrap();
        MediaDatabase::open(directory.path(), "server-one")
            .unwrap()
            .shutdown()
            .unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server-two"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn artist_identity_lookup_indexes_extend_existing_cache_without_rewriting_observations() {
        let directory = tempfile::tempdir().unwrap();
        MediaDatabase::open(directory.path(), "server-one")
            .unwrap()
            .shutdown()
            .unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let connection = Connection::open(&path).unwrap();
        super::super::artist_candidates::initialize_artist_query_functions(&connection).unwrap();
        connection.execute_batch(
            "DROP INDEX artist_domain_lookup_idx; DROP INDEX artist_external_nocase_lookup_idx;
                 INSERT INTO artists(provider,item_id,name,sort_name,sort_name_known,uri_known,
                     available_known,metadata_known,position_known,date_added_known,artist_type_known,
                     provider_mappings_known,external_ids_known)
                 VALUES ('library','007','Artist','Artist',1,0,0,0,0,0,0,0,0);",
        ).unwrap();
        connection.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server-two"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let item_id: String = database
                .writer
                .query_row("SELECT item_id FROM artists", [], |row| row.get(0))
                .unwrap();
            assert_eq!(item_id, "007");
            let indexes: i64 = database.writer.query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name IN ('artist_domain_lookup_idx','artist_external_nocase_lookup_idx')",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(indexes, 2);
            database.shutdown().unwrap();
        }
    }

    fn create_artist_only_database(path: &Path) {
        let mut connection = Connection::open(path).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch(
            "CREATE TABLE cache_file_metadata (singleton INTEGER PRIMARY KEY, server_id TEXT NOT NULL) STRICT;
             INSERT INTO cache_file_metadata VALUES (1,'server-one');",
        ).unwrap();
        initialize_artist_schema(&transaction).unwrap();
        let artist: crate::domain::media::Artist = serde_json::from_value(serde_json::json!({
            "provider":"library","item_id":"007","name":"Artist","favorite":true,
            "metadata":{"description":"durable detail"}
        }))
        .unwrap();
        super::super::artists::write_artist_observation(
            &transaction,
            &artist,
            &crate::domain::identifiers::UserId("user".to_owned()),
            crate::domain::media::FieldKnowledge::Known(true),
        )
        .unwrap();
        transaction
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        transaction.pragma_update(None, "user_version", 2).unwrap();
        transaction.commit().unwrap();
        connection.close().unwrap();
    }

    #[test]
    fn album_artist_relations_schema_two_upgrade_preserves_artist_details_and_user_state() {
        let directory = tempfile::tempdir().unwrap();
        create_artist_only_database(&directory.path().join(DATABASE_FILE_NAME));
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            let observed: (String,String,bool,bool) = database.writer.query_row(
                "SELECT artists.item_id,json_extract(metadata,'$.description'),rating,is_in_library
                 FROM artists JOIN artist_favorites USING(provider,item_id)
                 JOIN artist_library_entries USING(provider,item_id) WHERE user_id='user'", [],
                |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).unwrap();
            assert_eq!(
                observed,
                ("007".to_owned(), "durable detail".to_owned(), true, true)
            );
            let albums: i64 = database
                .writer
                .query_row("SELECT COUNT(*) FROM albums", [], |row| row.get(0))
                .unwrap();
            assert_eq!(albums, 0);
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn album_artist_relations_schema_two_upgrade_rejects_other_servers_and_foreign_objects() {
        for table in [
            None,
            Some("protected"),
            Some("albums"),
            Some("sqlitex_protected"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_artist_only_database(&path);
            if let Some(table) = table {
                let connection = Connection::open(&path).unwrap();
                connection
                    .execute_batch(&format!(
                        "CREATE TABLE {table}(value TEXT); INSERT INTO {table} VALUES ('keep');"
                    ))
                    .unwrap();
                connection.close().unwrap();
            }
            let before = std::fs::read(&path).unwrap();
            let server = if table.is_some() {
                "server-one"
            } else {
                "server-two"
            };
            assert!(MediaDatabase::open(directory.path(), server).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn album_artist_relations_malformed_schema_two_is_rejected_before_schema_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        create_artist_only_database(&path);
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        connection
            .execute_batch("DROP TABLE artists; CREATE TABLE artists(provider TEXT,item_id TEXT);")
            .unwrap();
        connection.close().unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server-one"),
            Err(CacheError::ForeignFormat)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    fn create_metadata_only_database(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE cache_file_metadata (
            singleton INTEGER PRIMARY KEY CHECK (singleton=1), server_id TEXT NOT NULL
        ) STRICT; INSERT INTO cache_file_metadata VALUES (1, 'server-one');",
            )
            .unwrap();
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        connection.close().unwrap();
    }

    #[test]
    fn metadata_only_schema_upgrades_once_without_changing_server_identity() {
        let directory = tempfile::tempdir().unwrap();
        create_metadata_only_database(&directory.path().join(DATABASE_FILE_NAME));
        for _ in 0..2 {
            let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
            assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
            assert_eq!(database.metadata().unwrap().server_id, "server-one");
            let count: i64 = database
                .writer
                .query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0);
            database.shutdown().unwrap();
        }
    }

    #[test]
    fn metadata_only_upgrade_rejects_other_servers_and_unrecognized_contents() {
        for other_table in [None, Some("protected"), Some("sqlitex_protected")] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            create_metadata_only_database(&path);
            if let Some(table) = other_table {
                let connection = Connection::open(&path).unwrap();
                connection
                    .execute_batch(&format!(
                        "CREATE TABLE {table} (value TEXT); INSERT INTO {table} VALUES ('keep');"
                    ))
                    .unwrap();
                connection.close().unwrap();
            }
            let before = std::fs::read(&path).unwrap();
            let server = if other_table.is_some() {
                "server-one"
            } else {
                "server-two"
            };
            assert!(MediaDatabase::open(directory.path(), server).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn foreign_and_empty_files_are_not_adopted() {
        for is_sqlite in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(DATABASE_FILE_NAME);
            if is_sqlite {
                Connection::open(&path)
                    .unwrap()
                    .execute_batch("CREATE TABLE protected_settings (value TEXT)")
                    .unwrap();
            } else {
                std::fs::write(&path, []).unwrap();
            }
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(
                MediaDatabase::open(directory.path(), "server-one"),
                Err(CacheError::ForeignFormat)
            ));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn unsupported_schema_is_not_migrated_or_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
        database
            .writer
            .pragma_update(None, "user_version", 999)
            .unwrap();
        database.shutdown().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server-one"),
            Err(CacheError::UnsupportedSchema(999))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn reader_keeps_a_consistent_snapshot_during_writer_commit() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = MediaDatabase::open(directory.path(), "server-one").unwrap();
        database
            .writer
            .execute_batch(
                "CREATE TABLE snapshot_probe (value INTEGER NOT NULL);
             INSERT INTO snapshot_probe VALUES (1);",
            )
            .unwrap();
        let mut reader = database.open_reader().unwrap();
        let snapshot = reader.transaction().unwrap();
        let read_value = |connection: &Connection| -> i32 {
            connection
                .query_row("SELECT value FROM snapshot_probe", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(read_value(&snapshot), 1);
        let write = database.writer.transaction().unwrap();
        write
            .execute("UPDATE snapshot_probe SET value = 2", [])
            .unwrap();
        assert_eq!(read_value(&snapshot), 1);
        write.commit().unwrap();
        assert_eq!(read_value(&snapshot), 1);
        snapshot.commit().unwrap();
        assert_eq!(read_value(&reader), 2);
        assert!(reader
            .execute("UPDATE snapshot_probe SET value = 3", [])
            .is_err());
        reader.close().unwrap();
        database.shutdown().unwrap();
    }

    #[test]
    fn empty_server_id_does_not_create_files() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), " \t"),
            Err(CacheError::InvalidServerId)
        ));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn sidecar_directories_are_rejected_before_database_creation() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join(format!("{DATABASE_FILE_NAME}-wal"))).unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server-one"),
            Err(CacheError::UnsafePath(_))
        ));
        assert!(!directory.path().join(DATABASE_FILE_NAME).exists());
    }

    #[cfg(windows)]
    #[test]
    fn junction_directory_and_sidecar_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let protected = tempfile::tempdir().unwrap();
        let protected_file = protected.path().join("settings.json");
        std::fs::write(&protected_file, b"protected synthetic settings").unwrap();
        let redirected = directory.path().join("redirected");
        let sidecar = directory.path().join(format!("{DATABASE_FILE_NAME}-wal"));
        for junction in [&redirected, &sidecar] {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(junction)
                .arg(protected.path())
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }
        assert!(matches!(
            MediaDatabase::open(&redirected, "server-one"),
            Err(CacheError::UnsafePath(_))
        ));
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server-one"),
            Err(CacheError::UnsafePath(_))
        ));
        assert_eq!(
            std::fs::read(&protected_file).unwrap(),
            b"protected synthetic settings"
        );
        assert_eq!(std::fs::read_dir(protected.path()).unwrap().count(), 1);
        std::fs::remove_dir(redirected).unwrap();
        std::fs::remove_dir(sidecar).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn redirected_directory_and_sidecar_are_rejected() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let protected = tempfile::tempdir().unwrap();
        let redirected = directory.path().join("redirected");
        symlink(protected.path(), &redirected).unwrap();
        assert!(matches!(
            MediaDatabase::open(&redirected, "server-one"),
            Err(CacheError::UnsafePath(_))
        ));
        symlink(
            protected.path(),
            directory.path().join(format!("{DATABASE_FILE_NAME}-wal")),
        )
        .unwrap();
        assert!(matches!(
            MediaDatabase::open(directory.path(), "server-one"),
            Err(CacheError::UnsafePath(_))
        ));
        assert_eq!(std::fs::read_dir(protected.path()).unwrap().count(), 0);
    }
}
