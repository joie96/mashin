//! Owns one session's queries, cancellable workers and non-abortable blocking work.
//! Closing suppresses results immediately; joining must run outside application state locks.

use super::errors::ApplicationError;
use super::events::EventPublisher;
use super::query_tasks::QueryTasks;
use crate::application::state::ApplicationState;
use crate::data::music_assistant::connection::MusicAssistantConnection;
use crate::domain::identifiers::UserId;
use crate::domain::session::AccessToken;
use crate::domain::settings::{default_local_output, DEFAULT_LOCAL_VOLUME};
use crate::domain::settings::{
    LogLevelPreference, LoginConfig, SelectedOutput, SessionConfig, ThemePreference,
};
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};
use tokio::task::{AbortHandle, JoinHandle};

pub(crate) trait SessionStorage: Send {
    fn load_session(&self) -> Result<Option<SessionConfig>, String>;
    fn save_session(&self, session: SessionConfig) -> Result<(), String>;
    fn clear_session(&self) -> Result<(), String>;
}

#[derive(Clone)]
pub(crate) struct SessionEnvironment {
    pub(crate) authenticate:
        fn(
            LoginConfig,
        )
            -> futures_util::future::BoxFuture<'static, Result<AuthenticatedIdentity, String>>,
    pub(crate) client_name: fn() -> String,
    pub(crate) connect:
        fn(
            crate::domain::session::MusicAssistantServerUrl,
            Option<AccessToken>,
        )
            -> Result<crate::data::music_assistant::connection::MusicAssistantConnection, String>,
    pub(crate) app_settings: Arc<dyn super::settings::AppSettingsStorage>,
    pub(crate) repositories: fn(
        Arc<crate::data::music_assistant::client::MusicAssistantClient>,
        crate::domain::session::MusicAssistantServerUrl,
        Option<UserId>,
    ) -> futures_util::future::BoxFuture<
        'static,
        Result<super::repositories::SessionServices, String>,
    >,
}

pub(crate) struct AuthenticatedIdentity {
    pub(crate) access_token: AccessToken,
    pub(crate) user_id: UserId,
    pub(crate) username: String,
}

pub(crate) struct RetiredWorker {
    worker: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl RetiredWorker {
    // The component has already requested cancellation before transferring its handle.
    pub(crate) fn new(worker: JoinHandle<()>) -> Self {
        Self {
            worker: tokio::sync::Mutex::new(Some(worker)),
        }
    }

    // Keep ownership across await so canceling a join cannot detach the worker.
    pub(crate) async fn join(&self) -> Result<(), ApplicationError> {
        let mut slot = self.worker.lock().await;
        let result = if let Some(worker) = slot.as_mut() {
            worker.await
        } else {
            return Ok(());
        };
        slot.take();
        match result {
            Err(error) if !error.is_cancelled() => Err(ApplicationError::internal(format!(
                "transport worker failed: {error}",
            ))),
            _ => Ok(()),
        }
    }
}

pub(crate) struct SessionTasks {
    queries: Arc<QueryTasks>,
    repositories_load: tokio::sync::Mutex<()>,
    user_data_load: tokio::sync::Mutex<()>,
    session_data_load: tokio::sync::Mutex<()>,
    cancellation: watch::Sender<bool>,
    state: Mutex<SessionTasksState>,
    join_guard: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct SessionTasksState {
    is_closed: bool,
    workers: Vec<JoinHandle<()>>,
}

impl Default for SessionTasks {
    fn default() -> Self {
        Self {
            queries: Arc::new(QueryTasks::default()),
            repositories_load: tokio::sync::Mutex::new(()),
            user_data_load: tokio::sync::Mutex::new(()),
            session_data_load: tokio::sync::Mutex::new(()),
            cancellation: watch::channel(false).0,
            state: Mutex::new(SessionTasksState::default()),
            join_guard: tokio::sync::Mutex::new(()),
        }
    }
}

impl SessionTasks {
    // Lifecycle and completion barriers.
    pub(crate) fn close(&self) {
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.is_closed = true;
            self.cancellation.send_replace(true);
        }
        self.queries.close();
    }

