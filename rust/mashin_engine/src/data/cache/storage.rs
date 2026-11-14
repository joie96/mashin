//! Blocking cache initialization and handle-relative deletion for cache files only.
//! Media files live at Cache root, image files in two-character hash shards below images.
//! Application data, credentials and logs are never deletion targets;
//! Unexpected directories and redirects are rejected; unrelated regular files stay untouched.
//! Callers must quiesce workers, downloads and leases before using the deletion primitives.
//! Server bindings persist independently in server.json and survive database replacement.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use cap_std::fs::{Dir, OpenOptions};

use crate::application::repositories::CacheUsage;
use super::database::{validate_directory, validate_file, CacheError, DATABASE_FILE_NAME};

const CACHE_DIRECTORY_NAME: &str = "Cache";
pub(super) const IMAGE_DATABASE_FILE_NAME: &str = "index.sqlite3";
const SERVER_FILE_NAME: &str = "server.json";
const SERVER_TEMP_FILE_NAME: &str = "server.json.tmp";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredServerIdentity {
    format_version: u32,
    server_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    addresses: Vec<String>,
}

pub(crate) struct CacheStorage {
    directory: Dir,
    path: PathBuf,
}

#[derive(Clone, Copy)]
enum CacheArea {
    Database,
    Images,
}

