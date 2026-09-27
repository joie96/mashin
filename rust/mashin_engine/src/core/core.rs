//! Process-wide application core that owns session and playback state.

use super::config::{
    default_sendspin_output, session_settings, SessionSettings, DEFAULT_LOCAL_VOLUME,
};
use super::models::{
    CoreError, LogLevelPreference, LoginConfig, MediaSessionEvent, MusicAssistantQuery,
    MusicAssistantQueryRequest, SelectedOutput, SessionConfig, ThemePreference,
};
use crate::logging::Logger;
use crate::music_assistant::auth_store::{platform_auth_store, AuthStore};
use crate::music_assistant::client::{get_radio_tracks, MusicAssistantClient};
use crate::music_assistant::models::identifiers::{AccessToken, MediaId, QueueItemId};
use crate::music_assistant::models::media::{
    Album, Artist, MediaItem, MediaType, MusicAssistantSearchResults, Playlist, Track,
};
use crate::music_assistant::models::queue::{QueueItem, RepeatMode};
use crate::music_assistant::models::recommendation::MusicAssistantRecommendationFolder;
use crate::music_assistant::models::session::ProviderManifest;
use crate::music_assistant::playlist_collage::build_playlist_collage;
use crate::music_assistant::user_data::{
    flush_scheduled_write_after_debounce, persist_write_request, UserDataController,
    UserDataSnapshot, UserDataWriteRequest,
};
use crate::playback::coordinator::PlaybackCoordinator;
use crate::playback::media_resolver::{resolve_music_assistant_media_items, resolve_radio_tracks};
use crate::playback::models::{
    MediaSessionState, OfflinePlaybackConfiguration, PlaybackEngineConfiguration, PlayerState,
    Queue, RemotePlaybackConfiguration,
};
use crate::sendspin::models::SendspinEngineConfiguration;
use once_cell::sync::Lazy;
use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

pub(crate) static CORE_COORDINATOR: Lazy<Mutex<CoreCoordinator>> =
    Lazy::new(|| Mutex::new(CoreCoordinator::new()));
static CORE_COMMAND_SERIALIZER: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static LOGGER: Logger = Logger::new(module_path!());
static CORE_NETWORK_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
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
    playback: PlaybackCoordinator,
    session_configuration: Option<SessionConfig>,
    user_data: Option<UserDataController>,
    user_data_session_generation: u64,
}

impl CoreCoordinator {
    // Construction and session configuration.
    pub(crate) fn new() -> Self {
        Self::with_auth_store(platform_auth_store())
    }

    fn with_auth_store(auth_store: Box<dyn AuthStore + Send>) -> Self {
        Self {
            auth_store,
            playback: PlaybackCoordinator::new(),
            session_configuration: None,
            user_data: None,
            user_data_session_generation: 0,
        }
    }

