//! Blocking source/variant index and file publication, independent of the media writer.
//! The caller must retain exclusive cache ownership and quiesce this owner before clearing.
//! No network work or productive activation belongs here; files precede atomic SQL commits.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cap_std::fs::OpenOptions;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use super::database::{validate_file, CacheError};
use super::image_files::{
    ImageFileLease, ImageFileLeases, ImageFileRecord, ImageFiles, ImagePayload,
};
use super::image_sources::{ImageSourceKey, ImageVariantKey};
use super::storage::{CacheStorage, IMAGE_DATABASE_FILE_NAME};

const APPLICATION_ID: i32 = 0x4D534849;
const SCHEMA_VERSION: i32 = 3;
const NOT_FOUND_FRESH_FOR: Duration = Duration::from_secs(30);
const READ_VARIANT_SQL: &str = "
SELECT variant.content_hash,variant.mime_type,variant.byte_size,
             source.revision,variant.source_revision,variant.observed_at
FROM image_variants AS variant JOIN image_sources AS source
    USING(source_kind,provider,reference)
WHERE variant.source_kind=?1 AND variant.provider=?2 AND variant.reference=?3
    AND variant.size=?4 AND variant.output_format=?5";
const SCHEMA_METADATA: &str = "
CREATE TABLE image_cache_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    server_id TEXT NOT NULL CHECK (length(trim(server_id))>0)
) STRICT;";
const SCHEMA_IMAGES: &str = "
CREATE TABLE image_sources (
    source_kind TEXT NOT NULL CHECK (source_kind IN ('proxy','reference','inline')),
    provider TEXT NOT NULL,
    reference TEXT NOT NULL CHECK (length(trim(reference))>0),
    revision INTEGER NOT NULL CHECK (revision>0),
    observed_at INTEGER NOT NULL CHECK (observed_at>=0),
    PRIMARY KEY (source_kind,provider,reference),
    CHECK ((source_kind='proxy' AND provider='' AND length(reference)=64
            AND reference NOT GLOB '*[^0-9a-fA-F]*')
        OR (source_kind IN ('reference','inline') AND length(trim(provider))>0))
) STRICT, WITHOUT ROWID;
CREATE TABLE image_variants (
    source_kind TEXT NOT NULL,
    provider TEXT NOT NULL,
    reference TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size IN (0,80,160,256,512,1024)),
    output_format TEXT NOT NULL CHECK (output_format IN ('','jpg','jpeg','png','svg')),
    source_revision INTEGER NOT NULL CHECK (source_revision>0),
    observed_at INTEGER NOT NULL CHECK (observed_at>=0),
    content_hash TEXT NOT NULL CHECK (length(content_hash)=64 AND content_hash NOT GLOB '*[^0-9a-f]*'),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png','image/jpeg','image/webp','image/gif')),
    byte_size INTEGER NOT NULL CHECK (byte_size>0),
    PRIMARY KEY (source_kind,provider,reference,size,output_format),
    FOREIGN KEY (source_kind,provider,reference)
        REFERENCES image_sources(source_kind,provider,reference) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;";

const SCHEMA_REFRESH: &str = "
CREATE TABLE image_refresh_states (
    source_kind TEXT NOT NULL,
    provider TEXT NOT NULL,
    reference TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size IN (0,80,160,256,512,1024)),
    output_format TEXT NOT NULL,
    source_revision INTEGER CHECK (source_revision>0),
    outcome TEXT NOT NULL CHECK (outcome IN ('found','not_found')),
    observed_at INTEGER NOT NULL CHECK (observed_at>=0),
    PRIMARY KEY (source_kind,provider,reference,size,output_format)
) STRICT, WITHOUT ROWID;";

pub(super) struct ImageIndex {
    connection: Connection,
    files: ImageFiles,
    refresh_observations: HashMap<ImageVariantKey, ImageRefreshObservation>,
}

pub(super) struct ImageRefreshState {
    pub(super) cached: Option<ImageVariantLease>,
    pub(super) source_revision: Option<i64>,
    pub(super) confirmed_at: Option<i64>,
    pub(super) is_recently_not_found: bool,
}

enum ImageRefreshObservation {
    Found {
        source_revision: i64,
        confirmed_at: i64,
    },
    NotFound {
        source_revision: Option<i64>,
        observed_at: Instant,
    },
}

pub(super) struct ImageVariantRead {
    pub(super) payload: ImagePayload,
    pub(super) source_revision: i64,
    pub(super) variant_revision: i64,
    pub(super) observed_at: i64,
}

pub(super) struct ImageVariantLease {
    pub(super) read: ImageVariantRead,
    _file: ImageFileLease,
}

impl ImageVariantRead {
    // Superseded variants remain usable until their own successful replacement.
    pub(super) fn is_current_source(&self) -> bool {
        self.source_revision == self.variant_revision
    }
}

struct StoredVariant {
    file: ImageFileRecord,
    source_revision: i64,
    variant_revision: i64,
    observed_at: i64,
}

