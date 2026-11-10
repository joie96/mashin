//! Owns shared session startup, event routing, and the lifetime of independent data workers.
//! Session replacement aborts workers; network operations never hold the coordinator lock.

use crate::application::errors::ApplicationError;
use crate::application::events::ApplicationEvent;
use crate::application::repositories::{with_request_priority, RequestPriority};
use crate::application::startup::{load_initial_session_data, InitialSessionDataResult};
use crate::application::Application;
use crate::data::music_assistant::event_hub::{EventHubLeaseReason, MusicAssistantEventHub};
use crate::logging::Logger;
use crate::playback::server::ServerEvent;
use std::sync::Arc;
use std::sync::Mutex;

static LOGGER: Logger = Logger::new(module_path!());

pub(super) struct SessionDataEvents {
    hub: Arc<Mutex<MusicAssistantEventHub>>,
    cancel_startup: tokio::sync::watch::Sender<bool>,
}

impl Drop for SessionDataEvents {
    fn drop(&mut self) {
        self.cancel_startup.send_replace(true);
        if let Ok(mut hub) = self.hub.lock() {
            hub.release(EventHubLeaseReason::SessionData);
        }
    }
}

impl Application {
    // Session startup reports both online components without retaining their result inventories.
    pub(crate) fn read_or_load_session_data(
        &self,
        expected_generation: u64,
    ) -> Result<(), ApplicationError> {
        let context = self.query_context()?;
        if context.generation != expected_generation {
            return Err(ApplicationError::command_failed(
                "session changed before loading session data",
            ));
        }
        let Some(_session_guard) = context.tasks.try_lock_session_data_load() else {
            return Ok(());
        };
        {
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            if coordinator.session_generation != expected_generation {
                return Err(ApplicationError::command_failed(
                    "session changed before loading session data",
                ));
            }
            if matches!(coordinator.initial_session_data_result, Some(Ok(()))) {
                return Ok(());
            }
        }
        let (repositories, generation) = self.session_repositories()?;
        if generation != expected_generation {
            return Err(ApplicationError::command_failed(
                "session changed before loading session data",
            ));
        }
        self.begin_initial_user_data(generation)
            .map_err(ApplicationError::command_failed)?;
        self.start_session_data_events(generation)?;
        let mut cancel_startup = {
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            if coordinator.session_generation != generation {
                return Err(ApplicationError::command_failed(
                    "session changed before session startup",
                ));
            }
            let events = coordinator
                .session_data_events
                .as_ref()
                .ok_or_else(|| ApplicationError::command_failed("session ended during startup"))?;
            events.cancel_startup.subscribe()
        };
        let publisher = crate::application::library::ApplicationUserDataPublisher::new(
            self.clone(),
            generation,
        )
        .map_err(ApplicationError::command_failed)?;
        let recommendations = self.recommendation_service(generation)?;
        let result = self.runtime.block_on(async {
            tokio::select! {
                biased;
                _ = async {
                    if !*cancel_startup.borrow() { let _ = cancel_startup.changed().await; }
                } => InitialSessionDataResult {
                    recommendation_data: Err("session ended during startup".into()),
                    user_data: Err("session ended during startup".into()),
                },
                result = load_initial_session_data(repositories.library.as_ref(), repositories.catalog.as_ref(), &recommendations, || self.publish_recommendations_changed(generation), publisher) => result,
            }
        });
        self.complete_initial_user_data(generation, result.user_data.clone())
            .map_err(ApplicationError::command_failed)?;
        let result = result.into_result();
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        if coordinator.session_generation != generation {
            return Err(ApplicationError::command_failed(
                "session changed while loading session data",
            ));
        }
        coordinator.initial_session_data_result = Some(result.clone());
        let events = coordinator.session_events();
        drop(coordinator);
        let providers = repositories.providers.clone();
        context.tasks.spawn(&self.runtime, async move {
            let refreshed = crate::application::repositories::with_request_priority(
                crate::application::repositories::RequestPriority::Background,
                async { providers.read_instances().await?.resolve().await },
            )
            .await;
            if refreshed.is_ok() {
                events.publish(ApplicationEvent::UserDataChanged { playlist: None, revision: None });
            }
        })?;
        result.map_err(ApplicationError::command_failed)
    }

    // Transport lookup is shared by independent data components, not owned by either one.
    pub(crate) fn session_repositories(
        &self,
    ) -> Result<(crate::application::repositories::SessionServices, u64), ApplicationError> {
        let expected_generation = self.query_context()?.generation;
        self.ensure_session_repositories_sync(expected_generation)?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        coordinator
            .connection
            .as_ref()
            .ok_or_else(|| ApplicationError::command_failed("a restored session is required"))?;
        let generation = coordinator.session_generation;
        coordinator
            .update_initial_user_data(generation, |_| {})
            .map_err(ApplicationError::command_failed)?;
        let repositories = coordinator
            .repositories
            .as_ref()
            .ok_or_else(|| {
                ApplicationError::command_failed("session repositories are unavailable")
            })?
            .clone();
        Ok((repositories, coordinator.session_generation))
    }

    // One session owns both workers and the shared live-event subscription.
    fn start_session_data_events(&self, expected_generation: u64) -> Result<(), ApplicationError> {
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        if coordinator.session_generation != expected_generation {
            return Err(ApplicationError::command_failed(
                "session changed before starting data workers",
            ));
        }
        if coordinator.session_data_events.is_some() {
            return Ok(());
        }
        let connection = coordinator
            .connection
            .as_ref()
            .ok_or_else(|| {
                ApplicationError::command_failed("session ended while starting session data events")
            })?
            .clone();
        let generation = coordinator.session_generation;
        let hub = connection.event_hub.clone();
        let _runtime_guard = self.runtime.enter();
        let mut receiver = {
            let mut hub = hub
                .lock()
                .map_err(|_| ApplicationError::internal("event hub lock poisoned"))?;
            let receiver = hub.subscribe();
            hub.acquire(EventHubLeaseReason::SessionData);
            receiver
        };
        let session_events = SessionDataEvents {
            hub: hub.clone(),
            cancel_startup: tokio::sync::watch::channel(false).0,
        };
        let event_publisher = coordinator.session_events();
        let application = self.clone();
        coordinator.session_tasks.spawn(&self.runtime, async move {
            loop {
                let refresh = match receiver.recv().await {
                    Ok(ServerEvent::Connected)
                    | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        event_publisher.publish(ApplicationEvent::RecommendationsChanged);
                        crate::application::library::UserDataRefresh::All
                    }
                    Ok(ServerEvent::PlaylistsChanged) => {
                        crate::application::library::UserDataRefresh::Playlists
                    }
                    Ok(ServerEvent::PlaylistChanged(playlist_id)) => {
                        crate::application::library::UserDataRefresh::Playlist(playlist_id.0)
                    }
                    Ok(ServerEvent::FavoritesChanged) => {
                        crate::application::library::UserDataRefresh::Favorites
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    _ => continue,
                };
                if let Err(error) = with_request_priority(
                    RequestPriority::Background,
                    application.refresh_user_data_for_session(Some(generation), refresh),
                )
                .await
                {
                    event_publisher.publish(ApplicationEvent::Error { error });
                }
            }
        })?;
        coordinator.session_data_events = Some(session_events);
        Ok(())
    }
}
