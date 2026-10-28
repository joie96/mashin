//! Process-wide application core that owns session and playback state.

use super::config::{session_settings, SessionSettings};
use super::models::{
    CoreError, MediaSessionEvent, MusicAssistantQuery, MusicAssistantQueryRequest, SessionConfig,
};
use crate::core::event_channel::publish_background_event;
use crate::logging::Logger;
use crate::music_assistant::session_store::{platform_session_store, SessionStore};
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::identifiers::MediaId;
use crate::music_assistant::models::media::{
    Album, Artist, MediaItem, MediaType, MusicAssistantSearchResults, Playlist, Track,
};
use crate::music_assistant::models::queue::QueueOption;
use crate::music_assistant::models::recommendation::RecommendationSnapshot;
use crate::music_assistant::models::session::ProviderManifest;
use crate::music_assistant::user_data::LibraryUserData;
use crate::music_assistant::user_data::UserDataSnapshot;
use crate::playback::engine::PlaybackEngine;
use crate::playback::models::{MediaSessionState, PlayerState, Queue};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use tokio::runtime::Runtime;

#[path = "session.rs"]
mod session;
#[path = "settings.rs"]
mod settings;
#[path = "user_data.rs"]
mod user_data;
pub(crate) use user_data::*;

pub(crate) static CORE_COORDINATOR: Lazy<Mutex<CoreCoordinator>> =
    Lazy::new(|| Mutex::new(CoreCoordinator::new()));
static CORE_COMMAND_SERIALIZER: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static CORE_USER_DATA_LOAD: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));
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
    session_store: Box<dyn SessionStore + Send>,
    playback: PlaybackEngine,
    session_configuration: Option<SessionConfig>,
    user_data: Option<LibraryUserData>,
    user_data_events: Option<user_data::UserDataEvents>,
    user_data_session_generation: u64,
    connection:
        Option<std::sync::Arc<crate::music_assistant::connection::MusicAssistantConnection>>,
    pending_commands: Vec<crate::playback::backend::PlaybackCommand>,
}

impl CoreCoordinator {
    // Construction and session configuration.
    pub(crate) fn new() -> Self {
        Self::with_session_store(platform_session_store())
    }

    fn with_session_store(session_store: Box<dyn SessionStore + Send>) -> Self {
        Self {
            session_store,
            playback: PlaybackEngine::new(),
            session_configuration: None,
            user_data: None,
            user_data_events: None,
            user_data_session_generation: 0,
            connection: None,
            pending_commands: Vec::new(),
        }
    }

    // Native shutdown removes all in-memory session data after extracting the final pending write.
    pub(crate) fn shutdown(&mut self) {
        self.playback.shutdown();
        self.pending_commands.clear();
        self.connection = None;
        self.session_configuration = None;
        self.invalidate_user_data();
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
    fn update_initial_user_data(
        &mut self,
        session_generation: u64,
        update: impl FnOnce(&mut LibraryUserData),
    ) -> Result<(), String> {
        if self.user_data_session_generation != session_generation {
            return Err("session changed while loading user data".to_string());
        }
        update(self.user_data.get_or_insert_with(LibraryUserData::default));
        Ok(())
    }

    fn invalidate_user_data(&mut self) {
        self.user_data_events = None;
        self.user_data = None;
        self.user_data_session_generation = self.user_data_session_generation.wrapping_add(1);
    }

    fn user_data_snapshot(&self) -> Result<UserDataSnapshot, String> {
        self.user_data
            .as_ref()
            .map(|user_data| user_data.snapshot.clone())
            .ok_or_else(|| "user data is not loaded".to_string())
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
    drop(_command_guard);
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
    if should_load_user_data {
        start_session_data_load();
    }
    Ok(events)
}

/// Drops session-owned caches and pending favorite wishes without persisting them.
pub(crate) fn shutdown() -> Result<(), CoreError> {
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .shutdown();

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
        start_session_data_load();
    }
    Ok((has_restored_session, events))
}

fn start_session_data_load() {
    spawn_blocking_operation(move || {
        if let Err(error) = read_or_load_user_data() {
            LOGGER.warn(format!("load user data failed: {}", error.message));
        }
    });
}

// Client-owned playlists are expanded; MA owns every other playback decision.
pub(crate) fn resolve_media_and_execute_command<F>(
    media_items: Vec<MediaItem>,
    option: QueueOption,
    operation: F,
) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut PlaybackEngine, Vec<MediaItem>) -> Result<(), String>,
{
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let is_starting =
        !media_items.is_empty() && matches!(option, QueueOption::Play | QueueOption::Replace);
    let start_request = if is_starting {
        Some(
            CORE_COORDINATOR
                .lock()
                .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
                .playback
                .begin_start(crate::playback::start::StartKind::Media)
                .map_err(CoreError::command_failed)?,
        )
    } else {
        None
    };
    drop(_command_guard);
    let result = CORE_NETWORK_RUNTIME.block_on(execute_resolved_media_command(
        media_items,
        start_request.as_ref(),
        None,
        operation,
    ));
    if result.is_err() {
        if let Some(request) = start_request {
            request.complete(false);
        }
        let coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        publish_background_event(MediaSessionEvent::PlayerStateChanged {
            player_state: coordinator.playback.player_state(),
        });
    }
    result
}