impl ImageIndex {
    // Blocking setup validates existing identity before changing connection pragmas or data.
    pub(super) fn open(storage: &CacheStorage, server_id: &str) -> Result<Self, CacheError> {
        if server_id.trim().is_empty() {
            return Err(CacheError::InvalidServerId);
        }
        match storage.read_server_id()? {
            Some(selected) if selected == server_id => {}
            Some(_) => return Err(CacheError::ServerMismatch),
            None => return Err(CacheError::CacheInactive),
        }
        let directory = storage.open_image_directory()?;
        let path = storage.image_directory().join(IMAGE_DATABASE_FILE_NAME);
        for suffix in ["", "-wal", "-shm", "-journal"] {
            validate_file(
                &storage
                    .image_directory()
                    .join(format!("{IMAGE_DATABASE_FILE_NAME}{suffix}")),
            )?;
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let is_new = match directory.open_with(IMAGE_DATABASE_FILE_NAME, &options) {
            Ok(file) => {
                drop(file);
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error.into()),
        };
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_secs(5))?;
        if is_new {
            let transaction = connection.transaction()?;
            transaction.execute_batch(SCHEMA_METADATA)?;
            transaction.execute_batch(SCHEMA_IMAGES)?;
            transaction.execute_batch(SCHEMA_REFRESH)?;
            transaction.execute("INSERT INTO image_cache_metadata VALUES(1,?1)", [server_id])?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        }
        match validate_identity(&connection, server_id) {
            Err(CacheError::UnsupportedSchema(_)) => {
                connection
                    .close()
                    .map_err(|(_, error)| CacheError::from(error))?;
                drop(directory);
                storage.clear_image_files()?;
                return Self::open(storage, server_id);
            }
            result => result?,
        }
        connection.pragma_update(None, "foreign_keys", true)?;
        let journal_mode: String =
            connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(CacheError::ForeignFormat);
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        let files = ImageFiles::open(storage)?;
        files.clear_incomplete_publications()?;
        Ok(Self {
            connection,
            files,
            refresh_observations: HashMap::new(),
        })
    }

    // Explicit close must finish before the lifecycle owner deletes image/index files.
    pub(super) fn shutdown(self) -> Result<(), CacheError> {
        self.connection.close().map_err(|(_, error)| error.into())
    }

    // Publication failure leaves the prior index intact; SQL failure may leave only orphan bytes.
    pub(super) fn publish_variant(
        &mut self,
        key: &ImageVariantKey,
        bytes: Vec<u8>,
        observed_at: i64,
    ) -> Result<(), CacheError> {
        let (source_kind, provider, reference) = source_parts(&key.source)?;
        if observed_at < 0 {
            return Err(CacheError::InvalidImageObservation(
                "invalid image observation time",
            ));
        }
        let file = self.files.publish(bytes)?;
        let byte_size = i64::try_from(file.byte_size).map_err(|_| {
            CacheError::InvalidImageObservation("image file size exceeds SQL integer range")
        })?;
        let transaction = self.connection.transaction()?;
        let previous = read_stored_variant(&transaction, key)?;
        if let Some(previous) = previous.as_ref().filter(|previous| {
            previous.file == file && previous.source_revision == previous.variant_revision
        }) {
            let source_revision = previous.source_revision;
            write_refresh_observation(
                &transaction,
                key,
                Some(source_revision),
                "found",
                observed_at,
            )?;
            transaction.commit()?;
            self.confirm_variant(key, source_revision, observed_at);
            return Ok(());
        }
        let previous_revision: Option<i64> = transaction.query_row(
            "SELECT revision FROM image_sources WHERE source_kind=?1 AND provider=?2 AND reference=?3",
            params![source_kind, provider, reference], |row| row.get(0),
        ).optional()?;
        let changed_content = previous
            .as_ref()
            .is_some_and(|previous| previous.file != file);
        let revision = match previous_revision {
            Some(revision) if changed_content => {
                revision
                    .checked_add(1)
                    .ok_or(CacheError::InvalidImageObservation(
                        "image source revision overflow",
                    ))?
            }
            Some(revision) => revision,
            None => 1,
        };
        transaction.execute(
            "INSERT INTO image_sources(source_kind,provider,reference,revision,observed_at)
             VALUES(?1,?2,?3,?4,?5) ON CONFLICT(source_kind,provider,reference) DO UPDATE
             SET revision=excluded.revision, observed_at=excluded.observed_at
             WHERE image_sources.revision IS NOT excluded.revision",
            params![source_kind, provider, reference, revision, observed_at],
        )?;
        transaction.execute(
            "INSERT INTO image_variants(source_kind,provider,reference,size,output_format,
                source_revision,observed_at,content_hash,mime_type,byte_size)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(source_kind,provider,reference,size,output_format) DO UPDATE SET
                source_revision=excluded.source_revision, observed_at=excluded.observed_at,
                content_hash=excluded.content_hash, mime_type=excluded.mime_type, byte_size=excluded.byte_size",
            params![source_kind, provider, reference, key.parameters.size.pixels(),
                key.parameters.output_format.proxy_parameter().unwrap_or(""), revision, observed_at,
                file.content_hash, file.mime_type, byte_size],
        )?;
        write_refresh_observation(&transaction, key, Some(revision), "found", observed_at)?;
        transaction.commit()?;
        self.confirm_variant(key, revision, observed_at);
        Ok(())
    }

    // Refresh metadata never changes image content rows or files.
    pub(super) fn record_not_found(
        &mut self,
        key: &ImageVariantKey,
        expected_source_revision: Option<i64>,
        observed_at: Instant,
    ) -> Result<(), CacheError> {
        let source_revision = read_source_revision(&self.connection, &key.source)?;
        if source_revision == expected_source_revision {
            write_refresh_observation(
                &self.connection,
                key,
                source_revision,
                "not_found",
                chrono::Utc::now().timestamp(),
            )?;
            self.refresh_observations.insert(
                key.clone(),
                ImageRefreshObservation::NotFound {
                    source_revision,
                    observed_at,
                },
            );
        }
        Ok(())
    }

    pub(super) fn read_refresh_state(
        &mut self,
        key: &ImageVariantKey,
        now: Instant,
    ) -> Result<ImageRefreshState, CacheError> {
        let cached = self.lease_variant(key)?;
        let source_revision = read_source_revision(&self.connection, &key.source)?;
        if !self.refresh_observations.contains_key(key) {
            if let Some(observation) = read_refresh_observation(&self.connection, key, now)? {
                self.refresh_observations.insert(key.clone(), observation);
            }
        }
        let (confirmed_at, is_recently_not_found) = match self.refresh_observations.get(key) {
            Some(ImageRefreshObservation::Found {
                source_revision: confirmed_revision,
                confirmed_at,
            }) if Some(*confirmed_revision) == source_revision
                && cached
                    .as_ref()
                    .is_some_and(|cached| cached.read.is_current_source()) =>
            {
                (Some(*confirmed_at), false)
            }
            Some(ImageRefreshObservation::NotFound {
                source_revision: missing_revision,
                observed_at,
            }) if *missing_revision == source_revision
                && now
                    .checked_duration_since(*observed_at)
                    .is_some_and(|age| age < NOT_FOUND_FRESH_FOR) =>
            {
                (None, true)
            }
            _ => (None, false),
        };
        Ok(ImageRefreshState {
            cached,
            source_revision,
            confirmed_at,
            is_recently_not_found,
        })
    }

    // Point lookup and file verification never reconstruct a media or image inventory.
    pub(super) fn read_variant(
        &self,
        key: &ImageVariantKey,
    ) -> Result<Option<ImageVariantRead>, CacheError> {
        Ok(self.lease_variant(key)?.map(|lease| lease.read))
    }

    /// Retains the verified file handle until the consumer drops the returned lease.
    pub(super) fn lease_variant(
        &self,
        key: &ImageVariantKey,
    ) -> Result<Option<ImageVariantLease>, CacheError> {
        let Some(stored) = read_stored_variant(&self.connection, key)? else {
            return Ok(None);
        };
        Ok(self
            .files
            .read_leased(&stored.file)?
            .map(|file| ImageVariantLease {
                read: ImageVariantRead {
                    payload: file.payload,
                    source_revision: stored.source_revision,
                    variant_revision: stored.variant_revision,
                    observed_at: stored.observed_at,
                },
                _file: file.lease,
            }))
    }

    pub(super) fn file_leases(&self) -> ImageFileLeases {
        self.files.file_leases()
    }

    // The last successful publication confirms freshness, even after a wall-clock correction.
    fn confirm_variant(&mut self, key: &ImageVariantKey, source_revision: i64, confirmed_at: i64) {
        self.refresh_observations.insert(
            key.clone(),
            ImageRefreshObservation::Found {
                source_revision,
                confirmed_at,
            },
        );
    }
}

