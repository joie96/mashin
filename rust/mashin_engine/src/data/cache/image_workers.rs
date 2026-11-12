//! One independent thread owns lazy image-index work and ordered publication.
//! Stateless validation uses two tracked blocking slots outside the index queue.
//! Shutdown drains accepted jobs, closes SQLite, then waits for issued file handles.
//! Cancellation of a waiter never cancels an accepted publication or its shutdown join.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use tokio::sync::{oneshot, watch};
use futures_util::FutureExt;
use crate::application::repositories::{current_priority, RequestPriority};

use super::database::{CacheError, CacheWalMaintenance};
use super::image_files::ImagePayload;
use super::image_index::{ImageIndex, ImageRefreshState, ImageVariantLease};
use super::image_sources::ImageVariantKey;
use super::storage::CacheStorage;

type ShutdownResult = Result<(), CacheError>;
type ImageJob = Box<dyn FnOnce(&mut Option<ImageIndex>, &CacheStorage, &str) + Send>;
type SharedFileRead = futures_util::future::WeakShared<futures_util::future::BoxFuture<'static, Result<Option<super::image_files::ImageFileRead>, CacheError>>>;

struct QueuedImageJob {
    operation: ImageJob,
    priority: RequestPriority,
    queued_at: Instant,
    is_read: bool,
    is_ready: Option<Arc<std::sync::atomic::AtomicBool>>,
}

fn next_image_job(pending: &mut Vec<QueuedImageJob>) -> Option<QueuedImageJob> {
    let first_write = pending.iter().position(|job| !job.is_read);
    let now = Instant::now();
    let selected = pending.iter().enumerate()
        .filter(|(index, job)| (job.is_read || Some(*index) == first_write)
            && job.is_ready.as_ref().is_none_or(|ready| ready.load(std::sync::atomic::Ordering::Acquire)))
        .max_by_key(|(index, job)| {
            let priority = if now.duration_since(job.queued_at).as_secs() >= 1 {
                RequestPriority::Foreground
            } else { job.priority };
            (priority, std::cmp::Reverse(*index))
        }).map(|(index, _)| index)?;
    Some(pending.remove(selected))
}

#[derive(Clone)]
pub(super) struct ImageWorkers {
    state: Arc<WorkerState>,
}

struct WorkerState {
    queue: Mutex<Option<WorkQueue>>,
    thumbnail_slots: Arc<tokio::sync::Semaphore>,
    admission: Arc<tokio::sync::Semaphore>,
    background_admission: Arc<tokio::sync::Semaphore>,
    validations: std::sync::atomic::AtomicUsize,
    file_reads: Mutex<std::collections::HashMap<super::image_files::ImageFileRecord, SharedFileRead>>,
    completion: watch::Sender<Option<ShutdownResult>>,
    shutdown_wait: Mutex<()>,
    shutdown_wake: Condvar,
}

struct WorkQueue {
    sender: mpsc::Sender<QueuedImageJob>,
    handle: JoinHandle<ShutdownResult>,
}

