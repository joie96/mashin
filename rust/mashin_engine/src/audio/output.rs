use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream, StreamConfig};
use rubato::{FftFixedInOut, Resampler};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use crate::logging::Logger;

static LOGGER: Logger = Logger::new(module_path!());

struct LocalPlaybackQueue {
    buffers: VecDeque<Arc<[i32]>>,
    sample_offset: usize,
    queued_samples: usize,
}

impl LocalPlaybackQueue {
    fn enqueue(&mut self, samples: Arc<[i32]>) {
        self.queued_samples += samples.len();
        self.buffers.push_back(samples);
    }

    fn next_sample(&mut self) -> Option<i32> {
        loop {
            let buffer = self.buffers.front()?;
            if self.sample_offset < buffer.len() {
                let sample = buffer[self.sample_offset];
                self.sample_offset += 1;
                self.queued_samples = self.queued_samples.saturating_sub(1);
                return Some(sample);
            }
            self.buffers.pop_front();
            self.sample_offset = 0;
        }
    }
}

enum AudioResampler {
    Passthrough,
    Rubato(RubatoResampler),
}

impl AudioResampler {
    fn new(
        source_sample_rate: u32,
        output_sample_rate: u32,
        channels: usize,
    ) -> Result<Self, String> {
        if source_sample_rate == output_sample_rate {
            return Ok(Self::Passthrough);
        }
        RubatoResampler::new(source_sample_rate, output_sample_rate, channels).map(Self::Rubato)
    }

    fn process(&mut self, samples: Arc<[i32]>) -> Result<Arc<[i32]>, String> {
        match self {
            Self::Passthrough => Ok(samples),
            Self::Rubato(resampler) => resampler.process(samples),
        }
    }

    fn finish(&mut self) -> Result<Arc<[i32]>, String> {
        match self {
            Self::Passthrough => Ok(Arc::from([])),
            Self::Rubato(resampler) => resampler.finish(),
        }
    }
}

struct RubatoResampler {
    resampler: FftFixedInOut<f32>,
    channels: usize,
    pending_input: Vec<Vec<f32>>,
    source_sample_rate: u32,
    output_sample_rate: u32,
    input_frames: u64,
    emitted_frames: u64,
    delay_frames: usize,
}

impl RubatoResampler {
    fn new(
        source_sample_rate: u32,
        output_sample_rate: u32,
        channels: usize,
    ) -> Result<Self, String> {
        let resampler = FftFixedInOut::new(
            source_sample_rate as usize,
            output_sample_rate as usize,
            1_024,
            channels,
        )
        .map_err(|error| format!("create audio resampler failed: {error}"))?;
        let delay_frames = resampler.output_delay();
        Ok(Self {
            resampler,
            channels,
            pending_input: vec![Vec::new(); channels],
            source_sample_rate,
            output_sample_rate,
            input_frames: 0,
            emitted_frames: 0,
            delay_frames,
        })
    }

    fn process(&mut self, samples: Arc<[i32]>) -> Result<Arc<[i32]>, String> {
        if samples.len() % self.channels != 0 {
            return Err("audio samples are not frame-aligned for the output channels".to_string());
        }

        self.input_frames += (samples.len() / self.channels) as u64;
        for frame in samples.chunks_exact(self.channels) {
            for (channel, sample) in self.pending_input.iter_mut().zip(frame) {
                channel.push(*sample as f32 / i32::MAX as f32);
            }
        }

        let mut output = Vec::new();
        while self.pending_input[0].len() >= self.resampler.input_frames_next() {
            let input = self
                .pending_input
                .iter_mut()
                .map(|channel| {
                    channel
                        .drain(..self.resampler.input_frames_next())
                        .collect()
                })
                .collect::<Vec<Vec<f32>>>();
            let mut resampled = vec![vec![0.0; self.resampler.output_frames_next()]; self.channels];
            let (_, output_frames) = self
                .resampler
                .process_into_buffer(&input, &mut resampled, None)
                .map_err(|error| format!("resample audio failed: {error}"))?;
            let skip = self.delay_frames.min(output_frames);
            self.delay_frames -= skip;
            for frame_index in skip..output_frames {
                for channel in &resampled {
                    output.push(
                        (channel[frame_index].clamp(-1.0, 1.0) * i32::MAX as f32).round() as i32,
                    );
                }
            }
        }
        self.emitted_frames += (output.len() / self.channels) as u64;
        Ok(Arc::from(output))
    }

