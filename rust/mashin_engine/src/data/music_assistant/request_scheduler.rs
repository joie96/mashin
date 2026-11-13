//! Session-owned read scheduling; subscribers own cancellation, not background workers.

use super::client::HttpCommandError;
use crate::logging::Logger;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::oneshot;

#[derive(Clone)]
enum ReadPayload {
    Json(Value),
    Bytes(Arc<[u8]>),
}

type Response = Result<ReadPayload, HttpCommandError>;
type Operation = Pin<Box<dyn Future<Output = Response> + Send>>;
static LOGGER: Logger = Logger::new(module_path!());

use crate::application::repositories::{current_priority, RequestPriority};
tokio::task_local! { static READ_GROUP_ID: u64; }

#[derive(Default)]
pub(crate) struct RequestScheduler {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    active: usize,
    startup_leases: usize,
    read_groups: HashMap<u64, RequestPriority>,
    optional_read_groups: HashSet<u64>,
    jobs: HashMap<String, Job>,
}

struct Job {
    id: u64,
    priority: RequestPriority,
    is_optional: bool,
    queued_at: Instant,
    subscribers: BTreeMap<u64, oneshot::Sender<Response>>,
    read_groups: HashSet<u64>,
    operation: Option<Operation>,
    running: Option<tokio::task::AbortHandle>,
}

struct Subscription {
    inner: Arc<Inner>,
    key: String,
    job_id: u64,
    subscriber_id: u64,
}

struct RunningRequest {
    inner: Arc<Inner>,
    key: String,
    job_id: u64,
}

pub(super) struct StartupLease {
    inner: Arc<Inner>,
}

pub(crate) struct ReadGroup {
    inner: Arc<Inner>,
    id: u64,
}

// Dynamic selections must be isolated before joining the shared network queue.
pub(super) fn command_read_key(command: &str, arguments: &Value) -> String {
    let requires_independent_read = arguments
        .get("allow_dynamic_tracks")
        .and_then(Value::as_bool)
        == Some(true)
        || matches!(
            command,
            "music/playlists/playlist_tracks" | "music/recommendations/items"
        )
        || (command == "music/tracks/similar_tracks"
            && arguments.get("allow_lookup").and_then(Value::as_bool) == Some(true))
        || (matches!(
            arguments.get("sort_field").and_then(Value::as_str),
            Some("random" | "random_play_count")
        ) && arguments.get("random_seed").is_none_or(Value::is_null));
    if requires_independent_read {
        format!("{command}:{arguments}:{}", uuid::Uuid::new_v4())
    } else {
        format!("{command}:{arguments}")
    }
}

impl ReadGroup {
    #[cfg(test)]
    pub(crate) fn effective_priority(&self) -> RequestPriority {
        self.inner.state.lock().unwrap().read_groups[&self.id]
    }

    #[cfg(test)]
    pub(crate) fn is_network_response_required(&self) -> bool {
        !self
            .inner
            .state
            .lock()
            .unwrap()
            .optional_read_groups
            .contains(&self.id)
    }

    // A repository refresh propagates its group through mapping and all associated technical reads.
    pub(crate) async fn scope<T>(&self, operation: impl Future<Output = T>) -> T {
        READ_GROUP_ID.scope(self.id, operation).await
    }

    // Reserve capacity before the local read races the first network request.
    pub(crate) fn reserve_interactive_lane(&self) {
        self.inner
            .state
            .lock()
            .expect("request scheduler lock poisoned")
            .optional_read_groups
            .insert(self.id);
    }

    // Missing local data makes even an already queued or running refresh required.
    pub(crate) fn require_network_response(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("request scheduler lock poisoned");
        state.optional_read_groups.remove(&self.id);
        for job in state
            .jobs
            .values_mut()
            .filter(|job| job.read_groups.contains(&self.id))
        {
            job.is_optional = false;
        }
        dispatch(&self.inner, &mut state);
    }

    pub(crate) fn promote_to_current_priority(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("request scheduler lock poisoned");
        let priority = state
            .read_groups
            .get_mut(&self.id)
            .expect("active read group exists");
        *priority = (*priority).max(current_priority());
        let priority = *priority;
        for job in state
            .jobs
            .values_mut()
            .filter(|job| job.read_groups.contains(&self.id))
        {
            job.priority = job.priority.max(priority);
        }
        dispatch(&self.inner, &mut state);
    }
}