impl ImageWorkers {
    // Blocking thread setup; index creation is deferred until the first live request.
    pub(super) fn initialize(
        storage: Arc<CacheStorage>,
        server_id: String,
    ) -> Result<Self, CacheError> {
        let (sender, receiver) = mpsc::channel::<QueuedImageJob>();
        let handle = std::thread::Builder::new()
            .name("mashin-cache-images".to_owned())
            .spawn(move || {
                let mut index = None;
                let mut maintenance: Option<CacheWalMaintenance> = None;
                let mut pending = Vec::new();
                loop {
                    pending.extend(receiver.try_iter());
                    let job = if let Some(job) = next_image_job(&mut pending) {
                        Ok(job)
                    } else { match match maintenance.as_ref().and_then(CacheWalMaintenance::delay) {
                        Some(delay) => receiver.recv_timeout(delay),
                        None => receiver
                            .recv()
                            .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                    } {
                        Ok(job) => { pending.push(job); continue; }
                        Err(error) => Err(error),
                    }};
                    match job {
                        Ok(job) => {
                            (job.operation)(&mut index, &storage, &server_id);
                            if let Some(index) = &index {
                                let maintenance = maintenance.get_or_insert_with(CacheWalMaintenance::new);
                                maintenance.record_activity();
                                if maintenance.delay().is_some_and(|delay| delay.is_zero()) {
                                    index.maintain_wal(maintenance);
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let (Some(index), Some(maintenance)) = (&index, &mut maintenance) {
                                index.maintain_wal(maintenance);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                if let Some(index) = index {
                    let leases = index.file_leases();
                    let result = index.shutdown();
                    leases.wait_until_released();
                    result
                } else {
                    Ok(())
                }
            })?;
        let (completion, _) = watch::channel(None);
        Ok(Self {
            state: Arc::new(WorkerState {
                queue: Mutex::new(Some(WorkQueue { sender, handle })),
                thumbnail_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                admission: Arc::new(tokio::sync::Semaphore::new(32)),
                background_admission: Arc::new(tokio::sync::Semaphore::new(8)),
                validations: std::sync::atomic::AtomicUsize::new(0),
                file_reads: Default::default(),
                completion,
                shutdown_wait: Mutex::new(()),
                shutdown_wake: Condvar::new(),
            }),
        })
    }

    pub(super) async fn shutdown(&self) -> ShutdownResult {
        let mut completion = self.state.completion.subscribe();
        if let Some(queue) = self.take_queue()? {
            let state = self.state.clone();
            tokio::task::spawn_blocking(move || state.finish_shutdown(queue.close_and_join()));
        }
        loop {
            if let Some(result) = completion.borrow_and_update().clone() {
                return result;
            }
            completion
                .changed()
                .await
                .map_err(|_| CacheError::WorkerUnavailable)?;
        }
    }

    /// Shared blocking completion for cache-owner Drop, never called on the async executor.
    pub(super) fn shutdown_blocking(&self) -> ShutdownResult {
        if let Some(queue) = self.take_queue()? {
            self.state.finish_shutdown(queue.close_and_join());
        }
        let mut guard = self
            .state
            .shutdown_wait
            .lock()
            .map_err(|_| CacheError::WorkerUnavailable)?;
        loop {
            if let Some(result) = self.state.completion.borrow().clone() {
                return result;
            }
            guard = self
                .state
                .shutdown_wake
                .wait(guard)
                .map_err(|_| CacheError::WorkerUnavailable)?;
        }
    }

    // Accepted file/index commands and leased reads.
    pub(super) async fn validate_bytes(&self, bytes: Vec<u8>) -> Result<ImagePayload, CacheError> {
        self.validate(move || ImagePayload::decode_bytes(bytes)).await
    }

    pub(super) async fn validate_inline(&self, source: String) -> Result<ImagePayload, CacheError> {
        self.validate(move || ImagePayload::decode_inline(&source)).await
    }

    async fn validate<Value: Send + 'static>(&self, operation: impl FnOnce() -> Result<Value, CacheError> + Send + 'static) -> Result<Value, CacheError> {
        let permit = self.state.thumbnail_slots.clone().acquire_owned().await.map_err(|_| CacheError::ShuttingDown)?;
        {
            let queue = self.state.queue.lock().map_err(|_| CacheError::WorkerUnavailable)?;
            if queue.is_none() { return Err(CacheError::ShuttingDown); }
            self.state.validations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let result = catch_unwind(AssertUnwindSafe(operation)).unwrap_or(Err(CacheError::WorkerPanicked));
            let _guard = state.shutdown_wait.lock().unwrap_or_else(|error| error.into_inner());
            state.validations.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            state.shutdown_wake.notify_all();
            result
        }).await.map_err(|_| CacheError::WorkerPanicked)?
    }

    pub(super) async fn publish_variant(
        &self,
        key: ImageVariantKey,
        bytes: Vec<u8>,
        observed_at: i64,
    ) -> Result<(), CacheError> {
        self.publish_prepared_variant(key, observed_at, None, move |files| files.publish(bytes)).await
    }

    pub(super) async fn publish_payload(
        &self,
        key: ImageVariantKey,
        payload: ImagePayload,
        observed_at: i64,
    ) -> Result<(), CacheError> {
        self.publish_prepared_variant(key, observed_at, None, move |files| files.publish_payload(payload)).await
    }

    async fn publish_prepared_variant(
        &self,
        key: ImageVariantKey,
        observed_at: i64,
        source_revision: Option<i64>,
        prepare: impl FnOnce(&super::image_files::ImageFiles) -> Result<super::image_files::ImageFileRecord, CacheError> + Send + 'static,
    ) -> Result<(), CacheError> {
        let files = self.execute(true, |index| Ok(index.publication_files())).await?;
        let permit = self.state.thumbnail_slots.clone().acquire_owned().await.map_err(|_| CacheError::ShuttingDown)?;
        let is_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let prepared = Arc::new(Mutex::new(None));
        let result = prepared.clone();
        let ready = is_ready.clone();
        let preparation = Box::new(move |sender: mpsc::Sender<QueuedImageJob>| {
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                *result.lock().unwrap_or_else(|error| error.into_inner()) = Some(
                    catch_unwind(AssertUnwindSafe(|| prepare(&files))).unwrap_or(Err(CacheError::WorkerPanicked))
                );
                ready.store(true, std::sync::atomic::Ordering::Release);
                let _ = sender.send(QueuedImageJob {
                    operation: Box::new(|_, _, _| {}),
                    priority: RequestPriority::Background,
                    queued_at: Instant::now(),
                    is_read: true,
                    is_ready: None,
                });
            });
        });
        self.enqueue_prepared(false, Some(is_ready), Some(preparation), move |index, storage, server_id| {
            let file = prepared.lock().unwrap_or_else(|error| error.into_inner()).take().ok_or(CacheError::WorkerUnavailable)??;
            if index.is_none() { *index = Some(ImageIndex::open(storage, server_id)?); }
            index.as_mut().ok_or(CacheError::WorkerUnavailable)?.publish_file_observation(&key, file, observed_at, source_revision)
        }).await
    }

    // Refresh metadata shares the index owner and its generation-scoped lifetime.
    pub(super) async fn record_not_found(
        &self,
        key: ImageVariantKey,
        expected_source_revision: Option<i64>,
    ) -> Result<(), CacheError> {
        self.execute(false, move |index| {
            index.record_not_found(&key, expected_source_revision, std::time::Instant::now())
        })
        .await
    }

    // File leases and refresh snapshots are read through the same owner queue.
    pub(super) async fn read_variant(
        &self,
        key: ImageVariantKey,
    ) -> Result<Option<ImageVariantLease>, CacheError> {
        let Some(pending) = self.execute(true, move |index| index.prepare_variant(&key)).await? else {
            return Ok(None);
        };
        pending.validate_with(|file| self.read_file(file)).await
    }

    /// Derives missing thumbnails off the index thread and fences publication by source revision.
    pub(super) async fn read_inline_refresh_state(
        &self,
        key: ImageVariantKey,
    ) -> Result<ImageRefreshState, CacheError> {
        let state = self.read_refresh_state(key.clone()).await?;
        if state.cached.as_ref().is_some_and(|cached| cached.read.is_current_source()) {
            return Ok(state);
        }
        let size = key.parameters.size.pixels();
        if size == 0 {
            return Ok(state);
        }
        let mut original_key = key.clone();
        original_key.parameters.size = super::image_sources::ImageSize::Original;
        let Some(original) = self
            .read_variant(original_key)
            .await?
            .filter(|original| original.read.is_current_source())
        else {
            return Ok(state);
        };
        let source_revision = original.read.source_revision;
        let observed_at = original.read.observed_at;
        let permit = self.state.thumbnail_slots.clone().acquire_owned().await.map_err(|_| CacheError::ShuttingDown)?;
        let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, CacheError> {
            let _permit = permit;
            let bitmap = image::load_from_memory(&original.read.payload.bytes)
                .map_err(|_| CacheError::InvalidImageObservation("invalid thumbnail source"))?;
            if bitmap.width() <= u32::from(size) && bitmap.height() <= u32::from(size) {
                return Ok(original.read.payload.bytes);
            }
            let thumbnail = bitmap.thumbnail(u32::from(size), u32::from(size));
            let mut encoded = std::io::Cursor::new(Vec::new());
            thumbnail
                .write_to(&mut encoded, image::ImageFormat::Png)
                .map_err(|_| CacheError::InvalidImageObservation("thumbnail encoding failed"))?;
            Ok(encoded.into_inner())
        })
        .await
        .map_err(|_| CacheError::WorkerUnavailable)??;
        let published_key = key.clone();
        self.publish_prepared_variant(published_key, observed_at, Some(source_revision), move |files| files.publish(bytes)).await?;
        self.read_refresh_state(key).await
    }

    pub(super) async fn read_refresh_state(
        &self,
        key: ImageVariantKey,
    ) -> Result<ImageRefreshState, CacheError> {
        let pending = self.execute(true, move |index| {
            index.prepare_refresh_state(&key, std::time::Instant::now())
        })
        .await?;
        if !pending.requires_validation() {
            return pending.validate();
        }
        pending.validate_with(|file| self.read_file(file)).await
    }

    async fn read_file(&self, file: super::image_files::PendingImageFileRead) -> Result<Option<super::image_files::ImageFileRead>, CacheError> {
        let read = {
            let mut reads = self.state.file_reads.lock().map_err(|_| CacheError::WorkerUnavailable)?;
            reads.retain(|_, read| read.upgrade().is_some());
            let key = file.record();
            if let Some(read) = reads.get(&key).and_then(|read| read.upgrade()) {
                read
            } else {
                let workers = self.clone();
                let read = async move { workers.validate(move || file.validate()).await }.boxed().shared();
                reads.insert(key, read.downgrade().ok_or(CacheError::WorkerUnavailable)?);
                read
            }
        };
        read.await
    }

    // Queue admission and per-request panic isolation.
    async fn execute<ResultValue, Operation>(
        &self,
        is_read: bool,
        operation: Operation,
    ) -> Result<ResultValue, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&mut ImageIndex) -> Result<ResultValue, CacheError> + Send + 'static,
    {
        self.enqueue(is_read, move |index, storage, server_id| {
            if index.is_none() {
                *index = Some(ImageIndex::open(storage, server_id)?);
            }
            operation(index.as_mut().ok_or(CacheError::WorkerUnavailable)?)
        })
        .await
    }

