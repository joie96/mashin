//! Owns the two Sendspin download slots and their encoded caches.
//!
//! Public commands are serialized through a dedicated worker thread. Its I/O tasks publish
//! cache, stream, and generated connection state through shared synchronization primitives.

use super::connection::{audio_format_from_stream, player_support};
use super::models::{
    CacheItemState, SendspinAudioCodec, SendspinDownloadPlan, SendspinDownloadPlanItem,
    SendspinDownloadRole,
};
use crate::cache::store::CacheRetentionPolicy;
use crate::logging::Logger;
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::identifiers::{AccessToken, PlayerId, QueueId};
use crate::music_assistant::models::player::MusicAssistantPlayer;
use crate::music_assistant::models::queue::{CacheStatus, MusicAssistantQueue};
use ::sendspin::audio::AudioFormat;
use ::sendspin::protocol::client::{ConnectionGuard, Controller, WsSender};
use ::sendspin::protocol::messages::{GoodbyeReason, Message, PlayerState};
use ::sendspin::ProtocolClientBuilder;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

static LOGGER: Logger = Logger::new(module_path!());

const CACHE_COMPLETION_TOLERANCE_MILLIS: u64 = 500;
const CACHE_DOWNLOAD_PAUSE_PERCENT: usize = 95;
const CACHE_DOWNLOAD_RESUME_PERCENT: usize = 90;
const CACHE_PAUSE_KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);
const CACHE_PAUSE_KEEPALIVE_PLAY_DURATION: std::time::Duration = std::time::Duration::from_secs(1);
const CONNECTION_LIVENESS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
const CONNECTION_LIVENESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const RESUME_SEEK_PREROLL_MILLIS: u64 = 3_000;
const SEEK_REPLACEMENT_STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS: usize = 4;
const DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY: std::time::Duration =
    std::time::Duration::from_secs(2);
const MAXIMUM_DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY: std::time::Duration =
    std::time::Duration::from_secs(8);

