//! A dedicated writer and two independent reader threads own all SQLite execution.
//! Accepted writes finish even if the awaiting caller disappears. Reads use WAL snapshots.
//! Explicit shutdown rejects new jobs, drains accepted work and waits for every handle close.
//! Dropping the last owner closes queues but does not synchronously join on the async executor.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use rusqlite::{Connection, Transaction};
use tokio::sync::{mpsc, oneshot, watch};

use super::database::{
    read_metadata, CacheError, CacheFileMetadata, MediaDatabase, WorkerConnections,
};

type DatabaseJob = Box<dyn FnOnce(&mut Connection) + Send>;
type JobReceiver = Arc<Mutex<mpsc::UnboundedReceiver<DatabaseJob>>>;
type ShutdownResult = Result<(), CacheError>;

#[derive(Clone)]
pub(crate) struct DatabaseWorkers {
    state: Arc<WorkerState>,
}

struct WorkerState {
    queues: Mutex<Option<WorkQueues>>,
    completion: watch::Sender<Option<ShutdownResult>>,
    shutdown_wait: Mutex<()>,
    shutdown_wake: Condvar,
}

struct WorkQueues {
    writer_sender: mpsc::UnboundedSender<DatabaseJob>,
    reader_sender: mpsc::UnboundedSender<DatabaseJob>,
    handles: Vec<JoinHandle<ShutdownResult>>,
}

#[derive(Clone, Copy)]
enum OperationKind {
    Read,
    Write,
}

impl DatabaseWorkers {
    // Initialization and lifecycle.
    /// Opens and validates connections off the async executor before starting their owners.
    pub(crate) async fn initialize(
        cache_directory: PathBuf,
        server_id: String,
    ) -> Result<Self, CacheError> {
        tokio::task::spawn_blocking(move || {
            let database = MediaDatabase::open(&cache_directory, &server_id)?;
            let queues = start_workers(database.into_worker_connections()?)?;
            let (completion, _) = watch::channel(None);
            Ok(Self {
                state: Arc::new(WorkerState {
                    queues: Mutex::new(Some(queues)),
                    completion,
                    shutdown_wait: Mutex::new(()),
                    shutdown_wake: Condvar::new(),
                }),
            })
        })
        .await
        .map_err(|_| CacheError::WorkerPanicked)?
    }