    async fn enqueue<ResultValue, Operation>(
        &self,
        is_read: bool,
        operation: Operation,
    ) -> Result<ResultValue, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&mut Option<ImageIndex>, &CacheStorage, &str) -> Result<ResultValue, CacheError>
            + Send
            + 'static,
    {
        self.enqueue_prepared(is_read, None, None, operation).await
    }

    async fn enqueue_prepared<ResultValue, Operation>(
        &self,
        is_read: bool,
        is_ready: Option<Arc<std::sync::atomic::AtomicBool>>,
        preparation: Option<Box<dyn FnOnce(mpsc::Sender<QueuedImageJob>) + Send>>,
        operation: Operation,
    ) -> Result<ResultValue, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&mut Option<ImageIndex>, &CacheStorage, &str) -> Result<ResultValue, CacheError> + Send + 'static,
    {
        let priority = current_priority();
        let background = if priority < RequestPriority::Icon {
            Some(self.state.background_admission.clone().acquire_owned().await.map_err(|_| CacheError::ShuttingDown)?)
        } else { None };
        let admission = self.state.admission.clone().acquire_owned().await.map_err(|_| CacheError::ShuttingDown)?;
        let (response, receiver) = oneshot::channel();
        let job: ImageJob = Box::new(move |index, storage, server_id| {
            let _admission = admission;
            let _background = background;
            if is_read && response.is_closed() {
                return;
            }
            let result = catch_unwind(AssertUnwindSafe(|| operation(index, storage, server_id)))
                .unwrap_or(Err(CacheError::WorkerPanicked));
            let _ = response.send(result);
        });
        {
            let queue = self
                .state
                .queue
                .lock()
                .map_err(|_| CacheError::WorkerUnavailable)?;
            let queue = queue.as_ref().ok_or(CacheError::ShuttingDown)?;
            queue.sender
                .send(QueuedImageJob { operation: job, priority, queued_at: Instant::now(), is_read, is_ready })
                .map_err(|_| CacheError::WorkerUnavailable)?;
            if let Some(preparation) = preparation { preparation(queue.sender.clone()); }
        }
        receiver.await.map_err(|_| CacheError::WorkerUnavailable)?
    }

    fn take_queue(&self) -> Result<Option<WorkQueue>, CacheError> {
        self.state.admission.close();
        self.state.background_admission.close();
        self.state.thumbnail_slots.close();
        Ok(self
            .state
            .queue
            .lock()
            .map_err(|_| CacheError::WorkerUnavailable)?
            .take())
    }
}

