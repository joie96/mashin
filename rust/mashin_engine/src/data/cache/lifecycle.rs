//! Serialized cache maintenance with persistent server binding and invalidatable DB access.
//! Maintenance continues after cancellation of its caller. Blocking file operations and SQL
//! never run on the async executor. Session catalogs hold generation-bound DB leases;
//! image workers drain accepted publications and issued file leases before physical maintenance.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rusqlite::Transaction;
use tokio::sync::{watch, Mutex};

use super::database::{CacheError, CacheFileMetadata};
use super::image_files::ImagePayload;
use super::image_index::{ImageRefreshState, ImageVariantLease};
use super::image_reads::ImageRefreshes;
use super::image_sources::ImageVariantKey;
use super::image_workers::ImageWorkers;
use super::storage::CacheStorage;
use super::workers::DatabaseWorkers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheMaintenance {
    ActivateServer,
    ActivateImages,
    ClearDatabase,
    ClearImages,
    Shutdown,
}

#[derive(Clone, Debug)]
pub(crate) struct CacheLifecycleSnapshot {
    pub(crate) server_id: Option<String>,
    pub(crate) database_generation: u64,
    pub(crate) image_generation: u64,
    pub(crate) is_database_active: bool,
    pub(crate) is_image_active: bool,
    pub(crate) is_shutdown: bool,
    pub(crate) maintenance: Option<CacheMaintenance>,
    pub(crate) last_error: Option<CacheError>,
}

#[derive(Clone)]
pub(crate) struct ServerCache {
    state: Arc<CacheState>,
}

struct CacheState {
    runtime: Mutex<CacheRuntime>,
    published_database: Arc<std::sync::Mutex<Option<DatabaseLease>>>,
    published_images: Arc<std::sync::Mutex<Option<ImageLease>>>,
    changes: watch::Sender<CacheLifecycleSnapshot>,
}

struct CacheRuntime {
    storage: Arc<CacheStorage>,
    database: Option<DatabaseLease>,
    images: Option<ImageLease>,
    snapshot: CacheLifecycleSnapshot,
    published_database: Arc<std::sync::Mutex<Option<DatabaseLease>>>,
    published_images: Arc<std::sync::Mutex<Option<ImageLease>>>,
}

#[derive(Clone)]
pub(crate) struct DatabaseLease {
    workers: DatabaseWorkers,
    is_active: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(super) struct ImageLease {
    workers: ImageWorkers,
    pub(super) refreshes: Arc<ImageRefreshes>,
    is_active: Arc<AtomicBool>,
    invalidation: watch::Sender<bool>,
    generation_id: uuid::Uuid,
}

impl ServerCache {
    // Initialization and lifecycle.
    pub(crate) async fn initialize(
        application_data_directory: PathBuf,
    ) -> Result<Self, CacheError> {
        let (storage, server_id) = tokio::task::spawn_blocking(move || {
            let storage = CacheStorage::open(&application_data_directory)?;
            let server_id = storage.read_server_id()?;
            Ok::<_, CacheError>((storage, server_id))
        })
        .await
        .map_err(|_| CacheError::WorkerPanicked)??;
        let snapshot = CacheLifecycleSnapshot {
            server_id,
            database_generation: 0,
            image_generation: 0,
            is_database_active: false,
            is_image_active: false,
            is_shutdown: false,
            maintenance: None,
            last_error: None,
        };
        let (changes, _) = watch::channel(snapshot.clone());
        let published_database = Arc::new(std::sync::Mutex::new(None));
        let published_images = Arc::new(std::sync::Mutex::new(None));
        Ok(Self {
            state: Arc::new(CacheState {
                runtime: Mutex::new(CacheRuntime {
                    storage: Arc::new(storage),
                    database: None,
                    images: None,
                    published_database: published_database.clone(),
                    published_images: published_images.clone(),
                    snapshot,
                }),
                published_database,
                published_images,
                changes,
            }),
        })
    }

    /// Accepts the observed server ID, never an address or user ID.
    pub(crate) async fn activate(&self, server_id: String) -> Result<(), CacheError> {
        self.run_maintenance(CacheMaintenance::ActivateServer, Some(server_id))
            .await
    }

    pub(super) async fn activate_images(&self, server_id: String) -> Result<(), CacheError> {
        self.run_maintenance(CacheMaintenance::ActivateImages, Some(server_id))
            .await
    }

    pub(crate) async fn shutdown(&self) -> Result<(), CacheError> {
        self.run_maintenance(CacheMaintenance::Shutdown, None).await
    }

    // Cache commands.
    pub(crate) async fn clear_database_cache(&self) -> Result<(), CacheError> {
        self.run_maintenance(CacheMaintenance::ClearDatabase, None)
            .await
    }

