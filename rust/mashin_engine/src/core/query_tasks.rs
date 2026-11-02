//! Owns cancellable read tasks; no registry lock is held during network I/O.

use super::models::CoreError;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Clone, Default)]
pub(crate) struct QueryTasks {
    requests: Arc<Mutex<HashMap<u64, ActiveQuery>>>,
}

struct ActiveQuery {
    cancel: oneshot::Sender<()>,
    identity: Arc<()>,
}

impl QueryTasks {
    // Registration precedes spawning, so immediate cancellation cannot miss the task.
    pub(crate) fn start(
        &self,
        runtime: &tokio::runtime::Handle,
        request_id: u64,
        timeout: Duration,
        operation: impl Future<Output = Result<Value, CoreError>> + Send + 'static,
        publish: impl FnOnce(Result<Value, CoreError>) + Send + 'static,
    ) -> Result<(), CoreError> {
        let (cancel, canceled) = oneshot::channel();
        let mut requests = self
            .requests
            .lock()
            .map_err(|_| CoreError::internal("query registry lock poisoned"))?;
        if requests.contains_key(&request_id) {
            return Err(CoreError::internal("query request ID is already active"));
        }
        let identity = Arc::new(());
        requests.insert(
            request_id,
            ActiveQuery {
                cancel,
                identity: identity.clone(),
            },
        );
        drop(requests);
        let tasks = self.clone();
        runtime.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = canceled => None,
                result = tokio::time::timeout(timeout, operation) => Some(
                    result.unwrap_or_else(|_| Err(CoreError::command_failed(format!(
                        "Music Assistant query exceeded its {} ms total deadline", timeout.as_millis(),
                    ))))
                ),
            };
            if let Ok(mut requests) = tasks.requests.lock() {
                if requests.get(&request_id).is_some_and(|request| Arc::ptr_eq(&request.identity, &identity)) {
                    requests.remove(&request_id);
                }
            }
            if let Some(result) = result {
                publish(result);
            }
        });
        Ok(())
    }

    // Dropping the selected operation also drops its in-flight HTTP/enrichment futures.
    pub(crate) fn cancel(&self, request_id: u64) -> Result<(), CoreError> {
        let cancel = self
            .requests
            .lock()
            .map_err(|_| CoreError::internal("query registry lock poisoned"))?
            .remove(&request_id);
        if let Some(cancel) = cancel {
            let _ = cancel.cancel.send(());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dropped(Option<oneshot::Sender<()>>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn cancellation_drops_network_work_without_blocking_another_query() {
        let tasks = QueryTasks::default();
        let (dropped, observed_drop) = oneshot::channel();
        let guard = Dropped(Some(dropped));
        tasks
            .start(
                &tokio::runtime::Handle::current(),
                1,
                Duration::from_secs(60),
                async move {
                    let _guard = guard;
                    std::future::pending().await
                },
                |_| panic!("canceled query must not publish"),
            )
            .unwrap();
        let (completed, result) = oneshot::channel();
        tasks
            .start(
                &tokio::runtime::Handle::current(),
                2,
                Duration::from_secs(60),
                async { Ok(Value::Bool(true)) },
                move |value| {
                    let _ = completed.send(value);
                },
            )
            .unwrap();
        tasks.cancel(1).unwrap();
        tasks.cancel(1).unwrap();
        tokio::time::timeout(Duration::from_secs(1), observed_drop)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.await.unwrap().unwrap(), Value::Bool(true));
        assert!(tasks.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canceled_task_cannot_remove_a_replacement_registration() {
        let tasks = QueryTasks::default();
        let (dropped, observed_drop) = oneshot::channel();
        let guard = Dropped(Some(dropped));
        tasks
            .start(
                &tokio::runtime::Handle::current(),
                1,
                Duration::from_secs(60),
                async move {
                    let _guard = guard;
                    std::future::pending().await
                },
                |_| panic!("canceled query must not publish"),
            )
            .unwrap();
        tasks.cancel(1).unwrap();
        let (dropped, replacement_drop) = oneshot::channel();
        let guard = Dropped(Some(dropped));
        tasks
            .start(
                &tokio::runtime::Handle::current(),
                1,
                Duration::from_secs(60),
                async move {
                    let _guard = guard;
                    std::future::pending().await
                },
                |_| panic!("replacement must be canceled"),
            )
            .unwrap();
        observed_drop.await.unwrap();
        assert!(tasks.requests.lock().unwrap().contains_key(&1));
        tasks.cancel(1).unwrap();
        replacement_drop.await.unwrap();
        assert!(tasks.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn total_deadline_releases_the_registry_and_drops_network_work() {
        let tasks = QueryTasks::default();
        let (dropped, observed_drop) = oneshot::channel();
        let guard = Dropped(Some(dropped));
        let (completed, result) = oneshot::channel();
        tasks
            .start(
                &tokio::runtime::Handle::current(),
                1,
                Duration::from_millis(1),
                async move {
                    let _guard = guard;
                    std::future::pending().await
                },
                move |value| {
                    let _ = completed.send(value);
                },
            )
            .unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(1), result)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        observed_drop.await.unwrap();
        assert!(tasks.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_queries_release_registration() {
        let tasks = QueryTasks::default();
        let (completed, result) = oneshot::channel();
        tasks
            .start(
                &tokio::runtime::Handle::current(),
                1,
                Duration::from_secs(1),
                async { Err(CoreError::command_failed("provider failed")) },
                move |value| {
                    let _ = completed.send(value);
                },
            )
            .unwrap();
        assert!(result.await.unwrap().is_err());
        assert!(tasks.requests.lock().unwrap().is_empty());
    }
}