    pub(crate) async fn join(&self) -> Result<(), ApplicationError> {
        self.close();
        let _join_guard = self.join_guard.lock().await;
        let mut failure = self.queries.join().await.err();
        let workers = {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| {
                failure.get_or_insert_with(|| {
                    ApplicationError::internal("session worker lock poisoned")
                });
                poisoned.into_inner()
            });
            std::mem::take(&mut state.workers)
        };
        for worker in workers {
            if let Err(error) = worker.await {
                if !error.is_cancelled() {
                    failure.get_or_insert_with(|| {
                        ApplicationError::internal(format!("session worker failed: {error}"))
                    });
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    // Worker registration is serialized with close, never with network I/O.
    pub(crate) fn spawn(
        &self,
        runtime: &Handle,
        operation: impl Future<Output = ()> + Send + 'static,
    ) -> Result<AbortHandle, ApplicationError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ApplicationError::internal("session worker lock poisoned"))?;
        if state.is_closed {
            return Err(ApplicationError::command_failed(
                "session task scope has ended",
            ));
        }
        let mut cancellation = self.cancellation.subscribe();
        state.workers.retain(|worker| !worker.is_finished());
        let worker = runtime.spawn(async move {
            tokio::select! {
                biased;
                _ = cancellation.changed() => {},
                _ = operation => {},
            }
        });
        let abort = worker.abort_handle();
        state.workers.push(worker);
        Ok(abort)
    }

    pub(crate) async fn run_blocking<Response: Send + 'static>(
        &self,
        runtime: &Handle,
        operation: impl FnOnce() -> Response + Send + 'static,
    ) -> Result<Response, ApplicationError> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| ApplicationError::internal("session worker lock poisoned"))?;
            if state.is_closed {
                return Err(ApplicationError::command_failed(
                    "session task scope has ended",
                ));
            }
            state.workers.retain(|worker| !worker.is_finished());
            state.workers.push(runtime.spawn_blocking(move || {
                let result = operation();
                let _ = sender.send(result);
            }));
        }
        receiver.await.map_err(|error| {
            ApplicationError::internal(format!("blocking session worker failed: {error}"))
        })
    }

    // Load coalescing belongs to this session, never to a replacement session.
    pub(crate) async fn lock_repositories_load(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.repositories_load.lock().await
    }

    pub(crate) async fn lock_user_data_load(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.user_data_load.lock().await
    }

    pub(crate) fn try_lock_session_data_load(&self) -> Option<tokio::sync::MutexGuard<'_, ()>> {
        self.session_data_load.try_lock().ok()
    }

    pub(crate) async fn lock_session_data_load(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.session_data_load.lock().await
    }

    // Queries share this session's cancellation and join boundary.
    pub(crate) fn queries(&self) -> Arc<QueryTasks> {
        self.queries.clone()
    }

    pub(crate) fn events(self: &Arc<Self>, sink: EventPublisher) -> EventPublisher {
        let session = Arc::downgrade(self);
        EventPublisher::new(move |event| {
            if let Some(session) = session.upgrade() {
                if let Ok(state) = session.state.lock() {
                    if !state.is_closed {
                        sink.publish(event);
                    }
                }
            }
        })
    }
}

impl Drop for SessionTasks {
    fn drop(&mut self) {
        self.close();
    }
}

// Authentication performs network I/O before the guarded session commit.
pub(crate) async fn authenticate(
    login: LoginConfig,
    environment: &SessionEnvironment,
) -> Result<SessionConfig, String> {
    if login.music_assistant_url.0.trim().is_empty() {
        return Err("Music Assistant URL is required".to_string());
    }
    if login.username.trim().is_empty() {
        return Err("Music Assistant username is required".to_string());
    }
    if login.password.is_empty() {
        return Err("Music Assistant password is required".to_string());
    }
    let server_url = login.music_assistant_url.clone();
    let identity = (environment.authenticate)(login).await?;
    Ok(SessionConfig {
        music_assistant_url: server_url,
        access_token: Some(identity.access_token),
        user_id: Some(identity.user_id),
        username: identity.username,
        selected_output: SelectedOutput::Local,
        local: default_local_output((environment.client_name)()),
        volume: DEFAULT_LOCAL_VOLUME,
        theme: ThemePreference::Dark,
        log_level: LogLevelPreference::Info,
        should_show_multi_device_button: true,
        preferred_provider: None,
    })
}