    /// Invalidates image access and waits for accepted work and file leases before deletion.
    pub(crate) async fn clear_image_cache(&self) -> Result<(), CacheError> {
        self.run_maintenance(CacheMaintenance::ClearImages, None)
            .await
    }

    // Physical usage is serialized with maintenance, without activating either database.
    pub(crate) async fn usage(&self) -> Result<crate::application::repositories::CacheUsage, CacheError> {
        let runtime = self.state.runtime.lock().await;
        if runtime.snapshot.is_shutdown {
            return Err(CacheError::ShuttingDown);
        }
        let storage = runtime.storage.clone();
        tokio::task::spawn_blocking(move || storage.usage())
            .await
            .map_err(|_| CacheError::WorkerPanicked)?
    }

    // Active access and lifecycle observation.
    /// Reads only a previously confirmed address binding, never an inferred URL identity.
    pub(crate) async fn server_id_for_address(
        &self,
        address: String,
    ) -> Result<Option<String>, CacheError> {
        let runtime = self.state.runtime.lock().await;
        if runtime.snapshot.is_shutdown {
            return Err(CacheError::ShuttingDown);
        }
        let storage = runtime.storage.clone();
        tokio::task::spawn_blocking(move || storage.server_id_for_address(&address))
            .await
            .map_err(|_| CacheError::WorkerPanicked)?
    }

    /// Serializes credential-free manifest updates with cancellation-independent maintenance.
    pub(crate) async fn bind_server_address(
        &self,
        server_id: String,
        address: String,
    ) -> Result<(), CacheError> {
        let state = self.state.clone();
        tokio::spawn(async move {
            let runtime = state.runtime.lock().await;
            if runtime.snapshot.is_shutdown {
                return Err(CacheError::ShuttingDown);
            }
            let storage = runtime.storage.clone();
            tokio::task::spawn_blocking(move || storage.bind_server_address(&server_id, &address))
                .await
                .map_err(|_| CacheError::WorkerPanicked)?
        })
        .await
        .map_err(|_| CacheError::WorkerPanicked)?
    }

    pub(crate) async fn database(&self) -> Result<DatabaseLease, CacheError> {
        let database = self
            .state
            .published_database
            .lock()
            .map_err(|_| CacheError::WorkerUnavailable)?;
        if self.state.changes.borrow().is_shutdown {
            return Err(CacheError::ShuttingDown);
        }
        database
            .clone()
            .filter(|database| database.is_active.load(Ordering::Acquire))
            .ok_or(CacheError::CacheInactive)
    }

    pub(crate) fn observe(&self) -> watch::Receiver<CacheLifecycleSnapshot> {
        self.state.changes.subscribe()
    }

    pub(super) async fn images(&self) -> Result<ImageLease, CacheError> {
        let images = self
            .state
            .published_images
            .lock()
            .map_err(|_| CacheError::WorkerUnavailable)?;
        if self.state.changes.borrow().is_shutdown {
            return Err(CacheError::ShuttingDown);
        }
        images
            .clone()
            .filter(|images| images.is_active.load(Ordering::Acquire))
            .ok_or(CacheError::CacheInactive)
    }

    // Cancellation-independent serialized maintenance.
    async fn run_maintenance(
        &self,
        operation: CacheMaintenance,
        server_id: Option<String>,
    ) -> Result<(), CacheError> {
        let state = self.state.clone();
        tokio::spawn(async move {
            let mut runtime = state.runtime.lock().await;
            if operation == CacheMaintenance::Shutdown && runtime.snapshot.is_shutdown {
                return runtime.snapshot.last_error.clone().map_or(Ok(()), Err);
            }
            if runtime.snapshot.is_shutdown {
                return Err(CacheError::ShuttingDown);
            }
            if let Some(server_id) = server_id.as_ref() {
                if server_id.trim().is_empty() {
                    return Err(CacheError::InvalidServerId);
                }
                if runtime.snapshot.server_id.as_ref() == Some(server_id)
                    && runtime.database.is_some()
                    && runtime.images.is_some()
                {
                    return Ok(());
                }
            }
            runtime.snapshot.maintenance = Some(operation);
            runtime.snapshot.last_error = None;
            state.changes.send_replace(runtime.snapshot.clone());
            let result = runtime.perform(operation, server_id, &state.changes).await;
            runtime.snapshot.maintenance = None;
            runtime.snapshot.last_error = result.as_ref().err().cloned();
            state.changes.send_replace(runtime.snapshot.clone());
            result
        })
        .await
        .map_err(|_| CacheError::WorkerPanicked)?
    }
}

impl DatabaseLease {
    // Generation-scoped database access for internal read and write adapters.
    pub(super) async fn read_snapshot<ResultValue, Operation>(
        &self,
        operation: Operation,
    ) -> Result<ResultValue, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&Transaction<'_>) -> Result<ResultValue, CacheError> + Send + 'static,
    {
        self.require_active()?;
        let result = self.workers.read_snapshot(operation).await;
        self.require_active()?;
        result
    }