// Runs before opening cache handles; a failed reset must be retried on the next initialization.
pub(crate) fn prepare_for_app_version(
    application_data_directory: &Path,
    app_version: &str,
) -> Result<(), String> {
    if app_version.trim().is_empty() {
        return Err("cache reset requires an app version".into());
    }
    let configuration = crate::configuration::ConfigurationFile::with_path(
        application_data_directory.join("config.json"),
    );
    if configuration.cache_app_version()?.as_deref() == Some(app_version) {
        return Ok(());
    }
    let cache_directory = application_data_directory.join(CACHE_DIRECTORY_NAME);
    match std::fs::symlink_metadata(&cache_directory) {
        Ok(_) => {
            validate_directory(&cache_directory).map_err(|error| error.to_string())?;
            std::fs::remove_dir_all(&cache_directory)
                .map_err(|error| format!("reset cache for app version failed: {error}"))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect cache for app version failed: {error}")),
    }
    configuration.save_cache_app_version(app_version)
}

impl CacheStorage {
    // Storage initialization.
    /// Creates missing application and cache directories and opens their file handles.
    pub(crate) fn open(application_data_directory: &Path) -> Result<Self, CacheError> {
        if application_data_directory.as_os_str().is_empty() {
            return Err(CacheError::UnsafePath(
                application_data_directory.to_owned(),
            ));
        }
        let application_path = if application_data_directory.is_absolute() {
            application_data_directory.to_owned()
        } else {
            std::env::current_dir()?.join(application_data_directory)
        };
        std::fs::create_dir_all(&application_path)?;
        validate_directory(&application_path)?;
        let application_path = application_path.canonicalize()?;
        let application_directory =
            Dir::open_ambient_dir(&application_path, cap_std::ambient_authority())?;
        let path = application_path.join(CACHE_DIRECTORY_NAME);
        create_directory(&application_directory, CACHE_DIRECTORY_NAME)?;
        validate_directory(&path)?;
        let directory = application_directory.open_dir(CACHE_DIRECTORY_NAME)?;
        let storage = Self { directory, path };
        storage.verify_root_entries()?;
        storage.initialize_area(CacheArea::Database)?;
        storage.initialize_area(CacheArea::Images)?;
        Ok(storage)
    }

    /// Reopens an existing cache while its database workers are quiescent.
    pub(super) fn open_existing_cache_directory(path: &Path) -> Result<Self, CacheError> {
        validate_directory(path)?;
        let path = path.canonicalize()?;
        let directory = Dir::open_ambient_dir(&path, cap_std::ambient_authority())?;
        let storage = Self { directory, path };
        storage.verify_root_entries()?;
        Ok(storage)
    }

    // Server-selection publication.
    pub(super) fn write_server_id(&self, server_id: &str) -> Result<(), CacheError> {
        if server_id.trim().is_empty() {
            return Err(CacheError::InvalidServerId);
        }
        let addresses = self
            .read_server_identity()?
            .filter(|identity| identity.server_id == server_id)
            .map(|identity| identity.addresses)
            .unwrap_or_default();
        self.write_server_identity(&StoredServerIdentity {
            format_version: 1,
            server_id: server_id.to_owned(),
            addresses,
        })
    }

    /// Records only an address whose server identity was confirmed by MA.
    pub(super) fn bind_server_address(
        &self,
        server_id: &str,
        address: &str,
    ) -> Result<(), CacheError> {
        let address = normalized_server_address(address)?;
        let mut identity = self
            .read_server_identity()?
            .ok_or(CacheError::CacheInactive)?;
        if identity.server_id != server_id {
            return Err(CacheError::ServerMismatch);
        }
        if !identity.addresses.contains(&address) {
            identity.addresses.push(address);
        }
        self.write_server_identity(&identity)
    }

    pub(super) fn server_id_for_address(
        &self,
        address: &str,
    ) -> Result<Option<String>, CacheError> {
        let address = normalized_server_address(address)?;
        Ok(self
            .read_server_identity()?
            .filter(|identity| identity.addresses.contains(&address))
            .map(|identity| identity.server_id))
    }

    // Atomic manifest publication; the lifecycle owner serializes writers.
    fn write_server_identity(&self, identity: &StoredServerIdentity) -> Result<(), CacheError> {
        validate_directory(&self.path)?;
        validate_file(&self.path.join(SERVER_FILE_NAME))?;
        validate_file(&self.path.join(SERVER_TEMP_FILE_NAME))?;
        match self.directory.remove_file(SERVER_TEMP_FILE_NAME) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = self.directory.open_with(SERVER_TEMP_FILE_NAME, &options)?;
        serde_json::to_writer(&mut file, identity)?;
        file.sync_all()?;
        drop(file);
        self.directory
            .rename(SERVER_TEMP_FILE_NAME, &self.directory, SERVER_FILE_NAME)?;
        Ok(())
    }

    // Quiescent physical deletion; lifecycle coordination is owned by the caller.
    /// Deletes database files only; this primitive neither stops workers nor reopens SQLite.
    pub(super) fn clear_database_files(&self) -> Result<(), CacheError> {
        self.clear_files(CacheArea::Database)
    }

    /// Deletes image files and their separate index, leaving all media database files intact.
    pub(super) fn clear_image_files(&self) -> Result<(), CacheError> {
        self.clear_files(CacheArea::Images)
    }

    // Read-only physical usage; files can disappear during worker maintenance.
    pub(crate) fn usage(&self) -> Result<CacheUsage, CacheError> {
        let mut metadata_bytes = 0;
        for entry in self.directory.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_str().is_some_and(|name| CacheArea::Database.owns_file(name)) {
                match self.directory.symlink_metadata(&name) {
                    Ok(metadata) if metadata.is_file() => metadata_bytes += metadata.len(),
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        let image_bytes = directory_bytes(&self.directory.open_dir("images")?)?;
        Ok(CacheUsage { metadata_bytes, image_bytes })
    }

    // Worker and file-owner locations.
    pub(super) fn database_directory(&self) -> PathBuf {
        self.path.clone()
    }

    pub(super) fn image_directory(&self) -> PathBuf {
        self.path.join(CacheArea::Images.directory_name())
    }

    /// Opens the image directory without following directory redirects.
    pub(super) fn open_image_directory(&self) -> Result<Dir, CacheError> {
        validate_directory(&self.path)?;
        self.verify_root_entries()?;
        self.initialize_area(CacheArea::Images)
    }

    pub(super) fn read_server_id(&self) -> Result<Option<String>, CacheError> {
        Ok(self
            .read_server_identity()?
            .map(|identity| identity.server_id))
    }

    fn read_server_identity(&self) -> Result<Option<StoredServerIdentity>, CacheError> {
        validate_file(&self.path.join(SERVER_FILE_NAME))?;
        match self.directory.open(SERVER_FILE_NAME) {
            Ok(file) => {
                let identity: StoredServerIdentity = serde_json::from_reader(file)?;
                if identity.format_version != 1 || identity.server_id.trim().is_empty() {
                    return Err(CacheError::InvalidServerId);
                }
                Ok(Some(identity))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    // Path validation and deletion plans.
    fn initialize_area(&self, area: CacheArea) -> Result<Dir, CacheError> {
        if matches!(area, CacheArea::Database) {
            return Ok(self.directory.try_clone()?);
        }
        create_directory(&self.directory, area.directory_name())?;
        let path = self.path.join(area.directory_name());
        validate_directory(&path)?;
        let directory = self.directory.open_dir(area.directory_name())?;
        Ok(directory)
    }

    fn verify_root_entries(&self) -> Result<(), CacheError> {
        for entry in self.directory.entries()? {
            let name = entry?.file_name();
            let path = self.path.join(&name);
            if name == "images" {
                validate_directory(&path)?;
            } else {
                validate_file(&path)?;
                if !self.directory.symlink_metadata(&name)?.is_file() {
                    return Err(CacheError::UnsafePath(path));
                }
            }
        }
        Ok(())
    }

    fn clear_files(&self, area: CacheArea) -> Result<(), CacheError> {
        validate_directory(&self.path)?;
        self.verify_root_entries()?;
        let directory = self.initialize_area(area)?;
        let area_path = self.path.join(area.directory_name());
        let mut deletion_plan: Vec<OsString> = Vec::new();
        let mut image_shards = Vec::new();
        for entry in directory.entries()? {
            let name = entry?.file_name();
            if matches!(area, CacheArea::Database) && is_preserved_root_entry(&name) {
                continue;
            }
            let path = area_path.join(&name);
            if matches!(area, CacheArea::Images)
                && name.to_str().is_some_and(is_image_shard)
            {
                validate_directory(&path)?;
                let shard = directory.open_dir(&name)?;
                let mut files = Vec::new();
                for entry in shard.entries()? {
                    let file_name = entry?.file_name();
                    let file_path = path.join(&file_name);
                    validate_file(&file_path)?;
                    if !shard.symlink_metadata(&file_name)?.is_file() {
                        return Err(CacheError::UnsafePath(file_path));
                    }
                    if !file_name.to_str().is_some_and(|file_name| {
                        is_image_file(file_name)
                            && file_name.starts_with(name.to_str().unwrap())
                    }) {
                        return Err(CacheError::ForeignCacheFile(file_path));
                    }
                    files.push(file_name);
                }
                image_shards.push((name, shard, files));
                continue;
            }
            validate_file(&path)?;
            let metadata = directory.symlink_metadata(&name)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(CacheError::UnsafePath(path));
            }
            if !name.to_str().is_some_and(|name| area.owns_file(name)) {
                continue;
            }
            deletion_plan.push(name);
        }
        for (name, shard, files) in image_shards {
            remove_planned_files(&shard, files)?;
            drop(shard);
            directory.remove_dir(name)?;
        }
        remove_planned_files(&directory, deletion_plan)
    }
}

impl CacheArea {
    // Reserved database and image cache names.
    fn directory_name(self) -> &'static str {
        match self {
            Self::Database => ".",
            Self::Images => "images",
        }
    }

    fn owns_file(self, name: &str) -> bool {
        let database_name = match self {
            Self::Database => DATABASE_FILE_NAME,
            Self::Images => IMAGE_DATABASE_FILE_NAME,
        };
        if ["", "-wal", "-shm", "-journal"]
            .iter()
            .any(|suffix| name == format!("{database_name}{suffix}"))
        {
            return true;
        }
        if name
            .strip_prefix(&format!("{database_name}.tmp-"))
            .is_some_and(|suffix| is_hex(suffix, 32))
        {
            return true;
        }
        matches!(self, Self::Images)
            && name
                .strip_prefix(".tmp-")
                .is_some_and(|suffix| is_hex(suffix, 32))
    }
}

// Handle-relative traversal never follows image-directory redirects.
fn directory_bytes(directory: &Dir) -> Result<u64, CacheError> {
    let mut bytes = 0;
    for entry in directory.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let result = (|| -> Result<u64, CacheError> {
            let metadata = directory.symlink_metadata(&name)?;
            if metadata.file_type().is_symlink() {
                return Ok(0);
            }
            if metadata.is_dir() {
                return directory_bytes(&directory.open_dir(&name)?);
            }
            Ok(if metadata.is_file() { metadata.len() } else { 0 })
        })();
        match result {
            Ok(size) => bytes += size,
            Err(CacheError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(bytes)
}

// Cache file-name validation.
fn is_image_shard(name: &str) -> bool {
    name.len() == 2
        && name.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_image_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(hash, extension)| {
        hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && matches!(extension, "png" | "jpg" | "webp" | "gif")
    })
}

fn is_preserved_root_entry(name: &OsStr) -> bool {
    [
        "images",
        SERVER_FILE_NAME,
        SERVER_TEMP_FILE_NAME,
    ]
    .iter()
    .any(|entry| name == OsStr::new(entry))
}

fn create_directory(parent: &Dir, name: &str) -> Result<bool, CacheError> {
    match parent.create_dir(name) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn remove_planned_files(directory: &Dir, names: Vec<OsString>) -> Result<(), CacheError> {
    for name in names {
        match directory.remove_file(name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalized_server_address(address: &str) -> Result<String, CacheError> {
    let url = reqwest::Url::parse(address).map_err(|_| CacheError::InvalidServerId)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CacheError::InvalidServerId);
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::super::workers::DatabaseWorkers;
    use super::*;

    #[test]
    fn app_version_change_clears_entire_cache_once_and_preserves_application_data() {
        let application = tempfile::tempdir().unwrap();
        let configuration = crate::configuration::ConfigurationFile::with_path(
            application.path().join("config.json"),
        );
        let preferences = crate::configuration::SessionPreferences::from(
            &crate::configuration::tests::session_config(),
        );
        configuration.save(&preferences).unwrap();
        configuration.save_cache_app_version("0.0.2-beta.2").unwrap();
        let cache = application.path().join(CACHE_DIRECTORY_NAME);
        std::fs::create_dir_all(cache.join("images")).unwrap();
        for path in [cache.join(DATABASE_FILE_NAME), cache.join(SERVER_FILE_NAME), cache.join("images/index.sqlite3"), cache.join("images/image.png")] {
            std::fs::write(path, b"cached content").unwrap();
        }
        std::fs::create_dir(application.path().join("Logs")).unwrap();
        std::fs::write(application.path().join("Logs/current.log"), b"preserved log").unwrap();
        std::fs::write(application.path().join("credentials.json"), b"preserved credentials").unwrap();
        prepare_for_app_version(application.path(), "0.0.2-beta.3").unwrap();
        assert!(!cache.exists());
        assert_eq!(configuration.load().unwrap(), Some(preferences));
        assert_eq!(std::fs::read(application.path().join("Logs/current.log")).unwrap(), b"preserved log");
        assert_eq!(std::fs::read(application.path().join("credentials.json")).unwrap(), b"preserved credentials");
        let storage = CacheStorage::open(application.path()).unwrap();
        std::fs::write(storage.database_directory().join(DATABASE_FILE_NAME), b"new cache").unwrap();
        drop(storage);
        prepare_for_app_version(application.path(), "0.0.2-beta.3").unwrap();
        assert_eq!(std::fs::read(cache.join(DATABASE_FILE_NAME)).unwrap(), b"new cache");
    }

    #[test]
    fn missing_app_version_clears_existing_cache_before_recording_current_version() {
        let application = tempfile::tempdir().unwrap();
        let cache = application.path().join(CACHE_DIRECTORY_NAME);
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join(DATABASE_FILE_NAME), b"existing cache").unwrap();
        prepare_for_app_version(application.path(), "0.0.2-beta.3").unwrap();
        assert!(!cache.exists());
        let configuration = crate::configuration::ConfigurationFile::with_path(application.path().join("config.json"));
        assert_eq!(configuration.cache_app_version().unwrap().as_deref(), Some("0.0.2-beta.3"));
    }

    #[test]
    fn failed_app_version_reset_does_not_record_the_new_version() {
        let application = tempfile::tempdir().unwrap();
        let configuration = crate::configuration::ConfigurationFile::with_path(application.path().join("config.json"));
        configuration.save_cache_app_version("0.0.2-beta.2").unwrap();
        let cache = application.path().join(CACHE_DIRECTORY_NAME);
        std::fs::write(&cache, b"not a cache directory").unwrap();
        assert!(prepare_for_app_version(application.path(), "0.0.2-beta.3").is_err());
        assert_eq!(configuration.cache_app_version().unwrap().as_deref(), Some("0.0.2-beta.2"));
        assert_eq!(std::fs::read(cache).unwrap(), b"not a cache directory");
    }

    fn image_name() -> String {
        format!("{}.png", "a".repeat(64))
    }

    fn image_path(storage: &CacheStorage) -> PathBuf {
        let shard = storage.image_directory().join("aa");
        std::fs::create_dir_all(&shard).unwrap();
        shard.join(image_name())
    }

    #[test]
    fn missing_application_data_directory_is_created_with_only_cache_and_images() {
        let application = tempfile::tempdir().unwrap();
        let root = application.path().join("missing").join("Mashin");
        let storage = CacheStorage::open(&root).unwrap();
        assert_eq!(storage.usage().unwrap().metadata_bytes, 0);
        assert_eq!(storage.usage().unwrap().image_bytes, 0);
        assert!(root.join("Cache/images").is_dir());
        assert_eq!(std::fs::read_dir(root.join("Cache")).unwrap().count(), 1);
    }

    #[test]
    fn existing_empty_cache_opens_without_creating_additional_files() {
        let application = tempfile::tempdir().unwrap();
        let cache = application.path().join(CACHE_DIRECTORY_NAME);
        std::fs::create_dir_all(cache.join("images")).unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        assert_eq!(storage.read_server_id().unwrap(), None);
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(cache.join("images")).unwrap().count(), 0);
    }

    #[test]
    fn cache_usage_counts_database_sidecars_and_entire_image_directory() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let root = storage.database_directory();
        for (suffix, length) in [("", 11), ("-wal", 13), ("-shm", 17), ("-journal", 19)] {
            std::fs::write(root.join(format!("{DATABASE_FILE_NAME}{suffix}")), vec![0; length]).unwrap();
        }
        std::fs::write(root.join(format!("{DATABASE_FILE_NAME}.tmp-{}", "a".repeat(32))), vec![0; 31]).unwrap();
        std::fs::write(application.path().join("settings.json"), vec![0; 100]).unwrap();
        std::fs::write(image_path(&storage), vec![0; 23]).unwrap();
        std::fs::write(storage.image_directory().join(IMAGE_DATABASE_FILE_NAME), vec![0; 29]).unwrap();
        std::fs::write(storage.image_directory().join(format!("{IMAGE_DATABASE_FILE_NAME}-wal")), vec![0; 37]).unwrap();
        let usage = storage.usage().unwrap();
        assert_eq!(usage.metadata_bytes, 91);
        assert_eq!(usage.image_bytes, 23 + 29 + 37);
        storage.clear_database_files().unwrap();
        assert_eq!(storage.usage().unwrap().metadata_bytes, 0);
        assert_eq!(storage.usage().unwrap().image_bytes, usage.image_bytes);
        storage.clear_image_files().unwrap();
        assert_eq!(storage.usage().unwrap().image_bytes, 0);
    }

    #[test]
    fn cache_layout_is_sibling_to_app_and_logs_and_preserves_root_metadata() {
        let application = tempfile::tempdir().unwrap();
        for name in ["App", "Logs", "mashin-cache"] {
            let directory = application.path().join(name);
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(
                directory.join("protected.txt"),
                b"protected synthetic content",
            )
            .unwrap();
        }
        let storage = CacheStorage::open(application.path()).unwrap();
        let cache = application.path().canonicalize().unwrap().join("Cache");
        assert_eq!(storage.database_directory(), cache);
        assert_eq!(storage.image_directory(), cache.join("images"));
        assert!(!cache.join("database").exists());
        storage.write_server_id("observed-server").unwrap();
        storage.bind_server_address("observed-server", "http://localhost:8095").unwrap();
        let identity = std::fs::read(cache.join(SERVER_FILE_NAME)).unwrap();
        std::fs::write(cache.join(DATABASE_FILE_NAME), b"synthetic database").unwrap();
        let image = image_path(&storage);
        std::fs::write(&image, b"synthetic image").unwrap();
        storage.clear_database_files().unwrap();
        assert!(!cache.join(DATABASE_FILE_NAME).exists());
        assert_eq!(std::fs::read(&image).unwrap(), b"synthetic image");
        storage.clear_image_files().unwrap();
        assert_eq!(std::fs::read(cache.join(SERVER_FILE_NAME)).unwrap(), identity);
        drop(storage);
        let storage = CacheStorage::open(application.path()).unwrap();
        assert_eq!(
            storage.read_server_id().unwrap().as_deref(),
            Some("observed-server")
        );
        assert_eq!(storage.server_id_for_address("http://localhost:8095/").unwrap().as_deref(), Some("observed-server"));
        for name in ["App", "Logs", "mashin-cache"] {
            assert_eq!(
                std::fs::read(application.path().join(name).join("protected.txt")).unwrap(),
                b"protected synthetic content"
            );
        }
    }

    #[test]
    fn server_manifest_survives_reopening_without_creating_a_database() {
        let application = tempfile::tempdir().unwrap();
        let cache = application.path().join(CACHE_DIRECTORY_NAME);
        std::fs::create_dir_all(cache.join("images")).unwrap();
        std::fs::write(cache.join(SERVER_FILE_NAME), br#"{"format_version":1,"server_id":"observed-server","addresses":["http://localhost:8095"]}"#).unwrap();
        let manifest = std::fs::read(cache.join(SERVER_FILE_NAME)).unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        assert_eq!(storage.read_server_id().unwrap().as_deref(), Some("observed-server"));
        assert_eq!(std::fs::read(cache.join(SERVER_FILE_NAME)).unwrap(), manifest);
        drop(storage);
        let reopened = CacheStorage::open(application.path()).unwrap();
        assert_eq!(reopened.server_id_for_address("http://localhost:8095").unwrap().as_deref(), Some("observed-server"));
        assert!(!cache.join(DATABASE_FILE_NAME).exists());
    }

    #[test]
    fn reopening_preserves_cache_and_unrelated_root_files() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let image_path = image_path(&storage);
        std::fs::write(&image_path, b"synthetic image").unwrap();
        drop(storage);
        let reopened = CacheStorage::open(application.path()).unwrap();
        assert_eq!(std::fs::read(image_path).unwrap(), b"synthetic image");
        drop(reopened);
        let application = tempfile::tempdir().unwrap();
        let candidate = application.path().join(CACHE_DIRECTORY_NAME);
        std::fs::create_dir(&candidate).unwrap();
        std::fs::write(candidate.join("config.json"), b"protected").unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        storage.clear_database_files().unwrap();
        assert_eq!(
            std::fs::read(candidate.join("config.json")).unwrap(),
            b"protected"
        );
        assert!(!candidate.join("database").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn database_clear_preserves_images_and_rejects_old_worker_jobs() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        for name in ["config.json", "credentials.json", "musicassistant.log"] {
            std::fs::write(application.path().join(name), b"protected synthetic data").unwrap();
        }
        let workers = DatabaseWorkers::initialize(
            storage.database_directory(),
            "synthetic-server".to_owned(),
        )
        .await
        .unwrap();
        workers.shutdown().await.unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            std::fs::write(
                storage
                    .database_directory()
                    .join(format!("{DATABASE_FILE_NAME}{suffix}")),
                b"synthetic sidecar",
            )
            .unwrap();
        }
        let temporary = storage
            .database_directory()
            .join(format!("{DATABASE_FILE_NAME}.tmp-{}", "b".repeat(32)));
        std::fs::write(&temporary, b"synthetic incomplete database").unwrap();
        let image_path = image_path(&storage);
        std::fs::write(&image_path, b"synthetic image").unwrap();
        let image_index = storage.image_directory().join(IMAGE_DATABASE_FILE_NAME);
        std::fs::write(&image_index, b"synthetic separate image index").unwrap();
        storage.clear_database_files().unwrap();
        storage.clear_database_files().unwrap();
        assert_eq!(
            std::fs::read_dir(storage.database_directory())
                .unwrap()
                .count(),
            1
        );
        assert_eq!(std::fs::read(&image_path).unwrap(), b"synthetic image");
        assert_eq!(
            std::fs::read(&image_index).unwrap(),
            b"synthetic separate image index"
        );
        assert!(matches!(
            workers.write_transaction(|_| Ok(())).await,
            Err(CacheError::ShuttingDown)
        ));
        let reopened = DatabaseWorkers::initialize(
            storage.database_directory(),
            "synthetic-server".to_owned(),
        )
        .await
        .unwrap();
        assert_eq!(
            reopened.metadata().await.unwrap().server_id,
            "synthetic-server"
        );
        reopened.shutdown().await.unwrap();
        for name in ["config.json", "credentials.json", "musicassistant.log"] {
            assert_eq!(
                std::fs::read(application.path().join(name)).unwrap(),
                b"protected synthetic data"
            );
        }
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn image_clear_removes_files_index_and_temporaries_but_not_media() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let database = storage.database_directory().join(DATABASE_FILE_NAME);
        std::fs::write(&database, b"synthetic media database").unwrap();
        std::fs::write(image_path(&storage), b"synthetic image").unwrap();
        for name in [
            format!(".tmp-{}", "a".repeat(32)),
            IMAGE_DATABASE_FILE_NAME.to_owned(),
            format!("{IMAGE_DATABASE_FILE_NAME}-wal"),
        ] {
            std::fs::write(
                storage.image_directory().join(name),
                b"synthetic image data",
            )
            .unwrap();
        }
        storage.clear_image_files().unwrap();
        storage.clear_image_files().unwrap();
        assert_eq!(
            std::fs::read_dir(storage.image_directory())
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            std::fs::read(database).unwrap(),
            b"synthetic media database"
        );
    }

    #[test]
    fn unrelated_files_are_preserved_and_unexpected_directories_prevent_deletion() {
        for is_directory in [false, true] {
            let application = tempfile::tempdir().unwrap();
            let storage = CacheStorage::open(application.path()).unwrap();
            let image = image_path(&storage);
            std::fs::write(&image, b"keep valid cache file").unwrap();
            let foreign = storage.image_directory().join("protected");
            if is_directory {
                std::fs::create_dir(&foreign).unwrap();
            } else {
                std::fs::write(&foreign, b"keep foreign file").unwrap();
            }
            let result = storage.clear_image_files();
            if is_directory {
                assert!(result.is_err());
                assert_eq!(std::fs::read(image).unwrap(), b"keep valid cache file");
            } else {
                result.unwrap();
                assert!(!image.exists());
                assert_eq!(std::fs::read(&foreign).unwrap(), b"keep foreign file");
            }
            assert!(foreign.exists());
        }
    }

    #[test]
    fn invalid_shard_contents_prevent_deleting_any_cache_files() {
        for (name, is_directory) in [
            ("protected.txt".to_owned(), false),
            (format!("{}.png", "b".repeat(64)), false),
            (format!("{}.bin", "a".repeat(64)), false),
            (format!("{}.png", "a".repeat(64)), true),
        ] {
            let application = tempfile::tempdir().unwrap();
            let storage = CacheStorage::open(application.path()).unwrap();
            let index = storage.image_directory().join(IMAGE_DATABASE_FILE_NAME);
            std::fs::write(&index, b"keep index").unwrap();
            let shard = storage.image_directory().join("aa");
            std::fs::create_dir(&shard).unwrap();
            let foreign = shard.join(name);
            if is_directory {
                std::fs::create_dir(&foreign).unwrap();
            } else {
                std::fs::write(&foreign, b"keep foreign content").unwrap();
            }
            assert!(storage.clear_image_files().is_err());
            assert_eq!(std::fs::read(index).unwrap(), b"keep index");
            assert!(foreign.exists());
        }
    }

    #[test]
    fn invalid_root_entries_prevent_deletion_in_both_areas() {
        for name in [
            "protected",
            SERVER_FILE_NAME,
            DATABASE_FILE_NAME,
        ] {
            let application = tempfile::tempdir().unwrap();
            let storage = CacheStorage::open(application.path()).unwrap();
            let image = image_path(&storage);
            std::fs::write(&image, b"keep image").unwrap();
            let sidecar = storage
                .database_directory()
                .join(format!("{DATABASE_FILE_NAME}-wal"));
            std::fs::write(&sidecar, b"keep sidecar").unwrap();
            let foreign = storage.database_directory().join(name);
            std::fs::create_dir(&foreign).unwrap();
            assert!(storage.clear_database_files().is_err());
            assert!(storage.clear_image_files().is_err());
            assert_eq!(std::fs::read(&image).unwrap(), b"keep image");
            assert_eq!(std::fs::read(&sidecar).unwrap(), b"keep sidecar");
            assert!(foreign.is_dir());
        }
    }

    #[cfg(windows)]
    #[test]
    fn junction_area_cannot_redirect_deletion_outside_the_cache() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let protected = tempfile::tempdir().unwrap();
        let image = protected.path().join(image_name());
        std::fs::write(&image, b"protected external image").unwrap();
        let area = storage.image_directory();
        let media = storage.database_directory().join(DATABASE_FILE_NAME);
        std::fs::write(&media, b"keep media").unwrap();
        std::fs::remove_dir(&area).unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&area)
            .arg(protected.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(storage.clear_database_files().is_err());
        assert!(storage.clear_image_files().is_err());
        assert_eq!(std::fs::read(media).unwrap(), b"keep media");
        assert_eq!(std::fs::read(image).unwrap(), b"protected external image");
        std::fs::remove_dir(area).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn deletion_error_is_reported_and_retry_succeeds() {
        use std::os::windows::fs::OpenOptionsExt;
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let image = image_path(&storage);
        std::fs::write(&image, b"synthetic locked image").unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&image)
            .unwrap();
        assert!(matches!(
            storage.clear_image_files(),
            Err(CacheError::Io(_))
        ));
        assert!(image.exists());
        drop(lock);
        storage.clear_image_files().unwrap();
        assert!(!image.exists());
    }

    #[cfg(windows)]
    #[test]
    fn open_area_handle_prevents_windows_path_replacement() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let area_path = storage.image_directory();
        let name = image_name();
        std::fs::write(area_path.join(&name), b"owned synthetic image").unwrap();
        let directory = storage.initialize_area(CacheArea::Images).unwrap();
        let moved = application.path().join("isolated-moved-cache-area");
        let replacement_error = std::fs::rename(&area_path, &moved).unwrap_err();
        assert_eq!(replacement_error.raw_os_error(), Some(32));
        let protected = tempfile::tempdir().unwrap();
        std::fs::write(protected.path().join(&name), b"protected external image").unwrap();
        remove_planned_files(&directory, vec![OsString::from(&name)]).unwrap();
        assert!(!area_path.join(&name).exists());
        assert!(!moved.exists());
        assert_eq!(
            std::fs::read(protected.path().join(&name)).unwrap(),
            b"protected external image"
        );
        drop(directory);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_file_cannot_redirect_deletion_outside_the_cache() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let protected = tempfile::tempdir().unwrap();
        let image = protected.path().join("protected-image");
        std::fs::write(&image, b"protected external image").unwrap();
        let redirected = image_path(&storage);
        std::os::unix::fs::symlink(&image, &redirected).unwrap();
        assert!(storage.clear_image_files().is_err());
        assert_eq!(std::fs::read(image).unwrap(), b"protected external image");
        std::fs::remove_file(redirected).unwrap();
    }
}
