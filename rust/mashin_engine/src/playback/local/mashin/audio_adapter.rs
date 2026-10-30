//! Mashin commands and metadata are translated here, outside the native audio owner.

use super::models::{
    OutputState, StreamCursor, StreamDescriptor, HEADER_BYTES, MAX_INTEGER, MAX_PAYLOAD_BYTES,
};
use super::protocol::{ControlCommand, ProtocolError, ResumeState, Welcome};
use super::recovery::{RecoveryState, RecoveryTarget};
use crate::audio::buffer::EncodedChunk;
use crate::audio::models::{AudioCommand, AudioState, AudioStream};
use crate::audio::models::{AudioEvent, AudioSnapshot};
use crate::audio::pipeline::AudioPipeline;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, watch, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputSnapshot {
    pub(crate) resume: ResumeState,
    pub(crate) current: Option<StreamDescriptor>,
    pub(crate) prepared: Option<StreamDescriptor>,
    pub(crate) buffer_capacity: usize,
    pub(crate) buffered_duration_millis: u64,
    pub(crate) error: Option<ProtocolError>,
    pub(crate) audible_events: Vec<(u64, AudioEvent)>,
}

#[derive(Clone)]
pub(crate) struct MashinAudioAdapter {
    pipeline: AudioPipeline,
    streams: Arc<std::sync::Mutex<HashMap<Uuid, StreamDescriptor>>>,
    commands: Arc<Mutex<()>>,
    last_media: Arc<std::sync::Mutex<Option<ControlCommand>>>,
    recovery: watch::Sender<RecoveryState>,
}

impl MashinAudioAdapter {
    // Only the pipeline owns CPAL, decoder state and encoded allocations.
    pub(crate) fn new(
        cache_bytes: usize,
        volume_percent: u8,
        is_muted: bool,
    ) -> Result<Self, String> {
        if cache_bytes < HEADER_BYTES + MAX_PAYLOAD_BYTES {
            return Err("invalid Mashin buffer budget".into());
        }
        Ok(Self {
            pipeline: AudioPipeline::new(cache_bytes, volume_percent, is_muted)?,
            streams: Arc::new(std::sync::Mutex::new(HashMap::new())),
            commands: Arc::new(Mutex::new(())),
            last_media: Arc::new(std::sync::Mutex::new(None)),
            recovery: watch::channel(RecoveryState::default()).0,
        })
    }