    fn finish(&mut self) -> Result<Arc<[i32]>, String> {
        let input_frames = self.input_frames;
        let target_frames =
            input_frames * u64::from(self.output_sample_rate) / u64::from(self.source_sample_rate);
        let mut output = Vec::new();
        while self.emitted_frames < target_frames {
            let padding = vec![0; self.resampler.input_frames_next() * self.channels];
            output.extend_from_slice(&self.process(padding.into())?);
        }
        let excess = (self.emitted_frames - target_frames) as usize * self.channels;
        output.truncate(output.len().saturating_sub(excess));
        self.input_frames = input_frames;
        self.emitted_frames = target_frames;
        Ok(output.into())
    }
}

pub(crate) struct AudioOutput {
    stream: Stream,
    queue: Arc<Mutex<LocalPlaybackQueue>>,
    volume: Arc<AtomicU8>,
    muted: Arc<AtomicBool>,
    underrun: Arc<AtomicBool>,
    underrun_active: Arc<AtomicBool>,
    played_samples: Arc<AtomicU64>,
    audible_samples: Arc<AtomicU64>,
    enqueued_output_samples: AtomicU64,
    enqueued_chunks: AtomicU64,
    output_sample_rate: u32,
    output_channels: u8,
    source_channels: u8,
    source_sample_rate: u32,
    resampler: Mutex<AudioResampler>,
}

impl AudioOutput {
    // Construction.
    pub(crate) fn new(
        sample_rate: u32,
        channels: u8,
        volume: u8,
        muted: bool,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device available".to_string())?;
        let device_config = device
            .default_output_config()
            .map_err(|error| format!("read default output device configuration failed: {error}"))?;
        let stream_config = device_config.config();
        if channels == 0
            || channels > 2
            || stream_config.channels == 0
            || stream_config.channels > 2
        {
            return Err(format!(
                "audio stream has {} channels but the default output device has {} channels",
                channels, stream_config.channels
            ));
        }
        let queue = Arc::new(Mutex::new(LocalPlaybackQueue {
            buffers: VecDeque::new(),
            sample_offset: 0,
            queued_samples: 0,
        }));
        let volume = Arc::new(AtomicU8::new(volume));
        let muted = Arc::new(AtomicBool::new(muted));
        let underrun = Arc::new(AtomicBool::new(false));
        let underrun_active = Arc::new(AtomicBool::new(false));
        let played_samples = Arc::new(AtomicU64::new(0));
        let audible_samples = Arc::new(AtomicU64::new(0));
        let resampler = AudioResampler::new(
            sample_rate,
            stream_config.sample_rate,
            usize::from(stream_config.channels),
        )?;
        LOGGER.debug(format!(
            "building local audio stream: {:?}, {} Hz, {} channels",
            device_config.sample_format(),
            stream_config.sample_rate,
            stream_config.channels,
        ));
        let stream = build_local_fifo_stream(
            &device,
            &stream_config,
            device_config.sample_format(),
            Arc::clone(&queue),
            Arc::clone(&volume),
            Arc::clone(&muted),
            Arc::clone(&underrun),
            Arc::clone(&underrun_active),
            Arc::clone(&played_samples),
            Arc::clone(&audible_samples),
        )?;
        LOGGER.debug("starting local audio stream");
        stream.play().map_err(|error| {
            LOGGER.error(format!("starting local audio stream failed: {error}"));
            error.to_string()
        })?;
        LOGGER.info(format!(
            "audio output started: {} Hz input, {} Hz output, {} channels",
            sample_rate, stream_config.sample_rate, channels
        ));
        Ok(Self {
            stream,
            queue,
            volume,
            muted,
            underrun,
            underrun_active,
            played_samples,
            audible_samples,
            enqueued_output_samples: AtomicU64::new(0),
            enqueued_chunks: AtomicU64::new(0),
            output_sample_rate: stream_config.sample_rate,
            output_channels: stream_config.channels.min(u16::from(u8::MAX)) as u8,
            source_channels: channels,
            source_sample_rate: sample_rate,
            resampler: Mutex::new(resampler),
        })
    }

    // Playback transport commands.
    pub(crate) fn transport_play(&self) -> Result<(), String> {
        self.stream
            .play()
            .map_err(|error| format!("starting local audio stream failed: {error}"))
    }

    // Pause the native stream, not just its samples, so Android stops reporting active music.
    pub(crate) fn transport_pause(&self) -> Result<(), String> {
        self.stream
            .pause()
            .map_err(|error| format!("pausing local audio stream failed: {error}"))
    }

