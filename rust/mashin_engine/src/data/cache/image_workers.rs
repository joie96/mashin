//! One independent thread owns lazy image-index work, file validation and publication.
//! Shutdown drains accepted jobs, closes SQLite, then waits for issued file handles.
//! Cancellation of a waiter never cancels an accepted publication or its shutdown join.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use tokio::sync::{mpsc, oneshot, watch};

use super::database::CacheError;
use super::image_files::ImagePayload;
use super::image_index::{ImageIndex, ImageRefreshState, ImageVariantLease};
use super::image_sources::ImageVariantKey;
use super::storage::CacheStorage;

type ShutdownResult = Result<(), CacheError>;
type ImageJob = Box<dyn FnOnce(&mut Option<ImageIndex>, &CacheStorage, &str) + Send>;

#[derive(Clone)]
pub(super) struct ImageWorkers {
    state: Arc<WorkerState>,
}

struct WorkerState {
    queue: Mutex<Option<WorkQueue>>,
    completion: watch::Sender<Option<ShutdownResult>>,
    shutdown_wait: Mutex<()>,
    shutdown_wake: Condvar,
}

struct WorkQueue {
    sender: mpsc::UnboundedSender<ImageJob>,
    handle: JoinHandle<ShutdownResult>,
}

impl ImageWorkers {
    // Blocking thread setup; index creation is deferred until the first live request.
    pub(super) fn initialize(
        storage: Arc<CacheStorage>,
        server_id: String,
    ) -> Result<Self, CacheError> {
        let (sender, mut receiver) = mpsc::unbounded_channel::<ImageJob>();
        let handle = std::thread::Builder::new()
            .name("mashin-cache-images".to_owned())
            .spawn(move || {
                let mut index = None;
                while let Some(job) = receiver.blocking_recv() {
                    job(&mut index, &storage, &server_id);
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
        self.enqueue(true, move |_, _, _| ImagePayload::decode_bytes(bytes))
            .await
    }

    pub(super) async fn validate_inline(&self, source: String) -> Result<ImagePayload, CacheError> {
        self.enqueue(true, move |_, _, _| ImagePayload::decode_inline(&source))
            .await
    }

    pub(super) async fn publish_variant(
        &self,
        key: ImageVariantKey,
        bytes: Vec<u8>,
        observed_at: i64,
    ) -> Result<(), CacheError> {
        self.execute(false, move |index| {
            index.publish_variant(&key, bytes, observed_at)
        })
        .await
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
        self.execute(true, move |index| index.lease_variant(&key))
            .await
    }

    pub(super) async fn read_refresh_state(
        &self,
        key: ImageVariantKey,
    ) -> Result<ImageRefreshState, CacheError> {
        self.execute(true, move |index| {
            index.read_refresh_state(&key, std::time::Instant::now())
        })
        .await
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
        let (response, receiver) = oneshot::channel();
        let job: ImageJob = Box::new(move |index, storage, server_id| {
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
            queue
                .as_ref()
                .ok_or(CacheError::ShuttingDown)?
                .sender
                .send(job)
                .map_err(|_| CacheError::WorkerUnavailable)?;
        }
        receiver.await.map_err(|_| CacheError::WorkerUnavailable)?
    }

    fn take_queue(&self) -> Result<Option<WorkQueue>, CacheError> {
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
        let _guard = self
            .shutdown_wait
            .lock()
            .unwrap_or_else(|error| error.into_inner());
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

    #[tokio::test(flavor = "current_thread")]
    async fn inline_validation_uses_the_shared_worker_without_opening_the_index() {
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
