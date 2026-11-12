//! A dedicated writer and two independent reader threads own all SQLite execution.
//! Accepted writes finish even if the awaiting caller disappears. Reads use WAL snapshots.
//! Explicit shutdown rejects new jobs, drains accepted work and waits for every handle close.
//! Dropping the last owner closes queues but does not synchronously join on the async executor.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use rusqlite::{Connection, Transaction};
use tokio::sync::{oneshot, watch, Semaphore};

use super::database::{
    read_metadata, CacheError, CacheFileMetadata, CacheWalMaintenance, MediaDatabase, WorkerConnections,
};

type DatabaseJob = Box<dyn FnOnce(&mut Connection) + Send>;
type JobReceiver = Arc<Mutex<DatabaseJobQueue>>;
type ShutdownResult = Result<(), CacheError>;
type ActiveReads = Arc<Mutex<BTreeMap<u64, ActiveSqlRead>>>;

struct QueuedDatabaseJob {
    operation: DatabaseJob,
    priority: crate::application::repositories::RequestPriority,
    queued_at: Instant,
    sql_id: u64,
}

struct DatabaseJobQueue {
    receiver: mpsc::Receiver<QueuedDatabaseJob>,
    pending: Vec<QueuedDatabaseJob>,
}

impl DatabaseJobQueue {
    // Writes remain FIFO; aged reads eventually precede newer foreground work.
    fn next(&mut self, is_writer: bool, delay: Option<std::time::Duration>) -> Result<DatabaseJob, mpsc::RecvTimeoutError> {
        if !is_writer {
            self.pending.extend(self.receiver.try_iter());
            let now = Instant::now();
            let selected = self.pending.iter().enumerate().max_by_key(|(_, job)| {
                let priority = if now.duration_since(job.queued_at).as_secs() >= 1 {
                    crate::application::repositories::RequestPriority::Foreground
                } else { job.priority };
                (priority, std::cmp::Reverse(job.sql_id))
            }).map(|(index, _)| index);
            if let Some(index) = selected {
                return Ok(self.pending.swap_remove(index).operation);
            }
        }
        match delay {
            Some(delay) => self.receiver.recv_timeout(delay),
            None => self.receiver.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        }.map(|job| job.operation)
    }
}

struct ActiveSqlRead {
    started: Instant,
    worker: String,
    origin: &'static str,
}

struct ActiveReadGuard {
    reads: ActiveReads,
    sql_id: u64,
}

impl ActiveReadGuard {
    // Diagnostics retain job identity, never a connection or a transaction.
    fn new(reads: ActiveReads, sql_id: u64, origin: &'static str, started: Instant) -> Self {
        reads
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                sql_id,
                ActiveSqlRead {
                    started,
                    worker: std::thread::current()
                        .name()
                        .unwrap_or("unknown")
                        .to_owned(),
                    origin,
                },
            );
        Self { reads, sql_id }
    }
}

impl Drop for ActiveReadGuard {
    fn drop(&mut self) {
        self.reads
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.sql_id);
    }
}

#[derive(Clone)]
pub(crate) struct DatabaseWorkers {
    state: Arc<WorkerState>,
}

struct WorkerState {
    active_reads: ActiveReads,
    read_admission: Arc<Semaphore>,
    background_read_admission: Arc<Semaphore>,
    queues: Mutex<Option<WorkQueues>>,
    completion: watch::Sender<Option<ShutdownResult>>,
    shutdown_wait: Mutex<()>,
    shutdown_wake: Condvar,
}

struct WorkQueues {
    writer_sender: mpsc::Sender<QueuedDatabaseJob>,
    reader_sender: mpsc::Sender<QueuedDatabaseJob>,
    handles: Vec<JoinHandle<ShutdownResult>>,
}

