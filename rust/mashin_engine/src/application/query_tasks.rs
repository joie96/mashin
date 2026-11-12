//! Owns cancellable read tasks and joins their cancellation before releasing a scope.
//! Delivery does not hold the registry lock; close waits for admitted deliveries.
//! Delivery callbacks must not cancel themselves or reenter close/join on the same scope.

use crate::application::errors::ApplicationError;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Default)]
pub(crate) struct QueryTasks {
    state: Arc<Mutex<QueryTasksState>>,
    delivery: Arc<RwLock<()>>,
    workers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    join_guard: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct QueryTasksState {
    requests: HashMap<QueryTaskId, ActiveQuery>,
    is_closed: bool,
    next_synchronous_id: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum QueryTaskId {
    Native(u64),
    Synchronous(u64),
}

struct ActiveQuery {
    cancel: oneshot::Sender<()>,
    delivery: Arc<Mutex<()>>,
}

impl QueryTasks {
    // Close is a delivery barrier; joining happens outside the session/coordinator lock.
    pub(crate) fn close(&self) {
        let _delivery = self
            .delivery
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.is_closed = true;
        for (_, request) in state.requests.drain() {
            let _ = request.cancel.send(());
        }
    }

    pub(crate) async fn join(&self) -> Result<(), ApplicationError> {
        self.close();
        let _join_guard = self.join_guard.lock().await;
        let workers = std::mem::take(
            &mut *self
                .workers
                .lock()
                .map_err(|_| ApplicationError::internal("query worker lock poisoned"))?,
        );
        let mut failure = None;
        for worker in workers {
            if let Err(error) = worker.await {
                failure.get_or_insert_with(|| {
                    ApplicationError::internal(format!("query worker failed: {error}"))
                });
            }
        }
        failure.map_or(Ok(()), Err)
    }

    // Dropping the selected operation also drops its in-flight HTTP/enrichment futures.
    pub(crate) fn cancel(&self, request_id: u64) -> Result<(), ApplicationError> {
        let delivery = self
            .state
            .lock()
            .map_err(|_| ApplicationError::internal("query registry lock poisoned"))?
            .requests
            .get(&QueryTaskId::Native(request_id))
            .map(|request| request.delivery.clone());
        let Some(delivery) = delivery else {
            return Ok(());
        };
        let _delivery = delivery
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cancel = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| ApplicationError::internal("query registry lock poisoned"))?;
            let task_id = QueryTaskId::Native(request_id);
            if state
                .requests
                .get(&task_id)
                .is_some_and(|request| Arc::ptr_eq(&request.delivery, &delivery))
            {
                state.requests.remove(&task_id)
            } else {
                None
            }
        };
        if let Some(cancel) = cancel {
            let _ = cancel.cancel.send(());
        }
        Ok(())
    }

    // Registration precedes spawning, so immediate cancellation cannot miss the task.
    pub(crate) fn start<Response: Send + 'static>(
        &self,
        runtime: &tokio::runtime::Handle,
        request_id: u64,
        timeout: Duration,
        operation: impl Future<Output = Result<Response, ApplicationError>> + Send + 'static,
        publish: impl FnOnce(Result<Response, ApplicationError>) + Send + 'static,
    ) -> Result<(), ApplicationError> {
        self.start_task(
            runtime,
            QueryTaskId::Native(request_id),
            timeout,
            operation,
            publish,
        )
    }

    pub(crate) async fn execute<Response: Send + 'static>(
        &self,
        runtime: &tokio::runtime::Handle,
        timeout: Duration,
        operation: impl Future<Output = Result<Response, ApplicationError>> + Send + 'static,
    ) -> Result<Response, ApplicationError> {
        let task_id = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| ApplicationError::internal("query registry lock poisoned"))?;
            let task_id = QueryTaskId::Synchronous(state.next_synchronous_id);
            state.next_synchronous_id = state.next_synchronous_id.wrapping_add(1);
            task_id
        };
        let (sender, receiver) = oneshot::channel();
        self.start_task(runtime, task_id, timeout, operation, move |result| {
            let _ = sender.send(result);
        })?;
        receiver
            .await
            .map_err(|_| ApplicationError::command_failed("query scope has ended"))?
    }

    fn start_task<Response: Send + 'static>(
        &self,
        runtime: &tokio::runtime::Handle,
        request_id: QueryTaskId,
        timeout: Duration,
        operation: impl Future<Output = Result<Response, ApplicationError>> + Send + 'static,
        publish: impl FnOnce(Result<Response, ApplicationError>) + Send + 'static,
    ) -> Result<(), ApplicationError> {
        let (cancel, canceled) = oneshot::channel();
        let mut state = self
            .state
            .lock()
            .map_err(|_| ApplicationError::internal("query registry lock poisoned"))?;
        if state.is_closed {
            return Err(ApplicationError::command_failed("query scope has ended"));
        }
        if state.requests.contains_key(&request_id) {
            return Err(ApplicationError::internal(
                "query request ID is already active",
            ));
        }
        let request_delivery = Arc::new(Mutex::new(()));
        let mut workers = self
            .workers
            .lock()
            .map_err(|_| ApplicationError::internal("query worker lock poisoned"))?;
        state.requests.insert(
            request_id,
            ActiveQuery {
                cancel,
                delivery: request_delivery.clone(),
            },
        );
        workers.retain(|worker| !worker.is_finished());
        let task_state = self.state.clone();
        let delivery = self.delivery.clone();
        workers.push(runtime.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = canceled => None,
                result = tokio::time::timeout(timeout, operation) => Some(
                    result.unwrap_or_else(|_| Err(ApplicationError::command_failed(format!(
                        "Music Assistant query exceeded its {} ms total deadline", timeout.as_millis(),
                    ))))
                ),
            };
            let _delivery = delivery.read().unwrap_or_else(|poisoned| poisoned.into_inner());
            let _request_delivery = request_delivery.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let should_publish = if let Ok(state) = task_state.lock() {
                if state.requests.get(&request_id).is_some_and(|request| Arc::ptr_eq(&request.delivery, &request_delivery)) {
                    !state.is_closed
                } else {
                    false
                }
            } else {
                false
            };
            if should_publish {
                if let Some(result) = result {
                    publish(result);
                }
            }
            if let Ok(mut state) = task_state.lock() {
                if state.requests.get(&request_id).is_some_and(|request| Arc::ptr_eq(&request.delivery, &request_delivery)) {
                    state.requests.remove(&request_id);
                }
            }
        }));
        Ok(())
    }
}