    // Metadata must already exist when the worker publishes a newly accepted stream.
    pub(crate) async fn execute(&self, command: ControlCommand) -> Result<OutputSnapshot, String> {
        let _command = self.commands.lock().await;
        let target = self.recovery.borrow().target.clone();
        let replacement = match &command {
            ControlCommand::SetMedia { current: Some(stream), .. }
                if target.as_ref().is_some_and(|target| stream.playback.queue_id == target.playback.queue_id && stream.playback.queue_item_id == target.playback.queue_item_id) => Some(stream.stream_id),
            _ => None,
        };
        let replaces_playback = matches!(&command, ControlCommand::SetMedia { .. } | ControlCommand::Stop { .. });
        let announcement = if let ControlCommand::SetMedia { media_revision, .. } = &command {
            let mut normalized = command.clone();
            if let ControlCommand::SetMedia { command_id, .. } = &mut normalized {
                *command_id = 0;
            }
            let previous = self
                .last_media
                .lock()
                .map_err(|_| "Mashin media lock poisoned")?;
            if previous.as_ref().is_some_and(|previous| matches!(previous, ControlCommand::SetMedia { media_revision: revision, .. } if revision == media_revision) && previous != &normalized) {
                return Err("Mashin media revision changed payload".into());
            }
            Some(normalized)
        } else {
            None
        };
        let mut descriptors = Vec::new();
        let audio = match command {
            ControlCommand::SetMedia {
                current,
                prepared,
                media_revision,
                initial_output_state,
                ..
            } => {
                if media_revision == 0 || media_revision > MAX_INTEGER {
                    return Err("invalid Mashin media revision".into());
                }
                for stream in current.iter().chain(prepared.iter()) {
                    stream.validate()?;
                    descriptors.push(stream.clone());
                }
                AudioCommand::SetMedia {
                    current: current.as_ref().map(audio_stream),
                    prepared: prepared.as_ref().map(audio_stream),
                    media_revision,
                    initial_output_state: match if target.as_ref().is_some_and(|target| target.is_paused && current.as_ref().is_some_and(|stream| stream.playback.queue_id == target.playback.queue_id && stream.playback.queue_item_id == target.playback.queue_item_id)) {
                        OutputState::Paused
                    } else { initial_output_state } {
                        OutputState::Playing => AudioState::Playing,
                        OutputState::Paused => AudioState::Paused,
                        _ => return Err("invalid Mashin initial output state".into()),
                    },
                }
            }
            ControlCommand::Play { stream_id, .. } => AudioCommand::Play { stream_id },
            ControlCommand::Pause { stream_id, .. } => AudioCommand::Pause { stream_id },
            ControlCommand::Stop { stream_id, .. } => AudioCommand::Stop { stream_id },
            ControlCommand::SetVolume { volume_percent, .. } => {
                AudioCommand::SetVolume { volume_percent }
            }
            ControlCommand::SetMute { is_muted, .. } => AudioCommand::SetMute { is_muted },
            ControlCommand::StateRequest { .. } => return Ok(self.snapshot()),
        };
        let previous = {
            let mut streams = self
                .streams
                .lock()
                .map_err(|_| "Mashin metadata lock poisoned")?;
            if descriptors.iter().any(|stream| {
                streams
                    .get(&stream.stream_id)
                    .is_some_and(|previous| previous != stream)
            }) {
                return Err("Mashin stream metadata is immutable".into());
            }
            let previous = streams.clone();
            for stream in descriptors {
                streams.insert(stream.stream_id, stream);
            }
            previous
        };
        let changes_pause = matches!(audio, AudioCommand::Play { .. } | AudioCommand::Pause { .. });
        if let Err(error) = self.pipeline.execute(audio).await {
            *self
                .streams
                .lock()
                .map_err(|_| "Mashin metadata lock poisoned")? = previous;
            return Err(error);
        }
        if let Some(announcement) = announcement {
            *self
                .last_media
                .lock()
                .map_err(|_| "Mashin media lock poisoned")? = Some(announcement);
        }
        if replaces_playback {
            self.recovery.send_modify(|state| {
                if let Some(stream_id) = replacement {
                    state.replacement_stream_id = Some(stream_id);
                    state.revision = state.revision.wrapping_add(1);
                } else {
                    state.cancel();
                    state.is_stopped = self.pipeline.snapshot().current.is_none();
                    state.is_paused = None;
                }
            });
        } else if changes_pause {
            self.recovery.send_modify(|state| state.is_paused = Some(self.pipeline.snapshot().status.output_state == AudioState::Paused));
        }
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| "Mashin metadata lock poisoned")?;
        let snapshot = self.pipeline.snapshot();
        streams.retain(|id, _| {
            snapshot
                .current
                .as_ref()
                .is_some_and(|stream| stream.stream_id == *id)
                || snapshot
                    .prepared
                    .as_ref()
                    .is_some_and(|stream| stream.stream_id == *id)
        });
        Ok(project(snapshot, &streams))
    }

    // Rejection frees unusable audio without turning the user's intent into a local stop.
    pub(crate) async fn reject_retained_playback(&self) -> Result<(), String> {
        let _command = self.commands.lock().await;
        let mut target = RecoveryTarget::from_snapshot(&self.snapshot());
        self.recovery.send_modify(|state| {
            if !state.is_stopped {
            if let (Some(target), Some(is_paused)) = (&mut target, state.is_paused) { target.is_paused = is_paused; }
                if state.target.is_none() {
                    state.retry_revision = state.retry_revision.wrapping_add(1);
                }
                if target.is_some() { state.target = target; }
                state.replacement_stream_id = None;
                state.revision = state.revision.wrapping_add(1);
            }
        });
        self.pipeline.execute(AudioCommand::Stop { stream_id: None }).await?;
        self.streams.lock().map_err(|_| "Mashin metadata lock poisoned")?.clear();
        Ok(())
    }

    // A replacement is recovered only when it has usable audio, not merely a media ACK.
    pub(crate) fn confirm_recovery(&self) {
        let snapshot = self.snapshot();
        self.recovery.send_if_modified(|state| {
            if state.replacement_stream_id.is_some()
                && state.replacement_stream_id == snapshot.resume.current_stream_id
                && (snapshot.resume.output_state == OutputState::Playing
                    || (snapshot.resume.output_state == OutputState::Paused && snapshot.buffered_duration_millis > 0))
            {
                state.cancel();
                return true;
            }
            false
        });
    }

    // Invalidate queued requests synchronously, before a UI command's asynchronous execution.
    pub(crate) fn cancel_recovery(&self) {
        self.recovery.send_modify(RecoveryState::cancel);
    }

    pub(crate) fn recovery(&self) -> watch::Receiver<RecoveryState> {
        self.recovery.subscribe()
    }

    pub(crate) fn authorize_local_command(&self, command: &AudioCommand) -> bool {
        let has_recovery = self.recovery.borrow().target.is_some();
        match command {
            AudioCommand::Play { .. } => self.recovery.send_modify(|state| state.set_paused(false)),
            AudioCommand::Pause { .. } => self.recovery.send_modify(|state| state.set_paused(true)),
            AudioCommand::Stop { .. } => self.recovery.send_modify(|state| {
                state.cancel();
                state.is_stopped = true;
            }),
            AudioCommand::SetVolume { .. } | AudioCommand::SetMute { .. } => {}
            _ => self.cancel_recovery(),
        }
        has_recovery
            && self.pipeline.snapshot().current.is_none()
            && matches!(command, AudioCommand::Play { .. } | AudioCommand::Pause { .. })
    }

    // Serialize local seeks against provider commands, retaining immutable descriptor provenance.
    pub(crate) async fn seek_cached(
        &self,
        stream_id: Uuid,
        position_millis: u64,
    ) -> Result<bool, String> {
        let _command = self.commands.lock().await;
        let snapshot = self.snapshot();
        let stream = snapshot
            .current
            .as_ref()
            .filter(|stream| stream.stream_id == stream_id)
            .ok_or("Mashin playback changed before local seek")?;
        if snapshot.resume.output_revision >= MAX_INTEGER {
            return Err("Mashin output revision exhausted".into());
        }
        let Some(position_frame) = seek_position_frame(stream, position_millis)? else {
            return Ok(false);
        };
        self.pipeline.seek_cached(stream_id, position_frame).await
    }

    pub(crate) async fn retain(&self, chunk: EncodedChunk) -> Result<OutputSnapshot, String> {
        self.pipeline.retain(chunk).await?;
        Ok(self.snapshot())
    }

    pub(crate) async fn reconcile(
        &self,
        welcome: Welcome,
        offered: &OutputSnapshot,
    ) -> Result<OutputSnapshot, String> {
        let _command = self.commands.lock().await;
        let mut snapshot = offered.clone();
        welcome.limits.validate()?;
        let mut profiles = welcome.audio_profiles.clone();
        profiles.sort_by_key(|profile| format!("{profile:?}"));
        profiles.dedup();
        if profiles.len() != 5
            || welcome.player_id.is_empty()
            || welcome.audio_binding_id.is_empty()
            || !matches!(
                welcome.desired_output_state,
                OutputState::Idle | OutputState::Playing | OutputState::Paused
            )
            || welcome.media_revision > MAX_INTEGER
        {
            return Err("invalid Mashin welcome".into());
        }
        if welcome.current_stream.as_ref().map(|state| &state.stream) != snapshot.current.as_ref() {
            return Err(format!(
                "Mashin current stream descriptor changed during handshake: client={:?}, server={:?}",
                snapshot.current.as_ref().map(|stream| stream.stream_id),
                welcome
                    .current_stream
                    .as_ref()
                    .map(|state| state.stream.stream_id),
            ));
        }
        if welcome.prepared_next.as_ref().map(|state| &state.stream) != snapshot.prepared.as_ref() {
            return Err(format!(
                "Mashin prepared stream descriptor changed during handshake: client={:?}, server={:?}",
                snapshot.prepared.as_ref().map(|stream| stream.stream_id),
                welcome
                    .prepared_next
                    .as_ref()
                    .map(|state| state.stream.stream_id),
            ));
        }
        if welcome.media_revision != snapshot.resume.media_revision {
            if snapshot.current.is_none()
                && snapshot.prepared.is_none()
                && snapshot.resume.output_state == OutputState::Idle
                && welcome.desired_output_state == OutputState::Idle
            {
                let reconciled = self
                    .pipeline
                    .reconcile_idle_revision(welcome.media_revision)
                    .await?;
                *self
                    .last_media
                    .lock()
                    .map_err(|_| "Mashin media lock poisoned")? = None;
                let streams = self
                    .streams
                    .lock()
                    .map_err(|_| "Mashin metadata lock poisoned")?;
                snapshot = project(reconciled, &streams);
            } else {
                return Err(format!(
                    "Mashin media revision changed during handshake: client={}, server={}",
                    snapshot.resume.media_revision, welcome.media_revision,
                ));
            }
        }
        for state in welcome
            .current_stream
            .iter()
            .chain(welcome.prepared_next.iter())
        {
            state.stream.validate()?;
            let cursor = snapshot
                .resume
                .received_streams
                .iter()
                .find(|cursor| cursor.stream_id == state.stream.stream_id)
                .ok_or("Mashin stream cursor missing")?;
            if state.next_received_chunk_index != cursor.next_chunk_index
                || state.next_produced_chunk_index < cursor.next_chunk_index
                || state.next_produced_chunk_index > MAX_INTEGER
                || state
                    .final_frame_count
                    .is_some_and(|frames| frames > MAX_INTEGER)
            {
                return Err("Mashin server did not reconcile retained chunks".into());
            }
        }
        Ok(self.snapshot())
    }

    // App-local controls use the audio contract without fabricated protocol IDs.
    pub(crate) fn pipeline(&self) -> AudioPipeline {
        self.pipeline.clone()
    }
    pub(crate) fn snapshot(&self) -> OutputSnapshot {
        let streams = self.streams.lock().expect("Mashin metadata lock poisoned");
        project(self.pipeline.snapshot(), &streams)
    }
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<AudioEvent> {
        self.pipeline.subscribe()
    }
}

