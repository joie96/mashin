//! Engine selection and canonical media-session state projection.

use super::local::playback::LocalPlayback;
use super::local::session::{AudioProfileChange, LocalSession};
use super::models::{PlaybackSessionConfiguration, PlaybackTarget};
use super::remote::playback::RemotePlayback;
use super::runtime::PeriodicWorker;
use super::start::{PlaybackStartRequest, StartKind};
use crate::core::event_channel::publish_background_event;
use crate::core::models::MediaSessionEvent;
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{QueueOption, RepeatMode};
use crate::music_assistant::playback_target::MusicAssistantPlaybackTargetConfiguration;
use crate::playback::backend::PlaybackBackend;
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{MediaSessionState, PlayerState, Queue};
use std::sync::{Arc, Mutex};

pub(crate) struct PlaybackSessionCheckpoint {
    target: Option<PlaybackTarget>,
    session: Option<Arc<LocalSession>>,
}

impl PlaybackSessionCheckpoint {
    #[cfg(test)]
    pub(crate) fn shares_local_session(&self, other: &Self) -> bool {
        match (&self.session, &other.session) {
            (Some(first), Some(second)) => Arc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        }
    }
}

pub(crate) struct PlaybackEngine {
    local: Arc<Mutex<LocalPlayback>>,
    remote: Arc<Mutex<RemotePlayback>>,
    worker: Option<PeriodicWorker>,
    target: Option<PlaybackTarget>,
    media_selection_revision: u64,
    pending_start: Option<PlaybackStartRequest>,
    has_pending_start_command: bool,
}

impl PlaybackEngine {
    // Construction and configuration; replacements are prepared before releasing the active session.
    pub(crate) fn new() -> Self {
        Self {
            local: Arc::new(Mutex::new(LocalPlayback::new())),
            remote: Arc::new(Mutex::new(RemotePlayback::new())),
            worker: None,
            target: None,
            media_selection_revision: 0,
            pending_start: None,
            has_pending_start_command: false,
        }
    }

