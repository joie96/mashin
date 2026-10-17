//! One native owner for CPAL, the decoder and the bounded current/next audio buffer.
//! Network reconnects only replace sockets; dropping this worker stops local audio.

use super::buffer::{BufferStatistics, StreamBuffer};
use super::models::{AudioCommand, AudioFormat, AudioState, AudioStatus};
use crate::audio::buffer::EncodedChunk;
use crate::audio::decoder::decode_chunk;

use crate::audio::output::AudioOutput;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use uuid::Uuid;

use super::models::{AudioEvent, AudioSnapshot};

static LOGGER: crate::logging::Logger = crate::logging::Logger::new("audio-buffer");
const BUFFER_SUMMARY_INTERVAL: Duration = Duration::from_secs(5);

trait OutputSink: Sized {
    fn new(format: &AudioFormat, volume: u8, is_muted: bool) -> Result<Self, String>;
    fn transport_play(&self) -> Result<(), String>;
    fn transport_pause(&self) -> Result<(), String>;
    fn enqueue(&self, samples: Arc<[i32]>) -> Result<(), String>;
    fn finish(&self) -> Result<(), String>;
    fn position_frame(&self) -> u64;
    fn is_audibly_drained(&self) -> bool;
    fn buffered_duration_millis(&self) -> u64;
    fn set_volume(&self, volume: u8);
    fn set_mute(&self, is_muted: bool);
}

impl OutputSink for AudioOutput {
    // Native output configuration.
    fn new(format: &AudioFormat, volume: u8, is_muted: bool) -> Result<Self, String> {
        AudioOutput::new(format.sample_rate_hz, format.channels, volume, is_muted)
    }

    // Transport and decoded-frame ownership.
    fn transport_play(&self) -> Result<(), String> {
        self.transport_play()
    }
    fn transport_pause(&self) -> Result<(), String> {
        self.transport_pause()
    }
    fn enqueue(&self, samples: Arc<[i32]>) -> Result<(), String> {
        self.enqueue(samples)
    }
    fn finish(&self) -> Result<(), String> {
        self.finish()
    }
    fn set_volume(&self, volume: u8) {
        self.set_volume(volume);
    }
    fn set_mute(&self, is_muted: bool) {
        self.set_mute(is_muted);
    }

    // Device-clock queries.
    fn position_frame(&self) -> u64 {
        self.audible_source_frames()
    }
    fn is_audibly_drained(&self) -> bool {
        self.is_audibly_drained()
    }
    fn buffered_duration_millis(&self) -> u64 {
        self.buffered_duration_millis().max(0) as u64
    }
}

struct PipelineState<Sink: OutputSink> {
    cache: StreamBuffer,
    sink: Option<Sink>,
    state: AudioState,
    revision: u64,
    position_frame: u64,
    position_origin_frame: u64,
    volume_percent: u8,
    is_muted: bool,
    should_play: bool,
    decoded_chunk_index: u64,
    decoded_frames: u64,
    is_final_decoded: bool,
    has_reported_end: bool,
    error: Option<String>,
    activation: Option<crate::audio::models::PreparedActivation>,
    audible_events: VecDeque<(u64, AudioEvent)>,
    event_counter: u64,
    last_buffer_summary: Instant,
}

impl<Sink: OutputSink> PipelineState<Sink> {
    // Configuration and lifecycle.
    fn new(cache_bytes: usize, volume_percent: u8, is_muted: bool) -> Result<Self, String> {
        if cache_bytes < 1 || volume_percent > 100 {
            return Err("invalid native audio output configuration".into());
        }
        Ok(Self {
            cache: StreamBuffer::new(cache_bytes),
            sink: None,
            state: AudioState::Idle,
            revision: 0,
            position_frame: 0,
            position_origin_frame: 0,
            volume_percent,
            is_muted,
            should_play: false,
            decoded_chunk_index: 1,
            decoded_frames: 0,
            is_final_decoded: false,
            has_reported_end: false,
            error: None,
            activation: None,
            audible_events: VecDeque::new(),
            event_counter: 0,
            last_buffer_summary: Instant::now(),
        })
    }

