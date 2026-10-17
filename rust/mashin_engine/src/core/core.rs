//! Process-wide application core that owns session and playback state.

use super::config::{session_settings, SessionSettings};
use super::models::{
    CoreError, MediaSessionEvent, MusicAssistantQuery, MusicAssistantQueryRequest, SessionConfig,
};
use crate::logging::Logger;
use crate::music_assistant::auth_store::{platform_auth_store, AuthStore};
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::identifiers::MediaId;
use crate::music_assistant::models::media::{
    Album, Artist, MediaItem, MediaType, MusicAssistantSearchResults, Playlist, Track,
};
use crate::music_assistant::models::recommendation::MusicAssistantRecommendationFolder;
use crate::music_assistant::models::session::ProviderManifest;
use crate::music_assistant::playlist_collage::build_playlist_collage;
use crate::music_assistant::user_data::{
    flush_scheduled_write_after_debounce, persist_write_request, UserDataController,
    UserDataSnapshot, UserDataWriteRequest,
};
use crate::playback::engine::PlaybackEngine;
use crate::playback::models::{MediaSessionState, PlayerState, Queue};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

#[path = "session.rs"]
mod session;
#[path = "settings.rs"]
mod settings;

pub(crate) static CORE_COORDINATOR: Lazy<Mutex<CoreCoordinator>> =
    Lazy::new(|| Mutex::new(CoreCoordinator::new()));
static CORE_COMMAND_SERIALIZER: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static CORE_USER_DATA_LOAD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static LOGGER: Logger = Logger::new(module_path!());
pub(crate) static CORE_NETWORK_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("create core network runtime")
});

/// Schedules blocking FFI-originated work outside the Flutter UI thread.
pub(crate) fn spawn_blocking_operation<F>(operation: F)
where
    F: FnOnce() + Send + 'static,
{
    CORE_NETWORK_RUNTIME.spawn_blocking(operation);
}

pub(crate) struct CoreCoordinator {
    auth_store: Box<dyn AuthStore + Send>,
    playback: PlaybackEngine,
    session_configuration: Option<SessionConfig>,
    user_data: Option<UserDataController>,
    user_data_session_generation: u64,
    connection:
        Option<std::sync::Arc<crate::music_assistant::connection::MusicAssistantConnection>>,
    pending_commands: Vec<crate::playback::backend::PlaybackCommand>,
}

impl CoreCoordinator {
    // Construction and session configuration.
    pub(crate) fn new() -> Self {
        Self::with_auth_store(platform_auth_store())
    }

    fn with_auth_store(auth_store: Box<dyn AuthStore + Send>) -> Self {
        Self {
            auth_store,
            playback: PlaybackEngine::new(),
            session_configuration: None,
            user_data: None,
            user_data_session_generation: 0,
            connection: None,
            pending_commands: Vec::new(),
        }
    }

    // Native shutdown removes all in-memory session data after extracting the final pending write.
    pub(crate) fn shutdown(&mut self) -> Option<UserDataWriteRequest> {
        let pending_write = self.pending_user_data_write();
        self.playback.shutdown();
        self.pending_commands.clear();
        self.connection = None;
        self.session_configuration = None;
        self.invalidate_user_data();
        pending_write
    }

    // Platform state reads project the active engine without storing duplicate core state.
    pub(crate) fn media_session_state(&self) -> MediaSessionState {
        self.playback.media_session_state()
    }

    // Pending work is extracted before releasing the application lock.
    fn take_pending_commands(
        &mut self,
    ) -> Result<Vec<crate::playback::backend::PlaybackCommand>, String> {
        let mut commands = std::mem::take(&mut self.pending_commands);
        commands.extend(self.playback.take_pending_commands()?);
        Ok(commands)
    }

    // User-data state.
    fn replace_user_data(&mut self, user_data: UserDataController) {
        self.invalidate_user_data();
        self.user_data = Some(user_data);
    }

    fn replace_user_data_for_session(
        &mut self,
        session_generation: u64,
        user_data: UserDataController,
    ) -> Result<(), String> {
        if self.user_data_session_generation != session_generation {
            return Err("session changed while loading user data".to_string());
        }
        self.replace_user_data(user_data);
        Ok(())
    }

    fn invalidate_user_data(&mut self) {
        self.user_data = None;
        self.user_data_session_generation = self.user_data_session_generation.wrapping_add(1);
    }

    fn pending_user_data_write(&self) -> Option<UserDataWriteRequest> {
        let session = self.session_configuration.as_ref()?.clone();
        let (revision, preferences) = self.user_data.as_ref()?.pending_write()?;
        Some(UserDataWriteRequest {
            revision,
            username: session.username,
            preferences,
            client: MusicAssistantClient::new(&session.music_assistant_url.0, session.access_token),
        })
    }

    fn user_data_snapshot(&self) -> Result<UserDataSnapshot, String> {
        self.user_data
            .as_ref()
            .map(UserDataController::snapshot)
            .ok_or_else(|| "user data is not loaded".to_string())
    }

    // Commit only validated candidates; failed batches leave active user data untouched.
    fn mutate_user_data<F>(
        &mut self,
        operation: F,
    ) -> Result<serde_json::Map<String, serde_json::Value>, String>
    where
        F: FnOnce(&mut UserDataController) -> Result<(), String>,
    {
        let mut candidate = self
            .user_data
            .as_ref()
            .ok_or_else(|| "user data is not loaded".to_string())?
            .clone();
        operation(&mut candidate)?;
        let preferences = candidate.preferences()?;
        self.user_data = Some(candidate);
        Ok(preferences)
    }
}