impl Drop for ReadGroup {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("request scheduler lock poisoned");
        state.read_groups.remove(&self.id);
        state.optional_read_groups.remove(&self.id);
        for job in state.jobs.values_mut() {
            job.read_groups.remove(&self.id);
        }
    }
}

impl RequestScheduler {
    #[cfg(test)]
    pub(crate) fn pending_read_count(&self) -> usize {
        self.inner.state.lock().unwrap().jobs.len()
    }

    pub(crate) fn create_read_group(&self) -> Arc<ReadGroup> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("request scheduler lock poisoned");
        state.next_id += 1;
        let id = state.next_id;
        state.read_groups.insert(id, current_priority());
        Arc::new(ReadGroup {
            inner: self.inner.clone(),
            id,
        })
    }

    // Optional hydration cannot take the background lane between startup requests.
    pub(super) fn begin_startup(&self) -> StartupLease {
        self.inner
            .state
            .lock()
            .expect("request scheduler lock poisoned")
            .startup_leases += 1;
        StartupLease {
            inner: self.inner.clone(),
        }
    }

    // Read subscriptions and priority promotion.
    pub(crate) async fn execute(
        &self,
        key: String,
        operation: impl Future<Output = Result<Value, HttpCommandError>> + Send + 'static,
    ) -> Result<Value, HttpCommandError> {
        match self
            .execute_read(format!("json:{key}"), async move {
                operation.await.map(ReadPayload::Json)
            })
            .await?
        {
            ReadPayload::Json(value) => Ok(value),
            ReadPayload::Bytes(_) => unreachable!("JSON and byte jobs have separate keys"),
        }
    }

    pub(super) async fn execute_bytes(
        &self,
        key: String,
        operation: impl Future<Output = Result<Vec<u8>, HttpCommandError>> + Send + 'static,
    ) -> Result<Arc<[u8]>, HttpCommandError> {
        match self
            .execute_read(format!("bytes:{key}"), async move {
                operation
                    .await
                    .map(|bytes| ReadPayload::Bytes(bytes.into()))
            })
            .await?
        {
            ReadPayload::Bytes(bytes) => Ok(bytes),
            ReadPayload::Json(_) => unreachable!("JSON and byte jobs have separate keys"),
        }
    }

    async fn execute_read(
        &self,
        key: String,
        operation: impl Future<Output = Response> + Send + 'static,
    ) -> Response {
        let (sender, receiver) = oneshot::channel();
        let subscription = {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("request scheduler lock poisoned");
            state.next_id += 1;
            let subscriber_id = state.next_id;
            let group_id = READ_GROUP_ID.try_with(|id| *id).ok();
            let is_optional = group_id.is_some_and(|id| state.optional_read_groups.contains(&id));
            let priority = group_id
                .and_then(|id| state.read_groups.get(&id).copied())
                .unwrap_or_else(current_priority)
                .max(current_priority());
            let job = state.jobs.entry(key.clone()).or_insert_with(|| Job {
                id: subscriber_id,
                priority,
                is_optional,
                queued_at: Instant::now(),
                subscribers: BTreeMap::new(),
                read_groups: HashSet::new(),
                operation: Some(Box::pin(operation)),
                running: None,
            });
            job.priority = job.priority.max(priority);
            job.is_optional &= is_optional;
            if let Some(group_id) = group_id {
                job.read_groups.insert(group_id);
            }
            job.subscribers.insert(subscriber_id, sender);
            let subscription = Subscription {
                inner: self.inner.clone(),
                key,
                job_id: job.id,
                subscriber_id,
            };
            dispatch(&self.inner, &mut state);
            subscription
        };
        let result = receiver.await.unwrap_or_else(|_| {
            Err(HttpCommandError::Transport {
                command: "scheduled read".into(),
                is_timeout: false,
                is_connect: false,
                details: "request worker stopped".into(),
            })
        });
        drop(subscription);
        result
    }
}

