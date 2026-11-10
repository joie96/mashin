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
    hydrating_playlist_ids: std::collections::HashSet<MediaId>,
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
            hydrating_playlist_ids: Default::default(),
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
        self.runtime
            .block_on(self.refresh_private_playlist_cover_for_session(item_id))
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
                let playlist = client.get_private_playlist(item_id).await?;
                let tracks = catalog.read_confirmed_playlist_contents(playlist).await?;
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
                let catalog = self.catalog_repository(generation)?;
                if let Some(playlist) = playlist.as_mut() {
                    playlist.items = catalog
                        .read_confirmed_playlist_contents(playlist.playlist.clone())
                        .await?;
                    playlist.is_loading_tracks = false;
                }
                let confirmed = playlist.clone();
                self.publish_initial_user_data(generation, |data| {
                    if let Some(playlist) = playlist {
                        data.snapshot
                            .record_playlist_metadata(playlist.playlist.clone(), user_id.as_str());
                        if let Some(existing) =
                            data.snapshot.playlists.iter_mut().find(|existing| {
                                existing.playlist.media.item_id.as_str() == item_id
                            })
                        {
                            existing.items = playlist.items;
                            existing.is_loading_tracks = false;
                        }
                    } else {
                        data.snapshot
                            .playlists
                            .retain(|existing| existing.playlist.media.item_id.as_str() != item_id);
                    }
                })?;
                if let Some(confirmed) = confirmed {
                    let events = self
                        .coordinator
                        .lock()
                        .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?
                        .session_events();
                    let persistence = catalog
                        .commit_confirmed_playlist_contents(confirmed.playlist, confirmed.items)
                        .await;
                    self.finish_playlist_persistence_for_session(generation, events, persistence)?;
                }
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
        let should_hydrate = matches!(
            &update,
            InitialUserDataUpdate::PlaylistPage(_) | InitialUserDataUpdate::Playlists(_)
        );
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
                    if playlist.is_loading_tracks || playlist.items != items {
                        playlist.items = items;
                        playlist.is_loading_tracks = false;
                    }
                }
            }
            InitialUserDataUpdate::UserIdentity(_) => unreachable!(),
        })?;
        if should_hydrate {
            self.schedule_playlist_hydration(generation)?;
        }
        Ok(())
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
    // Reserve each visible playlist once while its local read and background refresh are active.
    fn schedule_playlist_hydration(&self, generation: u64) -> Result<(), String> {
        let (playlists, catalog, tasks, events) = {
            let mut coordinator = self
                .coordinator
                .lock()
                .map_err(|_| "coordinator lock poisoned")?;
            if coordinator.session_generation != generation {
                return Err("session changed before playlist hydration".into());
            }
            let catalog = coordinator
                .repositories
                .as_ref()
                .ok_or("online services unavailable")?
                .catalog
                .clone();
            let data = coordinator
                .user_data
                .as_mut()
                .ok_or("library not initialized")?;
            let playlists = data
                .snapshot
                .playlists
                .iter()
                .filter(|playlist| {
                    playlist.is_loading_tracks
                        && !data
                            .hydrating_playlist_ids
                            .contains(&playlist.playlist.media.item_id)
                })
                .cloned()
                .collect::<Vec<_>>();
            data.hydrating_playlist_ids.extend(
                playlists
                    .iter()
                    .map(|playlist| playlist.playlist.media.item_id.clone()),
            );
            (
                playlists,
                catalog,
                coordinator.session_tasks.clone(),
                coordinator.session_events(),
            )
        };
        if playlists.is_empty() {
            return Ok(());
        }
        let publisher = ApplicationUserDataPublisher::new(self.clone(), generation)?;
        let item_ids = playlists
            .iter()
            .map(|playlist| playlist.playlist.media.item_id.clone())
            .collect::<Vec<_>>();
        let worker_ids = item_ids.clone();
        let application = self.clone();
        let result = tasks
            .spawn(&self.runtime, async move {
                let result = super::startup::load_initial_playlist_tracks(
                    catalog.as_ref(),
                    playlists,
                    |_| true,
                    publisher,
                )
                .await;
                if let Ok(mut coordinator) = application.coordinator.lock() {
                    if coordinator.session_generation == generation {
                        if let Some(data) = coordinator.user_data.as_mut() {
                            for item_id in worker_ids {
                                data.hydrating_playlist_ids.remove(&item_id);
                            }
                        }
                    }
                }
                if let Err(error) = result {
                    events.publish(ApplicationEvent::Error {
                        error: ApplicationError::command_failed(error),
                    });
                }
            })
            .map(|_| ())
            .map_err(|error| error.message);
        if result.is_err() {
            if let Ok(mut coordinator) = self.coordinator.lock() {
                if coordinator.session_generation == generation {
                    if let Some(data) = coordinator.user_data.as_mut() {
                        for item_id in item_ids {
                            data.hydrating_playlist_ids.remove(&item_id);
                        }
                    }
                }
            }
        }
        result
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

    async fn refresh_private_playlist_cover_for_session(
        &self,
        item_id: &str,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let (client, generation) = self.library_repository()?;
        let (services, _) = self.session_repositories()?;
        let user_id = self
            .current_session_user_id(client.as_ref(), generation)
            .await
            .map_err(ApplicationError::command_failed)?;
        let mut playlist = client
            .load_private_playlist(item_id, user_id.as_str())
            .await
            .map_err(ApplicationError::command_failed)?
            .ok_or_else(|| ApplicationError::command_failed("private playlist unavailable"))?;
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
            .await
            .map_err(ApplicationError::command_failed)?
        else {
            return Err(ApplicationError::command_failed(
                "playlist tracks unavailable",
            ));
        };
        playlist.items = items;
        let artwork = super::playlist_artwork::PlaylistArtwork::new(
            client,
            services.images,
            self.query_context()?.tasks,
            self.runtime.clone(),
        );
        artwork
            .refresh_private_playlist_cover(&mut playlist)
            .await
            .map_err(ApplicationError::command_failed)?;
        self.publish_and_persist_playlist_for_session(
            generation,
            user_id.as_str(),
            playlist.playlist,
        )
        .await
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
    async fn sidebar_cache_starts_with_first_metadata_page_once_and_rejects_retired_session() {
        use crate::application::startup::tests::SidebarCatalog;
        use crate::domain::catalog::CatalogLoadPhase;
        use std::sync::atomic::Ordering;

        let (client, worker) = server(Vec::new());
        let mut services = crate::bootstrap::session::online_services(Arc::new(client));
        let catalog = Arc::new(SidebarCatalog::new(Some(CatalogLoadPhase::Ready)));
        services.catalog = catalog.clone();
        let changed = Arc::new(tokio::sync::Notify::new());
        let published = changed.clone();
        let application = Application::new(
            crate::application::tests::environment(),
            tokio::runtime::Handle::current(),
            EventPublisher::new(move |_| {
                published.notify_one();
            }),
            Box::new(crate::application::tests::EmptySessionStorage),
        );
        let generation = {
            let mut coordinator = application.coordinator.lock().unwrap();
            coordinator.repositories = Some(services.clone());
            coordinator.user_data = Some(LibraryState::new(services.library.clone()));
            coordinator.connection = Some(Arc::new(
                crate::data::music_assistant::connection::MusicAssistantConnection::new(
                    crate::domain::session::MusicAssistantServerUrl("http://127.0.0.1:8095".into()),
                    Some(crate::domain::session::AccessToken("synthetic".into())),
                )
                .unwrap(),
            ));
            coordinator.session_generation
        };
        application.begin_initial_user_data(generation).unwrap();
        let playlist = PrivatePlaylist {
            playlist: serde_json::from_value(
                json!({"provider":"library","item_id":"42","name":"Cached"}),
            )
            .unwrap(),
            items: Vec::new(),
            is_loading_tracks: true,
        };
        for _ in 0..2 {
            application
                .apply_initial_user_data_update(
                    generation,
                    InitialUserDataUpdate::PlaylistPage(vec![playlist.clone()]),
                )
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                {
                    let coordinator = application.coordinator.lock().unwrap();
                    let snapshot = &coordinator.user_data.as_ref().unwrap().snapshot;
                    assert!(snapshot.is_loading_favorites);
                    assert!(snapshot.is_loading_playlists);
                    if !snapshot.playlists[0].is_loading_tracks {
                        assert_eq!(snapshot.playlists[0].items[0].duration, 120);
                        break;
                    }
                }
                changed.notified().await;
            }
        })
        .await
        .unwrap();
        application
            .apply_initial_user_data_update(
                generation,
                InitialUserDataUpdate::PlaylistPage(vec![playlist]),
            )
            .unwrap();
        assert_eq!(catalog.reads.load(Ordering::SeqCst), 1);
        {
            let mut coordinator = application.coordinator.lock().unwrap();
            coordinator.session_generation += 1;
            coordinator.user_data = Some(LibraryState::new(services.library.clone()));
        }
        drop(services);
        catalog.release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            while Arc::strong_count(&catalog) > 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(application
            .coordinator
            .lock()
            .unwrap()
            .user_data
            .as_ref()
            .unwrap()
            .snapshot
            .playlists
            .is_empty());
        worker.join().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn playlist_track_commands_wait_for_confirmation_publish_before_commit_and_preserve_neighbors(
    ) {
        for (should_remove, is_private) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            for (
                should_fail_task,
                should_fail_readback,
                should_fail_commit,
                should_change_session,
            ) in [
                (false, false, false, false),
                (true, false, false, false),
                (false, true, false, false),
                (false, false, true, false),
                (false, false, false, true),
            ] {
                let playlist = json!({"provider":"library","item_id":"42","name":"Confirmed","is_editable":true,"access":{"owner":"user","sharing":"private"}});
                let command = if should_remove {
                    "music/playlists/remove_playlist_tracks"
                } else {
                    "music/playlists/add_playlist_tracks"
                };
                let mut responses = vec![
                    (command, json!({"id":"mutation","status":"running"})),
                    (
                        "tasks/get",
                        json!({"id":"mutation","status":if should_fail_task { "failed" } else { "success" },"last_error":"synthetic task error"}),
                    ),
                ];
                let preflight_count = if is_private {
                    if should_remove {
                        2
                    } else {
                        1
                    }
                } else {
                    0
                };
                if is_private {
                    responses.insert(0, ("music/playlists/get", playlist.clone()));
                    if should_remove {
                        responses.insert(1, ("music/playlists/playlist_tracks", json!([
                            {"media_type":"track","provider":"library","item_id":"old","name":"Old","uri":"library://track/old","position":2},
                            {"media_type":"track","provider":"library","item_id":"old","name":"Old","uri":"library://track/old","position":5}
                        ])));
                    }
                }
                if !should_fail_task {
                    responses.push(("music/playlists/get", playlist.clone()));
                    responses.push(("music/playlists/playlist_tracks", if should_fail_readback { json!({"test_http_status":403}) } else { json!([
                        {"media_type":"track","provider":"library","item_id":"new","name":"New track","position":0},
                        {"media_type":"track","provider":"library","item_id":"new","name":"New track","position":1}
                    ]) }));
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
                let application = Application::new(
                    crate::application::tests::environment(),
                    tokio::runtime::Handle::current(),
                    EventPublisher::new(move |event| published.lock().unwrap().push(event)),
                    Box::new(crate::application::tests::EmptySessionStorage),
                );
                let mut services = crate::bootstrap::session::online_services(client);
                services.catalog = catalog.clone();
                let mut data = LibraryState::new(services.library.clone());
                let mut owned = PrivatePlaylist {
                    playlist: serde_json::from_value(playlist).unwrap(),
                    items: serde_json::from_value(
                        json!([{"provider":"library","item_id":"old","name":"Old track"}]),
                    )
                    .unwrap(),
                    is_loading_tracks: false,
                };
                owned.playlist.media.name = "Old playlist".into();
                data.snapshot.favorites.playlists = vec![owned.playlist.clone()];
                let mut neighbor = owned.clone();
                neighbor.playlist.media.item_id = MediaId("neighbor".into());
                data.snapshot.playlists = vec![owned, neighbor.clone()];
                let previous = data.snapshot.clone();
                {
                    let mut coordinator = application.coordinator.lock().unwrap();
                    coordinator.connection = Some(Arc::new(
                        crate::data::music_assistant::connection::MusicAssistantConnection::new(
                            crate::domain::session::MusicAssistantServerUrl(
                                "http://127.0.0.1:8095".into(),
                            ),
                            Some(crate::domain::session::AccessToken("synthetic".into())),
                        )
                        .unwrap(),
                    ));
                    coordinator.repositories = Some(services);
                    coordinator.user_data = Some(data);
                    coordinator.session_configuration = Some(serde_json::from_value(json!({
                        "musicAssistantUrl":"http://127.0.0.1:8095","accessToken":null,"userId":"user","username":"User","selectedOutput":"local",
                        "local":{"deviceId":"test","clientName":"Mashin test","cacheBytes":0},"volume":70
                    })).unwrap());
                }
                let operation = application.clone();
                let mut loading = tokio::task::spawn_blocking(move || {
                    if is_private && should_remove {
                        operation
                            .remove_private_playlist_tracks(
                                "42",
                                vec!["library://track/old".into()],
                            )
                            .map(|_| ())
                    } else if is_private {
                        operation.add_private_playlist_tracks("42", serde_json::from_value(json!([{"provider":"library","item_id":"new","name":"New track","uri":"library://track/new"}])).unwrap()).map(|_| ())
                    } else if should_remove {
                        operation.remove_playlist_tracks("42".into(), vec![0])
                    } else {
                        operation
                            .add_playlist_tracks("42".into(), vec!["library://track/new".into()])
                    }
                });
                if should_fail_task || should_fail_readback {
                    assert!(tokio::time::timeout(Duration::from_secs(3), loading)
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
                    assert!(catalog.writes.lock().unwrap().is_empty());
                    assert!(events.lock().unwrap().is_empty());
                } else {
                    tokio::select! {
                        result = &mut loading => panic!("command ended before cache commit: {result:?}"),
                        result = tokio::time::timeout(Duration::from_secs(3), catalog.entered.notified()) => result.unwrap(),
                    }
                    {
                        let coordinator = application.coordinator.lock().unwrap();
                        let current = &coordinator.user_data.as_ref().unwrap().snapshot;
                        assert_eq!(current.playlists[0].playlist.media.name, "Confirmed");
                        assert_eq!(current.playlists[0].items.len(), 2);
                        assert_eq!(current.playlists[0].items[1].media.position, Some(1));
                        assert_eq!(current.playlists[1], neighbor);
                        assert_eq!(current.favorites.playlists[0].media.name, "Confirmed");
                    }
                    assert_eq!(catalog.writes.lock().unwrap().len(), 1);
                    if should_change_session {
                        let mut coordinator = application.coordinator.lock().unwrap();
                        coordinator.session_generation += 1;
                        coordinator.user_data.as_mut().unwrap().snapshot.playlists =
                            vec![neighbor.clone()];
                    }
                    release.notify_one();
                    let result = tokio::time::timeout(Duration::from_secs(3), loading)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(result.is_err(), should_change_session);
                    assert_eq!(
                        events
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|event| matches!(event, ApplicationEvent::Error { .. })),
                        should_fail_commit && !should_change_session
                    );
                    if should_change_session {
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
                            vec![neighbor]
                        );
                    }
                }
                let requests = worker.join().unwrap();
                assert_eq!(
                    requests.len(),
                    preflight_count + if should_fail_task { 2 } else { 4 }
                );
                assert_eq!(requests[preflight_count + 1]["command"], "tasks/get");
                if is_private && should_remove {
                    assert_eq!(
                        requests[preflight_count]["args"]["positions_to_remove"],
                        json!([2, 5])
                    );
                }
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn private_playlist_commands_publish_before_commit_retain_cache_failure_and_reject_invalid_confirmation_or_session(
    ) {
        for (should_create_playlist, should_remove_playlist, should_refresh_cover) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            for (should_fail_commit, should_change_session, should_fail_confirmation) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, true, false),
                (false, false, true),
            ] {
                let original = json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Original","favorite":true,"is_editable":true,"metadata":{"description":"Old description","images":[{"type":"thumb","provider":"builtin","path":"old.jpg"}]},"access":{"owner":"user","sharing":"private"}});
                let response = if should_fail_confirmation {
                    json!({"test_http_status":403})
                } else if should_remove_playlist {
                    serde_json::Value::Null
                } else {
                    json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Server name","favorite":null,"metadata":{"description":"Server description","images":[{"type":"thumb","provider":"builtin","path":"confirmed.jpg","proxy_id":"a".repeat(64)}]},"access":{"owner":"user","sharing":"private"}})
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
                if should_refresh_cover {
                    use base64::Engine;
                    let mut png = std::io::Cursor::new(Vec::new());
                    image::DynamicImage::new_rgb8(2, 2)
                        .write_to(&mut png, image::ImageFormat::Png)
                        .unwrap();
                    let inline = format!(
                        "data:image/png;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
                    );
                    let track = json!({"media_type":"track","provider":"library","item_id":"cover-input","name":"Collage input","uri":"library://track/cover-input","metadata":{"images":[{"type":"thumb","provider":"builtin","path":inline}]}});
                    responses.insert(1, ("music/playlists/playlist_tracks", json!([track])));
                    responses.insert(2, ("music/playlists/get", original.clone()));
                }
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
                    if should_refresh_cover {
                        coordinator.connection = Some(Arc::new(crate::data::music_assistant::connection::MusicAssistantConnection::new(
                            crate::domain::session::MusicAssistantServerUrl("http://127.0.0.1:8095".into()),
                            Some(crate::domain::session::AccessToken("synthetic".into())),
                        ).unwrap()));
                    }
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
                    } else if should_refresh_cover {
                        operation
                            .refresh_private_playlist_cover_for_session("42")
                            .await
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
                    assert_eq!(
                        worker.join().unwrap().len(),
                        if should_refresh_cover { 4 } else { 2 }
                    );
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
                        if should_refresh_cover {
                            let metadata = snapshot.playlists[updated_index]
                                .playlist
                                .media
                                .metadata
                                .as_ref()
                                .unwrap();
                            assert_eq!(metadata.images[0].path, "confirmed.jpg");
                            assert_eq!(
                                metadata.images[0].proxy_id.as_deref(),
                                Some("a".repeat(64).as_str())
                            );
                        }
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
                    if should_refresh_cover {
                        4
                    } else if should_create_playlist && !should_change_session {
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
                } else if should_refresh_cover {
                    assert!(
                        requests[3]["args"]["update"]["metadata"]["images"][0]["path"]
                            .as_str()
                            .unwrap()
                            .starts_with("data:image/jpeg;base64,")
                    );
                    assert_eq!(
                        catalog.writes.lock().unwrap()[0]
                            .fields()
                            .metadata
                            .as_ref()
                            .unwrap()
                            .images[0]
                            .path,
                        "confirmed.jpg"
                    );
                } else {
                    assert_eq!(requests[1]["args"]["update"]["name"], "Requested name");
                }
            }
        }
    }
}