impl WorkerState {
    // One retained outcome is observed by every asynchronous or blocking shutdown waiter.
    fn finish_shutdown(&self, result: ShutdownResult) {
        let mut guard = self
            .shutdown_wait
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while self.validations.load(std::sync::atomic::Ordering::SeqCst) != 0 {
            guard = self.shutdown_wake.wait(guard).unwrap_or_else(|error| error.into_inner());
        }
        self.completion.send_replace(Some(result));
        self.shutdown_wake.notify_all();
    }
}

impl WorkQueue {
    fn close_and_join(self) -> ShutdownResult {
        drop(self.sender);
        self.handle
            .join()
            .unwrap_or(Err(CacheError::WorkerPanicked))
    }
}

#[cfg(test)]
mod tests {
    use super::super::image_files::tests::bitmap;
    use super::super::image_sources::{
        ImageOutputFormat, ImageSize, ImageSourceKey, ImageVariantParameters,
    };
    use super::*;
    use base64::Engine;
    use std::time::Duration;

    pub(crate) fn key() -> ImageVariantKey {
        ImageVariantKey {
            source: ImageSourceKey::ProxyId("a".repeat(64)),
            parameters: ImageVariantParameters {
                size: ImageSize::Pixels80,
                output_format: ImageOutputFormat::Png,
            },
        }
    }