    /// Cancellation of this waiter does not cancel draining or release SQLite prematurely.
    pub(crate) async fn shutdown(&self) -> ShutdownResult {
        let mut completion = self.begin_shutdown()?;
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

    /// Used only by blocking cleanup owners; shares completion with asynchronous shutdown.
    pub(super) fn shutdown_blocking(&self) -> ShutdownResult {
        let queues = self
            .state
            .queues
            .lock()
            .map_err(|_| CacheError::WorkerUnavailable)?
            .take();
        if let Some(queues) = queues {
            let result = catch_unwind(AssertUnwindSafe(|| queues.close_and_join()))
                .unwrap_or(Err(CacheError::WorkerPanicked));
            self.state.finish_shutdown(result);
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

    // Atomic writes and independent snapshot reads.
    /// Internal persistence primitive; success is delivered only after the durable commit.
    pub(super) async fn write_transaction<ResultValue, Operation>(
        &self,
        operation: Operation,
    ) -> Result<ResultValue, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&Transaction<'_>) -> Result<ResultValue, CacheError> + Send + 'static,
    {
        self.submit(OperationKind::Write, operation)?
            .await
            .map_err(|_| CacheError::WorkerUnavailable)?
    }

    /// Mapping remains on a reader thread and no SQL connection escapes to the caller.
    pub(super) async fn read_snapshot<ResultValue, Operation>(
        &self,
        operation: Operation,
    ) -> Result<ResultValue, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&Transaction<'_>) -> Result<ResultValue, CacheError> + Send + 'static,
    {
        self.submit(OperationKind::Read, operation)?
            .await
            .map_err(|_| CacheError::WorkerUnavailable)?
    }

    pub(crate) async fn metadata(&self) -> Result<CacheFileMetadata, CacheError> {
        self.read_snapshot(|snapshot| read_metadata(snapshot)).await
    }

    // Queue acceptance and coordinated shutdown.
    fn submit<ResultValue, Operation>(
        &self,
        kind: OperationKind,
        operation: Operation,
    ) -> Result<oneshot::Receiver<Result<ResultValue, CacheError>>, CacheError>
    where
        ResultValue: Send + 'static,
        Operation: FnOnce(&Transaction<'_>) -> Result<ResultValue, CacheError> + Send + 'static,
    {
        let (response, receiver) = oneshot::channel();
        let job: DatabaseJob = Box::new(move |connection| {
            if matches!(kind, OperationKind::Read) && response.is_closed() {
                return;
            }
            let result = catch_unwind(AssertUnwindSafe(|| {
                let transaction = connection.transaction()?;
                let value = operation(&transaction)?;
                transaction.commit()?;
                Ok(value)
            }))
            .unwrap_or(Err(CacheError::WorkerPanicked));
            let _ = response.send(result);
        });
        let queues = self
            .state
            .queues
            .lock()
            .map_err(|_| CacheError::WorkerUnavailable)?;
        let queues = queues.as_ref().ok_or(CacheError::ShuttingDown)?;
        let sender = match kind {
            OperationKind::Read => &queues.reader_sender,
            OperationKind::Write => &queues.writer_sender,
        };
        sender
            .send(job)
            .map_err(|_| CacheError::WorkerUnavailable)?;
        Ok(receiver)
    }

    fn begin_shutdown(&self) -> Result<watch::Receiver<Option<ShutdownResult>>, CacheError> {
        let mut queues = self
            .state
            .queues
            .lock()
            .map_err(|_| CacheError::WorkerUnavailable)?;
        let completion = self.state.completion.subscribe();
        if let Some(queues) = queues.take() {
            let state = self.state.clone();
            tokio::task::spawn_blocking(move || {
                let result = catch_unwind(AssertUnwindSafe(|| queues.close_and_join()))
                    .unwrap_or(Err(CacheError::WorkerPanicked));
                state.finish_shutdown(result);
            });
        }
        Ok(completion)
    }
}

impl WorkerState {
    // Shared completion for async waiters and blocking cleanup.
    fn finish_shutdown(&self, result: ShutdownResult) {
        let _guard = self
            .shutdown_wait
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.completion.send_replace(Some(result));
        self.shutdown_wake.notify_all();
    }
}

impl WorkQueues {
    // Worker completion and handle ownership.
    fn close_and_join(self) -> ShutdownResult {
        let Self {
            writer_sender,
            reader_sender,
            handles,
        } = self;
        drop(writer_sender);
        drop(reader_sender);
        let mut result = Ok(());
        for handle in handles {
            let worker_result = handle.join().unwrap_or(Err(CacheError::WorkerPanicked));
            if result.is_ok() {
                result = worker_result;
            }
        }
        result
    }
}

// Blocking initialization and execution.
fn start_workers(connections: WorkerConnections) -> Result<WorkQueues, CacheError> {
    let (writer_sender, writer_receiver) = mpsc::unbounded_channel();
    let (reader_sender, reader_receiver) = mpsc::unbounded_channel();
    let writer_receiver = Arc::new(Mutex::new(writer_receiver));
    let reader_receiver = Arc::new(Mutex::new(reader_receiver));
    let WorkerConnections {
        writer,
        readers: [first_reader, second_reader],
    } = connections;
    let mut queues = WorkQueues {
        writer_sender,
        reader_sender,
        handles: Vec::with_capacity(3),
    };
    for (name, connection, receiver) in [
        ("mashin-cache-writer", writer, writer_receiver),
        (
            "mashin-cache-reader-1",
            first_reader,
            reader_receiver.clone(),
        ),
        ("mashin-cache-reader-2", second_reader, reader_receiver),
    ] {
        match std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || run_worker(connection, receiver))
        {
            Ok(handle) => queues.handles.push(handle),
            Err(error) => {
                queues.close_and_join()?;
                return Err(error.into());
            }
        }
    }
    Ok(queues)
}

fn run_worker(mut connection: Connection, receiver: JobReceiver) -> ShutdownResult {
    let result = loop {
        let job = match receiver.lock() {
            Ok(mut receiver) => receiver.blocking_recv(),
            Err(_) => break Err(CacheError::WorkerUnavailable),
        };
        match job {
            Some(job) => job(&mut connection),
            None => break Ok(()),
        }
    };
    let close_result = connection.close().map_err(|(_, error)| error.into());
    result.and(close_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn workers(directory: &PathBuf) -> DatabaseWorkers {
        DatabaseWorkers::initialize(directory.clone(), "synthetic-server".to_owned())
            .await
            .unwrap()
    }

    async fn create_probe(workers: &DatabaseWorkers) {
        workers
            .write_transaction(|transaction| {
                transaction.execute("CREATE TABLE probe (value INTEGER NOT NULL)", [])?;
                transaction.execute("INSERT INTO probe VALUES (1)", [])?;
                Ok(())
            })
            .await
            .unwrap();
    }

    async fn value(workers: &DatabaseWorkers) -> i32 {
        workers
            .read_snapshot(|snapshot| {
                Ok(snapshot.query_row("SELECT value FROM probe", [], |row| row.get(0))?)
            })
            .await
            .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sql_and_mapping_run_off_the_async_executor() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        let executor_thread = std::thread::current().id();
        let writer_thread = workers
            .write_transaction(|_| Ok(std::thread::current().id()))
            .await
            .unwrap();
        let reader_thread = workers
            .read_snapshot(|_| Ok(std::thread::current().id()))
            .await
            .unwrap();
        assert_ne!(executor_thread, writer_thread);
        assert_ne!(executor_thread, reader_thread);
        assert_ne!(writer_thread, reader_thread);
        assert_eq!(
            workers.metadata().await.unwrap().server_id,
            "synthetic-server"
        );
        workers.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_finishes_while_writer_transaction_is_blocked() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        create_probe(&workers).await;
        let (started, start) = oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let write = workers
            .submit(OperationKind::Write, move |transaction| {
                transaction.execute("UPDATE probe SET value = 2", [])?;
                started.send(()).unwrap();
                hold.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
            .unwrap();
        start.await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), value(&workers))
                .await
                .unwrap(),
            1
        );
        release.send(()).unwrap();
        write.await.unwrap().unwrap();
        assert_eq!(value(&workers).await, 2);
        workers.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_write_rolls_back_and_next_commit_survives_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        create_probe(&workers).await;
        let failed = workers
            .write_transaction(|transaction| {
                transaction.execute("UPDATE probe SET value = 2", [])?;
                transaction.execute("INSERT INTO nonexistent_table VALUES (3)", [])?;
                Ok(())
            })
            .await;
        assert!(matches!(failed, Err(CacheError::Sqlite(_))));
        assert_eq!(value(&workers).await, 1);
        workers
            .write_transaction(|transaction| {
                transaction.execute("UPDATE probe SET value = 3", [])?;
                Ok(())
            })
            .await
            .unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "synthetic-server".to_owned())
                .await
                .unwrap();
        assert_eq!(value(&reopened).await, 3);
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn readers_cannot_write_and_panics_do_not_leave_transactions_open() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        create_probe(&workers).await;
        let read_write = workers
            .read_snapshot(|snapshot| {
                snapshot.execute("UPDATE probe SET value = 9", [])?;
                Ok(())
            })
            .await;
        assert!(matches!(read_write, Err(CacheError::Sqlite(_))));
        let panic_result = workers
            .write_transaction::<(), _>(|transaction| {
                transaction.execute("UPDATE probe SET value = 9", [])?;
                panic!("synthetic mapping failure");
            })
            .await;
        assert!(matches!(panic_result, Err(CacheError::WorkerPanicked)));
        assert_eq!(value(&workers).await, 1);
        workers
            .write_transaction(|transaction| {
                transaction.execute("UPDATE probe SET value = 4", [])?;
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(value(&workers).await, 4);
        workers.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_drains_accepted_writes_despite_cancelled_waiters() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        create_probe(&workers).await;
        let (started, start) = oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let write = workers
            .submit(OperationKind::Write, move |transaction| {
                transaction.execute("UPDATE probe SET value = 5", [])?;
                started.send(()).unwrap();
                hold.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
            .unwrap();
        let second_write = workers
            .submit(OperationKind::Write, |transaction| {
                transaction.execute("UPDATE probe SET value = value + 1", [])?;
                Ok(())
            })
            .unwrap();
        start.await.unwrap();
        drop(write);
        drop(second_write);
        let shutdown_owner = workers.clone();
        let (shutdown_started, shutdown_start) = oneshot::channel();
        let shutdown_waiter = tokio::spawn(async move {
            shutdown_started.send(()).unwrap();
            shutdown_owner.shutdown().await
        });
        shutdown_start.await.unwrap();
        shutdown_waiter.abort();
        assert!(shutdown_waiter.await.unwrap_err().is_cancelled());
        assert!(matches!(
            workers.metadata().await,
            Err(CacheError::ShuttingDown)
        ));
        assert!(matches!(
            workers.write_transaction(|_| Ok(())).await,
            Err(CacheError::ShuttingDown)
        ));
        release.send(()).unwrap();
        let (first_shutdown, second_shutdown) =
            tokio::join!(workers.shutdown(), workers.shutdown());
        first_shutdown.unwrap();
        second_shutdown.unwrap();
        workers.shutdown().await.unwrap();
        let reopened =
            DatabaseWorkers::initialize(directory.path().to_owned(), "synthetic-server".to_owned())
                .await
                .unwrap();
        assert_eq!(value(&reopened).await, 6);
        reopened.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn two_readers_run_in_parallel_without_a_third_connection() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        let started_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (started, mut starts) = mpsc::unbounded_channel();
        let mut releases = Vec::new();
        let mut results = Vec::new();
        for _ in 0..3 {
            let count = started_count.clone();
            let started = started.clone();
            let (release, hold) = std::sync::mpsc::channel();
            releases.push(release);
            results.push(
                workers
                    .submit(OperationKind::Read, move |_| {
                        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        started.send(std::thread::current().id()).unwrap();
                        hold.recv_timeout(Duration::from_secs(5)).unwrap();
                        Ok(())
                    })
                    .unwrap(),
            );
        }
        let first_thread = tokio::time::timeout(Duration::from_secs(2), starts.recv())
            .await
            .unwrap()
            .unwrap();
        let second_thread = tokio::time::timeout(Duration::from_secs(2), starts.recv())
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first_thread, second_thread);
        assert_eq!(started_count.load(std::sync::atomic::Ordering::SeqCst), 2);
        let completion = workers.begin_shutdown().unwrap();
        assert!(completion.borrow().is_none());
        for release in releases {
            release.send(()).unwrap();
        }
        for result in results {
            result.await.unwrap().unwrap();
        }
        workers.shutdown().await.unwrap();
        assert_eq!(started_count.load(std::sync::atomic::Ordering::SeqCst), 3);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_initialization_is_reported_without_activating_workers() {
        let directory = tempfile::tempdir().unwrap();
        let invalid = DatabaseWorkers::initialize(directory.path().to_owned(), String::new()).await;
        assert!(matches!(invalid, Err(CacheError::InvalidServerId)));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_and_async_shutdown_share_one_completion() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        let cleanup = workers.clone();
        let blocking = tokio::task::spawn_blocking(move || cleanup.shutdown_blocking());
        let (asynchronous, synchronous) = tokio::join!(workers.shutdown(), blocking);
        asynchronous.unwrap();
        synchronous.unwrap().unwrap();
        assert!(matches!(
            workers.metadata().await,
            Err(CacheError::ShuttingDown)
        ));
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_queued_read_does_not_cancel_other_consumers() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        let (started, mut starts) = mpsc::unbounded_channel();
        let mut releases = Vec::new();
        let mut active_reads = Vec::new();
        for _ in 0..2 {
            let started = started.clone();
            let (release, hold) = std::sync::mpsc::channel();
            releases.push(release);
            active_reads.push(
                workers
                    .submit(OperationKind::Read, move |_| {
                        started.send(()).unwrap();
                        hold.recv_timeout(Duration::from_secs(5)).unwrap();
                        Ok(())
                    })
                    .unwrap(),
            );
        }
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(2), starts.recv())
                .await
                .unwrap()
                .unwrap();
        }
        let was_mapped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = was_mapped.clone();
        let cancelled = workers
            .submit(OperationKind::Read, move |_| {
                probe.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        drop(cancelled);
        let survivor = workers
            .submit(OperationKind::Read, |snapshot| read_metadata(snapshot))
            .unwrap();
        for release in releases {
            release.send(()).unwrap();
        }
        for active in active_reads {
            active.await.unwrap().unwrap();
        }
        assert_eq!(
            survivor.await.unwrap().unwrap().server_id,
            "synthetic-server"
        );
        assert!(!was_mapped.load(std::sync::atomic::Ordering::SeqCst));
        workers.shutdown().await.unwrap();
        directory.close().unwrap();
    }
}