    pub(super) async fn write_transaction<ResultValue, Operation>(
        &self,
        operation: Operation,
    ) -> Result<ResultValue, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&Transaction<'_>) -> Result<ResultValue, CacheError> + Send + 'static,
    {
        self.require_active()?;
        let result = self.workers.write_transaction(operation).await;
        self.require_active()?;
        result
    }

    pub(crate) async fn metadata(&self) -> Result<CacheFileMetadata, CacheError> {
        self.read_snapshot(|snapshot| super::database::read_metadata(snapshot))
            .await
    }

    pub(super) fn require_active(&self) -> Result<(), CacheError> {
        if self.is_active.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(CacheError::Invalidated)
        }
    }
}

impl ImageLease {
    // Generation-scoped file/index commands never enter the media writer queue.
    pub(super) async fn validate_bytes(&self, bytes: Vec<u8>) -> Result<ImagePayload, CacheError> {
        self.require_active()?;
        let result = self.workers.validate_bytes(bytes).await;
        self.require_active()?;
        result
    }

    pub(super) async fn validate_inline(&self, source: String) -> Result<ImagePayload, CacheError> {
        self.require_active()?;
        let result = self.workers.validate_inline(source).await;
        self.require_active()?;
        result
    }

    pub(super) async fn publish_variant(
        &self,
        key: ImageVariantKey,
        bytes: Vec<u8>,
        observed_at: i64,
    ) -> Result<(), CacheError> {
        self.require_active()?;
        let result = self.workers.publish_variant(key, bytes, observed_at).await;
        self.require_active()?;
        result
    }

    pub(super) async fn publish_payload(
        &self,
        key: ImageVariantKey,
        payload: ImagePayload,
        observed_at: i64,
    ) -> Result<(), CacheError> {
        self.require_active()?;
        let result = self.workers.publish_payload(key, payload, observed_at).await;
        self.require_active()?;
        result
    }

    pub(super) async fn record_not_found(
        &self,
        key: ImageVariantKey,
        expected_source_revision: Option<i64>,
    ) -> Result<(), CacheError> {
        self.require_active()?;
        let result = self
            .workers
            .record_not_found(key, expected_source_revision)
            .await;
        self.require_active()?;
        result
    }

    // File leases and refresh snapshots retain the same generation guard.
    pub(super) async fn read_variant(
        &self,
        key: ImageVariantKey,
    ) -> Result<Option<ImageVariantLease>, CacheError> {
        self.require_active()?;
        let result = self.workers.read_variant(key).await;
        self.require_active()?;
        result
    }

    pub(super) async fn read_refresh_state(
        &self,
        key: ImageVariantKey,
    ) -> Result<ImageRefreshState, CacheError> {
        self.require_active()?;
        let result = self.workers.read_refresh_state(key).await;
        self.require_active()?;
        result
    }

    pub(super) async fn read_inline_refresh_state(
        &self,
        key: ImageVariantKey,
    ) -> Result<ImageRefreshState, CacheError> {
        self.require_active()?;
        let result = self.workers.read_inline_refresh_state(key).await;
        self.require_active()?;
        result
    }

    pub(super) fn require_active(&self) -> Result<(), CacheError> {
        if self.is_active.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(CacheError::Invalidated)
        }
    }

    // Refresh subscribers stop their network futures when this generation is invalidated.
    pub(super) fn observe_invalidation(&self) -> watch::Receiver<bool> {
        self.invalidation.subscribe()
    }

    pub(super) fn generation_id(&self) -> uuid::Uuid {
        self.generation_id
    }

    fn invalidate(&self) {
        self.is_active.store(false, Ordering::Release);
        self.invalidation.send_replace(true);
    }
}