    fn storage(path: &std::path::Path) -> Arc<CacheStorage> {
        let storage = CacheStorage::open(path).unwrap();
        storage.write_server_id("server").unwrap();
        Arc::new(storage)
    }

    #[test]
    fn image_queue_prioritizes_visible_reads_and_preserves_write_order() {
        let make = |priority, is_read, age| QueuedImageJob {
            operation: Box::new(|_, _, _| {}), priority, is_read,
            queued_at: Instant::now() - Duration::from_secs(age),
            is_ready: None,
        };
        let mut pending = vec![
            make(RequestPriority::Background, false, 0),
            make(RequestPriority::Icon, false, 0),
            make(RequestPriority::Icon, true, 0),
        ];
        assert!(next_image_job(&mut pending).unwrap().is_read);
        assert_eq!(next_image_job(&mut pending).unwrap().priority, RequestPriority::Background);
        assert_eq!(next_image_job(&mut pending).unwrap().priority, RequestPriority::Icon);
        pending = vec![make(RequestPriority::Background, true, 2), make(RequestPriority::Icon, true, 0)];
        assert_eq!(next_image_job(&mut pending).unwrap().priority, RequestPriority::Background);
    }

    #[tokio::test]
    async fn concurrent_local_reads_share_validation_and_one_cancellation_keeps_the_other() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let workers = ImageWorkers::initialize(storage.clone(), "server".into()).unwrap();
        workers.publish_variant(key(), bitmap(1), 1).await.unwrap();
        let reserved = workers.state.thumbnail_slots.clone().acquire_many_owned(2).await.unwrap();
        let mut first = Box::pin(workers.read_refresh_state(key()));
        let mut second = Box::pin(workers.read_refresh_state(key()));
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        workers.execute(true, |_| Ok(())).await.unwrap();
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        assert_eq!(workers.state.file_reads.lock().unwrap().len(), 1);
        drop(first);
        drop(reserved);
        let remaining = tokio::time::timeout(Duration::from_secs(2), second).await.unwrap().unwrap();
        assert_eq!(remaining.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        drop(remaining);
        let mut first = Box::pin(workers.read_refresh_state(key()));
        let mut second = Box::pin(workers.read_refresh_state(key()));
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        workers.execute(true, |_| Ok(())).await.unwrap();
        let reserved = workers.state.thumbnail_slots.clone().acquire_many_owned(2).await.unwrap();
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        drop(reserved);
        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();
        assert!(first.cached.as_ref().unwrap().shares_file_with(second.cached.as_ref().unwrap()));
        drop(first);
        drop(second);
        workers.shutdown().await.unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test]
    async fn prepared_publications_keep_index_readable_and_drain_in_order_after_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let workers = ImageWorkers::initialize(storage.clone(), "server".into()).unwrap();
        let (started, start) = oneshot::channel();
        let (release, hold) = mpsc::channel();
        let first = workers.clone();
        let publication = tokio::spawn(async move {
            first.publish_prepared_variant(key(), 1, None, move |files| {
                started.send(()).unwrap();
                hold.recv_timeout(Duration::from_secs(5)).unwrap();
                files.publish(bitmap(1))
            }).await
        });
        start.await.unwrap();
        let (prepared, preparation) = oneshot::channel();
        let second = workers.clone();
        let following = tokio::spawn(async move {
            second.publish_prepared_variant(key(), 2, None, move |files| {
                let result = files.publish(bitmap(2));
                prepared.send(()).unwrap();
                result
            }).await
        });
        tokio::time::timeout(Duration::from_secs(2), preparation).await.unwrap().unwrap();
        let current = tokio::time::timeout(Duration::from_secs(2), workers.read_variant(key())).await.unwrap().unwrap();
        assert!(current.is_none());
        assert!(!following.is_finished());
        publication.abort();
        let closing = workers.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), following).await.unwrap().unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), shutdown).await.unwrap().unwrap().unwrap();
        let index = ImageIndex::open(&storage, "server").unwrap();
        let read = index.prepare_variant(&key()).unwrap().unwrap().validate().unwrap().unwrap();
        assert_eq!(read.read.payload.bytes, bitmap(2));
        drop(read);
        index.shutdown().unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test]
    async fn background_admission_reserves_visible_capacity_and_closes_waiters_on_shutdown() {
        use crate::application::repositories::with_request_priority;
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let workers = ImageWorkers::initialize(storage.clone(), "server".into()).unwrap();
        let reserved = workers.state.background_admission.clone().acquire_many_owned(8).await.unwrap();
        let mut background = Box::pin(with_request_priority(RequestPriority::Background, workers.read_variant(key())));
        assert!(futures_util::poll!(&mut background).is_pending());
        assert!(tokio::time::timeout(Duration::from_secs(2), with_request_priority(RequestPriority::Icon, workers.read_variant(key()))).await.unwrap().unwrap().is_none());
        workers.shutdown().await.unwrap();
        assert!(matches!(background.await, Err(CacheError::ShuttingDown)));
        drop(reserved);
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test]
    async fn cached_image_validation_does_not_hold_the_index_owner() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let workers = ImageWorkers::initialize(storage.clone(), "server".into()).unwrap();
        workers.publish_variant(key(), bitmap(1), 1).await.unwrap();
        let reserved = workers.state.thumbnail_slots.clone().acquire_many_owned(2).await.unwrap();
        let reading = workers.clone();
        let read = tokio::spawn(async move { reading.read_refresh_state(key()).await });
        tokio::task::yield_now().await;
        let pending = tokio::time::timeout(Duration::from_secs(2), workers.execute(true, |index| index.prepare_variant(&key()))).await.unwrap().unwrap();
        assert!(pending.is_some());
        tokio::task::yield_now().await;
        assert!(!read.is_finished());
        let mut missing = key();
        missing.source = ImageSourceKey::ProxyId("b".repeat(64));
        let miss = tokio::time::timeout(Duration::from_secs(2), workers.read_refresh_state(missing)).await;
        drop(pending);
        drop(reserved);
        assert!(miss.unwrap().unwrap().cached.is_none(), "a cache miss waited for payload validation");
        let loaded = tokio::time::timeout(Duration::from_secs(2), read).await.unwrap().unwrap().unwrap();
        assert_eq!(loaded.cached.as_ref().unwrap().read.payload.bytes, bitmap(1));
        drop(loaded);
        workers.shutdown().await.unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test]
    async fn shutdown_waits_for_validation_after_its_waiter_is_cancelled() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let workers = ImageWorkers::initialize(storage.clone(), "server".into()).unwrap();
        let (started, start) = oneshot::channel();
        let (release, hold) = mpsc::channel();
        let validating = workers.clone();
        let validation = tokio::spawn(async move { validating.validate(move || {
            started.send(()).unwrap();
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
            ImagePayload::decode_bytes(bitmap(1))
        }).await });
        start.await.unwrap();
        validation.abort();
        let closing = workers.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        assert!(workers.validate_bytes(bitmap(1)).await.is_err());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), shutdown).await.unwrap().unwrap().unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inline_validation_uses_blocking_slots_without_opening_the_index() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let index = storage
            .image_directory()
            .join(super::super::storage::IMAGE_DATABASE_FILE_NAME);
        std::fs::write(&index, b"foreign index").unwrap();
        let workers = ImageWorkers::initialize(storage.clone(), "server".into()).unwrap();
        let bytes = bitmap(3);
        let inline = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        assert_eq!(workers.validate_inline(inline).await.unwrap().bytes, bytes);
        assert!(workers
            .validate_inline("data:image/png;base64,aW52YWxpZA==".into())
            .await
            .is_err());
        assert_eq!(std::fs::read(index).unwrap(), b"foreign index");
        workers.shutdown().await.unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn validation_does_not_require_a_readable_image_index() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let index = storage
            .image_directory()
            .join(super::super::storage::IMAGE_DATABASE_FILE_NAME);
        std::fs::write(&index, b"foreign index").unwrap();
        let workers = ImageWorkers::initialize(storage.clone(), "server".into()).unwrap();
        assert_eq!(
            workers.validate_bytes(bitmap(3)).await.unwrap().bytes,
            bitmap(3)
        );
        assert!(workers
            .validate_bytes(b"not an image".to_vec())
            .await
            .is_err());
        assert_eq!(std::fs::read(&index).unwrap(), b"foreign index");
        assert!(workers.read_variant(key()).await.is_err());
        workers.shutdown().await.unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lazy_worker_runs_off_executor_and_shutdown_waits_for_file_leases() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let workers = ImageWorkers::initialize(storage.clone(), "server".to_owned()).unwrap();
        assert!(!storage
            .image_directory()
            .join(super::super::storage::IMAGE_DATABASE_FILE_NAME)
            .exists());
        let executor_thread = std::thread::current().id();
        assert_ne!(
            workers
                .execute(true, |_| Ok(std::thread::current().id()))
                .await
                .unwrap(),
            executor_thread
        );
        workers.publish_variant(key(), bitmap(1), 1).await.unwrap();
        let leased = workers.read_variant(key()).await.unwrap().unwrap();
        assert_eq!(leased.read.payload.bytes, bitmap(1));
        let shutting = workers.clone();
        let shutdown = tokio::spawn(async move { shutting.shutdown().await });
        tokio::task::yield_now().await;
        assert!(matches!(
            workers.read_variant(key()).await,
            Err(CacheError::ShuttingDown)
        ));
        assert!(!shutdown.is_finished());
        drop(leased);
        tokio::time::timeout(Duration::from_secs(2), shutdown)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        workers.shutdown().await.unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepted_publication_and_shutdown_finish_after_waiters_are_cancelled() {
        let directory = tempfile::tempdir().unwrap();
        let storage = storage(directory.path());
        let workers = ImageWorkers::initialize(storage.clone(), "server".to_owned()).unwrap();
        let (started, start) = oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let writing = workers.clone();
        let write = tokio::spawn(async move {
            writing
                .execute(false, move |index| {
                    started.send(()).unwrap();
                    hold.recv_timeout(Duration::from_secs(5)).unwrap();
                    index.publish_variant(&key(), bitmap(2), 2)
                })
                .await
        });
        start.await.unwrap();
        write.abort();
        assert!(write.await.unwrap_err().is_cancelled());
        let closing = workers.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        tokio::task::yield_now().await;
        shutdown.abort();
        assert!(shutdown.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), workers.shutdown())
            .await
            .unwrap()
            .unwrap();
        let reopened = ImageIndex::open(&storage, "server").unwrap();
        assert_eq!(
            reopened
                .read_variant(&key())
                .unwrap()
                .unwrap()
                .payload
                .bytes,
            bitmap(2)
        );
        reopened.shutdown().unwrap();
        drop(workers);
        drop(storage);
        directory.close().unwrap();
    }
}