    // Session configuration, persistence, and output selection.
    pub(crate) fn configure_session(&mut self, session: SessionConfig) -> Result<(), String> {
        self.configure_selected_output(&session)?;
        self.playback.set_volume(session.volume);
        crate::logging::set_minimum_level(session.log_level.as_str())?;
        self.auth_store.save_session(session.clone())?;
        self.invalidate_user_data();
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn restore_persisted_session(&mut self) -> Result<bool, String> {
        let Some(session) = self.auth_store.load_session()? else {
            return Ok(false);
        };
        self.configure_selected_output(&session)?;
        self.playback.set_volume(session.volume);
        crate::logging::set_minimum_level(session.log_level.as_str())?;
        self.invalidate_user_data();
        self.session_configuration = Some(session);
        Ok(true)
    }

    pub(crate) fn authenticate_and_configure_session(
        &mut self,
        login: LoginConfig,
    ) -> Result<(), String> {
        if login.music_assistant_url.0.trim().is_empty() {
            return Err("Music Assistant URL is required".to_string());
        }
        if login.username.trim().is_empty() {
            return Err("Music Assistant username is required".to_string());
        }
        if login.password.is_empty() {
            return Err("Music Assistant password is required".to_string());
        }
        let unauthenticated_client = MusicAssistantClient::new(&login.music_assistant_url.0, None);
        let login_response = CORE_NETWORK_RUNTIME
            .block_on(unauthenticated_client.login(&login.username, &login.password))?;
        let access_token = access_token_from_login_response(login_response)?;
        let authenticated_client =
            MusicAssistantClient::new(&login.music_assistant_url.0, Some(access_token.clone()));
        if !CORE_NETWORK_RUNTIME.block_on(authenticated_client.has_valid_session()) {
            return Err("Music Assistant rejected the authenticated session".to_string());
        }
        let selected_output = default_sendspin_output(&login.music_assistant_url.0)?;
        self.configure_session(SessionConfig {
            music_assistant_url: login.music_assistant_url,
            access_token: Some(access_token),
            user_id: None,
            username: login.username,
            selected_output,
            volume: DEFAULT_LOCAL_VOLUME,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
        })
    }

    pub(crate) fn select_output(&mut self, selected_output: SelectedOutput) -> Result<(), String> {
        let mut session = self.session_configuration.clone().ok_or_else(|| {
            "a restored session is required before selecting an output".to_string()
        })?;
        session.selected_output = selected_output;
        self.configure_selected_output(&session)?;
        self.auth_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn select_default_sendspin_output(&mut self) -> Result<(), String> {
        let music_assistant_url = self
            .session_configuration
            .as_ref()
            .ok_or_else(|| {
                "a restored session is required before selecting the Sendspin output".to_string()
            })?
            .music_assistant_url
            .0
            .clone();
        self.select_output(default_sendspin_output(&music_assistant_url)?)
    }

    pub(crate) fn set_local_cache_size_bytes(
        &mut self,
        cache_size_bytes: usize,
    ) -> Result<(), String> {
        const MINIMUM_CACHE_SIZE_BYTES: usize = 25 * 1024 * 1024;
        const MAXIMUM_CACHE_SIZE_BYTES: usize = 1_000 * 1024 * 1024;
        if !(MINIMUM_CACHE_SIZE_BYTES..=MAXIMUM_CACHE_SIZE_BYTES).contains(&cache_size_bytes) {
            return Err("local cache size must be between 25 and 1000 MB".to_string());
        }
        let mut session = self.session_configuration.clone().ok_or_else(|| {
            "a restored session is required before changing the local cache size".to_string()
        })?;
        let SelectedOutput::Sendspin(configuration) = &mut session.selected_output else {
            return Err("the local cache is available only with the Sendspin output".to_string());
        };
        configuration.cache_bytes = cache_size_bytes;
        self.configure_selected_output(&session)?;
        self.auth_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn set_theme(&mut self, theme: ThemePreference) -> Result<(), String> {
        let mut session = self.session_configuration.clone().ok_or_else(|| {
            "a restored session is required before changing the theme".to_string()
        })?;
        session.theme = theme;
        self.auth_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn set_log_level(&mut self, log_level: LogLevelPreference) -> Result<(), String> {
        let mut session = self.session_configuration.clone().ok_or_else(|| {
            "a restored session is required before changing the log level".to_string()
        })?;
        session.log_level = log_level;
        self.auth_store.save_session(session.clone())?;
        crate::logging::set_minimum_level(log_level.as_str())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    // A failed engine configuration leaves the current session and selected output unchanged.
    fn configure_selected_output(&mut self, session: &SessionConfig) -> Result<(), String> {
        let configuration = match &session.selected_output {
            SelectedOutput::Offline(configuration) => {
                if configuration.library_root.trim().is_empty() {
                    return Err("offline library root is required".to_string());
                }
                PlaybackEngineConfiguration::Offline(OfflinePlaybackConfiguration {
                    library_root: PathBuf::from(configuration.library_root.clone()),
                })
            }
            SelectedOutput::Sendspin(configuration) => {
                PlaybackEngineConfiguration::Sendspin(SendspinEngineConfiguration {
                    sendspin_url: configuration.sendspin_url.clone(),
                    music_assistant_url: session.music_assistant_url.0.clone(),
                    username: session.username.clone(),
                    music_assistant_auth_token: session.access_token.clone(),
                    client_id: configuration.client_id.clone(),
                    client_name: configuration.client_name.clone(),
                    downloader_buffer_bytes: configuration.downloader_buffer_bytes,
                    cache_bytes: configuration.cache_bytes,
                    preferred_codec: configuration.preferred_codec.clone(),
                })
            }
            SelectedOutput::Remote(configuration) => {
                if configuration.player_id.as_str().trim().is_empty() {
                    return Err("Music Assistant remote player ID is required".to_string());
                }
                PlaybackEngineConfiguration::Remote(RemotePlaybackConfiguration {
                    music_assistant_url: session.music_assistant_url.clone(),
                    access_token: session.access_token.clone(),
                    player_id: configuration.player_id.clone(),
                })
            }
        };
        self.playback.configure_and_select_engine(configuration)?;
        self.playback.activate_selected_engine()?;
        Ok(())
    }

    // Playback lifecycle.
    pub(crate) fn activate_selected_engine(&mut self) -> Result<(), String> {
        self.playback.activate_selected_engine()
    }

    pub(crate) fn deactivate_active_engine(&mut self) {
        self.playback.deactivate_active_engine();
    }

    // Native shutdown removes all in-memory session data after extracting the final pending write.
    pub(crate) fn shutdown(&mut self) -> Option<UserDataWriteRequest> {
        let pending_write = self.pending_user_data_write();
        self.playback.shutdown();
        self.session_configuration = None;
        self.invalidate_user_data();
        pending_write
    }

    // Queue commands.
    pub(crate) fn play_media(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        self.playback.play_media(media_items)
    }

    pub(crate) fn play_media_next(
        &mut self,
        media_items: Vec<MediaItem>,
        replace_following: bool,
    ) -> Result<(), String> {
        self.playback
            .play_media_next(media_items, replace_following)
    }

    pub(crate) fn play_media_last(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        self.playback.play_media_last(media_items)
    }

    pub(crate) fn clear_queue(&mut self) -> Result<(), String> {
        self.playback.clear_queue()
    }

    pub(crate) fn move_queue_item(
        &mut self,
        item_id: QueueItemId,
        shift: i32,
    ) -> Result<(), String> {
        self.playback.move_queue_item(item_id, shift)
    }

    pub(crate) fn delete_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.playback.delete_queue_item(item_id)
    }

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.playback.play_queue_index(index)
    }

    // Playback transport commands.
    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        self.playback.transport_play()
    }

    pub(crate) fn transport_pause(&mut self) {
        self.playback.transport_pause();
    }

    pub(crate) fn transport_stop(&mut self) {
        self.playback.transport_stop();
    }

    pub(crate) fn transport_next(&mut self) -> Result<(), String> {
        self.playback.transport_next()
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        self.playback.transport_previous()
    }

    pub(crate) fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        self.playback.transport_seek(seconds)
    }

    // Playback preference commands.
    pub(crate) fn set_volume(&mut self, volume: u8) -> Result<(), String> {
        self.playback.set_volume(volume);
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required before changing volume".to_string())?;
        session.volume = volume;
        self.auth_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn set_muted(&mut self, is_muted: bool) {
        self.playback.set_muted(is_muted);
    }

    pub(crate) fn set_shuffle(&mut self, is_enabled: bool) {
        self.playback.set_shuffle(is_enabled);
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.playback.set_repeat_mode(mode);
    }

    pub(crate) fn set_autoplay(&mut self, is_enabled: bool) {
        self.playback.set_autoplay(is_enabled);
    }

    pub(crate) fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String> {
        let preferred_codec =
            serde_json::from_value(serde_json::Value::String(codec.trim().to_ascii_lowercase()))
                .map_err(|error| format!("invalid Sendspin codec: {error}"))?;
        let mut session = self.session_configuration.clone().ok_or_else(|| {
            "a restored session is required before changing the preferred codec".to_string()
        })?;
        let SelectedOutput::Sendspin(configuration) = &mut session.selected_output else {
            return Err("preferred codec is only available for the Sendspin output".to_string());
        };
        configuration.preferred_codec = preferred_codec;
        self.playback.set_preferred_codec(codec)?;
        self.auth_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    // Platform state reads project the active engine without storing duplicate core state.
    pub(crate) fn queue(&self) -> Queue {
        self.playback.queue()
    }

    pub(crate) fn player_state(&self) -> PlayerState {
        self.playback.player_state()
    }

    pub(crate) fn media_session_state(&self) -> MediaSessionState {
        MediaSessionState {
            queue: self.queue(),
            player_state: self.player_state(),
        }
    }

    // User-data state.
    fn replace_user_data(&mut self, user_data: UserDataController) {
        self.invalidate_user_data();
        self.user_data = Some(user_data);
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

    fn mutate_user_data<F>(
        &mut self,
        operation: F,
    ) -> Result<serde_json::Map<String, serde_json::Value>, String>
    where
        F: FnOnce(&mut UserDataController) -> Result<(), String>,
    {
        let user_data = self
            .user_data
            .as_mut()
            .ok_or_else(|| "user data is not loaded".to_string())?;
        operation(user_data)?;
        user_data.preferences()
    }
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
    let queue_before = coordinator.queue();
    let player_state_before = coordinator.player_state();
    operation(&mut coordinator)
        .map_err(CoreError::command_failed)
        .map(|()| collect_media_session_events(&coordinator, queue_before, player_state_before))
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
    let queue_before = coordinator.queue();
    let player_state_before = coordinator.player_state();
    let has_restored_session = coordinator
        .restore_persisted_session()
        .map_err(CoreError::command_failed)?;
    let events = collect_media_session_events(&coordinator, queue_before, player_state_before);
    Ok((has_restored_session, events))
}

// Resolves remote media before acquiring the coordinator for its final queue mutation.
pub(crate) fn resolve_media_and_execute_command<F>(
    media_items: Vec<MediaItem>,
    should_shuffle: bool,
    operation: F,
) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut CoreCoordinator, Vec<MediaItem>) -> Result<(), String>,
{
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let session = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .session_configuration
        .clone()
        .ok_or_else(|| {
            CoreError::command_failed("a restored session is required to resolve media")
        })?;
    let client = MusicAssistantClient::new(&session.music_assistant_url.0, session.access_token);
    let mut resolved_media_items = CORE_NETWORK_RUNTIME
        .block_on(resolve_music_assistant_media_items(&client, media_items))
        .map_err(CoreError::command_failed)?
        .into_iter()
        .map(MediaItem::Track)
        .collect::<Vec<_>>();
    if should_shuffle {
        resolved_media_items.shuffle(&mut rand::thread_rng());
    }
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let queue_before = coordinator.queue();
    let player_state_before = coordinator.player_state();
    operation(&mut coordinator, resolved_media_items)
        .map_err(CoreError::command_failed)
        .map(|()| collect_media_session_events(&coordinator, queue_before, player_state_before))
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
pub(crate) fn load_user_data() -> Result<UserDataSnapshot, CoreError> {
    let client = music_assistant_client_for_query("load user data")?;
    let user = CORE_NETWORK_RUNTIME
        .block_on(client.get_current_user())
        .map_err(CoreError::command_failed)?;
    let user_data = UserDataController::from_preferences(user.preferences)
        .map_err(CoreError::command_failed)?;
    let snapshot = user_data.snapshot();
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    coordinator.replace_user_data(user_data);
    Ok(snapshot)
}

pub(crate) fn read_user_data() -> Result<UserDataSnapshot, CoreError> {
    CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .user_data_snapshot()
        .map_err(CoreError::command_failed)
}

pub(crate) fn mutate_user_data<F>(operation: F) -> Result<UserDataSnapshot, CoreError>
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
    Ok(snapshot)
}

/// Applies a local playlist mutation, renders its cover outside the coordinator lock, then persists both.
pub(crate) fn mutate_local_playlist_tracks<F>(
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

pub(crate) fn search_music_assistant(
    query: &str,
    limit: u32,
) -> Result<MusicAssistantSearchResults, CoreError> {
    let client = music_assistant_client_for_query("search Music Assistant")?;
    CORE_NETWORK_RUNTIME
        .block_on(client.search_media(
            query,
            &[
                MediaType::Track,
                MediaType::Album,
                MediaType::Playlist,
                MediaType::Artist,
            ],
            limit.clamp(1, 100),
        ))
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

// Starts selected radio tracks before loading and appending dynamic recommendations.
pub(crate) fn start_sendspin_radio_and_execute_command(
    media_items: Vec<MediaItem>,
    publish_start_events: impl FnOnce(Vec<MediaSessionEvent>),
) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let _command_guard = CORE_COMMAND_SERIALIZER
        .lock()
        .map_err(|_| CoreError::internal("core command serializer lock poisoned"))?;
    let (session, active_engine_kind, local_playlist_tracks) = {
        let coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        let session = coordinator.session_configuration.clone().ok_or_else(|| {
            CoreError::command_failed("a restored session is required to resolve radio media")
        })?;
        let local_playlist_tracks = if media_items.iter().any(|item| matches!(
            item,
            MediaItem::Playlist(playlist)
                if playlist.media.provider.as_str().eq_ignore_ascii_case(crate::music_assistant::user_data::LOCAL_PLAYLIST_PROVIDER)
        )) {
            coordinator
                .user_data_snapshot()
                .map_err(CoreError::command_failed)?
                .playlists
                .into_iter()
                .map(|playlist| (playlist.playlist.media.item_id, playlist.items))
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };
        (
            session,
            coordinator.playback.active_engine_kind(),
            local_playlist_tracks,
        )
    };
    if active_engine_kind != crate::playback::models::PlaybackEngineKind::Sendspin {
        return Err(CoreError::command_failed(
            "Sendspin radio is only available with the Sendspin output",
        ));
    }
    let client = MusicAssistantClient::new(&session.music_assistant_url.0, session.access_token);
    let selection = CORE_NETWORK_RUNTIME
        .block_on(resolve_radio_tracks(
            &client,
            media_items,
            &local_playlist_tracks,
        ))
        .map_err(CoreError::command_failed)?;
    let start_events = {
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        let queue_before = coordinator.queue();
        let player_state_before = coordinator.player_state();
        coordinator
            .playback
            .start_radio(
                selection
                    .started_tracks
                    .iter()
                    .cloned()
                    .map(MediaItem::Track)
                    .collect(),
            )
            .map_err(CoreError::command_failed)?;
        collect_media_session_events(&coordinator, queue_before, player_state_before)
    };
    publish_start_events(start_events);
    let radio_tracks = CORE_NETWORK_RUNTIME
        .block_on(get_radio_tracks(
            &client,
            &selection.radio_seed,
            selection.started_tracks,
        ))
        .map_err(CoreError::command_failed)?;
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let queue_before = coordinator.queue();
    let player_state_before = coordinator.player_state();
    coordinator
        .playback
        .append_radio_tracks(selection.autoplay_seed_tracks, radio_tracks)
        .map_err(CoreError::command_failed)
        .map(|()| collect_media_session_events(&coordinator, queue_before, player_state_before))
}

// Initial reads use the same complete contracts as their corresponding events.
pub(crate) fn read_queue() -> Result<Queue, CoreError> {
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    Ok(coordinator.queue())
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
    Ok(coordinator.player_state())
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

fn access_token_from_login_response(
    login_response: serde_json::Value,
) -> Result<AccessToken, String> {
    if login_response
        .get("success")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        return Err("Music Assistant login failed".to_string());
    }
    let token = login_response
        .get("token")
        .or_else(|| login_response.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            "Music Assistant login response did not contain an access token".to_string()
        })?;
    Ok(AccessToken(token.to_string()))
}

fn tracks_from_media_items(media_items: &[MediaItem]) -> Vec<Track> {
    media_items
        .iter()
        .filter_map(|media_item| match media_item {
            MediaItem::Track(track) => Some(track.clone()),
            _ => None,
        })
        .collect()
}

fn tracks_from_queue(queue_items: &[QueueItem]) -> Vec<Track> {
    queue_items
        .iter()
        .filter_map(|item| match &item.media_item {
            Some(MediaItem::Track(track)) => Some(track.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::CoreCoordinator;
    use crate::core::models::{
        LogLevelPreference, SelectedOutput, SendspinOutputConfig, SessionConfig, ThemePreference,
    };
    use crate::music_assistant::auth_store::AuthStore;
    use crate::music_assistant::models::identifiers::AccessToken;
    use crate::music_assistant::models::queue::PlayState;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use crate::sendspin::models::SendspinAudioCodec;
    use std::sync::Mutex;

    struct TestAuthStore {
        session: Mutex<Option<SessionConfig>>,
    }

    impl TestAuthStore {
        fn new() -> Self {
            Self {
                session: Mutex::new(None),
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

    fn session_config() -> SessionConfig {
        SessionConfig {
            music_assistant_url: MusicAssistantServerUrl("https://music-assistant.example".into()),
            access_token: Some(AccessToken("secret-token".into())),
            user_id: None,
            username: "mashin".into(),
            selected_output: SelectedOutput::Sendspin(SendspinOutputConfig {
                sendspin_url: "ws://sendspin.example".into(),
                client_id: "mashin-test".into(),
                client_name: "Mashin Test".into(),
                downloader_buffer_bytes: 1_024,
                cache_bytes: 1_024,
                preferred_codec: SendspinAudioCodec::Pcm,
            }),
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
        let queue = coordinator.queue();
        let player_state = coordinator.player_state();

        assert!(coordinator.session_configuration.is_some());
        assert!(queue.items.is_empty());
        assert_eq!(player_state.playback_state, PlayState::Idle);
    }

    #[test]
    fn failed_configuration_does_not_create_a_core_session() {
        let mut coordinator = test_coordinator();
        let mut invalid_session = session_config();
        let SelectedOutput::Sendspin(configuration) = &mut invalid_session.selected_output else {
            panic!("test session must select Sendspin");
        };
        configuration.sendspin_url.clear();

        assert!(coordinator.configure_session(invalid_session).is_err());

        assert!(coordinator.session_configuration.is_none());
    }

    #[test]
    fn select_output_configures_the_requested_sendspin_engine() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("restore session");

        coordinator
            .select_output(session_config().selected_output)
            .expect("select Sendspin output");

        assert_eq!(
            coordinator.player_state().active_engine_kind,
            crate::playback::models::PlaybackEngineKind::Sendspin
        );
    }

    #[test]
    fn local_cache_size_is_reconfigured_and_persisted_in_the_session() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure session");

        coordinator
            .set_local_cache_size_bytes(512 * 1024 * 1024)
            .expect("update local cache size");

        let configuration = coordinator
            .session_configuration
            .as_ref()
            .expect("session configuration");
        let SelectedOutput::Sendspin(output) = &configuration.selected_output else {
            panic!("test session must select Sendspin");
        };
        assert_eq!(output.cache_bytes, 512 * 1024 * 1024);
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
            .set_theme(ThemePreference::Light)
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
            .set_log_level(LogLevelPreference::Warn)
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
    fn unavailable_output_does_not_replace_the_active_sendspin_engine() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("restore Sendspin session");
        let mut offline_session = session_config();
        offline_session.selected_output =
            SelectedOutput::Offline(crate::core::models::OfflineOutputConfig {
                library_root: "C:/Music".into(),
            });

        let error = coordinator
            .configure_session(offline_session)
            .expect_err("offline engine is not implemented");

        assert_eq!(error, "playback engine 'offline' is not implemented");
        assert!(coordinator.session_configuration.is_some());
        assert_eq!(
            coordinator.player_state().active_engine_kind,
            crate::playback::models::PlaybackEngineKind::Sendspin
        );
    }

    #[test]
    fn unavailable_remote_output_does_not_replace_the_active_sendspin_engine() {
        let mut coordinator = test_coordinator();
        coordinator
            .configure_session(session_config())
            .expect("configure Sendspin session");
        let mut remote_session = session_config();
        remote_session.selected_output =
            SelectedOutput::Remote(crate::core::models::RemoteOutputConfig {
                player_id: crate::music_assistant::models::identifiers::PlayerId(
                    "remote-player".into(),
                ),
            });

        let error = coordinator
            .configure_session(remote_session)
            .expect_err("remote engine is not implemented");

        assert_eq!(error, "playback engine 'remote' is not implemented");
        assert_eq!(
            coordinator.player_state().active_engine_kind,
            crate::playback::models::PlaybackEngineKind::Sendspin
        );
    }

    #[test]
    fn restored_session_configures_playback_without_exposing_its_token() {
        let mut coordinator = test_coordinator();

        coordinator
            .configure_session(session_config())
            .expect("restore session");
        let serialized_queue =
            serde_json::to_string(&coordinator.queue()).expect("serialize queue");
        let serialized_player_state =
            serde_json::to_string(&coordinator.player_state()).expect("serialize player state");

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

        assert_eq!(coordinator.player_state().volume, 37);
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
            coordinator.player_state().playback_state,
            PlayState::Uninitialized
        );

        assert!(coordinator
            .restore_persisted_session()
            .expect("restore persisted session"));
        assert!(coordinator.session_configuration.is_some());
    }
}