    pub(crate) fn configure_session(
        &mut self,
        configuration: PlaybackSessionConfiguration,
        connection: Arc<MusicAssistantConnection>,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<(), String> {
        if matches!(&configuration.target, PlaybackTarget::Remote(player) if player.as_str().trim().is_empty())
        {
            return Err("Music Assistant remote player ID is required".into());
        }
        let session = self
            .local
            .lock()
            .map_err(|_| "local playback lock poisoned")?
            .prepare_session(&configuration.local, connection.clone(), runtime)?;
        self.install_session(session, configuration.target, connection)
    }

    // Lifecycle; the presentation worker stops before taking any backend lock.
    pub(crate) fn restore_session(
        &mut self,
        checkpoint: PlaybackSessionCheckpoint,
        connection: Option<Arc<MusicAssistantConnection>>,
    ) -> Result<(), String> {
        match (checkpoint.session, checkpoint.target) {
            (Some(session), Some(target)) => self.install_session(
                session,
                target,
                connection.ok_or("playback checkpoint requires its MA connection")?,
            ),
            (None, None) => {
                self.shutdown();
                Ok(())
            }
            _ => Err("incomplete playback checkpoint".into()),
        }
    }

    pub(crate) fn activate(&mut self) -> Result<(), String> {
        self.with_active_backend(|backend| backend.activate())?;
        if self.worker.is_none() {
            self.worker = Some(match self.active_mode() {
                PlaybackMode::Local => PeriodicWorker::for_backend(&self.local),
                PlaybackMode::Remote => PeriodicWorker::for_backend(&self.remote),
            });
        }
        Ok(())
    }

    pub(crate) fn deactivate(&mut self) {
        self.cancel_start();
        self.media_selection_revision = self.media_selection_revision.wrapping_add(1);
        self.worker = None;
        self.with_active_backend(|backend| {
            backend.deactivate();
            Ok(())
        })
        .expect("active playback backend must remain accessible during deactivation");
    }

    pub(crate) fn shutdown(&mut self) {
        self.cancel_start();
        self.media_selection_revision = self.media_selection_revision.wrapping_add(1);
        self.worker = None;
        self.local
            .lock()
            .expect("local playback lock poisoned")
            .shutdown();
        self.remote
            .lock()
            .expect("remote playback lock poisoned")
            .shutdown();
        self.target = None;
    }

    // Queue commands.
    pub(crate) fn reserve_media_selection(&mut self) -> u64 {
        self.media_selection_revision = self.media_selection_revision.wrapping_add(1);
        self.media_selection_revision
    }

    pub(crate) fn queue_play_media(
        &mut self,
        media_items: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    ) -> Result<(), String> {
        if matches!(option, QueueOption::Play | QueueOption::Replace) {
            self.reserve_media_selection();
        }
        let is_starting = matches!(option, QueueOption::Play | QueueOption::Replace)
            && !media_items.is_empty();
        if is_starting && self.pending_start.as_ref().is_none_or(|request| !request.is_pending()) {
            self.begin_start(StartKind::Media)?;
        } else if matches!(option, QueueOption::Play | QueueOption::Replace)
            && media_items.is_empty()
        {
            self.cancel_start();
        }
        let result = self.with_active_backend(|engine| {
            engine.queue_play_media(media_items, option, start_item, shuffle)
        });
        if is_starting {
            if result.is_ok() {
                self.has_pending_start_command = true;
            } else {
                self.cancel_start();
            }
        }
        result
    }

    pub(crate) fn move_queue_item(
        &mut self,
        item_id: QueueItemId,
        position_shift: i32,
    ) -> Result<(), String> {
        self.with_active_backend(|engine| engine.move_queue_item(item_id, position_shift))
    }

    pub(crate) fn move_queue_item_to_end(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.with_active_backend(|engine| engine.move_queue_item_to_end(item_id))
    }

    pub(crate) fn delete_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.with_active_backend(|engine| engine.delete_queue_item(item_id))
    }

