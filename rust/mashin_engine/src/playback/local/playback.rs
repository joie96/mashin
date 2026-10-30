//! Mashin imports the initial MA selection once; subsequent playback belongs to its DAC.
//! Deactivating this presentation never drops the session-owned audio receiver.

use crate::audio::models::AudioCommand;
use crate::core::event_channel::publish_background_event;
use crate::core::models::MediaSessionEvent;
use crate::music_assistant::client::PlayerCommand;
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::models::identifiers::{PlayerId, QueueItemId};
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{PlayState, QueueOption, RepeatMode};
use crate::music_assistant::playback_target::MusicAssistantPlaybackTarget;
use crate::music_assistant::playback_target::MusicAssistantPlaybackTargetConfiguration;
use crate::playback::backend::{PlaybackBackend, PlaybackCommand};
use crate::playback::local::mashin::audio_adapter::OutputSnapshot;
use crate::playback::local::mashin::models::OutputState;
use crate::playback::local::mashin::models::StreamDescriptor;
use crate::playback::local::mashin::receiver::{ConnectionState, ReceiverSnapshot};
use crate::playback::local::mashin::recovery::RecoveryTarget;
use crate::playback::local::session::{LocalSession, LocalSessionConfiguration};
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{MediaSessionState, PlaybackConnectionState, PlayerState, Queue};
use crate::playback::start::{PlaybackStart, StartIdentity, StartObservation};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

pub(crate) struct LocalPlayback {
    session: Option<Arc<LocalSession>>,
    target: MusicAssistantPlaybackTarget,
    projected_player_id: Option<String>,
    pending_commands: Vec<PlaybackCommand>,
    last_queue: Option<Queue>,
    last_player: Option<PlayerState>,
    last_stream_id: Option<Uuid>,
    start: Arc<Mutex<PlaybackStart>>,
}

impl LocalPlayback {
    // Construction.
    pub(crate) fn new() -> Self {
        Self {
            session: None,
            target: MusicAssistantPlaybackTarget::new(),
            projected_player_id: None,
            pending_commands: Vec::new(),
            last_queue: None,
            last_player: None,
            last_stream_id: None,
            start: Arc::new(Mutex::new(PlaybackStart::default())),
        }
    }

    // Preparation leaves the active receiver owned until installation or rollback completes.
    pub(crate) fn prepare_session(
        &self,
        configuration: &LocalSessionConfiguration,
        connection: Arc<MusicAssistantConnection>,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<Arc<LocalSession>, String> {
        if let Some(session) = self.session.as_ref() {
            if session.matches(configuration, &connection)? {
                return Ok(session.clone());
            }
        }
        Ok(Arc::new(LocalSession::new(
            runtime,
            configuration,
            connection,
        )?))
    }

    pub(crate) fn configure(&mut self, session: Arc<LocalSession>) {
        self.shutdown();
        self.session = Some(session);
    }

    // Retained native ownership for engine installation and rollback.
    pub(crate) fn session(&self) -> Option<Arc<LocalSession>> {
        self.session.clone()
    }

    // Local controls are accepted by the same native owner as provider commands.
}

impl PlaybackBackend for LocalPlayback {
    // Presentation-only requests are independent from the protocol's native output state.
    fn start_presentation(&self) -> Arc<Mutex<PlaybackStart>> {
        self.start.clone()
    }

    fn start_observation(&self) -> StartObservation {
        let output = self.session.as_ref().map(|session| session.output.snapshot());
        StartObservation {
            identity: StartIdentity::Local(output.as_ref().and_then(|output| output.current.as_ref())
                .map(|stream| stream.stream_id)),
            revision: output.as_ref().map(|output| output.resume.output_revision).unwrap_or(0),
            state: output.as_ref().map(|output| match output.resume.output_state {
                OutputState::Idle | OutputState::Ended => PlayState::Idle,
                OutputState::Buffering => PlayState::Buffering,
                OutputState::Playing => PlayState::Playing,
                OutputState::Paused => PlayState::Paused,
                OutputState::Error => PlayState::Error,
            }).unwrap_or(PlayState::Uninitialized),
        }
    }