impl CacheRuntime {
    // Worker quiescence, physical maintenance and publication.
    async fn perform(
        &mut self,
        operation: CacheMaintenance,
        server_id: Option<String>,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Result<(), CacheError> {
        match operation {
            CacheMaintenance::ActivateServer | CacheMaintenance::ActivateImages => {
                let server_id = server_id.ok_or(CacheError::InvalidServerId)?;
                if self.snapshot.server_id.as_ref() != Some(&server_id) {
                    self.stop_caches(changes).await?;
                    let storage = self.storage.clone();
                    let next_server_id = server_id.clone();
                    blocking(move || {
                        storage.clear_database_files()?;
                        storage.clear_image_files()?;
                        storage.write_server_id(&next_server_id)
                    })
                    .await?;
                    self.snapshot.server_id = Some(server_id);
                }
                if operation == CacheMaintenance::ActivateServer {
                    self.start_database().await?;
                }
                self.start_images().await
            }
            CacheMaintenance::ClearDatabase => {
                self.stop_database(changes).await?;
                let storage = self.storage.clone();
                blocking(move || storage.clear_database_files()).await?;
                self.start_database().await
            }
            CacheMaintenance::ClearImages => {
                self.stop_images(changes).await?;
                let storage = self.storage.clone();
                blocking(move || storage.clear_image_files()).await?;
                self.start_images().await
            }
            CacheMaintenance::Shutdown => {
                self.snapshot.is_shutdown = true;
                self.stop_caches(changes).await
            }
        }
    }

    async fn stop_database(
        &mut self,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Result<(), CacheError> {
        if let Some(database) = self.invalidate_database(changes) {
            database.workers.shutdown().await?;
        }
        Ok(())
    }

    async fn stop_images(
        &mut self,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Result<(), CacheError> {
        if let Some(images) = self.invalidate_images(changes) {
            images.workers.shutdown().await?;
        }
        Ok(())
    }

    async fn stop_caches(
        &mut self,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Result<(), CacheError> {
        let database = self.invalidate_database(changes);
        let images = self.invalidate_images(changes);
        let (database_result, image_result) = tokio::join!(
            async {
                match database {
                    Some(database) => database.workers.shutdown().await,
                    None => Ok(()),
                }
            },
            async {
                match images {
                    Some(images) => images.workers.shutdown().await,
                    None => Ok(()),
                }
            },
        );
        database_result.and(image_result)
    }

    fn invalidate_database(
        &mut self,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Option<DatabaseLease> {
        self.snapshot.database_generation += 1;
        self.snapshot.is_database_active = false;
        let database = self.database.take();
        if let Some(database) = database.as_ref() {
            database.is_active.store(false, Ordering::Release);
        }
        self.published_database
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        changes.send_replace(self.snapshot.clone());
        database
    }

    fn invalidate_images(
        &mut self,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Option<ImageLease> {
        self.snapshot.image_generation += 1;
        self.snapshot.is_image_active = false;
        let images = self.images.take();
        if let Some(images) = images.as_ref() {
            images.invalidate();
        }
        self.published_images
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        changes.send_replace(self.snapshot.clone());
        images
    }

    async fn start_database(&mut self) -> Result<(), CacheError> {
        if self.database.is_some() {
            return Ok(());
        }
        if let Some(server_id) = self.snapshot.server_id.as_ref() {
            let workers =
                DatabaseWorkers::initialize(self.storage.database_directory(), server_id.clone())
                    .await?;
            self.database = Some(DatabaseLease {
                workers,
                is_active: Arc::new(AtomicBool::new(true)),
            });
            *self
                .published_database
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = self.database.clone();
            self.snapshot.is_database_active = true;
        }
        Ok(())
    }

    async fn start_images(&mut self) -> Result<(), CacheError> {
        if self.images.is_some() {
            return Ok(());
        }
        if let Some(server_id) = self.snapshot.server_id.clone() {
            let storage = self.storage.clone();
            let workers =
                tokio::task::spawn_blocking(move || ImageWorkers::initialize(storage, server_id))
                    .await
                    .map_err(|_| CacheError::WorkerPanicked)??;
            self.images = Some(ImageLease {
                refreshes: Arc::new(ImageRefreshes::default()),
                workers,
                is_active: Arc::new(AtomicBool::new(true)),
                invalidation: watch::channel(false).0,
                generation_id: uuid::Uuid::new_v4(),
            });
            *self
                .published_images
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = self.images.clone();
            self.snapshot.is_image_active = true;
        }
        Ok(())
    }
}

impl Drop for CacheState {
    fn drop(&mut self) {
        let runtime = self.runtime.get_mut();
        runtime
            .published_database
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        runtime
            .published_images
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let database = runtime.database.take();
        let images = runtime.images.take();
        if let Some(database) = database.as_ref() {
            database.is_active.store(false, Ordering::Release);
        }
        if let Some(images) = images.as_ref() {
            images.invalidate();
        }
        if database.is_none() && images.is_none() {
            return;
        }
        let storage = runtime.storage.clone();
        let changes = self.changes.clone();
        let mut snapshot = runtime.snapshot.clone();
        snapshot.is_database_active = false;
        snapshot.is_image_active = false;
        snapshot.is_shutdown = true;
        snapshot.database_generation += 1;
        snapshot.image_generation += 1;
        let _ = std::thread::Builder::new()
            .name("mashin-cache-drop".to_owned())
            .spawn(move || {
                let database_result = database
                    .map(|database| database.workers.shutdown_blocking())
                    .unwrap_or(Ok(()));
                let image_result = images
                    .map(|images| images.workers.shutdown_blocking())
                    .unwrap_or(Ok(()));
                snapshot.last_error = database_result.and(image_result).err();
                drop(storage);
                changes.send_replace(snapshot);
            });
    }
}

// Blocking filesystem execution.
async fn blocking<Operation>(operation: Operation) -> Result<(), CacheError>
where
    Operation: FnOnce() -> Result<(), CacheError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| CacheError::WorkerPanicked)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn initialize(directory: &std::path::Path, server_id: &str) -> ServerCache {
        let cache = ServerCache::initialize(directory.to_owned()).await.unwrap();
        cache.activate(server_id.to_owned()).await.unwrap();
        cache
    }

    async fn seed(database: &DatabaseLease) {
        database
            .write_transaction(|transaction| {
                transaction.execute("CREATE TABLE probe (value INTEGER NOT NULL)", [])?;
                transaction.execute("INSERT INTO probe VALUES (7)", [])?;
                Ok(())
            })
            .await
            .unwrap();
    }

    async fn has_probe(database: &DatabaseLease) -> bool {
        database
            .read_snapshot(|snapshot| {
                Ok(snapshot.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = 'probe'",
                    [],
                    |row| row.get::<_, i32>(0),
                )? == 1)
            })
            .await
            .unwrap()
    }

    fn image_path(storage: &CacheStorage) -> std::path::PathBuf {
        let shard = storage.image_directory().join("aa");
        std::fs::create_dir_all(&shard).unwrap();
        shard.join(format!("{}.png", "a".repeat(64)))
    }

    fn image_key() -> ImageVariantKey {
        use super::super::image_sources::{
            ImageOutputFormat, ImageSize, ImageSourceKey, ImageVariantParameters,
        };
        ImageVariantKey {
            source: ImageSourceKey::ProxyId("b".repeat(64)),
            parameters: ImageVariantParameters {
                size: ImageSize::Pixels80,
                output_format: ImageOutputFormat::Png,
            },
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cache_usage_does_not_activate_databases_or_server_identity() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ServerCache::initialize(directory.path().to_owned()).await.unwrap();
        let usage = cache.usage().await.unwrap();
        assert_eq!(usage.metadata_bytes, 0);
        assert_eq!(usage.image_bytes, 0);
        let snapshot = cache.observe().borrow().clone();
        assert!(!snapshot.is_database_active);
        assert!(!snapshot.is_image_active);
        assert_eq!(snapshot.database_generation, 0);
        assert_eq!(snapshot.image_generation, 0);
        assert!(snapshot.server_id.is_none());
        assert!(!directory.path().join("Cache").join(super::super::database::DATABASE_FILE_NAME).exists());
        assert!(!directory.path().join("Cache/images/index.sqlite3").exists());
        cache.shutdown().await.unwrap();
        assert!(matches!(cache.usage().await, Err(CacheError::ShuttingDown)));
        drop(cache);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn image_clear_drains_leases_without_blocking_media_and_survives_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let database = cache.database().await.unwrap();
        seed(&database).await;
        let images = cache.images().await.unwrap();
        images
            .publish_variant(image_key(), super::super::image_files::tests::bitmap(3), 1)
            .await
            .unwrap();
        let leased = images.read_variant(image_key()).await.unwrap().unwrap();
        let before = cache.observe().borrow().clone();
        let mut changes = cache.observe();
        let clearing = cache.clone();
        let clear = tokio::spawn(async move { clearing.clear_image_cache().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while changes.borrow().is_image_active {
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            images.require_active(),
            Err(CacheError::Invalidated)
        ));
        assert!(matches!(
            images
                .publish_variant(image_key(), super::super::image_files::tests::bitmap(4), 2)
                .await,
            Err(CacheError::Invalidated)
        ));
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), cache.images())
                .await
                .unwrap(),
            Err(CacheError::CacheInactive)
        ));
        assert!(has_probe(&database).await);
        assert!(!clear.is_finished());
        clear.abort();
        assert!(clear.await.unwrap_err().is_cancelled());
        drop(leased);
        tokio::time::timeout(Duration::from_secs(2), async {
            while changes.borrow().maintenance.is_some() {
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(changes.borrow().last_error.is_none());
        assert_eq!(
            changes.borrow().database_generation,
            before.database_generation
        );
        assert_eq!(
            changes.borrow().image_generation,
            before.image_generation + 1
        );
        assert!(cache
            .images()
            .await
            .unwrap()
            .read_variant(image_key())
            .await
            .unwrap()
            .is_none());
        cache.shutdown().await.unwrap();
        drop(cache);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn images_survive_database_clear_and_restart_but_not_server_switch() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        cache.bind_server_address("server-a".into(), "http://localhost:8095".into()).await.unwrap();
        let images = cache.images().await.unwrap();
        let bytes = super::super::image_files::tests::bitmap(5);
        images
            .publish_variant(image_key(), bytes.clone(), 1)
            .await
            .unwrap();
        let leased = images.read_variant(image_key()).await.unwrap().unwrap();
        let generation = cache.observe().borrow().image_generation;
        cache.activate("server-a".to_owned()).await.unwrap();
        cache.clear_database_cache().await.unwrap();
        images.require_active().unwrap();
        assert_eq!(cache.observe().borrow().image_generation, generation);
        assert_eq!(leased.read.payload.bytes, bytes);
        drop(leased);
        cache.shutdown().await.unwrap();
        drop(cache);
        assert!(matches!(
            images.require_active(),
            Err(CacheError::Invalidated)
        ));
        let cache = initialize(directory.path(), "server-a").await;
        assert_eq!(cache.server_id_for_address("http://localhost:8095".into()).await.unwrap().as_deref(), Some("server-a"));
        let images = cache.images().await.unwrap();
        let leased = images.read_variant(image_key()).await.unwrap().unwrap();
        assert_eq!(leased.read.payload.bytes, bytes);
        let database = cache.database().await.unwrap();
        let mut changes = cache.observe();
        let switching = cache.clone();
        let switch = tokio::spawn(async move { switching.activate("server-b".to_owned()).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while changes.borrow().is_image_active {
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            database.metadata().await,
            Err(CacheError::Invalidated)
        ));
        assert!(matches!(
            images.require_active(),
            Err(CacheError::Invalidated)
        ));
        assert!(!switch.is_finished());
        drop(leased);
        tokio::time::timeout(Duration::from_secs(2), switch)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(cache
            .images()
            .await
            .unwrap()
            .read_variant(image_key())
            .await
            .unwrap()
            .is_none());
        cache.activate("server-a".to_owned()).await.unwrap();
        assert!(cache
            .images()
            .await
            .unwrap()
            .read_variant(image_key())
            .await
            .unwrap()
            .is_none());
        cache.shutdown().await.unwrap();
        cache.shutdown().await.unwrap();
        drop(cache);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_cache_drains_image_leases() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let images = cache.images().await.unwrap();
        images
            .publish_variant(image_key(), super::super::image_files::tests::bitmap(6), 1)
            .await
            .unwrap();
        let leased = images.read_variant(image_key()).await.unwrap().unwrap();
        cache
            .state
            .runtime
            .lock()
            .await
            .stop_database(&cache.state.changes)
            .await
            .unwrap();
        assert!(matches!(
            cache.database().await,
            Err(CacheError::CacheInactive)
        ));
        let mut changes = cache.observe();
        drop(cache);
        assert!(matches!(
            images.require_active(),
            Err(CacheError::Invalidated)
        ));
        let another = ServerCache::initialize(directory.path().to_owned()).await.unwrap();
        another.shutdown().await.unwrap();
        drop(another);
        assert!(!changes.borrow().is_shutdown);
        drop(leased);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !changes.borrow().is_shutdown {
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(changes.borrow().last_error.is_none());
        let reopened = initialize(directory.path(), "server-a").await;
        assert!(reopened
            .images()
            .await
            .unwrap()
            .read_variant(image_key())
            .await
            .unwrap()
            .is_some());
        reopened.shutdown().await.unwrap();
        drop(reopened);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_invalidates_both_areas_and_drains_image_leases() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let database = cache.database().await.unwrap();
        let images = cache.images().await.unwrap();
        images
            .publish_variant(image_key(), super::super::image_files::tests::bitmap(7), 1)
            .await
            .unwrap();
        let leased = images.read_variant(image_key()).await.unwrap().unwrap();
        let mut changes = cache.observe();
        let closing = cache.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while changes.borrow().is_image_active {
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            database.metadata().await,
            Err(CacheError::Invalidated)
        ));
        assert!(matches!(
            images.require_active(),
            Err(CacheError::Invalidated)
        ));
        assert!(matches!(
            cache.images().await,
            Err(CacheError::ShuttingDown)
        ));
        let another = ServerCache::initialize(directory.path().to_owned()).await.unwrap();
        another.shutdown().await.unwrap();
        drop(another);
        assert!(!shutdown.is_finished());
        shutdown.abort();
        assert!(shutdown.await.unwrap_err().is_cancelled());
        drop(leased);
        tokio::time::timeout(Duration::from_secs(2), cache.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(cache.observe().borrow().maintenance.is_none());
        drop(cache);
        let reopened = initialize(directory.path(), "server-a").await;
        assert!(reopened
            .images()
            .await
            .unwrap()
            .read_variant(image_key())
            .await
            .unwrap()
            .is_some());
        reopened.shutdown().await.unwrap();
        drop(reopened);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn same_server_reactivation_and_restart_preserve_observations() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let database = cache.database().await.unwrap();
        seed(&database).await;
        let before = cache.observe().borrow().clone();
        cache.activate("server-a".to_owned()).await.unwrap();
        assert_eq!(
            cache.observe().borrow().database_generation,
            before.database_generation
        );
        assert!(has_probe(&database).await);
        cache.shutdown().await.unwrap();
        let reopened = initialize(directory.path(), "server-a").await;
        assert!(has_probe(&reopened.database().await.unwrap()).await);
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clearing_each_area_preserves_the_other_and_notifies_generations() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let storage = CacheStorage::open(directory.path()).unwrap();
        let image = image_path(&storage);
        let index = storage
            .image_directory()
            .join(super::super::storage::IMAGE_DATABASE_FILE_NAME);
        std::fs::write(&image, b"synthetic image").unwrap();
        std::fs::write(&index, b"synthetic index").unwrap();
        let old = cache.database().await.unwrap();
        seed(&old).await;
        let before = cache.observe().borrow().clone();
        cache.clear_database_cache().await.unwrap();
        assert!(matches!(old.metadata().await, Err(CacheError::Invalidated)));
        assert!(matches!(
            old.write_transaction(|_| Ok(())).await,
            Err(CacheError::Invalidated)
        ));
        assert!(!has_probe(&cache.database().await.unwrap()).await);
        assert_eq!(std::fs::read(&image).unwrap(), b"synthetic image");
        assert_eq!(std::fs::read(&index).unwrap(), b"synthetic index");
        assert_eq!(
            cache.observe().borrow().image_generation,
            before.image_generation
        );
        let current = cache.database().await.unwrap();
        seed(&current).await;
        let database_generation = cache.observe().borrow().database_generation;
        cache.clear_image_cache().await.unwrap();
        cache.clear_image_cache().await.unwrap();
        assert!(has_probe(&current).await);
        assert!(!image.exists());
        assert!(!index.exists());
        assert_eq!(
            cache.observe().borrow().database_generation,
            database_generation
        );
        assert_eq!(
            storage.read_server_id().unwrap().as_deref(),
            Some("server-a")
        );
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn server_switch_clears_both_areas_and_never_restores_a_previous_server() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let storage = CacheStorage::open(directory.path()).unwrap();
        let image = image_path(&storage);
        std::fs::write(&image, b"server-a image").unwrap();
        let old = cache.database().await.unwrap();
        seed(&old).await;
        cache.activate("server-b".to_owned()).await.unwrap();
        assert!(matches!(old.metadata().await, Err(CacheError::Invalidated)));
        assert!(!image.exists());
        assert!(!has_probe(&cache.database().await.unwrap()).await);
        assert_eq!(
            storage.read_server_id().unwrap().as_deref(),
            Some("server-b")
        );
        cache.activate("server-a".to_owned()).await.unwrap();
        assert!(!has_probe(&cache.database().await.unwrap()).await);
        cache.shutdown().await.unwrap();
        let reopened = initialize(directory.path(), "server-b").await;
        assert!(!has_probe(&reopened.database().await.unwrap()).await);
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clearing_waits_for_accepted_write_and_survives_caller_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let old = cache.database().await.unwrap();
        seed(&old).await;
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let write = tokio::spawn(async move {
            old.write_transaction(move |transaction| {
                transaction.execute("UPDATE probe SET value = 9", [])?;
                started.send(()).unwrap();
                hold.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
            .await
        });
        start.await.unwrap();
        let mut changes = cache.observe();
        let clearing = cache.clone();
        let clear = tokio::spawn(async move { clearing.clear_database_cache().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !changes.borrow_and_update().is_database_active {
                    break;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(!clear.is_finished());
        clear.abort();
        assert!(clear.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        assert!(matches!(write.await.unwrap(), Err(CacheError::Invalidated)));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if changes.borrow_and_update().maintenance.is_none() {
                    break;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(!has_probe(&cache.database().await.unwrap()).await);
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn initialization_preserves_data_and_dropping_cache_drains_handles() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let old = cache.database().await.unwrap();
        let mut changes = cache.observe();
        let another = ServerCache::initialize(directory.path().to_owned()).await.unwrap();
        another.shutdown().await.unwrap();
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let writing = old.clone();
        let write = tokio::spawn(async move {
            writing
                .write_transaction(move |transaction| {
                    transaction.execute("CREATE TABLE accepted_before_drop (value INTEGER)", [])?;
                    started.send(()).unwrap();
                    hold.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(())
                })
                .await
        });
        start.await.unwrap();
        drop(cache);
        assert!(matches!(old.metadata().await, Err(CacheError::Invalidated)));
        release.send(()).unwrap();
        assert!(matches!(write.await.unwrap(), Err(CacheError::Invalidated)));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if changes.borrow_and_update().is_shutdown {
                    break;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(matches!(old.metadata().await, Err(CacheError::Invalidated)));
        let reopened = initialize(directory.path(), "server-a").await;
        assert!(reopened
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                Ok(snapshot.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = 'accepted_before_drop'",
                    [],
                    |row| row.get::<_, i32>(0),
                )? == 1)
            })
            .await
            .unwrap());
        reopened.shutdown().await.unwrap();
        reopened.shutdown().await.unwrap();
        assert!(matches!(
            reopened.activate("server-b".to_owned()).await,
            Err(CacheError::ShuttingDown)
        ));
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn failed_switch_does_not_activate_target_and_retry_is_safe() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("config.json"), b"protected settings").unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let storage = CacheStorage::open(directory.path()).unwrap();
        let image = image_path(&storage);
        std::fs::write(&image, b"synthetic locked image").unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&image)
            .unwrap();
        assert!(cache.activate("server-b".to_owned()).await.is_err());
        assert!(matches!(
            cache.database().await,
            Err(CacheError::CacheInactive)
        ));
        assert_eq!(
            storage.read_server_id().unwrap().as_deref(),
            Some("server-a")
        );
        assert!(cache.observe().borrow().last_error.is_some());
        drop(lock);
        cache.activate("server-b".to_owned()).await.unwrap();
        assert_eq!(
            cache
                .database()
                .await
                .unwrap()
                .metadata()
                .await
                .unwrap()
                .server_id,
            "server-b"
        );
        assert!(!image.exists());
        assert_eq!(
            std::fs::read(directory.path().join("config.json")).unwrap(),
            b"protected settings"
        );
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unidentified_existing_data_is_preserved_and_empty_id_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(directory.path()).unwrap();
        let image = image_path(&storage);
        std::fs::write(&image, b"unidentified image").unwrap();
        let reopened = ServerCache::initialize(directory.path().to_owned()).await.unwrap();
        reopened.shutdown().await.unwrap();
        assert_eq!(std::fs::read(image).unwrap(), b"unidentified image");
        let empty = tempfile::tempdir().unwrap();
        let cache = ServerCache::initialize(empty.path().to_owned())
            .await
            .unwrap();
        assert!(matches!(
            cache.activate(" \t".to_owned()).await,
            Err(CacheError::InvalidServerId)
        ));
        assert!(!cache.observe().borrow().is_database_active);
        cache.clear_database_cache().await.unwrap();
        cache.clear_image_cache().await.unwrap();
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_clears_are_serialized_and_keep_only_current_access() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let old = cache.database().await.unwrap();
        seed(&old).await;
        let before = cache.observe().borrow().clone();
        let (first, second, images) = tokio::join!(
            cache.clear_database_cache(),
            cache.clear_database_cache(),
            cache.clear_image_cache(),
        );
        first.unwrap();
        second.unwrap();
        images.unwrap();
        assert!(matches!(old.metadata().await, Err(CacheError::Invalidated)));
        assert!(!has_probe(&cache.database().await.unwrap()).await);
        let after = cache.observe().borrow().clone();
        assert_eq!(after.database_generation, before.database_generation + 2);
        assert_eq!(after.image_generation, before.image_generation + 1);
        assert!(after.maintenance.is_none());
        assert!(after.last_error.is_none());
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn repeated_shutdown_preserves_a_recorded_completion_error() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        cache.shutdown().await.unwrap();
        cache.state.runtime.lock().await.snapshot.last_error = Some(CacheError::WorkerPanicked);
        assert!(matches!(
            cache.shutdown().await,
            Err(CacheError::WorkerPanicked)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn database_access_does_not_wait_for_the_maintenance_mutex() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let maintenance = cache.state.runtime.lock().await;
        let database = tokio::time::timeout(Duration::from_secs(2), cache.database())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(database.metadata().await.unwrap().server_id, "server-a");
        drop(maintenance);
        cache.shutdown().await.unwrap();
    }
}