impl ApplicationState {
    // Session configuration, persistence, and output selection.
    pub(crate) fn configure_session(&mut self, mut session: SessionConfig) -> Result<(), String> {
        crate::domain::settings::refresh_local_client_name(
            &mut session.local,
            (self.environment.client_name)(),
        );
        self.configure_and_persist_session(&session)?;
        crate::logging::set_minimum_level(session.log_level.as_str())?;
        self.invalidate_session_data();
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn restore_persisted_session(&mut self) -> Result<bool, String> {
        if self.session_configuration.is_some() {
            return Ok(true);
        }
        let Some(mut session) = self.session_storage.load_session()? else {
            return Ok(false);
        };
        crate::domain::settings::refresh_local_client_name(
            &mut session.local,
            (self.environment.client_name)(),
        );
        self.configure_selected_output(&session)?;
        crate::logging::set_minimum_level(session.log_level.as_str())?;
        self.invalidate_session_data();
        self.session_configuration = Some(session);
        Ok(true)
    }

    pub(crate) fn commit_authenticated_session(
        &mut self,
        expected_generation: u64,
        session: SessionConfig,
    ) -> Result<(), String> {
        if self.session_generation != expected_generation {
            return Err("session changed while authenticating".into());
        }
        self.configure_session(session)
    }

    // The caller revokes the detached token outside application state locks.
    pub(crate) fn logout(
        &mut self,
    ) -> Result<Option<std::sync::Arc<MusicAssistantConnection>>, String> {
        self.session_storage.clear_session()?;
        let connection = self.connection.clone();
        self.shutdown();
        Ok(connection)
    }

    pub(crate) fn select_output(&mut self, selected_output: SelectedOutput) -> Result<(), String> {
        let mut session = self.session_configuration.clone().ok_or_else(|| {
            "a restored session is required before selecting an output".to_string()
        })?;
        session.selected_output = selected_output;
        self.configure_and_persist_session(&session)?;
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn select_local_output(&mut self) -> Result<(), String> {
        self.select_output(SelectedOutput::Local)
    }

    pub(crate) fn configure_and_persist_session(
        &mut self,
        session: &SessionConfig,
    ) -> Result<(), String> {
        let previous = self.session_configuration.clone();
        let previous_playback = self.playback.checkpoint();
        let previous_connection = self.connection.clone();
        let requires_configuration = previous.as_ref().is_none_or(|current| {
            current.selected_output != session.selected_output
                || current.music_assistant_url != session.music_assistant_url
                || current.access_token != session.access_token
                || current.local != session.local
        });
        if requires_configuration {
            self.configure_selected_output(session)?;
        }
        if let Err(error) = self.session_storage.save_session(session.clone()) {
            if requires_configuration {
                self.playback.restore_session(
                    previous_playback,
                    previous_connection.clone().map(|connection| {
                        connection
                            as std::sync::Arc<dyn crate::playback::server::PlaybackConnection>
                    }),
                )?;
                let rejected = std::mem::replace(&mut self.connection, previous_connection);
                self.retire_replaced_connection(rejected);
            }
            return Err(error);
        }
        self.retire_replaced_connection(previous_connection);
        Ok(())
    }

    // Validate and prepare replacements before releasing the currently owned session.
    fn configure_selected_output(&mut self, session: &SessionConfig) -> Result<(), String> {
        if matches!(&session.selected_output, SelectedOutput::Remote(remote) if remote.player_id.as_str().trim().is_empty())
        {
            return Err("Music Assistant remote player ID is required".to_string());
        }
        let connection = match self.connection.as_ref().filter(|connection| {
            connection.matches(&session.music_assistant_url, &session.access_token)
        }) {
            Some(connection) => connection.clone(),
            None => std::sync::Arc::new((self.environment.connect)(
                session.music_assistant_url.clone(),
                session.access_token.clone(),
            )?),
        };
        if let Some(user_id) = &session.user_id {
            connection
                .client
                .initialize_current_user_id(user_id.clone())?;
        }
        let configured = self.playback.configure_session(
            crate::playback::models::PlaybackSessionConfiguration {
                local: crate::playback::local::session::LocalSessionConfiguration {
                    device_id: session.local.device_id.clone(),
                    client_name: session.local.client_name.clone(),
                    cache_bytes: session.local.cache_bytes,
                    audio_profile: session.local.audio_profile,
                    volume: session.volume,
                },
                target: match &session.selected_output {
                    SelectedOutput::Local => crate::playback::models::PlaybackTarget::Local,
                    SelectedOutput::Remote(remote) => {
                        crate::playback::models::PlaybackTarget::Remote(remote.player_id.clone())
                    }
                },
            },
            connection.clone(),
            &self.runtime,
        );
        if let Err(error) = configured {
            self.retire_replaced_connection(Some(connection));
            return Err(error);
        }
        self.connection = Some(connection);
        Ok(())
    }

    // Persist identity enrichment only for the session that requested it.
    pub(crate) fn remember_current_user_id(
        &mut self,
        generation: u64,
        user_id: UserId,
    ) -> Result<(), String> {
        if self.session_generation != generation {
            return Err("session changed while resolving the current user".into());
        }
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required".to_owned())?;
        if let Some(current) = &session.user_id {
            return if current == &user_id {
                Ok(())
            } else {
                Err("Music Assistant session user ID cannot change".into())
            };
        }
        session.user_id = Some(user_id);
        self.session_storage.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }
}
