//! Instance-bound application access shared by native adapters and session workers.
//! Clones share one coordinator and command lock; only bootstrap owns the executor.

use crate::application::state::ApplicationState;
use events::EventPublisher;
use session::SessionStorage;
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;

pub(crate) mod commands;
pub(crate) mod dispatcher;
pub(crate) mod errors;
pub(crate) mod events;
pub(crate) mod favorites;
pub(crate) mod library;
mod lifecycle;
pub(crate) mod playback;
pub(crate) mod playlist_artwork;
pub(crate) mod queries;
mod query_tasks;
pub(crate) mod radio;
pub(crate) mod recommendations;
pub(crate) mod repositories;
pub(crate) mod session;
mod session_data;
pub(crate) mod settings;
pub(crate) mod snapshots;
pub(crate) mod startup;
mod state;

#[derive(Clone)]
pub(crate) struct Application {
    pub(crate) runtime: Handle,
    pub(crate) coordinator: Arc<Mutex<ApplicationState>>,
    pub(crate) command_serializer: Arc<Mutex<()>>,
    pub(crate) environment: session::SessionEnvironment,
}

impl Application {
    // Clones share state and serialization, but never own the process executor.
    pub(crate) fn new(
        environment: session::SessionEnvironment,
        runtime: Handle,
        event_publisher: EventPublisher,
        session_storage: Box<dyn SessionStorage>,
    ) -> Self {
        Self {
            coordinator: Arc::new(Mutex::new(ApplicationState::new(
                environment.clone(),
                runtime.clone(),
                event_publisher,
                session_storage,
            ))),
            runtime,
            command_serializer: Arc::new(Mutex::new(())),
            environment,
        }
    }

    // Blocking native commands run off the UI thread on this application's executor.
    pub(crate) fn spawn_blocking_operation<F>(&self, operation: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.runtime.spawn_blocking(operation);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::domain::settings::SessionConfig;

    pub(crate) struct EmptySessionStorage;

    pub(crate) fn environment() -> session::SessionEnvironment {
        #[derive(Default)]
        struct TestAppSettings(Mutex<Option<crate::domain::settings::UpdateChannel>>);
        impl settings::AppSettingsStorage for TestAppSettings {
            fn get_update_channel(
                &self,
                default: crate::domain::settings::UpdateChannel,
            ) -> Result<crate::domain::settings::UpdateChannel, String> {
                Ok(*self.0.lock().unwrap().get_or_insert(default))
            }
            fn save_update_channel(
                &self,
                channel: crate::domain::settings::UpdateChannel,
            ) -> Result<(), String> {
                *self.0.lock().unwrap() = Some(channel);
                Ok(())
            }
        }
        session::SessionEnvironment {
            authenticate: |login| {
                Box::pin(crate::data::music_assistant::session::authenticate(login))
            },
            client_name: || "Mashin (Test)".into(),
            connect: crate::data::music_assistant::connection::MusicAssistantConnection::new,
            app_settings: Arc::new(TestAppSettings::default()),
            repositories: |client, _server, _user| {
                Box::pin(async move { Ok(crate::bootstrap::session::online_services(client)) })
            },
        }
    }

    pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
        static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
        })
    }

    impl SessionStorage for EmptySessionStorage {
        fn load_session(&self) -> Result<Option<SessionConfig>, String> {
            Ok(None)
        }
        fn save_session(&self, _: SessionConfig) -> Result<(), String> {
            Err("test storage is read-only".into())
        }
        fn clear_session(&self) -> Result<(), String> {
            Ok(())
        }
    }
}