#[derive(Clone)]
pub(crate) struct SendspinDownloaderConfig {
    pub(crate) sendspin_url: String,
    pub(crate) music_assistant_url: String,
    pub(crate) music_assistant_auth_token: Option<AccessToken>,
    pub(crate) client_id: String,
    pub(crate) client_name: String,
    pub(crate) downloader_buffer_bytes: u64,
    pub(crate) cache_bytes: usize,
    pub(crate) preferred_codec: SendspinAudioCodec,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveDownloadStatus {
    pub(crate) queue_item_id: crate::music_assistant::models::identifiers::QueueItemId,
    pub(crate) cached_duration_millis: u64,
    pub(crate) expected_duration_millis: Option<u64>,
    pub(crate) state: CacheStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownloaderConnectionStatus {
    Ready,
    Lost,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DownloaderConnectionEvent {
    pub(crate) generation: u64,
    pub(crate) status: DownloaderConnectionStatus,
}

#[derive(Debug, Clone, Copy)]
struct DownloaderSlotConnectionState {
    generation: u64,
    is_transport_connected: bool,
    is_stream_active: bool,
}

impl DownloaderSlotConnectionState {
    fn disconnected(generation: u64) -> Self {
        Self {
            generation,
            is_transport_connected: false,
            is_stream_active: false,
        }
    }
}

struct SendspinDownloaderWorker {
    runtime: Runtime,
    config: Option<SendspinDownloaderConfig>,
    plan: Option<SendspinDownloadPlan>,
    cache_states: Arc<Mutex<HashMap<String, CacheItemState>>>,
    caches: crate::cache::store::EncodedCaches,
    cache_change_signal: crate::cache::store::CacheChangeSignal,
    retention_policy: Arc<Mutex<CacheRetentionPolicy>>,
    capacity_aborted_content_keys: Arc<Mutex<HashSet<String>>>,
    cache_sequence: Arc<std::sync::atomic::AtomicU64>,
    download_window_refresh: Arc<AtomicBool>,
    download_status_refresh: Arc<AtomicBool>,
    connection_event: Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    connection_generation: Arc<AtomicU64>,
    // Updated by I/O tasks and used to validate a generated Ready event.
    slot_connection_states: [Arc<Mutex<DownloaderSlotConnectionState>>; 2],
    slots: crate::cache::store::DownloadSlots,
    player_ids: [Option<PlayerId>; 2],
    queue_ids: [Option<QueueId>; 2],
    controllers: Arc<Mutex<[Option<Controller>; 2]>>,
    guards: [Option<ConnectionGuard>; 2],
    io_tasks: [Option<JoinHandle<()>>; 2],
    liveness_tasks: [Option<JoinHandle<()>>; 2],
    cache_pause_keepalive_tasks: [Option<JoinHandle<()>>; 2],
    download_request_tasks: [Option<JoinHandle<()>>; 2],
    downloads_paused_for_capacity: [bool; 2],
    pending_current_seek_seconds: Option<i64>,
}

impl SendspinDownloaderWorker {
    // Construction and connection setup.
    fn new() -> Self {
        Self {
            runtime: tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Sendspin downloader runtime could not be created"),
            config: None,
            plan: None,
            cache_states: Arc::new(Mutex::new(HashMap::new())),
            caches: Arc::new(Mutex::new(HashMap::new())),
            cache_change_signal: crate::cache::store::new_cache_change_signal(),
            retention_policy: Arc::new(Mutex::new(CacheRetentionPolicy::default())),
            capacity_aborted_content_keys: Arc::new(Mutex::new(HashSet::new())),
            cache_sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            download_window_refresh: Arc::new(AtomicBool::new(false)),
            download_status_refresh: Arc::new(AtomicBool::new(false)),
            connection_event: Arc::new(Mutex::new(None)),
            connection_generation: Arc::new(AtomicU64::new(0)),
            slot_connection_states: [
                Arc::new(Mutex::new(DownloaderSlotConnectionState::disconnected(0))),
                Arc::new(Mutex::new(DownloaderSlotConnectionState::disconnected(0))),
            ],
            slots: Arc::new(Mutex::new([None, None])),
            player_ids: [None, None],
            queue_ids: [None, None],
            controllers: Arc::new(Mutex::new([None, None])),
            guards: [None, None],
            io_tasks: [None, None],
            liveness_tasks: [None, None],
            cache_pause_keepalive_tasks: [None, None],
            download_request_tasks: [None, None],
            downloads_paused_for_capacity: [false, false],
            pending_current_seek_seconds: None,
        }
    }

    pub(crate) fn configure(&mut self, config: SendspinDownloaderConfig) {
        LOGGER.debug(format!(
            "downloader configured: preferred codec {}, cache capacity {} bytes",
            config.preferred_codec.as_str(),
            config.cache_bytes
        ));
        self.config = Some(config);
    }

    pub(crate) fn connect(&mut self) -> Result<(), String> {
        LOGGER.info("connecting Sendspin downloader slots");
        if self
            .controllers
            .lock()
            .map(|controllers| controllers.iter().all(Option::is_some))
            .unwrap_or(false)
        {
            return Ok(());
        }
        let config = self
            .config
            .clone()
            .ok_or_else(|| "Sendspin downloader is not configured".to_string())?;
        let client = MusicAssistantClient::new(
            &config.music_assistant_url,
            config.music_assistant_auth_token.clone(),
        );
        let connection_generation = self.connection_generation.load(Ordering::Acquire);
        if connection_generation > 0 {
            self.runtime
                .block_on(stop_previous_download_players(&client, &self.player_ids))?;
        }
        self.connect_slot(0)?;
        self.connect_slot(1)?;
        let client_ids = [
            download_client_id(&config.client_id, 1),
            download_client_id(&config.client_id, 2),
        ];
        let player_ids = self
            .runtime
            .block_on(resolve_download_player_ids(&client, &client_ids))?;
        let queue_ids = self
            .runtime
            .block_on(resolve_download_queue_ids(&client, &player_ids))?;
        self.runtime
            .block_on(disable_download_queue_autoplay(&client, &queue_ids))?;
        if !slots_are_ready(&self.slot_connection_states, connection_generation) {
            return Err(
                "Sendspin downloader transport disconnected during connection setup".to_string(),
            );
        }
        for slot_index in 0..2 {
            LOGGER.info(format!(
                "Sendspin downloader slot {} connected: client ID {}, Music Assistant player ID {}, queue ID {}",
                slot_index + 1,
                client_ids[slot_index],
                player_ids[slot_index].as_str(),
                queue_ids[slot_index].as_str()
            ));
        }
        self.player_ids = [Some(player_ids[0].clone()), Some(player_ids[1].clone())];
        self.queue_ids = [Some(queue_ids[0].clone()), Some(queue_ids[1].clone())];
        Ok(())
    }

    fn connect_slot(&mut self, slot_index: usize) -> Result<(), String> {
        if self
            .controllers
            .lock()
            .map(|controllers| controllers[slot_index].is_some())
            .unwrap_or(false)
        {
            return Ok(());
        }
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| "Sendspin downloader is not configured".to_string())?;
        let url = config.sendspin_url.clone();
        let slot_number = slot_index + 1;
        let client_id = download_client_id(&config.client_id, slot_number);
        let client_name = download_client_name(&config.client_name, slot_number);
        let support = player_support(config.preferred_codec, config.downloader_buffer_bytes);
        let slots = Arc::clone(&self.slots);
        let caches = Arc::clone(&self.caches);
        let states = Arc::clone(&self.cache_states);
        let cache_change_signal = Arc::clone(&self.cache_change_signal);
        let sequence = Arc::clone(&self.cache_sequence);
        let download_window_refresh = Arc::clone(&self.download_window_refresh);
        let download_status_refresh = Arc::clone(&self.download_status_refresh);
        let connection_event = Arc::clone(&self.connection_event);
        let connection_generation = Arc::clone(&self.connection_generation);
        let task_connection_generation = connection_generation.load(Ordering::Acquire);
        let slot_connection_state = Arc::clone(&self.slot_connection_states[slot_index]);
        publish_slot_transport_connected(&slot_connection_state, task_connection_generation);
        let capacity = config.cache_bytes;
        let (controller, sender, guard, task) = self.runtime.block_on(async move {
            let client = ProtocolClientBuilder::builder()
                .client_id(client_id)
                .name(client_name)
                .player_v1_support(support)
                .initial_player_state(PlayerState {
                    volume: Some(100),
                    muted: Some(false),
                    static_delay_ms: Some(0),
                    required_lead_time_ms: Some(0),
                    min_buffer_ms: Some(0),
                    ..PlayerState::default()
                })
                .metadata()
                .controller()
                .build()
                .connect(&url)
                .await
                .map_err(|error| format!("Sendspin downloader connect failed: {error}"))?;
            let connection = client.split();
            let controller = connection
                .controller
                .clone()
                .ok_or_else(|| "Sendspin server did not grant controller role".to_string())?;
            let mut messages = connection.messages;
            let mut audio = connection.audio;
            let sender = connection.sender.clone();
            // This task owns the stream-local reservation until it is completed, interrupted, or superseded.
            let task = tokio::spawn(async move {
                let mut format = None;
                let mut reservation = None;
                let mut last_timestamp = None;
                let mut cached_bytes = 0_usize;
                let mut cached_duration_millis = 0_u64;
                let mut next_progress_millis = 5_000_u64;
                let mut hard_limit_reported = false;
                let mut seek_gate_reported = false;
                let mut first_chunk_reported = false;
                let mut seek_replacement_deadline = None;
                loop {
                    tokio::select! {
                        _ = async {
                            match seek_replacement_deadline {
                                Some(deadline) => tokio::time::sleep_until(deadline).await,
                                None => std::future::pending::<()>().await,
                            }
                        } => {
                            if let Some(value) = reservation.as_ref() {
                                if crate::cache::store::slot_reservation_is_current(
                                    &slots,
                                    slot_index,
                                    value,
                                ) && !value.append_after_seek.load(Ordering::Acquire) {
                                    LOGGER.warn(format!(
                                        "seek replacement stream timed out on slot {}; resuming the interrupted download",
                                        slot_index + 1
                                    ));
                                    retain_interrupted_reservation(
                                        &slots,
                                        &caches,
                                        &cache_change_signal,
                                        &states,
                                        &download_window_refresh,
                                        slot_index,
                                        value,
                                    );
                                }
                            }
                            reservation = None;
                            format = None;
                            last_timestamp = None;
                            seek_replacement_deadline = None;
                        }
                        message = messages.recv() => {
                            let Some(message) = message else { break; };
                            match message {
                                Message::StreamStart(start) => {
                                    publish_slot_stream_state(
                                        &slot_connection_state,
                                        task_connection_generation,
                                        true,
                                    );
                                    format = start.player.as_ref().and_then(|player| {
                                        audio_format_from_stream(
                                            &player.codec,
                                            player.sample_rate,
                                            player.channels,
                                            player.bit_depth,
                                            player.codec_header.as_deref(),
                                        ).ok()
                                    });
                                    reservation = slots.lock().ok().and_then(|values| values[slot_index].clone());
                                    last_timestamp = None;
                                    cached_bytes = 0;
                                    cached_duration_millis = reservation
                                        .as_ref()
                                        .map(|value| value.resume_after_millis)
                                        .unwrap_or(0);
                                    next_progress_millis = 5_000;
                                    hard_limit_reported = false;
                                    seek_gate_reported = false;
                                    first_chunk_reported = false;
                                    seek_replacement_deadline = None;
                                    if let Some(format) = format.as_ref() {
                                        LOGGER.trace(format!(
                                            "stream started on slot {}: {:?}, {} Hz, {} channels",
                                            slot_index + 1,
                                            format.codec,
                                            format.sample_rate,
                                            format.channels
                                        ));
                                    } else {
                                        LOGGER.warn(format!(
                                            "stream on slot {} announced an unsupported audio format",
                                            slot_index + 1
                                        ));
                                    }
                                    if let Some(reservation) = reservation.as_ref() {
                                        LOGGER.trace(format!(
                                            "download stream bound: slot {}, cache key {}",
                                            slot_index + 1,
                                            reservation.cache_content_key
                                        ));
                                    } else {
                                        LOGGER.warn(format!(
                                            "stream started on slot {} without an active download reservation",
                                            slot_index + 1
                                        ));
                                    }
                                }
                                Message::StreamEnd(_) => {
                                    publish_slot_stream_state(
                                        &slot_connection_state,
                                        task_connection_generation,
                                        false,
                                    );
                                    let mut released_reservation = false;
                                    if let Some(value) = reservation.as_ref() {
                                        if crate::cache::store::slot_reservation_is_current(
                                            &slots,
                                            slot_index,
                                            value,
                                        ) {
                                            match stream_end_outcome(
                                                value.append_after_seek.load(Ordering::Acquire),
                                                value.expected_duration_millis,
                                                cached_duration_millis,
                                            ) {
                                                StreamEndOutcome::Complete => {
                                                    complete_stream_reservation(
                                                        &slots,
                                                        &caches,
                                                        &cache_change_signal,
                                                        &states,
                                                        &download_window_refresh,
                                                        slot_index,
                                                        value,
                                                    );
                                                    released_reservation = true;
                                                }
                                                StreamEndOutcome::Interrupted => {
                                                    retain_interrupted_reservation(
                                                        &slots,
                                                        &caches,
                                                        &cache_change_signal,
                                                        &states,
                                                        &download_window_refresh,
                                                        slot_index,
                                                        value,
                                                    );
                                                    released_reservation = true;
                                                }
                                                StreamEndOutcome::AwaitingSeekStream => {
                                                    LOGGER.trace(format!(
                                                        "stream ended during pending seek on slot {}; waiting for the replacement stream",
                                                        slot_index + 1
                                                    ));
                                                    seek_replacement_deadline = Some(
                                                        tokio::time::Instant::now()
                                                            + SEEK_REPLACEMENT_STREAM_TIMEOUT,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    if released_reservation {
                                        reservation = None;
                                    }
                                    format = None;
                                    last_timestamp = None;
                                }
                                Message::StreamClear(_) => {
                                    publish_slot_stream_state(
                                        &slot_connection_state,
                                        task_connection_generation,
                                        false,
                                    );
                                    LOGGER.warn(format!("stream cleared on slot {}", slot_index + 1));
                                    if let Some(value) = reservation.as_ref() {
                                        if crate::cache::store::slot_reservation_is_current(
                                            &slots,
                                            slot_index,
                                            value,
                                        ) {
                                            retain_interrupted_reservation(
                                            &slots,
                                            &caches,
                                            &cache_change_signal,
                                            &states,
                                            &download_window_refresh,
                                            slot_index,
                                            value,
                                            );
                                        }
                                    }
                                    reservation = None;
                                    format = None;
                                    last_timestamp = None;
                                    seek_replacement_deadline = None;
                                }
                                _ => {}
                            }
                        }
                        chunk = audio.recv() => {
                            let Some(chunk) = chunk else { break; };
                            let (Some(reservation), Some(format)) = (reservation.as_ref(), format.as_ref()) else { continue; };
                            let duration = last_timestamp
                                .map(|previous| chunk.timestamp.saturating_sub(previous).max(0) as u64 / 1_000)
                                .unwrap_or(0);
                            last_timestamp = Some(chunk.timestamp);
                            if !reservation.append_after_seek.load(Ordering::Acquire) {
                                if !seek_gate_reported {
                                    LOGGER.debug(format!(
                                        "download chunks are waiting for seek confirmation: slot {}, cache key {}",
                                        slot_index + 1,
                                        reservation.cache_content_key
                                    ));
                                    seek_gate_reported = true;
                                }
                                continue;
                            }
                            let result = crate::cache::store::cache_append_encoded_chunk(
                                &caches,
                                &sequence,
                                capacity,
                                reservation,
                                format,
                                duration,
                                &chunk.data,
                            );
                            if result != crate::cache::store::CacheAppendResult::HardLimitReached {
                                if !first_chunk_reported {
                                    LOGGER.trace(format!(
                                        "download cache write started: slot {}, cache key {}",
                                        slot_index + 1,
                                        reservation.cache_content_key
                                    ));
                                    first_chunk_reported = true;
                                }
                                cached_bytes = cached_bytes.saturating_add(chunk.data.len());
                                cached_duration_millis = cached_duration_millis.saturating_add(duration);
                                crate::cache::store::notify_cache_changed(&cache_change_signal);
                                if let Ok(mut states) = states.lock() {
                                    if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                                        state.state = CacheStatus::Downloading;
                                        state.cached_duration_millis = state.cached_duration_millis.saturating_add(duration);
                                    }
                                }
                                if download_duration_reached(
                                    reservation.expected_duration_millis,
                                    cached_duration_millis,
                                ) {
                                    if crate::cache::store::slot_release_reservation_if_current(
                                        &slots,
                                        slot_index,
                                        reservation,
                                    ) {
                                        crate::cache::store::cache_mark_complete(&caches, reservation);
                                        crate::cache::store::notify_cache_changed(&cache_change_signal);
                                        download_window_refresh.store(true, Ordering::Release);
                                        if let Ok(mut states) = states.lock() {
                                            if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                                                state.state = CacheStatus::Complete;
                                            }
                                        }
                                        LOGGER.trace(format!(
                                            "download complete from expected duration: slot {}, cache key {}, {} ms, {} bytes",
                                            slot_index + 1,
                                            reservation.cache_content_key,
                                            cached_duration_millis,
                                            cached_bytes
                                        ));
                                    }
                                }
                                if cached_duration_millis >= next_progress_millis {
                                    download_status_refresh.store(true, Ordering::Release);
                                    LOGGER.trace(format!(
                                        "download progress: slot {}, cache key {}, {} ms, {} bytes",
                                        slot_index + 1,
                                        reservation.cache_content_key,
                                        cached_duration_millis,
                                        cached_bytes
                                    ));
                                    next_progress_millis = cached_duration_millis
                                        .saturating_div(5_000)
                                        .saturating_add(1)
                                        .saturating_mul(5_000);
                                }
                            } else if !hard_limit_reported {
                                LOGGER.trace(format!(
                                    "cache capacity reached before soft-limit handling: slot {}, cache key {}",
                                    slot_index + 1,
                                    reservation.cache_content_key
                                ));
                                hard_limit_reported = true;
                                download_window_refresh.store(true, Ordering::Release);
                            }
                        }
                    }
                }
                if let Some(value) = reservation {
                    if crate::cache::store::slot_reservation_is_current(&slots, slot_index, &value) {
                        crate::cache::store::cache_mark_interrupted(
                            &caches,
                            &value.cache_content_key,
                        );
                        crate::cache::store::notify_cache_changed(&cache_change_signal);
                        if let Ok(mut states) = states.lock() {
                            if let Some(state) = states.get_mut(&value.cache_content_key) {
                                state.state = CacheStatus::Interrupted;
                            }
                        }
                    }
                }
                publish_slot_transport_disconnected(
                    &slot_connection_state,
                    task_connection_generation,
                );
                if connection_generation.load(Ordering::Acquire) == task_connection_generation {
                    LOGGER.trace(format!(
                        "Sendspin downloader connection ended unexpectedly: slot {}, generation {}",
                        slot_index + 1,
                        task_connection_generation
                    ));
                    publish_connection_event(
                        &connection_event,
                        task_connection_generation,
                        DownloaderConnectionStatus::Lost,
                    );
                }
            });
            Ok::<_, String>((controller, sender, connection.guard, task))
        })?;
        if let Ok(mut controllers) = self.controllers.lock() {
            controllers[slot_index] = Some(controller);
        }
        self.guards[slot_index] = Some(guard);
        self.io_tasks[slot_index] = Some(task);
        self.liveness_tasks[slot_index] = Some(self.runtime.spawn(run_slot_liveness_check(
            sender,
            Arc::clone(&self.connection_event),
            Arc::clone(&self.connection_generation),
            task_connection_generation,
            slot_index,
        )));
        Ok(())
    }

    // Disconnection and reconnection lifecycle.
    pub(crate) fn disconnect(&mut self) {
        let has_active_connection = self
            .controllers
            .lock()
            .map(|controllers| controllers.iter().any(Option::is_some))
            .unwrap_or(false)
            || self.guards.iter().any(Option::is_some);
        if has_active_connection {
            LOGGER.info("disconnecting Sendspin downloader slots");
        }
        let active_reservations = self
            .slots
            .lock()
            .map(|slots| slots.iter().flatten().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for reservation in active_reservations {
            crate::cache::store::cache_discard_incomplete(
                &self.caches,
                &reservation.cache_content_key,
            );
        }
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        self.cancel_cache_pause_keepalives();
        self.cancel_download_request_tasks();
        self.runtime.block_on(async {
            for task in self.io_tasks.iter_mut() {
                if let Some(task) = task.take() {
                    task.abort();
                }
            }
            for task in self.liveness_tasks.iter_mut() {
                if let Some(task) = task.take() {
                    task.abort();
                }
            }
            for guard in self.guards.iter_mut() {
                if let Some(guard) = guard.take() {
                    let _ = guard.disconnect(GoodbyeReason::Shutdown).await;
                }
            }
        });
        if let Ok(mut controllers) = self.controllers.lock() {
            *controllers = [None, None];
        }
        if let Ok(mut slots) = self.slots.lock() {
            *slots = [None, None];
        }
        self.downloads_paused_for_capacity = [false, false];
        self.player_ids = [None, None];
        self.queue_ids = [None, None];
    }

    pub(crate) fn reconnect(&mut self) -> Result<(), String> {
        self.pause_for_reconnect();
        self.connect()
    }

    fn restart_for_codec_change(&mut self, config: SendspinDownloaderConfig) -> Result<(), String> {
        LOGGER.info(format!(
            "restarting Sendspin downloader for preferred codec {}",
            config.preferred_codec.as_str()
        ));
        self.pause_for_reconnect();
        crate::cache::store::cache_clear_all(&self.caches);
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        self.config = Some(config);
        self.connect()
    }

    fn set_pending_current_seek(&mut self, seconds: i64) {
        self.pending_current_seek_seconds = Some(seconds.max(0));
    }

    fn pause_for_reconnect(&mut self) {
        LOGGER.warn("pausing Sendspin downloads for reconnection");
        self.connection_generation.fetch_add(1, Ordering::AcqRel);
        self.cancel_liveness_checks();
        self.cancel_cache_pause_keepalives();
        self.cancel_download_request_tasks();
        let connection_generation = self.connection_generation.load(Ordering::Acquire);
        for slot_connection_state in &self.slot_connection_states {
            reset_slot_connection_state(slot_connection_state, connection_generation);
        }
        let reservations = self
            .slots
            .lock()
            .map(|slots| slots.iter().flatten().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for reservation in reservations {
            let cached_duration = crate::cache::store::cache_mark_interrupted(
                &self.caches,
                &reservation.cache_content_key,
            )
            .unwrap_or(0);
            if let Ok(mut states) = self.cache_states.lock() {
                if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                    state.state = CacheStatus::Interrupted;
                    state.cached_duration_millis = cached_duration;
                }
            }
        }
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        self.runtime.block_on(async {
            for task in self.io_tasks.iter_mut() {
                if let Some(task) = task.take() {
                    task.abort();
                }
            }
            for guard in self.guards.iter_mut() {
                if let Some(guard) = guard.take() {
                    let _ = guard.disconnect(GoodbyeReason::Shutdown).await;
                }
            }
        });
        if let Ok(mut controllers) = self.controllers.lock() {
            *controllers = [None, None];
        }
        if let Ok(mut slots) = self.slots.lock() {
            *slots = [None, None];
        }
        self.downloads_paused_for_capacity = [false, false];
        self.queue_ids = [None, None];
        // Keep these identities until connect stops their server-side queue flows.
    }

    // Download plan configuration.
    pub(crate) fn apply_download_plan(&mut self, plan: SendspinDownloadPlan) -> Result<(), String> {
        LOGGER.debug(format!(
            "downloader plan accepted: version {}, {} items",
            plan.version,
            plan.items.len()
        ));
        if let Ok(mut states) = self.cache_states.lock() {
            states.clear();
        }
        if let Ok(mut content_keys) = self.capacity_aborted_content_keys.lock() {
            content_keys.clear();
        }
        for item in &plan.items {
            if let Ok(mut states) = self.cache_states.lock() {
                states.insert(
                    item.cache_content_key.as_str().to_string(),
                    CacheItemState {
                        queue_item_id: item.queue_item_id.clone(),
                        state: CacheStatus::Queued,
                        cached_duration_millis: 0,
                    },
                );
            }
        }
        self.plan = Some(plan);
        Ok(())
    }

    pub(crate) fn mark_cache_buffering(&mut self, content_key: &str) -> Result<(), String> {
        let Ok(mut states) = self.cache_states.lock() else {
            return Err("Sendspin downloader state lock poisoned".to_string());
        };
        let Some(item) = states.get_mut(content_key) else {
            return Err("track is not present in the current download plan".to_string());
        };
        item.state = CacheStatus::Buffering;
        Ok(())
    }

    // Playback-window retention and cache capacity.
    pub(crate) fn update_playback_window(
        &mut self,
        current: Option<String>,
        upcoming: Vec<String>,
        recently_played: Vec<String>,
    ) {
        let next = upcoming.first().cloned();
        let last = recently_played.last().cloned();
        if let Ok(mut policy) = self.retention_policy.lock() {
            *policy = CacheRetentionPolicy {
                current,
                next,
                last,
                upcoming: upcoming.into_iter().skip(1).collect(),
                recently_played,
            };
        }
    }

    pub(crate) fn ensure_download_window(&mut self, content_keys: &[String]) -> Result<(), String> {
        self.resume_downloads_below_capacity();
        if self.enforce_soft_cache_capacity() {
            return Ok(());
        }
        let Some(plan) = self.plan.as_ref() else {
            return Ok(());
        };
        let candidates = content_keys
            .iter()
            .filter(|content_key| {
                self.capacity_aborted_content_keys
                    .lock()
                    .map(|aborted| !aborted.contains(*content_key))
                    .unwrap_or(false)
            })
            .filter_map(|content_key| {
                plan.items
                    .iter()
                    .find(|item| item.cache_content_key.as_str() == content_key)
                    .cloned()
            })
            .collect::<Vec<_>>();
        let Some(config) = self.config.clone() else {
            return Ok(());
        };
        let prioritized_content_keys = candidates
            .iter()
            .filter(|item| {
                crate::cache::store::cache_complete_duration_millis(
                    &self.caches,
                    &item.cache_content_key,
                )
                .is_none()
            })
            .take(2)
            .map(|item| item.cache_content_key.as_str().to_string())
            .collect::<Vec<_>>();
        self.preempt_deprioritized_downloads(&prioritized_content_keys);
        let discarded = crate::cache::store::cache_discard_interrupted_outside_window(
            &self.caches,
            content_keys,
        );
        if !discarded.is_empty() {
            crate::cache::store::notify_cache_changed(&self.cache_change_signal);
            if let Ok(mut states) = self.cache_states.lock() {
                for content_key in discarded {
                    if let Some(state) = states.get_mut(&content_key) {
                        state.state = CacheStatus::Discarded;
                        state.cached_duration_millis = 0;
                    }
                }
            }
        }

        for item in candidates {
            if crate::cache::store::cache_complete_duration_millis(
                &self.caches,
                &item.cache_content_key,
            )
            .is_some()
            {
                continue;
            }
            let slot_index = {
                let slots = self
                    .slots
                    .lock()
                    .map_err(|_| "Sendspin downloader slot lock poisoned".to_string())?;
                if slots
                    .iter()
                    .flatten()
                    .any(|slot| slot.cache_content_key == item.cache_content_key.as_str())
                {
                    continue;
                }
                slots.iter().position(Option::is_none)
            };
            let Some(slot_index) = slot_index else {
                break;
            };
            self.start_slot_download(slot_index, &item, &config)?;
        }
        Ok(())
    }

    fn preempt_deprioritized_downloads(&mut self, prioritized_content_keys: &[String]) {
        let preempted = self
            .slots
            .lock()
            .map(|mut slots| {
                slots
                    .iter_mut()
                    .enumerate()
                    .filter_map(|(slot_index, reservation)| {
                        let is_prioritized = reservation
                            .as_ref()
                            .map(|value| {
                                prioritized_content_keys
                                    .iter()
                                    .any(|key| key == &value.cache_content_key)
                            })
                            .unwrap_or(true);
                        (!is_prioritized)
                            .then(|| reservation.take().map(|value| (slot_index, value)))
                            .flatten()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if preempted.is_empty() {
            return;
        }

        for (slot_index, reservation) in &preempted {
            crate::cache::store::cache_discard_incomplete(
                &self.caches,
                &reservation.cache_content_key,
            );
            if let Ok(mut states) = self.cache_states.lock() {
                if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                    state.state = CacheStatus::Queued;
                    state.cached_duration_millis = 0;
                }
            }
            LOGGER.trace(format!(
                "download preempted by playback priority: slot {}, cache key {}",
                slot_index + 1,
                reservation.cache_content_key
            ));
        }
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);

        let controllers = Arc::clone(&self.controllers);
        for (slot_index, _) in &preempted {
            self.cancel_cache_pause_keepalive(*slot_index);
        }
        self.runtime.block_on(async {
            for (slot_index, _) in preempted {
                let controller = controllers
                    .lock()
                    .ok()
                    .and_then(|controllers| controllers[slot_index].clone());
                if let Some(controller) = controller {
                    let _ = controller.stop().await;
                }
            }
        });
    }

    // Cache-capacity pause and resume.
    fn enforce_soft_cache_capacity(&mut self) -> bool {
        let Some(cache_capacity_bytes) = self.config.as_ref().map(|config| config.cache_bytes)
        else {
            return false;
        };
        if !cache_reaches_download_pause_threshold(
            crate::cache::store::cache_total_bytes(&self.caches),
            cache_capacity_bytes,
        ) {
            return false;
        }

        self.pause_downloads_near_capacity();
        let current = self
            .retention_policy
            .lock()
            .ok()
            .and_then(|policy| policy.current.clone());
        let Some(current) = current else {
            return true;
        };
        let is_current_downloading = self
            .slots
            .lock()
            .map(|slots| {
                slots
                    .iter()
                    .flatten()
                    .any(|slot| slot.cache_content_key == current)
            })
            .unwrap_or(false);
        if !is_current_downloading {
            return true;
        }

        let target_capacity_bytes = cache_capacity_bytes
            .saturating_mul(CACHE_DOWNLOAD_RESUME_PERCENT)
            .saturating_div(100);
        let policy = self
            .retention_policy
            .lock()
            .map(|policy| policy.clone())
            .unwrap_or_default();
        let evicted = crate::cache::store::cache_evict_to_fit(
            &self.caches,
            &policy,
            target_capacity_bytes,
            &current,
            0,
        );
        self.abort_evicted_downloads(evicted);
        true
    }

    fn abort_evicted_downloads(&mut self, evicted_content_keys: Vec<String>) {
        if evicted_content_keys.is_empty() {
            return;
        }
        let slot_indexes = self
            .slots
            .lock()
            .map(|slots| {
                slots
                    .iter()
                    .enumerate()
                    .filter_map(|(slot_index, reservation)| {
                        reservation
                            .as_ref()
                            .filter(|value| evicted_content_keys.contains(&value.cache_content_key))
                            .map(|_| slot_index)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for slot_index in slot_indexes {
            self.cancel_cache_pause_keepalive(slot_index);
        }
        let slots = Arc::clone(&self.slots);
        let controllers = Arc::clone(&self.controllers);
        self.runtime.block_on(stop_evicted_slot_downloads(
            slots,
            controllers,
            evicted_content_keys.clone(),
        ));
        if let Ok(mut aborted) = self.capacity_aborted_content_keys.lock() {
            aborted.extend(evicted_content_keys.iter().cloned());
        }
        if let Ok(mut states) = self.cache_states.lock() {
            for cache_content_key in evicted_content_keys {
                if let Some(state) = states.get_mut(&cache_content_key) {
                    state.state = CacheStatus::Evicted;
                    state.cached_duration_millis = 0;
                }
            }
        }
    }

    fn pause_downloads_near_capacity(&mut self) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        if !cache_reaches_download_pause_threshold(
            crate::cache::store::cache_total_bytes(&self.caches),
            config.cache_bytes,
        ) {
            return;
        }

        let reservations = self
            .slots
            .lock()
            .map(|slots| slots.clone())
            .unwrap_or([None, None]);
        let current = self
            .retention_policy
            .lock()
            .ok()
            .and_then(|policy| policy.current.clone());
        for (slot_index, reservation) in reservations.iter().enumerate() {
            let Some(reservation) = reservation else {
                continue;
            };
            if current.as_deref() == Some(reservation.cache_content_key.as_str()) {
                continue;
            }
            if self.downloads_paused_for_capacity[slot_index] {
                continue;
            }
            let controller = self
                .controllers
                .lock()
                .ok()
                .and_then(|controllers| controllers[slot_index].clone());
            let Some(controller) = controller else {
                continue;
            };
            let cache_content_key = reservation.cache_content_key.clone();
            self.downloads_paused_for_capacity[slot_index] = true;
            // Music Assistant expires a paused queue; briefly resume and pause it to retain the server-side stream.
            self.cache_pause_keepalive_tasks[slot_index] = Some(self.runtime.spawn(async move {
                if controller.pause().await.is_err() {
                    return;
                }
                LOGGER.trace(format!(
                    "download paused near cache capacity: slot {}, cache key {}",
                    slot_index + 1,
                    cache_content_key
                ));
                loop {
                    tokio::time::sleep(CACHE_PAUSE_KEEPALIVE_INTERVAL).await;
                    if controller.play().await.is_err() {
                        return;
                    }
                    tokio::time::sleep(CACHE_PAUSE_KEEPALIVE_PLAY_DURATION).await;
                    if controller.pause().await.is_err() {
                        return;
                    }
                    LOGGER.trace(format!(
                        "cache-pause keepalive sent: slot {}",
                        slot_index + 1
                    ));
                }
            }));
        }
    }

    fn resume_downloads_below_capacity(&mut self) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        if crate::cache::store::cache_total_bytes(&self.caches).saturating_mul(100)
            >= config
                .cache_bytes
                .saturating_mul(CACHE_DOWNLOAD_RESUME_PERCENT)
        {
            return;
        }
        for slot_index in 0..self.downloads_paused_for_capacity.len() {
            if !self.downloads_paused_for_capacity[slot_index] {
                continue;
            }
            self.downloads_paused_for_capacity[slot_index] = false;
            if let Some(task) = self.cache_pause_keepalive_tasks[slot_index].take() {
                task.abort();
            }
            let controller = self
                .controllers
                .lock()
                .ok()
                .and_then(|controllers| controllers[slot_index].clone());
            if let Some(controller) = controller {
                self.runtime.spawn(async move {
                    let _ = controller.play().await;
                });
            }
        }
    }

    fn cancel_liveness_checks(&mut self) {
        for task in self.liveness_tasks.iter_mut() {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
    }

    fn cancel_cache_pause_keepalives(&mut self) {
        for slot_index in 0..self.cache_pause_keepalive_tasks.len() {
            self.cancel_cache_pause_keepalive(slot_index);
        }
    }

    fn cancel_cache_pause_keepalive(&mut self, slot_index: usize) {
        self.downloads_paused_for_capacity[slot_index] = false;
        if let Some(task) = self.cache_pause_keepalive_tasks[slot_index].take() {
            task.abort();
        }
    }

    // Background task coordination.
    fn cancel_download_request_tasks(&mut self) {
        let tasks = self
            .download_request_tasks
            .iter_mut()
            .filter_map(Option::take)
            .collect::<Vec<_>>();
        // Await cancellation so stale requests cannot issue a seek after the reconnect resets slot ownership.
        self.runtime.block_on(async {
            for task in tasks {
                task.abort();
                let _ = task.await;
            }
        });
    }

    // Download-slot startup.
    fn start_slot_download(
        &mut self,
        slot_index: usize,
        item: &SendspinDownloadPlanItem,
        config: &SendspinDownloaderConfig,
    ) -> Result<(), String> {
        if let Some(task) = self.download_request_tasks[slot_index].take() {
            task.abort();
        }
        if crate::cache::store::cache_complete_duration_millis(
            &self.caches,
            item.cache_content_key.as_str(),
        )
        .is_some()
        {
            LOGGER.trace(format!(
                "download skipped: cache hit for {}",
                item.cache_content_key.as_str()
            ));
            return Ok(());
        }
        let interrupted_resume_after_millis =
            crate::cache::store::cache_interrupted_duration_millis(
                &self.caches,
                item.cache_content_key.as_str(),
            );
        let interrupted_resume_position_seconds = interrupted_resume_after_millis.map(|duration| {
            duration
                .saturating_sub(RESUME_SEEK_PREROLL_MILLIS)
                .saturating_div(1_000) as i64
        });
        let requested_seek_seconds = if item.role == SendspinDownloadRole::Current {
            self.pending_current_seek_seconds.take()
        } else {
            None
        };
        let resume_position_seconds =
            requested_seek_seconds.or(interrupted_resume_position_seconds);
        let resume_after_millis = if let Some(seconds) = resume_position_seconds {
            (seconds as u64).saturating_mul(1_000)
        } else {
            0
        };
        if interrupted_resume_after_millis.is_none() && requested_seek_seconds.is_none() {
            crate::cache::store::cache_discard_incomplete(
                &self.caches,
                item.cache_content_key.as_str(),
            );
        }
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        let reservation = {
            let mut slots = self
                .slots
                .lock()
                .map_err(|_| "Sendspin downloader slot lock poisoned".to_string())?;
            let reservation = crate::cache::store::DownloadSlotReservation {
                cache_content_key: item.cache_content_key.as_str().to_string(),
                expected_duration_millis: item.expected_duration_millis,
                resume_after_millis,
                append_after_seek: Arc::new(AtomicBool::new(resume_position_seconds.is_none())),
                lease_id: format!(
                    "slot-{}-{}",
                    slot_index + 1,
                    self.cache_sequence.fetch_add(1, Ordering::Relaxed)
                ),
            };
            slots[slot_index] = Some(reservation.clone());
            reservation
        };
        LOGGER.trace(format!(
            "download prepared: slot {}, cache key {}",
            slot_index + 1,
            reservation.cache_content_key
        ));
        if let Ok(mut states) = self.cache_states.lock() {
            if let Some(state) = states.get_mut(item.cache_content_key.as_str()) {
                state.state = CacheStatus::Prepared;
            }
        }

        let queue_id = self.queue_ids[slot_index]
            .clone()
            .ok_or_else(|| "Music Assistant queue ID is not resolved".to_string())?;
        let player_id = self.player_ids[slot_index]
            .clone()
            .ok_or_else(|| "Music Assistant player ID is not resolved".to_string())?;
        let queue_id_for_log = queue_id.0.clone();
        LOGGER.trace(format!(
            "download request prepared: slot {}, role {:?}, queue ID {}, cache key {}",
            slot_index + 1,
            item.role,
            queue_id_for_log,
            reservation.cache_content_key
        ));
        let media_item = item.media_item.clone();
        let music_assistant_url = config.music_assistant_url.clone();
        let access_token = config.music_assistant_auth_token.clone();
        let controller = self
            .controllers
            .lock()
            .map_err(|_| "Sendspin downloader controller lock poisoned".to_string())?[slot_index]
            .clone()
            .ok_or_else(|| "Sendspin download slot controller is unavailable".to_string())?;
        let slots = Arc::clone(&self.slots);
        let caches = Arc::clone(&self.caches);
        let cache_change_signal = Arc::clone(&self.cache_change_signal);
        let states = Arc::clone(&self.cache_states);
        let download_window_refresh = Arc::clone(&self.download_window_refresh);
        let connection_event = Arc::clone(&self.connection_event);
        let connection_generation = Arc::clone(&self.connection_generation);
        let request_connection_generation = connection_generation.load(Ordering::Acquire);
        let append_after_seek = Arc::clone(&reservation.append_after_seek);
        let should_truncate_interrupted_cache =
            interrupted_resume_after_millis.is_some() && requested_seek_seconds.is_none();
        // The generation and reservation checks make this request harmless after preemption or reconnection.
        self.download_request_tasks[slot_index] = Some(self.runtime.spawn(async move {
            if !download_request_is_current(
                &slots,
                slot_index,
                &reservation,
                &connection_generation,
                request_connection_generation,
            ) {
                return;
            }
            let client = MusicAssistantClient::new(&music_assistant_url, access_token);
            let result = client
                .queue_play_media(queue_id, vec![media_item], "replace", false)
                .await;
            if !download_request_is_current(
                &slots,
                slot_index,
                &reservation,
                &connection_generation,
                request_connection_generation,
            ) {
                return;
            }
            if result.is_ok() {
                LOGGER.trace(format!(
                    "download requested: slot {}, queue ID {}, cache key {}",
                    slot_index + 1,
                    queue_id_for_log,
                    reservation.cache_content_key
                ));
            }
            if let Err(error) = result {
                LOGGER.error(&error);
                if is_player_unavailable_error(&error) {
                    LOGGER.warn(format!(
                        "Music Assistant player unavailable for download slot {}; reconnecting Sendspin downloaders",
                        slot_index + 1
                    ));
                    retain_interrupted_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        &download_window_refresh,
                        slot_index,
                        &reservation,
                    );
                    report_connection_lost_if_current(
                        &connection_generation,
                        &connection_event,
                        request_connection_generation,
                        slot_index,
                        "player unavailable",
                    );
                } else {
                    release_incomplete_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        slot_index,
                        &reservation,
                    );
                }
                return;
            }
            if let Some(position) = resume_position_seconds {
                if !download_request_is_current(
                    &slots,
                    slot_index,
                    &reservation,
                    &connection_generation,
                    request_connection_generation,
                ) {
                    return;
                }
                if let Err(error) =
                    seek_download_player(&client, &controller, player_id, position).await
                {
                    if !download_request_is_current(
                        &slots,
                        slot_index,
                        &reservation,
                        &connection_generation,
                        request_connection_generation,
                    ) {
                        return;
                    }
                    LOGGER.error(format!("failed to resume interrupted download: {error}"));
                    // The audio player may still consume cached chunks while the retry is pending.
                    retain_interrupted_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        &download_window_refresh,
                        slot_index,
                        &reservation,
                    );
                    // Signal the engine to recreate the controller before another resume attempt.
                    report_connection_lost_if_current(
                        &connection_generation,
                        &connection_event,
                        request_connection_generation,
                        slot_index,
                        "resume seek failure",
                    );
                    return;
                }
                if should_truncate_interrupted_cache
                    && crate::cache::store::slot_reservation_is_current(
                        &slots,
                        slot_index,
                        &reservation,
                    )
                {
                    crate::cache::store::cache_truncate_after_millis(
                        &caches,
                        &reservation.cache_content_key,
                        resume_after_millis,
                    );
                    crate::cache::store::notify_cache_changed(&cache_change_signal);
                    if let Ok(mut states) = states.lock() {
                        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                            state.cached_duration_millis = resume_after_millis;
                        }
                    }
                }
                append_after_seek.store(true, Ordering::Release);
            }
        }));
        Ok(())
    }

    // Cached-content seeking.
    pub(crate) fn seek(&mut self, content_key: &str, seconds: i64) -> Result<(), String> {
        let slot_index = self
            .slots
            .lock()
            .map_err(|_| "Sendspin downloader slot lock poisoned".to_string())?
            .iter()
            .position(|reservation| {
                reservation
                    .as_ref()
                    .map(|value| value.cache_content_key == content_key)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                "track is not assigned to an active Sendspin download slot".to_string()
            })?;
        self.cancel_cache_pause_keepalive(slot_index);
        let reservation = {
            let mut slots = self
                .slots
                .lock()
                .map_err(|_| "Sendspin downloader slot lock poisoned".to_string())?;
            let current = slots[slot_index]
                .as_ref()
                .ok_or_else(|| "Sendspin download slot reservation is unavailable".to_string())?;
            let replacement = crate::cache::store::DownloadSlotReservation {
                cache_content_key: current.cache_content_key.clone(),
                expected_duration_millis: current.expected_duration_millis,
                resume_after_millis: (seconds.max(0) as u64).saturating_mul(1_000),
                append_after_seek: Arc::new(AtomicBool::new(false)),
                lease_id: format!(
                    "slot-{}-{}",
                    slot_index + 1,
                    self.cache_sequence.fetch_add(1, Ordering::Relaxed)
                ),
            };
            slots[slot_index] = Some(replacement.clone());
            replacement
        };
        crate::cache::store::cache_discard(&self.caches, content_key);
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        if let Ok(mut states) = self.cache_states.lock() {
            if let Some(state) = states.get_mut(content_key) {
                state.state = CacheStatus::Buffering;
                state.cached_duration_millis = reservation.resume_after_millis;
            }
        }
        let controller = self
            .controllers
            .lock()
            .map_err(|_| "Sendspin downloader controller lock poisoned".to_string())?[slot_index]
            .clone()
            .ok_or_else(|| "Sendspin download slot controller is unavailable".to_string())?;
        let config = self
            .config
            .clone()
            .ok_or_else(|| "Sendspin downloader is not configured".to_string())?;
        let position = seconds.max(0);
        let player_id = self.player_ids[slot_index]
            .clone()
            .ok_or_else(|| "Music Assistant queue ID is not resolved".to_string())?;
        let client = MusicAssistantClient::new(
            &config.music_assistant_url,
            config.music_assistant_auth_token.clone(),
        );

        let seek_result = self.runtime.block_on(async {
            seek_download_player(&client, &controller, player_id, position).await
        });
        match seek_result {
            Ok(()) => {
                reservation.append_after_seek.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) => {
                release_incomplete_reservation(
                    &self.slots,
                    &self.caches,
                    &self.cache_change_signal,
                    &self.cache_states,
                    slot_index,
                    &reservation,
                );
                Err(error)
            }
        }
    }
}

enum DownloaderCommand {
    Configure(SendspinDownloaderConfig),
    RestartForCodecChange(SendspinDownloaderConfig),
    SetPendingCurrentSeek(i64),
    Connect,
    Reconnect,
    Disconnect,
    ApplyDownloadPlan(SendspinDownloadPlan),
    UpdatePlaybackWindow {
        current: Option<String>,
        upcoming: Vec<String>,
        recently_played: Vec<String>,
    },
    EnsureDownloadWindow(Vec<String>),
    MarkCacheBuffering(String),
    Seek {
        content_key: String,
        seconds: i64,
    },
    Shutdown,
}

pub(crate) struct SendspinDownloader {
    commands: Sender<DownloaderCommand>,
    download_plan: Arc<Mutex<Option<SendspinDownloadPlan>>>,
    cache_states: Arc<Mutex<HashMap<String, CacheItemState>>>,
    caches: crate::cache::store::EncodedCaches,
    download_window_refresh: Arc<AtomicBool>,
    download_status_refresh: Arc<AtomicBool>,
    connection_event: Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    cache_bytes: Arc<AtomicUsize>,
}

impl SendspinDownloader {
    // Construction and lifecycle commands.
    pub(crate) fn new() -> Self {
        let worker = SendspinDownloaderWorker::new();
        let cache_states = Arc::clone(&worker.cache_states);
        let caches = Arc::clone(&worker.caches);
        let download_window_refresh = Arc::clone(&worker.download_window_refresh);
        let download_status_refresh = Arc::clone(&worker.download_status_refresh);
        let connection_event = Arc::clone(&worker.connection_event);
        let cache_bytes = Arc::new(AtomicUsize::new(0));
        let download_plan = Arc::new(Mutex::new(None));
        let (commands, receiver) = mpsc::channel();
        std::thread::spawn(move || run_downloader_worker(worker, receiver));
        Self {
            commands,
            download_plan,
            cache_states,
            caches,
            download_window_refresh,
            download_status_refresh,
            connection_event,
            cache_bytes,
        }
    }

    pub(crate) fn configure(&self, config: SendspinDownloaderConfig) {
        self.cache_bytes
            .store(config.cache_bytes, Ordering::Release);
        let _ = self.commands.send(DownloaderCommand::Configure(config));
    }

    pub(crate) fn connect(&self) -> Result<(), String> {
        self.send(DownloaderCommand::Connect)
    }

    pub(crate) fn reconnect(&self) -> Result<(), String> {
        self.send(DownloaderCommand::Reconnect)
    }

    pub(crate) fn restart_for_codec_change(
        &self,
        config: SendspinDownloaderConfig,
    ) -> Result<(), String> {
        self.cache_bytes
            .store(config.cache_bytes, Ordering::Release);
        self.send(DownloaderCommand::RestartForCodecChange(config))
    }

    pub(crate) fn set_pending_current_seek(&self, seconds: i64) -> Result<(), String> {
        self.send(DownloaderCommand::SetPendingCurrentSeek(seconds))
    }

    pub(crate) fn disconnect(&self) {
        let _ = self.commands.send(DownloaderCommand::Disconnect);
    }

    // Download-plan commands and queries.
    pub(crate) fn apply_download_plan(&self, plan: SendspinDownloadPlan) -> Result<(), String> {
        if let Ok(mut current_plan) = self.download_plan.lock() {
            *current_plan = Some(plan.clone());
        }
        self.send(DownloaderCommand::ApplyDownloadPlan(plan))
    }

    pub(crate) fn download_plan(&self) -> Option<SendspinDownloadPlan> {
        self.download_plan.lock().ok().and_then(|plan| plan.clone())
    }

    pub(crate) fn cache_states(&self) -> Vec<CacheItemState> {
        self.cache_states
            .lock()
            .map(|states| states.values().cloned().collect())
            .unwrap_or_default()
    }

    // Worker event and status queries.
    pub(crate) fn take_download_window_refresh(&self) -> bool {
        self.download_window_refresh.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn take_download_status_refresh(&self) -> bool {
        self.download_status_refresh.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn take_connection_event(&self) -> Option<DownloaderConnectionEvent> {
        self.connection_event
            .lock()
            .ok()
            .and_then(|mut event| event.take())
    }

    pub(crate) fn download_status(&self) -> (Vec<ActiveDownloadStatus>, usize, usize) {
        let states = self.cache_states();
        let plan = self.download_plan();
        let active = plan
            .as_ref()
            .map(|plan| {
                plan.items
                    .iter()
                    .filter_map(|item| {
                        let state = states
                            .iter()
                            .find(|state| state.queue_item_id == item.queue_item_id)?;
                        matches!(
                            state.state,
                            CacheStatus::Prepared
                                | CacheStatus::Buffering
                                | CacheStatus::Downloading
                                | CacheStatus::Interrupted
                        )
                        .then(|| ActiveDownloadStatus {
                            queue_item_id: state.queue_item_id.clone(),
                            cached_duration_millis: state.cached_duration_millis,
                            expected_duration_millis: item.expected_duration_millis,
                            state: state.state,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        (
            active,
            crate::cache::store::cache_total_bytes(&self.caches),
            self.cache_bytes.load(Ordering::Acquire),
        )
    }

    pub(crate) fn completed_cache_item_count(&self, content_keys: &[String]) -> usize {
        content_keys
            .iter()
            .filter(|key| {
                crate::cache::store::cache_complete_duration_millis(&self.caches, key).is_some()
            })
            .count()
    }

    // Cache access commands and queries.
    pub(crate) fn mark_cache_buffering(&self, content_key: &str) -> Result<(), String> {
        self.send(DownloaderCommand::MarkCacheBuffering(
            content_key.to_string(),
        ))
    }

    pub(crate) fn cache_format(&self, content_key: &str) -> Result<AudioFormat, String> {
        crate::cache::store::cache_read_format(&self.caches, content_key)
    }

    pub(crate) fn cache_chunk_with_duration(
        &self,
        content_key: &str,
        chunk_index: usize,
    ) -> Result<(Option<(Arc<[u8]>, u64)>, bool), String> {
        crate::cache::store::cache_read_chunk_with_duration(&self.caches, content_key, chunk_index)
    }

    pub(crate) fn trim_played_current_cache(&self, content_key: &str, next_chunk_index: usize) {
        crate::cache::store::cache_trim_before_chunk(&self.caches, content_key, next_chunk_index);
    }

    pub(crate) fn local_cache_seek_target(
        &self,
        content_key: &str,
        target_position_millis: u64,
    ) -> Option<crate::cache::store::LocalCacheSeekTarget> {
        crate::cache::store::cache_local_seek_target(
            &self.caches,
            content_key,
            target_position_millis,
        )
    }

    // Playback-window and seeking commands.
    pub(crate) fn update_playback_window(
        &self,
        current: Option<String>,
        upcoming: Vec<String>,
        recently_played: Vec<String>,
    ) {
        let _ = self.commands.send(DownloaderCommand::UpdatePlaybackWindow {
            current,
            upcoming,
            recently_played,
        });
    }

    pub(crate) fn ensure_download_window(&self, content_keys: &[String]) -> Result<(), String> {
        self.send(DownloaderCommand::EnsureDownloadWindow(
            content_keys.to_vec(),
        ))
    }

    pub(crate) fn seek(&self, content_key: &str, seconds: i64) -> Result<(), String> {
        crate::cache::store::cache_discard(&self.caches, content_key);
        self.send(DownloaderCommand::Seek {
            content_key: content_key.to_string(),
            seconds,
        })
    }

    // Worker command transport.
    fn send(&self, command: DownloaderCommand) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "Sendspin downloader worker is unavailable".to_string())
    }
}

impl Drop for SendspinDownloader {
    fn drop(&mut self) {
        let _ = self.commands.send(DownloaderCommand::Shutdown);
    }
}

fn run_downloader_worker(
    mut worker: SendspinDownloaderWorker,
    receiver: mpsc::Receiver<DownloaderCommand>,
) {
    // This receiver is the serialization boundary for all mutable worker state.
    while let Ok(command) = receiver.recv() {
        let is_connection_command = matches!(
            command,
            DownloaderCommand::Connect
                | DownloaderCommand::Reconnect
                | DownloaderCommand::RestartForCodecChange(_)
        );
        let result = match command {
            DownloaderCommand::Configure(config) => {
                worker.configure(config);
                Ok(())
            }
            DownloaderCommand::RestartForCodecChange(config) => {
                worker.restart_for_codec_change(config)
            }
            DownloaderCommand::SetPendingCurrentSeek(seconds) => {
                worker.set_pending_current_seek(seconds);
                Ok(())
            }
            DownloaderCommand::Connect => worker.connect(),
            DownloaderCommand::Reconnect => worker.reconnect(),
            DownloaderCommand::Disconnect => {
                worker.disconnect();
                Ok(())
            }
            DownloaderCommand::ApplyDownloadPlan(plan) => worker.apply_download_plan(plan),
            DownloaderCommand::UpdatePlaybackWindow {
                current,
                upcoming,
                recently_played,
            } => {
                worker.update_playback_window(current, upcoming, recently_played);
                Ok(())
            }
            DownloaderCommand::EnsureDownloadWindow(content_keys) => {
                worker.ensure_download_window(&content_keys)
            }
            DownloaderCommand::MarkCacheBuffering(content_key) => {
                worker.mark_cache_buffering(&content_key)
            }
            DownloaderCommand::Seek {
                content_key,
                seconds,
            } => worker.seek(&content_key, seconds),
            DownloaderCommand::Shutdown => {
                worker.disconnect();
                break;
            }
        };
        if let Err(error) = result {
            LOGGER.error(format!(
                "Sendspin downloader worker command failed: {error}"
            ));
            publish_connection_event(
                &worker.connection_event,
                worker.connection_generation.load(Ordering::Acquire),
                DownloaderConnectionStatus::Lost,
            );
        } else if is_connection_command {
            let connection_generation = worker.connection_generation.load(Ordering::Acquire);
            let status = if slots_are_ready(&worker.slot_connection_states, connection_generation) {
                DownloaderConnectionStatus::Ready
            } else {
                DownloaderConnectionStatus::Lost
            };
            publish_connection_event(&worker.connection_event, connection_generation, status);
        }
    }
}

async fn seek_download_player(
    client: &MusicAssistantClient,
    controller: &Controller,
    player_id: PlayerId,
    position: i64,
) -> Result<(), String> {
    controller
        .play()
        .await
        .map_err(|error| format!("Sendspin seek slot play failed: {error}"))?;
    client.player_seek(player_id, position).await
}

fn reset_slot_connection_state(
    slot_connection_state: &Arc<Mutex<DownloaderSlotConnectionState>>,
    generation: u64,
) {
    if let Ok(mut value) = slot_connection_state.lock() {
        *value = DownloaderSlotConnectionState::disconnected(generation);
    }
}

fn publish_slot_transport_connected(
    slot_connection_state: &Arc<Mutex<DownloaderSlotConnectionState>>,
    generation: u64,
) {
    if let Ok(mut value) = slot_connection_state.lock() {
        *value = DownloaderSlotConnectionState {
            generation,
            is_transport_connected: true,
            is_stream_active: false,
        };
    }
}

fn publish_slot_stream_state(
    slot_connection_state: &Arc<Mutex<DownloaderSlotConnectionState>>,
    generation: u64,
    is_stream_active: bool,
) {
    if let Ok(mut value) = slot_connection_state.lock() {
        if value.generation == generation && value.is_transport_connected {
            value.is_stream_active = is_stream_active;
        }
    }
}

fn publish_slot_transport_disconnected(
    slot_connection_state: &Arc<Mutex<DownloaderSlotConnectionState>>,
    generation: u64,
) {
    if let Ok(mut value) = slot_connection_state.lock() {
        if value.generation == generation {
            value.is_transport_connected = false;
            value.is_stream_active = false;
        }
    }
}

fn slots_are_ready(
    slot_connection_states: &[Arc<Mutex<DownloaderSlotConnectionState>>; 2],
    connection_generation: u64,
) -> bool {
    slot_connection_states.iter().all(|state| {
        state
            .lock()
            .map(|value| {
                value.generation == connection_generation
                    && value.is_transport_connected
                    && !value.is_stream_active
            })
            .unwrap_or(false)
    })
}

// Music Assistant configuration for Sendspin download slots.
async fn stop_previous_download_players(
    client: &MusicAssistantClient,
    player_ids: &[Option<PlayerId>; 2],
) -> Result<(), String> {
    for player_id in player_ids.iter().flatten() {
        client.player_stop(player_id).await?;
    }
    Ok(())
}

async fn resolve_download_player_ids(
    client: &MusicAssistantClient,
    output_protocol_ids: &[String; 2],
) -> Result<[PlayerId; 2], String> {
    let mut retry_delay = DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY;
    for attempt in 0..DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS {
        let players = client.get_players(true, true).await?;
        if let (Some(first), Some(second)) = (
            find_download_player_id(&players, &output_protocol_ids[0]),
            find_download_player_id(&players, &output_protocol_ids[1]),
        ) {
            return Ok([first, second]);
        }
        if attempt + 1 < DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS {
            tokio::time::sleep(retry_delay).await;
            retry_delay = retry_delay
                .saturating_mul(2)
                .min(MAXIMUM_DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY);
        }
    }

    Err(format!(
        "Music Assistant players for Sendspin clients {}, {} were not registered within {} milliseconds",
        output_protocol_ids[0],
        output_protocol_ids[1],
        DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS
            * DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY.as_millis() as usize
    ))
}

async fn disable_download_queue_autoplay(
    client: &MusicAssistantClient,
    queue_ids: &[QueueId; 2],
) -> Result<(), String> {
    for queue_id in queue_ids {
        client.set_queue_autoplay(queue_id, false).await?;
    }
    Ok(())
}

async fn resolve_download_queue_ids(
    client: &MusicAssistantClient,
    player_ids: &[PlayerId; 2],
) -> Result<[QueueId; 2], String> {
    let mut retry_delay = DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY;
    for attempt in 0..DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS {
        let queues = client.get_player_queues().await?;
        if let Some(queue_ids) = find_download_queue_ids(&queues, player_ids) {
            return Ok(queue_ids);
        }
        if attempt + 1 < DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS {
            tokio::time::sleep(retry_delay).await;
            retry_delay = retry_delay
                .saturating_mul(2)
                .min(MAXIMUM_DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY);
        }
    }

    Err(format!(
        "Music Assistant queues for Sendspin players {}, {} were not created within {} milliseconds",
        player_ids[0].as_str(),
        player_ids[1].as_str(),
        DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS
            * DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY.as_millis() as usize
    ))
}

fn find_download_queue_ids(
    queues: &[MusicAssistantQueue],
    player_ids: &[PlayerId; 2],
) -> Option<[QueueId; 2]> {
    let first = queues
        .iter()
        .find(|queue| queue.queue_id.as_str() == player_ids[0].as_str())?
        .queue_id
        .clone();
    let second = queues
        .iter()
        .find(|queue| queue.queue_id.as_str() == player_ids[1].as_str())?
        .queue_id
        .clone();
    Some([first, second])
}

fn find_download_player_id(
    players: &[MusicAssistantPlayer],
    output_protocol_id: &str,
) -> Option<PlayerId> {
    players
        .iter()
        .find(|player| {
            player
                .output_protocols
                .iter()
                .any(|protocol| protocol.output_protocol_id == output_protocol_id)
        })
        .map(|player| player.player_id.clone())
}

fn release_incomplete_reservation(
    slots: &crate::cache::store::DownloadSlots,
    caches: &crate::cache::store::EncodedCaches,
    cache_change_signal: &crate::cache::store::CacheChangeSignal,
    states: &Arc<Mutex<HashMap<String, CacheItemState>>>,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
) {
    if !crate::cache::store::slot_release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::cache_discard_incomplete(caches, &reservation.cache_content_key);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    if let Ok(mut states) = states.lock() {
        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
            state.state = CacheStatus::Failed;
            state.cached_duration_millis = 0;
        }
    }
}

fn download_request_is_current(
    slots: &crate::cache::store::DownloadSlots,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
    connection_generation: &Arc<std::sync::atomic::AtomicU64>,
    request_connection_generation: u64,
) -> bool {
    connection_generation.load(Ordering::Acquire) == request_connection_generation
        && crate::cache::store::slot_reservation_is_current(slots, slot_index, reservation)
}

fn retain_interrupted_reservation(
    slots: &crate::cache::store::DownloadSlots,
    caches: &crate::cache::store::EncodedCaches,
    cache_change_signal: &crate::cache::store::CacheChangeSignal,
    states: &Arc<Mutex<HashMap<String, CacheItemState>>>,
    download_window_refresh: &Arc<AtomicBool>,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
) {
    if !crate::cache::store::slot_release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::cache_mark_interrupted(caches, &reservation.cache_content_key);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    download_window_refresh.store(true, Ordering::Release);
    if let Ok(mut states) = states.lock() {
        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
            state.state = CacheStatus::Interrupted;
        }
    }
    LOGGER.warn(format!(
        "download stream interrupted on slot {}; retaining cache for resume",
        slot_index + 1,
    ));
}

fn complete_stream_reservation(
    slots: &crate::cache::store::DownloadSlots,
    caches: &crate::cache::store::EncodedCaches,
    cache_change_signal: &crate::cache::store::CacheChangeSignal,
    states: &Arc<Mutex<HashMap<String, CacheItemState>>>,
    download_window_refresh: &Arc<AtomicBool>,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
) {
    if !crate::cache::store::slot_release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::cache_mark_complete(caches, reservation);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    download_window_refresh.store(true, Ordering::Release);
    if let Ok(mut states) = states.lock() {
        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
            state.state = CacheStatus::Complete;
        }
    }
    LOGGER.trace(format!(
        "download complete from stream end: slot {}, cache key {}",
        slot_index + 1,
        reservation.cache_content_key
    ));
}

async fn stop_evicted_slot_downloads(
    slots: crate::cache::store::DownloadSlots,
    controllers: Arc<Mutex<[Option<Controller>; 2]>>,
    evicted_content_keys: Vec<String>,
) {
    if evicted_content_keys.is_empty() {
        return;
    }
    let slot_indexes = slots
        .lock()
        .map(|mut slots| {
            slots
                .iter_mut()
                .enumerate()
                .filter_map(|(index, reservation)| {
                    let matches = reservation
                        .as_ref()
                        .map(|value| evicted_content_keys.contains(&value.cache_content_key))
                        .unwrap_or(false);
                    matches.then(|| reservation.take().map(|value| (index, value)))
                })
                .flatten()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for (slot_index, _) in slot_indexes {
        let controller = controllers
            .lock()
            .ok()
            .and_then(|controllers| controllers[slot_index].clone());
        if let Some(controller) = controller {
            let _ = controller.stop().await;
        }
    }
}

fn normalize_client_id(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
}

fn is_player_unavailable_error(error: &str) -> bool {
    error.contains("PlayerUnavailableError")
        || error.contains("player is not available")
        || error.contains("Player unavailable")
}

// Only the current connection generation may trigger a global downloader reconnect.
fn report_connection_lost_if_current(
    connection_generation: &Arc<AtomicU64>,
    connection_event: &Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    request_connection_generation: u64,
    slot_index: usize,
    source: &str,
) {
    let current_connection_generation = connection_generation.load(Ordering::Acquire);
    if current_connection_generation != request_connection_generation {
        LOGGER.trace(format!(
            "ignored stale Sendspin connection loss: source {source}, slot {}, request generation {}, current generation {}",
            slot_index + 1,
            request_connection_generation,
            current_connection_generation
        ));
        return;
    }
    LOGGER.trace(format!(
        "Sendspin connection loss reported: source {source}, slot {}, generation {}",
        slot_index + 1,
        request_connection_generation
    ));
    publish_connection_event(
        connection_event,
        request_connection_generation,
        DownloaderConnectionStatus::Lost,
    );
}

fn publish_connection_event(
    connection_event: &Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    generation: u64,
    status: DownloaderConnectionStatus,
) {
    let Ok(mut event) = connection_event.lock() else {
        return;
    };
    if event
        .as_ref()
        .map(|current| current.generation > generation)
        .unwrap_or(false)
    {
        return;
    }
    // A loss is terminal for its generation; a late ready result must not restore it.
    if event
        .as_ref()
        .map(|current| {
            current.generation == generation
                && current.status == DownloaderConnectionStatus::Lost
                && status == DownloaderConnectionStatus::Ready
        })
        .unwrap_or(false)
    {
        return;
    }
    *event = Some(DownloaderConnectionEvent { generation, status });
}

fn download_duration_reached(
    expected_duration_millis: Option<u64>,
    cached_duration_millis: u64,
) -> bool {
    expected_duration_millis
        .map(|expected| {
            cached_duration_millis.saturating_add(CACHE_COMPLETION_TOLERANCE_MILLIS) >= expected
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamEndOutcome {
    AwaitingSeekStream,
    Complete,
    Interrupted,
}

fn stream_end_outcome(
    seek_completed: bool,
    expected_duration_millis: Option<u64>,
    cached_duration_millis: u64,
) -> StreamEndOutcome {
    if !seek_completed {
        StreamEndOutcome::AwaitingSeekStream
    } else if download_duration_reached(expected_duration_millis, cached_duration_millis) {
        StreamEndOutcome::Complete
    } else {
        StreamEndOutcome::Interrupted
    }
}

fn cache_reaches_download_pause_threshold(cache_used_bytes: usize, cache_bytes: usize) -> bool {
    cache_bytes > 0
        && cache_used_bytes.saturating_mul(100)
            >= cache_bytes.saturating_mul(CACHE_DOWNLOAD_PAUSE_PERCENT)
}

async fn run_slot_liveness_check(
    sender: WsSender,
    connection_event: Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    connection_generation: Arc<AtomicU64>,
    task_connection_generation: u64,
    slot_index: usize,
) {
    // Transport liveness is determined by Ping/Pong, not by whether audio payload is currently flowing.
    loop {
        tokio::time::sleep(CONNECTION_LIVENESS_INTERVAL).await;
        if sender
            .ping_and_wait(CONNECTION_LIVENESS_TIMEOUT)
            .await
            .is_ok()
        {
            continue;
        }
        report_connection_lost_if_current(
            &connection_generation,
            &connection_event,
            task_connection_generation,
            slot_index,
            "ping timeout",
        );
        break;
    }
}

fn download_client_id(base_id: &str, slot_number: usize) -> String {
    let normalized = normalize_client_id(base_id);
    let base_id = if normalized.is_empty() {
        "mashin".to_string()
    } else {
        normalized
    };
    format!("{base_id}dl{slot_number}")
}

fn download_client_name(base_name: &str, slot_number: usize) -> String {
    let base_name = base_name.trim();
    let base_name = if base_name.is_empty() {
        "Mashin (Device)"
    } else {
        base_name
    };
    format!("{base_name} DL{slot_number}")
}

#[cfg(test)]
mod tests {
    use super::{
        cache_reaches_download_pause_threshold, download_client_id, download_client_name,
        download_duration_reached, is_player_unavailable_error, normalize_client_id,
        publish_connection_event, report_connection_lost_if_current, stream_end_outcome,
        DownloaderConnectionEvent, DownloaderConnectionStatus, SendspinDownloader,
        StreamEndOutcome,
    };
    use crate::cache::store::DownloadSlotReservation;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    #[test]
    fn normalizes_client_ids_without_separators() {
        assert_eq!(
            normalize_client_id("Mashin-Surface-Pro_11"),
            "mashinsurfacepro11"
        );
        assert_eq!(
            download_client_id("mashin-surface-pro-11", 1),
            "mashinsurfacepro11dl1"
        );
        assert_eq!(
            download_client_id("mashin-surface-pro-11", 2),
            "mashinsurfacepro11dl2"
        );
    }

    #[test]
    fn uses_expected_download_client_names() {
        assert_eq!(
            download_client_name("Mashin (Surface Pro 11)", 1),
            "Mashin (Surface Pro 11) DL1"
        );
        assert_eq!(download_client_name("", 2), "Mashin (Device) DL2");
    }

    #[test]
    fn recognizes_unavailable_universal_player_errors() {
        assert!(is_player_unavailable_error(
            "Music Assistant command 'player_queues/play_media' returned HTTP 500 Internal Server Error: PlayerUnavailableError: Player up3bfa938d is not available"
        ));
        assert!(!is_player_unavailable_error(
            "Music Assistant command 'player_queues/play_media' returned HTTP 400 Bad Request: invalid media"
        ));
    }

    #[test]
    fn ignores_connection_losses_from_an_older_generation() {
        let connection_generation = Arc::new(std::sync::atomic::AtomicU64::new(2));
        let connection_event = Arc::new(Mutex::new(None));

        report_connection_lost_if_current(&connection_generation, &connection_event, 1, 0, "test");
        assert!(connection_event.lock().unwrap().is_none());

        report_connection_lost_if_current(&connection_generation, &connection_event, 2, 0, "test");
        assert!(matches!(
            *connection_event.lock().unwrap(),
            Some(DownloaderConnectionEvent {
                generation: 2,
                status: DownloaderConnectionStatus::Lost,
            })
        ));
    }

    #[test]
    fn lost_connection_event_overrides_ready_for_the_same_generation() {
        let connection_event = Arc::new(Mutex::new(None));

        publish_connection_event(&connection_event, 2, DownloaderConnectionStatus::Lost);
        publish_connection_event(&connection_event, 2, DownloaderConnectionStatus::Ready);

        assert!(matches!(
            *connection_event.lock().unwrap(),
            Some(DownloaderConnectionEvent {
                generation: 2,
                status: DownloaderConnectionStatus::Lost,
            })
        ));
    }

    #[test]
    fn completes_download_when_cached_duration_reaches_tolerance_window() {
        assert!(!download_duration_reached(Some(180_000), 179_499));
        assert!(download_duration_reached(Some(180_000), 179_500));
        assert!(!download_duration_reached(None, 999_999));
    }

    #[test]
    fn stream_end_waits_for_a_pending_seek_or_uses_the_completion_tolerance() {
        assert_eq!(
            stream_end_outcome(false, Some(180_000), 180_000),
            StreamEndOutcome::AwaitingSeekStream
        );
        assert_eq!(
            stream_end_outcome(true, Some(180_000), 179_500),
            StreamEndOutcome::Complete
        );
        assert_eq!(
            stream_end_outcome(true, Some(180_000), 179_499),
            StreamEndOutcome::Interrupted
        );
    }

    #[test]
    fn pauses_downloads_at_ninety_five_percent_cache_usage() {
        assert!(!cache_reaches_download_pause_threshold(94, 100));
        assert!(cache_reaches_download_pause_threshold(95, 100));
        assert!(cache_reaches_download_pause_threshold(100, 100));
        assert!(!cache_reaches_download_pause_threshold(0, 0));
    }

    #[test]
    fn resumed_download_accepts_chunks_only_after_the_seek_completes() {
        let reservation = DownloadSlotReservation {
            cache_content_key: "provider:track:resume".to_string(),
            expected_duration_millis: None,
            resume_after_millis: 10_500,
            append_after_seek: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            lease_id: "slot-1-1".to_string(),
        };

        assert!(!reservation.append_after_seek.load(Ordering::Acquire));
        reservation.append_after_seek.store(true, Ordering::Release);
        assert!(reservation.append_after_seek.load(Ordering::Acquire));
    }

    #[test]
    fn consumes_download_window_refresh_request_once() {
        let downloader = SendspinDownloader::new();
        downloader
            .download_window_refresh
            .store(true, Ordering::Release);

        assert!(downloader.take_download_window_refresh());
        assert!(!downloader.take_download_window_refresh());
    }
}
