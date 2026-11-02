//! Owns shared session startup, event routing, and the lifetime of independent data workers.
//! Session replacement aborts workers; network operations never hold the coordinator lock.

use super::*;
use crate::music_assistant::event_hub::{
    EventHubLeaseReason, MusicAssistantEvent, MusicAssistantEventHub,
};
use crate::music_assistant::session_data::InitialSessionDataUpdate;
use std::sync::Arc;

static CORE_SESSION_DATA_LOAD: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

pub(super) struct SessionDataEvents {
    hub: Arc<Mutex<MusicAssistantEventHub>>,
    events_worker: tokio::task::JoinHandle<()>,
    recommendation_data_worker: tokio::task::JoinHandle<()>,
}

impl Drop for SessionDataEvents {
    fn drop(&mut self) {
        self.events_worker.abort();
        self.recommendation_data_worker.abort();
        if let Ok(mut hub) = self.hub.lock() {
            hub.release(EventHubLeaseReason::SessionData);
        }
    }
}

// The session result is independent of each component's cached loading result.
pub(crate) fn read_or_load_session_data() -> Result<(), CoreError> {
    let _session_guard = CORE_NETWORK_RUNTIME.block_on(CORE_SESSION_DATA_LOAD.lock());
    {
        let coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        if let Some(result) = &coordinator.initial_session_data_result {
            return result.clone().map_err(CoreError::command_failed);
        }
    }
    let _user_data_guard = CORE_NETWORK_RUNTIME.block_on(CORE_USER_DATA_LOAD.lock());
    let (client, generation) = session_client()?;
    user_data::begin_initial_user_data(generation).map_err(CoreError::command_failed)?;
    start_session_data_events()?;
    let result =
        CORE_NETWORK_RUNTIME.block_on(client.load_initial_session_data(|update| match update {
            InitialSessionDataUpdate::RecommendationData => {
                publish_background_event(MediaSessionEvent::RecommendationsChanged);
                Ok(())
            }
            InitialSessionDataUpdate::UserData(update) => {
                user_data::apply_initial_user_data_update(generation, update)
            }
        }));
    user_data::complete_initial_user_data(generation, result.user_data.clone())
        .map_err(CoreError::command_failed)?;
    let result = result.into_result();
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    if coordinator.session_generation != generation {
        return Err(CoreError::command_failed(
            "session changed while loading session data",
        ));
    }
    coordinator.initial_session_data_result = Some(result.clone());
    result.map_err(CoreError::command_failed)
}

// Transport lookup is shared by independent data components, not owned by either one.
pub(super) fn session_client() -> Result<(Arc<MusicAssistantClient>, u64), CoreError> {
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let client = coordinator
        .connection
        .as_ref()
        .ok_or_else(|| CoreError::command_failed("a restored session is required"))?
        .client
        .clone();
    Ok((client, coordinator.session_generation))
}

// One session owns both workers and the shared live-event subscription.
fn start_session_data_events() -> Result<(), CoreError> {
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    if coordinator.session_data_events.is_some() {
        return Ok(());
    }
    let connection = coordinator
        .connection
        .as_ref()
        .ok_or_else(|| {
            CoreError::command_failed("session ended while starting session data events")
        })?
        .clone();
    let generation = coordinator.session_generation;
    let hub = connection.event_hub.clone();
    let _runtime_guard = CORE_NETWORK_RUNTIME.enter();
    let mut receiver = {
        let mut hub = hub
            .lock()
            .map_err(|_| CoreError::internal("event hub lock poisoned"))?;
        let receiver = hub.subscribe();
        hub.acquire(EventHubLeaseReason::SessionData);
        receiver
    };
    let client = connection.client.clone();
    let recommendation_data_client = client.clone();
    let recommendation_data_worker = CORE_NETWORK_RUNTIME.spawn(async move {
        recommendation_data_client
            .run_recommendation_data_retries(|| {
                publish_background_event(MediaSessionEvent::RecommendationsChanged)
            })
            .await;
    });
    let events_worker = CORE_NETWORK_RUNTIME.spawn(async move {
        loop {
            let refresh = match receiver.recv().await {
                Ok(MusicAssistantEvent::Connected)
                | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    client.retry_recommendation_data_after_reconnect().await;
                    user_data::UserDataRefresh::All
                }
                Ok(MusicAssistantEvent::PlaylistsChanged) => user_data::UserDataRefresh::Playlists,
                Ok(MusicAssistantEvent::PlaylistChanged(playlist_id)) => {
                    user_data::UserDataRefresh::Playlist(playlist_id.0)
                }
                Ok(MusicAssistantEvent::FavoritesChanged) => user_data::UserDataRefresh::Favorites,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                _ => continue,
            };
            if let Err(error) =
                user_data::refresh_user_data_for_session(Some(generation), refresh).await
            {
                crate::ffi::errors::record_and_publish_error(error);
            }
        }
    });
    coordinator.session_data_events = Some(SessionDataEvents {
        hub,
        events_worker,
        recommendation_data_worker,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_assistant::models::identifiers::AccessToken;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;

    #[test]
    fn session_invalidation_clears_common_and_component_results() {
        let mut coordinator = CoreCoordinator::new();
        coordinator.initial_session_data_result = Some(Err("recommendations unavailable".into()));
        let mut user_data = LibraryUserData::default();
        user_data.initial_load_result = Some(Ok(()));
        coordinator.user_data = Some(user_data);
        let generation = coordinator.session_generation;
        coordinator.invalidate_session_data();
        assert!(coordinator.initial_session_data_result.is_none());
        assert!(coordinator.user_data.is_none());
        assert_ne!(coordinator.session_generation, generation);
    }

    #[tokio::test]
    async fn session_drop_aborts_independent_data_workers() {
        let hub = MusicAssistantEventHub::new(
            &MusicAssistantServerUrl("http://127.0.0.1:1".into()),
            Some(&AccessToken("test-token".into())),
        )
        .unwrap();
        let events_worker = tokio::spawn(std::future::pending::<()>());
        let recommendation_data_worker = tokio::spawn(std::future::pending::<()>());
        let events_abort = events_worker.abort_handle();
        let recommendations_abort = recommendation_data_worker.abort_handle();
        drop(SessionDataEvents {
            hub: Arc::new(Mutex::new(hub)),
            events_worker,
            recommendation_data_worker,
        });
        tokio::task::yield_now().await;
        assert!(events_abort.is_finished());
        assert!(recommendations_abort.is_finished());
    }
}
