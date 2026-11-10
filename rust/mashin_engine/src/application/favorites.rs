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
        events.publish(ApplicationEvent::UserDataChanged { playlist: None, revision: None });
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
                if !data.intents.has_active_worker(&identity) {
                    return;
                }
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
            events.publish(ApplicationEvent::UserDataChanged { playlist: None, revision: None });
            changes.notify_waiters();
            if let Err(error) = outcome {
                events.publish(ApplicationEvent::Error {
                    error: ApplicationError::command_failed(error),
                });
            } else if let Ok(confirmed) = outcome {
                if matches!(
                    &confirmed,
                    MediaItem::Artist(_)
                        | MediaItem::Album(_)
                        | MediaItem::Track(_)
                        | MediaItem::Playlist(_)
                ) {
                    let persistence = async {
                        self.catalog_repository(generation)
                            .map_err(|error| error.message)?
                            .commit_confirmed_media(confirmed)
                            .await
                    }
                    .await;
                    if let Err(error) = persistence {
                        events.publish(ApplicationEvent::Error {
                            error: ApplicationError::command_failed(format!(
                                "favorite cache persistence failed: {error}"
                            )),
                        });
                    }
                }
            }
            if finished {
                return;
            }
        }
    }
}

#[cfg(test)]
pub(in crate::application) mod tests {
    use super::*;
    use crate::application::repositories::CatalogClient;
    use crate::domain::catalog::{CatalogQuery, CatalogValue};
    use crate::domain::media::MediaType;
    use futures_util::future::BoxFuture;
    use serde_json::{json, Value};
    use std::sync::Mutex;

    pub(in crate::application) struct RecordingCatalog {
        pub(in crate::application) online: Arc<dyn CatalogClient>,
        pub(in crate::application) writes: Mutex<Vec<MediaItem>>,
        pub(in crate::application) entered: tokio::sync::Notify,
        pub(in crate::application) release: Option<Arc<tokio::sync::Notify>>,
        pub(in crate::application) should_fail_commit: bool,
    }

    impl CatalogClient for RecordingCatalog {
        fn commit_confirmed_playlist_contents(
            &self,
            playlist: crate::domain::media::Playlist,
            _tracks: Vec<crate::domain::media::Track>,
        ) -> BoxFuture<'_, Result<(), String>> {
            self.commit_confirmed_media(MediaItem::Playlist(playlist))
        }