// Protocol-specific metadata and wire representation stay at this boundary.
fn seek_position_frame(
    stream: &StreamDescriptor,
    position_millis: u64,
) -> Result<Option<u64>, String> {
    if position_millis % 1000 != 0 {
        return Err("Mashin cached seek must start at a whole second".into());
    }
    if stream
        .duration_millis
        .is_some_and(|duration| position_millis >= duration)
    {
        return Ok(None);
    }
    let Some(relative_millis) = position_millis.checked_sub(stream.media_start_position_millis)
    else {
        return Ok(None);
    };
    relative_millis
        .checked_mul(u64::from(stream.output_format.sample_rate_hz))
        .map(|frames| frames / 1000)
        .filter(|frames| *frames <= MAX_INTEGER)
        .map(Some)
        .ok_or_else(|| "Mashin seek position exhausted".into())
}

fn audio_stream(stream: &StreamDescriptor) -> AudioStream {
    AudioStream {
        stream_id: stream.stream_id,
        output_format: stream.output_format.audio_format(),
    }
}

// Device mutations retain internal revisions, but the wire revision belongs to a stream.
fn project(snapshot: AudioSnapshot, streams: &HashMap<Uuid, StreamDescriptor>) -> OutputSnapshot {
    let state = snapshot.status;
    OutputSnapshot {
        resume: ResumeState {
            activation: snapshot
                .activation
                .map(|activation| super::protocol::PreparedActivation {
                    previous_stream_id: activation.previous_stream_id,
                    stream_id: activation.stream_id,
                    media_revision: activation.media_revision,
                    previous_position_frame: activation.previous_position_frame,
                }),
            current_stream_id: state.current_stream_id,
            output_revision: if state.current_stream_id.is_some() {
                state.output_revision
            } else {
                0
            },
            output_state: match state.output_state {
                AudioState::Idle => OutputState::Idle,
                AudioState::Buffering => OutputState::Buffering,
                AudioState::Playing => OutputState::Playing,
                AudioState::Paused => OutputState::Paused,
                AudioState::Ended => OutputState::Ended,
                AudioState::Error => OutputState::Error,
            },
            position_frame: state.position_frame,
            volume_percent: state.volume_percent,
            is_muted: state.is_muted,
            media_revision: state.media_revision,
            received_streams: state
                .received_streams
                .into_iter()
                .map(|cursor| StreamCursor {
                    stream_id: cursor.stream_id,
                    next_chunk_index: cursor.next_chunk_index,
                })
                .collect(),
        },
        current: snapshot
            .current
            .and_then(|stream| streams.get(&stream.stream_id).cloned()),
        prepared: snapshot
            .prepared
            .and_then(|stream| streams.get(&stream.stream_id).cloned()),
        buffer_capacity: snapshot
            .buffer_capacity
            .saturating_sub(HEADER_BYTES)
            .min(MAX_PAYLOAD_BYTES),
        buffered_duration_millis: snapshot.buffered_duration_millis,
        error: snapshot.error.map(|message| ProtocolError {
            code: "media_unsupported".into(),
            message,
            is_retryable: false,
            retry_after_millis: None,
        }),
        audible_events: snapshot.audible_events,
    }
}