// Scheduling and completion keep at most one non-interactive request in flight.
fn dispatch(inner: &Arc<Inner>, state: &mut State) {
    while state.active < 2 {
        let has_non_interactive_read = state.jobs.values().any(|job| {
            job.running.is_some() && (job.is_optional || job.priority < RequestPriority::Icon)
        });
        let candidate = state
            .jobs
            .iter()
            .filter(|(_, job)| {
                job.operation.is_some()
                    && (job.priority != RequestPriority::Background || state.startup_leases == 0)
                    && ((!job.is_optional && job.priority >= RequestPriority::Icon)
                        || !has_non_interactive_read)
            })
            .max_by_key(|(_, job)| (!job.is_optional, job.priority, std::cmp::Reverse(job.id)))
            .map(|(key, _)| key.clone());
        let Some(key) = candidate else { break };
        let job = state.jobs.get_mut(&key).expect("queued request exists");
        let operation = job.operation.take().expect("queued request has operation");
        let priority = job.priority;
        let queued_at = job.queued_at;
        let command = key
            .strip_prefix("json:")
            .map(|key| key.split(':').next().unwrap_or_default())
            .unwrap_or("imageproxy")
            .to_owned();
        let running = RunningRequest {
            inner: inner.clone(),
            key,
            job_id: job.id,
        };
        state.active += 1;
        let task = tokio::spawn(async move {
            let started = Instant::now();
            let wait_ms = queued_at.elapsed().as_millis();
            let response = operation.await;
            LOGGER.trace(format_args!(
                "Read finished: command={} priority={:?} wait_ms={} elapsed_ms={} success={}",
                command,
                priority,
                wait_ms,
                started.elapsed().as_millis(),
                response.is_ok()
            ));
            running.finish(Some(response));
        });
        job.running = Some(task.abort_handle());
    }
}

fn remove_job(state: &mut State, key: &str, job_id: u64) -> Option<Job> {
    if state.jobs.get(key).is_none_or(|job| job.id != job_id) {
        return None;
    }
    let job = state.jobs.remove(key)?;
    if job.running.is_some() {
        state.active -= 1;
    }
    Some(job)
}

impl RunningRequest {
    fn finish(&self, response: Option<Response>) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("request scheduler lock poisoned");
        if let Some(job) = remove_job(&mut state, &self.key, self.job_id) {
            if let Some(response) = response {
                for subscriber in job.subscribers.into_values() {
                    let _ = subscriber.send(response.clone());
                }
            }
            dispatch(&self.inner, &mut state);
        }
    }
}

impl Drop for RunningRequest {
    fn drop(&mut self) {
        self.finish(None);
    }
}

