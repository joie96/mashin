//! Shared execution of typed requests; state stays with the owners migrated in M2-M4.

use super::commands::ApplicationCommand;
use super::queries::{ApplicationQuery, QueryResult};
use super::Application;
use crate::application::errors::ApplicationError;
use crate::application::events::ApplicationEvent;
use crate::application::queries::SessionQueryContext;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::MediaId;
use crate::domain::library::UserDataSnapshot;
use crate::domain::media::MediaItem;
use crate::domain::session::MusicAssistantServerUrl;
use crate::domain::settings::{LoginConfig, RemoteOutputConfig, SelectedOutput};
use crate::playback::models::{QueueOption, RepeatMode};

pub(crate) struct CommandResult {
    pub(crate) value: CommandValue,
    pub(crate) events: Vec<ApplicationEvent>,
}

pub(crate) enum CommandValue {
    Unit,
    RestoredSession(bool),
    UserData(UserDataSnapshot),
}

impl Application {
    // Command execution; adapters own result and event delivery.
    pub(crate) fn execute_command(
        &self,
        command: ApplicationCommand,
    ) -> Result<CommandResult, ApplicationError> {
        use ApplicationCommand::*;

        if matches!(
            &command,
            SetUserDataFavorites { .. }
                | AddPlaylistTracks { .. }
                | RemovePlaylistTracks { .. }
                | CreatePrivatePlaylist { .. }
                | RenamePrivatePlaylist { .. }
                | RefreshPrivatePlaylistCover { .. }
                | RemovePrivatePlaylist { .. }
                | AddPrivatePlaylistTracks { .. }
                | RemovePrivatePlaylistTracks { .. }
        ) && !self.query_context()?.is_authenticated
        {
            return Err(ApplicationError::command_failed(
                "an authenticated connection is required for library changes",
            ));
        }
        let mut value = CommandValue::Unit;
        let events = match command {
            Login {
                music_assistant_url,
                username,
                password,
            } => {
                let generation = self.query_context()?.generation;
                let session = self
                    .runtime
                    .block_on(super::session::authenticate(
                        LoginConfig {
                            music_assistant_url: MusicAssistantServerUrl(music_assistant_url),
                            username,
                            password,
                        },
                        &self.environment,
                    ))
                    .map_err(ApplicationError::command_failed)?;
                self.execute_command_and_collect_events(|coordinator| {
                    coordinator.commit_authenticated_session(generation, session)
                })?
            }
            Logout {} => {
                let mut connection = None;
                let events = self.execute_command_and_collect_events(|coordinator| {
                    connection = coordinator.logout()?;
                    Ok(())
                })?;
                if let Some(connection) = connection {
                    let _ = self.runtime.block_on(connection.client.logout());
                }
                events
            }
            RestorePersistedSession {} => {
                let (restored, events) = self.restore_persisted_session_and_collect_events()?;
                value = CommandValue::RestoredSession(restored);
                events
            }
            ActivateEngine {} => {
                self.execute_playback_command_and_collect_events(|playback| playback.activate())?
            }
            DeactivateEngine {} => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.deactivate();
                    Ok(())
                })?
            }
            Shutdown {} => {
                self.shutdown()?;
                Vec::new()
            }
            SelectLocalOutput {} => self.execute_command_and_collect_events(|coordinator| {
                coordinator.select_local_output()
            })?,
            SelectRemotePlayer { player_id } => {
                self.execute_command_and_collect_events(|coordinator| {
                    coordinator
                        .select_output(SelectedOutput::Remote(RemoteOutputConfig { player_id }))
                })?
            }
            TransportPlay {} => self.execute_playback_command_and_collect_events(|playback| {
                playback.transport_play()
            })?,
            TransportPause {} => self.execute_playback_command_and_collect_events(|playback| {
                playback.transport_pause()
            })?,
            TransportStop {} => self.execute_playback_command_and_collect_events(|playback| {
                playback.transport_stop();
                Ok(())
            })?,
            TransportNext {} => self.execute_playback_command_and_collect_events(|playback| {
                playback.transport_next()
            })?,
            TransportPrevious {} => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.transport_previous()
                })?
            }
            TransportSeek { seconds } => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.transport_seek(seconds)
                })?
            }
            TransportRepeatMode { mode } => {
                let mode = match mode {
                    0 => RepeatMode::Off,
                    1 => RepeatMode::One,
                    2 => RepeatMode::All,
                    _ => return Err(ApplicationError::command_failed("invalid repeat mode")),
                };
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.set_repeat_mode(mode);
                    Ok(())
                })?
            }
            TransportShuffle { enabled } => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.set_shuffle(enabled);
                    Ok(())
                })?
            }
            SetAutoplay { enabled } => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.set_autoplay(enabled);
                    Ok(())
                })?
            }
            SetVolume { volume } => self.execute_command_and_collect_events(|coordinator| {
                coordinator.set_volume(volume.min(100))
            })?,
            SetMuted { muted } => self.execute_playback_command_and_collect_events(|playback| {
                playback.set_muted(muted);
                Ok(())
            })?,
            SaveSessionSettings { settings } => {
                self.execute_command_and_collect_events(|coordinator| {
                    coordinator.save_session_settings(settings)
                })?
            }
            SaveUpdateChannel { channel } => {
                self.save_update_channel(channel)?;
                Vec::new()
            }
            SetPreferredProvider { preferred_provider } => self
                .execute_command_and_collect_events(|coordinator| {
                    coordinator.set_preferred_provider(preferred_provider)
                })?,
            SetUserDataFavorites {
                media_items,
                favorite,
            } => {
                self.set_user_data_favorites(media_items, favorite)?;
                Vec::new()
            }
            QueuePlayMedia { request } => {
                if request.option == QueueOption::Unknown {
                    return Err(ApplicationError::command_failed("unknown queue option"));
                }
                if request.media_items.is_empty() && request.option == QueueOption::Replace {
                    self.execute_playback_command_and_collect_events(|playback| {
                        playback.queue_play_media(Vec::new(), request.option, None, false)
                    })?
                } else {
                    self.execute_media_command(
                        request.media_items,
                        request.option,
                        |playback, tracks| {
                            playback.queue_play_media(
                                tracks,
                                request.option,
                                request.start_item,
                                request.shuffle,
                            )
                        },
                    )?
                }
            }
            StartRadio { media_items } => self.start_radio_and_execute_command(media_items)?,
            MoveQueueItem {
                item_id,
                position_shift,
            } => self.execute_playback_command_and_collect_events(|playback| {
                playback.move_queue_item(item_id, position_shift)
            })?,
            MoveQueueItemToEnd { item_id } => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.move_queue_item_to_end(item_id)
                })?
            }
            DeleteQueueItems { item_ids } => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.delete_queue_items(item_ids)
                })?
            }
            SelectQueueItem { item_id } => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.select_queue_item(item_id)
                })?
            }
            PlayQueueIndex { index } => {
                self.execute_playback_command_and_collect_events(|playback| {
                    playback.play_queue_index(index)
                })?
            }
            SetAudioProfile { audio_profile } => {
                self.execute_command_and_collect_events(|coordinator| {
                    coordinator.set_audio_profile(
                        &audio_profile,
                        std::sync::Arc::downgrade(&self.coordinator),
                    )
                })?
            }
            AddPlaylistTracks { playlist_id, uris } => {
                self.add_playlist_tracks(playlist_id, uris)?;
                Vec::new()
            }
            RemovePlaylistTracks {
                playlist_id,
                positions,
            } => {
                self.remove_playlist_tracks(playlist_id, positions)?;
                Vec::new()
            }
            CreatePrivatePlaylist { name } => {
                self.create_private_playlist(name)?;
                Vec::new()
            }
            RenamePrivatePlaylist { playlist_id, name } => {
                self.rename_private_playlist(&playlist_id, name)?;
                Vec::new()
            }
            RefreshPrivatePlaylistCover { playlist_id } => {
                value = CommandValue::UserData(self.refresh_private_playlist_cover(&playlist_id)?);
                Vec::new()
            }
            RemovePrivatePlaylist { playlist_id } => {
                self.remove_private_playlist(&playlist_id)?;
                Vec::new()
            }
            AddPrivatePlaylistTracks {
                playlist_id,
                tracks,
            } => {
                self.add_private_playlist_tracks(&playlist_id, tracks)?;
                Vec::new()
            }
            RemovePrivatePlaylistTracks { playlist_id, uris } => {
                self.remove_private_playlist_tracks(&playlist_id, uris)?;
                Vec::new()
            }
        };
        Ok(CommandResult { value, events })
    }

    // Query execution and cancellation; adapters own serialization and delivery.
    pub(crate) fn start_query(
        &self,
        request_id: u64,
        query: ApplicationQuery,
        publish: impl FnOnce(Result<QueryResult, ApplicationError>) + Send + 'static,
    ) -> Result<(), ApplicationError> {
        let context = self.query_context()?;
        let tasks = context.tasks.queries();
        if matches!(query, ApplicationQuery::CancelQuery {}) {
            return tasks.cancel(request_id);
        }
        tasks.start(
            &self.runtime,
            request_id,
            std::time::Duration::from_secs(45),
            self.clone().execute_query(query, context),
            publish,
        )
    }

    pub(crate) fn execute_query_sync(
        &self,
        query: ApplicationQuery,
    ) -> Result<QueryResult, ApplicationError> {
        let context = self.query_context()?;
        let tasks = context.tasks.queries();
        self.runtime.block_on(tasks.execute(
            &self.runtime,
            std::time::Duration::from_secs(45),
            self.clone().execute_query(query, context),
        ))
    }

    // Capture the accepted session before scheduling; a later login cannot redirect the request.
    async fn execute_query(
        self,
        query: ApplicationQuery,
        context: SessionQueryContext,
    ) -> Result<QueryResult, ApplicationError> {
        use ApplicationQuery::*;
        if context.is_authenticated
            && !matches!(
                query,
                GetSessionSettings {}
                    | GetUpdateChannel { .. }
                    | RefreshQueue {}
                    | RefreshPlayerState {}
                    | MediaSessionState {}
                    | CancelQuery {}
            )
        {
            self.ensure_session_repositories(context.generation).await?;
        }
        let query = match query {
            GetSessionSettings {} => {
                return self
                    .read_session_settings()
                    .map(QueryResult::SessionSettings)
            }
            GetUpdateChannel { default_channel } => {
                return self
                    .get_update_channel(default_channel)
                    .map(QueryResult::UpdateChannel)
            }
            RefreshQueue {} => return self.read_queue().map(QueryResult::Queue),
            RefreshPlayerState {} => return self.read_player_state().map(QueryResult::PlayerState),
            MediaSessionState {} => {
                return self
                    .read_media_session_state()
                    .map(QueryResult::MediaSessionState)
            }
            GetUserData {} => {
                return self.read_user_data().await.map(QueryResult::UserData);
            }
            LoadUserData {} => {
                if !context.is_authenticated {
                    return self.read_user_data().await.map(QueryResult::UserData);
                }
                let runtime = self.runtime.clone();
                let application = self.clone();
                let result = context
                    .tasks
                    .run_blocking(&runtime, move || {
                        application.read_or_load_user_data(context.generation)
                    })
                    .await?
                    .map(QueryResult::UserData)?;
                return self
                    .project_provider_availability(context.generation, result)
                    .await;
            }
            StartSessionDataLoad {} => {
                if !context.is_authenticated {
                    return Err(ApplicationError::command_failed(
                        "an authenticated connection is required for startup refresh",
                    ));
                }
                self.start_session_data_load(context.generation);
                return Ok(QueryResult::Unit);
            }
            CancelQuery {} => {
                return Err(ApplicationError::internal(
                    "cancel query requires an active request ID",
                ))
            }
            query => query,
        };
        if !context.is_authenticated {
            return Err(ApplicationError::command_failed(
                "a restored session is required",
            ));
        }
        let generation = context.generation;
        let result: Result<QueryResult, String> = match query {
            Recommendations { view, refresh } => {
                let recommendations = self.recommendation_service(generation)?;
                let events = {
                    let coordinator = self.coordinator.lock().map_err(|_| {
                        ApplicationError::internal("core coordinator lock poisoned")
                    })?;
                    if coordinator.session_generation != generation {
                        return Err(ApplicationError::command_failed(
                            "session changed before recommendations",
                        ));
                    }
                    coordinator.session_events()
                };
                recommendations
                    .get_recommendation_data(view, refresh, |snapshot| {
                        events.publish(
                            crate::application::events::ApplicationEvent::RecommendationsUpdated { view, snapshot },
                        );
                    }, |query, snapshot| {
                        events.publish(
                            crate::application::events::ApplicationEvent::CatalogReadChanged { query, snapshot },
                        );
                    })
                    .await
                    .map(QueryResult::Recommendations)
            }
            ProviderManifests {} => self
                .provider_repository(generation)?
                .get_manifests()
                .await
                .map(QueryResult::ProviderManifests),
            ProviderIconData { provider } => self
                .provider_repository(generation)?
                .get_icon_data(&provider)
                .await
                .map(QueryResult::ProviderIconData),
            MediaArtwork { item } => {
                let repository = {
                    let coordinator = self.coordinator.lock().map_err(|_| {
                        ApplicationError::internal("core coordinator lock poisoned")
                    })?;
                    if coordinator.session_generation != generation {
                        return Err(ApplicationError::command_failed(
                            "session changed before artwork read",
                        ));
                    }
                    coordinator
                        .repositories
                        .as_ref()
                        .ok_or_else(|| {
                            ApplicationError::command_failed("session artwork is unavailable")
                        })?
                        .artwork
                        .clone()
                };
                let artwork = repository.get_media_artwork(&item).await;
                if self.query_context()?.generation != generation {
                    return Err(ApplicationError::command_failed(
                        "session changed during artwork read",
                    ));
                }
                Ok(QueryResult::MediaArtwork(artwork))
            }
            RecentlyPlayedTracks { limit } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::RecentlyPlayed {
                        limit: limit.clamp(1, 100),
                    },
                    false,
                )
                .await
            }
            SearchMedia {
                query,
                media_type,
                limit,
            } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::Search {
                        query,
                        media_type,
                        limit: limit.clamp(1, 100),
                    },
                    false,
                )
                .await
            }
            Artist { item_id, provider } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::Artist {
                        item_id: MediaId(item_id),
                        provider,
                    },
                    false,
                )
                .await
            }
            Album { item_id, provider } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::Album {
                        item_id: MediaId(item_id),
                        provider,
                    },
                    false,
                )
                .await
            }
            AlbumTracks { item_id, provider } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::AlbumTracks {
                        item_id: MediaId(item_id),
                        provider,
                    },
                    false,
                )
                .await
            }
            Playlist { item_id, provider } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::Playlist {
                        item_id: MediaId(item_id),
                        provider,
                    },
                    false,
                )
                .await
            }
            PlaylistTracks {
                item_id,
                provider,
                force_refresh,
                allow_dynamic_tracks,
            } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::PlaylistTracks {
                        item_id: MediaId(item_id),
                        provider,
                        allow_dynamic_tracks,
                    },
                    force_refresh,
                )
                .await
            }
            ArtistAlbums { item_id, provider } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::ArtistAlbums {
                        item_id: MediaId(item_id),
                        provider,
                    },
                    false,
                )
                .await
            }
            ArtistTopTracks { item_id, provider } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::ArtistTopTracks {
                        item_id: MediaId(item_id),
                        provider,
                    },
                    false,
                )
                .await
            }
            SimilarArtists {
                item_id,
                provider,
                limit,
            } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::SimilarArtists {
                        item_id: MediaId(item_id),
                        provider,
                        limit: limit.clamp(1, 100),
                    },
                    false,
                )
                .await
            }
            SimilarTracks {
                item_id,
                provider,
                limit,
                allow_lookup,
            } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::SimilarTracks {
                        item_id: MediaId(item_id),
                        provider,
                        limit: limit.clamp(1, 100),
                        allow_lookup,
                    },
                    false,
                )
                .await
            }
            TrackVersions { item_id, provider } => {
                self.refresh_catalog(
                    generation,
                    CatalogQuery::TrackVersions {
                        item_id: MediaId(item_id),
                        provider,
                    },
                    false,
                )
                .await
            }
            GetSessionSettings {}
            | GetUpdateChannel { .. }
            | RefreshQueue {}
            | RefreshPlayerState {}
            | MediaSessionState {}
            | GetUserData {}
            | LoadUserData {}
            | StartSessionDataLoad {}
            | CancelQuery {} => {
                return Err(ApplicationError::internal("state query was not dispatched"))
            }
        };
        self.project_provider_availability(
            generation,
            result.map_err(ApplicationError::command_failed)?,
        )
        .await
    }

    // Observations publish cached data before remote completion while the request remains cancellable.
    async fn refresh_catalog(
        &self,
        generation: u64,
        query: CatalogQuery,
        force_refresh: bool,
    ) -> Result<QueryResult, String> {
        let repository = self
            .catalog_repository(generation)
            .map_err(|error| error.message)?;
        let is_recently_played = matches!(query, CatalogQuery::RecentlyPlayed { .. });
        let should_publish_observations = matches!(
            query,
            CatalogQuery::Artist { .. }
                | CatalogQuery::Album { .. }
                | CatalogQuery::AlbumTracks { .. }
                | CatalogQuery::PlaylistTracks { .. }
                | CatalogQuery::SimilarArtists { .. }
                | CatalogQuery::ArtistAlbums { .. }
                | CatalogQuery::ArtistTopTracks { .. }
                | CatalogQuery::ArtistTracks { .. }
                | CatalogQuery::ProviderItem {
                    media_type: crate::domain::media::MediaType::Artist
                        | crate::domain::media::MediaType::Album,
                    ..
                }
                | CatalogQuery::Search {
                    media_type: crate::domain::media::MediaType::Artist
                        | crate::domain::media::MediaType::Album,
                    ..
                }
        );
        let events = {
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| "core coordinator lock poisoned".to_owned())?;
            if coordinator.session_generation != generation {
                return Err("session changed before catalog observation".to_owned());
            }
            coordinator.session_events()
        };
        use crate::domain::catalog::CatalogLoadPhase;
        use futures_util::StreamExt;
        let mut observations = repository.observe_catalog(query.clone(), force_refresh);
        let mut completed = None;
        let mut published_value = None;
        while let Some(snapshot) = observations.next().await {
            if self
                .query_context()
                .map_err(|error| error.message)?
                .generation
                != generation
            {
                return Err("session changed during catalog observation".to_owned());
            }
            if should_publish_observations {
                if let Some(value) = snapshot.value.clone() {
                    if published_value.as_ref() != Some(&value) {
                        published_value = Some(value.clone());
                        events.publish(
                            crate::application::events::ApplicationEvent::CatalogUpdated {
                                query: query.clone(),
                                value,
                            },
                        );
                    }
                }
                events.publish(
                    crate::application::events::ApplicationEvent::CatalogReadChanged {
                        query: query.clone(),
                        snapshot: snapshot.clone(),
                    },
                );
            }
            match snapshot.phase {
                CatalogLoadPhase::Ready => {
                    completed = snapshot.value;
                    break;
                }
                CatalogLoadPhase::Failed | CatalogLoadPhase::Invalidated => {
                    return Err(snapshot
                        .error
                        .unwrap_or_else(|| "catalog refresh failed".to_owned()))
                }
                _ => {}
            }
        }
        let value =
            completed.ok_or_else(|| "catalog observation ended before completion".to_owned())?;
        if self
            .query_context()
            .map_err(|error| error.message)?
            .generation
            != generation
        {
            return Err("session changed during catalog request".into());
        }
        match value {
            CatalogValue::Artist(item) => Ok(QueryResult::Artist(item)),
            CatalogValue::Item(item) => Ok(QueryResult::MediaItems(vec![item])),
            CatalogValue::Album(item) => Ok(QueryResult::Album(item)),
            CatalogValue::Track(item) => Ok(QueryResult::MediaItems(vec![MediaItem::Track(item)])),
            CatalogValue::Playlist(item) => Ok(QueryResult::Playlist(item)),
            CatalogValue::Playlists(items) => Ok(QueryResult::MediaItems(
                items.into_iter().map(MediaItem::Playlist).collect(),
            )),
            CatalogValue::Tracks(items) if is_recently_played => Ok(QueryResult::MediaItems(
                items.into_iter().map(MediaItem::Track).collect(),
            )),
            CatalogValue::Tracks(items) => Ok(QueryResult::Tracks(items)),
            CatalogValue::Albums(items) => Ok(QueryResult::Albums(items)),
            CatalogValue::Artists(items) => Ok(QueryResult::Artists(items)),
            CatalogValue::Search(items) => Ok(QueryResult::SearchMedia(items)),
            CatalogValue::RecommendationItems(items) => Ok(QueryResult::MediaItems(items)),
            CatalogValue::RecommendationFolders(_) => {
                Err("recommendation folders require their view projection".into())
            }
        }
    }

    async fn project_provider_availability(
        &self,
        generation: u64,
        mut result: QueryResult,
    ) -> Result<QueryResult, ApplicationError> {
        if result.has_media() {
            let repository = self.provider_repository(generation)?;
            let providers = repository.get_instances().await.ok();
            if self.query_context()?.generation != generation {
                return Err(ApplicationError::command_failed(
                    "session changed during provider request",
                ));
            }
            result.project_availability(providers.as_deref());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod phase_e_tests {
    use super::*;
    use crate::application::events::EventPublisher;
    use crate::data::music_assistant::client::tests::server;
    use crate::data::music_assistant::connection::MusicAssistantConnection;
    use crate::domain::session::AccessToken;
    use serde_json::json;
    use std::sync::Arc;

    fn application(base_url: &str, events: EventPublisher) -> Application {
        let application = Application::new(
            crate::bootstrap::session::environment(),
            crate::application::tests::runtime().handle().clone(),
            events,
            Box::new(crate::application::tests::EmptySessionStorage),
        );
        application.coordinator.lock().unwrap().connection = Some(Arc::new(
            MusicAssistantConnection::new(
                MusicAssistantServerUrl(base_url.into()),
                Some(AccessToken("phase-e-test-token".into())),
            )
            .unwrap(),
        ));
        application
    }

    #[test]
    fn phase_e_cache_initialization_failure_preserves_online_reads_without_creating_cache() {
        let root = crate::bootstrap::session::application_data_directory().unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let (client, worker) = server(vec![
            ("auth/me", json!({"test_http_status":503})),
            ("auth/me", json!({"test_http_status":503})),
            (
                "music/recently_played_items",
                json!([{"media_type":"track","item_id":"first","provider":"test","name":"First","image":null,"available":true}]),
            ),
            ("providers", json!([])),
            (
                "music/recently_played_items",
                json!([{"media_type":"track","item_id":"second","provider":"test","name":"Second","image":null,"available":true}]),
            ),
            ("providers", json!([])),
        ]);
        let application = application(client.base_url(), EventPublisher::new(|_| {}));
        assert!(!root.join("Cache").exists());
        for expected in ["first", "second"] {
            let result = application
                .execute_query_sync(ApplicationQuery::RecentlyPlayedTracks { limit: 10 })
                .unwrap();
            let QueryResult::MediaItems(items) = result else {
                panic!("expected tracks");
            };
            assert_eq!(items[0].fields().item_id.as_str(), expected);
        }
        application.shutdown().unwrap();
        assert!(application
            .coordinator
            .lock()
            .unwrap()
            .repositories
            .is_none());
        assert!(!root.join("Cache").exists());
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 6);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["command"] == "auth/me")
                .count(),
            2
        );
    }

    #[test]
    fn phase_e_playlist_publishes_cached_tracks_before_delayed_server_response() {
        use crate::application::events::ApplicationEvent;
        use crate::domain::catalog::{CatalogDataSource, CatalogValue};
        use std::time::Duration;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let owner = Arc::new(crate::data::cache::catalog::CatalogCacheOwner::new(
            move || Ok(root.clone()),
        ));
        let shutdown_owner = owner.clone();
        let mut environment = crate::bootstrap::session::environment();
        environment.repositories = Arc::new(move |client, _server, user, generation| {
            let owner = owner.clone();
            Box::pin(async move {
                let mut services = crate::bootstrap::session::online_services(client.clone());
                services.catalog = Arc::new(owner.session(client, user, generation));
                Ok(services)
            })
        });
        environment.shutdown_repositories = Arc::new(move |generation| {
            let owner = shutdown_owner.clone();
            Box::pin(async move { owner.shutdown(generation).await })
        });
        let (entered, arrival) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut reads = 0;
        let (client, worker) =
            crate::data::music_assistant::client::tests::server_with_request_handler(
                vec![
                    (
                        "auth/me",
                        json!({"user_id":"user","username":"synthetic","role":"user","enabled":true}),
                    ),
                    ("info", json!({"server_id":"server","schema_version":42})),
                    (
                        "music/playlists/playlist_tracks",
                        json!([{"media_type":"track","provider":"source-a","item_id":"track","name":"Cached","position":1}]),
                    ),
                    ("providers", json!([])),
                    (
                        "music/playlists/playlist_tracks",
                        json!([{"media_type":"track","provider":"source-a","item_id":"track","name":"Server","position":4}]),
                    ),
                    ("providers", json!([])),
                ],
                move |request| {
                    if request["command"] == "music/playlists/playlist_tracks" {
                        assert_eq!(
                            request["args"],
                            json!({"item_id":"playlist","provider_instance_id_or_domain":"library","force_refresh":false,"allow_dynamic_tracks":false})
                        );
                        reads += 1;
                        if reads == 2 {
                            entered.send(()).unwrap();
                            gate.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                    }
                },
            );
        let (sink, events) = std::sync::mpsc::channel();
        let application = Arc::new(Application::new(
            environment,
            crate::application::tests::runtime().handle().clone(),
            EventPublisher::new(move |event| {
                sink.send(event).unwrap();
            }),
            Box::new(crate::application::tests::EmptySessionStorage),
        ));
        application.coordinator.lock().unwrap().connection = Some(Arc::new(
            MusicAssistantConnection::new(
                MusicAssistantServerUrl(client.base_url().into()),
                Some(AccessToken("phase-e-test-token".into())),
            )
            .unwrap(),
        ));
        let query = || ApplicationQuery::PlaylistTracks {
            item_id: "playlist".into(),
            provider: "library".into(),
            force_refresh: false,
            allow_dynamic_tracks: false,
        };
        assert!(matches!(
            application.execute_query_sync(query()).unwrap(),
            QueryResult::Tracks(_)
        ));
        while let Ok(event) = events.try_recv() {
            if let ApplicationEvent::CatalogReadChanged { snapshot, .. } = event {
                assert!(
                    snapshot.cache_error.is_none(),
                    "warmup cache error: {:?}",
                    snapshot.cache_error
                );
            }
        }
        let pending_application = application.clone();
        let pending = std::thread::spawn(move || pending_application.execute_query_sync(query()));
        arrival.recv_timeout(Duration::from_secs(2)).unwrap();
        loop {
            if let ApplicationEvent::CatalogReadChanged { snapshot, .. } =
                events.recv_timeout(Duration::from_secs(2)).unwrap()
            {
                if snapshot.source == Some(CatalogDataSource::Cache) {
                    let Some(CatalogValue::Tracks(tracks)) = snapshot.value else {
                        panic!("expected cached tracks")
                    };
                    assert_eq!(tracks[0].media.name, "Cached");
                    assert!(!pending.is_finished());
                    break;
                }
            }
        }
        release.send(()).unwrap();
        let QueryResult::Tracks(tracks) = pending.join().unwrap().unwrap() else {
            panic!("expected server tracks")
        };
        assert_eq!(tracks[0].media.name, "Server");
        application.shutdown().unwrap();
        assert_eq!(worker.join().unwrap().len(), 6);
    }

    #[test]
    fn phase_e_removed_queries_are_rejected_at_native_boundary() {
        use crate::adapters::models::NativeOperationRequest;
        for kind in [
            "imageData",
            "searchInventory",
            "cacheAssignments",
            "selectCacheAssignment",
        ] {
            assert!(
                serde_json::from_value::<NativeOperationRequest>(
                    json!({"requestId":71,"kind":kind})
                )
                .is_err(),
                "obsolete operation accepted: {kind}"
            );
        }
    }

    #[test]
    fn phase_e_online_read_requires_active_session_after_shutdown() {
        let application = application("http://127.0.0.1:1", EventPublisher::new(|_| {}));
        application.shutdown().unwrap();
        let result = application.execute_query_sync(ApplicationQuery::Artist {
            item_id: "one".into(),
            provider: "test".into(),
        });
        assert!(result.is_err());
        assert!(application
            .coordinator
            .lock()
            .unwrap()
            .repositories
            .is_none());
    }
}