// Persistence mappings and format validation.
fn write_refresh_observation(
    connection: &Connection,
    key: &ImageVariantKey,
    source_revision: Option<i64>,
    outcome: &str,
    observed_at: i64,
) -> Result<(), CacheError> {
    let (source_kind, provider, reference) = source_parts(&key.source)?;
    connection.execute(
        "INSERT INTO image_refresh_states(source_kind,provider,reference,size,output_format,source_revision,outcome,observed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(source_kind,provider,reference,size,output_format)
         DO UPDATE SET source_revision=excluded.source_revision,outcome=excluded.outcome,observed_at=excluded.observed_at
         WHERE image_refresh_states.source_revision IS NOT excluded.source_revision
            OR image_refresh_states.outcome IS NOT excluded.outcome
            OR image_refresh_states.observed_at IS NOT excluded.observed_at",
        params![source_kind, provider, reference, key.parameters.size.pixels(),
            key.parameters.output_format.proxy_parameter().unwrap_or(""), source_revision, outcome, observed_at],
    )?;
    Ok(())
}

fn read_refresh_observation(
    connection: &Connection,
    key: &ImageVariantKey,
    now: Instant,
) -> Result<Option<ImageRefreshObservation>, CacheError> {
    let (source_kind, provider, reference) = source_parts(&key.source)?;
    let stored: Option<(Option<i64>, String, i64)> = connection
        .query_row(
            "SELECT source_revision,outcome,observed_at FROM image_refresh_states
         WHERE source_kind=?1 AND provider=?2 AND reference=?3 AND size=?4 AND output_format=?5",
            params![
                source_kind,
                provider,
                reference,
                key.parameters.size.pixels(),
                key.parameters.output_format.proxy_parameter().unwrap_or("")
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((revision, outcome, observed_at)) = stored else {
        return Ok(None);
    };
    match (outcome.as_str(), revision) {
        ("found", Some(source_revision)) => Ok(Some(ImageRefreshObservation::Found {
            source_revision,
            confirmed_at: observed_at,
        })),
        ("not_found", _) => {
            let age = chrono::Utc::now()
                .timestamp()
                .checked_sub(observed_at)
                .and_then(|age| u64::try_from(age).ok());
            Ok(age
                .filter(|age| Duration::from_secs(*age) < NOT_FOUND_FRESH_FOR)
                .and_then(|age| now.checked_sub(Duration::from_secs(age)))
                .map(|observed_at| ImageRefreshObservation::NotFound {
                    source_revision: revision,
                    observed_at,
                }))
        }
        _ => Ok(None),
    }
}

fn read_source_revision(
    connection: &Connection,
    source: &ImageSourceKey,
) -> Result<Option<i64>, CacheError> {
    let (source_kind, provider, reference) = source_parts(source)?;
    Ok(connection.query_row(
        "SELECT revision FROM image_sources WHERE source_kind=?1 AND provider=?2 AND reference=?3",
        params![source_kind, provider, reference], |row| row.get(0),
    ).optional()?)
}

fn source_parts(source: &ImageSourceKey) -> Result<(&str, String, String), CacheError> {
    match source {
        ImageSourceKey::ProxyId(proxy_id) => {
            if proxy_id.len() != 64 || !proxy_id.bytes().all(|value| value.is_ascii_hexdigit()) {
                return Err(CacheError::InvalidImageObservation("invalid proxy ID"));
            }
            Ok(("proxy", String::new(), proxy_id.clone()))
        }
        ImageSourceKey::ModelReference { provider, path } => {
            if provider.as_str().trim().is_empty() || path.trim().is_empty() {
                return Err(CacheError::InvalidImageObservation(
                    "missing image reference",
                ));
            }
            let trimmed = path.trim_start().to_ascii_lowercase();
            if trimmed.starts_with("data:") || trimmed.starts_with('<') {
                return Err(CacheError::InvalidImageObservation(
                    "inline image persistence requires an observed proxy ID",
                ));
            }
            if let Ok(url) = reqwest::Url::parse(path) {
                if !url.username().is_empty()
                    || url.password().is_some()
                    || url.query_pairs().any(|(name, _)| {
                        matches!(
                            name.to_ascii_lowercase().as_str(),
                            "token"
                                | "access_token"
                                | "api_key"
                                | "apikey"
                                | "auth"
                                | "authorization"
                                | "password"
                                | "sig"
                                | "signature"
                                | "x-amz-credential"
                                | "x-amz-signature"
                        )
                    })
                {
                    return Err(CacheError::InvalidImageObservation(
                        "image reference contains credentials",
                    ));
                }
            }
            Ok(("reference", provider.as_str().to_owned(), path.clone()))
        }
        ImageSourceKey::ProviderIconReference { provider, role } => {
            super::image_sources::validate_provider_icon_reference(provider, role)?;
            let reference = serde_json::to_string(&serde_json::json!({"providerIcon": role}))
                .map_err(|_| {
                    CacheError::InvalidImageObservation("invalid provider icon reference")
                })?;
            Ok(("inline", provider.as_str().to_owned(), reference))
        }
        ImageSourceKey::InlineReference {
            provider,
            owner_uri,
            image_role,
            position,
        } => {
            super::image_sources::validate_inline_reference(provider, owner_uri, image_role)?;
            let reference =
                serde_json::to_string(&(owner_uri, image_role, position)).map_err(|_| {
                    CacheError::InvalidImageObservation("invalid inline model reference")
                })?;
            Ok(("inline", provider.as_str().to_owned(), reference))
        }
    }
}

fn read_stored_variant(
    connection: &Connection,
    key: &ImageVariantKey,
) -> Result<Option<StoredVariant>, CacheError> {
    let (source_kind, provider, reference) = source_parts(&key.source)?;
    Ok(connection
        .query_row(
            READ_VARIANT_SQL,
            params![
                source_kind,
                provider,
                reference,
                key.parameters.size.pixels(),
                key.parameters.output_format.proxy_parameter().unwrap_or("")
            ],
            |row| {
                Ok(StoredVariant {
                    file: ImageFileRecord {
                        content_hash: row.get(0)?,
                        mime_type: row.get(1)?,
                        byte_size: u64::try_from(row.get::<_, i64>(2)?).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                2,
                                rusqlite::types::Type::Integer,
                                Box::new(error),
                            )
                        })?,
                    },
                    source_revision: row.get(3)?,
                    variant_revision: row.get(4)?,
                    observed_at: row.get(5)?,
                })
            },
        )
        .optional()?)
}