impl Drop for QueryTasks {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    #[test]
    fn query_registration_and_cancellation_do_not_wait_for_delivery() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let tasks = Arc::new(QueryTasks::default());
        let (publishing, published) = mpsc::channel();
        let (release, released) = mpsc::channel();
        tasks
            .start(
                runtime.handle(),
                1,
                Duration::from_secs(5),
                async { Ok(()) },
                move |_| {
                    publishing.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                },
            )
            .unwrap();
        published.recv_timeout(Duration::from_secs(5)).unwrap();
        let (registered, registration) = mpsc::channel();
        let registering_tasks = tasks.clone();
        let handle = runtime.handle().clone();
        let worker = std::thread::spawn(move || {
            let result = registering_tasks
                .start(
                    &handle,
                    2,
                    Duration::from_secs(5),
                    std::future::pending::<Result<(), ApplicationError>>(),
                    |_| panic!("canceled query was published"),
                )
                .and_then(|_| registering_tasks.cancel(2));
            registered.send(result).unwrap();
        });
        let result = registration.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        worker.join().unwrap();
        runtime.block_on(tasks.join()).unwrap();
        result
            .expect("registration waited for unrelated delivery")
            .unwrap();
    }

    #[test]
    fn query_cancellation_waits_only_for_its_own_delivery() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let tasks = Arc::new(QueryTasks::default());
        let (publishing, published) = mpsc::channel();
        let (release, released) = mpsc::channel();
        tasks
            .start(
                runtime.handle(),
                1,
                Duration::from_secs(5),
                async { Ok(()) },
                move |_| {
                    publishing.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                },
            )
            .unwrap();
        published.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(tasks
            .start(
                runtime.handle(),
                1,
                Duration::from_secs(5),
                async { Ok(()) },
                |_| {}
            )
            .is_err());
        let (canceling, entered_cancel) = mpsc::channel();
        let (canceled, cancellation) = mpsc::channel();
        let canceling_tasks = tasks.clone();
        let worker = std::thread::spawn(move || {
            canceling.send(()).unwrap();
            canceled.send(canceling_tasks.cancel(1)).unwrap();
        });
        entered_cancel.recv_timeout(Duration::from_secs(5)).unwrap();
        let premature_cancel = cancellation.recv_timeout(Duration::from_millis(50));
        let unrelated = tasks.start(
            runtime.handle(),
            2,
            Duration::from_secs(5),
            std::future::pending::<Result<(), ApplicationError>>(),
            |_| {},
        );
        release.send(()).unwrap();
        worker.join().unwrap();
        runtime.block_on(tasks.join()).unwrap();
        assert!(
            premature_cancel.is_err(),
            "cancellation passed its active delivery"
        );
        cancellation
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        unrelated.unwrap();
    }

    #[test]
    fn query_close_waits_for_delivery_and_rejects_late_results() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let tasks = Arc::new(QueryTasks::default());
        let publications = Arc::new(AtomicUsize::new(0));
        let (publishing, published) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let delivered = publications.clone();
        tasks
            .start(
                runtime.handle(),
                1,
                Duration::from_secs(5),
                async { Ok(()) },
                move |_| {
                    publishing.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                    delivered.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap();
        published.recv_timeout(Duration::from_secs(5)).unwrap();
        let (complete, completion) = oneshot::channel();
        let delivered = publications.clone();
        tasks
            .start(
                runtime.handle(),
                2,
                Duration::from_secs(5),
                async move {
                    completion
                        .await
                        .map_err(|_| ApplicationError::command_failed("test completion dropped"))
                },
                move |_| {
                    delivered.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap();
        let (closing, entered_close) = mpsc::channel();
        let (closed, close_finished) = mpsc::channel();
        let closing_tasks = tasks.clone();
        let worker = std::thread::spawn(move || {
            closing.send(()).unwrap();
            closing_tasks.close();
            closed.send(()).unwrap();
        });
        entered_close.recv_timeout(Duration::from_secs(5)).unwrap();
        let premature_close = close_finished.recv_timeout(Duration::from_millis(50));
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(premature_close.is_err(), "close passed an active delivery");
        let _ = complete.send(());
        assert!(tasks
            .start(
                runtime.handle(),
                3,
                Duration::from_secs(5),
                async { Ok(()) },
                |_| panic!("closed query was published")
            )
            .is_err());
        runtime.block_on(tasks.join()).unwrap();
        assert_eq!(publications.load(Ordering::SeqCst), 1);
    }
}