#[derive(Clone, Copy, Debug)]
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
            let active_reads = Arc::new(Mutex::new(BTreeMap::new()));
            let queues = start_workers(database.into_worker_connections()?, active_reads.clone())?;
            let (completion, _) = watch::channel(None);
            Ok(Self {
                state: Arc::new(WorkerState {
                    active_reads,
                    read_admission: Arc::new(Semaphore::new(32)),
                    background_read_admission: Arc::new(Semaphore::new(8)),
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
        self.state.read_admission.close();
        self.state.background_read_admission.close();
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
        let started = Instant::now();
        let priority = crate::application::repositories::current_priority();
        let background = if priority < crate::application::repositories::RequestPriority::Icon {
            Some(self.state.background_read_admission.clone().acquire_owned().await.map_err(|_| CacheError::ShuttingDown)?)
        } else { None };
        let admission = self.state.read_admission.clone().acquire_owned().await.map_err(|_| CacheError::ShuttingDown)?;
        crate::logging::Logger::new("Cache.SQL").trace(format_args!(
            "stage=admitted priority={priority:?} admission_ms={} admitted_reads={}",
            started.elapsed().as_millis(), 32 - self.state.read_admission.available_permits()
        ));
        self.submit(OperationKind::Read, move |snapshot| {
            let _admission = admission;
            let _background = background;
            operation(snapshot)
        })?
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
        static NEXT_SQL_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sql_id = NEXT_SQL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let origin = std::any::type_name::<Operation>();
        let active_reads = self.state.active_reads.clone();
        let queued_at = Instant::now();
        let job: DatabaseJob = Box::new(move |connection| {
            let logger = crate::logging::Logger::new("Cache.SQL");
            if matches!(kind, OperationKind::Read) && response.is_closed() {
                logger.trace(format_args!("sql_id={sql_id} operation={kind:?} stage=skipped reason=consumer_canceled origin={origin}"));
                return;
            }
            let started = Instant::now();
            let thread = std::thread::current();
            let worker = thread.name().unwrap_or("unknown");
            let active_read = matches!(kind, OperationKind::Read)
                .then(|| ActiveReadGuard::new(active_reads, sql_id, origin, started));
            logger.trace(format_args!("sql_id={sql_id} operation={kind:?} stage=started worker={worker} queue_ms={} autocommit={} origin={origin}", started.duration_since(queued_at).as_millis(), connection.is_autocommit()));
            let result = catch_unwind(AssertUnwindSafe(|| {
                let transaction = connection.transaction()?;
                logger.trace(format_args!(
                    "sql_id={sql_id} operation={kind:?} stage=transaction_started worker={worker}"
                ));
                let value = operation(&transaction)?;
                transaction.commit()?;
                Ok(value)
            }))
            .unwrap_or(Err(CacheError::WorkerPanicked));
            let elapsed = started.elapsed();
            drop(active_read);
            let transaction_state = connection.transaction_state(Some(rusqlite::MAIN_DB));
            logger.trace(format_args!(
                "sql_id={sql_id} operation={kind:?} stage=finished worker={worker} queue_ms={} execution_ms={} success={} autocommit={} txn_state={transaction_state:?} origin={origin}",
                started.duration_since(queued_at).as_millis(),
                elapsed.as_millis(),
                result.is_ok(),
                connection.is_autocommit()
            ));
            if matches!(kind, OperationKind::Read) && elapsed.as_secs() >= 1 {
                logger.warn(format_args!(
                    "sql_id={sql_id} slow_read worker={worker} execution_ms={} queue_ms={} success={} origin={origin}",
                    elapsed.as_millis(),
                    started.duration_since(queued_at).as_millis(),
                    result.is_ok()
                ));
            }
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
            .send(QueuedDatabaseJob {
                operation: job,
                priority: crate::application::repositories::current_priority(),
                queued_at,
                sql_id,
            })
            .map_err(|_| CacheError::WorkerUnavailable)?;
        Ok(receiver)
    }

    fn begin_shutdown(&self) -> Result<watch::Receiver<Option<ShutdownResult>>, CacheError> {
        self.state.read_admission.close();
        self.state.background_read_admission.close();
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
fn start_workers(
    connections: WorkerConnections,
    active_reads: ActiveReads,
) -> Result<WorkQueues, CacheError> {
    let (writer_sender, writer_receiver) = mpsc::channel();
    let (reader_sender, reader_receiver) = mpsc::channel();
    let writer_receiver = Arc::new(Mutex::new(DatabaseJobQueue { receiver: writer_receiver, pending: Vec::new() }));
    let reader_receiver = Arc::new(Mutex::new(DatabaseJobQueue { receiver: reader_receiver, pending: Vec::new() }));
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
        let active_reads = active_reads.clone();
        match std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                run_worker(
                    connection,
                    receiver,
                    name == "mashin-cache-writer",
                    active_reads,
                )
            }) {
            Ok(handle) => queues.handles.push(handle),
            Err(error) => {
                queues.close_and_join()?;
                return Err(error.into());
            }
        }
    }
    Ok(queues)
}

fn run_worker(
    mut connection: Connection,
    receiver: JobReceiver,
    is_writer: bool,
    active_reads: ActiveReads,
) -> ShutdownResult {
    crate::logging::Logger::new("Cache.SQL").trace(format_args!(
        "stage=connection_opened worker={} autocommit={} txn_state={:?}",
        std::thread::current().name().unwrap_or("unknown"),
        connection.is_autocommit(),
        connection.transaction_state(Some(rusqlite::MAIN_DB)),
    ));
    let mut maintenance = is_writer.then(CacheWalMaintenance::new);
    let result = loop {
        let job = match receiver.lock() {
            Ok(mut receiver) => receiver.next(is_writer, maintenance.as_ref().and_then(CacheWalMaintenance::delay)),
            Err(_) => break Err(CacheError::WorkerUnavailable),
        };
        match job {
            Ok(job) => {
                job(&mut connection);
                if let Some(maintenance) = &mut maintenance {
                    maintenance.record_activity();
                    if maintenance.delay().is_some_and(|delay| delay.is_zero()) {
                        trace_active_reads(&active_reads);
                        maintenance.maintain(&connection, "metadata");
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(maintenance) = &mut maintenance {
                    trace_active_reads(&active_reads);
                    maintenance.maintain(&connection, "metadata");
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
        }
    };
    let close_result = connection.close().map_err(|(_, error)| error.into());
    result.and(close_result)
}

// Snapshot diagnostics before logging so event delivery never holds the reader registry lock.
fn trace_active_reads(active_reads: &ActiveReads) {
    let reads: Vec<_> = {
        let reads = active_reads
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        reads
            .iter()
            .map(|(sql_id, read)| {
                (
                    *sql_id,
                    read.worker.clone(),
                    read.origin,
                    read.started.elapsed().as_millis(),
                )
            })
            .collect()
    };
    let logger = crate::logging::Logger::new("Cache.WAL");
    logger.trace(format_args!(
        "area=metadata stage=reader_snapshot active_reads={}",
        reads.len()
    ));
    for (sql_id, worker, origin, elapsed_ms) in reads {
        logger.trace(format_args!("area=metadata stage=active_reader sql_id={sql_id} worker={worker} elapsed_ms={elapsed_ms} origin={origin}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn queued_reads_prioritize_foreground_preserve_fifo_and_age_background_work() {
        use crate::application::repositories::RequestPriority;
        let (sender, receiver) = mpsc::channel();
        let output = Arc::new(Mutex::new(Vec::new()));
        for (sql_id, priority, age) in [
            (1, RequestPriority::Background, 2),
            (2, RequestPriority::Background, 0),
            (3, RequestPriority::Foreground, 0),
            (4, RequestPriority::Foreground, 0),
        ] {
            let output = output.clone();
            sender.send(QueuedDatabaseJob {
                operation: Box::new(move |_| output.lock().unwrap().push(sql_id)),
                priority,
                queued_at: Instant::now() - Duration::from_secs(age),
                sql_id,
            }).unwrap();
        }
        drop(sender);
        let mut queue = DatabaseJobQueue { receiver, pending: Vec::new() };
        let mut connection = Connection::open_in_memory().unwrap();
        for _ in 0..4 { queue.next(false, None).unwrap()(&mut connection); }
        assert_eq!(*output.lock().unwrap(), vec![1, 3, 4, 2]);
        assert!(matches!(queue.next(false, None), Err(mpsc::RecvTimeoutError::Disconnected)));
    }

    async fn workers(directory: &PathBuf) -> DatabaseWorkers {
        DatabaseWorkers::initialize(directory.clone(), "synthetic-server".to_owned())
            .await
            .unwrap()
    }

    #[test]
    fn implicit_sql_reader_can_block_wal_while_autocommit_is_enabled() {
        let directory = tempfile::tempdir().unwrap();
        let WorkerConnections {
            writer,
            readers: [reader, second_reader],
        } = MediaDatabase::open(directory.path(), "synthetic-server")
            .unwrap()
            .into_worker_connections()
            .unwrap();
        writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        writer.execute_batch("CREATE TABLE reader_probe (value INTEGER); INSERT INTO reader_probe VALUES (1),(2); CREATE TABLE writer_probe (payload BLOB);").unwrap();
        let mut maintenance = CacheWalMaintenance::new();
        maintenance.maintain(&writer, "test");
        let mut statement = reader.prepare("SELECT value FROM reader_probe").unwrap();
        let mut rows = statement.query([]).unwrap();
        assert!(rows.next().unwrap().is_some());
        assert!(reader.is_autocommit());
        assert_eq!(
            reader.transaction_state(Some(rusqlite::MAIN_DB)).unwrap(),
            rusqlite::TransactionState::Read
        );
        writer
            .execute(
                "INSERT INTO writer_probe VALUES (zeroblob(8 * 1024 * 1024))",
                [],
            )
            .unwrap();
        maintenance.maintain(&writer, "test");
        assert!(maintenance.delay().is_some());
        let wal_path = directory.path().join("metadata.sqlite3-wal");
        assert!(std::fs::metadata(&wal_path).unwrap().len() > 4 * 1024 * 1024);
        drop(rows);
        drop(statement);
        assert_eq!(
            reader.transaction_state(Some(rusqlite::MAIN_DB)).unwrap(),
            rusqlite::TransactionState::None
        );
        maintenance.maintain(&writer, "test");
        assert!(maintenance.delay().is_none());
        assert_eq!(std::fs::metadata(&wal_path).unwrap().len(), 0);
        drop((reader, second_reader, writer));
        directory.close().unwrap();
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

    #[tokio::test(flavor = "current_thread")]
    async fn background_admission_reserves_foreground_capacity_and_closes_on_shutdown() {
        use crate::application::repositories::{with_request_priority, RequestPriority};
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        let held = workers.state.background_read_admission.clone().acquire_many_owned(8).await.unwrap();
        let mut background = Box::pin(with_request_priority(RequestPriority::Background, workers.read_snapshot(|_| panic!("unadmitted read must not execute"))));
        assert!(futures_util::poll!(background.as_mut()).is_pending());
        assert_eq!(workers.read_snapshot(|_| Ok(42)).await.unwrap(), 42);
        workers.shutdown().await.unwrap();
        assert!(matches!(background.await, Err::<(), _>(CacheError::ShuttingDown)));
        drop(held);
        directory.close().unwrap();
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
    async fn active_reader_diagnostics_are_visible_before_completion_and_retired_on_error() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        let (started, ready) = oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let result = workers
            .submit(OperationKind::Read, move |transaction| {
                transaction.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))?;
                let _ = started.send(());
                hold.recv_timeout(Duration::from_secs(5)).unwrap();
                Err::<(), _>(CacheError::WorkerUnavailable)
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), ready)
            .await
            .unwrap()
            .unwrap();
        {
            let reads = workers.state.active_reads.lock().unwrap();
            assert_eq!(reads.len(), 1);
            let read = reads.values().next().unwrap();
            assert!(read.worker.starts_with("mashin-cache-reader-"));
            assert!(read
                .origin
                .contains("active_reader_diagnostics_are_visible_before_completion"));
        }
        trace_active_reads(&workers.state.active_reads);
        release.send(()).unwrap();
        assert!(matches!(
            result.await.unwrap(),
            Err(CacheError::WorkerUnavailable)
        ));
        assert!(workers.state.active_reads.lock().unwrap().is_empty());
        workers.shutdown().await.unwrap();
        directory.close().unwrap();
    }

    #[test]
    fn active_reader_diagnostics_are_retired_on_panic() {
        let reads = Arc::new(Mutex::new(BTreeMap::new()));
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _guard = ActiveReadGuard::new(reads.clone(), 1, "test", Instant::now());
            assert_eq!(reads.lock().unwrap().len(), 1);
            panic!("synthetic reader panic");
        }));
        assert!(result.is_err());
        assert!(reads.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn idle_writer_truncates_a_large_wal_without_another_job() {
        let directory = tempfile::tempdir().unwrap();
        let workers = workers(&directory.path().to_owned()).await;
        workers
            .write_transaction(|transaction| {
                transaction.execute_batch(
                    "CREATE TABLE idle_probe(value BLOB NOT NULL);
                 INSERT INTO idle_probe VALUES(zeroblob(8388608));",
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let wal_path = directory.path().join(format!(
            "{}-wal",
            super::super::database::DATABASE_FILE_NAME
        ));
        assert!(std::fs::metadata(&wal_path).unwrap().len() > 4 * 1024 * 1024);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if std::fs::metadata(&wal_path).unwrap().len() == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        workers.shutdown().await.unwrap();
        directory.close().unwrap();
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
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
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
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
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