#[cfg(test)]
mod tests {
    use super::super::models::{test_stream, AudioProfileId};
    use super::super::protocol::ProtocolLimits;
    use super::*;

    #[tokio::test]
    async fn idle_wire_snapshot_after_stop_clears_stream_revision_and_receipts() {
        for is_player_wide in [false, true] {
            let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
            let current = test_stream();
            output
                .execute(ControlCommand::SetMedia {
                    command_id: 1,
                    current: Some(current.clone()),
                    prepared: Some(test_stream()),
                    media_revision: 1,
                    initial_output_state: OutputState::Paused,
                })
                .await
                .unwrap();
            let snapshot = output
                .execute(ControlCommand::Stop {
                    command_id: 2,
                    stream_id: (!is_player_wide).then_some(current.stream_id),
                })
                .await
                .unwrap();
            assert_eq!(snapshot.resume.current_stream_id, None);
            assert_eq!(snapshot.resume.output_state, OutputState::Idle);
            assert_eq!(snapshot.resume.output_revision, 0);
            assert_eq!(snapshot.resume.position_frame, 0);
            assert!(snapshot.resume.received_streams.is_empty());
            assert!(snapshot.resume.activation.is_none());
            assert!(snapshot.current.is_none());
            assert!(snapshot.prepared.is_none());
            assert_eq!(output.snapshot(), snapshot);
            assert!(output.pipeline.snapshot().status.output_revision > 0);
            assert_eq!(
                output
                    .reconcile(idle_welcome(2), &snapshot)
                    .await
                    .unwrap()
                    .resume
                    .output_revision,
                0
            );
        }
    }