    // Configuration and presentation lifecycle.
    fn activate(&mut self) -> Result<(), String> {
        self.synchronize_projection()
    }
    fn deactivate(&mut self) {
        self.start.lock().expect("playback start lock poisoned").cancel();
        self.target.deactivate();
        self.projected_player_id = None;
        self.last_queue = None;
        self.last_player = None;
        self.last_stream_id = None;
    }
    fn shutdown(&mut self) {
        self.deactivate();
        self.target.shutdown();
        self.session = None;
        self.pending_commands.clear();
    }

    // MA owns queue composition, shuffle, repeat and radio recommendations.
    // Server-owned queue commands.
    fn queue_play_media(
        &mut self,
        media: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    ) -> Result<(), String> {
        self.synchronize_projection()?;
        if matches!(option, QueueOption::Play | QueueOption::Replace) {
            self.take_playback_authority();
        } else {
            self.take_local_authority();
        }
        self.target
            .queue_play_media(media, option, start_item, shuffle)
    }
    fn move_queue_item(&mut self, item_id: QueueItemId, position_shift: i32) -> Result<(), String> {
        self.take_local_authority();
        self.target.move_queue_item(item_id, position_shift)
    }
    fn move_queue_item_to_end(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.take_local_authority();
        self.target.move_queue_item_to_end(item_id)
    }
    fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String> {
        self.take_local_authority();
        self.target.delete_queue_items(item_ids)
    }
    fn select_queue_item(&mut self, id: QueueItemId) -> Result<(), String> {
        self.take_playback_authority();
        self.target.select_queue_item(id)
    }
    fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.take_playback_authority();
        self.target.play_queue_index(index)
    }