// Playback commands share application serialization and events without duplicating the engine API.
pub(crate) fn execute_playback_command_and_collect_events<F>(
    operation: F,
) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut PlaybackEngine) -> Result<(), String>,
{
    execute_command_and_collect_events(|coordinator| operation(&mut coordinator.playback))
}

// Commands publish only the projections whose values changed under the command lock.
pub(crate) fn execute_command_and_collect_events<F>(
    operation: F,
) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut CoreCoordinator) -> Result<(), String>,
{
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let queue_before = coordinator.playback.queue();
    let player_state_before = coordinator.playback.player_state();
    let session_generation = coordinator.user_data_session_generation;
    let result = operation(&mut coordinator);
    let commands = coordinator
        .take_pending_commands()
        .map_err(CoreError::command_failed)?;
    result.map_err(CoreError::command_failed)?;
    let should_load_user_data = coordinator.user_data_session_generation != session_generation
        && coordinator.session_configuration.is_some();
    drop(coordinator);
    for command in commands {
        CORE_NETWORK_RUNTIME
            .block_on(command)
            .map_err(CoreError::command_failed)?;
    }
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let events = collect_media_session_events(&coordinator, queue_before, player_state_before);
    drop(coordinator);
    drop(_command_guard);
    if should_load_user_data {
        start_user_data_load();
    }
    Ok(events)
}

/// Stops native work after attempting the final coalesced UserData write for a bounded duration.
pub(crate) fn shutdown() -> Result<(), CoreError> {
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let pending_write = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .shutdown();

    if let Some(write_request) = pending_write {
        // An unavailable server must not prevent native shutdown.
        let _ = CORE_NETWORK_RUNTIME.block_on(tokio::time::timeout(
            Duration::from_secs(3),
            persist_write_request(write_request),
        ));
    }
    Ok(())
}

pub(crate) fn restore_persisted_session_and_collect_events(
) -> Result<(bool, Vec<MediaSessionEvent>), CoreError> {
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let queue_before = coordinator.playback.queue();
    let player_state_before = coordinator.playback.player_state();
    let has_restored_session = coordinator
        .restore_persisted_session()
        .map_err(CoreError::command_failed)?;
    let should_load_user_data = has_restored_session && coordinator.user_data.is_none();
    let events = collect_media_session_events(&coordinator, queue_before, player_state_before);
    drop(coordinator);
    drop(_command_guard);
    if should_load_user_data {
        start_user_data_load();
    }
    Ok((has_restored_session, events))
}

fn start_user_data_load() {
    spawn_blocking_operation(|| {
        if let Err(error) = read_or_load_user_data() {
            LOGGER.warn(format!("load user data failed: {}", error.message));
        }
    });
}

// Client-owned playlists are expanded; MA owns every other playback decision.
pub(crate) fn resolve_media_and_execute_command<F>(
    media_items: Vec<MediaItem>,
    operation: F,
) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut PlaybackEngine, Vec<MediaItem>) -> Result<(), String>,
{
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let user_data = if media_items.iter().any(is_local_playlist) {
        Some(read_or_load_user_data()?)
    } else {
        None
    };
    let resolved_media_items = expand_local_playlist_media(media_items, user_data.as_ref())
        .map_err(CoreError::command_failed)?;
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let queue_before = coordinator.playback.queue();
    let player_state_before = coordinator.playback.player_state();
    let result = operation(&mut coordinator.playback, resolved_media_items);
    let commands = coordinator
        .take_pending_commands()
        .map_err(CoreError::command_failed)?;
    result.map_err(CoreError::command_failed)?;
    drop(coordinator);
    for command in commands {
        CORE_NETWORK_RUNTIME
            .block_on(command)
            .map_err(CoreError::command_failed)?;
    }
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    Ok(collect_media_session_events(
        &coordinator,
        queue_before,
        player_state_before,
    ))
}

fn is_local_playlist(item: &MediaItem) -> bool {
    matches!(item, MediaItem::Playlist(playlist) if playlist.media.provider.as_str() == crate::music_assistant::user_data::LOCAL_PLAYLIST_PROVIDER)
}

fn expand_local_playlist_media(
    media_items: Vec<MediaItem>,
    user_data: Option<&UserDataSnapshot>,
) -> Result<Vec<MediaItem>, String> {
    let mut expanded = Vec::new();
    for item in media_items {
        if is_local_playlist(&item) {
            let playlist = user_data
                .and_then(|data| {
                    data.playlists
                        .iter()
                        .find(|playlist| playlist.playlist.media.item_id == item.fields().item_id)
                })
                .ok_or("local playlist is no longer available")?;
            expanded.extend(playlist.items.iter().cloned().map(MediaItem::Track));
        } else {
            expanded.push(item);
        }
    }
    Ok(expanded)
}

// Music Assistant queries.
fn music_assistant_client_for_query(operation: &str) -> Result<MusicAssistantClient, CoreError> {
    let session = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .session_configuration
        .clone()
        .ok_or_else(|| {
            CoreError::command_failed(format!("a restored session is required to {operation}"))
        })?;
    Ok(MusicAssistantClient::new(
        &session.music_assistant_url.0,
        session.access_token,
    ))
}

