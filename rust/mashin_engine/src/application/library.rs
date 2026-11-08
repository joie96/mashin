//! One UI session projection of server-confirmed favorites and playlists, never a media inventory.
use super::errors::ApplicationError;
use super::events::{ApplicationEvent, EventPublisher};
use super::repositories::{InitialUserDataUpdate, LibraryClient, UserDataPublisher};
use super::Application;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::favorites::FavoriteIntents;
use crate::domain::identifiers::MediaId;
use crate::domain::library::UserDataSnapshot;
use crate::domain::media::{MediaItem, Playlist, Track};
use std::sync::Arc;
pub(crate) struct LibraryState {
    pub(crate) repository: Arc<dyn LibraryClient>,
    pub(crate) snapshot: UserDataSnapshot,
    pub(crate) initial_load_result: Option<Result<(), String>>,
    pub(crate) intents: FavoriteIntents,
    pub(crate) favorite_changes: Arc<tokio::sync::Notify>,
}
impl LibraryState {
    // Construction.
    pub(crate) fn new(repository: Arc<dyn LibraryClient>) -> Self {
        Self {
            repository,
            snapshot: Default::default(),
            initial_load_result: None,
            intents: Default::default(),
            favorite_changes: Default::default(),
        }
    }
    // Pending intents affect only the detached UI snapshot.
    pub(crate) fn project_snapshot(&self) -> UserDataSnapshot {
        self.intents.project(&self.snapshot)
    }
}
pub(crate) struct ApplicationUserDataPublisher {
    application: Application,
    generation: u64,
}
impl ApplicationUserDataPublisher {
    // Publication is bound to the session that requested the data.
    pub(crate) fn new(application: Application, generation: u64) -> Result<Self, String> {
        if application
            .query_context()
            .map_err(|error| error.message)?
            .generation
            != generation
        {
            return Err("session changed before library request".into());
        }
        Ok(Self {
            application,
            generation,
        })
    }
}
impl UserDataPublisher for ApplicationUserDataPublisher {
    // Reject stale sessions at the application publication boundary.
    fn publish(
        &mut self,
        update: InitialUserDataUpdate,
    ) -> futures_util::future::BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.application
                .apply_initial_user_data_update(self.generation, update)
        })
    }
}
pub(crate) enum UserDataRefresh {
    All,
    Favorites,
    Playlists,
    Playlist(String),
}
impl Application {
    // Playlist commands publish only server-confirmed changes.
    pub(crate) fn add_playlist_tracks(
        &self,
        item_id: String,
        uris: Vec<String>,
    ) -> Result<(), ApplicationError> {
        let (client, generation) = self.library_repository()?;
        self.runtime
            .block_on(client.add_playlist_tracks(&item_id, uris))
            .map_err(ApplicationError::command_failed)?;
        self.runtime.block_on(
            self.refresh_user_data_for_session(
                Some(generation),
                UserDataRefresh::Playlist(item_id),
            ),
        )?;
        Ok(())
    }
    pub(crate) fn remove_playlist_tracks(
        &self,
        item_id: String,
        positions: Vec<u32>,
    ) -> Result<(), ApplicationError> {
        let (client, generation) = self.library_repository()?;
        self.runtime
            .block_on(client.remove_playlist_tracks(&item_id, positions))
            .map_err(ApplicationError::command_failed)?;
        self.runtime.block_on(
            self.refresh_user_data_for_session(
                Some(generation),
                UserDataRefresh::Playlist(item_id),
            ),
        )?;
        Ok(())
    }
    pub(crate) fn create_private_playlist(
        &self,
        name: String,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        self.runtime
            .block_on(self.create_private_playlist_for_session(&name))
    }
    pub(crate) fn rename_private_playlist(
        &self,
        item_id: &str,
        name: String,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        self.runtime
            .block_on(self.rename_private_playlist_for_session(item_id, &name))
    }

