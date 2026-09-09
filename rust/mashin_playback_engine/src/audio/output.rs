use ::sendspin::audio::AudioFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream, StreamConfig};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use crate::log_error;

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

    fn clear(&mut self) {
        self.buffers.clear();
        self.sample_offset = 0;
        self.queued_samples = 0;
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

pub(crate) struct AudioOutput {
    _stream: Stream,
    queue: Arc<Mutex<LocalPlaybackQueue>>,
    volume: Arc<AtomicU8>,
    muted: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    played_samples: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u8,
}

impl AudioOutput {
    pub(crate) fn new(format: &AudioFormat, volume: u8, muted: bool) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device available".to_string())?;
        let device_config = device
            .default_output_config()
            .map_err(|err| err.to_string())?;
        let stream_config = StreamConfig {
            channels: format.channels as u16,
            sample_rate: cpal::SampleRate::from(format.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };
        let queue = Arc::new(Mutex::new(LocalPlaybackQueue {
            buffers: VecDeque::new(),
            sample_offset: 0,
            queued_samples: 0,
        }));
        let volume = Arc::new(AtomicU8::new(volume));
        let muted = Arc::new(AtomicBool::new(muted));
        let paused = Arc::new(AtomicBool::new(false));
        let played_samples = Arc::new(AtomicU64::new(0));
        let stream = build_local_fifo_stream(
            &device,
            &stream_config,
            device_config.sample_format(),
            Arc::clone(&queue),
            Arc::clone(&volume),
            Arc::clone(&muted),
            Arc::clone(&paused),
            Arc::clone(&played_samples),
        )?;
        stream.play().map_err(|err| err.to_string())?;
        Ok(Self {
            _stream: stream,
            queue,
            volume,
            muted,
            paused,
            played_samples,
            sample_rate: format.sample_rate,
            channels: format.channels,
        })
    }

    pub(crate) fn enqueue(&self, samples: Arc<[i32]>) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.enqueue(samples);
        }
    }

    pub(crate) fn clear(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
        self.played_samples.store(0, Ordering::Relaxed);
    }

    pub(crate) fn buffered_duration_millis(&self) -> i64 {
        let Ok(queue) = self.queue.lock() else {
            return 0;
        };
        let samples_per_second =
            u64::from(self.sample_rate).saturating_mul(u64::from(self.channels.max(1)));
        if samples_per_second == 0 {
            return 0;
        }
        (queue.queued_samples as u64)
            .saturating_mul(1_000)
            .saturating_div(samples_per_second) as i64
    }

    pub(crate) fn playback_position_millis(&self) -> i64 {
        let samples_per_second =
            u64::from(self.sample_rate).saturating_mul(u64::from(self.channels.max(1)));
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
    played_samples: Arc<AtomicU64>,
) -> Result<Stream, String> {
    macro_rules! build_stream {
        ($sample:ty) => {{
            let queue = Arc::clone(&queue);
            let volume = Arc::clone(&volume);
            let muted = Arc::clone(&muted);
            let paused = Arc::clone(&paused);
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
                    for destination in output {
                        let sample = match queue.next_sample() {
                            Some(value) => {
                                played_samples.fetch_add(1, Ordering::Relaxed);
                                value
                            }
                            None => 0,
                        } as f32
                            / i32::MAX as f32
                            * gain;
                        *destination = <$sample>::from_sample(sample);
                    }
                },
                |error| log_error(&format!("local audio stream error: {error}")),
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
