//! Owns the active session state and retirement lists; application workers join outside its lock.
use crate::application::events::EventPublisher;
use crate::application::library::LibraryState;
use crate::application::session::{RetiredWorker, SessionStorage, SessionTasks};
use crate::domain::library::UserDataSnapshot;
use crate::domain::settings::SessionConfig;
use crate::playback::engine::PlaybackEngine;
use crate::playback::models::MediaSessionState;
use std::sync::Arc;
pub(crate) struct ApplicationState {
    pub(crate) environment: crate::application::session::SessionEnvironment,
    pub(crate) runtime: tokio::runtime::Handle,
    event_publisher: EventPublisher,
    pub(crate) session_tasks: Arc<SessionTasks>,
    pub(super) retired_session_tasks: Vec<Arc<SessionTasks>>,
    pub(super) retired_workers: Vec<Arc<RetiredWorker>>,
    pub(crate) session_storage: Box<dyn SessionStorage>,
    pub(crate) playback: PlaybackEngine,
    pub(crate) session_configuration: Option<SessionConfig>,
    pub(crate) user_data: Option<LibraryState>,
    pub(crate) repositories: Option<crate::application::repositories::SessionServices>,
    pub(super) session_data_events: Option<super::session_data::SessionDataEvents>,
    pub(super) initial_session_data_result: Option<Result<(), String>>,
    pub(crate) session_generation: u64,
    pub(crate) connection:
        Option<std::sync::Arc<crate::data::music_assistant::connection::MusicAssistantConnection>>,
    pub(crate) pending_commands: Vec<crate::playback::backend::PlaybackCommand>,
}

impl ApplicationState {
    // Construction and session configuration.
    pub(crate) fn new(
        environment: crate::application::session::SessionEnvironment,
        runtime: tokio::runtime::Handle,
        event_publisher: EventPublisher,
        session_storage: Box<dyn SessionStorage>,
    ) -> Self {
        Self {
            environment,
            runtime,
            playback: PlaybackEngine::new(event_publisher.clone()),
            event_publisher,
            session_tasks: Arc::new(SessionTasks::default()),
            retired_session_tasks: Vec::new(),
            retired_workers: Vec::new(),
            session_storage,
            session_configuration: None,
            user_data: None,
            repositories: None,
            session_data_events: None,
            initial_session_data_result: None,
            session_generation: 0,
            connection: None,
            pending_commands: Vec::new(),
        }
    }

    // Close session state synchronously; the application joins retired workers outside locks.
    pub(crate) fn shutdown(&mut self) {
        self.playback.shutdown();
        self.pending_commands.clear();
        let connection = self.connection.take();
        self.session_configuration = None;
        self.invalidate_session_data();
        self.retire_replaced_connection(connection);
    }

    pub(crate) fn invalidate_session_data(&mut self) {
        self.session_tasks.close();
        self.retired_session_tasks.push(std::mem::replace(
            &mut self.session_tasks,
            Arc::new(SessionTasks::default()),
        ));
        self.session_data_events = None;
        self.initial_session_data_result = None;
        self.repositories = None;
        self.user_data = None;
        self.session_generation = self.session_generation.wrapping_add(1);
    }

    // Platform state reads project the active engine without storing duplicate core state.
    pub(crate) fn media_session_state(&self) -> MediaSessionState {
        self.playback.media_session_state()
    }

    // Pending work is extracted before releasing the application lock.
    pub(crate) fn take_pending_commands(
        &mut self,
    ) -> Result<Vec<crate::playback::backend::PlaybackCommand>, String> {
        let mut commands = std::mem::take(&mut self.pending_commands);
        commands.extend(self.playback.take_pending_commands()?);
        Ok(commands)
    }

    // Session-bound delivery and user-data state.
    pub(crate) fn session_events(&self) -> EventPublisher {
        self.session_tasks.events(self.event_publisher.clone())
    }