    pub(crate) fn remove_private_playlist(
        &self,
        item_id: &str,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        self.runtime
            .block_on(self.remove_private_playlist_for_session(item_id))
    }
    pub(crate) fn refresh_private_playlist_cover(
        &self,
        item_id: &str,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let (client, generation) = self.library_repository()?;
        let (services, _) = self.session_repositories()?;
        self.runtime
            .block_on(async {
                let user_id = self
                    .current_session_user_id(client.as_ref(), generation)
                    .await?;
                let mut playlist = client
                    .load_private_playlist(item_id, user_id.as_str())
                    .await?
                    .ok_or("private playlist unavailable")?;
                let CatalogValue::Tracks(items) = services
                    .catalog
                    .load_catalog(
                        CatalogQuery::PlaylistTracks {
                            item_id: MediaId(item_id.into()),
                            provider: "library".into(),
                            allow_dynamic_tracks: false,
                        },
                        true,
                    )
                    .await?
                else {
                    return Err("playlist tracks unavailable".to_owned());
                };
                playlist.items = items;
                let base_url = self
                    .coordinator
                    .lock()
                    .map_err(|_| "coordinator lock poisoned")?
                    .connection
                    .as_ref()
                    .ok_or("session ended")?
                    .client
                    .base_url()
                    .to_owned();
                let artwork = super::playlist_artwork::PlaylistArtwork::new(
                    client,
                    services.images,
                    base_url,
                    self.query_context().map_err(|error| error.message)?.tasks,
                    self.runtime.clone(),
                );
                artwork
                    .refresh_private_playlist_cover(&mut playlist)
                    .await?;
                self.publish_initial_user_data(generation, |data| {
                    data.snapshot
                        .playlists
                        .retain(|existing| existing.playlist.media.item_id.as_str() != item_id);
                    data.snapshot.playlists.push(playlist);
                })
            })
            .map_err(ApplicationError::command_failed)?;
        self.read_user_data_sync()
    }
    pub(crate) fn add_private_playlist_tracks(
        &self,
        item_id: &str,
        tracks: Vec<Track>,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let (client, generation) = self.library_repository()?;
        self.runtime
            .block_on(async {
                client.get_private_playlist(item_id).await?;
                let uris = tracks
                    .iter()
                    .map(|track| {
                        track
                            .media
                            .uri
                            .as_ref()
                            .map(|uri| uri.as_str().to_owned())
                            .ok_or("playlist track URI required".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                client.add_playlist_tracks(item_id, uris).await
            })
            .map_err(ApplicationError::command_failed)?;
        self.runtime.block_on(self.refresh_user_data_for_session(
            Some(generation),
            UserDataRefresh::Playlist(item_id.into()),
        ))
    }
    pub(crate) fn remove_private_playlist_tracks(
        &self,
        item_id: &str,
        uris: Vec<String>,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let (client, generation) = self.library_repository()?;
        let catalog = self.catalog_repository(generation)?;
        self.runtime
            .block_on(async {
                client.get_private_playlist(item_id).await?;
                let CatalogValue::Tracks(tracks) = catalog
                    .load_catalog(
                        CatalogQuery::PlaylistTracks {
                            item_id: MediaId(item_id.into()),
                            provider: "library".into(),
                            allow_dynamic_tracks: false,
                        },
                        true,
                    )
                    .await?
                else {
                    return Err("playlist tracks unavailable".to_owned());
                };
                let positions = tracks
                    .iter()
                    .filter(|track| {
                        track
                            .media
                            .uri
                            .as_ref()
                            .is_some_and(|uri| uris.iter().any(|target| target == uri.as_str()))
                    })
                    .map(|track| {
                        track
                            .media
                            .position
                            .ok_or("MA did not return playlist position".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                client.remove_playlist_tracks(item_id, positions).await
            })
            .map_err(ApplicationError::command_failed)?;
        self.runtime.block_on(self.refresh_user_data_for_session(
            Some(generation),
            UserDataRefresh::Playlist(item_id.into()),
        ))
    }
    // Online queries and UI snapshots.
    pub(crate) fn read_user_data_sync(&self) -> Result<UserDataSnapshot, ApplicationError> {
        self.library_repository()?;
        self.runtime.block_on(self.read_user_data())
    }
    pub(crate) async fn read_user_data(&self) -> Result<UserDataSnapshot, ApplicationError> {
        self.coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?
            .user_data_snapshot()
            .map_err(ApplicationError::command_failed)
    }
    pub(crate) fn read_or_load_user_data(
        &self,
        generation: u64,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let context = self.query_context()?;
        if context.generation != generation {
            return Err(ApplicationError::command_failed(
                "session changed before user data load",
            ));
        }
        let _load = self.runtime.block_on(context.tasks.lock_user_data_load());
        let (client, current) = self.library_repository()?;
        if current != generation {
            return Err(ApplicationError::command_failed(
                "session changed before user data load",
            ));
        }
        self.begin_initial_user_data(generation)
            .map_err(ApplicationError::command_failed)?;
        let publisher = ApplicationUserDataPublisher::new(self.clone(), generation)
            .map_err(ApplicationError::command_failed)?;
        let catalog = self.catalog_repository(generation)?;
        let result = self
            .runtime
            .block_on(super::startup::load_initial_user_data(
                client.as_ref(),
                catalog.as_ref(),
                publisher,
            ));
        self.complete_initial_user_data(generation, result.clone())
            .map_err(ApplicationError::command_failed)?;
        result.map_err(ApplicationError::command_failed)?;
        self.read_user_data_sync()
    }
    pub(crate) fn refresh_user_data(&self) -> Result<UserDataSnapshot, ApplicationError> {
        self.runtime
            .block_on(self.refresh_user_data_for_session(None, UserDataRefresh::All))
    }
    pub(crate) async fn refresh_user_data_for_session(
        &self,
        expected: Option<u64>,
        refresh: UserDataRefresh,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let context = self.query_context()?;
        if expected.is_some_and(|generation| generation != context.generation) {
            return Err(ApplicationError::command_failed(
                "session changed before refresh",
            ));
        }
        let _load = context.tasks.lock_user_data_load().await;
        let (client, generation) = self.library_repository()?;
        if generation != context.generation {
            return Err(ApplicationError::command_failed(
                "session changed before refresh",
            ));
        }
        let intent_revision = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?
            .user_data
            .as_ref()
            .map(|data| data.intents.revision())
            .unwrap_or_default();
        match refresh {
            UserDataRefresh::All => {
                let user_id = self
                    .current_session_user_id(client.as_ref(), generation)
                    .await?;
                let catalog = self.catalog_repository(generation)?;
                let mut snapshot = client
                    .load_library_user_data(catalog.as_ref(), user_id.as_str())
                    .await?;
                self.publish_initial_user_data(generation, |data| {
                    data.snapshot
                        .apply_playlist_metadata(std::mem::take(&mut snapshot.playlists), true);
                    snapshot.playlists = std::mem::take(&mut data.snapshot.playlists);
                    data.snapshot = snapshot;
                    data.intents.reconcile_complete_read(intent_revision);
                })?;
            }
            UserDataRefresh::Favorites => {
                let catalog = self.catalog_repository(generation)?;
                let favorites = client.load_favorites(catalog.as_ref()).await?;
                self.publish_initial_user_data(generation, |data| {
                    data.snapshot.favorites = favorites;
                    data.snapshot.has_loaded_favorites = true;
                    data.snapshot.favorite_overrides.clear();
                    data.intents.reconcile_complete_read(intent_revision);
                })?;
            }
            UserDataRefresh::Playlists => {
                let user_id = self
                    .current_session_user_id(client.as_ref(), generation)
                    .await?;
                let catalog = self.catalog_repository(generation)?;
                let mut publisher =
                    |update| self.apply_initial_user_data_update(generation, update);
                let playlists = client
                    .load_private_playlists(
                        catalog.as_ref(),
                        user_id.as_str(),
                        true,
                        &mut publisher,
                    )
                    .await?;
                self.publish_initial_user_data(generation, |data| {
                    data.snapshot.apply_playlist_metadata(playlists, true)
                })?;
            }
            UserDataRefresh::Playlist(item_id) => {
                let user_id = self
                    .current_session_user_id(client.as_ref(), generation)
                    .await?;
                let mut playlist = client
                    .load_private_playlist(&item_id, user_id.as_str())
                    .await?;
                if let Some(playlist) = playlist.as_mut() {
                    let catalog = self.catalog_repository(generation)?;
                    let CatalogValue::Tracks(items) = catalog
                        .load_catalog(
                            CatalogQuery::PlaylistTracks {
                                item_id: MediaId(item_id.clone()),
                                provider: "library".into(),
                                allow_dynamic_tracks: false,
                            },
                            true,
                        )
                        .await?
                    else {
                        return Err(ApplicationError::command_failed(
                            "playlist tracks unavailable",
                        ));
                    };
                    playlist.items = items;
                    playlist.is_loading_tracks = false;
                }
                self.publish_initial_user_data(generation, |data| {
                    data.snapshot
                        .playlists
                        .retain(|existing| existing.playlist.media.item_id.as_str() != item_id);
                    if let Some(playlist) = playlist {
                        data.snapshot.playlists.push(playlist);
                    }
                })?;
            }
        }
        self.schedule_playlist_hydration(generation)?;
        self.read_user_data().await
    }
    // Only the initiating session may update its UI projection.
    pub(crate) fn begin_initial_user_data(&self, generation: u64) -> Result<(), String> {
        self.publish_initial_user_data(generation, |data| {
            data.snapshot.is_loading_favorites = true;
            data.snapshot.is_loading_playlists = true;
        })
    }
    pub(crate) fn complete_initial_user_data(
        &self,
        generation: u64,
        result: Result<(), String>,
    ) -> Result<(), String> {
        let is_success = result.is_ok();
        self.publish_initial_user_data(generation, |data| {
            data.snapshot.is_loading_favorites = false;
            data.snapshot.is_loading_playlists = false;
            data.initial_load_result = Some(result);
        })?;
        if is_success {
            self.schedule_playlist_hydration(generation)?;
        }
        Ok(())
    }
    pub(crate) fn apply_initial_user_data_update(
        &self,
        generation: u64,
        update: InitialUserDataUpdate,
    ) -> Result<(), String> {
        if let InitialUserDataUpdate::UserIdentity(user_id) = update {
            return self
                .coordinator
                .lock()
                .map_err(|_| "coordinator lock poisoned")?
                .remember_current_user_id(generation, user_id);
        }
        self.publish_initial_user_data(generation, |data| match update {
            InitialUserDataUpdate::Favorites(favorites) => {
                data.snapshot.favorites = favorites;
                data.snapshot.has_loaded_favorites = true;
                data.snapshot.is_loading_favorites = false;
            }
            InitialUserDataUpdate::FavoritePage(favorites) => {
                for item in favorites.items() {
                    data.snapshot.favorites.set_item(item, true);
                }
            }
            InitialUserDataUpdate::PlaylistPage(playlists) => {
                data.snapshot.apply_playlist_metadata(playlists, false)
            }
            InitialUserDataUpdate::Playlists(playlists) => {
                data.snapshot.apply_playlist_metadata(playlists, true)
            }
            InitialUserDataUpdate::PlaylistTracks { item_id, items } => {
                if let Some(playlist) = data
                    .snapshot
                    .playlists
                    .iter_mut()
                    .find(|playlist| playlist.playlist.media.item_id == item_id)
                {
                    playlist.items = items;
                    playlist.is_loading_tracks = false;
                }
            }
            InitialUserDataUpdate::UserIdentity(_) => unreachable!(),
        })
    }
    pub(crate) fn publish_initial_user_data(
        &self,
        generation: u64,
        update: impl FnOnce(&mut LibraryState),
    ) -> Result<(), String> {
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| "coordinator lock poisoned")?;
        coordinator.update_initial_user_data(generation, update)?;
        let events = coordinator.session_events();
        drop(coordinator);
        events.publish(ApplicationEvent::UserDataChanged);
        Ok(())
    }
    pub(crate) fn publish_recommendations_changed(&self, generation: u64) -> Result<(), String> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| "coordinator lock poisoned")?;
        if coordinator.session_generation != generation {
            return Err("session changed during recommendations".into());
        }
        let events = coordinator.session_events();
        drop(coordinator);
        events.publish(ApplicationEvent::RecommendationsChanged);
        Ok(())
    }
    // Only visible private-playlist summaries request background tracks; results stay session-bound.
    fn schedule_playlist_hydration(&self, generation: u64) -> Result<(), String> {
        let (playlists, catalog, tasks, events) = {
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| "coordinator lock poisoned")?;
            if coordinator.session_generation != generation {
                return Err("session changed before playlist hydration".into());
            }
            let data = coordinator
                .user_data
                .as_ref()
                .ok_or("library not initialized")?;
            let playlists = data
                .snapshot
                .playlists
                .iter()
                .filter(|playlist| playlist.is_loading_tracks)
                .cloned()
                .collect::<Vec<_>>();
            let services = coordinator
                .repositories
                .as_ref()
                .ok_or("online services unavailable")?;
            (
                playlists,
                services.catalog.clone(),
                coordinator.session_tasks.clone(),
                coordinator.session_events(),
            )
        };
        if playlists.is_empty() {
            return Ok(());
        }
        let publisher = ApplicationUserDataPublisher::new(self.clone(), generation)?;
        tasks
            .spawn(&self.runtime, async move {
                if let Err(error) = super::startup::load_initial_playlist_tracks(
                    catalog.as_ref(),
                    playlists,
                    |_| true,
                    publisher,
                )
                .await
                {
                    events.publish(ApplicationEvent::Error {
                        error: ApplicationError::command_failed(error),
                    });
                }
            })
            .map(|_| ())
            .map_err(|error| error.message)
    }
    // Private session-bound playlist commands.
    async fn create_private_playlist_for_session(
        &self,
        name: &str,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        if name.trim().is_empty() {
            return Err(ApplicationError::command_failed("playlist name required"));
        }
        let (client, generation) = self.library_repository()?;
        let user_id = self
            .current_session_user_id(client.as_ref(), generation)
            .await
            .map_err(ApplicationError::command_failed)?;
        let confirmed = client
            .create_private_playlist(name.trim(), user_id.as_str())
            .await
            .map_err(ApplicationError::command_failed)?;
        let snapshot = self
            .publish_and_persist_playlist_for_session(generation, user_id.as_str(), confirmed)
            .await?;
        self.schedule_playlist_hydration(generation)
            .map_err(ApplicationError::command_failed)?;
        Ok(snapshot)
    }

    async fn rename_private_playlist_for_session(
        &self,
        item_id: &str,
        name: &str,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        if name.trim().is_empty() {
            return Err(ApplicationError::command_failed("playlist name required"));
        }
        let (client, generation) = self.library_repository()?;
        let user_id = self
            .current_session_user_id(client.as_ref(), generation)
            .await
            .map_err(ApplicationError::command_failed)?;
        let confirmed = client
            .rename_private_playlist(item_id, name.trim())
            .await
            .map_err(ApplicationError::command_failed)?;
        self.publish_and_persist_playlist_for_session(generation, user_id.as_str(), confirmed)
            .await
    }

    async fn remove_private_playlist_for_session(
        &self,
        item_id: &str,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let (client, generation) = self.library_repository()?;
        self.current_session_user_id(client.as_ref(), generation)
            .await
            .map_err(ApplicationError::command_failed)?;
        let removed = client
            .remove_private_playlist(item_id)
            .await
            .map_err(ApplicationError::command_failed)?;
        let catalog = self.catalog_repository(generation)?;
        self.publish_initial_user_data(generation, |data| {
            data.snapshot.record_playlist_removal(&removed);
            data.intents
                .retire_removed_item(&MediaItem::Playlist(removed.clone()));
            data.favorite_changes.notify_waiters();
        })
        .map_err(ApplicationError::command_failed)?;
        let events = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?
            .session_events();
        let persistence = catalog.commit_confirmed_playlist_removal(removed).await;
        self.finish_playlist_persistence_for_session(generation, events, persistence)
    }

    // Server metadata is visible before persistence; cache errors do not fail the server command.
    async fn publish_and_persist_playlist_for_session(
        &self,
        generation: u64,
        user_id: &str,
        confirmed: Playlist,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let catalog = self.catalog_repository(generation)?;
        self.publish_initial_user_data(generation, |data| {
            data.snapshot
                .record_playlist_metadata(confirmed.clone(), user_id)
        })
        .map_err(ApplicationError::command_failed)?;
        let events = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?
            .session_events();
        let persistence = catalog
            .commit_confirmed_media(MediaItem::Playlist(confirmed))
            .await;
        self.finish_playlist_persistence_for_session(generation, events, persistence)
    }

    fn finish_playlist_persistence_for_session(
        &self,
        generation: u64,
        events: EventPublisher,
        persistence: Result<(), String>,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?;
        if coordinator.session_generation != generation {
            return Err(ApplicationError::command_failed(
                "session changed during playlist confirmation",
            ));
        }
        let data = coordinator
            .user_data
            .as_ref()
            .ok_or_else(|| ApplicationError::command_failed("library not initialized"))?;
        let snapshot = data.project_snapshot();
        drop(coordinator);
        if let Err(error) = persistence {
            events.publish(ApplicationEvent::Error {
                error: ApplicationError::command_failed(format!(
                    "playlist cache persistence failed: {error}"
                )),
            });
        }
        Ok(snapshot)
    }
    // Private session identity and refresh helpers.
    async fn current_session_user_id(
        &self,
        client: &dyn LibraryClient,
        generation: u64,
    ) -> Result<crate::domain::identifiers::UserId, String> {
        let user_id = client.get_current_user_id().await?;
        self.coordinator
            .lock()
            .map_err(|_| "coordinator lock poisoned")?
            .remember_current_user_id(generation, user_id.clone())?;
        Ok(user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::events::EventPublisher;
    use crate::application::favorites::tests::RecordingCatalog;
    use crate::data::music_assistant::client::tests::server;
    use crate::domain::identifiers::UserId;
    use crate::domain::library::PrivatePlaylist;
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::Duration;

    #[tokio::test(flavor = "current_thread")]
    async fn private_playlist_commands_publish_before_commit_retain_cache_failure_and_reject_invalid_confirmation_or_session(
    ) {
        for (should_create_playlist, should_remove_playlist) in
            [(false, false), (true, false), (false, true)]
        {
            for (should_fail_commit, should_change_session, should_fail_confirmation) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, true, false),
                (false, false, true),
            ] {
                let original = json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Original","favorite":true,"metadata":{"description":"Old description"},"access":{"owner":"user","sharing":"private"}});
                let response = if should_fail_confirmation {
                    json!({"test_http_status":403})
                } else if should_remove_playlist {
                    serde_json::Value::Null
                } else {
                    json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Server name","favorite":null,"metadata":{"description":"Server description"},"access":{"owner":"user","sharing":"private"}})
                };
                let (first_command, confirmation_command) = if should_create_playlist {
                    (
                        "music/playlists/create_playlist",
                        "music/playlists/set_access",
                    )
                } else if should_remove_playlist {
                    ("music/playlists/get", "music/library/remove_item")
                } else {
                    ("music/playlists/get", "music/playlists/update")
                };
                let mut responses = vec![
                    (first_command, original.clone()),
                    (confirmation_command, response),
                ];
                if should_create_playlist && !should_change_session && !should_fail_confirmation {
                    responses.push(("music/playlists/playlist_tracks", json!([])));
                }
                let (client, worker) = server(responses);
                client
                    .initialize_current_user_id(UserId("user".into()))
                    .unwrap();
                let client = Arc::new(client);
                let release = Arc::new(tokio::sync::Notify::new());
                let catalog = Arc::new(RecordingCatalog {
                    online: client.clone(),
                    writes: Default::default(),
                    entered: Default::default(),
                    release: Some(release.clone()),
                    should_fail_commit,
                });
                let events = Arc::new(Mutex::new(Vec::new()));
                let published = events.clone();
                let changed = Arc::new(tokio::sync::Notify::new());
                let published_change = changed.clone();
                let application = Application::new(
                    crate::application::tests::environment(),
                    tokio::runtime::Handle::current(),
                    EventPublisher::new(move |event| {
                        published.lock().unwrap().push(event);
                        published_change.notify_one();
                    }),
                    Box::new(crate::application::tests::EmptySessionStorage),
                );
                let mut services = crate::bootstrap::session::online_services(client);
                services.catalog = catalog.clone();
                let mut data = LibraryState::new(services.library.clone());
                let original: crate::domain::media::Playlist =
                    serde_json::from_value(original).unwrap();
                let old_tracks: Vec<Track> = serde_json::from_value(
                    json!([{"provider":"library","item_id":"track","name":"Track"}]),
                )
                .unwrap();
                let owned = PrivatePlaylist {
                    playlist: original.clone(),
                    items: old_tracks.clone(),
                    is_loading_tracks: false,
                };
                let mut other = owned.clone();
                other.playlist.media.item_id = MediaId("other".into());
                other.playlist.media.name = "Other".into();
                data.snapshot.playlists = if should_create_playlist {
                    vec![other.clone()]
                } else {
                    vec![owned, other.clone()]
                };
                if !should_create_playlist {
                    data.snapshot.favorites.playlists = vec![original.clone()];
                }
                let removed_worker = if should_remove_playlist {
                    let item = MediaItem::Playlist(original.clone());
                    data.snapshot.favorite_overrides.push(
                        crate::domain::favorites::FavoriteOverride {
                            item: item.clone(),
                            is_favorite: true,
                        },
                    );
                    let (identity, _, _) =
                        data.intents.submit(&data.snapshot.favorites, item, true);
                    data.intents.begin_work(&identity);
                    Some(identity)
                } else {
                    None
                };
                let previous = data.snapshot.clone();
                let expected_tracks = if should_create_playlist {
                    Vec::new()
                } else {
                    old_tracks.clone()
                };
                let updated_index = usize::from(should_create_playlist);
                {
                    let mut coordinator = application.coordinator.lock().unwrap();
                    coordinator.repositories = Some(services);
                    coordinator.user_data = Some(data);
                    coordinator.session_configuration = Some(
                        serde_json::from_value(json!({
                            "musicAssistantUrl":"http://127.0.0.1:8095", "accessToken":null,
                            "userId":"user", "username":"User", "selectedOutput":"local",
                            "local":{"deviceId":"test","clientName":"Mashin test","cacheBytes":0},
                            "volume":70
                        }))
                        .unwrap(),
                    );
                }
                let operation = application.clone();
                let mut loading = tokio::spawn(async move {
                    if should_create_playlist {
                        operation
                            .create_private_playlist_for_session(" Requested name ")
                            .await
                    } else if should_remove_playlist {
                        operation.remove_private_playlist_for_session("42").await
                    } else {
                        operation
                            .rename_private_playlist_for_session("42", " Requested name ")
                            .await
                    }
                });
                if should_fail_confirmation {
                    assert!(tokio::time::timeout(Duration::from_secs(2), loading)
                        .await
                        .unwrap()
                        .unwrap()
                        .is_err());
                    assert_eq!(
                        application
                            .coordinator
                            .lock()
                            .unwrap()
                            .user_data
                            .as_ref()
                            .unwrap()
                            .snapshot,
                        previous
                    );
                    assert!(events.lock().unwrap().is_empty());
                    assert!(catalog.writes.lock().unwrap().is_empty());
                    assert_eq!(worker.join().unwrap().len(), 2);
                    continue;
                }
                tokio::select! {
                    result = &mut loading => panic!("playlist command ended before cache commit: {result:?}"),
                    result = tokio::time::timeout(Duration::from_secs(2), catalog.entered.notified()) => result.unwrap(),
                }
                {
                    let coordinator = application.coordinator.lock().unwrap();
                    let data = coordinator.user_data.as_ref().unwrap();
                    let snapshot = &data.snapshot;
                    if should_remove_playlist {
                        assert_eq!(snapshot.playlists, vec![other.clone()]);
                        assert!(snapshot.favorites.playlists.is_empty());
                        assert!(snapshot.favorite_overrides.is_empty());
                        let identity = removed_worker.as_ref().unwrap();
                        assert!(!data.intents.has_active_worker(identity));
                        assert_eq!(
                            data.intents.work(identity),
                            crate::domain::favorites::FavoriteWork::Finished
                        );
                        assert!(data.project_snapshot().favorites.playlists.is_empty());
                    } else {
                        assert_eq!(
                            snapshot.playlists[updated_index].playlist.media.name,
                            "Server name"
                        );
                        assert_eq!(snapshot.playlists[updated_index].items, expected_tracks);
                        assert_eq!(
                            snapshot.playlists[updated_index].is_loading_tracks,
                            should_create_playlist
                        );
                        assert_eq!(
                            snapshot.playlists[usize::from(!should_create_playlist)],
                            other
                        );
                        if should_create_playlist {
                            assert!(snapshot.favorites.playlists.is_empty());
                        } else {
                            assert_eq!(snapshot.favorites.playlists[0].media.name, "Server name");
                        }
                    }
                }
                assert_eq!(
                    events
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|event| matches!(event, ApplicationEvent::UserDataChanged))
                        .count(),
                    1
                );
                assert!(!loading.is_finished());
                if should_change_session {
                    let mut coordinator = application.coordinator.lock().unwrap();
                    coordinator.session_generation += 1;
                    coordinator.user_data.as_mut().unwrap().snapshot.playlists =
                        vec![other.clone()];
                }
                release.notify_one();
                let result = tokio::time::timeout(Duration::from_secs(2), loading)
                    .await
                    .unwrap()
                    .unwrap();
                if should_change_session {
                    assert!(result.is_err());
                    assert_eq!(
                        application
                            .coordinator
                            .lock()
                            .unwrap()
                            .user_data
                            .as_ref()
                            .unwrap()
                            .snapshot
                            .playlists,
                        vec![other]
                    );
                    assert!(!events
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|event| matches!(event, ApplicationEvent::Error { .. })));
                } else {
                    let snapshot = result.unwrap();
                    if should_remove_playlist {
                        assert_eq!(snapshot.playlists, vec![other.clone()]);
                        assert!(snapshot.favorites.playlists.is_empty());
                        assert!(snapshot.favorite_overrides.is_empty());
                    } else {
                        assert_eq!(
                            snapshot.playlists[updated_index].playlist.media.name,
                            "Server name"
                        );
                        assert_eq!(snapshot.playlists[updated_index].items, expected_tracks);
                    }
                    if should_create_playlist {
                        tokio::time::timeout(Duration::from_secs(2), async {
                            loop {
                                let notification = changed.notified();
                                let has_loaded_tracks = !application
                                    .coordinator
                                    .lock()
                                    .unwrap()
                                    .user_data
                                    .as_ref()
                                    .unwrap()
                                    .snapshot
                                    .playlists[updated_index]
                                    .is_loading_tracks;
                                if has_loaded_tracks {
                                    break;
                                }
                                notification.await;
                            }
                        })
                        .await
                        .unwrap();
                        let coordinator = application.coordinator.lock().unwrap();
                        let hydrated = &coordinator.user_data.as_ref().unwrap().snapshot;
                        assert!(hydrated.playlists[updated_index].items.is_empty());
                        assert_eq!(hydrated.playlists[0], other);
                        assert!(hydrated.favorites.playlists.is_empty());
                    }
                    assert_eq!(
                        events
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|event| matches!(event, ApplicationEvent::Error { .. }))
                            .count(),
                        usize::from(should_fail_commit)
                    );
                }
                assert_eq!(catalog.writes.lock().unwrap().len(), 1);
                assert_eq!(
                    catalog.writes.lock().unwrap()[0].fields().name,
                    if should_remove_playlist {
                        "Original"
                    } else {
                        "Server name"
                    }
                );
                assert_eq!(
                    events
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|event| matches!(event, ApplicationEvent::UserDataChanged))
                        .count(),
                    if should_create_playlist && !should_change_session {
                        2
                    } else {
                        1
                    }
                );
                let requests = worker.join().unwrap();
                assert_eq!(
                    requests.len(),
                    if should_create_playlist && !should_change_session {
                        3
                    } else {
                        2
                    }
                );
                if should_create_playlist {
                    assert_eq!(requests[0]["args"]["name"], "Requested name");
                    assert_eq!(requests[1]["args"]["owner"], "user");
                } else if should_remove_playlist {
                    assert_eq!(
                        requests[1]["args"],
                        json!({"media_type":"playlist","library_item_id":"42"})
                    );
                } else {
                    assert_eq!(requests[1]["args"]["update"]["name"], "Requested name");
                }
            }
        }
    }
}