// User-data queries and delayed persistence keep Music Assistant I/O outside the coordinator lock.
fn load_user_data() -> Result<UserDataSnapshot, CoreError> {
    let (client, session_generation) = {
        let coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        let session = coordinator.session_configuration.as_ref().ok_or_else(|| {
            CoreError::command_failed("a restored session is required to load user data")
        })?;
        (
            MusicAssistantClient::new(&session.music_assistant_url.0, session.access_token.clone()),
            coordinator.user_data_session_generation,
        )
    };
    let user = CORE_NETWORK_RUNTIME
        .block_on(client.get_current_user())
        .map_err(CoreError::command_failed)?;
    let user_data = UserDataController::from_preferences(user.preferences)
        .map_err(CoreError::command_failed)?;
    let snapshot = user_data.snapshot();
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    coordinator
        .replace_user_data_for_session(session_generation, user_data)
        .map_err(CoreError::command_failed)?;
    Ok(snapshot)
}

pub(crate) fn read_user_data() -> Result<UserDataSnapshot, CoreError> {
    CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .user_data_snapshot()
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_or_load_user_data() -> Result<UserDataSnapshot, CoreError> {
    let _load_guard = CORE_USER_DATA_LOAD
        .lock()
        .map_err(|_| CoreError::internal("user data load lock poisoned"))?;
    let user_data = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .user_data
        .as_ref()
        .map(UserDataController::snapshot);
    match user_data {
        Some(snapshot) => Ok(snapshot),
        None => load_user_data(),
    }
}

// Complete sparse media only before insertion into the originating user-data session.
pub(crate) fn set_user_data_favorites(
    items: Vec<MediaItem>,
    is_favorite: bool,
) -> Result<UserDataSnapshot, CoreError> {
    let (items, session_generation) = if is_favorite {
        let (items, generation) = complete_user_data_media(items)?;
        (items, Some(generation))
    } else {
        (items, None)
    };
    mutate_user_data(session_generation, |user_data| {
        user_data.set_favorites(items, is_favorite);
        Ok(())
    })
}

pub(crate) fn add_local_playlist_tracks(
    playlist_id: &str,
    tracks: Vec<Track>,
) -> Result<UserDataSnapshot, CoreError> {
    let (items, session_generation) =
        complete_user_data_media(tracks.into_iter().map(MediaItem::Track).collect())?;
    let tracks = items
        .into_iter()
        .map(|item| match item {
            MediaItem::Track(track) => Ok(track),
            _ => Err(CoreError::internal("expected completed playlist track")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    mutate_local_playlist_tracks(Some(session_generation), playlist_id, false, |user_data| {
        user_data.add_local_playlist_tracks(playlist_id, tracks)
    })
}

fn complete_user_data_media(items: Vec<MediaItem>) -> Result<(Vec<MediaItem>, u64), CoreError> {
    let (client, session_generation) = {
        let coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        let session = coordinator.session_configuration.as_ref().ok_or_else(|| {
            CoreError::command_failed("a restored session is required to complete user data")
        })?;
        (
            MusicAssistantClient::new(&session.music_assistant_url.0, session.access_token.clone()),
            coordinator.user_data_session_generation,
        )
    };
    let items = CORE_NETWORK_RUNTIME
        .block_on(client.complete_user_data_media(items))
        .map_err(CoreError::command_failed)?;
    Ok((items, session_generation))
}

// User-data mutations serialize commands and reject stale network completions.
pub(crate) fn mutate_user_data<F>(
    expected_session_generation: Option<u64>,
    operation: F,
) -> Result<UserDataSnapshot, CoreError>
where
    F: FnOnce(&mut UserDataController) -> Result<(), String>,
{
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let (snapshot, session_generation, should_schedule) = {
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        coordinator.session_configuration.as_ref().ok_or_else(|| {
            CoreError::command_failed("a restored session is required to modify user data")
        })?;
        if expected_session_generation
            .is_some_and(|generation| generation != coordinator.user_data_session_generation)
        {
            return Err(CoreError::command_failed(
                "user data session changed during media completion",
            ));
        }
        coordinator
            .mutate_user_data(operation)
            .map_err(CoreError::command_failed)?;
        let snapshot = coordinator
            .user_data_snapshot()
            .map_err(CoreError::command_failed)?;
        let session_generation = coordinator.user_data_session_generation;
        let should_schedule = coordinator
            .user_data
            .as_mut()
            .expect("user data was checked above")
            .schedule_write();
        (snapshot, session_generation, should_schedule)
    };

    if should_schedule {
        start_user_data_write_worker(session_generation);
    }
    crate::core::event_channel::publish_background_event(MediaSessionEvent::UserDataChanged);
    Ok(snapshot)
}

/// Applies a local playlist mutation, renders its cover outside the coordinator lock, then persists both.
pub(crate) fn mutate_local_playlist_tracks<F>(
    expected_session_generation: Option<u64>,
    playlist_id: &str,
    clear_cover_when_empty: bool,
    operation: F,
) -> Result<UserDataSnapshot, CoreError>
where
    F: FnOnce(&mut UserDataController) -> Result<(), String>,
{
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let (tracks, base_url, session_generation) = {
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        if expected_session_generation
            .is_some_and(|generation| generation != coordinator.user_data_session_generation)
        {
            return Err(CoreError::command_failed(
                "user data session changed during media completion",
            ));
        }
        let base_url = coordinator
            .session_configuration
            .as_ref()
            .ok_or_else(|| {
                CoreError::command_failed("a restored session is required to modify user data")
            })?
            .music_assistant_url
            .0
            .clone();
        coordinator
            .mutate_user_data(operation)
            .map_err(CoreError::command_failed)?;
        let tracks = coordinator
            .user_data_snapshot()
            .map_err(CoreError::command_failed)?
            .playlists
            .into_iter()
            .find(|playlist| {
                playlist
                    .playlist
                    .media
                    .item_id
                    .as_str()
                    .eq_ignore_ascii_case(playlist_id)
            })
            .ok_or_else(|| CoreError::command_failed("local playlist was not found"))?
            .items;
        (tracks, base_url, coordinator.user_data_session_generation)
    };
    let cover = CORE_NETWORK_RUNTIME.block_on(build_playlist_collage(&tracks, &base_url));
    let (snapshot, should_schedule) = {
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        if coordinator.user_data_session_generation != session_generation {
            return Err(CoreError::command_failed(
                "user data session changed during cover generation",
            ));
        }
        coordinator
            .mutate_user_data(|user_data| {
                user_data.set_local_playlist_cover(playlist_id, cover, clear_cover_when_empty)
            })
            .map_err(CoreError::command_failed)?;
        let snapshot = coordinator
            .user_data_snapshot()
            .map_err(CoreError::command_failed)?;
        let should_schedule = coordinator
            .user_data
            .as_mut()
            .expect("user data was checked above")
            .schedule_write();
        (snapshot, should_schedule)
    };
    if should_schedule {
        start_user_data_write_worker(session_generation);
    }
    Ok(snapshot)
}

fn start_user_data_write_worker(session_generation: u64) {
    CORE_NETWORK_RUNTIME.spawn(async move {
        flush_scheduled_write_after_debounce(
            || user_data_write_delay(session_generation),
            || user_data_write_request(session_generation),
            |revision, response| finish_user_data_write(session_generation, revision, response),
        )
        .await;
    });
}

fn user_data_write_delay(session_generation: u64) -> Option<Duration> {
    let coordinator = CORE_COORDINATOR.lock().ok()?;
    if coordinator.user_data_session_generation != session_generation {
        return None;
    }
    coordinator.user_data.as_ref()?.write_delay(Instant::now())
}

fn user_data_write_request(session_generation: u64) -> Option<UserDataWriteRequest> {
    let coordinator = CORE_COORDINATOR.lock().ok()?;
    if coordinator.user_data_session_generation != session_generation {
        return None;
    }
    let session = coordinator.session_configuration.as_ref()?.clone();
    let (revision, preferences) = coordinator.user_data.as_ref()?.pending_write()?;
    Some(UserDataWriteRequest {
        revision,
        username: session.username,
        preferences,
        client: MusicAssistantClient::new(&session.music_assistant_url.0, session.access_token),
    })
}

fn finish_user_data_write(
    session_generation: u64,
    written_revision: u64,
    response: Result<crate::music_assistant::models::session::MusicAssistantUser, String>,
) -> bool {
    let Ok(mut coordinator) = CORE_COORDINATOR.lock() else {
        return true;
    };
    if coordinator.user_data_session_generation != session_generation {
        return true;
    }
    coordinator
        .user_data
        .as_mut()
        .map(|user_data| {
            user_data.finish_write(written_revision, response.ok().map(|user| user.preferences))
        })
        .unwrap_or(true)
}

pub(crate) fn start_music_assistant_query(
    request: MusicAssistantQueryRequest,
) -> Result<(), CoreError> {
    let client = music_assistant_client_for_query("execute a Music Assistant query")?;
    CORE_NETWORK_RUNTIME.spawn(async move {
        macro_rules! serialize_query {
            ($future:expr) => {
                $future
                    .await
                    .map_err(CoreError::command_failed)
                    .and_then(|response| {
                        serde_json::to_value(response).map_err(|error| {
                            CoreError::internal(format!("serialize query response failed: {error}"))
                        })
                    })
            };
        }

        let result = match request.query {
            MusicAssistantQuery::Recommendations => serialize_query!(client.get_recommendations()),
            MusicAssistantQuery::ProviderManifests => {
                serialize_query!(client.get_provider_manifests())
            }
            MusicAssistantQuery::ProviderIconData { provider } => {
                serialize_query!(client.get_provider_icon_data(&provider))
            }
            MusicAssistantQuery::RecentlyPlayedTracks { limit } => serialize_query!(async {
                client
                    .get_recently_played_tracks(limit.clamp(1, 100))
                    .await
                    .map(|tracks| tracks.into_iter().map(MediaItem::Track).collect::<Vec<_>>())
            }),
            MusicAssistantQuery::SearchMedia {
                query,
                media_type,
                limit,
            } => {
                serialize_query!(client.search_media(&query, &[media_type], limit.clamp(1, 100)))
            }
            MusicAssistantQuery::Artist { item_id, provider } => {
                serialize_query!(client.get_artist(&MediaId(item_id), &provider))
            }
            MusicAssistantQuery::Album { item_id, provider } => {
                serialize_query!(client.get_album(&MediaId(item_id), &provider))
            }
            MusicAssistantQuery::AlbumTracks { item_id, provider } => {
                serialize_query!(client.get_album_tracks(&MediaId(item_id), &provider))
            }
            MusicAssistantQuery::Playlist { item_id, provider } => {
                serialize_query!(client.get_playlist(&MediaId(item_id), &provider))
            }
            MusicAssistantQuery::PlaylistTracks {
                item_id,
                provider,
                force_refresh,
                allow_dynamic_tracks,
            } => serialize_query!(client.get_playlist_tracks(
                &MediaId(item_id),
                &provider,
                force_refresh,
                allow_dynamic_tracks,
            )),
            MusicAssistantQuery::ArtistAlbums { item_id, provider } => {
                serialize_query!(client.get_artist_albums(&MediaId(item_id), &provider))
            }
            MusicAssistantQuery::ArtistTopTracks { item_id, provider } => {
                serialize_query!(client.get_artist_top_tracks(&MediaId(item_id), &provider))
            }
            MusicAssistantQuery::SimilarArtists {
                item_id,
                provider,
                limit,
            } => serialize_query!(client.get_similar_artists(
                &MediaId(item_id),
                &provider,
                serde_json::json!({ "limit": limit.clamp(1, 100) }),
            )),
            MusicAssistantQuery::SimilarTracks {
                item_id,
                provider,
                limit,
                allow_lookup,
            } => serialize_query!(client.get_similar_tracks(
                &MediaId(item_id),
                &provider,
                serde_json::json!({
                    "limit": limit.clamp(1, 100),
                    "allow_lookup": allow_lookup,
                }),
            )),
            MusicAssistantQuery::TrackVersions { item_id, provider } => {
                serialize_query!(client.get_track_versions(&MediaId(item_id), &provider))
            }
        };

        match result {
            Ok(result) => crate::ffi::event_channel::publish_json_value(serde_json::json!({
                "type": "query_completed",
                "requestId": request.request_id,
                "result": result,
            })),
            Err(error) => crate::ffi::event_channel::publish_json_value(serde_json::json!({
                "type": "query_failed",
                "requestId": request.request_id,
                "error": error,
            })),
        }
    });
    Ok(())
}

// Synchronous Music Assistant reads for Android MediaLibrary browse callbacks.
pub(crate) fn read_music_assistant_recommendations(
) -> Result<Vec<MusicAssistantRecommendationFolder>, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant recommendations")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_recommendations())
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_recently_played_tracks(
    limit: u32,
) -> Result<Vec<Track>, CoreError> {
    let client = music_assistant_client_for_query("read recently played tracks")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_recently_played_tracks(limit.clamp(1, 100)))
        .map_err(CoreError::command_failed)
}

pub(crate) fn search_music_assistant_media(
    query: &str,
    media_type: MediaType,
    limit: u32,
) -> Result<MusicAssistantSearchResults, CoreError> {
    let client = music_assistant_client_for_query("search Music Assistant")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.search_media(query, &[media_type], limit.clamp(1, 100)))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_provider_manifests() -> Result<Vec<ProviderManifest>, CoreError>
{
    let client = music_assistant_client_for_query("read Music Assistant provider manifests")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_provider_manifests())
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_provider_icon_data(
    provider: &str,
) -> Result<Option<String>, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant provider icon")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_provider_icon_data(provider))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_artist(
    item_id: &str,
    provider: &str,
) -> Result<Artist, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant artist")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_artist(&MediaId(item_id.to_string()), provider))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_album(
    item_id: &str,
    provider: &str,
) -> Result<Album, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant album")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_album(&MediaId(item_id.to_string()), provider))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_album_tracks(
    item_id: &str,
    provider: &str,
) -> Result<Vec<Track>, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant album tracks")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_album_tracks(&MediaId(item_id.to_string()), provider))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_playlist(
    item_id: &str,
    provider: &str,
) -> Result<Playlist, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant playlist")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_playlist(&MediaId(item_id.to_string()), provider))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_playlist_tracks(
    item_id: &str,
    provider: &str,
    force_refresh: bool,
    allow_dynamic_tracks: bool,
) -> Result<Vec<Track>, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant playlist tracks")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_playlist_tracks(
            &MediaId(item_id.to_string()),
            provider,
            force_refresh,
            allow_dynamic_tracks,
        ))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_artist_albums(
    item_id: &str,
    provider: &str,
) -> Result<Vec<Album>, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant artist albums")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_artist_albums(&MediaId(item_id.to_string()), provider))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_artist_top_tracks(
    item_id: &str,
    provider: &str,
) -> Result<Vec<Track>, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant artist top tracks")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_artist_top_tracks(&MediaId(item_id.to_string()), provider))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_similar_artists(
    item_id: &str,
    provider: &str,
    limit: u32,
) -> Result<Vec<Artist>, CoreError> {
    let client = music_assistant_client_for_query("read similar Music Assistant artists")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_similar_artists(
            &MediaId(item_id.to_string()),
            provider,
            serde_json::json!({ "limit": limit.clamp(1, 100) }),
        ))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_similar_tracks(
    item_id: &str,
    provider: &str,
    limit: u32,
    allow_lookup: bool,
) -> Result<Vec<Track>, CoreError> {
    let client = music_assistant_client_for_query("read similar Music Assistant tracks")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_similar_tracks(
            &MediaId(item_id.to_string()),
            provider,
            serde_json::json!({
                "limit": limit.clamp(1, 100),
                "allow_lookup": allow_lookup,
            }),
        ))
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_music_assistant_track_versions(
    item_id: &str,
    provider: &str,
) -> Result<Vec<Track>, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant track versions")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_track_versions(&MediaId(item_id.to_string()), provider))
        .map_err(CoreError::command_failed)
}