impl Drop for StartupLease {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("request scheduler lock poisoned");
        state.startup_leases -= 1;
        dispatch(&self.inner, &mut state);
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("request scheduler lock poisoned");
        let Some(job) = state
            .jobs
            .get_mut(&self.key)
            .filter(|job| job.id == self.job_id)
        else {
            return;
        };
        job.subscribers.remove(&self.subscriber_id);
        if job.subscribers.is_empty() {
            if let Some(job) = remove_job(&mut state, &self.key, self.job_id) {
                if let Some(task) = job.running {
                    task.abort();
                }
            }
            dispatch(&self.inner, &mut state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn cached_refreshes_reserve_capacity_until_a_network_response_is_required() {
        for required_group in 0..2 {
            let scheduler = Arc::new(RequestScheduler::default());
            let mut requests = Vec::new();
            let mut groups = Vec::new();
            let mut starts = Vec::new();
            for key in ["cached-albums", "cached-top-tracks"] {
                let group = scheduler.create_read_group();
                group.reserve_interactive_lane();
                let owner = scheduler.clone();
                let request_group = group.clone();
                let (started, start) = oneshot::channel();
                requests.push(tokio::spawn(async move {
                    request_group
                        .scope(owner.execute(key.into(), async move {
                            started.send(()).unwrap();
                            std::future::pending::<Result<Value, HttpCommandError>>().await
                        }))
                        .await
                }));
                groups.push(group);
                starts.push(start);
            }
            tokio::time::timeout(std::time::Duration::from_secs(2), &mut starts[0])
                .await
                .unwrap()
                .unwrap();
            groups[1].promote_to_current_priority();
            tokio::task::yield_now().await;
            assert_eq!(scheduler.inner.state.lock().unwrap().active, 1);
            assert!(matches!(
                starts[1].try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert_eq!(
                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    scheduler.execute_bytes("visible-cover".into(), async { Ok(vec![1, 2, 3]) })
                )
                .await
                .unwrap()
                .unwrap()
                .as_ref(),
                &[1, 2, 3]
            );
            assert_eq!(scheduler.inner.state.lock().unwrap().active, 1);
            groups[required_group].require_network_response();
            tokio::time::timeout(std::time::Duration::from_secs(2), &mut starts[1])
                .await
                .unwrap()
                .unwrap();
            assert_eq!(scheduler.inner.state.lock().unwrap().active, 2);
            for request in requests {
                request.abort();
                assert!(request.await.unwrap_err().is_cancelled());
            }
            assert_eq!(scheduler.pending_read_count(), 0);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn required_consumer_reuses_optional_work_and_releases_the_reserved_lane() {
        let scheduler = Arc::new(RequestScheduler::default());
        let shared_group = scheduler.create_read_group();
        shared_group.reserve_interactive_lane();
        let owner = scheduler.clone();
        let (started, start) = oneshot::channel();
        let (release, hold) = oneshot::channel();
        let first = tokio::spawn(async move {
            shared_group
                .scope(owner.execute("shared".into(), async move {
                    started.send(()).unwrap();
                    hold.await.unwrap();
                    Ok(serde_json::json!(["fresh"]))
                }))
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), start)
            .await
            .unwrap()
            .unwrap();
        let queued_group = scheduler.create_read_group();
        queued_group.reserve_interactive_lane();
        let owner = scheduler.clone();
        let (started, mut start) = oneshot::channel();
        let queued = tokio::spawn(async move {
            queued_group
                .scope(owner.execute("other-refresh".into(), async move {
                    started.send(()).unwrap();
                    std::future::pending::<Result<Value, HttpCommandError>>().await
                }))
                .await
        });
        tokio::task::yield_now().await;
        assert!(matches!(
            start.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let owner = scheduler.clone();
        let required = tokio::spawn(async move {
            owner
                .execute("shared".into(), async {
                    panic!("shared work must not restart")
                })
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), start)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(scheduler.inner.state.lock().unwrap().active, 2);
        assert_eq!(
            scheduler.inner.state.lock().unwrap().jobs["json:shared"]
                .subscribers
                .len(),
            2
        );
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), required)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            serde_json::json!(["fresh"])
        );
        queued.abort();
        assert!(queued.await.unwrap_err().is_cancelled());
        assert_eq!(scheduler.pending_read_count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn canceling_hung_reads_releases_both_slots_for_another_page() {
        struct NotifyDrop(Option<oneshot::Sender<()>>);
        impl Drop for NotifyDrop {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }

        let scheduler = Arc::new(RequestScheduler::default());
        let mut requests = Vec::new();
        let mut cancellations = Vec::new();
        for key in ["artist-albums", "artist-top-tracks"] {
            let (started, start) = oneshot::channel();
            let (dropped, drop_observed) = oneshot::channel();
            let owner = scheduler.clone();
            requests.push(tokio::spawn(async move {
                owner
                    .execute(key.into(), async move {
                        let _guard = NotifyDrop(Some(dropped));
                        started.send(()).unwrap();
                        std::future::pending::<Result<Value, HttpCommandError>>().await
                    })
                    .await
            }));
            tokio::time::timeout(std::time::Duration::from_secs(2), start)
                .await
                .unwrap()
                .unwrap();
            cancellations.push(drop_observed);
        }

        let owner = scheduler.clone();
        let next_page = tokio::spawn(async move {
            owner
                .execute("other-page".into(), async {
                    Ok(serde_json::json!({"loaded": true}))
                })
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(scheduler.pending_read_count(), 3);
        assert_eq!(scheduler.inner.state.lock().unwrap().active, 2);
        assert!(!next_page.is_finished());

        for request in &requests {
            request.abort();
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            for request in requests {
                assert!(request.await.unwrap_err().is_cancelled());
            }
            for cancellation in cancellations {
                cancellation.await.unwrap();
            }
            assert_eq!(
                next_page.await.unwrap().unwrap(),
                serde_json::json!({"loaded": true})
            );
        })
        .await
        .unwrap();
        assert_eq!(scheduler.pending_read_count(), 0);
        assert_eq!(scheduler.inner.state.lock().unwrap().active, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn canceling_one_json_consumer_preserves_the_shared_request() {
        let scheduler = Arc::new(RequestScheduler::default());
        let (started, start) = oneshot::channel();
        let (release, hold) = oneshot::channel();
        let owner = scheduler.clone();
        let first = tokio::spawn(async move {
            owner
                .execute("shared-artist".into(), async move {
                    started.send(()).unwrap();
                    hold.await.unwrap();
                    Ok(serde_json::json!(["track"]))
                })
                .await
        });
        start.await.unwrap();
        let owner = scheduler.clone();
        let second = tokio::spawn(async move {
            owner
                .execute("shared-artist".into(), async {
                    panic!("a shared JSON request must not execute twice")
                })
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(
            scheduler.inner.state.lock().unwrap().jobs["json:shared-artist"]
                .subscribers
                .len(),
            2
        );
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert_eq!(scheduler.pending_read_count(), 1);
        release.send(()).unwrap();
        let response = tokio::time::timeout(std::time::Duration::from_secs(2), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response, serde_json::json!(["track"]));
        assert_eq!(scheduler.pending_read_count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn byte_subscribers_share_one_buffer_and_do_not_join_json_jobs() {
        let scheduler = RequestScheduler::default();
        let (first, second, json) = tokio::join!(
            scheduler.execute_bytes("same".into(), async {
                tokio::task::yield_now().await;
                Ok(vec![1, 2, 3])
            }),
            scheduler.execute_bytes("same".into(), async {
                panic!("duplicate byte operation ran")
            }),
            scheduler.execute("same".into(), async { Ok(serde_json::json!({"value": 4})) }),
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(&*first, &[1, 2, 3]);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(json.unwrap(), serde_json::json!({"value": 4}));
        assert_eq!(scheduler.pending_read_count(), 0);
        assert_eq!(
            &*scheduler
                .execute_bytes("same".into(), async { Ok(vec![9]) })
                .await
                .unwrap(),
            &[9]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn byte_cancellation_preserves_other_subscribers_and_last_drop_cancels_work() {
        let scheduler = Arc::new(RequestScheduler::default());
        let (started, start) = oneshot::channel();
        let (release, hold) = oneshot::channel();
        let owner = scheduler.clone();
        let first = tokio::spawn(async move {
            owner
                .execute_bytes("shared".into(), async move {
                    started.send(()).unwrap();
                    hold.await.unwrap();
                    Ok(vec![7])
                })
                .await
        });
        start.await.unwrap();
        let owner = scheduler.clone();
        let second = tokio::spawn(async move {
            owner
                .execute_bytes("shared".into(), async { panic!("duplicate operation ran") })
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(
            scheduler.inner.state.lock().unwrap().jobs["bytes:shared"]
                .subscribers
                .len(),
            2
        );
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        assert_eq!(&*second.await.unwrap().unwrap(), &[7]);

        let (started, start) = oneshot::channel();
        let (dropped, drop_observed) = oneshot::channel();
        struct NotifyDrop(Option<oneshot::Sender<()>>);
        impl Drop for NotifyDrop {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }
        let owner = scheduler.clone();
        let pending = tokio::spawn(async move {
            owner
                .execute_bytes("cancelled".into(), async move {
                    let _guard = NotifyDrop(Some(dropped));
                    started.send(()).unwrap();
                    std::future::pending::<Result<Vec<u8>, HttpCommandError>>().await
                })
                .await
        });
        start.await.unwrap();
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        tokio::time::timeout(std::time::Duration::from_secs(2), drop_observed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(scheduler.pending_read_count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn foreground_byte_subscriber_promotes_queued_prefetch_during_startup() {
        use crate::application::repositories::with_request_priority;
        let scheduler = Arc::new(RequestScheduler::default());
        let startup = scheduler.begin_startup();
        let (started, mut start) = oneshot::channel();
        let owner = scheduler.clone();
        let prefetch = tokio::spawn(async move {
            with_request_priority(
                RequestPriority::Background,
                owner.execute_bytes("prefetch".into(), async move {
                    started.send(()).unwrap();
                    Ok(vec![8])
                }),
            )
            .await
        });
        tokio::task::yield_now().await;
        assert_eq!(scheduler.pending_read_count(), 1);
        assert!(matches!(
            start.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let visible = with_request_priority(
            RequestPriority::Foreground,
            scheduler.execute_bytes("prefetch".into(), async {
                panic!("promoted prefetch must not download twice")
            }),
        )
        .await
        .unwrap();
        start.await.unwrap();
        assert!(Arc::ptr_eq(&visible, &prefetch.await.unwrap().unwrap()));
        assert_eq!(&*visible, &[8]);
        drop(startup);
        assert_eq!(scheduler.pending_read_count(), 0);
    }
}