    pub(crate) fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.media_selection_revision = self.media_selection_revision.wrapping_add(1);
        self.begin_start(StartKind::Media)?;
        let result = self.with_active_backend(|engine| engine.select_queue_item(item_id));
        if result.is_ok() {
            self.has_pending_start_command = true;
        } else {
            self.cancel_start();
        }
        result
    }

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.media_selection_revision = self.media_selection_revision.wrapping_add(1);
        self.begin_start(StartKind::Media)?;
        let result = self.with_active_backend(|engine| engine.play_queue_index(index));
        if result.is_ok() {
            self.has_pending_start_command = true;
        } else {
            self.cancel_start();
        }
        result
    }

    // Playback transport.
    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        self.begin_start(StartKind::Resume)?;
        let result = self.with_active_backend(|engine| engine.transport_play());
        if result.is_ok() {
            self.has_pending_start_command = true;
        } else {
            self.cancel_start();
        }
        result
    }

    pub(crate) fn transport_pause(&mut self) -> Result<(), String> {
        self.cancel_start();
        self.with_active_backend(|engine| engine.transport_pause())
    }

    pub(crate) fn transport_stop(&mut self) {
        self.cancel_start();
        self.media_selection_revision = self.media_selection_revision.wrapping_add(1);
        let _ = self.with_active_backend(|engine| {
            engine.transport_stop();
            Ok(())
        });
    }

    pub(crate) fn transport_next(&mut self) -> Result<(), String> {
        self.with_active_backend(|engine| engine.transport_next())
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        self.with_active_backend(|engine| engine.transport_previous())
    }

    pub(crate) fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        self.with_active_backend(|engine| engine.transport_seek(seconds))
    }

    // Playback preferences and committed session settings.
    pub(crate) fn set_volume(&mut self, volume: u8) {
        let _ = self.with_active_backend(|engine| {
            engine.set_volume(volume);
            Ok(())
        });
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        let _ = self.with_active_backend(|engine| {
            engine.set_muted(muted);
            Ok(())
        });
    }

    pub(crate) fn set_shuffle(&mut self, enabled: bool) {
        let _ = self.with_active_backend(|engine| {
            engine.set_shuffle(enabled);
            Ok(())
        });
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: RepeatMode) {
        let _ = self.with_active_backend(|engine| {
            engine.set_repeat_mode(mode);
            Ok(())
        });
    }

    pub(crate) fn set_autoplay(&mut self, is_enabled: bool) {
        let _ = self.with_active_backend(|engine| {
            engine.set_autoplay(is_enabled);
            Ok(())
        });
    }

    pub(crate) fn prepare_audio_profile_change(
        &self,
        profile: super::local::mashin::models::AudioProfileId,
    ) -> Result<AudioProfileChange, String> {
        self.local
            .lock()
            .map_err(|_| "local playback lock poisoned")?
            .session()
            .as_ref()
            .map(|session| LocalSession::prepare_audio_profile_change(session, profile))
            .ok_or_else(|| "local session is not configured".into())
    }

    // Identity validation and application persistence commit atomically under the caller's lock.
    pub(crate) fn commit_audio_profile_change(
        &mut self,
        change: &AudioProfileChange,
        persist: impl FnOnce(super::local::mashin::models::AudioProfileId) -> Result<(), String>,
    ) -> Result<(), String> {
        if !self
            .local
            .lock()
            .map_err(|_| "local playback lock poisoned")?
            .session()
            .as_ref()
            .is_some_and(|session| change.belongs_to(session))
        {
            return Err("session changed during profile request".into());
        }
        change.commit(persist)
    }

    pub(crate) fn take_pending_commands(
        &mut self,
    ) -> Result<Vec<crate::playback::backend::PlaybackCommand>, String> {
        let commands = self.with_active_backend(|engine| Ok(engine.take_pending_commands()))?;
        if !self.has_pending_start_command {
            return Ok(commands);
        }
        self.has_pending_start_command = false;
        let Some(request) = self.pending_start.take() else {
            return Ok(commands);
        };
        Ok(vec![Box::pin(async move {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                async move {
                    for command in commands {
                        command.await?;
                    }
                    Ok(())
                },
            )
            .await
            .unwrap_or_else(|_| Err("Playback start timed out".into()));
            request.complete(result.is_ok());
            result
        })])
    }

    // Publish intent before I/O while keeping the native/server state authoritative.
    pub(crate) fn begin_start(&mut self, kind: StartKind) -> Result<PlaybackStartRequest, String> {
        let (start, observation) = self.with_active_backend(|backend| {
            Ok((backend.start_presentation(), backend.start_observation()))
        })?;
        let request = PlaybackStartRequest::begin(start, kind, observation);
        self.pending_start = Some(request.clone());
        self.has_pending_start_command = false;
        publish_background_event(MediaSessionEvent::PlayerStateChanged {
            player_state: self.player_state(),
        });
        Ok(request)
    }

    pub(crate) fn cancel_start(&mut self) {
        self.pending_start = None;
        self.has_pending_start_command = false;
        let _ = self.with_active_backend(|backend| {
            backend
                .start_presentation()
                .lock()
                .expect("playback start lock poisoned")
                .cancel();
            Ok(())
        });
    }

    // Snapshots, request identity and checkpoints retaining native ownership for persistence rollback.
    pub(crate) fn media_selection_revision(&self) -> u64 {
        self.media_selection_revision
    }

    pub(crate) fn active_mode(&self) -> PlaybackMode {
        match self.target {
            Some(PlaybackTarget::Remote(_)) => PlaybackMode::Remote,
            _ => PlaybackMode::Local,
        }
    }

    pub(crate) fn queue(&self) -> Queue {
        self.with_active_backend(|backend| Ok(backend.queue()))
            .expect("active playback backend must remain accessible for queue reads")
    }

    pub(crate) fn player_state(&self) -> PlayerState {
        let mut player_state = self
            .with_active_backend(|backend| Ok(backend.player_state()))
            .expect("active playback backend must remain accessible for state reads");
        player_state.active_mode = self.active_mode();
        player_state
    }

    pub(crate) fn media_session_state(&self) -> MediaSessionState {
        let mut state = self
            .with_active_backend(|backend| Ok(backend.media_session_state()))
            .expect("active playback backend must remain accessible for session reads");
        state.player_state.active_mode = self.active_mode();
        state
    }

    pub(crate) fn checkpoint(&self) -> PlaybackSessionCheckpoint {
        PlaybackSessionCheckpoint {
            target: self.target.clone(),
            session: self
                .local
                .lock()
                .expect("local playback lock poisoned")
                .session(),
        }
    }

    // Session installation and fixed-backend dispatch; no extensible registry or erased ownership.
    fn install_session(
        &mut self,
        session: Arc<LocalSession>,
        target: PlaybackTarget,
        connection: Arc<MusicAssistantConnection>,
    ) -> Result<(), String> {
        self.deactivate();
        {
            let mut local = self
                .local
                .lock()
                .map_err(|_| "local playback lock poisoned")?;
            if !local
                .session()
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &session))
            {
                local.configure(session);
            }
        }
        match &target {
            PlaybackTarget::Local => {
                self.remote
                    .lock()
                    .map_err(|_| "remote playback lock poisoned")?
                    .shutdown();
            }
            PlaybackTarget::Remote(player_id) => {
                self.remote
                    .lock()
                    .map_err(|_| "remote playback lock poisoned")?
                    .configure(MusicAssistantPlaybackTargetConfiguration {
                        player_id: player_id.clone(),
                        connection,
                    })?;
            }
        }
        self.target = Some(target);
        self.activate()
    }

    fn with_active_backend<T>(
        &self,
        operation: impl FnOnce(&mut dyn PlaybackBackend) -> Result<T, String>,
    ) -> Result<T, String> {
        match self.active_mode() {
            PlaybackMode::Local => operation(
                &mut *self
                    .local
                    .lock()
                    .map_err(|_| "local playback lock poisoned")?,
            ),
            PlaybackMode::Remote => operation(
                &mut *self
                    .remote
                    .lock()
                    .map_err(|_| "remote playback lock poisoned")?,
            ),
        }
    }
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{LogLevelPreference, SelectedOutput, SessionConfig, ThemePreference};
    use crate::music_assistant::models::identifiers::{AccessToken, PlayerId};
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use crate::playback::local::session::LocalSessionConfiguration;

    #[test]
    fn selection_intent_invalidates_radio_but_transport_preferences_do_not() {
        let mut engine = PlaybackEngine::new();
        let initial = engine.media_selection_revision();
        engine.set_volume(40);
        engine.set_muted(true);
        let _ = engine.transport_pause();
        assert_eq!(engine.media_selection_revision(), initial);
        assert!(engine
            .queue_play_media(Vec::new(), QueueOption::Play, None, false)
            .is_err());
        assert_eq!(engine.media_selection_revision(), initial.wrapping_add(1));
        let _ = engine.queue_play_media(Vec::new(), QueueOption::Add, None, false);
        assert_eq!(engine.media_selection_revision(), initial.wrapping_add(1));
        engine.transport_stop();
        assert_eq!(engine.media_selection_revision(), initial.wrapping_add(2));
        let _ = engine.select_queue_item(QueueItemId("item".into()));
        assert_eq!(engine.media_selection_revision(), initial.wrapping_add(3));
        let _ = engine.play_queue_index(0);
        assert_eq!(engine.media_selection_revision(), initial.wrapping_add(4));
        engine.deactivate();
        assert_eq!(engine.media_selection_revision(), initial.wrapping_add(5));
        let older_radio = engine.reserve_media_selection();
        let newer_radio = engine.reserve_media_selection();
        assert_ne!(older_radio, newer_radio);
        assert_eq!(engine.media_selection_revision(), newer_radio);
    }

    fn configuration() -> (PlaybackSessionConfiguration, Arc<MusicAssistantConnection>) {
        let persisted = SessionConfig {
            music_assistant_url: MusicAssistantServerUrl("http://127.0.0.1:1".into()),
            access_token: Some(AccessToken("test-token".into())),
            user_id: None,
            username: "test".into(),
            selected_output: SelectedOutput::Local,
            local: crate::core::config::default_local_output(),
            volume: 30,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
        };
        let connection = Arc::new(
            MusicAssistantConnection::new(
                persisted.music_assistant_url.clone(),
                persisted.access_token.clone(),
            )
            .unwrap(),
        );
        (
            PlaybackSessionConfiguration {
                local: LocalSessionConfiguration {
                    device_id: persisted.local.device_id,
                    client_name: persisted.local.client_name,
                    cache_bytes: 1_048_576,
                    audio_profile: persisted.local.audio_profile,
                    volume: 30,
                },
                target: PlaybackTarget::Local,
            },
            connection,
        )
    }

    #[test]
    fn mode_switch_retains_local_receiver_and_audio_preferences() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut configuration, hub) = configuration();
        let mut engine = PlaybackEngine::new();
        engine
            .configure_session(configuration.clone(), hub.clone(), &runtime)
            .unwrap();
        let local = engine.checkpoint();
        engine.set_volume(44);
        for command in engine.take_pending_commands().unwrap() {
            runtime.block_on(command).unwrap();
        }
        configuration.target = PlaybackTarget::Remote(PlayerId("remote".into()));
        engine
            .configure_session(configuration.clone(), hub.clone(), &runtime)
            .unwrap();
        assert_eq!(engine.active_mode(), PlaybackMode::Remote);
        assert!(engine.checkpoint().shares_local_session(&local));
        configuration.target = PlaybackTarget::Local;
        engine
            .configure_session(configuration, hub, &runtime)
            .unwrap();
        assert_eq!(engine.active_mode(), PlaybackMode::Local);
        assert!(engine.checkpoint().shares_local_session(&local));
        assert_eq!(engine.player_state().volume, 44);
    }

    #[test]
    fn pause_stop_and_output_switch_cancel_pending_start_requests() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut configuration, connection) = configuration();
        let mut engine = PlaybackEngine::new();
        engine.configure_session(configuration.clone(), connection.clone(), &runtime).unwrap();
        let pause_request = engine.begin_start(StartKind::Media).unwrap();
        assert_eq!(engine.media_session_state().player_state.playback_state, crate::music_assistant::models::queue::PlayState::Buffering);
        engine.transport_pause().unwrap();
        assert!(!pause_request.is_pending());
        let stop_request = engine.begin_start(StartKind::Media).unwrap();
        engine.transport_stop();
        assert!(!stop_request.is_pending());
        let switched_request = engine.begin_start(StartKind::Media).unwrap();
        configuration.target = PlaybackTarget::Remote(PlayerId("remote".into()));
        engine.configure_session(configuration, connection, &runtime).unwrap();
        assert!(!switched_request.is_pending());
        switched_request.complete(false);
        assert_ne!(engine.player_state().playback_state, crate::music_assistant::models::queue::PlayState::Error);
    }

    #[test]
    fn preferences_and_enqueue_only_do_not_consume_a_media_resolution_request() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (configuration, connection) = configuration();
        let mut engine = PlaybackEngine::new();
        engine.configure_session(configuration, connection, &runtime).unwrap();
        for option in [QueueOption::Add, QueueOption::Next, QueueOption::ReplaceNext] {
            let _ = engine.queue_play_media(Vec::new(), option, None, false);
            assert!(engine.pending_start.is_none());
        }
        let request = engine.begin_start(StartKind::Media).unwrap();
        engine.set_volume(40);
        for command in engine.take_pending_commands().unwrap() {
            runtime.block_on(command).unwrap();
        }
        assert!(engine.pending_start.is_some());
        assert!(request.is_pending());
        assert!(!engine.has_pending_start_command);
    }

    #[test]
    fn profile_commit_keeps_receiver_across_mode_switch_and_propagates_storage_failure() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut configuration, connection) = configuration();
        let mut engine = PlaybackEngine::new();
        engine
            .configure_session(configuration.clone(), connection.clone(), &runtime)
            .unwrap();
        let checkpoint = engine.checkpoint();
        let profile = configuration.local.audio_profile;
        let change = engine.prepare_audio_profile_change(profile).unwrap();
        configuration.target = PlaybackTarget::Remote(PlayerId("remote".into()));
        engine
            .configure_session(configuration, connection, &runtime)
            .unwrap();
        let error = engine
            .commit_audio_profile_change(&change, |_| Err("storage unavailable".into()))
            .unwrap_err();
        assert_eq!(error, "storage unavailable");
        let mut has_committed = false;
        engine
            .commit_audio_profile_change(&change, |requested| {
                assert_eq!(requested, profile);
                has_committed = true;
                Ok(())
            })
            .unwrap();
        assert!(has_committed);
        assert!(checkpoint.shares_local_session(&engine.checkpoint()));
    }

    #[test]
    fn stale_profile_change_never_persists_into_a_replaced_session() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut configuration, connection) = configuration();
        let mut engine = PlaybackEngine::new();
        assert!(engine
            .prepare_audio_profile_change(configuration.local.audio_profile)
            .is_err());
        engine
            .configure_session(configuration.clone(), connection.clone(), &runtime)
            .unwrap();
        let change = engine
            .prepare_audio_profile_change(configuration.local.audio_profile)
            .unwrap();
        configuration.local.cache_bytes *= 2;
        engine
            .configure_session(configuration, connection, &runtime)
            .unwrap();
        let mut has_persisted = false;
        assert!(engine
            .commit_audio_profile_change(&change, |_| {
                has_persisted = true;
                Ok(())
            })
            .is_err());
        assert!(!has_persisted);
        engine.shutdown();
        assert!(engine
            .commit_audio_profile_change(&change, |_| panic!("ended session must not persist"))
            .is_err());
    }

    #[test]
    fn committed_profile_preference_does_not_recreate_receiver_on_mode_change() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut configuration, connection) = configuration();
        let mut engine = PlaybackEngine::new();
        engine
            .configure_session(configuration.clone(), connection.clone(), &runtime)
            .unwrap();
        let checkpoint = engine.checkpoint();
        let profile = "opus_192k".parse().unwrap();
        let change = engine.prepare_audio_profile_change(profile).unwrap();
        engine
            .commit_audio_profile_change(&change, |_| Ok(()))
            .unwrap();
        configuration.local.audio_profile = profile;
        configuration.target = PlaybackTarget::Remote(PlayerId("remote".into()));
        engine
            .configure_session(configuration, connection, &runtime)
            .unwrap();
        assert!(checkpoint.shares_local_session(&engine.checkpoint()));
    }

    #[test]
    fn checkpoint_restores_native_identity_after_reconfiguration() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut configuration, hub) = configuration();
        let mut engine = PlaybackEngine::new();
        engine
            .configure_session(configuration.clone(), hub.clone(), &runtime)
            .unwrap();
        let local = engine.checkpoint();
        let checkpoint = engine.checkpoint();
        configuration.local.cache_bytes *= 2;
        engine
            .configure_session(configuration, hub.clone(), &runtime)
            .unwrap();
        assert!(!engine.checkpoint().shares_local_session(&local));
        engine.restore_session(checkpoint, Some(hub)).unwrap();
        assert!(engine.checkpoint().shares_local_session(&local));
        engine.shutdown();
        assert!(!engine.checkpoint().shares_local_session(&local));
    }
}