    // Manual next consumes only MA's already registered successor grant, even offline.
    // Local transport and shared queue preferences.
    fn transport_play(&mut self) -> Result<(), String> {
        self.synchronize_projection()?;
        if self.session.as_ref().is_some_and(|session| session.output.recovery().borrow().target.is_some()) {
            return self.local_command(AudioCommand::Play { stream_id: None });
        }
        self.take_playback_authority();
        if self.session.as_ref().is_some_and(|session| session.output.snapshot().current.is_none()) {
            return self.target.transport_play();
        }
        self.local_command(AudioCommand::Play { stream_id: None })
    }
    fn transport_pause(&mut self) -> Result<(), String> {
        self.local_command(AudioCommand::Pause { stream_id: None })
    }
    fn transport_stop(&mut self) {
        let result = self.local_command(AudioCommand::Stop { stream_id: None });
        self.record_failure(result);
    }
    fn transport_next(&mut self) -> Result<(), String> {
        self.take_playback_authority();
        if let Some(session) = self.session.as_ref() {
            let snapshot = session.output.snapshot();
            if let (Some(current), Some(prepared)) = (snapshot.current, snapshot.prepared) {
                return self.local_command(AudioCommand::ActivatePrepared {
                    current_stream_id: current.stream_id,
                    stream_id: prepared.stream_id,
                    media_revision: snapshot.resume.media_revision,
                });
            }
            if snapshot.resume.activation.is_some() {
                return Err("Awaiting a new MA-authorized successor".into());
            }
        }
        self.target.transport_next()
    }
    fn transport_previous(&mut self) -> Result<(), String> {
        self.take_playback_authority();
        self.target.transport_previous()
    }
    fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        let position_millis = seek_position_millis(seconds)?;
        self.take_playback_authority();
        let session = self
            .session
            .as_ref()
            .ok_or("Mashin session is not configured")?
            .clone();
        let stream_id = session
            .output
            .snapshot()
            .current
            .ok_or("No active Mashin stream")?
            .stream_id;
        self.pending_commands.push(Box::pin(async move {
            let receiver = session.receiver.snapshot();
            if receiver.control_connection != ConnectionState::Connected {
                return Err("Local seek requires an active Mashin Control connection".into());
            }
            if session
                .output
                .seek_cached(stream_id, position_millis)
                .await?
            {
                return Ok(());
            }
            if session.receiver.snapshot().control_connection != ConnectionState::Connected
                || session.output.snapshot().resume.current_stream_id != Some(stream_id)
            {
                return Err("Mashin playback changed before seek fallback".into());
            }
            let player_id = PlayerId(
                receiver
                    .player_id
                    .ok_or("Mashin player is not registered")?,
            );
            session
                .connection
                .client
                .execute_player_command(
                    &player_id,
                    PlayerCommand::Seek((position_millis / 1000) as f64),
                )
                .await
        }));
        Ok(())
    }
    fn set_volume(&mut self, volume: u8) {
        let result = self.local_command(AudioCommand::SetVolume {
            volume_percent: volume,
        });
        self.record_failure(result);
    }
    fn set_muted(&mut self, is_muted: bool) {
        let result = self.local_command(AudioCommand::SetMute { is_muted });
        self.record_failure(result);
    }
    fn set_shuffle(&mut self, enabled: bool) {
        self.take_local_authority();
        self.target.set_shuffle(enabled);
    }
    fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.take_local_authority();
        self.target.set_repeat_mode(mode);
    }
    fn set_autoplay(&mut self, enabled: bool) {
        self.take_local_authority();
        self.target.set_autoplay(enabled);
    }
    fn take_pending_commands(&mut self) -> Vec<PlaybackCommand> {
        let mut commands = std::mem::take(&mut self.pending_commands);
        commands.extend(self.target.take_pending_commands());
        commands
    }

    // Only the initial selection uses MA timing; audible playback always uses local timing.
    // Queue and audible player projections.
    fn queue(&self) -> Queue {
        self.media_session_state().queue
    }
    fn player_state(&self) -> PlayerState {
        self.media_session_state().player_state
    }
    fn media_session_state(&self) -> MediaSessionState {
        self.presentation_snapshot().0
    }

    // Presentation polling does not own the session's native audio worker.
    // Presentation event publication.
    fn process_playback_cycle(&mut self) {
        if self.synchronize_projection().is_err() {
            return;
        }
        let (state, stream_id) = self.presentation_snapshot();
        for event in self.presentation_events(state, stream_id) {
            publish_background_event(event);
        }
    }
}

impl Drop for LocalPlayback {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl LocalPlayback {
    // One audio snapshot drives queue identity, timing and publication invalidation.
    fn presentation_snapshot(&self) -> (MediaSessionState, Option<Uuid>) {
        let Some(session) = self.session.as_ref() else {
            let mut queue = self.target.queue();
            queue.current_index = None;
            return (
                MediaSessionState {
                    queue,
                    player_state: PlayerState {
                        active_mode: PlaybackMode::Local,
                        ..PlayerState::default()
                    },
                },
                None,
            );
        };
        let output = session.output.snapshot();
        let stream_id = output.current.as_ref().map(|stream| stream.stream_id);
        let synchronized = self.target.synchronized_snapshot();
        let mut state = project_media_session_state(
            synchronized.as_ref().map(|snapshot| snapshot.queue.clone()).unwrap_or_else(|| self.target.queue()),
                &output,
                &session.receiver.snapshot(),
                self.last_queue.as_ref(),
            );
        state.player_state.playback_state = self.start.lock().expect("playback start lock poisoned")
            .project(StartObservation {
                identity: StartIdentity::Local(stream_id),
                revision: output.resume.output_revision,
                state: state.player_state.playback_state,
            }, Instant::now());
        let recovery = session.output.recovery().borrow().target.clone();
        if output.current.is_none() {
            if let Some(target) = &recovery {
                project_recovery_intent(&mut state, target);
            }
        }
        let has_local_activity = output.current.is_some()
            || recovery.is_some()
            || output.resume.output_revision != 0
            || matches!(state.player_state.playback_state, PlayState::Buffering | PlayState::Error);
        session.queue_bootstrap.lock().expect("queue bootstrap lock poisoned").project(
            &mut state,
            synchronized.as_ref(),
            has_local_activity,
        );
        (state, stream_id)
    }