// Playlist mutations use the configured Music Assistant session without changing playback state.
pub(crate) fn add_playlist_tracks(playlist_id: String, uris: Vec<String>) -> Result<(), CoreError> {
    let client = music_assistant_client_for_query("add playlist tracks")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.add_playlist_tracks(&playlist_id, uris))
        .map_err(CoreError::command_failed)
}

pub(crate) fn remove_playlist_tracks(
    playlist_id: String,
    positions_to_remove: Vec<u32>,
) -> Result<(), CoreError> {
    let client = music_assistant_client_for_query("remove playlist tracks")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.remove_playlist_tracks(&playlist_id, positions_to_remove))
        .map_err(CoreError::command_failed)
}

// Radio is a single MA queue command, not a second client-side recommendation engine.
pub(crate) fn start_radio_and_execute_command(
    media_items: Vec<MediaItem>,
) -> Result<Vec<MediaSessionEvent>, CoreError> {
    resolve_media_and_execute_command(media_items, |playback, media| playback.start_radio(media))
}

// Initial reads use the same complete contracts as their corresponding events.
pub(crate) fn read_queue() -> Result<Queue, CoreError> {
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    Ok(coordinator.playback.queue())
}

pub(crate) fn read_media_session_state() -> Result<MediaSessionState, CoreError> {
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    Ok(coordinator.media_session_state())
}