    // Commands are applied atomically on the audio owner, never on a network task.
    fn execute(&mut self, command: AudioCommand) -> Result<(), String> {
        match command {
            AudioCommand::ActivatePrepared {
                current_stream_id,
                stream_id,
                next_revision,
            } => {
                if self.activation.as_ref().is_some_and(|activation| {
                    activation.previous_stream_id == current_stream_id
                        && activation.stream_id == stream_id
                        && activation.next_revision == next_revision
                }) && self
                    .cache
                    .current()
                    .is_some_and(|stream| stream.descriptor.stream_id == stream_id)
                {
                    return Ok(());
                }
                let activation = crate::audio::models::PreparedActivation {
                    previous_stream_id: current_stream_id,
                    stream_id,
                    next_revision,
                    previous_position_frame: self.position_frame,
                };
                let output_revision = self
                    .revision
                    .checked_add(1)
                    .ok_or("output revision overflow")?;
                let event_id = self
                    .event_counter
                    .checked_add(1)
                    .ok_or("audio event overflow")?;
                self.cache
                    .activate_prepared(current_stream_id, stream_id, next_revision)?;
                self.reset_output();
                self.activation = Some(activation.clone());
                self.should_play = true;
                self.state = AudioState::Buffering;
                self.event_counter = event_id;
                if self.audible_events.len() == 32 {
                    self.audible_events.pop_front();
                }
                self.audible_events.push_back((
                    self.event_counter,
                    AudioEvent::PreparedActivated {
                        activation,
                        output_revision,
                    },
                ));
            }
            AudioCommand::SetMedia {
                stream, autoplay, ..
            } => {
                self.cache.set_current(stream)?;
                self.activation = None;
                self.reset_output();
                self.should_play = autoplay;
                self.state = if autoplay {
                    AudioState::Buffering
                } else {
                    AudioState::Paused
                };
            }
            AudioCommand::PrepareNext {
                current_stream_id,
                stream,
                next_revision,
                ..
            } => {
                return self
                    .cache
                    .prepare_next(current_stream_id, stream, next_revision);
            }
            AudioCommand::ClearNext {
                current_stream_id,
                next_revision,
                ..
            } => {
                return self.cache.clear_next(current_stream_id, next_revision);
            }
            AudioCommand::Play { stream_id, .. } => {
                self.require_stream(stream_id)?;
                if self.cache.current().is_none() {
                    return Err("no current native audio stream".into());
                }
                if let Some(sink) = &self.sink {
                    sink.transport_play()?;
                }
                self.should_play = true;
                self.state = if self.decoded_frames > self.position_frame {
                    AudioState::Playing
                } else {
                    AudioState::Buffering
                };
            }
            AudioCommand::Pause { stream_id, .. } => {
                self.require_stream(stream_id)?;
                if let Some(sink) = &self.sink {
                    sink.transport_pause()?;
                }
                self.should_play = false;
                self.state = if self.cache.current().is_some() {
                    AudioState::Paused
                } else {
                    AudioState::Idle
                };
            }
            AudioCommand::Stop { stream_id, .. } => {
                self.require_stream(stream_id)?;
                self.cache.clear();
                self.activation = None;
                self.reset_output();
                self.should_play = false;
                self.state = AudioState::Idle;
            }
            AudioCommand::SetVolume { volume_percent, .. } => {
                if volume_percent > 100 {
                    return Err("invalid native audio volume".into());
                }
                if let Some(sink) = &self.sink {
                    sink.set_volume(volume_percent);
                }
                self.volume_percent = volume_percent;
            }
            AudioCommand::SetMute { is_muted, .. } => {
                if let Some(sink) = &self.sink {
                    sink.set_mute(is_muted);
                }
                self.is_muted = is_muted;
            }
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("native audio output revision exhausted")?;
        Ok(())
    }

    // Cache misses leave both playback and receipt ownership untouched.
    fn seek_cached(&mut self, stream_id: Uuid, position_frame: u64) -> Result<bool, String> {
        self.require_stream(Some(stream_id))?;
        if self.error.is_some() || self.state == AudioState::Idle {
            return Err("no active native audio stream".into());
        }
        let current = self
            .cache
            .current()
            .ok_or("no current native audio stream")?;
        let sample_rate = u64::from(current.descriptor.output_format.sample_rate_hz);
        if position_frame % sample_rate != 0 {
            return Err("cached seek must start at a whole-second chunk boundary".into());
        }
        let index = position_frame / sample_rate + 1;
        if !current
            .chunk(index)
            .is_some_and(|chunk| chunk.first_frame == position_frame)
        {
            return Ok(false);
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or("native audio output revision exhausted")?;
        self.reset_output();
        self.position_origin_frame = position_frame;
        self.position_frame = position_frame;
        self.decoded_frames = position_frame;
        self.decoded_chunk_index = index;
        self.revision = revision;
        self.state = if self.should_play {
            AudioState::Buffering
        } else {
            AudioState::Paused
        };
        Ok(true)
    }

    fn retain(&mut self, chunk: EncodedChunk) -> Result<(), String> {
        self.cache.retain(chunk)?;
        self.cache.release_played_chunks(self.position_frame);
        Ok(())
    }

    // Snapshots retain receipt cursors independently of decoded or released chunks.
    fn snapshot(&self) -> AudioSnapshot {
        AudioSnapshot {
            status: AudioStatus {
                current_stream_id: self
                    .cache
                    .current()
                    .map(|stream| stream.descriptor.stream_id),
                output_revision: self.revision,
                output_state: self.state,
                position_frame: self.position_frame,
                volume_percent: self.volume_percent,
                is_muted: self.is_muted,
                next_revision: self.cache.next_revision(),
                received_streams: self.cache.cursors(),
            },
            current: self.cache.current().map(|stream| stream.descriptor.clone()),
            prepared: self
                .cache
                .prepared()
                .map(|stream| stream.descriptor.clone()),
            buffer_capacity: self.cache.advertised_capacity(),
            buffered_duration_millis: self
                .cache
                .statistics(self.position_frame)
                .current_remaining_millis,
            error: self.error.clone(),
            activation: self.activation.clone(),
            audible_events: self.audible_events.iter().cloned().collect(),
        }
    }

    // Summaries use audible position and include retained encoded audio, not just decoded PCM.
    fn take_buffer_statistics(&mut self, now: Instant) -> Option<BufferStatistics> {
        if self.cache.current().is_none()
            || now.saturating_duration_since(self.last_buffer_summary) < BUFFER_SUMMARY_INTERVAL
        {
            return None;
        }
        self.last_buffer_summary = now;
        Some(self.cache.statistics(self.position_frame))
    }

    // Only this worker decodes; at most two seconds of PCM are submitted ahead.
    fn pump(&mut self) -> Result<Vec<AudioEvent>, String> {
        self.pump_with(decode_chunk)
    }

    fn pump_with(
        &mut self,
        decode: impl FnOnce(&EncodedChunk, &AudioFormat) -> Result<Arc<[i32]>, String>,
    ) -> Result<Vec<AudioEvent>, String> {
        if self.error.is_some() {
            return Ok(Vec::new());
        }
        let Some(current) = self.cache.current() else {
            return Ok(Vec::new());
        };
        let stream_id = current.descriptor.stream_id;
        if let Some(sink) = &self.sink {
            self.position_frame = self
                .position_origin_frame
                .saturating_add(sink.position_frame())
                .min(self.decoded_frames);
            if self.is_final_decoded && sink.is_audibly_drained() {
                self.position_frame = self.decoded_frames;
            }
        }
        self.cache.release_played_chunks(self.position_frame);
        let mut events = Vec::new();
        if self.should_play && self.is_final_decoded && self.position_frame == self.decoded_frames {
            if !self.has_reported_end {
                events.push(AudioEvent::TrackEnded { stream_id });
                self.has_reported_end = true;
                self.state = AudioState::Ended;
            }
            if self
                .cache
                .prepared()
                .is_some_and(|stream| stream.cursor().next_chunk_index > 1)
            {
                let (stream_id, next_revision) = self.cache.promote_next(self.position_frame)?;
                self.reset_output();
                self.activation = None;
                self.revision = self
                    .revision
                    .checked_add(1)
                    .ok_or("native audio output revision exhausted")?;
                self.state = AudioState::Buffering;
                events.push(AudioEvent::TrackStarted {
                    stream_id,
                    next_revision,
                    output_revision: self.revision,
                });
            }
            return Ok(events);
        }
        if self.is_final_decoded
            || self
                .sink
                .as_ref()
                .is_some_and(|sink| sink.buffered_duration_millis() >= 1000)
        {
            return Ok(events);
        }
        let current = self.cache.current().unwrap();
        let Some(chunk) = current.chunk(self.decoded_chunk_index) else {
            if self.should_play
                && self
                    .sink
                    .as_ref()
                    .is_none_or(|sink| sink.is_audibly_drained())
            {
                self.state = AudioState::Buffering;
            }
            return Ok(events);
        };
        let format = current.descriptor.output_format.clone();
        let samples = decode(chunk, &format)?;
        if self.sink.is_none() {
            let sink = Sink::new(&format, self.volume_percent, self.is_muted)?;
            if !self.should_play {
                sink.transport_pause()?;
            }
            self.sink = Some(sink);
        }
        let sink = self.sink.as_ref().unwrap();
        sink.enqueue(samples)?;
        if chunk.is_final {
            sink.finish()?;
            self.is_final_decoded = true;
        }
        self.decoded_frames += chunk.frame_count;
        self.decoded_chunk_index += 1;
        if self.should_play {
            self.state = AudioState::Playing;
        }
        Ok(events)
    }

    fn fail(&mut self) {
        self.sink = None;
        self.state = AudioState::Error;
        self.error = Some("Local audio output failed".into());
    }

    // Identity guards and state reset keep stale commands away from newer playback.
    fn require_stream(&self, stream_id: Option<Uuid>) -> Result<(), String> {
        if stream_id.is_some()
            && self
                .cache
                .current()
                .map(|stream| stream.descriptor.stream_id)
                != stream_id
        {
            return Err("stale native audio stream command".into());
        }
        Ok(())
    }

    fn reset_output(&mut self) {
        self.sink = None;
        self.position_frame = 0;
        self.position_origin_frame = 0;
        self.decoded_chunk_index = 1;
        self.decoded_frames = 0;
        self.is_final_decoded = false;
        self.has_reported_end = false;
        self.error = None;
    }
}

enum Operation {
    Command(AudioCommand),
    Retain(EncodedChunk),
    SeekCached {
        stream_id: Uuid,
        position_frame: u64,
    },
}

struct Request {
    operation: Operation,
    response: oneshot::Sender<Result<(AudioSnapshot, bool), String>>,
}

struct Worker {
    requests: mpsc::Sender<Request>,
    snapshot: watch::Receiver<AudioSnapshot>,
    events: broadcast::Sender<AudioEvent>,
    should_keep_running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.should_keep_running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

#[derive(Clone)]
pub(crate) struct AudioPipeline {
    worker: Arc<Worker>,
}

impl AudioPipeline {
    // The owner thread is independent of runtime workers and UI/connection lifetimes.
    pub(crate) fn new(
        cache_bytes: usize,
        volume_percent: u8,
        is_muted: bool,
    ) -> Result<Self, String> {
        Self::start::<AudioOutput>(cache_bytes, volume_percent, is_muted)
    }

    fn start<Sink: OutputSink + 'static>(
        cache_bytes: usize,
        volume_percent: u8,
        is_muted: bool,
    ) -> Result<Self, String> {
        let initial = PipelineState::<Sink>::new(cache_bytes, volume_percent, is_muted)?.snapshot();
        let (snapshots, snapshot) = watch::channel(initial);
        let (requests, mut receiver) = mpsc::channel::<Request>(4);
        let (events, _) = broadcast::channel(64);
        let event_sender = events.clone();
        let should_keep_running = Arc::new(AtomicBool::new(true));
        let running = should_keep_running.clone();
        let thread = std::thread::Builder::new()
            .name("audio-pipeline".into())
            .spawn(move || {
                let mut output = PipelineState::<Sink>::new(cache_bytes, volume_percent, is_muted)
                    .expect("validated native audio output configuration");
                while running.load(Ordering::Acquire) {
                    for _ in 0..4 {
                        let request = match receiver.try_recv() {
                            Ok(request) => request,
                            Err(mpsc::error::TryRecvError::Empty) => break,
                            Err(mpsc::error::TryRecvError::Disconnected) => return,
                        };
                        let previous_event_id = output.event_counter;
                        let result = match request.operation {
                            Operation::Command(command) => output.execute(command).map(|()| true),
                            Operation::Retain(chunk) => output.retain(chunk).map(|()| true),
                            Operation::SeekCached { stream_id, position_frame } => output.seek_cached(stream_id, position_frame),
                        }
                        .map(|has_applied| (output.snapshot(), has_applied));
                        snapshots.send_replace(output.snapshot());
                        for (_, event) in output.audible_events.iter().filter(|(id, _)| *id > previous_event_id) {
                            let _ = event_sender.send(event.clone());
                        }
                        let _ = request.response.send(result);
                    }
                    match output.pump() {
                        Ok(events) => {
                            for event in events {
                                output.event_counter += 1;
                                if output.audible_events.len() == 32 {
                                    output.audible_events.pop_front();
                                }
                                output
                                    .audible_events
                                    .push_back((output.event_counter, event.clone()));
                                let _ = event_sender.send(event);
                            }
                        }
                        Err(_) => output.fail(),
                    }
                    if let Some(statistics) = output.take_buffer_statistics(Instant::now()) {
                        LOGGER.debug(format!(
                            "Buffer: {:.1} / {:.1} KiB occupied, {} chunks cached, {:.2} s remaining ({:.2} s next)",
                            statistics.retained_bytes as f64 / 1024.0,
                            statistics.byte_capacity as f64 / 1024.0,
                            statistics.cached_chunks,
                            statistics.current_remaining_millis as f64 / 1000.0,
                            statistics.prepared_remaining_millis as f64 / 1000.0,
                        ));
                    }
                    let snapshot = output.snapshot();
                    if *snapshots.borrow() != snapshot {
                        snapshots.send_replace(snapshot);
                    }
                    std::thread::park_timeout(Duration::from_millis(10));
                }
            })
            .map_err(|_| "create native audio audio worker failed")?;
        Ok(Self {
            worker: Arc::new(Worker {
                requests,
                snapshot,
                events,
                should_keep_running,
                thread: Some(thread),
            }),
        })
    }

    // Network tasks wait for the native acceptance boundary before acknowledging.
    pub(crate) async fn execute(&self, command: AudioCommand) -> Result<AudioSnapshot, String> {
        self.submit(Operation::Command(command))
            .await
            .map(|(snapshot, _)| snapshot)
    }
    pub(crate) async fn retain(&self, chunk: EncodedChunk) -> Result<AudioSnapshot, String> {
        self.submit(Operation::Retain(chunk))
            .await
            .map(|(snapshot, _)| snapshot)
    }
    pub(crate) async fn seek_cached(
        &self,
        stream_id: Uuid,
        position_frame: u64,
    ) -> Result<bool, String> {
        self.submit(Operation::SeekCached {
            stream_id,
            position_frame,
        })
        .await
        .map(|(_, has_applied)| has_applied)
    }
    // Read-only state and audible-transition events.
    pub(crate) fn snapshot(&self) -> AudioSnapshot {
        self.worker.snapshot.borrow().clone()
    }
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<AudioEvent> {
        self.worker.events.subscribe()
    }

    async fn submit(&self, operation: Operation) -> Result<(AudioSnapshot, bool), String> {
        let (response, receiver) = oneshot::channel();
        self.worker
            .requests
            .send(Request {
                operation,
                response,
            })
            .await
            .map_err(|_| "native audio audio worker stopped")?;
        if let Some(thread) = &self.worker.thread {
            thread.thread().unpark();
        }
        receiver
            .await
            .map_err(|_| "native audio audio worker stopped")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::models::test_stream;
    use std::cell::Cell;

    struct FakeSink {
        position: Cell<u64>,
        is_drained: Cell<bool>,
    }
    impl OutputSink for FakeSink {
        fn new(_: &AudioFormat, _: u8, _: bool) -> Result<Self, String> {
            Ok(Self {
                position: Cell::new(0),
                is_drained: Cell::new(false),
            })
        }
        fn transport_play(&self) -> Result<(), String> {
            Ok(())
        }
        fn transport_pause(&self) -> Result<(), String> {
            Ok(())
        }
        fn enqueue(&self, _: Arc<[i32]>) -> Result<(), String> {
            Ok(())
        }
        fn finish(&self) -> Result<(), String> {
            Ok(())
        }
        fn position_frame(&self) -> u64 {
            self.position.get()
        }
        fn is_audibly_drained(&self) -> bool {
            self.is_drained.get()
        }
        fn buffered_duration_millis(&self) -> u64 {
            0
        }
        fn set_volume(&self, _: u8) {}
        fn set_mute(&self, _: bool) {}
    }

    fn chunk(stream_id: Uuid) -> EncodedChunk {
        EncodedChunk {
            stream_id,
            index: 1,
            first_frame: 0,
            frame_count: 480,
            is_final: true,
            encoded_bytes: Arc::from([1u8]),
            payload_offset: 0,
        }
    }

    #[test]
    fn cached_seek_preserves_receipt_and_successor_and_rebases_output_clock() {
        for autoplay in [true, false] {
            let mut output = PipelineState::<FakeSink>::new(1024, 30, false).unwrap();
            let current = test_stream();
            let next = test_stream();
            output
                .execute(AudioCommand::SetMedia {
                    stream: current.clone(),
                    autoplay,
                })
                .unwrap();
            output
                .execute(AudioCommand::PrepareNext {
                    current_stream_id: current.stream_id,
                    stream: next.clone(),
                    next_revision: 1,
                })
                .unwrap();
            for index in 1..=3 {
                let mut cached = chunk(current.stream_id);
                cached.index = index;
                cached.first_frame = (index - 1) * 48_000;
                cached.frame_count = 48_000;
                cached.is_final = index == 3;
                output.retain(cached).unwrap();
            }
            output.retain(chunk(next.stream_id)).unwrap();
            let current_bytes = output
                .cache
                .current()
                .unwrap()
                .chunk(2)
                .unwrap()
                .encoded_bytes
                .clone();
            let prepared_bytes = output
                .cache
                .prepared()
                .unwrap()
                .chunk(1)
                .unwrap()
                .encoded_bytes
                .clone();
            let before = output.snapshot();
            assert!(!output.seek_cached(current.stream_id, 144_000).unwrap());
            assert_eq!(output.snapshot(), before);
            assert!(output.seek_cached(Uuid::new_v4(), 48_000).is_err());
            assert_eq!(output.snapshot(), before);
            assert!(output.seek_cached(current.stream_id, 48_001).is_err());
            assert_eq!(output.snapshot(), before);
            assert!(output.seek_cached(current.stream_id, 48_000).unwrap());
            let after = output.snapshot();
            assert_eq!(after.status.position_frame, 48_000);
            assert_eq!(
                after.status.output_revision,
                before.status.output_revision + 1
            );
            assert_eq!(
                after.status.received_streams,
                before.status.received_streams
            );
            assert_eq!(after.prepared, before.prepared);
            assert!(Arc::ptr_eq(
                &current_bytes,
                &output
                    .cache
                    .current()
                    .unwrap()
                    .chunk(2)
                    .unwrap()
                    .encoded_bytes
            ));
            assert!(Arc::ptr_eq(
                &prepared_bytes,
                &output
                    .cache
                    .prepared()
                    .unwrap()
                    .chunk(1)
                    .unwrap()
                    .encoded_bytes
            ));
            assert_eq!(
                after.status.output_state,
                if autoplay {
                    AudioState::Buffering
                } else {
                    AudioState::Paused
                }
            );
            output
                .pump_with(|cached, _| {
                    assert_eq!(cached.index, 2);
                    Ok(Arc::from([0i32]))
                })
                .unwrap();
            output.sink.as_ref().unwrap().position.set(4800);
            output.pump_with(|_, _| Ok(Arc::from([0i32]))).unwrap();
            assert_eq!(output.position_frame, 52_800);
            assert!(output.seek_cached(current.stream_id, 0).unwrap());
            assert_eq!(
                output.snapshot().status.received_streams,
                before.status.received_streams
            );
        }
    }

    #[test]
    fn history_eviction_precedes_ack_snapshot_and_preserves_unplayed_a_and_b() {
        let mut output = PipelineState::<FakeSink>::new(1000, 30, false).unwrap();
        let current = test_stream();
        let next = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: current.clone(),
                autoplay: false,
            })
            .unwrap();
        output
            .execute(AudioCommand::PrepareNext {
                current_stream_id: current.stream_id,
                stream: next.clone(),
                next_revision: 1,
            })
            .unwrap();
        for index in 1..=3 {
            let mut cached = chunk(current.stream_id);
            cached.index = index;
            cached.first_frame = (index - 1) * 48000;
            cached.frame_count = 48000;
            cached.is_final = false;
            cached.encoded_bytes = vec![0; if index == 3 { 600 } else { 100 }].into();
            output.retain(cached).unwrap();
        }
        output.position_frame = 96000;
        for index in 1..=4 {
            let mut cached = chunk(next.stream_id);
            cached.index = index;
            cached.first_frame = (index - 1) * 48000;
            cached.frame_count = 48000;
            cached.is_final = false;
            cached.encoded_bytes = vec![0; 100].into();
            output.retain(cached).unwrap();
            let ack = output.snapshot();
            assert_eq!(ack.status.received_streams[0].next_chunk_index, 4);
            assert_eq!(ack.status.received_streams[1].next_chunk_index, index + 1);
            assert_eq!(
                ack.buffer_capacity,
                if index <= 2 {
                    200
                } else {
                    (4 - index) as usize * 100
                }
            );
            assert!(output.cache.current().unwrap().chunk(3).is_some());
            assert!(output.cache.prepared().unwrap().chunk(1).is_some());
        }
        assert!(!output.seek_cached(current.stream_id, 0).unwrap());
    }

    #[test]
    fn playback_reclaims_history_under_pressure_without_waiting_for_another_chunk() {
        let mut output = PipelineState::<FakeSink>::new(1000, 30, false).unwrap();
        let stream = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: stream.clone(),
                autoplay: true,
            })
            .unwrap();
        for index in 1..=3 {
            let mut cached = chunk(stream.stream_id);
            cached.index = index;
            cached.first_frame = (index - 1) * 48000;
            cached.frame_count = 48000;
            cached.is_final = false;
            cached.encoded_bytes = vec![0; 300].into();
            output.retain(cached).unwrap();
        }
        output.pump_with(|_, _| Ok(Arc::from([0i32]))).unwrap();
        assert_eq!(output.snapshot().buffer_capacity, 100);
        output.sink.as_ref().unwrap().position.set(48000);
        output.pump_with(|_, _| Ok(Arc::from([0i32]))).unwrap();
        assert_eq!(output.snapshot().buffer_capacity, 400);
        assert!(output.cache.current().unwrap().chunk(1).is_none());
        assert!(output.cache.current().unwrap().chunk(2).is_some());
        assert_eq!(
            output.snapshot().status.received_streams[0].next_chunk_index,
            4
        );
    }

    #[test]
    fn cached_seek_supports_retained_backwards_targets_and_short_eof_chunks() {
        let mut output = PipelineState::<FakeSink>::new(1024, 30, false).unwrap();
        let stream = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: stream.clone(),
                autoplay: false,
            })
            .unwrap();
        let mut first = chunk(stream.stream_id);
        first.frame_count = 48_000;
        first.is_final = false;
        output.retain(first).unwrap();
        let mut last = chunk(stream.stream_id);
        last.index = 2;
        last.first_frame = 48_000;
        output.retain(last).unwrap();
        let cursors = output.snapshot().status.received_streams;
        assert!(output.seek_cached(stream.stream_id, 48_000).unwrap());
        assert!(output.seek_cached(stream.stream_id, 0).unwrap());
        assert!(output.seek_cached(stream.stream_id, 48_000).unwrap());
        output
            .pump_with(|cached, _| {
                assert!(cached.is_final);
                assert_eq!(cached.frame_count, 480);
                Ok(Arc::from([0i32]))
            })
            .unwrap();
        assert_eq!(output.decoded_frames, 48_480);
        assert_eq!(output.snapshot().status.received_streams, cursors);
        assert_eq!(output.state, AudioState::Paused);
        assert!(output.audible_events.is_empty());
        let before = output.snapshot();
        assert!(!output.seek_cached(stream.stream_id, 96_000).unwrap());
        assert_eq!(output.snapshot(), before);
        output.revision = u64::MAX;
        let before = output.snapshot();
        assert!(output.seek_cached(stream.stream_id, 48_000).is_err());
        assert_eq!(output.snapshot(), before);
        output.revision = 0;
        output
            .execute(AudioCommand::Stop { stream_id: None })
            .unwrap();
        assert!(output.seek_cached(stream.stream_id, 0).is_err());
    }

    #[test]
    fn manual_activation_preserves_164_seconds_and_is_idempotent() {
        let mut output = PipelineState::<FakeSink>::new(1_048_576, 30, false).unwrap();
        let current = test_stream();
        let next = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: current.clone(),
                autoplay: true,
            })
            .unwrap();
        output
            .execute(AudioCommand::PrepareNext {
                current_stream_id: current.stream_id,
                stream: next.clone(),
                next_revision: 1,
            })
            .unwrap();
        let mut unfinished = chunk(current.stream_id);
        unfinished.is_final = false;
        unfinished.frame_count = 48_000;
        output.retain(unfinished).unwrap();
        for index in 0..164 {
            let mut cached = chunk(next.stream_id);
            cached.index = index + 1;
            cached.first_frame = index * 48_000;
            cached.frame_count = 48_000;
            cached.is_final = index == 163;
            output.retain(cached).unwrap();
        }
        assert_eq!(
            output.cache.statistics(0).prepared_remaining_millis,
            164_000
        );
        let bytes = output
            .cache
            .prepared()
            .unwrap()
            .chunk(1)
            .unwrap()
            .encoded_bytes
            .clone();
        let stale = AudioCommand::ActivatePrepared {
            current_stream_id: current.stream_id,
            stream_id: next.stream_id,
            next_revision: 2,
        };
        let before = output.snapshot();
        assert!(output.execute(stale).is_err());
        assert_eq!(output.snapshot(), before);
        let activate = AudioCommand::ActivatePrepared {
            current_stream_id: current.stream_id,
            stream_id: next.stream_id,
            next_revision: 1,
        };
        output.execute(activate.clone()).unwrap();
        assert_eq!(output.cache.statistics(0).current_remaining_millis, 164_000);
        assert_eq!(
            output.snapshot().status.received_streams[0].next_chunk_index,
            165
        );
        assert!(Arc::ptr_eq(
            &bytes,
            &output
                .cache
                .current()
                .unwrap()
                .chunk(1)
                .unwrap()
                .encoded_bytes
        ));
        assert!(output.snapshot().prepared.is_none());
        assert!(matches!(
            output.audible_events[0].1,
            AudioEvent::PreparedActivated { .. }
        ));
        assert_eq!(output.audible_events.len(), 1);
        output.position_frame = 4800;
        output.execute(activate).unwrap();
        assert_eq!(output.position_frame, 4800);
        assert_eq!(output.audible_events.len(), 1);
        output
            .execute(AudioCommand::Stop { stream_id: None })
            .unwrap();
        assert!(output.snapshot().activation.is_none());
    }

    #[test]
    fn buffer_summary_is_rate_limited_and_only_reports_configured_audio() {
        let mut output = PipelineState::<FakeSink>::new(1_048_576, 30, false).unwrap();
        let start = output.last_buffer_summary;
        assert!(
            output
                .take_buffer_statistics(start + BUFFER_SUMMARY_INTERVAL)
                .is_none()
        );
        let current = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: current.clone(),
                autoplay: false,
            })
            .unwrap();
        output.retain(chunk(current.stream_id)).unwrap();
        assert!(
            output
                .take_buffer_statistics(start + Duration::from_secs(4))
                .is_none()
        );
        let summary = output
            .take_buffer_statistics(start + BUFFER_SUMMARY_INTERVAL)
            .unwrap();
        assert_eq!(summary.cached_chunks, 1);
        assert_eq!(summary.retained_bytes, 1);
        assert_eq!(summary.current_remaining_millis, 10);
        assert!(
            output
                .take_buffer_statistics(start + Duration::from_secs(9))
                .is_none()
        );
        assert!(
            output
                .take_buffer_statistics(start + Duration::from_secs(10))
                .is_some()
        );
        output
            .execute(AudioCommand::Stop { stream_id: None })
            .unwrap();
        assert!(
            output
                .take_buffer_statistics(start + Duration::from_secs(15))
                .is_none()
        );
    }

    #[test]
    fn transport_eof_does_not_promote_before_audible_end() {
        let mut output = PipelineState::<FakeSink>::new(1_048_576, 30, false).unwrap();
        let current = test_stream();
        let next = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: current.clone(),
                autoplay: true,
            })
            .unwrap();
        output
            .execute(AudioCommand::PrepareNext {
                current_stream_id: current.stream_id,
                stream: next.clone(),
                next_revision: 1,
            })
            .unwrap();
        output.retain(chunk(current.stream_id)).unwrap();
        output.retain(chunk(next.stream_id)).unwrap();
        assert!(
            output
                .pump_with(|_, _| Ok(Arc::from([])))
                .unwrap()
                .is_empty()
        );
        assert!(output.pump().unwrap().is_empty());
        assert_eq!(
            output.snapshot().status.current_stream_id,
            Some(current.stream_id)
        );
        output.sink.as_ref().unwrap().position.set(479);
        assert!(output.pump().unwrap().is_empty());
        output.sink.as_ref().unwrap().is_drained.set(true);
        let events = output.pump().unwrap();
        assert_eq!(
            events[0],
            AudioEvent::TrackEnded {
                stream_id: current.stream_id
            }
        );
        assert!(
            matches!(events[1], AudioEvent::TrackStarted { stream_id, next_revision: 1, .. } if stream_id == next.stream_id)
        );
        assert_eq!(
            output.snapshot().status.current_stream_id,
            Some(next.stream_id)
        );
        assert_eq!(
            output.snapshot().status.received_streams[0].next_chunk_index,
            2
        );
    }

    #[test]
    fn rejected_commands_do_not_replace_playback_or_volume() {
        let mut output = PipelineState::<FakeSink>::new(1_048_576, 30, false).unwrap();
        let stream = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: stream.clone(),
                autoplay: true,
            })
            .unwrap();
        let before = output.snapshot();
        assert!(
            output
                .execute(AudioCommand::Pause {
                    stream_id: Some(Uuid::new_v4())
                })
                .is_err()
        );
        assert!(
            output
                .execute(AudioCommand::SetVolume {
                    volume_percent: 101
                })
                .is_err()
        );
        let mut invalid = test_stream();
        invalid.output_format.channels = 0;
        assert!(
            output
                .execute(AudioCommand::SetMedia {
                    stream: invalid,
                    autoplay: true
                })
                .is_err()
        );
        assert_eq!(before, output.snapshot());
    }

    #[test]
    fn pause_then_play_of_decoded_final_chunk_is_playing_not_buffering() {
        let mut output = PipelineState::<FakeSink>::new(1_048_576, 30, false).unwrap();
        let stream = test_stream();
        output
            .execute(AudioCommand::SetMedia {
                stream: stream.clone(),
                autoplay: true,
            })
            .unwrap();
        output.retain(chunk(stream.stream_id)).unwrap();
        output.pump_with(|_, _| Ok(Arc::from([]))).unwrap();
        output
            .execute(AudioCommand::Pause {
                stream_id: Some(stream.stream_id),
            })
            .unwrap();
        output
            .execute(AudioCommand::Play {
                stream_id: Some(stream.stream_id),
            })
            .unwrap();
        output.pump().unwrap();
        assert_eq!(output.snapshot().status.output_state, AudioState::Playing);
    }

    #[tokio::test]
    async fn worker_receipts_are_bounded_and_shutdown_has_no_runtime_dependency() {
        let output = AudioPipeline::new(1_048_576, 30, false).unwrap();
        let snapshot = output
            .execute(AudioCommand::SetMute { is_muted: true })
            .await
            .unwrap();
        assert!(snapshot.status.is_muted);
        assert_eq!(snapshot.status.output_revision, 1);
        assert!(output.retain(chunk(Uuid::new_v4())).await.is_err());
        assert!(output.snapshot().status.received_streams.is_empty());
    }
}