    fn presentation_events(
        &mut self,
        state: MediaSessionState,
        stream_id: Option<Uuid>,
    ) -> Vec<MediaSessionEvent> {
        let mut events = Vec::new();
        if self.last_stream_id != stream_id || self.last_queue.as_ref() != Some(&state.queue) {
            self.last_stream_id = stream_id;
            self.last_queue = Some(state.queue.clone());
            events.push(MediaSessionEvent::QueueChanged { queue: state.queue });
        }
        if self.last_player.as_ref() != Some(&state.player_state) {
            self.last_player = Some(state.player_state.clone());
            events.push(MediaSessionEvent::PlayerStateChanged {
                player_state: state.player_state,
            });
        }
        events
    }

    // Native command adaptation and MA projection synchronization.
    fn take_local_authority(&self) {
        if let Some(session) = &self.session {
            session.queue_bootstrap.lock().expect("queue bootstrap lock poisoned").take_local_authority();
        }
    }

    fn take_playback_authority(&self) {
        self.take_local_authority();
        if let Some(session) = &self.session {
            session.output.cancel_recovery();
        }
    }

    fn local_command(&mut self, command: AudioCommand) -> Result<(), String> {
        let session = self
            .session
            .as_ref()
            .ok_or("Mashin session is not configured")?;
        if !matches!(command, AudioCommand::SetVolume { .. } | AudioCommand::SetMute { .. }) {
            session.queue_bootstrap.lock().expect("queue bootstrap lock poisoned").take_local_authority();
        }
        if session.output.authorize_local_command(&command) {
            return Ok(());
        }
        let output = session.output.pipeline();
        self.pending_commands.push(Box::pin(async move {
            output.execute(command).await.map(|_| ())
        }));
        Ok(())
    }

    fn record_failure(&mut self, result: Result<(), String>) {
        if let Err(error) = result {
            self.pending_commands
                .push(Box::pin(async move { Err(error) }));
        }
    }