pub(crate) fn read_session_settings() -> Result<SessionSettings, CoreError> {
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    Ok(session_settings(coordinator.session_configuration.as_ref()))
}

pub(crate) fn read_player_state() -> Result<PlayerState, CoreError> {
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    Ok(coordinator.playback.player_state())
}

fn collect_media_session_events(
    coordinator: &CoreCoordinator,
    queue_before: Queue,
    player_state_before: PlayerState,
) -> Vec<MediaSessionEvent> {
    let media_session_state = coordinator.media_session_state();
    let mut events = Vec::with_capacity(2);
    if !media_session_state
        .queue
        .has_same_structure_as(&queue_before)
    {
        events.push(MediaSessionEvent::QueueChanged {
            queue: media_session_state.queue,
        });
    }
    if media_session_state.player_state != player_state_before {
        events.push(MediaSessionEvent::PlayerStateChanged {
            player_state: media_session_state.player_state,
        });
    }
    events
}

#[cfg(test)]
mod tests {
    use super::CoreCoordinator;
    use crate::core::models::{LogLevelPreference, SelectedOutput, SessionConfig, ThemePreference};
    use crate::music_assistant::auth_store::AuthStore;
    use crate::music_assistant::models::identifiers::AccessToken;
    use crate::music_assistant::models::queue::PlayState;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use std::sync::Mutex;

