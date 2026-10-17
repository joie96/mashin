//! Codec and frame contracts independent of receiver protocols and MA metadata.

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioSnapshot {
    pub(crate) status: AudioStatus,
    pub(crate) current: Option<AudioStream>,
    pub(crate) prepared: Option<AudioStream>,
    pub(crate) buffer_capacity: usize,
    pub(crate) buffered_duration_millis: u64,
    pub(crate) error: Option<String>,
    pub(crate) activation: Option<PreparedActivation>,
    pub(crate) audible_events: Vec<(u64, AudioEvent)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedActivation {
    pub(crate) previous_stream_id: Uuid,
    pub(crate) stream_id: Uuid,
    pub(crate) media_revision: u64,
    pub(crate) previous_position_frame: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AudioEvent {
    PreparedActivated {
        activation: PreparedActivation,
        output_revision: u64,
    },
    TrackStarted {
        stream_id: Uuid,
        media_revision: u64,
        output_revision: u64,
    },
    TrackEnded {
        stream_id: Uuid,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioState {
    Idle,
    Buffering,
    Playing,
    Paused,
    Ended,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioStream {
    pub(crate) stream_id: Uuid,
    pub(crate) output_format: AudioFormat,
}

impl AudioStream {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.output_format.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChunkCursor {
    pub(crate) stream_id: Uuid,
    pub(crate) next_chunk_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioStatus {
    pub(crate) current_stream_id: Option<Uuid>,
    pub(crate) output_revision: u64,
    pub(crate) output_state: AudioState,
    pub(crate) position_frame: u64,
    pub(crate) volume_percent: u8,
    pub(crate) is_muted: bool,
    pub(crate) media_revision: u64,
    pub(crate) received_streams: Vec<ChunkCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AudioCommand {
    ActivatePrepared {
        current_stream_id: Uuid,
        stream_id: Uuid,
        media_revision: u64,
    },
    SetMedia {
        current: Option<AudioStream>,
        prepared: Option<AudioStream>,
        media_revision: u64,
        initial_output_state: AudioState,
    },
    Play {
        stream_id: Option<Uuid>,
    },
    Pause {
        stream_id: Option<Uuid>,
    },
    Stop {
        stream_id: Option<Uuid>,
    },
    SetVolume {
        volume_percent: u8,
    },
    SetMute {
        is_muted: bool,
    },
}

#[cfg(test)]
pub(crate) fn test_stream() -> AudioStream {
    AudioStream {
        stream_id: Uuid::new_v4(),
        output_format: test_format(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioCodec {
    Flac,
    Opus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioFormat {
    pub(crate) codec: AudioCodec,
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: u8,
    pub(crate) bit_depth: Option<u8>,
}

impl AudioFormat {
    // Decoder constraints apply independently of the network profile selection.
    pub(crate) fn validate(&self) -> Result<(), String> {
        let is_valid = match self.codec {
            AudioCodec::Flac => {
                matches!(self.sample_rate_hz, 44_100 | 48_000)
                    && matches!(self.channels, 1 | 2)
                    && self.bit_depth == Some(16)
            }
            AudioCodec::Opus => {
                self.sample_rate_hz == 48_000 && self.channels == 2 && self.bit_depth.is_none()
            }
        };
        if is_valid {
            Ok(())
        } else {
            Err("unsupported audio format".into())
        }
    }
}

#[cfg(test)]
pub(crate) fn test_format() -> AudioFormat {
    AudioFormat {
        codec: AudioCodec::Flac,
        sample_rate_hz: 48_000,
        channels: 2,
        bit_depth: Some(16),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoder_format_rejects_invalid_layouts() {
        let mut format = test_format();
        assert!(format.validate().is_ok());
        format.channels = 0;
        assert!(format.validate().is_err());
        format.channels = 2;
        format.codec = AudioCodec::Opus;
        assert!(format.validate().is_err());
        format.bit_depth = None;
        assert!(format.validate().is_ok());
        format.sample_rate_hz = 44_100;
        assert!(format.validate().is_err());
    }
}
