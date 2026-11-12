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
            ClearDatabaseCache {}
                | ClearImageCache {}
                | SetUserDataFavorites { .. }
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
            ClearDatabaseCache {} => {
                let context = self.query_context()?;
                let generation = context.generation;
                let catalog = self.catalog_repository(generation)?;
                self.runtime
                    .block_on(catalog.clear_database_cache())
                    .map_err(ApplicationError::command_failed)?;
                if self.query_context()?.generation != generation {
                    return Err(ApplicationError::command_failed(
                        "session changed during database cache maintenance",
                    ));
                }
                let application = self.clone();
                let _ = context.tasks.spawn(&self.runtime, async move {
                    if let Err(error) = application
                        .refresh_user_data_for_session(
                            Some(generation),
                            super::library::UserDataRefresh::All,
                        )
                        .await
                    {
                        crate::logging::Logger::new(module_path!()).debug(format_args!(
                            "User data refresh after cache maintenance failed: {}",
                            error.message
                        ));
                    }
                });
                vec![ApplicationEvent::CacheCleared {
                    cache: super::events::CacheArea::Database,
                }]
            }
            ClearImageCache {} => {
                let generation = self.query_context()?.generation;
                self.ensure_session_repositories_sync(generation)?;
                let images = {
                    let coordinator = self.coordinator.lock().map_err(|_| {
                        ApplicationError::internal("core coordinator lock poisoned")
                    })?;
                    if coordinator.session_generation != generation {
                        return Err(ApplicationError::command_failed(
                            "session changed before image cache maintenance",
                        ));
                    }
                    coordinator
                        .repositories
                        .as_ref()
                        .ok_or_else(|| {
                            ApplicationError::command_failed("session images are unavailable")
                        })?
                        .images
                        .clone()
                };
                self.runtime
                    .block_on(images.clear_image_cache())
                    .map_err(ApplicationError::command_failed)?;
                if self.query_context()?.generation != generation {
                    return Err(ApplicationError::command_failed(
                        "session changed during image cache maintenance",
                    ));
                }
                vec![ApplicationEvent::CacheCleared {
                    cache: super::events::CacheArea::Images,
                }]
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
        use super::repositories::with_request_priority;
        let priority = query.request_priority();
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
            GetCacheUsage {} => self.catalog_repository(generation)?.cache_usage().await.map(QueryResult::CacheUsage),
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
                            crate::application::events::ApplicationEvent::CatalogReadChanged { query, snapshot: (&snapshot).into() },
                        );
                    })
                    .await
                    .map(QueryResult::Recommendations)
            }
            ProviderManifests {} => {
                self.refresh_catalog(generation, CatalogQuery::ProviderManifests {}, false)
                    .await
            }
            ProviderIconData { provider } => self
                .provider_repository(generation)?
                .get_icon_data(&provider)
                .await
                .map(QueryResult::ProviderIconData),
            query @ (ReadImage { .. } | ObserveImage { .. }) => {
                let (request, refresh, observation_id) = match query {
                    ReadImage { request, refresh } => (request, refresh, None),
                    ObserveImage { request, observation_id, .. } => (request, true, Some(observation_id)),
                    _ => unreachable!(),
                };
                let images = {
                    let coordinator = self.coordinator.lock().map_err(|_| {
                        ApplicationError::internal("core coordinator lock poisoned")
                    })?;
                    if coordinator.session_generation != generation {
                        return Err(ApplicationError::command_failed(
                            "session changed before image read",
                        ));
                    }
                    coordinator
                        .repositories
                        .as_ref()
                        .ok_or_else(|| {
                            ApplicationError::command_failed("session images are unavailable")
                        })?
                        .images
                        .clone()
                };
                let snapshot = if let Some(observation_id) = observation_id {
                    let events = self.coordinator.lock().map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?.session_events();
                    super::queries::ImageReadSnapshot::observe(images.as_ref(), &request, |snapshot| {
                        events.publish(ApplicationEvent::ImageReadUpdated { observation_id, snapshot });
                    }).await?
                } else {
                    super::queries::ImageReadSnapshot::load(images.as_ref(), &request, refresh).await?
                };
                if snapshot.did_publish_change {
                    if let Some(proxy_id) = request.image.proxy_id.filter(|proxy_id| {
                        proxy_id.len() == 64
                            && proxy_id.bytes().all(|value| value.is_ascii_hexdigit())
                    }) {
                        let events = {
                            let coordinator = self.coordinator.lock().map_err(|_| {
                                ApplicationError::internal("core coordinator lock poisoned")
                            })?;
                            if coordinator.session_generation != generation {
                                return Err(ApplicationError::command_failed(
                                    "session changed during image publication",
                                ));
                            }
                            coordinator.session_events()
                        };
                        events.publish(ApplicationEvent::ImageSourceChanged { proxy_id });
                    }
                }
                Ok(QueryResult::ImageRead(snapshot))
            }
            MediaArtwork { item } => {
                let artwork = self.resolve_media_artwork(generation, &item).await?;
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
            ArtistAlbums {
                item_id,
                provider,
                ..
            } => {
                with_request_priority(
                    priority,
                    self.refresh_catalog(
                        generation,
                        CatalogQuery::ArtistAlbums {
                            item_id: MediaId(item_id),
                            provider,
                        },
                        false,
                    ),
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
        with_request_priority(
            priority,
            self.project_provider_availability(
                generation,
                result.map_err(ApplicationError::command_failed)?,
            ),
        )
        .await
    }

    // Mapping reads share the catalog cache and publish early metadata to visible artwork consumers.
    async fn resolve_media_artwork(
        &self,
        generation: u64,
        item: &MediaItem,
    ) -> Result<Option<crate::domain::media::MediaItemImage>, String> {
        use futures_util::{stream::FuturesOrdered, StreamExt};
        if let Some(image) = crate::domain::artwork::select_artwork(item) {
            return Ok(Some(image));
        }
        let media_type = item.media_type();
        let mut reads = FuturesOrdered::new();
        for mapping in &item.fields().provider_mappings {
            reads.push_back(self.refresh_catalog(
                generation,
                CatalogQuery::ProviderItem {
                    item_id: mapping.item_id.clone(),
                    provider: mapping.provider_instance.as_str().to_owned(),
                    media_type,
                },
                false,
            ));
        }
        while let Some(result) = reads.next().await {
            if let Ok(QueryResult::MediaItems(items)) = result {
                if let Some(image) = items
                    .iter()
                    .find_map(crate::domain::artwork::select_artwork)
                {
                    return Ok(Some(image));
                }
            }
        }
        Ok(None)
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
        let (events, publication) = {
            let mut coordinator = self
                .coordinator
                .lock()
                .map_err(|_| "core coordinator lock poisoned".to_owned())?;
            if coordinator.session_generation != generation {
                return Err("session changed before catalog observation".to_owned());
            }
            (coordinator.session_events(), coordinator.catalog_publication(&query))
        };
        use crate::domain::catalog::CatalogLoadPhase;
        use futures_util::StreamExt;
        let started = std::time::Instant::now();
        static NEXT_DISPATCH_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let dispatch_id = NEXT_DISPATCH_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let logger = crate::logging::Logger::new("Dispatcher.Catalog");
        let mut first_data_ms = None;
        logger.trace(format_args!("dispatch_id={dispatch_id} query={} stage=started force_refresh={force_refresh}", query.kind_name()));
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
            if let Some(value) = snapshot.value.clone() {
                if !published_value.as_ref().is_some_and(|previous| std::sync::Arc::ptr_eq(previous, &value) || previous == &value) {
                    let elapsed_ms = started.elapsed().as_millis();
                    if first_data_ms.is_none() {
                        first_data_ms = Some(elapsed_ms);
                        logger.trace(format_args!("dispatch_id={dispatch_id} query={} stage=first_data source={:?} knowledge={:?} elapsed_ms={elapsed_ms}", query.kind_name(), snapshot.source, snapshot.knowledge));
                    } else {
                        logger.trace(format_args!("dispatch_id={dispatch_id} query={} stage=updated source={:?} elapsed_ms={elapsed_ms}", query.kind_name(), snapshot.source));
                    }
                    let publication_value = publication.value_for_publication(snapshot.publication_revision, &value, published_value.is_none());
                    published_value = Some(value.clone());
                    if let Some(value) = publication_value { events.publish(
                        crate::application::events::ApplicationEvent::CatalogUpdated {
                            query: query.clone(),
                            value: std::sync::Arc::unwrap_or_clone(value),
                        },
                    ); }
                }
            }
            events.publish(
                crate::application::events::ApplicationEvent::CatalogReadChanged {
                    query: query.clone(),
                    snapshot: (&snapshot).into(),
                },
            );
            match snapshot.phase {
                CatalogLoadPhase::Ready => {
                    logger.trace(format_args!("dispatch_id={dispatch_id} query={} stage=finished source={:?} first_data_ms={first_data_ms:?} total_ms={}", query.kind_name(), snapshot.source, started.elapsed().as_millis()));
                    completed = snapshot.value;
                    break;
                }
                CatalogLoadPhase::Failed | CatalogLoadPhase::Invalidated => {
                    logger.trace(format_args!("dispatch_id={dispatch_id} query={} stage=finished phase={:?} had_data={} total_ms={}", query.kind_name(), snapshot.phase, first_data_ms.is_some(), started.elapsed().as_millis()));
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
        match std::sync::Arc::unwrap_or_clone(value) {
            CatalogValue::ProviderManifests(manifests) => {
                Ok(QueryResult::ProviderManifests(manifests))
            }
            CatalogValue::ProviderInstances(_) => {
                Err("provider instances require their provider projection".into())
            }
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
            let context = self.query_context()?;
            if context.generation != generation {
                return Err(ApplicationError::command_failed(
                    "session changed before provider read",
                ));
            }
            let (events, availability) = {
                let coordinator = self
                    .coordinator
                    .lock()
                    .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
                if coordinator.session_generation != generation {
                    return Err(ApplicationError::command_failed(
                        "session changed before provider read",
                    ));
                }
                (coordinator.session_events(), coordinator.provider_availability.clone())
            };
            let providers = match repository.read_instances().await {
                Ok(read) => {
                    if let Some(error) = read.current.cache_error {
                        events.publish(crate::application::events::ApplicationEvent::Error {
                            error: ApplicationError::command_failed(error),
                        });
                    }
                    let current = read.current.items;
                    availability.replace(current.clone());
                    if let (Some(refresh), Some(refresh_guard)) =
                        (read.refresh, context.tasks.try_lock_provider_refresh())
                    {
                        let previous = current.clone();
                        context.tasks.spawn(&self.runtime, async move {
                            let _refresh_guard = refresh_guard;
                            match refresh.await {
                                Ok(update) => {
                                    availability.replace(update.items.clone());
                                    if let Some(error) = update.cache_error {
                                        events.publish(crate::application::events::ApplicationEvent::Error { error: ApplicationError::command_failed(error) });
                                    }
                                    if update.items != previous {
                                        events.publish(crate::application::events::ApplicationEvent::ProviderInstancesChanged);
                                        events.publish(crate::application::events::ApplicationEvent::UserDataChanged { playlist: None, revision: None });
                                        events.publish(crate::application::events::ApplicationEvent::RecommendationsChanged);
                                    }
                                }
                                Err(crate::application::repositories::ProviderRefreshError::Failed(error)) => events.publish(crate::application::events::ApplicationEvent::Error { error: ApplicationError::command_failed(error) }),
                                Err(crate::application::repositories::ProviderRefreshError::Invalidated(_)) => {}
                            }
                        })?;
                    }
                    current
                }
                Err(_) => None,
            };
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

    struct DelayedProviders {
        release: Arc<tokio::sync::Notify>,
        outcome: usize,
    }

    impl crate::application::repositories::ProviderClient for DelayedProviders {
        fn read_manifests(
            &self,
        ) -> futures_util::future::BoxFuture<
            '_,
            Result<
                crate::application::repositories::ProviderRead<
                    crate::domain::providers::ProviderManifest,
                >,
                String,
            >,
        > {
            Box::pin(async { Err("unexpected manifest read".into()) })
        }

        fn read_instances(
            &self,
        ) -> futures_util::future::BoxFuture<
            '_,
            Result<
                crate::application::repositories::ProviderRead<
                    crate::domain::providers::ProviderInstance,
                >,
                String,
            >,
        > {
            use crate::application::repositories::{
                ProviderRead, ProviderRefreshError, ProviderUpdate,
            };
            let release = self.release.clone();
            let outcome = self.outcome;
            Box::pin(async move {
                let instances = vec![crate::domain::providers::ProviderInstance {
                    instance_id: crate::domain::identifiers::ProviderInstanceId("source".into()),
                    available: true,
                }];
                let previous = instances.clone();
                Ok(ProviderRead {
                    current: ProviderUpdate {
                        items: if outcome == 4 { None } else { Some(instances) },
                        cache_error: None,
                    },
                    refresh: Some(Box::pin(async move {
                        release.notified().await;
                        match outcome {
                            0 => Ok(ProviderUpdate {
                                items: Some(previous),
                                cache_error: None,
                            }),
                            1 | 4 | 5 => Ok(ProviderUpdate {
                                items: Some(Vec::new()),
                                cache_error: None,
                            }),
                            2 => Err(ProviderRefreshError::Failed("offline".into())),
                            _ => Err(ProviderRefreshError::Invalidated("retired".into())),
                        }
                    })),
                })
            })
        }

        fn get_icon_data<'request>(
            &'request self,
            _provider: &'request str,
        ) -> futures_util::future::BoxFuture<'request, Result<Option<String>, String>> {
            Box::pin(async { Err("unexpected icon read".into()) })
        }
    }

    #[tokio::test]
    async fn provider_projection_returns_cache_before_refresh_and_notifies_only_changed_instances()
    {
        for outcome in 0..6 {
            let (client, worker) = server(Vec::new());
            let events = Arc::new(std::sync::Mutex::new(Vec::new()));
            let published = events.clone();
            let application = application(
                client.base_url(),
                EventPublisher::new(move |event| published.lock().unwrap().push(event)),
            );
            let release = Arc::new(tokio::sync::Notify::new());
            let mut services = crate::bootstrap::session::online_services(Arc::new(client));
            services.providers = Arc::new(DelayedProviders {
                release: release.clone(),
                outcome,
            });
            application.coordinator.lock().unwrap().repositories = Some(services);
            let context = application.query_context().unwrap();
            for _ in 0..2 {
                let tracks = serde_json::from_value(json!([{
                    "item_id":"track", "provider":"library", "name":"Cached", "provider_mappings":[{
                        "item_id":"track", "provider_domain":"test", "provider_instance":"source", "available":true
                    }]
                }])).unwrap();
                let projected = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    application.project_provider_availability(
                        context.generation,
                        QueryResult::Tracks(tracks),
                    ),
                )
                .await
                .unwrap()
                .unwrap();
                let QueryResult::Tracks(tracks) = projected else {
                    panic!("expected tracks")
                };
                assert_eq!(tracks[0].media.available, Some(true));
            }
            assert!(events.lock().unwrap().is_empty());
            if outcome == 5 {
                context.tasks.close();
            }
            release.notify_one();
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while context.tasks.try_lock_provider_refresh().is_none() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let events = events.lock().unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, ApplicationEvent::ProviderInstancesChanged))
                    .count(),
                usize::from(matches!(outcome, 1 | 4))
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, ApplicationEvent::Error { .. }))
                    .count(),
                usize::from(outcome == 2)
            );
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn provider_completion_preserves_cached_failure_but_rejects_invalidated_session() {
        use crate::application::repositories::ProviderClient;
        for outcome in [2, 3] {
            let release = Arc::new(tokio::sync::Notify::new());
            let providers = DelayedProviders {
                release: release.clone(),
                outcome,
            };
            let read = providers.read_instances().await.unwrap();
            release.notify_one();
            let result = read.resolve().await;
            assert_eq!(result.is_ok(), outcome == 2);
        }
    }

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
        assert_eq!(requests.len(), 8);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["command"] == "auth/me")
                .count(),
            4
        );
    }

    #[test]
    fn phase_e_playlist_publishes_cached_tracks_before_delayed_server_response() {
        assert_cached_catalog_publication(
            json!({"kind":"playlistTracks","itemId":"playlist","provider":"library"}),
            "music/playlists/playlist_tracks",
            json!({"item_id":"playlist","provider_instance_id_or_domain":"library","force_refresh":false,"allow_dynamic_tracks":false,"limit":0}),
            json!([{"media_type":"track","provider":"source-a","item_id":"track","name":"Cached","position":1}]),
            json!([{"media_type":"track","provider":"source-a","item_id":"track","name":"Server","position":4}]),
        );
    }

    #[test]
    fn catalog_searches_and_playlist_headers_publish_cache_before_remote_completion() {
        assert_cached_catalog_publication(
            json!({"kind":"playlist","itemId":"playlist","provider":"library"}),
            "music/playlists/get",
            json!({"item_id":"playlist","provider_instance_id_or_domain":"library"}),
            json!({"media_type":"playlist","provider":"library","item_id":"playlist","name":"Cached"}),
            json!({"media_type":"playlist","provider":"library","item_id":"playlist","name":"Server"}),
        );
        for (media_type, field) in [("track", "tracks"), ("playlist", "playlists")] {
            assert_cached_catalog_publication(
                json!({"kind":"searchMedia","query":"query","mediaType":media_type,"limit":10}),
                "music/search",
                json!({"search_query":"query","media_types":[media_type],"limit":10,"library_only":false}),
                json!({field:[{"media_type":media_type,"provider":"source-a","item_id":"item","name":"Cached"}]}),
                json!({field:[{"media_type":media_type,"provider":"source-a","item_id":"item","name":"Server"}]}),
            );
        }
    }

    #[test]
    fn artwork_mappings_publish_cached_metadata_before_remote_completion() {
        assert_cached_catalog_publication(
            json!({"kind":"mediaArtwork","item":{
                "media_type":"artist","item_id":"canonical","provider":"library","name":"Artist",
                "provider_mappings":[{"item_id":"mapped","provider_domain":"source","provider_instance":"source--one"}]
            }}),
            "music/item",
            json!({"media_type":"artist","item_id":"mapped","provider_instance_id_or_domain":"source--one","allow_update_metadata":false}),
            json!({"media_type":"artist","item_id":"mapped","provider":"source--one","name":"Cached",
                "metadata":{"images":[{"type":"thumb","provider":"source","path":"Cached"}]}}),
            json!({"media_type":"artist","item_id":"mapped","provider":"source--one","name":"Server",
                "metadata":{"images":[{"type":"thumb","provider":"source","path":"Server"}]}}),
        );
    }

    struct ArtworkMappings {
        started: Arc<std::sync::Mutex<Vec<CatalogQuery>>>,
        dropped: Arc<std::sync::atomic::AtomicUsize>,
    }

    struct ArtworkReadGuard(Arc<std::sync::atomic::AtomicUsize>);

    impl Drop for ArtworkReadGuard {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl crate::application::repositories::CatalogClient for ArtworkMappings {
        fn begin_startup(&self) -> Box<dyn Send> {
            Box::new(())
        }

        fn load_catalog(
            &self,
            _: CatalogQuery,
            _: bool,
        ) -> futures_util::future::BoxFuture<'_, Result<CatalogValue, String>> {
            Box::pin(async { panic!("artwork must observe the existing catalog") })
        }

        fn observe_catalog(
            &self,
            query: CatalogQuery,
            _: bool,
        ) -> futures_util::stream::BoxStream<'_, crate::domain::catalog::CatalogLoadSnapshot>
        {
            use crate::domain::catalog::{
                CatalogDataSource, CatalogFreshness, CatalogLoadPhase, CatalogLoadSnapshot,
                CatalogReadKnowledge,
            };
            use futures_util::StreamExt;
            self.started.lock().unwrap().push(query.clone());
            let CatalogQuery::ProviderItem {
                item_id,
                provider,
                media_type,
            } = query
            else {
                panic!("expected mapping")
            };
            assert_eq!(media_type, crate::domain::media::MediaType::Artist);
            let item = serde_json::from_value(json!({"media_type":"artist","item_id":item_id,
                "provider":provider,"name":"Cached","metadata":{"images":[{"type":"thumb","provider":"source","path":"Cached"}]}})).unwrap();
            let snapshot = CatalogLoadSnapshot {
                publication_revision: 0,
                value: Some(std::sync::Arc::new(CatalogValue::Item(item))),
                source: Some(CatalogDataSource::Cache),
                cache_error: None,
                phase: CatalogLoadPhase::Refreshing,
                knowledge: CatalogReadKnowledge::ObservedResponse,
                freshness: CatalogFreshness::Stale,
                observed_at: None,
                error: None,
            };
            let guard = ArtworkReadGuard(self.dropped.clone());
            Box::pin(
                futures_util::stream::once(async move { snapshot })
                    .chain(futures_util::stream::pending())
                    .inspect(move |_| {
                        let _ = &guard;
                    }),
            )
        }
    }

    #[test]
    fn artwork_opens_every_mapping_before_refresh_and_drop_releases_every_observer() {
        use futures_util::FutureExt;
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let published = events.clone();
        let application = application(
            "http://127.0.0.1:1",
            EventPublisher::new(move |event| published.lock().unwrap().push(event)),
        );
        let started = Arc::new(std::sync::Mutex::new(Vec::new()));
        let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = crate::data::music_assistant::client::MusicAssistantClient::new(
            "http://127.0.0.1:1",
            Some(crate::domain::session::AccessToken("synthetic".into())),
        );
        let mut services = crate::bootstrap::session::online_services(Arc::new(client));
        services.catalog = Arc::new(ArtworkMappings {
            started: started.clone(),
            dropped: dropped.clone(),
        });
        application.coordinator.lock().unwrap().repositories = Some(services);
        let item = serde_json::from_value(json!({"media_type":"artist","item_id":"canonical","provider":"library","name":"Artist",
            "provider_mappings": (0..5).map(|id| json!({"item_id":format!("mapped-{id}"),"provider_domain":"source","provider_instance":"source--one"})).collect::<Vec<_>>()
        })).unwrap();
        let context = application.query_context().unwrap();
        let result = crate::application::tests::runtime().block_on(async {
            application
                .resolve_media_artwork(context.generation, &item)
                .now_or_never()
        });
        assert!(result.is_none());
        assert_eq!(started.lock().unwrap().len(), 5);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| matches!(event, ApplicationEvent::CatalogUpdated { .. }))
                .count(),
            5
        );
        assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 5);
    }

    fn assert_cached_catalog_publication(
        request: serde_json::Value,
        command: &'static str,
        arguments: serde_json::Value,
        cached_response: serde_json::Value,
        server_response: serde_json::Value,
    ) {
        use crate::application::events::ApplicationEvent;
        use crate::domain::catalog::{CatalogDataSource, CatalogValue};
        use std::time::Duration;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let owner = Arc::new(crate::data::cache::catalog::SessionCacheOwner::new(
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
        let is_artwork = request["kind"] == "mediaArtwork";
        let mut responses = vec![
            ("auth/me", json!({"user_id":"user","username":"synthetic","role":"user","enabled":true})),
            ("info", json!({"server_id":"server","schema_version":42})),
            (command, cached_response),
        ];
        if !is_artwork { responses.push(("providers", json!([]))); }
        responses.push((command, server_response));
        if !is_artwork { responses.push(("providers", json!([]))); }
        let (client, worker) =
            crate::data::music_assistant::client::tests::server_with_request_handler(
                responses,
                move |request| {
                    if request["command"] == command {
                        assert_eq!(request["args"], arguments);
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
        let query = || serde_json::from_value::<ApplicationQuery>(request.clone()).unwrap();
        application.execute_query_sync(query()).unwrap();
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
        let pending_query = query();
        let pending =
            std::thread::spawn(move || pending_application.execute_query_sync(pending_query));
        arrival.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut published_cache = false;
        loop {
            match events.recv_timeout(Duration::from_secs(2)).unwrap() {
                ApplicationEvent::CatalogUpdated { value, .. } => {
                    let name = match &value {
                        CatalogValue::Tracks(items) => &items[0].media.name,
                        CatalogValue::Playlist(item) => &item.media.name,
                        CatalogValue::Item(item) => &item.fields().name,
                        CatalogValue::Search(items) if !items.tracks.is_empty() => {
                            &items.tracks[0].media.name
                        }
                        CatalogValue::Search(items) => &items.playlists[0].media.name,
                        _ => panic!("unexpected cached value"),
                    };
                    assert_eq!(name, "Cached");
                    published_cache = true;
                }
                ApplicationEvent::CatalogReadChanged { snapshot, .. }
                    if snapshot.source == Some(CatalogDataSource::Cache) =>
                {
                    assert!(published_cache);
                    assert!(!pending.is_finished());
                    break;
                }
                _ => {}
            }
        }
        release.send(()).unwrap();
        let result = pending.join().unwrap().unwrap();
        let name = match &result {
            QueryResult::Tracks(items) => &items[0].media.name,
            QueryResult::Playlist(item) => &item.media.name,
            QueryResult::MediaArtwork(Some(image)) => &image.path,
            QueryResult::SearchMedia(items) if !items.tracks.is_empty() => {
                &items.tracks[0].media.name
            }
            QueryResult::SearchMedia(items) => &items.playlists[0].media.name,
            _ => panic!("unexpected server value"),
        };
        assert_eq!(name, "Server");
        application.shutdown().unwrap();
        assert_eq!(worker.join().unwrap().len(), if is_artwork { 4 } else { 6 });
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