    struct TestAuthStore {
        session: Mutex<Option<SessionConfig>>,
        should_fail_writes: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl TestAuthStore {
        fn new() -> Self {
            Self {
                session: Mutex::new(None),
                should_fail_writes: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }
    }

    impl AuthStore for TestAuthStore {
        fn load_session(&self) -> Result<Option<SessionConfig>, String> {
            self.session
                .lock()
                .map(|session| session.clone())
                .map_err(|_| "test auth store lock poisoned".to_string())
        }

        fn save_session(&self, session: SessionConfig) -> Result<(), String> {
            if self
                .should_fail_writes
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err("test storage unavailable".into());
            }
            self.session
                .lock()
                .map(|mut current| *current = Some(session))
                .map_err(|_| "test auth store lock poisoned".to_string())
        }

        fn clear_session(&self) -> Result<(), String> {
            self.session
                .lock()
                .map(|mut current| *current = None)
                .map_err(|_| "test auth store lock poisoned".to_string())
        }
    }

    fn test_coordinator() -> CoreCoordinator {
        CoreCoordinator::with_auth_store(Box::new(TestAuthStore::new()))
    }

    fn settings_update() -> crate::core::models::SessionSettingsUpdate {
        crate::core::models::SessionSettingsUpdate {
            local_cache_size_bytes: 512 * 1024 * 1024,
            theme: ThemePreference::Light,
            log_level: LogLevelPreference::Warn,
        }
    }

    fn session_config() -> SessionConfig {
        SessionConfig {
            music_assistant_url: MusicAssistantServerUrl("https://music-assistant.example".into()),
            access_token: Some(AccessToken("secret-token".into())),
            user_id: None,
            username: "mashin".into(),
            selected_output: SelectedOutput::Local,
            local: crate::core::config::default_local_output(),
            volume: 100,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
        }
    }

    #[test]
    fn configured_session_activates_the_selected_playback_engine() {
        let mut coordinator = test_coordinator();

        coordinator
            .configure_session(session_config())
            .expect("restore session");
        let queue = coordinator.playback.queue();
        let player_state = coordinator.playback.player_state();

        assert!(coordinator.session_configuration.is_some());
        assert!(queue.items.is_empty());
        assert_eq!(player_state.playback_state, PlayState::Idle);
    }

