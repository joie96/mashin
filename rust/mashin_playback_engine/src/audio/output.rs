use ::sendspin::audio::AudioFormat;
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
    fn new(source_sample_rate: u32, output_sample_rate: u32, channels: usize) -> Result<Self, String> {
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
}

struct RubatoResampler {
    resampler: FftFixedInOut<f32>,
    channels: usize,
    pending_input: Vec<Vec<f32>>,
}

impl RubatoResampler {
    fn new(source_sample_rate: u32, output_sample_rate: u32, channels: usize) -> Result<Self, String> {
        let resampler = FftFixedInOut::new(
            source_sample_rate as usize,
            output_sample_rate as usize,
            1_024,
            channels,
        )
        .map_err(|error| format!("create audio resampler failed: {error}"))?;
        Ok(Self {
            resampler,
            channels,
            pending_input: vec![Vec::new(); channels],
        })
    }

    fn process(&mut self, samples: Arc<[i32]>) -> Result<Arc<[i32]>, String> {
        if samples.len() % self.channels != 0 {
            return Err("audio samples are not frame-aligned for the output channels".to_string());
        }

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
                .map(|channel| channel.drain(..self.resampler.input_frames_next()).collect())
                .collect::<Vec<Vec<f32>>>();
            let mut resampled = vec![vec![0.0; self.resampler.output_frames_next()]; self.channels];
            let (_, output_frames) = self
                .resampler
                .process_into_buffer(&input, &mut resampled, None)
                .map_err(|error| format!("resample audio failed: {error}"))?;
            for frame_index in 0..output_frames {
                for channel in &resampled {
                    output.push(
                        (channel[frame_index].clamp(-1.0, 1.0) * i32::MAX as f32).round() as i32,
                    );
                }
            }
        }
        Ok(Arc::from(output))
    }
}

pub(crate) struct AudioOutput {
    _stream: Stream,
    queue: Arc<Mutex<LocalPlaybackQueue>>,
    volume: Arc<AtomicU8>,
    muted: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    underrun: Arc<AtomicBool>,
    underrun_active: Arc<AtomicBool>,
    played_samples: Arc<AtomicU64>,
    enqueued_chunks: AtomicU64,
    output_sample_rate: u32,
    output_channels: u8,
    resampler: Mutex<AudioResampler>,
}

impl AudioOutput {
    pub(crate) fn new(format: &AudioFormat, volume: u8, muted: bool) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device available".to_string())?;
        let device_config = device
            .default_output_config()
            .map_err(|error| format!("read default output device configuration failed: {error}"))?;
        let stream_config = device_config.config();
        if stream_config.channels != u16::from(format.channels) {
            return Err(format!(
                "audio stream has {} channels but the default output device has {} channels",
                format.channels,
                stream_config.channels
            ));
        }
        let queue = Arc::new(Mutex::new(LocalPlaybackQueue {
            buffers: VecDeque::new(),
            sample_offset: 0,
            queued_samples: 0,
        }));
        let volume = Arc::new(AtomicU8::new(volume));
        let muted = Arc::new(AtomicBool::new(muted));
        let paused = Arc::new(AtomicBool::new(false));
        let underrun = Arc::new(AtomicBool::new(false));
        let underrun_active = Arc::new(AtomicBool::new(false));
        let played_samples = Arc::new(AtomicU64::new(0));
        let resampler = AudioResampler::new(
            format.sample_rate,
            stream_config.sample_rate,
            usize::from(format.channels),
        )?;
        let stream = build_local_fifo_stream(
            &device,
            &stream_config,
            device_config.sample_format(),
            Arc::clone(&queue),
            Arc::clone(&volume),
            Arc::clone(&muted),
            Arc::clone(&paused),
            Arc::clone(&underrun),
            Arc::clone(&underrun_active),
            Arc::clone(&played_samples),
        )?;
        stream.play().map_err(|err| err.to_string())?;
        LOGGER.info(format!(
            "audio output started: {} Hz input, {} Hz output, {} channels",
            format.sample_rate, stream_config.sample_rate, format.channels
        ));
        Ok(Self {
            _stream: stream,
            queue,
            volume,
            muted,
            paused,
            underrun,
            underrun_active,
            played_samples,
            enqueued_chunks: AtomicU64::new(0),
            output_sample_rate: stream_config.sample_rate,
            output_channels: stream_config.channels.min(u16::from(u8::MAX)) as u8,
            resampler: Mutex::new(resampler),
        })
    }

    pub(crate) fn enqueue(&self, samples: Arc<[i32]>) -> Result<(), String> {
        let samples = self
            .resampler
            .lock()
            .map_err(|_| "audio resampler lock is poisoned".to_string())?
            .process(samples)?;
        if let Ok(mut queue) = self.queue.lock() {
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

    pub(crate) fn buffered_duration_millis(&self) -> i64 {
        let Ok(queue) = self.queue.lock() else {
            return 0;
        };
        let samples_per_second =
            u64::from(self.output_sample_rate).saturating_mul(u64::from(self.output_channels.max(1)));
        if samples_per_second == 0 {
            return 0;
        }
        (queue.queued_samples as u64)
            .saturating_mul(1_000)
            .saturating_div(samples_per_second) as i64
    }

    pub(crate) fn playback_position_millis(&self) -> i64 {
        let samples_per_second =
            u64::from(self.output_sample_rate).saturating_mul(u64::from(self.output_channels.max(1)));
        if samples_per_second == 0 {
            return 0;
        }
        self.played_samples
            .load(Ordering::Relaxed)
            .saturating_mul(1_000)
            .saturating_div(samples_per_second) as i64
    }

    pub(crate) fn set_volume(&self, volume: u8) {
        self.volume.store(volume, Ordering::Relaxed);
    }

    pub(crate) fn set_mute(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        LOGGER.debug(format!("audio output {}", if paused { "paused" } else { "resumed" }));
    }

    pub(crate) fn take_underrun(&self) -> bool {
        let detected = self.underrun.swap(false, Ordering::Relaxed);
        if detected {
            LOGGER.warn("local audio output underrun; waiting for decoded audio");
        }
        detected
    }

    fn buffered_duration_millis_from_samples(&self, samples: usize) -> i64 {
        let samples_per_second =
            u64::from(self.output_sample_rate).saturating_mul(u64::from(self.output_channels.max(1)));
        if samples_per_second == 0 {
            return 0;
        }
        (samples as u64)
            .saturating_mul(1_000)
            .saturating_div(samples_per_second) as i64
    }
}

fn build_local_fifo_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    queue: Arc<Mutex<LocalPlaybackQueue>>,
    volume: Arc<AtomicU8>,
    muted: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    underrun: Arc<AtomicBool>,
    underrun_active: Arc<AtomicBool>,
    played_samples: Arc<AtomicU64>,
) -> Result<Stream, String> {
    macro_rules! build_stream {
        ($sample:ty) => {{
            let queue = Arc::clone(&queue);
            let volume = Arc::clone(&volume);
            let muted = Arc::clone(&muted);
            let paused = Arc::clone(&paused);
            let underrun = Arc::clone(&underrun);
            let underrun_active = Arc::clone(&underrun_active);
            let played_samples = Arc::clone(&played_samples);
            device.build_output_stream(
                config.clone(),
                move |output: &mut [$sample], _| {
                    if paused.load(Ordering::Relaxed) {
                        output.fill(<$sample>::EQUILIBRIUM);
                        return;
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
                    for destination in output {
                        let sample = match queue.next_sample() {
                            Some(value) => {
                                played_samples.fetch_add(1, Ordering::Relaxed);
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