        fn commit_confirmed_playlist_removal(
            &self,
            playlist: crate::domain::media::Playlist,
        ) -> BoxFuture<'_, Result<(), String>> {
            self.commit_confirmed_media(MediaItem::Playlist(playlist))
        }

        // Record the persistence boundary independently of online confirmation.
        fn commit_confirmed_media(&self, item: MediaItem) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.writes.lock().unwrap().push(item);
                self.entered.notify_one();
                if let Some(release) = &self.release {
                    release.notified().await;
                }
                if self.should_fail_commit {
                    Err("test cache unavailable".into())
                } else {
                    Ok(())
                }
            })
        }

        // Online capability forwarding does not create a second transport.
        fn begin_startup(&self) -> Box<dyn Send> {
            self.online.begin_startup()
        }
        fn load_catalog(
            &self,
            query: CatalogQuery,
            force_refresh: bool,
        ) -> BoxFuture<'_, Result<CatalogValue, String>> {
            self.online.load_catalog(query, force_refresh)
        }
    }

    // Use the real online mutation/readback client with an isolated application projection.
    fn application(
        client: Arc<crate::data::music_assistant::client::MusicAssistantClient>,
        catalog: Arc<RecordingCatalog>,
        events: Arc<Mutex<Vec<ApplicationEvent>>>,
        media_type: MediaType,
        is_favorite: bool,
    ) -> (Application, String) {
        let application = Application::new(
            crate::application::tests::environment(),
            tokio::runtime::Handle::current(),
            EventPublisher::new(move |event| events.lock().unwrap().push(event)),
            Box::new(crate::application::tests::EmptySessionStorage),
        );
        let mut services = crate::bootstrap::session::online_services(client);
        services.catalog = catalog;
        let mut data = crate::application::library::LibraryState::new(services.library.clone());
        let original: MediaItem = serde_json::from_value(json!({
            "media_type":media_type,"provider":"source-a","item_id":"remote","name":"Unconfirmed"
        }))
        .unwrap();
        if !is_favorite {
            data.snapshot.record_favorite_state(original.clone(), true);
        }
        let (identity, _, should_start) =
            data.intents
                .submit(&data.snapshot.favorites, original, is_favorite);
        assert!(should_start);
        let mut coordinator = application.coordinator.lock().unwrap();
        coordinator.repositories = Some(services);
        coordinator.user_data = Some(data);
        drop(coordinator);
        (application, identity)
    }

    // Server failures and incomplete or contradictory readbacks cannot reach the writer.
    #[tokio::test(flavor = "current_thread")]
    async fn favorite_worker_commits_only_successful_authoritative_readback() {
        use crate::data::music_assistant::client::tests::server;
        for media_type in [
            MediaType::Artist,
            MediaType::Album,
            MediaType::Track,
            MediaType::Playlist,
        ] {
            for is_favorite in [true, false] {
                let confirmed = json!({
                    "media_type":media_type,"provider":"library","item_id":"007","name":"Confirmed","favorite":if is_favorite { Some(true) } else { None },
                    "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote","in_library":true}]
                });
                let mut unknown = confirmed.clone();
                unknown.as_object_mut().unwrap().remove("favorite");
                let mut contradictory = confirmed.clone();
                contradictory["favorite"] = json!(!is_favorite);
                for (write_fails, readback, should_commit) in [
                    (true, Value::Null, false),
                    (false, json!({"test_http_status":503}), false),
                    (false, Value::Null, false),
                    (false, unknown, false),
                    (false, contradictory, false),
                    (false, confirmed.clone(), true),
                ] {
                    let mut responses = vec![(
                        "music/favorites/set_item",
                        if write_fails {
                            json!({"test_http_status":503})
                        } else {
                            Value::Null
                        },
                    )];
                    if !write_fails {
                        responses.push(("music/get_library_item", readback));
                    }
                    let (client, worker) = server(responses);
                    let client = Arc::new(client);
                    let catalog = Arc::new(RecordingCatalog {
                        online: client.clone(),
                        writes: Default::default(),
                        entered: Default::default(),
                        release: None,
                        should_fail_commit: false,
                    });
                    let events = Arc::new(Mutex::new(Vec::new()));
                    let (application, identity) = application(
                        client.clone(),
                        catalog.clone(),
                        events.clone(),
                        media_type.clone(),
                        is_favorite,
                    );
                    let publisher = application.coordinator.lock().unwrap().session_events();
                    application
                        .clone()
                        .run_favorite_worker(client, 0, identity, publisher)
                        .await;
                    assert_eq!(
                        catalog.writes.lock().unwrap().len(),
                        usize::from(should_commit)
                    );
                    let coordinator = application.coordinator.lock().unwrap();
                    let data = coordinator.user_data.as_ref().unwrap();
                    let favorites = data.snapshot.favorites.items();
                    assert_eq!(
                        favorites.len(),
                        usize::from(if should_commit {
                            is_favorite
                        } else {
                            !is_favorite
                        })
                    );
                    if should_commit {
                        let expected: MediaItem =
                            serde_json::from_value(confirmed.clone()).unwrap();
                        assert_eq!(catalog.writes.lock().unwrap()[0], expected);
                        assert!(data.snapshot.favorite_overrides.iter().any(|entry| entry
                            .item
                            .fields()
                            .name
                            == "Confirmed"
                            && entry.is_favorite == is_favorite));
                    }
                    assert_eq!(
                        events
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|event| matches!(event, ApplicationEvent::Error { .. })),
                        !should_commit
                    );
                    drop(coordinator);
                    let requests = worker.join().unwrap();
                    assert_eq!(requests.len(), if write_fails { 1 } else { 2 });
                    assert_eq!(requests[0]["command"], "music/favorites/set_item");
                    assert_eq!(
                        requests[0]["args"]["favorite"],
                        if is_favorite {
                            json!(true)
                        } else {
                            Value::Null
                        }
                    );
                    assert_eq!(
                        requests[0]["args"]["item"]["media_type"],
                        serde_json::to_value(&media_type).unwrap()
                    );
                }
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retired_playlist_favorite_worker_discards_late_success_and_failure() {
        use crate::data::music_assistant::client::tests::server_with_request_handler;
        let confirmed = json!({
            "media_type":"playlist","provider":"library","item_id":"007",
            "name":"Removed playlist","favorite":true,
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote"}]
        });
        let removed: MediaItem = serde_json::from_value(confirmed.clone()).unwrap();
        let mut contradictory = confirmed.clone();
        contradictory["favorite"] = Value::Null;
        for readback in [confirmed, json!({"test_http_status":503}), contradictory] {
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let signal = entered.clone();
            let barrier = release.clone();
            let (client, server_worker) = server_with_request_handler(
                vec![
                    ("music/favorites/set_item", Value::Null),
                    ("music/get_library_item", readback),
                ],
                move |request| {
                    if request["command"] == "music/get_library_item" {
                        signal.notify_one();
                        barrier.wait();
                    }
                },
            );
            let client = Arc::new(client);
            let catalog = Arc::new(RecordingCatalog {
                online: client.clone(),
                writes: Default::default(),
                entered: Default::default(),
                release: None,
                should_fail_commit: false,
            });
            let events = Arc::new(Mutex::new(Vec::new()));
            let (application, identity) = application(
                client.clone(),
                catalog.clone(),
                events.clone(),
                MediaType::Playlist,
                true,
            );
            let publisher = application.coordinator.lock().unwrap().session_events();
            let worker = tokio::spawn(application.clone().run_favorite_worker(
                client,
                0,
                identity.clone(),
                publisher,
            ));
            tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
                .await
                .unwrap();
            let expected = {
                let mut coordinator = application.coordinator.lock().unwrap();
                let data = coordinator.user_data.as_mut().unwrap();
                let MediaItem::Playlist(playlist) = &removed else {
                    unreachable!()
                };
                data.snapshot.record_playlist_removal(playlist);
                data.intents.retire_removed_item(&removed);
                data.favorite_changes.notify_waiters();
                assert!(!data.intents.has_active_worker(&identity));
                data.project_snapshot()
            };
            release.wait();
            worker.await.unwrap();
            assert_eq!(
                application
                    .coordinator
                    .lock()
                    .unwrap()
                    .user_data
                    .as_ref()
                    .unwrap()
                    .project_snapshot(),
                expected
            );
            assert!(catalog.writes.lock().unwrap().is_empty());
            assert!(events.lock().unwrap().is_empty());
            assert_eq!(server_worker.join().unwrap().len(), 2);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn favorite_worker_publishes_before_commit_and_retains_confirmation_on_cache_error() {
        use crate::data::music_assistant::client::tests::server;
        for media_type in [
            MediaType::Artist,
            MediaType::Album,
            MediaType::Track,
            MediaType::Playlist,
        ] {
            let (client, worker) = server(vec![
                ("music/favorites/set_item", Value::Null),
                (
                    "music/get_library_item",
                    json!({
                        "media_type":media_type,"provider":"library","item_id":"007","name":"Confirmed","favorite":true
                    }),
                ),
            ]);
            let client = Arc::new(client);
            let release = Arc::new(tokio::sync::Notify::new());
            let catalog = Arc::new(RecordingCatalog {
                online: client.clone(),
                writes: Default::default(),
                entered: Default::default(),
                release: Some(release.clone()),
                should_fail_commit: true,
            });
            let events = Arc::new(Mutex::new(Vec::new()));
            let (application, identity) = application(
                client.clone(),
                catalog.clone(),
                events.clone(),
                media_type,
                true,
            );
            let publisher = application.coordinator.lock().unwrap().session_events();
            let task = tokio::spawn(
                application
                    .clone()
                    .run_favorite_worker(client, 0, identity, publisher),
            );
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                catalog.entered.notified(),
            )
            .await
            .unwrap();
            {
                let coordinator = application.coordinator.lock().unwrap();
                let data = coordinator.user_data.as_ref().unwrap();
                assert_eq!(
                    data.snapshot.favorites.items()[0].fields().name,
                    "Confirmed"
                );
                assert_eq!(data.project_snapshot(), data.snapshot);
            }
            release.notify_one();
            task.await.unwrap();
            assert_eq!(
                application
                    .coordinator
                    .lock()
                    .unwrap()
                    .user_data
                    .as_ref()
                    .unwrap()
                    .snapshot
                    .favorites
                    .items()[0]
                    .fields()
                    .name,
                "Confirmed"
            );
            assert!(events.lock().unwrap().iter().any(|event| matches!(event, ApplicationEvent::Error { error } if error.message.contains("favorite cache persistence failed"))));
            assert_eq!(catalog.writes.lock().unwrap().len(), 1);
            worker.join().unwrap();
        }
    }
}
