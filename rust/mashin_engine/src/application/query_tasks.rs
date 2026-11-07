//! Owns cancellable read tasks and joins their cancellation before releasing a scope.
//! Result delivery is a synchronous barrier and must not reenter the same registry.

use crate::application::errors::ApplicationError;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Default)]
pub(crate) struct QueryTasks {
    state: Arc<Mutex<QueryTasksState>>,
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
    identity: Arc<()>,
}

impl QueryTasks {
    // Close is a delivery barrier; joining happens outside the session/coordinator lock.
    pub(crate) fn close(&self) {
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
        let cancel = self
            .state
            .lock()
            .map_err(|_| ApplicationError::internal("query registry lock poisoned"))?
            .requests
            .remove(&QueryTaskId::Native(request_id));
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
        let identity = Arc::new(());
        let mut workers = self
            .workers
            .lock()
            .map_err(|_| ApplicationError::internal("query worker lock poisoned"))?;
        state.requests.insert(
            request_id,
            ActiveQuery {
                cancel,
                identity: identity.clone(),
            },
        );
        workers.retain(|worker| !worker.is_finished());
        let task_state = self.state.clone();
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
            if let Ok(mut state) = task_state.lock() {
                if state.requests.get(&request_id).is_some_and(|request| Arc::ptr_eq(&request.identity, &identity)) {
                    state.requests.remove(&request_id);
                    if !state.is_closed {
                        if let Some(result) = result {
                            publish(result);
                        }
                    }
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