// Shared media execution releases core ownership while the queued backend commands await I/O.
async fn execute_resolved_media_command<F>(
    media_items: Vec<MediaItem>,
    start_request: Option<&crate::playback::start::PlaybackStartRequest>,
    selection_revision: Option<u64>,
    operation: F,
) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut PlaybackEngine, Vec<MediaItem>) -> Result<(), String>,
{
    let command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    if start_request.is_some_and(|request| !request.is_pending()) {
        return Err(CoreError::command_failed(
            "playback start was cancelled or replaced",
        ));
    }
    if selection_revision
        .is_some_and(|revision| coordinator.playback.media_selection_revision() != revision)
    {
        return Err(CoreError::command_failed(
            "radio selection changed while resolving media",
        ));
    }
    let queue_before = coordinator.playback.queue();
    let player_state_before = coordinator.playback.player_state();
    let result = operation(&mut coordinator.playback, media_items);
    let commands = coordinator
        .take_pending_commands()
        .map_err(CoreError::command_failed)?;
    result.map_err(CoreError::command_failed)?;
    drop(coordinator);
    drop(command_guard);
    for command in commands {
        command.await.map_err(CoreError::command_failed)?;
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

// Music Assistant queries.
fn music_assistant_client_for_query(
    operation: &str,
) -> Result<std::sync::Arc<MusicAssistantClient>, CoreError> {
    CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .connection
        .as_ref()
        .map(|connection| connection.client.clone())
        .ok_or_else(|| {
            CoreError::command_failed(format!("a restored session is required to {operation}"))
        })
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
            MusicAssistantQuery::Recommendations { view } => {
                serialize_query!(client.get_recommendations(view))
            }
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
    view: crate::music_assistant::models::recommendation::RecommendationView,
) -> Result<RecommendationSnapshot, CoreError> {
    let client = music_assistant_client_for_query("read Music Assistant recommendations")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.get_recommendations(view))
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

// Radio I/O leaves transport responsive; each queue write validates its original selection.
pub(crate) fn start_radio_and_execute_command(
    media_items: Vec<MediaItem>,
) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let (client, selection_revision) = {
        let _command_guard = CORE_COMMAND_SERIALIZER
            .lock()
            .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        let client = coordinator
            .connection
            .as_ref()
            .ok_or_else(|| {
                CoreError::command_failed("a restored session is required to start radio")
            })?
            .client
            .clone();
        (client, coordinator.playback.reserve_media_selection())
    };
    CORE_NETWORK_RUNTIME
        .block_on(crate::music_assistant::radio::start_radio(
            &client,
            media_items,
            |media, option| async move {
                let expected_revision = if option == QueueOption::Replace {
                    selection_revision
                } else {
                    selection_revision.wrapping_add(1)
                };
                let events = execute_resolved_media_command(
                    media,
                    None,
                    Some(expected_revision),
                    |playback, media| playback.queue_play_media(media, option, None, false),
                )
                .await
                .map_err(|error| error.message)?;
                for event in events {
                    publish_background_event(event);
                }
                Ok(())
            },
        ))
        .map_err(CoreError::command_failed)?;
    Ok(Vec::new())
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
    use crate::music_assistant::session_store::SessionStore;
    use crate::music_assistant::models::identifiers::{AccessToken, UserId};
    use crate::music_assistant::models::queue::PlayState;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use std::sync::Mutex;

    struct TestSessionStore {
        session: Mutex<Option<SessionConfig>>,
        should_fail_writes: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl TestSessionStore {
        fn new() -> Self {
            Self {
                session: Mutex::new(None),
                should_fail_writes: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }
    }

    impl SessionStore for TestSessionStore {
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
        CoreCoordinator::with_session_store(Box::new(TestSessionStore::new()))
    }

    fn settings_update() -> crate::core::models::SessionSettingsUpdate {
        crate::core::models::SessionSettingsUpdate {
            local_cache_size_bytes: 512 * 1024 * 1024,
            theme: ThemePreference::Light,
            log_level: LogLevelPreference::Warn,
            should_show_multi_device_button: false,
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
            should_show_multi_device_button: true,
            preferred_provider: None,
        }
    }

    #[test]
    fn current_user_id_is_persisted_without_reconfiguring_the_session() {
        let mut coordinator = test_coordinator();
        let session = session_config();
        coordinator
            .session_store
            .save_session(session.clone())
            .unwrap();
        coordinator.session_configuration = Some(session.clone());
        let generation = coordinator.user_data_session_generation;
        coordinator
            .remember_current_user_id(generation, UserId("user-1".into()))
            .unwrap();
        let mut expected = session;
        expected.user_id = Some(UserId("user-1".into()));
        assert_eq!(coordinator.session_configuration, Some(expected.clone()));
        assert_eq!(
            coordinator.session_store.load_session().unwrap(),
            Some(expected)
        );
    }

    #[test]
    fn current_user_id_rejects_replaced_sessions_and_failed_storage_writes() {
        let store = TestSessionStore::new();
        let should_fail_writes = store.should_fail_writes.clone();
        let mut coordinator = CoreCoordinator::with_session_store(Box::new(store));
        let session = session_config();
        coordinator
            .session_store
            .save_session(session.clone())
            .unwrap();
        coordinator.session_configuration = Some(session.clone());
        let generation = coordinator.user_data_session_generation;
        assert!(coordinator
            .remember_current_user_id(generation.wrapping_add(1), UserId("other".into()))
            .is_err());
        should_fail_writes.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(coordinator
            .remember_current_user_id(generation, UserId("user-1".into()))
            .is_err());
        assert_eq!(coordinator.session_configuration, Some(session.clone()));
        assert_eq!(
            coordinator.session_store.load_session().unwrap(),
            Some(session)
        );
    }

    #[test]
    fn current_user_id_from_stored_session_initializes_the_shared_client() {
        let mut coordinator = test_coordinator();
        let mut session = session_config();
        session.user_id = Some(UserId("user-1".into()));
        coordinator.configure_session(session).unwrap();
        let client = &coordinator.connection.as_ref().unwrap().client;
        assert_eq!(
            super::CORE_NETWORK_RUNTIME
                .block_on(client.get_current_user_id())
                .unwrap()
                .as_str(),
            "user-1"
        );
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
            should_show_multi_device_button: false,
        };
        coordinator.save_session_settings(update).unwrap();
        let stored = coordinator.session_store.load_session().unwrap().unwrap();
        assert_eq!(coordinator.playback.player_state().volume, stored.volume);
        let settings = crate::core::config::session_settings(Some(&stored));
        assert_eq!(settings.local_cache_size_bytes, 500 * 1024 * 1024);
        assert_eq!(settings.theme, ThemePreference::Light);
        assert_eq!(settings.log_level, LogLevelPreference::Warn);
        assert!(!settings.should_show_multi_device_button);
        assert!(coordinator
            .save_session_settings(super::super::models::SessionSettingsUpdate {
                local_cache_size_bytes: 1,
                theme: ThemePreference::Dark,
                log_level: LogLevelPreference::Error,
                should_show_multi_device_button: true,
            })
            .is_err());
        assert_eq!(
            crate::core::config::session_settings(coordinator.session_configuration.as_ref()),
            settings
        );
        assert_eq!(
            crate::core::config::session_settings(
                coordinator.session_store.load_session().unwrap().as_ref()
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
                .session_store
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
    fn preferred_provider_persists_without_reconfiguring_playback_and_survives_settings_save() {
        let mut coordinator = test_coordinator();
        coordinator.configure_session(session_config()).unwrap();
        let native = coordinator.playback.checkpoint();
        let preferred = crate::core::models::PreferredProvider {
            provider_instance: "tidal-instance".into(),
            provider_domain: "tidal".into(),
        };
        coordinator
            .set_preferred_provider(Some(preferred.clone()))
            .unwrap();
        assert!(coordinator
            .playback
            .checkpoint()
            .shares_local_session(&native));
        assert_eq!(
            coordinator
                .session_store
                .load_session()
                .unwrap()
                .unwrap()
                .preferred_provider,
            Some(preferred.clone())
        );
        coordinator
            .save_session_settings(settings_update())
            .unwrap();
        assert_eq!(
            crate::core::config::session_settings(coordinator.session_configuration.as_ref())
                .preferred_provider,
            Some(preferred)
        );
        coordinator.set_preferred_provider(None).unwrap();
        assert_eq!(
            coordinator
                .session_store
                .load_session()
                .unwrap()
                .unwrap()
                .preferred_provider,
            None
        );
    }

    #[test]
    fn preferred_provider_failed_write_preserves_memory_and_stored_settings() {
        let store = TestSessionStore::new();
        let should_fail_writes = store.should_fail_writes.clone();
        let mut coordinator = CoreCoordinator::with_session_store(Box::new(store));
        coordinator.configure_session(session_config()).unwrap();
        let previous = coordinator.session_configuration.clone();
        should_fail_writes.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(coordinator
            .set_preferred_provider(Some(crate::core::models::PreferredProvider {
                provider_instance: "tidal-instance".into(),
                provider_domain: "tidal".into(),
            }))
            .is_err());
        assert_eq!(coordinator.session_configuration, previous);
        assert_eq!(coordinator.session_store.load_session().unwrap(), previous);
    }

    #[test]
    fn failed_settings_write_restores_the_previous_native_session_and_connection() {
        let store = TestSessionStore::new();
        let should_fail_writes = store.should_fail_writes.clone();
        let mut coordinator = CoreCoordinator::with_session_store(Box::new(store));
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
            coordinator.session_store.load_session().unwrap(),
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
    fn pending_network_command_does_not_block_following_control_commands() {
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        let (started_sender, started_receiver) = std::sync::mpsc::channel();
        let pending = std::thread::spawn(move || {
            super::execute_command_and_collect_events(|coordinator| {
                coordinator.pending_commands.push(Box::pin(async move {
                    started_sender.send(()).unwrap();
                    release_receiver
                        .await
                        .map_err(|_| "test release dropped".to_string())
                }));
                Ok(())
            })
        });
        started_receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let (control_sender, control_receiver) = std::sync::mpsc::channel();
        let control = std::thread::spawn(move || {
            let result = super::execute_command_and_collect_events(|_| Ok(()));
            control_sender.send(result).unwrap();
        });
        let control_result = control_receiver.recv_timeout(std::time::Duration::from_secs(2));
        release_sender.send(()).unwrap();
        pending.join().unwrap().unwrap();
        control.join().unwrap();
        assert!(control_result
            .expect("control command must not await network completion")
            .is_ok());
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
    fn favorite_mutations_accept_sparse_media_without_network_completion() {
        let mut coordinator = test_coordinator();
        coordinator.user_data = Some(super::LibraryUserData::default());
        let incomplete: crate::music_assistant::models::media::Track = serde_json::from_value(serde_json::json!({
            "item_id": "track", "provider": "library", "name": "Track", "uri": "library://track/track"
        })).unwrap();
        let (_, _, should_start) = coordinator.user_data.as_mut().unwrap().submit(
            crate::music_assistant::models::media::MediaItem::Track(incomplete),
            true,
        );
        assert!(should_start);
        assert_eq!(
            coordinator
                .user_data_snapshot()
                .unwrap()
                .favorites
                .tracks
                .len(),
            1
        );
    }

    #[test]
    fn restoring_an_active_session_keeps_loaded_user_data() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure session");
        coordinator.user_data = Some(super::LibraryUserData::default());
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

        assert!(coordinator
            .update_initial_user_data(old_generation, |data| data.initial_load_result =
                Some(Ok(())))
            .is_err());
        assert!(coordinator.user_data.is_none());
    }

    #[test]
    fn initial_user_data_progress_preserves_session_generation_and_workers() {
        let mut coordinator = test_coordinator();
        let generation = coordinator.user_data_session_generation;
        coordinator
            .update_initial_user_data(generation, |data| data.revision = 42)
            .unwrap();
        coordinator
            .update_initial_user_data(generation, |data| data.initial_load_result = Some(Ok(())))
            .unwrap();
        assert_eq!(coordinator.user_data_session_generation, generation);
        let data = coordinator.user_data.as_ref().unwrap();
        assert_eq!(data.revision, 42);
        assert_eq!(data.initial_load_result, Some(Ok(())));
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