    #[test]
    fn session_settings_commit_all_preferences_and_reject_invalid_cache_without_mutation() {
        let mut coordinator = test_coordinator();
        coordinator.configure_session(session_config()).unwrap();
        let update = super::super::models::SessionSettingsUpdate {
            local_cache_size_bytes: 500 * 1024 * 1024,
            theme: ThemePreference::Light,
            log_level: LogLevelPreference::Warn,
        };
        coordinator.save_session_settings(update).unwrap();
        let stored = coordinator.auth_store.load_session().unwrap().unwrap();
        assert_eq!(coordinator.playback.player_state().volume, stored.volume);
        let settings = crate::core::config::session_settings(Some(&stored));
        assert_eq!(settings.local_cache_size_bytes, 500 * 1024 * 1024);
        assert_eq!(settings.theme, ThemePreference::Light);
        assert_eq!(settings.log_level, LogLevelPreference::Warn);
        assert!(coordinator
            .save_session_settings(super::super::models::SessionSettingsUpdate {
                local_cache_size_bytes: 1,
                theme: ThemePreference::Dark,
                log_level: LogLevelPreference::Error,
            })
            .is_err());
        assert_eq!(
            crate::core::config::session_settings(coordinator.session_configuration.as_ref()),
            settings
        );
        assert_eq!(
            crate::core::config::session_settings(
                coordinator.auth_store.load_session().unwrap().as_ref()
            ),
            settings
        );
    }

    #[test]
    fn settings_preserve_the_stable_mashin_device_identity() {
        let mut coordinator = test_coordinator();
        let mut session = session_config();
        session.local.cache_bytes = settings_update().local_cache_size_bytes;
        let device_id = session.local.device_id.clone();
        coordinator.configure_session(session).unwrap();
        let native = coordinator.playback.checkpoint();
        coordinator
            .save_session_settings(settings_update())
            .unwrap();
        assert_eq!(
            coordinator
                .auth_store
                .load_session()
                .unwrap()
                .unwrap()
                .local
                .device_id,
            device_id
        );
        assert!(coordinator
            .playback
            .checkpoint()
            .shares_local_session(&native));
    }

    #[test]
    fn failed_settings_write_restores_the_previous_native_session_and_connection() {
        let store = TestAuthStore::new();
        let should_fail_writes = store.should_fail_writes.clone();
        let mut coordinator = CoreCoordinator::with_auth_store(Box::new(store));
        coordinator.configure_session(session_config()).unwrap();
        let native = coordinator.playback.checkpoint();
        let hub = coordinator.connection.as_ref().unwrap().clone();
        let previous = coordinator.session_configuration.clone().unwrap();
        should_fail_writes.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(coordinator
            .save_session_settings(settings_update())
            .is_err());
        assert!(coordinator
            .playback
            .checkpoint()
            .shares_local_session(&native));
        assert!(std::sync::Arc::ptr_eq(
            coordinator.connection.as_ref().unwrap(),
            &hub
        ));
        assert_eq!(coordinator.session_configuration, Some(previous.clone()));
        assert_eq!(
            coordinator.auth_store.load_session().unwrap(),
            Some(previous)
        );
    }

    #[test]
    fn failed_configuration_does_not_create_a_core_session() {
        let mut coordinator = test_coordinator();
        let mut invalid_session = session_config();
        invalid_session.local.device_id.clear();

        assert!(coordinator.configure_session(invalid_session).is_err());

        assert!(coordinator.session_configuration.is_none());
    }

    #[test]
    fn select_output_reuses_the_same_mashin_device_identity() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("restore session");

        coordinator
            .select_local_output()
            .expect("select Mashin output");

        assert_eq!(
            coordinator.playback.player_state().active_mode,
            crate::playback::mode::PlaybackMode::Local
        );
    }

    #[tokio::test]
    async fn remote_media_reaches_ma_without_local_expansion_or_shuffle() {
        let media: Vec<crate::music_assistant::models::media::MediaItem> = serde_json::from_value(serde_json::json!([
            {"media_type":"album", "item_id":"album", "provider":"library", "name":"Album", "uri":"library://album/album"},
            {"media_type":"playlist", "item_id":"playlist", "provider":"library", "name":"Playlist", "uri":"library://playlist/playlist"}
        ])).unwrap();
        let resolved = super::expand_local_playlist_media(media.clone(), None).unwrap();
        assert_eq!(
            serde_json::to_value(resolved).unwrap(),
            serde_json::to_value(media).unwrap()
        );
    }

    #[test]
    fn local_cache_size_is_reconfigured_and_persisted_in_the_session() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure session");

        coordinator
            .save_session_settings(settings_update())
            .expect("update local cache size");