    #[tokio::test]
    async fn idle_wire_snapshot_after_volume_and_mute_keeps_zero_revision() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        for command in [
            ControlCommand::SetVolume {
                command_id: 1,
                volume_percent: 42,
            },
            ControlCommand::SetMute {
                command_id: 2,
                is_muted: true,
            },
        ] {
            let snapshot = output.execute(command).await.unwrap();
            assert_eq!(snapshot.resume.current_stream_id, None);
            assert_eq!(snapshot.resume.output_state, OutputState::Idle);
            assert_eq!(snapshot.resume.output_revision, 0);
            assert_eq!(snapshot.resume.position_frame, 0);
            assert!(snapshot.resume.received_streams.is_empty());
            assert_eq!(snapshot.resume.volume_percent, 42);
            assert_eq!(output.snapshot(), snapshot);
        }
        assert!(output.snapshot().resume.is_muted);
    }

    #[tokio::test]
    async fn idle_handshake_adopts_revision_without_changing_output() {
        let output = MashinAudioAdapter::new(1_048_576, 30, true).unwrap();
        output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: None,
                prepared: None,
                media_revision: 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .unwrap();
        let mut expected = output.snapshot();
        for revision in [7, 7, 0, 3] {
            expected.resume.media_revision = revision;
            assert_eq!(
                output.reconcile(idle_welcome(revision), &output.snapshot()).await.unwrap(),
                expected
            );
            assert_eq!(output.snapshot(), expected);
            assert!(output.last_media.lock().unwrap().is_none());
        }
        let command = ControlCommand::SetMedia {
            command_id: 2,
            current: Some(test_stream()),
            prepared: None,
            media_revision: 4,
            initial_output_state: OutputState::Paused,
        };
        let mut stale = command.clone();
        if let ControlCommand::SetMedia { media_revision, .. } = &mut stale {
            *media_revision = 3;
        }
        assert!(output.execute(stale).await.is_err());
        assert_eq!(output.snapshot(), expected);
        let accepted = output.execute(command.clone()).await.unwrap();
        assert_eq!(output.execute(command.clone()).await.unwrap(), accepted);
        let mut changed = command;
        if let ControlCommand::SetMedia { prepared, .. } = &mut changed {
            *prepared = Some(test_stream());
        }
        assert!(output.execute(changed).await.is_err());
        assert_eq!(output.snapshot(), accepted);
    }

    #[tokio::test]
    async fn handshake_revision_reconciliation_requires_both_sides_idle() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let before = output.snapshot();
        for state in [OutputState::Playing, OutputState::Paused] {
            let mut welcome = idle_welcome(7);
            welcome.desired_output_state = state;
            assert!(output.reconcile(welcome, &before).await.is_err());
            assert_eq!(output.snapshot(), before);
        }
        assert!(output
            .reconcile(idle_welcome(MAX_INTEGER + 1), &before)
            .await
            .is_err());
        assert_eq!(output.snapshot(), before);
    }

    #[tokio::test]
    async fn handshake_preserves_registered_stream_and_revision_checks() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let current = test_stream();
        let prepared = test_stream();
        let before = output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: Some(current.clone()),
                prepared: Some(prepared.clone()),
                media_revision: 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .unwrap();
        let stream_snapshot = |stream| super::super::protocol::StreamSnapshot {
            stream,
            next_received_chunk_index: 1,
            next_produced_chunk_index: 1,
            is_complete: false,
            final_frame_count: None,
            error: None,
        };
        let mut welcome = idle_welcome(1);
        welcome.current_stream = Some(stream_snapshot(current));
        welcome.prepared_next = Some(stream_snapshot(prepared));
        welcome.desired_output_state = OutputState::Paused;
        assert_eq!(output.reconcile(welcome.clone(), &before).await.unwrap(), before);
        for change in ["current", "prepared", "revision", "descriptor"] {
            let mut changed = welcome.clone();
            let expected_reason = match change {
                "current" => {
                    changed.current_stream = None;
                    "current stream descriptor"
                }
                "prepared" => {
                    changed.prepared_next = None;
                    "prepared stream descriptor"
                }
                "revision" => {
                    changed.media_revision = 2;
                    "media revision"
                }
                _ => {
                    changed
                        .current_stream
                        .as_mut()
                        .unwrap()
                        .stream
                        .playback
                        .queue_item_id
                        .0 = "changed".into();
                    "current stream descriptor"
                }
            };
            assert!(output
                .reconcile(changed, &before)
                .await
                .unwrap_err()
                .contains(expected_reason));
            assert_eq!(output.snapshot(), before);
        }
        assert!(output.pipeline.reconcile_idle_revision(2).await.is_err());
        assert_eq!(output.snapshot(), before);
    }

    #[tokio::test]
    async fn welcome_validates_the_offered_snapshot_without_reverting_local_playback() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let current = test_stream();
        let prepared = test_stream();
        let offered = output.execute(ControlCommand::SetMedia {
            command_id: 1,
            current: Some(current.clone()),
            prepared: Some(prepared.clone()),
            media_revision: 1,
            initial_output_state: OutputState::Paused,
        }).await.unwrap();
        let stream_snapshot = |stream| super::super::protocol::StreamSnapshot {
            stream, next_received_chunk_index: 1, next_produced_chunk_index: 1,
            is_complete: false, final_frame_count: None, error: None,
        };
        let mut welcome = idle_welcome(1);
        welcome.current_stream = Some(stream_snapshot(current));
        welcome.prepared_next = Some(stream_snapshot(prepared.clone()));
        welcome.desired_output_state = OutputState::Paused;
        let activated = output.execute(ControlCommand::SetMedia {
            command_id: 2,
            current: Some(prepared),
            prepared: None,
            media_revision: 2,
            initial_output_state: OutputState::Paused,
        }).await.unwrap();
        assert_eq!(output.reconcile(welcome.clone(), &offered).await.unwrap(), activated);
        assert_eq!(output.snapshot(), activated);
        welcome.current_stream = None;
        assert!(output.reconcile(welcome, &offered).await.is_err());
        assert_eq!(output.snapshot(), activated);
    }

    #[test]
    fn cached_seek_uses_descriptor_origin_and_bounds() {
        let mut stream = test_stream();
        stream.media_start_position_millis = 150_000;
        assert_eq!(seek_position_frame(&stream, 149_000).unwrap(), None);
        assert_eq!(seek_position_frame(&stream, 150_000).unwrap(), Some(0));
        assert_eq!(
            seek_position_frame(&stream, 160_000).unwrap(),
            Some(480_000)
        );
        assert_eq!(seek_position_frame(&stream, 180_000).unwrap(), None);
        assert!(seek_position_frame(&stream, 160_001).is_err());
        stream.output_format.sample_rate_hz = 44_100;
        assert_eq!(
            seek_position_frame(&stream, 160_000).unwrap(),
            Some(441_000)
        );
        stream.duration_millis = None;
        assert!(seek_position_frame(&stream, MAX_INTEGER / 1000 * 1000).is_err());
    }

    #[tokio::test]
    async fn cached_seek_miss_and_stale_identity_leave_output_unchanged() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let stream = test_stream();
        output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: Some(stream.clone()),
                prepared: None,
                media_revision: 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .unwrap();
        let before = output.snapshot();
        assert!(!output.seek_cached(stream.stream_id, 40_000).await.unwrap());
        assert_eq!(output.snapshot(), before);
        assert!(output.seek_cached(Uuid::new_v4(), 40_000).await.is_err());
        assert_eq!(output.snapshot(), before);
    }

    #[tokio::test]
    async fn accepted_audio_state_always_has_immutable_protocol_metadata() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let stream = test_stream();
        let accepted = output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: Some(stream.clone()),
                prepared: None,
                media_revision: 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .unwrap();
        assert_eq!(accepted.current, Some(stream.clone()));
        let mut changed = stream.clone();
        changed.playback.queue_item_id.0 = "different".into();
        assert!(output
            .execute(ControlCommand::SetMedia {
                command_id: 2,
                current: Some(changed),
                prepared: None,
                media_revision: 2,
                initial_output_state: OutputState::Paused,
            })
            .await
            .is_err());
        assert_eq!(output.snapshot(), accepted);
        assert_eq!(output.snapshot().current, Some(stream));
    }

    #[tokio::test]
    async fn complete_payload_stays_immutable_after_local_promotion() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let current = test_stream();
        let prepared = test_stream();
        let mut grant = ControlCommand::SetMedia {
            command_id: 1,
            current: Some(current.clone()),
            prepared: Some(prepared.clone()),
            media_revision: 1,
            initial_output_state: OutputState::Playing,
        };
        output.execute(grant.clone()).await.unwrap();
        output
            .pipeline
            .execute(AudioCommand::ActivatePrepared {
                current_stream_id: current.stream_id,
                stream_id: prepared.stream_id,
                media_revision: 1,
            })
            .await
            .unwrap();
        let before = output.snapshot();
        if let ControlCommand::SetMedia {
            command_id,
            current,
            ..
        } = &mut grant
        {
            *command_id = 2;
            current.as_mut().unwrap().playback.queue_item_id.0 = "changed-history".into();
        }
        assert!(output.execute(grant).await.is_err());
        assert_eq!(output.snapshot(), before);
    }

    #[tokio::test]
    async fn protocol_rejects_unsafe_successor_revision_before_audio_changes() {
        let output = MashinAudioAdapter::new(1_048_576, 30, false).unwrap();
        let current = test_stream();
        output
            .execute(ControlCommand::SetMedia {
                command_id: 1,
                current: Some(current.clone()),
                prepared: None,
                media_revision: 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .unwrap();
        let before = output.snapshot();
        assert!(output
            .execute(ControlCommand::SetMedia {
                command_id: 2,
                current: Some(current),
                prepared: Some(test_stream()),
                media_revision: MAX_INTEGER + 1,
                initial_output_state: OutputState::Paused,
            })
            .await
            .is_err());
        assert_eq!(output.snapshot(), before);
        assert_eq!(output.streams.lock().unwrap().len(), 1);
    }

    fn idle_welcome(media_revision: u64) -> Welcome {
        Welcome {
            player_id: "mashin-test".into(),
            provider_version: "1".into(),
            server_revision: "test".into(),
            limits: ProtocolLimits::default(),
            audio_profiles: AudioProfileId::ALL.to_vec(),
            selected_audio_profile: AudioProfileId::Flac,
            audio_binding_id: "test-binding".into(),
            current_stream: None,
            desired_output_state: OutputState::Idle,
            media_revision,
            prepared_next: None,
        }
    }
}
