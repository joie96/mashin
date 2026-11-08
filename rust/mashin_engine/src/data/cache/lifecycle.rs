//! Serialized cache maintenance with persistent server binding and invalidatable DB access.
//! Maintenance continues after cancellation of its caller. Blocking file operations and SQL
//! never run on the async executor. Session catalogs hold generation-bound DB leases;
//! image maintenance currently owns only the physical image cache area, not image downloads.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rusqlite::Transaction;
use tokio::sync::{watch, Mutex};

use super::database::{CacheError, CacheFileMetadata};
use super::storage::CacheStorage;
use super::workers::DatabaseWorkers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheMaintenance {
    ActivateServer,
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
    changes: watch::Sender<CacheLifecycleSnapshot>,
}

struct CacheRuntime {
    storage: Arc<CacheStorage>,
    owner_lock: Option<Arc<std::fs::File>>,
    database: Option<DatabaseLease>,
    snapshot: CacheLifecycleSnapshot,
    published_database: Arc<std::sync::Mutex<Option<DatabaseLease>>>,
}

#[derive(Clone)]
pub(crate) struct DatabaseLease {
    workers: DatabaseWorkers,
    is_active: Arc<AtomicBool>,
}

impl ServerCache {
    // Initialization and lifecycle.
    pub(crate) async fn initialize(
        application_data_directory: PathBuf,
    ) -> Result<Self, CacheError> {
        let (storage, owner_lock, server_id) = tokio::task::spawn_blocking(move || {
            let storage = CacheStorage::open(&application_data_directory)?;
            let owner_lock = storage.lock_owner()?;
            let server_id = storage.read_server_id()?;
            Ok::<_, CacheError>((storage, owner_lock, server_id))
        })
        .await
        .map_err(|_| CacheError::WorkerPanicked)??;
        let snapshot = CacheLifecycleSnapshot {
            server_id,
            database_generation: 0,
            image_generation: 0,
            is_database_active: false,
            is_shutdown: false,
            maintenance: None,
            last_error: None,
        };
        let (changes, _) = watch::channel(snapshot.clone());
        let published_database = Arc::new(std::sync::Mutex::new(None));
        Ok(Self {
            state: Arc::new(CacheState {
                runtime: Mutex::new(CacheRuntime {
                    storage: Arc::new(storage),
                    owner_lock: Some(Arc::new(owner_lock)),
                    database: None,
                    published_database: published_database.clone(),
                    snapshot,
                }),
                published_database,
                changes,
            }),
        })
    }

    /// Accepts the observed server ID, never an address or user ID.
    pub(crate) async fn activate(&self, server_id: String) -> Result<(), CacheError> {
        self.run_maintenance(CacheMaintenance::ActivateServer, Some(server_id))
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

    /// Physical image lifecycle only until the image download/lease owner is connected.
    pub(crate) async fn clear_image_cache(&self) -> Result<(), CacheError> {
        self.run_maintenance(CacheMaintenance::ClearImages, None)
            .await
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

impl CacheRuntime {
    // Worker quiescence, physical maintenance and publication.
    async fn perform(
        &mut self,
        operation: CacheMaintenance,
        server_id: Option<String>,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Result<(), CacheError> {
        match operation {
            CacheMaintenance::ActivateServer => {
                let server_id = server_id.ok_or(CacheError::InvalidServerId)?;
                self.stop_database(changes).await?;
                if self.snapshot.server_id.as_ref() != Some(&server_id) {
                    self.snapshot.image_generation += 1;
                    changes.send_replace(self.snapshot.clone());
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
                self.start_database().await
            }
            CacheMaintenance::ClearDatabase => {
                self.stop_database(changes).await?;
                let storage = self.storage.clone();
                blocking(move || storage.clear_database_files()).await?;
                self.start_database().await
            }
            CacheMaintenance::ClearImages => {
                self.snapshot.image_generation += 1;
                changes.send_replace(self.snapshot.clone());
                let storage = self.storage.clone();
                blocking(move || storage.clear_image_files()).await
            }
            CacheMaintenance::Shutdown => {
                self.snapshot.is_shutdown = true;
                self.snapshot.image_generation += 1;
                let result = self.stop_database(changes).await;
                self.owner_lock.take();
                result
            }
        }
    }

    async fn stop_database(
        &mut self,
        changes: &watch::Sender<CacheLifecycleSnapshot>,
    ) -> Result<(), CacheError> {
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
        if let Some(database) = database {
            database.workers.shutdown().await?;
        }
        Ok(())
    }

    async fn start_database(&mut self) -> Result<(), CacheError> {
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
}

impl Drop for CacheState {
    fn drop(&mut self) {
        let runtime = self.runtime.get_mut();
        runtime
            .published_database
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let Some(database) = runtime.database.take() else {
            return;
        };
        database.is_active.store(false, Ordering::Release);
        let owner_lock = runtime.owner_lock.take();
        let fallback_lock = owner_lock.clone();
        let storage = runtime.storage.clone();
        let changes = self.changes.clone();
        let mut snapshot = runtime.snapshot.clone();
        snapshot.is_database_active = false;
        snapshot.is_shutdown = true;
        snapshot.database_generation += 1;
        snapshot.image_generation += 1;
        if std::thread::Builder::new()
            .name("mashin-cache-drop".to_owned())
            .spawn(move || {
                let result = database.workers.shutdown_blocking();
                snapshot.last_error = result.err();
                if let Some(file) = owner_lock.as_ref() {
                    if let Err(error) = file.unlock() {
                        if snapshot.last_error.is_none() {
                            snapshot.last_error = Some(error.into());
                        }
                    }
                }
                drop(owner_lock);
                drop(storage);
                changes.send_replace(snapshot);
            })
            .is_err()
        {
            // Fail closed if cleanup cannot start: never release ownership over running SQL.
            std::mem::forget(fallback_lock);
        }
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

    fn image_name() -> String {
        format!("mashin-image-{}.bin", "a".repeat(64))
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
        let image = storage.image_directory().join(image_name());
        let index = storage.image_directory().join("mashin-image-cache.sqlite3");
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
        let image = storage.image_directory().join(image_name());
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
    async fn ownership_is_exclusive_and_dropping_owner_drains_handles() {
        let directory = tempfile::tempdir().unwrap();
        let cache = initialize(directory.path(), "server-a").await;
        let old = cache.database().await.unwrap();
        let mut changes = cache.observe();
        assert!(matches!(
            ServerCache::initialize(directory.path().to_owned()).await,
            Err(CacheError::CacheAlreadyOwned)
        ));
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
        assert!(matches!(
            ServerCache::initialize(directory.path().to_owned()).await,
            Err(CacheError::CacheAlreadyOwned)
        ));
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
        let image = storage.image_directory().join(image_name());
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
        let image = storage.image_directory().join(image_name());
        std::fs::write(&image, b"unidentified image").unwrap();
        assert!(matches!(
            ServerCache::initialize(directory.path().to_owned()).await,
            Err(CacheError::InvalidCacheOwnership(_))
        ));
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
