//! Blocking SQLite connection setup for the media writer and independent WAL readers.
//! Opening validates the cache format and server identity before modifying existing files.
//! Incompatible Mashin cache schemas are reset; foreign or unidentified files stay untouched.
//! This module neither creates application directories nor deletes existing files.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags, Transaction};

use super::album_artist_relations::initialize_album_artist_schema;
use super::album_reads::{initialize_album_read_schema, initialize_album_track_read_schema};
use super::albums::initialize_album_details_schema;
use super::artist_reads::initialize_artist_read_schema;
use super::artists::{initialize_artist_lookup_indexes, initialize_artist_schema};
use super::catalog_sections::initialize_artist_section_schema;
use super::tracks::initialize_track_schema;

pub(super) const DATABASE_FILE_NAME: &str = "metadata.sqlite3";
const APPLICATION_ID: i32 = 0x4D534843;
const SCHEMA_VERSION: i32 = 17;
const WAL_CHECKPOINT_PAGES: i64 = 512;
const WAL_RETAINED_BYTES: i64 = 4 * 1024 * 1024;
const WAL_WARNING_BYTES: u64 = 32 * 1024 * 1024;

pub(super) struct CacheWalMaintenance {
    next_attempt: Option<Instant>,
    has_warned: bool,
}

impl CacheWalMaintenance {
    // Configuration and activity scheduling.
    pub(super) fn new() -> Self {
        Self {
            next_attempt: Some(Instant::now() + Duration::from_secs(1)),
            has_warned: false,
        }
    }

    pub(super) fn record_activity(&mut self) {
        self.next_attempt.get_or_insert_with(|| Instant::now() + Duration::from_secs(1));
    }

    pub(super) fn delay(&self) -> Option<Duration> {
        self.next_attempt
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    // Maintenance never waits for reader locks and never changes the caller's busy timeout.
    pub(super) fn maintain(&mut self, connection: &Connection, area: &str) {
        let logger = crate::logging::Logger::new("Cache.WAL");
        logger.trace(format_args!(
            "area={area} stage=started autocommit={} txn_state={:?}",
            connection.is_autocommit(),
            connection.transaction_state(Some(rusqlite::MAIN_DB))
        ));
        match self.checkpoint(connection, area) {
            Ok((is_pending, bytes, frames, checkpointed)) => {
                self.next_attempt = is_pending.then(|| Instant::now() + Duration::from_secs(5));
                if bytes >= WAL_WARNING_BYTES && !self.has_warned {
                    logger.warn(format_args!("area={area} wal_bytes={bytes} frames={frames} checkpointed={checkpointed} retry_pending={is_pending}"));
                    self.has_warned = true;
                } else if bytes < WAL_WARNING_BYTES {
                    self.has_warned = false;
                }
                logger.trace(format_args!("area={area} checkpointed={checkpointed} frames={frames} wal_bytes={bytes} retry_pending={is_pending}"));
            }
            Err(error) => {
                self.next_attempt = Some(Instant::now() + Duration::from_secs(5));
                if !self.has_warned {
                    logger.warn(format_args!(
                        "area={area} checkpoint_failed={error} retry_pending=true"
                    ));
                    self.has_warned = true;
                }
            }
        }
    }

    // SQLite owns checkpointing and truncation; file metadata is used only for the size threshold.
    fn checkpoint(
        &self,
        connection: &Connection,
        area: &str,
    ) -> Result<(bool, u64, i64, i64), CacheError> {
        let logger = crate::logging::Logger::new("Cache.WAL");
        let started = Instant::now();
        let busy_timeout: i64 =
            connection.pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
        connection.busy_timeout(Duration::ZERO)?;
        let result = (|| {
            let (busy, frames, checkpointed): (i64, i64, i64) =
                connection.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
            logger.trace(format_args!("area={area} stage=passive busy={busy} frames={frames} checkpointed={checkpointed} elapsed_ms={}", started.elapsed().as_millis()));
            let wal_path = format!(
                "{}-wal",
                connection.path().ok_or(CacheError::WorkerUnavailable)?
            );
            let bytes = match std::fs::metadata(&wal_path) {
                Ok(metadata) => metadata.len(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(error) => return Err(error.into()),
            };
            if frames != checkpointed {
                return Ok((true, bytes, frames, checkpointed));
            }
            if bytes > WAL_RETAINED_BYTES as u64 {
                let (busy, remaining, transferred): (i64, i64, i64) =
                    connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })?;
                logger.trace(format_args!("area={area} stage=truncate busy={busy} frames={remaining} checkpointed={transferred} elapsed_ms={}", started.elapsed().as_millis()));
                return Ok((
                    busy != 0,
                    std::fs::metadata(&wal_path)?.len(),
                    frames,
                    checkpointed,
                ));
            }
            Ok((false, bytes, frames, checkpointed))
        })();
        let restored = connection.pragma_update(None, "busy_timeout", busy_timeout);
        restored?;
        result
    }
}

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
    ForeignCacheFile(PathBuf),
    Json(Arc<serde_json::Error>),
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
            Self::ForeignCacheFile(path) => {
                write!(formatter, "Unexpected file in cache: {}", path.display())
            }
            Self::Json(error) => write!(formatter, "Invalid cache metadata: {error}"),
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
    // Connection initialization and lifecycle.
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
        }
        match validate_identity(&writer, server_id) {
            Err(CacheError::UnsupportedSchema(_)) => {
                let storage =
                    super::storage::CacheStorage::open_existing_cache_directory(&directory)?;
                if storage.read_server_id()?.as_deref() != Some(server_id) {
                    return Err(CacheError::ServerMismatch);
                }
                writer
                    .close()
                    .map_err(|(_, error)| CacheError::from(error))?;
                storage.clear_database_files()?;
                return Self::open(&directory, server_id);
            }
            result => result?,
        }
        super::artist_candidates::initialize_artist_query_functions(&writer)?;
        let transaction = writer.transaction()?;
        initialize_artist_lookup_indexes(&transaction)?;
        initialize_artist_query_indexes(&transaction)?;
        transaction.commit()?;
        writer.pragma_update(None, "foreign_keys", true)?;
        writer.pragma_update(None, "locking_mode", "NORMAL")?;
        configure_cache_wal(&writer)?;
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