    // Sample enqueueing.
    pub(crate) fn enqueue(&self, samples: Arc<[i32]>) -> Result<(), String> {
        let samples = convert_channels(samples, self.source_channels, self.output_channels)?;
        let samples = self
            .resampler
            .lock()
            .map_err(|_| "audio resampler lock is poisoned".to_string())?
            .process(samples)?;
        {
            let mut queue = self
                .queue
                .lock()
                .map_err(|_| "audio queue lock is poisoned")?;
            self.enqueued_output_samples
                .fetch_add(samples.len() as u64, Ordering::Release);
            queue.enqueue(samples);
            if self.enqueued_chunks.fetch_add(1, Ordering::Relaxed) % 20 == 0 {
                LOGGER.trace(format!(
                    "audio output enqueued: {} samples buffered ({} ms)",
                    queue.queued_samples,
                    self.buffered_duration_millis_from_samples(queue.queued_samples)
                ));
            }
        }
        self.underrun_active.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn finish(&self) -> Result<(), String> {
        let tail = self
            .resampler
            .lock()
            .map_err(|_| "audio resampler lock is poisoned")?
            .finish()?;
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| "audio queue lock is poisoned")?;
        self.enqueued_output_samples
            .fetch_add(tail.len() as u64, Ordering::Release);
        queue.enqueue(tail);
        Ok(())
    }

    // Buffered-duration and position queries.
    pub(crate) fn buffered_duration_millis(&self) -> i64 {
        let Ok(queue) = self.queue.lock() else {
            return 0;
        };
        let samples_per_second = u64::from(self.output_sample_rate)
            .saturating_mul(u64::from(self.output_channels.max(1)));
        if samples_per_second == 0 {
            return 0;
        }
        (queue.queued_samples as u64)
            .saturating_mul(1_000)
            .saturating_div(samples_per_second) as i64
    }

    pub(crate) fn playback_position_millis(&self) -> i64 {
        let samples_per_second = u64::from(self.output_sample_rate)
            .saturating_mul(u64::from(self.output_channels.max(1)));
        if samples_per_second == 0 {
            return 0;
        }
        self.played_samples
            .load(Ordering::Relaxed)
            .saturating_mul(1_000)
            .saturating_div(samples_per_second) as i64
    }

    pub(crate) fn audible_source_frames(&self) -> u64 {
        self.audible_samples
            .load(Ordering::Acquire)
            .saturating_mul(u64::from(self.source_sample_rate))
            / u64::from(self.output_channels)
            / u64::from(self.output_sample_rate)
    }

    pub(crate) fn is_audibly_drained(&self) -> bool {
        self.audible_samples.load(Ordering::Acquire)
            >= self.enqueued_output_samples.load(Ordering::Acquire)
    }

    // Output controls and underrun state.
    pub(crate) fn set_volume(&self, volume: u8) {
        self.volume.store(volume, Ordering::Relaxed);
    }

