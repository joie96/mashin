//! One UI session projection of server-confirmed favorites and playlists, never a media inventory.
use super::errors::ApplicationError;
use super::events::ApplicationEvent;
use super::repositories::{InitialUserDataUpdate, LibraryClient, UserDataPublisher};
use super::Application;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::favorites::FavoriteIntents;
use crate::domain::identifiers::MediaId;
use crate::domain::library::UserDataSnapshot;
use crate::domain::media::Track;
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
    // Commands wait for the server before requesting the visible playlist projection again.
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
        if name.trim().is_empty() {
            return Err(ApplicationError::command_failed("playlist name required"));
        }
        let (client, generation) = self.library_repository()?;
        self.runtime
            .block_on(async {
                let user_id = self
                    .current_session_user_id(client.as_ref(), generation)
                    .await?;
                client
                    .create_private_playlist(name.trim(), user_id.as_str())
                    .await
            })
            .map_err(ApplicationError::command_failed)?;
        self.refresh_for_session(generation)
    }
    pub(crate) fn rename_private_playlist(
        &self,
        item_id: &str,
        name: String,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        if name.trim().is_empty() {
            return Err(ApplicationError::command_failed("playlist name required"));
        }
        let (client, generation) = self.library_repository()?;
        self.runtime
            .block_on(async {
                let playlist = client.get_private_playlist(item_id).await?;
                client.rename_private_playlist(&playlist, name.trim()).await
            })
            .map_err(ApplicationError::command_failed)?;
        self.refresh_for_session(generation)
    }
    pub(crate) fn remove_private_playlist(
        &self,
        item_id: &str,
    ) -> Result<UserDataSnapshot, ApplicationError> {
        let (client, generation) = self.library_repository()?;
        self.runtime
            .block_on(async {
                client.get_private_playlist(item_id).await?;
                client.delete_private_playlist(item_id).await
            })
            .map_err(ApplicationError::command_failed)?;
        self.refresh_for_session(generation)
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
        let result = self
            .runtime
            .block_on(super::startup::load_initial_user_data(
                client.as_ref(),
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
                let snapshot = client.load_library_user_data(user_id.as_str()).await?;
                self.publish_initial_user_data(generation, |data| {
                    data.snapshot = snapshot;
                    data.intents.reconcile_complete_read(intent_revision);
                })?;
            }
            UserDataRefresh::Favorites => {
                let favorites = client.load_favorites().await?;
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
                let playlists = client.load_private_playlists(user_id.as_str()).await?;
                self.publish_initial_user_data(generation, |data| {
                    data.snapshot.playlists = playlists
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
            InitialUserDataUpdate::Playlists(playlists) => {
                data.snapshot.playlists = playlists;
                data.snapshot.is_loading_playlists = false;
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
    fn refresh_for_session(&self, generation: u64) -> Result<UserDataSnapshot, ApplicationError> {
        self.runtime.block_on(
            self.refresh_user_data_for_session(Some(generation), UserDataRefresh::Playlists),
        )
    }
}
