//! Mashin presents MA's queue but projects position and output controls from its DAC.
//! Deactivating this presentation never drops the session-owned audio receiver.

use crate::audio::models::AudioCommand;
use crate::core::event_channel::publish_background_event;
use crate::core::models::MediaSessionEvent;
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::models::identifiers::{PlayerId, QueueItemId};
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{PlayState, QueueOption, RepeatMode};
use crate::music_assistant::playback_target::MusicAssistantPlaybackTarget;
use crate::music_assistant::playback_target::MusicAssistantPlaybackTargetConfiguration;
use crate::music_assistant::queue_projection::QueueSnapshotRequest;
use crate::playback::backend::{PlaybackBackend, PlaybackCommand};
use crate::playback::local::mashin::models::OutputState;
use crate::playback::local::mashin::receiver::ConnectionState;
use crate::playback::local::session::{LocalSession, LocalSessionConfiguration};
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{PlaybackConnectionState, PlayerState, Queue};
use std::sync::Arc;

pub(crate) struct LocalPlayback {
    session: Option<Arc<LocalSession>>,
    target: MusicAssistantPlaybackTarget,
    projected_player_id: Option<String>,
    pending_commands: Vec<PlaybackCommand>,
    last_queue: Option<Queue>,
    last_queue_revision: Option<(QueueSnapshotRequest, bool)>,
    last_player: Option<PlayerState>,
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
            last_queue_revision: None,
            last_player: None,
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
    // Configuration and presentation lifecycle.
    fn activate(&mut self) -> Result<(), String> {
        self.synchronize_projection()
    }
    fn deactivate(&mut self) {
        self.target.deactivate();
        self.projected_player_id = None;
        self.last_queue = None;
        self.last_queue_revision = None;
        self.last_player = None;
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
        self.target
            .queue_play_media(media, option, start_item, shuffle)
    }
    fn start_radio(&mut self, media: Vec<MediaItem>) -> Result<(), String> {
        self.target.start_radio(media)
    }
    fn insert_queue_items(&mut self, index: usize, media: Vec<MediaItem>) -> Result<(), String> {
        self.target.insert_queue_items(index, media)
    }
    fn append_queue_items(&mut self, media: Vec<MediaItem>) -> Result<(), String> {
        self.target.append_queue_items(media)
    }
    fn move_queue_items(&mut self, ids: Vec<QueueItemId>, target: usize) -> Result<(), String> {
        self.target.move_queue_items(ids, target)
    }
    fn delete_queue_items(&mut self, ids: Vec<QueueItemId>) -> Result<(), String> {
        self.target.delete_queue_items(ids)
    }
    fn select_queue_item(&mut self, id: QueueItemId) -> Result<(), String> {
        self.target.select_queue_item(id)
    }
    fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.target.play_queue_index(index)
    }

    // Manual next consumes only MA's already registered successor grant, even offline.
    // Local transport and shared queue preferences.
    fn transport_play(&mut self) -> Result<(), String> {
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
        if let Some(session) = self.session.as_ref() {
            let snapshot = session.output.snapshot();
            if let (Some(current), Some(prepared)) = (snapshot.current, snapshot.prepared) {
                return self.local_command(AudioCommand::ActivatePrepared {
                    current_stream_id: current.stream_id,
                    stream_id: prepared.stream_id,
                    next_revision: snapshot.resume.next_revision,
                });
            }
            if snapshot.resume.activation.is_some() {
                return Err("Awaiting a new MA-authorized successor".into());
            }
        }
        self.target.transport_next()
    }
    fn transport_previous(&mut self) -> Result<(), String> {
        self.target.transport_previous()
    }
    fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        self.target.transport_seek(seconds)
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
        self.target.set_shuffle(enabled);
    }
    fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.target.set_repeat_mode(mode);
    }
    fn set_autoplay(&mut self, enabled: bool) {
        self.target.set_autoplay(enabled);
    }
    fn take_pending_commands(&mut self) -> Vec<PlaybackCommand> {
        let mut commands = std::mem::take(&mut self.pending_commands);
        commands.extend(self.target.take_pending_commands());
        commands
    }

    // Only queue metadata comes from MA; audible position never uses server elapsed time.
    // Queue and audible player projections.
    fn queue(&self) -> Queue {
        self.target.queue()
    }
    fn player_state(&self) -> PlayerState {
        let Some(session) = &self.session else {
            return PlayerState {
                active_mode: PlaybackMode::Local,
                ..PlayerState::default()
            };
        };
        let output = session.output.snapshot();
        let connection = session.receiver.snapshot();
        let item_id = output
            .current
            .as_ref()
            .map(|stream| stream.playback.queue_item_id.clone());
        let position_millis = output.current.as_ref().map(|stream| {
            stream.media_start_position_millis.saturating_add(
                output.resume.position_frame.saturating_mul(1000)
                    / u64::from(stream.output_format.sample_rate_hz),
            ) as i64
        });
        PlayerState {
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
            current_index: item_id
                .as_ref()
                .and_then(|id| self.target.queue_item_index(id)),
            current_queue_item_id: item_id,
            position_millis,
            duration_millis: output
                .current
                .as_ref()
                .and_then(|stream| stream.duration_millis.map(|duration| duration as i64)),
            buffered_position_millis: position_millis
                .map(|position| position.saturating_add(output.buffered_duration_millis as i64)),
            volume: output.resume.volume_percent,
            muted: output.resume.is_muted,
        }
    }

    // Presentation polling does not own the session's native audio worker.
    // Presentation event publication.
    fn process_playback_cycle(&mut self) {
        if self.synchronize_projection().is_err() {
            return;
        }
        let revision = self.target.queue_revision();
        if self.last_queue_revision != revision {
            self.last_queue_revision = revision;
            let queue = self.queue();
            if self.last_queue.as_ref() != Some(&queue) {
                self.last_queue = Some(queue.clone());
                publish_background_event(MediaSessionEvent::QueueChanged { queue });
            }
        }
        let player = self.player_state();
        if self.last_player.as_ref() != Some(&player) {
            self.last_player = Some(player.clone());
            publish_background_event(MediaSessionEvent::PlayerStateChanged {
                player_state: player,
            });
        }
    }
}

impl Drop for LocalPlayback {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl LocalPlayback {
    // Native command adaptation and MA projection synchronization.
    fn local_command(&mut self, command: AudioCommand) -> Result<(), String> {
        let output = self
            .session
            .as_ref()
            .ok_or("Mashin session is not configured")?
            .output
            .pipeline();
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
        self.projected_player_id = Some(player_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_engine_has_no_audio_and_cannot_start_local_planning() {
        let mut engine = LocalPlayback::new();
        assert_eq!(engine.player_state().active_mode, PlaybackMode::Local);
        assert!(engine.transport_play().is_err());
        assert!(engine.play_queue_index(0).is_err());
        assert!(engine.start_radio(Vec::new()).is_err());
        engine.shutdown();
    }
}
