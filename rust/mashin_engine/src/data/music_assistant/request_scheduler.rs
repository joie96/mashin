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

type Response = Result<Value, HttpCommandError>;
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
    background_active: usize,
    startup_leases: usize,
    read_groups: HashMap<u64, RequestPriority>,
    jobs: HashMap<String, Job>,
}

struct Job {
    id: u64,
    priority: RequestPriority,
    queued_at: Instant,
    subscribers: BTreeMap<u64, oneshot::Sender<Response>>,
    read_groups: HashSet<u64>,
    operation: Option<Operation>,
    running: Option<(bool, tokio::task::AbortHandle)>,
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

    // A repository refresh propagates its group through mapping and all associated technical reads.
    pub(crate) async fn scope<T>(&self, operation: impl Future<Output = T>) -> T {
        READ_GROUP_ID.scope(self.id, operation).await
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
            let priority = group_id
                .and_then(|id| state.read_groups.get(&id).copied())
                .unwrap_or_else(current_priority)
                .max(current_priority());
            let job = state.jobs.entry(key.clone()).or_insert_with(|| Job {
                id: subscriber_id,
                priority,
                queued_at: Instant::now(),
                subscribers: BTreeMap::new(),
                read_groups: HashSet::new(),
                operation: Some(Box::pin(operation)),
                running: None,
            });
            job.priority = job.priority.max(priority);
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
        let candidate = state
            .jobs
            .iter()
            .filter(|(_, job)| {
                job.operation.is_some()
                    && (job.priority != RequestPriority::Background || state.startup_leases == 0)
                    && (job.priority >= RequestPriority::Icon || state.background_active == 0)
            })
            .max_by_key(|(_, job)| (job.priority, std::cmp::Reverse(job.id)))
            .map(|(key, _)| key.clone());
        let Some(key) = candidate else { break };
        let job = state.jobs.get_mut(&key).expect("queued request exists");
        let operation = job.operation.take().expect("queued request has operation");
        let is_background = job.priority < RequestPriority::Icon;
        let priority = job.priority;
        let queued_at = job.queued_at;
        let command = key.split(':').next().unwrap_or_default().to_owned();
        let running = RunningRequest {
            inner: inner.clone(),
            key,
            job_id: job.id,
        };
        state.active += 1;
        state.background_active += usize::from(is_background);
        let task = tokio::spawn(async move {
            let started = Instant::now();
            let wait_ms = queued_at.elapsed().as_millis();
            let response = operation.await;
            LOGGER.debug(format_args!(
                "Read finished: command={} priority={:?} wait_ms={} elapsed_ms={} success={}",
                command,
                priority,
                wait_ms,
                started.elapsed().as_millis(),
                response.is_ok()
            ));
            running.finish(Some(response));
        });
        job.running = Some((is_background, task.abort_handle()));
    }
}

fn remove_job(state: &mut State, key: &str, job_id: u64) -> Option<Job> {
    if state.jobs.get(key).is_none_or(|job| job.id != job_id) {
        return None;
    }
    let job = state.jobs.remove(key)?;
    if let Some((is_background, _)) = &job.running {
        state.active -= 1;
        state.background_active -= usize::from(*is_background);
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
                if let Some((_, task)) = job.running {
                    task.abort();
                }
            }
            dispatch(&self.inner, &mut state);
        }
    }
}