    // The provider assigns the real MA player identity in welcome.
    fn synchronize_projection(&mut self) -> Result<(), String> {
        let session = self
            .session
            .as_ref()
            .ok_or("Mashin session is not configured")?;
        let Some(player_id) = session.receiver.snapshot().player_id else {
            return Ok(());
        };
        if self.projected_player_id.as_ref() == Some(&player_id) {
            return Ok(());
        }
        self.target
            .configure(MusicAssistantPlaybackTargetConfiguration {
                player_id: PlayerId(player_id.clone()),
                connection: session.connection.clone(),
            })?;
        self.target.activate()?;
        session.queue_bootstrap.lock().expect("queue bootstrap lock poisoned").start_loading();
        self.projected_player_id = Some(player_id);
        Ok(())
    }
}

// The audible stream owns the local cursor, never the last server-reported index.
fn project_queue(
    mut queue: Queue,
    current: Option<&StreamDescriptor>,
    previous: Option<&Queue>,
) -> Queue {
    queue.current_index = None;
    let Some(stream) = current.filter(|stream| stream.playback.queue_id == queue.queue_id) else {
        return queue;
    };
    let item_index = |items: &[crate::music_assistant::models::queue::QueueItem]| {
        items.iter().position(|item| {
            item.queue_id == stream.playback.queue_id
                && item.queue_item_id == stream.playback.queue_item_id
        })
    };
    queue.current_index = item_index(&queue.items);
    if queue.current_index.is_none() {
        if let Some((previous, index)) = previous
            .filter(|previous| previous.queue_id == queue.queue_id)
            .and_then(|previous| item_index(&previous.items).map(|index| (previous, index)))
        {
            queue.items = previous.items.clone();
            queue.current_index = Some(index);
        }
    }
    queue
}

// Read-only local projection; server snapshots cannot override the audible cursor.
fn project_media_session_state(
    queue: Queue,
    output: &OutputSnapshot,
    connection: &ReceiverSnapshot,
    previous: Option<&Queue>,
) -> MediaSessionState {
    let queue = project_queue(queue, output.current.as_ref(), previous);
    let position_millis = output.current.as_ref().map(|stream| {
        stream.media_start_position_millis.saturating_add(
            output.resume.position_frame.saturating_mul(1000)
                / u64::from(stream.output_format.sample_rate_hz),
        ) as i64
    });
    let player_state = PlayerState {
        active_mode: PlaybackMode::Local,
        connection_state: if connection.control_connection == ConnectionState::Connected
            && connection.audio_connection == ConnectionState::Connected
        {
            PlaybackConnectionState::Online
        } else if connection.control_connection == ConnectionState::Connecting {
            PlaybackConnectionState::Connecting
        } else {
            PlaybackConnectionState::Reconnecting
        },
        playback_state: match output.resume.output_state {
            OutputState::Idle | OutputState::Ended => PlayState::Idle,
            OutputState::Buffering => PlayState::Buffering,
            OutputState::Playing => PlayState::Playing,
            OutputState::Paused => PlayState::Paused,
            OutputState::Error => PlayState::Error,
        },
        current_queue_item_id: output
            .current
            .as_ref()
            .map(|stream| stream.playback.queue_item_id.clone()),
        current_index: queue.current_index,
        position_millis,
        duration_millis: output
            .current
            .as_ref()
            .and_then(|stream| stream.duration_millis.map(|duration| duration as i64)),
        buffered_position_millis: position_millis
            .map(|position| position.saturating_add(output.buffered_duration_millis as i64)),
        volume: output.resume.volume_percent,
        muted: output.resume.is_muted,
    };
    MediaSessionState {
        queue,
        player_state,
    }
}

// Presentation retains intent while the protocol truthfully reports an empty output cache.
fn project_recovery_intent(state: &mut MediaSessionState, target: &RecoveryTarget) {
    state.player_state.playback_state = if target.is_paused {
        PlayState::Paused
    } else {
        PlayState::Buffering
    };
    state.player_state.current_queue_item_id = Some(target.playback.queue_item_id.clone());
    state.player_state.position_millis = Some((target.position_seconds * 1000) as i64);
    state.player_state.buffered_position_millis = state.player_state.position_millis;
    if state.queue.queue_id == target.playback.queue_id {
        state.queue.current_index = state.queue.items.iter()
            .position(|item| item.queue_item_id == target.playback.queue_item_id);
        state.player_state.current_index = state.queue.current_index;
        state.player_state.duration_millis = state.queue.current_index
            .and_then(|index| state.queue.items[index].duration)
            .map(|seconds| i64::from(seconds) * 1000);
    }
}

// Cached chunks start at whole seconds; invalid UI values never reach either seek path.
fn seek_position_millis(seconds: f64) -> Result<u64, String> {
    if !seconds.is_finite()
        || seconds < 0.0
        || seconds >= (super::mashin::models::MAX_INTEGER / 1000) as f64
    {
        return Err(
            "seek position must be finite, nonnegative and within the supported range".into(),
        );
    }
    Ok((seconds.floor() as u64) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_queue_cursor_tracks_stream_identity_not_server_index() {
        let mut stream = super::super::mashin::models::test_stream();
        let queue: Queue = Queue {
            queue_id: stream.playback.queue_id.clone(),
            items: vec![
                queue_item(&stream, "a"),
                queue_item(&stream, "c"),
                queue_item(&stream, "b"),
            ],
            current_index: Some(0),
            shuffle_enabled: true,
            repeat_mode: RepeatMode::All,
            autoplay_enabled: true,
        };
        stream.playback.queue_item_id = QueueItemId("b".into());
        let projected = project_queue(queue.clone(), Some(&stream), None);
        assert_eq!(projected.current_index, Some(2));
        assert_eq!(projected.items, queue.items);
        assert!(projected.shuffle_enabled && projected.autoplay_enabled);
        assert_eq!(projected.repeat_mode, RepeatMode::All);
        assert_eq!(queue.current_index, Some(0));
        assert_eq!(
            project_queue(projected.clone(), Some(&stream), None),
            projected
        );
        assert_eq!(project_queue(queue.clone(), None, None).current_index, None);
        stream.playback.queue_item_id = QueueItemId("missing".into());
        assert_eq!(
            project_queue(queue.clone(), Some(&stream), None).current_index,
            None
        );
        stream.playback.queue_item_id = QueueItemId("b".into());
        stream.playback.queue_id.0 = "different-queue".into();
        assert_eq!(
            project_queue(queue, Some(&stream), None).current_index,
            None
        );
    }

    #[test]
    fn recovery_presentation_keeps_play_pause_and_position_without_faking_cache() {
        let stream = super::super::mashin::models::test_stream();
        let mut state = MediaSessionState {
            queue: Queue { queue_id: stream.playback.queue_id.clone(), items: vec![queue_item(&stream, "item")], ..Default::default() },
            player_state: PlayerState::default(),
        };
        let mut target = RecoveryTarget { playback: stream.playback, position_seconds: 162, is_paused: false };
        project_recovery_intent(&mut state, &target);
        assert_eq!(state.player_state.playback_state, PlayState::Buffering);
        assert_eq!(state.player_state.position_millis, Some(162_000));
        assert_eq!(state.player_state.current_queue_item_id, Some(QueueItemId("item".into())));
        assert_eq!(state.queue.current_index, Some(0));
        target.is_paused = true;
        project_recovery_intent(&mut state, &target);
        assert_eq!(state.player_state.playback_state, PlayState::Paused);
    }

    #[test]
    fn cached_seek_rounds_down_and_rejects_invalid_ui_targets() {
        assert_eq!(seek_position_millis(40.9).unwrap(), 40_000);
        assert_eq!(seek_position_millis(0.0).unwrap(), 0);
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, f64::MAX] {
            assert!(seek_position_millis(invalid).is_err());
        }
    }

    #[test]
    fn cached_seek_requires_live_control_before_either_seek_path() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let connection = Arc::new(
                MusicAssistantConnection::new(
                    crate::music_assistant::models::session::MusicAssistantServerUrl(
                        "http://127.0.0.1:1".into(),
                    ),
                    Some(crate::music_assistant::models::identifiers::AccessToken(
                        "test-token".into(),
                    )),
                )
                .unwrap(),
            );
            let session = Arc::new(
                LocalSession::new(
                    &runtime,
                    &LocalSessionConfiguration {
                        device_id: "test-device".into(),
                        client_name: "Seek test".into(),
                        cache_bytes: 1_048_576,
                        audio_profile: super::super::mashin::models::AudioProfileId::Flac,
                        volume: 30,
                    },
                    connection,
                )
                .unwrap(),
            );
            session
                .output
                .execute(super::super::mashin::protocol::ControlCommand::SetMedia {
                    command_id: 1,
                    current: Some(super::super::mashin::models::test_stream()),
                    prepared: None,
                    media_revision: 1,
                    initial_output_state: OutputState::Paused,
                })
                .await
                .unwrap();
            let before = session.output.snapshot();
            let mut engine = LocalPlayback::new();
            engine.configure(session.clone());
            engine.transport_seek(40.9).unwrap();
            let commands = engine.take_pending_commands();
            assert_eq!(commands.len(), 1);
            for command in commands {
                assert_eq!(
                    command.await.unwrap_err(),
                    "Local seek requires an active Mashin Control connection"
                );
            }
            assert_eq!(session.output.snapshot(), before);
        });
    }

    #[test]
    fn start_presentation_does_not_change_native_protocol_state() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let connection = Arc::new(MusicAssistantConnection::new(
            crate::music_assistant::models::session::MusicAssistantServerUrl("http://127.0.0.1:1".into()),
            Some(crate::music_assistant::models::identifiers::AccessToken("test-token".into())),
        ).unwrap());
        let session = Arc::new(LocalSession::new(&runtime, &LocalSessionConfiguration {
            device_id: "start-test".into(),
            client_name: "Start test".into(),
            cache_bytes: 1_048_576,
            audio_profile: super::super::mashin::models::AudioProfileId::Flac,
            volume: 30,
        }, connection).unwrap());
        runtime.block_on(session.output.execute(super::super::mashin::protocol::ControlCommand::SetMedia {
            command_id: 1,
            current: Some(super::super::mashin::models::test_stream()),
            prepared: None,
            media_revision: 1,
            initial_output_state: OutputState::Paused,
        })).unwrap();
        let before = session.output.snapshot();
        let mut backend = LocalPlayback::new();
        backend.configure(session.clone());
        let request = crate::playback::start::PlaybackStartRequest::begin(
            backend.start_presentation(), crate::playback::start::StartKind::Resume,
            backend.start_observation(),
        );
        backend.transport_play().unwrap();
        assert_eq!(backend.player_state().playback_state, PlayState::Buffering);
        assert_eq!(backend.presentation_snapshot().0.player_state.playback_state, PlayState::Buffering);
        assert_eq!(session.output.snapshot(), before);
        for command in backend.take_pending_commands() {
            runtime.block_on(command).unwrap();
        }
        request.complete(true);
        assert_eq!(backend.player_state().playback_state, PlayState::Buffering);
        backend.deactivate();
        assert!(!request.is_pending());
    }

    #[tokio::test]
    async fn offline_next_projects_and_publishes_b_without_a_server_queue_update() {
        use super::super::mashin::audio_adapter::MashinAudioAdapter;
        use super::super::mashin::models::{AudioProfileId, test_stream};
        use super::super::mashin::protocol::ControlCommand;

        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let mut current = test_stream();
        current.playback.queue_item_id = QueueItemId("a".into());
        let mut prepared = test_stream();
        prepared.playback.queue_id = current.playback.queue_id.clone();
        prepared.playback.queue_item_id = QueueItemId("b".into());
        prepared.duration_millis = Some(240_000);
        output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: Some(current.clone()),
                prepared: Some(prepared.clone()),
                media_revision: 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .unwrap();
        let queue = Queue {
            queue_id: current.playback.queue_id.clone(),
            items: vec![
                queue_item(&current, "a"),
                queue_item(&current, "c"),
                queue_item(&current, "b"),
            ],
            current_index: Some(0),
            ..Queue::default()
        };
        let mut connection = ReceiverSnapshot {
            control_connection: ConnectionState::Disconnected,
            audio_connection: ConnectionState::Disconnected,
            player_id: Some(current.playback.player_id.0.clone()),
            audio_profile: AudioProfileId::Flac,
        };
        let mut playback = LocalPlayback::new();
        let initial =
            project_media_session_state(queue.clone(), &output.snapshot(), &connection, None);
        playback.presentation_events(initial, Some(current.stream_id));
        output
            .pipeline()
            .execute(AudioCommand::ActivatePrepared {
                current_stream_id: current.stream_id,
                stream_id: prepared.stream_id,
                media_revision: 1,
            })
            .await
            .unwrap();
        let snapshot = output.snapshot();
        let promoted = project_media_session_state(
            queue.clone(),
            &snapshot,
            &connection,
            playback.last_queue.as_ref(),
        );
        assert_eq!(promoted.queue.current_index, Some(2));
        assert_eq!(
            promoted.player_state.current_index,
            promoted.queue.current_index
        );
        assert_eq!(
            promoted.player_state.current_queue_item_id,
            Some(prepared.playback.queue_item_id.clone())
        );
        assert_eq!(promoted.player_state.position_millis, Some(0));
        assert_eq!(promoted.player_state.duration_millis, Some(240_000));
        assert_eq!(
            promoted.player_state.connection_state,
            PlaybackConnectionState::Reconnecting
        );
        let events = playback.presentation_events(promoted.clone(), Some(prepared.stream_id));
        assert!(
            matches!(&events[0], MediaSessionEvent::QueueChanged { queue } if queue.current_index == Some(2))
        );
        assert!(
            matches!(&events[1], MediaSessionEvent::PlayerStateChanged { player_state } if player_state.current_index == Some(2))
        );
        assert!(
            playback
                .presentation_events(promoted.clone(), Some(prepared.stream_id))
                .is_empty()
        );
        connection.control_connection = ConnectionState::Connected;
        connection.audio_connection = ConnectionState::Connected;
        let reconnected =
            project_media_session_state(queue, &snapshot, &connection, Some(&promoted.queue));
        assert_eq!(reconnected.queue.current_index, Some(2));
        assert_eq!(
            reconnected.player_state.current_queue_item_id,
            promoted.player_state.current_queue_item_id
        );
        assert!(playback.take_pending_commands().is_empty());

        let mut natural = snapshot;
        natural.resume.activation = None;
        natural.resume.position_frame = u64::from(prepared.output_format.sample_rate_hz) * 12;
        let natural =
            project_media_session_state(reconnected.queue.clone(), &natural, &connection, None);
        assert_eq!(natural.queue.current_index, Some(2));
        assert_eq!(natural.player_state.position_millis, Some(12_000));
        assert_eq!(
            natural.player_state.current_queue_item_id,
            Some(prepared.playback.queue_item_id)
        );
    }

    #[test]
    fn retained_metadata_and_repeat_keep_the_audible_queue_cursor() {
        let mut stream = super::super::mashin::models::test_stream();
        stream.playback.queue_item_id = QueueItemId("b".into());
        let previous = Queue {
            queue_id: stream.playback.queue_id.clone(),
            items: vec![queue_item(&stream, "a"), queue_item(&stream, "b")],
            current_index: Some(0),
            repeat_mode: RepeatMode::One,
            ..Queue::default()
        };
        let mut stale = previous.clone();
        stale.items.pop();
        stale.shuffle_enabled = true;
        let projected = project_queue(stale, Some(&stream), Some(&previous));
        assert_eq!(projected.current_index, Some(1));
        assert_eq!(projected.items[1].name, "b");
        assert!(projected.shuffle_enabled);
        assert_eq!(projected.repeat_mode, RepeatMode::One);
        let mut playback = LocalPlayback::new();
        let state = MediaSessionState {
            queue: projected,
            player_state: PlayerState {
                current_index: Some(1),
                current_queue_item_id: Some(stream.playback.queue_item_id.clone()),
                ..PlayerState::default()
            },
        };
        playback.presentation_events(state.clone(), Some(stream.stream_id));
        let repeated = playback.presentation_events(state, Some(Uuid::new_v4()));
        assert_eq!(repeated.len(), 1);
        assert!(
            matches!(&repeated[0], MediaSessionEvent::QueueChanged { queue } if queue.current_index == Some(1))
        );
        let mut unrelated = previous.clone();
        unrelated.queue_id.0 = "other-queue".into();
        unrelated.items.clear();
        assert_eq!(
            project_queue(unrelated, Some(&stream), Some(&previous)).current_index,
            None
        );
    }

    #[test]
    fn unconfigured_engine_has_no_audio_and_cannot_start_local_planning() {
        let mut engine = LocalPlayback::new();
        assert_eq!(engine.player_state().active_mode, PlaybackMode::Local);
        assert!(engine.transport_play().is_err());
        assert!(engine.play_queue_index(0).is_err());
        assert!(
            engine
                .queue_play_media(Vec::new(), QueueOption::Play, None, false)
                .is_err()
        );
        engine.shutdown();
    }

    fn queue_item(
        stream: &StreamDescriptor,
        identifier: &str,
    ) -> crate::music_assistant::models::queue::QueueItem {
        crate::music_assistant::models::queue::QueueItem {
            queue_id: stream.playback.queue_id.clone(),
            queue_item_id: QueueItemId(identifier.into()),
            name: identifier.into(),
            duration: Some(300),
            sort_index: 0,
            streamdetails: None,
            media_item: None,
            image: None,
            index: 0,
            available: true,
        }
    }
}