// WAL configuration.
/// Bounds retained WAL space without blocking readers or limiting active transactions.
pub(super) fn configure_cache_wal(connection: &Connection) -> Result<(), CacheError> {
    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(CacheError::ForeignFormat);
    }
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "wal_autocheckpoint", WAL_CHECKPOINT_PAGES)?;
    connection.pragma_update(None, "journal_size_limit", WAL_RETAINED_BYTES)?;
    Ok(())
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
    super::providers::initialize_provider_schema(transaction)?;
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
    if read_metadata(connection)?.server_id != server_id {
        return Err(CacheError::ServerMismatch);
    }
    if schema_version != SCHEMA_VERSION {
        return Err(CacheError::UnsupportedSchema(schema_version));
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
                "provider_instance_reads",
                "provider_instances",
                "provider_manifest_icons",
                "provider_manifest_reads",
                "provider_manifests",
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
    fn incompatible_owned_media_schema_is_reset_without_touching_images_or_server_identity() {
        let application = tempfile::tempdir().unwrap();
        let storage = super::super::storage::CacheStorage::open(application.path()).unwrap();
        storage.write_server_id("server-one").unwrap();
        let directory = storage.database_directory();
        let image = storage.image_directory().join("preserved-image");
        std::fs::write(&image, b"preserved image area").unwrap();
        let database = MediaDatabase::open(&directory, "server-one").unwrap();
        database
            .writer
            .execute("INSERT INTO users(user_id) VALUES('observed-user')", [])
            .unwrap();
        database
            .writer
            .pragma_update(None, "user_version", 999)
            .unwrap();
        database.shutdown().unwrap();
        let path = directory.join(DATABASE_FILE_NAME);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            MediaDatabase::open(&directory, "other-server"),
            Err(CacheError::ServerMismatch)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let database = MediaDatabase::open(&directory, "server-one").unwrap();
        assert_eq!(database.metadata().unwrap().schema_version, SCHEMA_VERSION);
        assert_eq!(
            database
                .writer
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM users", [], |row| row.get(0))
                .unwrap(),
            0
        );
        assert_eq!(std::fs::read(&image).unwrap(), b"preserved image area");
        assert_eq!(
            storage.read_server_id().unwrap().as_deref(),
            Some("server-one")
        );
        database.shutdown().unwrap();
    }

    #[test]
    fn unsupported_schema_without_server_manifest_is_not_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
        database
            .writer
            .pragma_update(None, "user_version", 999)
            .unwrap();
        database.shutdown().unwrap();
        let path = directory.path().join(DATABASE_FILE_NAME);
        let before = std::fs::read(&path).unwrap();
        assert!(MediaDatabase::open(directory.path(), "server-one").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn wal_activity_cannot_postpone_due_maintenance_or_blocked_retries() {
        let mut maintenance = CacheWalMaintenance::new();
        let deadline = Instant::now() - Duration::from_secs(1);
        maintenance.next_attempt = Some(deadline);
        for _ in 0..100 { maintenance.record_activity(); }
        assert_eq!(maintenance.next_attempt, Some(deadline));
        assert_eq!(maintenance.delay(), Some(Duration::ZERO));
        let retry = Instant::now() + Duration::from_secs(5);
        maintenance.next_attempt = Some(retry);
        maintenance.record_activity();
        assert_eq!(maintenance.next_attempt, Some(retry));
        maintenance.next_attempt = None;
        maintenance.record_activity();
        assert!(maintenance.delay().is_some());
    }

    #[test]
    fn idle_wal_maintenance_retries_without_another_write_and_preserves_reader_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
        database
            .writer
            .execute_batch(
                "CREATE TABLE idle_wal_probe(value BLOB NOT NULL);
             INSERT INTO idle_wal_probe VALUES(zeroblob(262144));
             PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        let mut reader = database.open_reader().unwrap();
        let snapshot = reader.transaction().unwrap();
        let read_byte = |connection: &Connection| -> Vec<u8> {
            connection
                .query_row("SELECT substr(value,1,1) FROM idle_wal_probe", [], |row| {
                    row.get(0)
                })
                .unwrap()
        };
        assert_eq!(read_byte(&snapshot), vec![0]);
        for revision in 1..=32_u8 {
            database
                .writer
                .execute(
                    "UPDATE idle_wal_probe SET value=?1",
                    [vec![revision; 262144]],
                )
                .unwrap();
        }
        let mut maintenance = CacheWalMaintenance::new();
        let started = Instant::now();
        maintenance.maintain(&database.writer, "test");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(maintenance.delay().is_some());
        assert_eq!(read_byte(&snapshot), vec![0]);
        snapshot.commit().unwrap();
        maintenance.maintain(&database.writer, "test");
        assert!(maintenance.delay().is_none());
        assert_eq!(
            std::fs::metadata(directory.path().join(format!("{DATABASE_FILE_NAME}-wal")))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(read_byte(&reader), vec![32]);
        let timeout: i64 = database
            .writer
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 5000);
        maintenance.record_activity();
        assert!(maintenance.delay().is_some());
        reader.close().unwrap();
        database.shutdown().unwrap();
    }

    #[test]
    fn idle_wal_maintenance_does_not_wait_for_a_reader_blocking_truncation() {
        let directory = tempfile::tempdir().unwrap();
        let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
        database
            .writer
            .pragma_update(None, "wal_autocheckpoint", 0)
            .unwrap();
        database
            .writer
            .execute_batch(
                "CREATE TABLE idle_truncate_probe(value BLOB NOT NULL);
             INSERT INTO idle_truncate_probe VALUES(zeroblob(8388608));",
            )
            .unwrap();
        let mut reader = database.open_reader().unwrap();
        let snapshot = reader.transaction().unwrap();
        let length: i64 = snapshot
            .query_row("SELECT length(value) FROM idle_truncate_probe", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(length, 8388608);
        let mut maintenance = CacheWalMaintenance::new();
        let started = Instant::now();
        maintenance.maintain(&database.writer, "test");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(maintenance.delay().is_some());
        snapshot.commit().unwrap();
        maintenance.maintain(&database.writer, "test");
        assert!(maintenance.delay().is_none());
        assert_eq!(
            std::fs::metadata(directory.path().join(format!("{DATABASE_FILE_NAME}-wal")))
                .unwrap()
                .len(),
            0
        );
        reader.close().unwrap();
        database.shutdown().unwrap();
    }

    #[test]
    fn wal_reclaims_retained_space_after_a_pinned_reader_finishes() {
        let directory = tempfile::tempdir().unwrap();
        let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
        let checkpoint_pages: i64 = database
            .writer
            .pragma_query_value(None, "wal_autocheckpoint", |row| row.get(0))
            .unwrap();
        let retained_bytes: i64 = database
            .writer
            .pragma_query_value(None, "journal_size_limit", |row| row.get(0))
            .unwrap();
        assert_eq!(checkpoint_pages, WAL_CHECKPOINT_PAGES);
        assert_eq!(retained_bytes, WAL_RETAINED_BYTES);
        database
            .writer
            .execute_batch(
                "CREATE TABLE wal_probe (value BLOB NOT NULL);
             INSERT INTO wal_probe VALUES(zeroblob(262144));
             PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        let mut reader = database.open_reader().unwrap();
        let snapshot = reader.transaction().unwrap();
        let read_first_byte = |connection: &Connection| -> Vec<u8> {
            connection
                .query_row("SELECT substr(value,1,1) FROM wal_probe", [], |row| {
                    row.get(0)
                })
                .unwrap()
        };
        assert_eq!(read_first_byte(&snapshot), vec![0]);
        for revision in 1..=32_u8 {
            database
                .writer
                .execute("UPDATE wal_probe SET value=?1", [vec![revision; 262144]])
                .unwrap();
        }
        let wal_path = directory.path().join(format!("{DATABASE_FILE_NAME}-wal"));
        assert!(std::fs::metadata(&wal_path).unwrap().len() > WAL_RETAINED_BYTES as u64);
        assert_eq!(read_first_byte(&snapshot), vec![0]);
        snapshot.commit().unwrap();
        for revision in 33..=48_u8 {
            database
                .writer
                .execute("UPDATE wal_probe SET value=?1", [vec![revision; 262144]])
                .unwrap();
        }
        assert!(std::fs::metadata(&wal_path).unwrap().len() <= WAL_RETAINED_BYTES as u64);
        assert_eq!(read_first_byte(&reader), vec![48]);
        reader.close().unwrap();
        database.shutdown().unwrap();
        let database = MediaDatabase::open(directory.path(), "server-one").unwrap();
        assert_eq!(read_first_byte(&database.writer), vec![48]);
        database.shutdown().unwrap();
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