fn validate_identity(connection: &Connection, server_id: &str) -> Result<(), CacheError> {
    let application_id: i32 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(CacheError::ForeignFormat);
    }
    let version = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let selected: String = connection.query_row(
        "SELECT server_id FROM image_cache_metadata WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    if selected != server_id {
        return Err(CacheError::ServerMismatch);
    }
    if version != SCHEMA_VERSION {
        return Err(CacheError::UnsupportedSchema(version));
    }
    let objects = connection
        .prepare(
            "SELECT name,type FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
        )?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let expected: Vec<_> = vec![
        "image_cache_metadata",
        "image_refresh_states",
        "image_sources",
        "image_variants",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), "table".to_owned()))
    .collect();
    if objects != expected {
        return Err(CacheError::ForeignFormat);
    }
    connection.prepare(
        "SELECT source_kind,provider,reference,revision,observed_at FROM image_sources LIMIT 0",
    )?;
    connection.prepare("SELECT source_kind,provider,reference,size,output_format,source_revision,observed_at,content_hash,mime_type,byte_size FROM image_variants LIMIT 0")?;
    if version == SCHEMA_VERSION {
        connection.prepare("SELECT source_kind,provider,reference,size,output_format,source_revision,outcome,observed_at FROM image_refresh_states LIMIT 0")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::image_files::tests::bitmap;
    use super::super::image_sources::{ImageOutputFormat, ImageSize, ImageVariantParameters};
    use super::*;

    fn key(proxy_id: &str, size: ImageSize) -> ImageVariantKey {
        ImageVariantKey {
            source: ImageSourceKey::ProxyId(proxy_id.repeat(64)),
            parameters: ImageVariantParameters {
                size,
                output_format: ImageOutputFormat::JpegPreserveTransparency,
            },
        }
    }

    fn storage(application: &std::path::Path) -> CacheStorage {
        let storage = CacheStorage::open(application).unwrap();
        storage.write_server_id("server").unwrap();
        storage
    }

    #[test]
    fn incompatible_owned_image_schema_is_reset_without_touching_media_files() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let media = storage
            .database_directory()
            .join(super::super::database::DATABASE_FILE_NAME);
        std::fs::write(&media, b"preserved media area").unwrap();
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        let selected = key("a", ImageSize::Pixels80);
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        index
            .connection
            .pragma_update(None, "user_version", 0)
            .unwrap();
        index.shutdown().unwrap();
        assert!(ImageIndex::open(&storage, "other").is_err());
        let reopened = ImageIndex::open(&storage, "server").unwrap();
        assert!(reopened.read_variant(&selected).unwrap().is_none());
        assert_eq!(std::fs::read(media).unwrap(), b"preserved media area");
        reopened.shutdown().unwrap();
    }

    #[test]
    fn inline_model_slots_persist_without_payload_keys_after_reopen() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        let proxy = key("a", ImageSize::Pixels80);
        index.publish_variant(&proxy, bitmap(1), 1).unwrap();
        index.shutdown().unwrap();
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        assert_eq!(
            index
                .connection
                .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            SCHEMA_VERSION
        );
        assert_eq!(
            index.read_variant(&proxy).unwrap().unwrap().payload.bytes,
            bitmap(1)
        );
        let mut inline = proxy.clone();
        inline.source = ImageSourceKey::InlineReference {
            provider: crate::domain::identifiers::ProviderInstanceId("builtin".into()),
            owner_uri: "library://album/7".into(),
            image_role: "thumb".into(),
            position: 0,
        };
        index.publish_variant(&inline, bitmap(2), 2).unwrap();
        let mut sibling = inline.clone();
        if let ImageSourceKey::InlineReference { position, .. } = &mut sibling.source {
            *position = 1;
        }
        assert!(index.read_variant(&sibling).unwrap().is_none());
        let reference: String = index
            .connection
            .query_row(
                "SELECT reference FROM image_sources WHERE source_kind='inline'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<(String, String, u32)>(&reference).unwrap(),
            ("library://album/7".into(), "thumb".into(), 0)
        );
        index.shutdown().unwrap();
        let index = ImageIndex::open(&storage, "server").unwrap();
        assert_eq!(
            index.read_variant(&inline).unwrap().unwrap().payload.bytes,
            bitmap(2)
        );
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn failed_refresh_metadata_rolls_back_and_preserves_content_after_reopen() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let selected = key("a", ImageSize::Pixels80);
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        index.shutdown().unwrap();
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        assert_eq!(
            index
                .connection
                .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            SCHEMA_VERSION
        );
        assert_eq!(
            index
                .read_variant(&selected)
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            bitmap(1)
        );
        index.publish_variant(&selected, bitmap(1), 100).unwrap();
        index.connection.execute_batch("CREATE TEMP TRIGGER reject_refresh BEFORE UPDATE ON image_refresh_states BEGIN SELECT RAISE(FAIL,'synthetic metadata failure'); END;").unwrap();
        assert!(index.publish_variant(&selected, bitmap(2), 200).is_err());
        let state = index.read_refresh_state(&selected, Instant::now()).unwrap();
        assert_eq!(state.confirmed_at, Some(100));
        assert_eq!(state.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        drop(state);
        index.shutdown().unwrap();
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        let state = index.read_refresh_state(&selected, Instant::now()).unwrap();
        assert_eq!(state.confirmed_at, Some(100));
        assert_eq!(state.cached.as_ref().unwrap().read.source_revision, 1);
        drop(state);
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn expired_future_and_superseded_durable_negative_states_do_not_suppress_refresh() {
        for offset in [-31, 60] {
            let application = tempfile::tempdir().unwrap();
            let storage = storage(application.path());
            let selected = key("a", ImageSize::Pixels80);
            let mut index = ImageIndex::open(&storage, "server").unwrap();
            index
                .record_not_found(&selected, None, Instant::now())
                .unwrap();
            index
                .connection
                .execute(
                    "UPDATE image_refresh_states SET observed_at=?1",
                    [chrono::Utc::now().timestamp() + offset],
                )
                .unwrap();
            index.shutdown().unwrap();
            let mut index = ImageIndex::open(&storage, "server").unwrap();
            assert!(
                !index
                    .read_refresh_state(&selected, Instant::now())
                    .unwrap()
                    .is_recently_not_found
            );
            index
                .record_not_found(&selected, None, Instant::now())
                .unwrap();
            let mut sibling = selected.clone();
            sibling.parameters.size = ImageSize::Pixels512;
            index.publish_variant(&sibling, bitmap(2), 1).unwrap();
            index.shutdown().unwrap();
            let mut index = ImageIndex::open(&storage, "server").unwrap();
            assert!(
                !index
                    .read_refresh_state(&selected, Instant::now())
                    .unwrap()
                    .is_recently_not_found
            );
            index.shutdown().unwrap();
            drop(storage);
            application.close().unwrap();
        }
    }

    #[test]
    fn identical_publication_persists_confirmation_without_image_content_writes() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        let selected = key("a", ImageSize::Pixels80);
        let sibling = key("a", ImageSize::Pixels512);
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        index.publish_variant(&sibling, bitmap(2), 1).unwrap();
        let changes = index.connection.total_changes();
        index.publish_variant(&selected, bitmap(1), 100).unwrap();
        index.publish_variant(&selected, bitmap(1), 50).unwrap();
        assert_eq!(index.connection.total_changes(), changes + 2);
        let changes = index.connection.total_changes();
        index.publish_variant(&selected, bitmap(1), 50).unwrap();
        assert_eq!(index.connection.total_changes(), changes);
        index.connection.execute_batch("CREATE TEMP TRIGGER reject_runtime_confirmation BEFORE UPDATE ON image_variants BEGIN SELECT RAISE(FAIL,'synthetic commit failure'); END;").unwrap();
        assert!(index.publish_variant(&selected, bitmap(9), 10000).is_err());
        index
            .connection
            .execute_batch("DROP TRIGGER reject_runtime_confirmation;")
            .unwrap();
        let state = index.read_refresh_state(&selected, Instant::now()).unwrap();
        assert_eq!(state.confirmed_at, Some(50));
        assert_eq!(state.cached.as_ref().unwrap().read.observed_at, 1);
        assert!(!state.is_recently_not_found);
        drop(state);
        let source_time: i64 = index
            .connection
            .query_row("SELECT observed_at FROM image_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(source_time, 1);
        assert_eq!(
            index
                .read_refresh_state(&sibling, Instant::now())
                .unwrap()
                .confirmed_at,
            Some(1)
        );
        index.publish_variant(&sibling, bitmap(3), 101).unwrap();
        assert_eq!(
            index
                .read_refresh_state(&selected, Instant::now())
                .unwrap()
                .confirmed_at,
            None
        );
        index.publish_variant(&selected, bitmap(1), 102).unwrap();
        assert_eq!(
            index
                .read_refresh_state(&selected, Instant::now())
                .unwrap()
                .confirmed_at,
            Some(102)
        );
        index.shutdown().unwrap();
        let mut reopened = ImageIndex::open(&storage, "server").unwrap();
        let state = reopened
            .read_refresh_state(&selected, Instant::now())
            .unwrap();
        assert_eq!(state.confirmed_at, Some(102));
        assert_eq!(state.cached.as_ref().unwrap().read.observed_at, 102);
        drop(state);
        reopened.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn not_found_observations_are_variant_scoped_expire_and_survive_restart() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        let selected = key("a", ImageSize::Pixels80);
        let now = Instant::now();
        let changes = index.connection.total_changes();
        index.record_not_found(&selected, None, now).unwrap();
        let state = index.read_refresh_state(&selected, now).unwrap();
        assert!(state.is_recently_not_found);
        assert!(state.cached.is_none());
        assert_eq!(state.source_revision, None);
        assert!(
            !index
                .read_refresh_state(&key("a", ImageSize::Pixels512), now)
                .unwrap()
                .is_recently_not_found
        );
        assert!(
            !index
                .read_refresh_state(&key("b", ImageSize::Pixels80), now)
                .unwrap()
                .is_recently_not_found
        );
        assert!(
            index
                .read_refresh_state(
                    &selected,
                    now + NOT_FOUND_FRESH_FOR - Duration::from_nanos(1)
                )
                .unwrap()
                .is_recently_not_found
        );
        assert!(
            !index
                .read_refresh_state(&selected, now + NOT_FOUND_FRESH_FOR)
                .unwrap()
                .is_recently_not_found
        );
        assert_eq!(index.connection.total_changes(), changes + 1);
        index.record_not_found(&selected, None, now).unwrap();
        index.shutdown().unwrap();
        let mut reopened = ImageIndex::open(&storage, "server").unwrap();
        assert!(
            reopened
                .read_refresh_state(&selected, now)
                .unwrap()
                .is_recently_not_found
        );
        reopened.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn successful_publication_and_source_revision_changes_invalidate_not_found() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        let selected = key("a", ImageSize::Pixels80);
        let sibling = key("a", ImageSize::Pixels512);
        let now = Instant::now();
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        index.publish_variant(&sibling, bitmap(2), 1).unwrap();
        let changes = index.connection.total_changes();
        index.record_not_found(&selected, Some(1), now).unwrap();
        let state = index.read_refresh_state(&selected, now).unwrap();
        assert!(state.is_recently_not_found);
        assert_eq!(state.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        drop(state);
        index.publish_variant(&selected, bitmap(1), 2).unwrap();
        assert!(
            !index
                .read_refresh_state(&selected, now)
                .unwrap()
                .is_recently_not_found
        );
        assert_eq!(index.connection.total_changes(), changes + 2);
        index.record_not_found(&selected, Some(1), now).unwrap();
        index.publish_variant(&sibling, bitmap(3), 3).unwrap();
        index.record_not_found(&selected, Some(1), now).unwrap();
        let state = index.read_refresh_state(&selected, now).unwrap();
        assert!(!state.is_recently_not_found);
        assert_eq!(state.source_revision, Some(2));
        assert!(!state.cached.as_ref().unwrap().read.is_current_source());
        drop(state);
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn durable_variants_reopen_and_survive_media_cache_clearing_with_shared_bytes() {
        use sha2::Digest;

        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let bytes = bitmap(7);
        let first = key("a", ImageSize::Pixels80);
        let second = key("b", ImageSize::Pixels512);
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        index.publish_variant(&first, bytes.clone(), 10).unwrap();
        index.publish_variant(&second, bytes.clone(), 11).unwrap();
        assert_eq!(
            std::fs::read_dir(storage.image_directory().join(
                &format!("{:x}", sha2::Sha256::digest(&bytes))[..2],
            ))
                .unwrap()
                .filter(|entry| entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".png"))
                .count(),
            1
        );
        std::fs::write(
            storage
                .database_directory()
                .join(super::super::database::DATABASE_FILE_NAME),
            b"synthetic media cache",
        )
        .unwrap();
        storage.clear_database_files().unwrap();
        assert_eq!(
            index.read_variant(&first).unwrap().unwrap().payload.bytes,
            bytes
        );
        index.shutdown().unwrap();
        let index = ImageIndex::open(&storage, "server").unwrap();
        for selected in [&first, &second] {
            let read = index.read_variant(selected).unwrap().unwrap();
            assert_eq!(read.payload.bytes, bytes);
            assert_eq!(read.payload.mime_type, "image/png");
            assert!(read.is_current_source());
        }
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn replacement_invalidates_other_variants_but_retains_their_visible_content() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let small = key("a", ImageSize::Pixels80);
        let large = key("a", ImageSize::Pixels512);
        let unrelated = key("b", ImageSize::Pixels80);
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        index.publish_variant(&small, bitmap(1), 1).unwrap();
        index.publish_variant(&large, bitmap(2), 2).unwrap();
        index.publish_variant(&unrelated, bitmap(3), 3).unwrap();
        assert!(index
            .read_variant(&small)
            .unwrap()
            .unwrap()
            .is_current_source());
        index.publish_variant(&small, bitmap(4), 4).unwrap();
        let current = index.read_variant(&small).unwrap().unwrap();
        assert_eq!(current.source_revision, 2);
        assert!(current.is_current_source());
        let stale = index.read_variant(&large).unwrap().unwrap();
        assert!(!stale.is_current_source());
        assert_eq!(stale.payload.bytes, bitmap(2));
        assert!(index
            .read_variant(&unrelated)
            .unwrap()
            .unwrap()
            .is_current_source());
        index.shutdown().unwrap();
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        assert!(!index
            .read_variant(&large)
            .unwrap()
            .unwrap()
            .is_current_source());
        index.publish_variant(&large, bitmap(2), 5).unwrap();
        assert!(index
            .read_variant(&large)
            .unwrap()
            .unwrap()
            .is_current_source());
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn failed_sql_commit_rolls_back_source_and_variant_without_hiding_previous_bytes() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let selected = key("a", ImageSize::Pixels80);
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        index.connection.execute_batch("CREATE TEMP TRIGGER fail_image_update BEFORE UPDATE ON image_variants BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
        assert!(index.publish_variant(&selected, bitmap(2), 2).is_err());
        let read = index.read_variant(&selected).unwrap().unwrap();
        assert_eq!(read.payload.bytes, bitmap(1));
        assert_eq!(read.source_revision, 1);
        assert_eq!(read.observed_at, 1);
        assert!(index
            .publish_variant(&key("b", ImageSize::Pixels80), b"invalid".to_vec(), 3)
            .is_err());
        assert!(index
            .read_variant(&key("b", ImageSize::Pixels80))
            .unwrap()
            .is_none());
        index.shutdown().unwrap();
        let index = ImageIndex::open(&storage, "server").unwrap();
        assert_eq!(
            index
                .read_variant(&selected)
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            bitmap(1)
        );
        assert_eq!(
            index
                .connection
                .query_row("SELECT COUNT(*) FROM image_variants", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn missing_or_corrupt_files_are_misses_and_confirmation_only_updates_refresh_metadata() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let selected = key("a", ImageSize::Pixels80);
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        let changes = index.connection.total_changes();
        index.publish_variant(&selected, bitmap(1), 999).unwrap();
        assert_eq!(index.connection.total_changes(), changes + 1);
        assert_eq!(
            index.read_variant(&selected).unwrap().unwrap().observed_at,
            1
        );
        let stored = read_stored_variant(&index.connection, &selected)
            .unwrap()
            .unwrap();
        let path = storage
            .image_directory()
            .join(super::super::image_files::image_file_name(
                &stored.file.content_hash, &stored.file.mime_type,
            ).unwrap());
        std::fs::write(&path, b"corrupt").unwrap();
        assert!(index.read_variant(&selected).unwrap().is_none());
        index.publish_variant(&selected, bitmap(1), 2).unwrap();
        assert_eq!(index.connection.total_changes(), changes + 2);
        std::fs::remove_file(&path).unwrap();
        assert!(index.read_variant(&selected).unwrap().is_none());
        index.publish_variant(&selected, bitmap(1), 3).unwrap();
        assert_eq!(
            index
                .read_variant(&selected)
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            bitmap(1)
        );
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn server_mismatch_and_foreign_files_are_not_rewritten() {
        for scenario in ["server", "foreign", "extra-table"] {
            let application = tempfile::tempdir().unwrap();
            let storage = storage(application.path());
            let path = storage.image_directory().join(IMAGE_DATABASE_FILE_NAME);
            if scenario == "foreign" {
                std::fs::write(&path, b"foreign synthetic data").unwrap();
            } else {
                ImageIndex::open(&storage, "server")
                    .unwrap()
                    .shutdown()
                    .unwrap();
                let connection = Connection::open(&path).unwrap();
                match scenario {
                    "server" => {
                        connection
                            .execute("UPDATE image_cache_metadata SET server_id='other'", [])
                            .unwrap();
                    }
                    _ => connection
                        .execute_batch("CREATE TABLE foreign_content(value TEXT);")
                        .unwrap(),
                }
                connection.close().unwrap();
            }
            let before = std::fs::read(&path).unwrap();
            assert!(ImageIndex::open(&storage, "server").is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            drop(storage);
            application.close().unwrap();
        }
    }

    #[test]
    fn indexed_point_read_excludes_unrelated_invalid_content_and_recovers_owned_temporaries() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let selected = key("a", ImageSize::Pixels80);
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        index.connection.execute_batch("PRAGMA ignore_check_constraints=ON;
            WITH RECURSIVE items(value) AS (VALUES(1) UNION ALL SELECT value+1 FROM items WHERE value<2000)
            INSERT INTO image_sources SELECT 'reference','bulk',CAST(value AS TEXT),1,0 FROM items;
            INSERT INTO image_variants SELECT source_kind,provider,reference,80,'jpg',1,0,'invalid-hash','image/png',1
                FROM image_sources WHERE provider='bulk';
            PRAGMA ignore_check_constraints=OFF;").unwrap();
        assert_eq!(
            index
                .read_variant(&selected)
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            bitmap(1)
        );
        let plan = index
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {READ_VARIANT_SQL}"))
            .unwrap()
            .query_map(params!["proxy", "", "a".repeat(64), 80, "jpg"], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains("SEARCH variant USING PRIMARY KEY"), "{plan}");
        assert!(plan.contains("SEARCH source USING PRIMARY KEY"), "{plan}");
        index.shutdown().unwrap();
        let temporary = storage
            .image_directory()
            .join(format!(".tmp-{}", "a".repeat(32)));
        let unrelated = storage.image_directory().join("protected-fixture.txt");
        std::fs::write(&temporary, b"incomplete cache publication").unwrap();
        std::fs::write(&unrelated, b"synthetic protected data").unwrap();
        let index = ImageIndex::open(&storage, "server").unwrap();
        assert!(!temporary.exists());
        assert_eq!(
            std::fs::read(unrelated).unwrap(),
            b"synthetic protected data"
        );
        assert_eq!(
            index
                .read_variant(&selected)
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            bitmap(1)
        );
        let invalid = ImageVariantKey {
            source: ImageSourceKey::ModelReference {
                provider: crate::domain::identifiers::ProviderInstanceId("bulk".to_owned()),
                path: "1".to_owned(),
            },
            parameters: selected.parameters,
        };
        assert!(matches!(
            index.read_variant(&invalid),
            Err(CacheError::InvalidImageObservation(_))
        ));
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn failed_atomic_rename_preserves_index_and_cleans_the_synced_temporary_file() {
        use sha2::{Digest, Sha256};
        use std::os::windows::fs::OpenOptionsExt;

        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let selected = key("a", ImageSize::Pixels80);
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        index.publish_variant(&selected, bitmap(1), 1).unwrap();
        let replacement = bitmap(2);
        let replacement_hash = format!("{:x}", Sha256::digest(&replacement));
        let replacement_path = storage.image_directory().join(
            super::super::image_files::image_file_name(&replacement_hash, "image/png").unwrap(),
        );
        std::fs::create_dir_all(replacement_path.parent().unwrap()).unwrap();
        std::fs::write(&replacement_path, b"corrupt previous publication").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&replacement_path)
            .unwrap();
        assert!(matches!(
            index.publish_variant(&selected, replacement.clone(), 2),
            Err(CacheError::Io(_))
        ));
        assert_eq!(
            index
                .read_variant(&selected)
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            bitmap(1)
        );
        assert!(!std::fs::read_dir(storage.image_directory())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-")));
        drop(locked);
        index
            .publish_variant(&selected, replacement.clone(), 3)
            .unwrap();
        assert_eq!(
            index
                .read_variant(&selected)
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            replacement
        );
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn exact_reference_contexts_are_separate_and_inline_or_credential_copies_are_rejected() {
        let application = tempfile::tempdir().unwrap();
        let storage = storage(application.path());
        let mut index = ImageIndex::open(&storage, "server").unwrap();
        for (provider, path, color) in [
            ("one", "007/cover.png", 1),
            ("one", "7/cover.png", 2),
            ("two", "007/cover.png", 3),
        ] {
            let selected = ImageVariantKey {
                source: ImageSourceKey::ModelReference {
                    provider: crate::domain::identifiers::ProviderInstanceId(provider.to_owned()),
                    path: path.to_owned(),
                },
                parameters: key("a", ImageSize::Pixels80).parameters,
            };
            index.publish_variant(&selected, bitmap(color), 1).unwrap();
            assert_eq!(
                index
                    .read_variant(&selected)
                    .unwrap()
                    .unwrap()
                    .payload
                    .bytes,
                bitmap(color)
            );
        }
        for path in [
            "data:image/png;base64,private-bytes",
            "<svg/>",
            "https://example.test/cover?token=synthetic-secret",
            "https://user:password@example.test/cover",
        ] {
            let selected = ImageVariantKey {
                source: ImageSourceKey::ModelReference {
                    provider: crate::domain::identifiers::ProviderInstanceId("one".to_owned()),
                    path: path.to_owned(),
                },
                parameters: key("a", ImageSize::Pixels80).parameters,
            };
            assert!(index.publish_variant(&selected, bitmap(4), 2).is_err());
        }
        assert_eq!(
            index
                .connection
                .query_row("SELECT COUNT(*) FROM image_sources", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
        index.shutdown().unwrap();
        drop(storage);
        application.close().unwrap();
    }
}