    pub(crate) fn update_initial_user_data(
        &mut self,
        session_generation: u64,
        update: impl FnOnce(&mut LibraryState),
    ) -> Result<(), String> {
        if self.session_generation != session_generation {
            return Err("session changed while loading user data".to_string());
        }
        if self.user_data.is_none() {
            let repositories = self
                .repositories
                .as_ref()
                .ok_or("session repositories must be initialized before publication")?;
            self.user_data = Some(LibraryState::new(repositories.library.clone()));
        }
        update(self.user_data.as_mut().expect("library state initialized"));
        Ok(())
    }

    pub(crate) fn user_data_snapshot(&self) -> Result<UserDataSnapshot, String> {
        self.user_data
            .as_ref()
            .map(|state| state.project_snapshot())
            .ok_or_else(|| "user data is not loaded".to_string())
    }

    // Transport ownership is transferred only after a replacement commits or rolls back.
    pub(crate) fn retire_replaced_connection(
        &mut self,
        previous: Option<Arc<crate::data::music_assistant::connection::MusicAssistantConnection>>,
    ) {
        let Some(previous) = previous else {
            return;
        };
        if self
            .connection
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &previous))
        {
            return;
        }
        let workers = previous
            .event_hub
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .shutdown();
        self.retired_workers.extend(
            workers
                .into_iter()
                .map(|worker| Arc::new(RetiredWorker::new(worker))),
        );
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::ApplicationState;
    use crate::application::session::SessionStorage;

    use crate::domain::session::{AccessToken, MusicAssistantServerUrl};
    use crate::domain::settings::{
        LogLevelPreference, SelectedOutput, SessionConfig, ThemePreference,
    };

    use std::sync::Mutex;

    struct TestSessionStorage {
        session: Mutex<Option<SessionConfig>>,
        should_fail_writes: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl TestSessionStorage {
        fn new() -> Self {
            Self {
                session: Mutex::new(None),
                should_fail_writes: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }
    }

    impl SessionStorage for TestSessionStorage {
        fn load_session(&self) -> Result<Option<SessionConfig>, String> {
            self.session
                .lock()
                .map(|session| session.clone())
                .map_err(|_| "test auth store lock poisoned".to_string())
        }

        fn save_session(&self, session: SessionConfig) -> Result<(), String> {
            if self
                .should_fail_writes
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("test storage unavailable".into());
            }
            self.session
                .lock()
                .map(|mut current| *current = Some(session))
                .map_err(|_| "test auth store lock poisoned".to_string())
        }

        fn clear_session(&self) -> Result<(), String> {
            if self
                .should_fail_writes
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("test storage unavailable".into());
            }
            self.session
                .lock()
                .map(|mut current| *current = None)
                .map_err(|_| "test auth store lock poisoned".to_string())
        }
    }

    pub(crate) fn test_coordinator() -> ApplicationState {
        ApplicationState::new(
            crate::application::tests::environment(),
            crate::application::tests::runtime().handle().clone(),
            crate::application::events::EventPublisher::new(|_| {}),
            Box::new(TestSessionStorage::new()),
        )
    }

    fn test_application() -> crate::application::Application {
        crate::application::Application::new(
            crate::application::tests::environment(),
            crate::application::tests::runtime().handle().clone(),
            crate::application::events::EventPublisher::new(|_| {}),
            Box::new(TestSessionStorage::new()),
        )
    }

    fn settings_update() -> crate::domain::settings::SessionSettingsUpdate {
        crate::domain::settings::SessionSettingsUpdate {
            theme: ThemePreference::Light,
            log_level: LogLevelPreference::Warn,
            should_show_multi_device_button: false,
        }
    }

    fn session_config() -> SessionConfig {
        SessionConfig {
            music_assistant_url: MusicAssistantServerUrl("https://music-assistant.example".into()),
            access_token: Some(AccessToken("secret-token".into())),
            user_id: None,
            username: "mashin".into(),
            selected_output: SelectedOutput::Local,
            local: crate::domain::settings::default_local_output("Mashin (Test)".into()),
            volume: 100,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
            should_show_multi_device_button: true,
            preferred_provider: None,
        }
    }
}
