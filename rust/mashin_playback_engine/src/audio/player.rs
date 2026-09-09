use ::sendspin::audio::decode::{Decoder, FlacDecoder, OpusDecoder, PcmDecoder, PcmEndian};
use ::sendspin::audio::{AudioFormat, Codec};
use std::sync::Arc;

use super::output::AudioOutput;
use super::source::{AudioSource, SourceReadResult};

pub(crate) enum ActiveDecoder {
    Pcm(PcmDecoder),
    Opus(OpusDecoder),
    Flac(FlacDecoder),
}

impl ActiveDecoder {
    pub(crate) fn from_format(format: &AudioFormat) -> Result<Self, String> {
        match format.codec {
            Codec::Pcm => Ok(Self::Pcm(PcmDecoder::with_endian(
                format.bit_depth,
                PcmEndian::Little,
            ))),
            Codec::Opus => OpusDecoder::new(format.sample_rate, format.channels)
                .map(Self::Opus)
                .map_err(|error| error.to_string()),
            Codec::Flac => match format.codec_header.as_deref() {
                Some(header) => FlacDecoder::with_header(header)
                    .map(Self::Flac)
                    .map_err(|error| error.to_string()),
                None => Ok(Self::Flac(FlacDecoder::new())),
            },
            Codec::Mp3 => Err("mp3 playback is not supported".to_string()),
        }
    }

    pub(crate) fn decode(&self, data: &[u8]) -> Result<Arc<[i32]>, String> {
        match self {
            Self::Pcm(decoder) => decoder.decode(data).map_err(|err| err.to_string()),
            Self::Opus(decoder) => decoder.decode(data).map_err(|err| err.to_string()),
            Self::Flac(decoder) => decoder.decode(data).map_err(|err| err.to_string()),
        }
    }
}

pub(crate) struct AudioPlayer {
    source: Option<AudioSource>,
    decoder: Option<ActiveDecoder>,
    output: Option<AudioOutput>,
    state: AudioPlayerState,
    next_chunk_index: usize,
    source_ended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioPlayerState {
    Idle,
    Buffering,
    Playing,
    Ended,
    Error,
}

impl AudioPlayer {
    pub(crate) fn new() -> Self {
        Self {
            source: None,
            decoder: None,
            output: None,
            state: AudioPlayerState::Idle,
            next_chunk_index: 0,
            source_ended: false,
        }
    }

    pub(crate) fn prepare(&mut self, source: AudioSource) {
        self.stop();
        self.source = Some(source);
        self.state = AudioPlayerState::Buffering;
        self.next_chunk_index = 0;
        self.source_ended = false;
    }

    pub(crate) fn start(
        &mut self,
        format: &AudioFormat,
        volume: u8,
        muted: bool,
    ) -> Result<(), String> {
        if self.source.is_none() {
            return Err("audio source is not prepared".to_string());
        }
        self.decoder = Some(ActiveDecoder::from_format(format)?);
        self.output = Some(AudioOutput::new(format, volume, muted)?);
        self.state = AudioPlayerState::Buffering;
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        self.output = None;
        self.decoder = None;
        self.source = None;
        self.state = AudioPlayerState::Idle;
        self.next_chunk_index = 0;
        self.source_ended = false;
    }

    pub(crate) fn set_volume(&self, volume: u8) {
        if let Some(output) = self.output.as_ref() {
            output.set_volume(volume);
        }
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        if let Some(output) = self.output.as_ref() {
            output.set_mute(muted);
        }
    }

    pub(crate) fn enqueue_encoded(&self, data: &[u8]) -> Result<(), String> {
        let decoder = self
            .decoder
            .as_ref()
            .ok_or_else(|| "audio player is not started".to_string())?;
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| "audio output is not started".to_string())?;
        output.enqueue(decoder.decode(data)?);
        Ok(())
    }

    pub(crate) fn is_started(&self) -> bool {
        self.decoder.is_some() && self.output.is_some()
    }

    pub(crate) fn state(&self) -> AudioPlayerState {
        self.state
    }

    pub(crate) fn pump_until<F>(&mut self, max_buffer_millis: i64, mut read: F)
    where
        F: FnMut(usize) -> SourceReadResult,
    {
        if !self.is_started()
            || !matches!(
                self.state,
                AudioPlayerState::Buffering | AudioPlayerState::Playing
            )
        {
            return;
        }

        while self.buffered_duration_millis() < max_buffer_millis {
            let chunk = match read(self.next_chunk_index) {
                SourceReadResult::Data(chunk) => Some(chunk),
                SourceReadResult::Waiting => None,
                SourceReadResult::EndOfStream => {
                    self.source_ended = true;
                    None
                }
                SourceReadResult::Error(_) => {
                    self.state = AudioPlayerState::Error;
                    return;
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            if self.enqueue_encoded(&chunk).is_err() {
                self.state = AudioPlayerState::Error;
                return;
            }
            self.next_chunk_index = self.next_chunk_index.saturating_add(1);
        }

        self.state = if self.buffered_duration_millis() > 0 {
            AudioPlayerState::Playing
        } else if self.source_ended {
            AudioPlayerState::Ended
        } else {
            AudioPlayerState::Buffering
        };
    }

    pub(crate) fn buffered_duration_millis(&self) -> i64 {
        self.output
            .as_ref()
            .map(AudioOutput::buffered_duration_millis)
            .unwrap_or(0)
    }

    pub(crate) fn playback_position_millis(&self) -> i64 {
        self.output
            .as_ref()
            .map(AudioOutput::playback_position_millis)
            .unwrap_or(0)
    }
}
