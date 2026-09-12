use ::sendspin::audio::decode::{Decoder, FlacDecoder, OpusDecoder, PcmDecoder, PcmEndian};
use ::sendspin::audio::{AudioFormat, Codec};
use std::sync::Arc;

use super::output::AudioOutput;
use super::source::{AudioSource, SourceReadResult};
use crate::logging::Logger;

static LOGGER: Logger = Logger::new(module_path!());

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
    skip_remaining_millis: u64,
    input_channels: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioPlayerState {
    Idle,
    Buffering,
    Playing,
    Paused,
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
            skip_remaining_millis: 0,
            input_channels: 0,
        }
    }

    pub(crate) fn prepare_with_seek(
        &mut self,
        source: AudioSource,
        chunk_index: usize,
        skip_millis: u64,
    ) {
        self.stop();
        self.source = Some(source);
        self.state = AudioPlayerState::Buffering;
        self.next_chunk_index = chunk_index;
        self.source_ended = false;
        self.skip_remaining_millis = skip_millis;
        LOGGER.debug("audio source prepared");
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
        self.input_channels = usize::from(format.channels.max(1));
        self.state = AudioPlayerState::Buffering;
        LOGGER.info(format!(
            "audio playback started: {:?}, {} Hz, {} channels",
            format.codec, format.sample_rate, format.channels
        ));
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        self.output = None;
        self.decoder = None;
        self.source = None;
        self.state = AudioPlayerState::Idle;
        self.next_chunk_index = 0;
        self.source_ended = false;
        self.skip_remaining_millis = 0;
        self.input_channels = 0;
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

    pub(crate) fn pause(&mut self) {
        if !matches!(
            self.state,
            AudioPlayerState::Buffering | AudioPlayerState::Playing
        ) {
            return;
        }
        if let Some(output) = self.output.as_ref() {
            output.set_paused(true);
        }
        self.state = AudioPlayerState::Paused;
    }

    pub(crate) fn resume(&mut self) {
        if self.state != AudioPlayerState::Paused {
            return;
        }
        if let Some(output) = self.output.as_ref() {
            output.set_paused(false);
        }
        self.state = if self.buffered_duration_millis() > 0 {
            AudioPlayerState::Playing
        } else {
            AudioPlayerState::Buffering
        };
    }

    pub(crate) fn enqueue_encoded(
        &self,
        data: &[u8],
        skip_millis: u64,
        chunk_duration_millis: u64,
    ) -> Result<(), String> {
        let decoder = self
            .decoder
            .as_ref()
            .ok_or_else(|| "audio player is not started".to_string())?;
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| "audio output is not started".to_string())?;
        let samples = decoder.decode(data)?;
        if samples.is_empty() || skip_millis == 0 || chunk_duration_millis == 0 {
            return output.enqueue(samples);
        }
        let skipped_samples = samples
            .len()
            .saturating_mul(skip_millis.min(chunk_duration_millis) as usize)
            .saturating_div(chunk_duration_millis as usize);
        let skipped_samples = skipped_samples
            .saturating_div(self.input_channels.max(1))
            .saturating_mul(self.input_channels.max(1));
        if skipped_samples >= samples.len() {
            return Ok(());
        }
        output.enqueue(Arc::from(samples[skipped_samples..].to_vec()))
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

        let underrun_detected = self
            .output
            .as_ref()
            .map(AudioOutput::take_underrun)
            .unwrap_or(false);

        while self.buffered_duration_millis() < max_buffer_millis {
            let chunk = match read(self.next_chunk_index) {
                SourceReadResult::Data { data, duration_millis } => Some((data, duration_millis)),
                SourceReadResult::Waiting => None,
                SourceReadResult::EndOfStream => {
                    self.source_ended = true;
                    None
                }
                SourceReadResult::Error(error) => {
                    LOGGER.error(format!("audio source read failed: {error}"));
                    self.state = AudioPlayerState::Error;
                    return;
                }
            };
            let Some((chunk, duration_millis)) = chunk else {
                break;
            };
            let skip_millis = self.skip_remaining_millis.min(duration_millis);
            if let Err(error) = self.enqueue_encoded(&chunk, skip_millis, duration_millis) {
                LOGGER.error(format!("audio chunk decode failed: {error}"));
                self.state = AudioPlayerState::Error;
                return;
            }
            self.skip_remaining_millis = self.skip_remaining_millis.saturating_sub(duration_millis);
            self.next_chunk_index = self.next_chunk_index.saturating_add(1);
        }

        self.state = if self.buffered_duration_millis() > 0 {
            AudioPlayerState::Playing
        } else if underrun_detected {
            AudioPlayerState::Buffering
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
