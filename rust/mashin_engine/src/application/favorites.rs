//! Session-owned favorite intentions; successful server readback alone confirms visible state.
use super::errors::ApplicationError;
use super::events::{ApplicationEvent, EventPublisher};
use super::repositories::LibraryClient;
use super::Application;
use crate::domain::favorites::FavoriteWork;
use crate::domain::library::UserDataSnapshot;
use crate::domain::media::MediaItem;
use std::sync::Arc;
impl Application {
    // User wishes remain distinct from server confirmation.
    pub(crate) fn set_user_data_favorites(
        &self,
        items: Vec<MediaItem>,
        is_favorite: bool,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        if items.iter().any(|item| {
            !matches!(
                item,
                MediaItem::Track(_)
                    | MediaItem::Album(_)
                    | MediaItem::Artist(_)
                    | MediaItem::Playlist(_)
            )
        }) {
            return Err(ApplicationError::command_failed(
                "unsupported favorite media type",
            ));
        }
        let (client, generation) = self.library_repository()?;
        let (workers, tasks, events) = {
            let mut coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?;
            if coordinator.session_generation != generation || coordinator.connection.is_none() {
                return Err(ApplicationError::command_failed(
                    "session changed before favorite command",
                ));
            }
            let data = coordinator
                .user_data
                .as_mut()
                .ok_or_else(|| ApplicationError::internal("library not initialized"))?;
            let workers = items
                .into_iter()
                .filter_map(|item| {
                    let (identity, _, should_start) =
                        data.intents
                            .submit(&data.snapshot.favorites, item, is_favorite);
                    should_start.then_some(identity)
                })
                .collect::<Vec<_>>();
            (
                workers,
                coordinator.session_tasks.clone(),
                coordinator.session_events(),
            )
        };
        for identity in workers {
            if let Err(error) = tasks.spawn(
                &self.runtime,
                self.clone().run_favorite_worker(
                    client.clone(),
                    generation,
                    identity.clone(),
                    events.clone(),
                ),
            ) {
                self.publish_initial_user_data(generation, |data| {
                    data.intents.mark_uncertain(&identity)
                })
                .map_err(ApplicationError::command_failed)?;
                return Err(error);
            }
        }
        events.publish(ApplicationEvent::UserDataChanged);
        self.read_user_data_sync()
    }
    // Mutation success and authoritative rating must both hold before publication.
    async fn run_favorite_worker(
        self,
        client: Arc<dyn LibraryClient>,
        generation: u64,
        identity: String,
        events: EventPublisher,
    ) {
        let changes = {
            let Ok(coordinator) = self.coordinator.lock() else {
                return;
            };
            if coordinator.session_generation != generation {
                return;
            }
            let Some(data) = coordinator.user_data.as_ref() else {
                return;
            };
            data.favorite_changes.clone()
        };
        loop {
            let notified = changes.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let work = {
                let Ok(mut coordinator) = self.coordinator.lock() else {
                    return;
                };
                if coordinator.session_generation != generation {
                    return;
                }
                let Some(data) = coordinator.user_data.as_mut() else {
                    return;
                };
                data.intents.begin_work(&identity)
            };
            let (item, is_favorite) = match work {
                FavoriteWork::Write { item, is_favorite } => (item, is_favorite),
                FavoriteWork::WaitForAliases => {
                    notified.await;
                    continue;
                }
                FavoriteWork::Finished => return,
            };
            let outcome = async {
                client.set_favorite(&item, is_favorite).await?;
                let confirmed = client
                    .resolve_library_item(&item)
                    .await?
                    .ok_or("authoritative favorite rating unavailable")?;
                if !confirmed
                    .fields()
                    .favorite
                    .known_value()
                    .is_some_and(|rating| (*rating == Some(true)) == is_favorite)
                {
                    return Err("favorite readback contradicts requested state".to_owned());
                }
                Ok::<_, String>(confirmed)
            }
            .await;
            let finished = {
                let Ok(mut coordinator) = self.coordinator.lock() else {
                    return;
                };
                if coordinator.session_generation != generation {
                    return;
                }
                let Some(data) = coordinator.user_data.as_mut() else {
                    return;
                };
                match &outcome {
                    Ok(confirmed) => {
                        data.snapshot
                            .record_favorite_state(confirmed.clone(), is_favorite);
                        data.intents.finish(&identity, is_favorite, Some(confirmed))
                    }
                    Err(_) => {
                        data.intents.mark_uncertain(&identity);
                        true
                    }
                }
            };
            events.publish(ApplicationEvent::UserDataChanged);
            changes.notify_waiters();
            if let Err(error) = outcome {
                events.publish(ApplicationEvent::Error {
                    error: ApplicationError::command_failed(error),
                });
            }
            if finished {
                return;
            }
        }
    }
}