        let configuration = coordinator
            .session_configuration
            .as_ref()
            .expect("session configuration");
        assert_eq!(configuration.local.cache_bytes, 512 * 1024 * 1024);
        assert_eq!(
            crate::core::config::session_settings(Some(configuration)).local_cache_size_bytes,
            512 * 1024 * 1024
        );
    }

    #[test]
    fn theme_is_persisted_in_the_session_settings() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure session");

        coordinator
            .save_session_settings(settings_update())
            .expect("update theme");

        let configuration = coordinator
            .session_configuration
            .as_ref()
            .expect("session configuration");
        assert_eq!(configuration.theme, ThemePreference::Light);
        assert_eq!(
            crate::core::config::session_settings(Some(configuration)).theme,
            ThemePreference::Light
        );
    }

    #[test]
    fn log_level_is_persisted_in_the_session_settings() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure session");

        coordinator
            .save_session_settings(settings_update())
            .expect("update log level");

        let configuration = coordinator
            .session_configuration
            .as_ref()
            .expect("session configuration");
        assert_eq!(configuration.log_level, LogLevelPreference::Warn);
        assert_eq!(
            crate::core::config::session_settings(Some(configuration)).log_level,
            LogLevelPreference::Warn
        );
    }

    #[test]
    fn invalid_remote_output_does_not_replace_the_active_mashin_engine() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("restore Mashin session");
        let mut invalid_session = coordinator.session_configuration.clone().unwrap();
        invalid_session.selected_output =
            SelectedOutput::Remote(crate::core::models::RemoteOutputConfig {
                player_id: crate::music_assistant::models::identifiers::PlayerId(String::new()),
            });

        let error = coordinator
            .configure_session(invalid_session)
            .expect_err("remote identity is required");

        assert!(error.contains("player ID is required"));
        assert!(coordinator.session_configuration.is_some());
        assert_eq!(
            coordinator.playback.player_state().active_mode,
            crate::playback::mode::PlaybackMode::Local
        );
    }

    #[test]
    fn remote_output_projects_ma_state_without_changing_local_volume() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure Mashin session");
        let mut remote_session = session_config();
        remote_session.music_assistant_url.0 = "http://127.0.0.1:1".into();
        remote_session.volume = 37;
        remote_session.selected_output =
            SelectedOutput::Remote(crate::core::models::RemoteOutputConfig {
                player_id: crate::music_assistant::models::identifiers::PlayerId(
                    "remote-player".into(),
                ),
            });

        coordinator
            .configure_session(remote_session)
            .expect("configure remote engine");
        assert_eq!(
            coordinator.playback.player_state().active_mode,
            crate::playback::mode::PlaybackMode::Remote
        );
        assert_eq!(
            coordinator.playback.player_state().buffered_position_millis,
            None
        );
        coordinator.set_volume(82).expect("dispatch remote volume");
        assert_eq!(
            coordinator.session_configuration.as_ref().unwrap().volume,
            37
        );
        coordinator.shutdown();
    }

    #[test]
    fn restored_session_configures_playback_without_exposing_its_token() {
        let mut coordinator = test_coordinator();

        coordinator
            .configure_session(session_config())
            .expect("restore session");
        let serialized_queue =
            serde_json::to_string(&coordinator.playback.queue()).expect("serialize queue");
        let serialized_player_state = serde_json::to_string(&coordinator.playback.player_state())
            .expect("serialize player state");

        assert!(!serialized_queue.contains("secret-token"));
        assert!(!serialized_player_state.contains("secret-token"));
    }

    #[test]
    fn restored_session_applies_the_persisted_local_audio_volume() {
        let mut coordinator = test_coordinator();
        let mut session = session_config();
        session.volume = 37;
        coordinator
            .configure_session(session)
            .expect("configure session");

        coordinator.shutdown();
        coordinator
            .restore_persisted_session()
            .expect("restore session");

        assert_eq!(coordinator.playback.player_state().volume, 37);
    }

    #[test]
    fn incomplete_user_data_mutations_fail_without_changing_active_state() {
        let mut coordinator = test_coordinator();
        coordinator.user_data =
            Some(super::UserDataController::from_preferences(serde_json::Map::new()).unwrap());
        let original = coordinator.user_data_snapshot().unwrap();
        let incomplete: crate::music_assistant::models::media::Track = serde_json::from_value(serde_json::json!({
            "item_id": "track", "provider": "library", "name": "Track", "uri": "library://track/track"
        })).unwrap();
        let failure = coordinator
            .mutate_user_data(|user_data| {
                let playlist = user_data.create_local_playlist("New playlist".to_string())?;
                user_data
                    .add_local_playlist_tracks(playlist.media.item_id.as_str(), vec![incomplete])
            })
            .unwrap_err();
        assert!(failure.contains("missing provider mappings"));
        assert_eq!(coordinator.user_data_snapshot().unwrap(), original);
    }

    #[test]
    fn restoring_an_active_session_keeps_loaded_user_data() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure session");
        coordinator.user_data = Some(
            super::UserDataController::from_preferences(serde_json::Map::new())
                .expect("empty preferences"),
        );
        let generation = coordinator.user_data_session_generation;

        assert!(coordinator
            .restore_persisted_session()
            .expect("restore session"));
        assert!(coordinator.user_data.is_some());
        assert_eq!(coordinator.user_data_session_generation, generation);
    }

    #[test]
    fn old_user_data_load_cannot_replace_a_new_session() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure session");
        let old_generation = coordinator.user_data_session_generation;
        coordinator
            .configure_session(session_config())
            .expect("replace session");

        let user_data = super::UserDataController::from_preferences(serde_json::Map::new())
            .expect("empty preferences");
        assert!(coordinator
            .replace_user_data_for_session(old_generation, user_data)
            .is_err());
        assert!(coordinator.user_data.is_none());
    }

    #[test]
    fn failed_command_returns_a_structured_core_error() {
        let error =
            super::execute_command_and_collect_events(|_| Err("invalid test command".to_string()))
                .expect_err("command should fail");

        assert_eq!(
            error.code,
            crate::core::models::CoreErrorCode::CommandFailed
        );
        assert_eq!(error.source, crate::core::models::CoreErrorSource::Core);
        assert_eq!(error.message, "invalid test command");
        assert!(!error.is_retryable);
    }

    #[test]
    fn shutdown_releases_in_memory_session_configuration_without_erasing_persistence() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("restore session");

        coordinator.shutdown();
        assert!(coordinator.session_configuration.is_none());
        assert_eq!(
            coordinator.playback.player_state().playback_state,
            PlayState::Uninitialized
        );

        assert!(coordinator
            .restore_persisted_session()
            .expect("restore persisted session"));
        assert!(coordinator.session_configuration.is_some());
    }
}
