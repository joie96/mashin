//! Mashin commands and metadata are translated here, outside the native audio owner.

use super::models::{
    HEADER_BYTES, MAX_INTEGER, MAX_PAYLOAD_BYTES, OutputState, StreamCursor, StreamDescriptor,
};
use super::protocol::{ControlCommand, ProtocolError, ResumeState, Welcome};
use crate::audio::buffer::EncodedChunk;
use crate::audio::models::{AudioCommand, AudioState, AudioStream};
use crate::audio::models::{AudioEvent, AudioSnapshot};
use crate::audio::pipeline::AudioPipeline;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};
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
        })
    }

    // Metadata must already exist when the worker publishes a newly accepted stream.
    pub(crate) async fn execute(&self, command: ControlCommand) -> Result<OutputSnapshot, String> {
        let _command = self.commands.lock().await;
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
                    initial_output_state: match initial_output_state {
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

    pub(crate) async fn reconcile(&self, welcome: Welcome) -> Result<OutputSnapshot, String> {
        let _command = self.commands.lock().await;
        let snapshot = self.snapshot();
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
        if welcome.current_stream.as_ref().map(|state| &state.stream) != snapshot.current.as_ref()
            || welcome.prepared_next.as_ref().map(|state| &state.stream)
                != snapshot.prepared.as_ref()
            || welcome.media_revision != snapshot.resume.media_revision
        {
            return Err("Mashin playback changed during handshake".into());
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
        Ok(snapshot)
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
            output_revision: state.output_revision,
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
    use super::super::models::test_stream;
    use super::*;

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
        assert!(
            output
                .execute(ControlCommand::SetMedia {
                    command_id: 2,
                    current: Some(changed),
                    prepared: None,
                    media_revision: 2,
                    initial_output_state: OutputState::Paused,
                })
                .await
                .is_err()
        );
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
        assert!(
            output
                .execute(ControlCommand::SetMedia {
                    command_id: 2,
                    current: Some(current),
                    prepared: Some(test_stream()),
                    media_revision: MAX_INTEGER + 1,
                    initial_output_state: OutputState::Paused,
                })
                .await
                .is_err()
        );
        assert_eq!(output.snapshot(), before);
        assert_eq!(output.streams.lock().unwrap().len(), 1);
    }
}