    pub(crate) fn set_mute(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub(crate) fn take_underrun(&self) -> bool {
        let detected = self.underrun.swap(false, Ordering::Relaxed);
        if detected {
            LOGGER.warn("local audio output underrun; waiting for decoded audio");
        }
        detected
    }

    // Buffer duration conversion.
    fn buffered_duration_millis_from_samples(&self, samples: usize) -> i64 {
        let samples_per_second = u64::from(self.output_sample_rate)
            .saturating_mul(u64::from(self.output_channels.max(1)));
        if samples_per_second == 0 {
            return 0;
        }
        (samples as u64)
            .saturating_mul(1_000)
            .saturating_div(samples_per_second) as i64
    }
}

fn convert_channels(
    samples: Arc<[i32]>,
    source_channels: u8,
    output_channels: u8,
) -> Result<Arc<[i32]>, String> {
    if source_channels == 0 || samples.len() % usize::from(source_channels) != 0 {
        return Err("audio samples are not frame-aligned".into());
    }
    match (source_channels, output_channels) {
        (1, 1) | (2, 2) => Ok(samples),
        (1, 2) => Ok(samples
            .iter()
            .flat_map(|sample| [*sample, *sample])
            .collect::<Vec<_>>()
            .into()),
        (2, 1) => Ok(samples
            .chunks_exact(2)
            .map(|frame| ((i64::from(frame[0]) + i64::from(frame[1])) / 2) as i32)
            .collect::<Vec<_>>()
            .into()),
        _ => Err("unsupported audio channel conversion".into()),
    }
}

fn build_local_fifo_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    queue: Arc<Mutex<LocalPlaybackQueue>>,
    volume: Arc<AtomicU8>,
    muted: Arc<AtomicBool>,
    underrun: Arc<AtomicBool>,
    underrun_active: Arc<AtomicBool>,
    played_samples: Arc<AtomicU64>,
    audible_samples: Arc<AtomicU64>,
) -> Result<Stream, String> {
    macro_rules! build_stream {
        ($sample:ty) => {{
            let queue = Arc::clone(&queue);
            let volume = Arc::clone(&volume);
            let muted = Arc::clone(&muted);
            let underrun = Arc::clone(&underrun);
            let underrun_active = Arc::clone(&underrun_active);
            let played_samples = Arc::clone(&played_samples);
            let audible_samples = Arc::clone(&audible_samples);
            let mut pending_playback = VecDeque::<(cpal::StreamInstant, u64)>::new();
            let sample_rate = config.sample_rate;
            let channels = config.channels;
            device.build_output_stream(
                config.clone(),
                move |output: &mut [$sample], info| {
                    let timestamp = info.timestamp();
                    while pending_playback
                        .front()
                        .is_some_and(|(end, _)| timestamp.callback >= *end)
                    {
                        let (_, position) = pending_playback.pop_front().unwrap();
                        audible_samples.store(position, Ordering::Release);
                    }
                    let gain = if muted.load(Ordering::Relaxed) {
                        0.0
                    } else {
                        volume.load(Ordering::Relaxed) as f32 / 100.0
                    };
                    let Ok(mut queue) = queue.try_lock() else {
                        output.fill(<$sample>::EQUILIBRIUM);
                        return;
                    };
                    let mut underrun_detected = false;
                    let mut last_content_sample = 0;
                    for (sample_index, destination) in output.iter_mut().enumerate() {
                        let sample = match queue.next_sample() {
                            Some(value) => {
                                played_samples.fetch_add(1, Ordering::Relaxed);
                                last_content_sample = sample_index + 1;
                                value
                            }
                            None => {
                                underrun_detected = true;
                                0
                            }
                        } as f32
                            / i32::MAX as f32
                            * gain;
                        *destination = <$sample>::from_sample(sample);
                    }
                    if last_content_sample != 0 {
                        let duration = std::time::Duration::from_secs_f64(
                            last_content_sample as f64
                                / f64::from(channels)
                                / f64::from(sample_rate),
                        );
                        let end = timestamp.playback + duration;
                        pending_playback.push_back((end, played_samples.load(Ordering::Relaxed)));
                    }
                    if underrun_detected {
                        if underrun_active
                            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                            .is_ok()
                        {
                            underrun.store(true, Ordering::Relaxed);
                        }
                    }
                },
                |error| {
                    LOGGER.error(format!("local audio stream error: {error}"));
                },
                None,
            )
        }};
    }

    let stream = match sample_format {
        SampleFormat::F32 => build_stream!(f32),
        SampleFormat::F64 => build_stream!(f64),
        SampleFormat::I8 => build_stream!(i8),
        SampleFormat::I16 => build_stream!(i16),
        SampleFormat::I32 => build_stream!(i32),
        SampleFormat::I64 => build_stream!(i64),
        SampleFormat::U8 => build_stream!(u8),
        SampleFormat::U16 => build_stream!(u16),
        SampleFormat::U32 => build_stream!(u32),
        SampleFormat::U64 => build_stream!(u64),
        _ => {
            return Err(format!(
                "unsupported output sample format: {sample_format:?}"
            ))
        }
    };
    stream.map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_mapping_preserves_frames_and_avoids_integer_overflow() {
        assert_eq!(
            &*convert_channels(Arc::from([12, -4]), 1, 2).unwrap(),
            &[12, 12, -4, -4]
        );
        assert_eq!(
            &*convert_channels(Arc::from([i32::MAX, i32::MAX]), 2, 1).unwrap(),
            &[i32::MAX]
        );
        assert!(convert_channels(Arc::from([1]), 2, 1).is_err());
    }

    #[test]
    fn resampler_flushes_short_final_chunk_without_padding_or_lost_frames() {
        for (source, target) in [(44100, 48000), (48000, 44100)] {
            let mut resampler = AudioResampler::new(source, target, 2).unwrap();
            let samples = resampler.process(vec![1000; 351 * 2].into()).unwrap();
            let tail = resampler.finish().unwrap();
            assert_eq!(
                samples.len() + tail.len(),
                (351 * target as usize / source as usize) * 2
            );
            assert!(resampler.finish().unwrap().is_empty());
        }
    }
}
