//! Owns the two Sendspin download slots and their encoded caches.
//!
//! Public commands are serialized through a dedicated worker thread. Its I/O tasks publish
//! cache, stream, and generated connection state through shared synchronization primitives.

use super::connection::{audio_format_from_stream, player_support};
use super::models::{
    CacheItemState, SendspinAudioCodec, SendspinDownloadPlan, SendspinDownloadPlanItem,
    SendspinDownloadPlanRequest, SendspinDownloadQueueItem,
};
use crate::logging::Logger;
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::identifiers::CacheContentKey;
use crate::music_assistant::models::identifiers::{AccessToken, PlayerId, QueueId};
use crate::music_assistant::models::player::MusicAssistantPlayer;
use crate::music_assistant::models::queue::{CacheStatus, MusicAssistantQueue};
use ::sendspin::audio::AudioFormat;
use ::sendspin::protocol::client::{ConnectionGuard, Controller, WsSender};
use ::sendspin::protocol::messages::{GoodbyeReason, Message, PlayerState};
use ::sendspin::ProtocolClientBuilder;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

static LOGGER: Logger = Logger::new(module_path!());

const CACHE_CAPACITY_PRESSURE_PERCENT: usize = 95;
const CACHE_CAPACITY_RECOVERY_PERCENT: usize = 90;
const CONNECTION_LIVENESS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
const CONNECTION_LIVENESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const SEEK_REPLACEMENT_STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const SEEK_CONFIRMATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const DOWNLOAD_PROGRESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
const MAXIMUM_PENDING_SEEK_AUDIO_BYTES: usize = 8 * 1024 * 1024;
const DOWNLOAD_QUEUE_REGISTRATION_ATTEMPTS: usize = 4;
const DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY: std::time::Duration =
    std::time::Duration::from_secs(2);
const MAXIMUM_DOWNLOAD_QUEUE_REGISTRATION_RETRY_DELAY: std::time::Duration =
    std::time::Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, Eq)]
enum DownloadCapacityMode {
    Dual,
    Single { completion_key: String },
    Zero,
}

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
    download_plan: Arc<Mutex<Option<SendspinDownloadPlan>>>,
    cache_states: Arc<Mutex<HashMap<String, CacheItemState>>>,
    caches: crate::cache::store::EncodedCaches,
    cache_change_signal: crate::cache::store::CacheChangeSignal,
    capacity_mode: DownloadCapacityMode,
    download_capacity_mode: Arc<Mutex<DownloadCapacityMode>>,
    cache_touch_sequence: Arc<std::sync::atomic::AtomicU64>,
    next_reservation_id: u64,
    download_status_refresh: Arc<AtomicBool>,
    confirmed_download_progress: Arc<AtomicU64>,
    event_sender: Sender<DownloaderCommand>,
    connection_event: Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    connection_generation: Arc<AtomicU64>,
    downloads_allowed: Arc<AtomicBool>,
    // Updated by I/O tasks and used to validate a generated Ready event.
    slot_connection_states: [Arc<Mutex<DownloaderSlotConnectionState>>; 2],
    slots: crate::cache::store::DownloadSlots,
    player_ids: [Option<PlayerId>; 2],
    queue_ids: [Option<QueueId>; 2],
    controllers: Arc<Mutex<[Option<Controller>; 2]>>,
    guards: [Option<ConnectionGuard>; 2],
    io_tasks: [Option<JoinHandle<()>>; 2],
    liveness_tasks: [Option<JoinHandle<()>>; 2],
    download_request_tasks: [Option<JoinHandle<()>>; 2],
    pending_current_seek_seconds: Option<i64>,
}

impl SendspinDownloaderWorker {
    // Construction and connection setup.
    fn new(
        event_sender: Sender<DownloaderCommand>,
        download_plan: Arc<Mutex<Option<SendspinDownloadPlan>>>,
        download_capacity_mode: Arc<Mutex<DownloadCapacityMode>>,
    ) -> Self {
        Self {
            runtime: tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Sendspin downloader runtime could not be created"),
            config: None,
            plan: None,
            download_plan,
            cache_states: Arc::new(Mutex::new(HashMap::new())),
            caches: Arc::new(Mutex::new(HashMap::new())),
            cache_change_signal: crate::cache::store::new_cache_change_signal(),
            capacity_mode: DownloadCapacityMode::Dual,
            download_capacity_mode,
            cache_touch_sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            next_reservation_id: 0,
            download_status_refresh: Arc::new(AtomicBool::new(false)),
            confirmed_download_progress: Arc::new(AtomicU64::new(0)),
            event_sender,
            connection_event: Arc::new(Mutex::new(None)),
            connection_generation: Arc::new(AtomicU64::new(0)),
            downloads_allowed: Arc::new(AtomicBool::new(false)),
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
            download_request_tasks: [None, None],
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

    // Establishes both downloader transports and binds them to their Music Assistant queues.
    pub(crate) fn connect(&mut self) -> Result<(), String> {
        LOGGER.info("connecting Sendspin downloader slots");
        if self
            .controllers
            .lock()
            .map(|controllers| controllers.iter().all(Option::is_some))
            .unwrap_or(false)
        {
            return if self.downloads_allowed.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err("Sendspin downloader reconnect required after connection loss".to_string())
            };
        }
        let config = self
            .config
            .clone()
            .ok_or_else(|| "Sendspin downloader is not configured".to_string())?;
        let client = MusicAssistantClient::new(
            &config.music_assistant_url,
            config.music_assistant_auth_token.clone(),
        );
        let client_ids = [
            download_client_id(&config.client_id, 1),
            download_client_id(&config.client_id, 2),
        ];
        self.runtime
            .block_on(stop_existing_download_players(&client, &client_ids))?;
        self.connect_slot(0)?;
        self.connect_slot(1)?;
        let player_ids = self
            .runtime
            .block_on(resolve_download_player_ids(&client, &client_ids))?;
        let queue_ids = self
            .runtime
            .block_on(resolve_download_queue_ids(&client, &player_ids))?;
        self.runtime
            .block_on(disable_download_queue_autoplay(&client, &queue_ids))?;
        let connection_generation = self.connection_generation.load(Ordering::Acquire);
        if !slots_are_ready(&self.slot_connection_states, connection_generation) {
            return Err(
                "Sendspin downloader transport disconnected during connection setup".to_string(),
            );
        }
        let connection_event = self
            .connection_event
            .lock()
            .map_err(|_| "Sendspin downloader connection event lock poisoned".to_string())?;
        if connection_event.is_some_and(|event| {
            event.generation == connection_generation
                && event.status == DownloaderConnectionStatus::Lost
        }) {
            return Err("Sendspin downloader connection lost during setup".to_string());
        }
        self.downloads_allowed.store(true, Ordering::Release);
        drop(connection_event);
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

    // Starts one stream task and publishes its generation-scoped connection state.
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
        let cache_touch_sequence = Arc::clone(&self.cache_touch_sequence);
        let event_sender = self.event_sender.clone();
        let download_status_refresh = Arc::clone(&self.download_status_refresh);
        let confirmed_download_progress = Arc::clone(&self.confirmed_download_progress);
        let connection_event = Arc::clone(&self.connection_event);
        let connection_generation = Arc::clone(&self.connection_generation);
        let downloads_allowed = Arc::clone(&self.downloads_allowed);
        let task_connection_generation = connection_generation.load(Ordering::Acquire);
        let slot_connection_state = Arc::clone(&self.slot_connection_states[slot_index]);
        publish_slot_transport_connected(&slot_connection_state, task_connection_generation);
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
            let stream_controller = controller.clone();
            // This task owns the stream-local reservation until it is completed, interrupted, or superseded.
            let task = tokio::spawn(async move {
                let mut format: Option<AudioFormat> = None;
                let mut reservation: Option<crate::cache::store::DownloadSlotReservation> = None;
                let mut pending_audio_chunk: Option<(i64, Arc<[u8]>)> = None;
                let mut buffered_audio_chunks: VecDeque<(u64, Arc<[u8]>)> = VecDeque::new();
                let mut buffered_audio_bytes = 0_usize;
                let mut stream_ended_before_seek_confirmation = false;
                let mut cached_bytes = 0_usize;
                let mut cached_duration_millis = 0_u64;
                let mut last_chunk_duration_millis = 0_u64;
                let mut next_progress_millis = 5_000_u64;
                let mut seek_gate_reported = false;
                let mut first_chunk_reported = false;
                let mut seek_replacement_deadline = None;
                let mut seek_confirmation_deadline: Option<(String, tokio::time::Instant)> = None;
                let mut stalled_content_key: Option<String> = None;
                let mut download_progress_deadline = None;
                let mut observed_cached_bytes = 0;
                loop {
                    if cached_bytes > observed_cached_bytes {
                        observed_cached_bytes = cached_bytes;
                        confirmed_download_progress.fetch_add(1, Ordering::Release);
                        download_progress_deadline = Some(
                            tokio::time::Instant::now() + DOWNLOAD_PROGRESS_TIMEOUT,
                        );
                    }
                    if seek_confirmation_deadline.as_ref().is_some_and(|(lease_id, _)| {
                        reservation.as_ref().is_none_or(|value| {
                            value.lease_id != *lease_id
                                || value.append_after_seek.load(Ordering::Acquire)
                        })
                    }) {
                        seek_confirmation_deadline = None;
                    }
                    if !downloads_allowed.load(Ordering::Acquire) {
                        buffered_audio_chunks.clear();
                        buffered_audio_bytes = 0;
                        pending_audio_chunk = None;
                        stream_ended_before_seek_confirmation = false;
                    }
                    if !buffered_audio_chunks.is_empty() || stream_ended_before_seek_confirmation {
                        if let (Some(value), Some(stream_format)) = (reservation.as_ref(), format.as_ref()) {
                            if !crate::cache::store::slot_reservation_is_current(&slots, slot_index, value) {
                                reservation = None;
                                format = None;
                                pending_audio_chunk = None;
                                buffered_audio_chunks.clear();
                                buffered_audio_bytes = 0;
                                stream_ended_before_seek_confirmation = false;
                                continue;
                            }
                            if value.append_after_seek.load(Ordering::Acquire) {
                                while let Some((duration, data)) = buffered_audio_chunks.pop_front() {
                                    append_download_chunk(
                                        &slots, &downloads_allowed, &caches, &cache_change_signal, &cache_touch_sequence,
                                        &states, &event_sender, &download_status_refresh,
                                        slot_index, value, stream_format, duration, &data,
                                        &mut cached_bytes, &mut cached_duration_millis,
                                        &mut first_chunk_reported, &mut next_progress_millis,
                                    );
                                }
                                buffered_audio_bytes = 0;
                                if stream_ended_before_seek_confirmation {
                                    if let Some((_, data)) = pending_audio_chunk.take() {
                                        let duration = terminal_chunk_duration_millis(
                                            last_chunk_duration_millis,
                                            value.expected_duration_millis,
                                            cached_duration_millis,
                                        );
                                        append_download_chunk(
                                            &slots, &downloads_allowed, &caches, &cache_change_signal, &cache_touch_sequence,
                                            &states, &event_sender, &download_status_refresh,
                                            slot_index, value, stream_format, duration, &data,
                                            &mut cached_bytes, &mut cached_duration_millis,
                                            &mut first_chunk_reported, &mut next_progress_millis,
                                        );
                                    }
                                    if crate::cache::store::slot_reservation_is_current(&slots, slot_index, value) {
                                        retain_interrupted_reservation(
                                            &slots, &caches, &cache_change_signal, &states,
                                            &event_sender, slot_index, value,
                                        );
                                    }
                                    reservation = None;
                                    format = None;
                                    stream_ended_before_seek_confirmation = false;
                                    seek_replacement_deadline = None;
                                }
                            }
                        }
                    }
                    tokio::select! {
                        _ = async {
                            match (download_progress_deadline, reservation.as_ref()) {
                                (Some(deadline), Some(_)) if downloads_allowed.load(Ordering::Acquire) => {
                                    tokio::time::sleep_until(deadline).await
                                }
                                _ => std::future::pending::<()>().await,
                            }
                        } => {
                            if let Some(value) = reservation.as_ref() {
                                if connection_generation.load(Ordering::Acquire) == task_connection_generation
                                    && crate::cache::store::slot_reservation_is_current(&slots, slot_index, value) {
                                    LOGGER.warn(format!(
                                        "download stalled on slot {} for cache key {}",
                                        slot_index + 1,
                                        value.cache_content_key
                                    ));
                                    downloads_allowed.store(false, Ordering::Release);
                                    report_connection_lost_if_current(
                                        &connection_generation, &connection_event,
                                        task_connection_generation, slot_index, "download progress timeout",
                                    );
                                }
                            }
                            download_progress_deadline = None;
                        }
                        _ = async {
                            match seek_confirmation_deadline.as_ref() {
                                Some((_, deadline)) => tokio::time::sleep_until(*deadline).await,
                                None => std::future::pending::<()>().await,
                            }
                        } => {
                            if let Some(value) = reservation.as_ref() {
                                if connection_generation.load(Ordering::Acquire) == task_connection_generation
                                    && crate::cache::store::slot_reservation_is_current(&slots, slot_index, value)
                                    && !value.append_after_seek.load(Ordering::Acquire)
                                {
                                    LOGGER.warn(format!(
                                        "seek confirmation timed out on slot {} for cache key {}",
                                        slot_index + 1,
                                        value.cache_content_key
                                    ));
                                    downloads_allowed.store(false, Ordering::Release);
                                    report_connection_lost_if_current(
                                        &connection_generation, &connection_event,
                                        task_connection_generation, slot_index, "seek confirmation timeout",
                                    );
                                }
                            }
                            seek_confirmation_deadline = None;
                        }
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
                                        &event_sender,
                                        slot_index,
                                        value,
                                    );
                                }
                            }
                            reservation = None;
                            format = None;
                            pending_audio_chunk = None;
                            buffered_audio_chunks.clear();
                            buffered_audio_bytes = 0;
                            stream_ended_before_seek_confirmation = false;
                            seek_replacement_deadline = None;
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(20)), if !buffered_audio_chunks.is_empty() || stream_ended_before_seek_confirmation => {}
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
                                    pending_audio_chunk = None;
                                    buffered_audio_chunks.clear();
                                    buffered_audio_bytes = 0;
                                    stream_ended_before_seek_confirmation = false;
                                    cached_bytes = 0;
                                    observed_cached_bytes = 0;
                                    last_chunk_duration_millis = 0;
                                    cached_duration_millis = reservation
                                        .as_ref()
                                        .map(|value| value.resume_after_millis)
                                        .unwrap_or(0);
                                    next_progress_millis = 5_000;
                                    seek_gate_reported = false;
                                    first_chunk_reported = false;
                                    seek_replacement_deadline = None;
                                    if let Some(value) = reservation.as_ref() {
                                        download_progress_deadline = Some(watchdog_deadline(
                                            stalled_content_key.as_deref().zip(download_progress_deadline),
                                            &value.cache_content_key,
                                            tokio::time::Instant::now(),
                                            DOWNLOAD_PROGRESS_TIMEOUT,
                                        ));
                                        stalled_content_key = Some(value.cache_content_key.clone());
                                    }
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
                                            "stopping stream on slot {} without an active download reservation",
                                            slot_index + 1
                                        ));
                                        let _ = stream_controller.stop().await;
                                    }
                                }
                                Message::StreamEnd(_) => {
                                    publish_slot_stream_state(
                                        &slot_connection_state,
                                        task_connection_generation,
                                        false,
                                    );
                                    let awaiting_confirmation = reservation.as_ref().is_some_and(|value| {
                                        crate::cache::store::slot_reservation_is_current(&slots, slot_index, value)
                                            && (!value.append_after_seek.load(Ordering::Acquire)
                                                || !buffered_audio_chunks.is_empty())
                                            && pending_audio_chunk.is_some()
                                    });
                                    if awaiting_confirmation {
                                        stream_ended_before_seek_confirmation = true;
                                        seek_replacement_deadline = Some(
                                            tokio::time::Instant::now() + SEEK_REPLACEMENT_STREAM_TIMEOUT,
                                        );
                                        continue;
                                    }
                                    if let (Some(value), Some(format), Some((_, data))) = (
                                        reservation.as_ref(),
                                        format.as_ref(),
                                        pending_audio_chunk.take(),
                                    ) {
                                        if crate::cache::store::slot_reservation_is_current(
                                            &slots,
                                            slot_index,
                                            value,
                                        ) && value.append_after_seek.load(Ordering::Acquire) {
                                            let duration = terminal_chunk_duration_millis(
                                                last_chunk_duration_millis,
                                                value.expected_duration_millis,
                                                cached_duration_millis,
                                            );
                                            append_download_chunk(
                                                &slots, &downloads_allowed, &caches, &cache_change_signal,
                                                &cache_touch_sequence, &states, &event_sender,
                                                &download_status_refresh, slot_index, value, format,
                                                duration, &data, &mut cached_bytes,
                                                &mut cached_duration_millis, &mut first_chunk_reported,
                                                &mut next_progress_millis,
                                            );
                                        }
                                    }
                                    let mut released_reservation = false;
                                    if let Some(value) = reservation.as_ref() {
                                        if crate::cache::store::slot_reservation_is_current(
                                            &slots,
                                            slot_index,
                                            value,
                                        ) {
                                            match stream_end_outcome(value.append_after_seek.load(Ordering::Acquire)) {
                                                StreamEndOutcome::Interrupted => {
                                                    retain_interrupted_reservation(
                                                        &slots,
                                                        &caches,
                                                        &cache_change_signal,
                                                        &states,
                                                        &event_sender,
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
                                    if released_reservation
                                        || reservation.as_ref().is_some_and(|value| {
                                            !crate::cache::store::slot_reservation_is_current(
                                                &slots,
                                                slot_index,
                                                value,
                                            )
                                        })
                                    {
                                        reservation = None;
                                    }
                                    format = None;
                                    pending_audio_chunk = None;
                                    buffered_audio_chunks.clear();
                                    buffered_audio_bytes = 0;
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
                                            &event_sender,
                                            slot_index,
                                            value,
                                            );
                                        }
                                    }
                                    reservation = None;
                                    format = None;
                                    pending_audio_chunk = None;
                                    buffered_audio_chunks.clear();
                                    buffered_audio_bytes = 0;
                                    stream_ended_before_seek_confirmation = false;
                                    seek_replacement_deadline = None;
                                }
                                _ => {}
                            }
                        }
                        chunk = audio.recv(), if !stream_ended_before_seek_confirmation && (buffered_audio_chunks.is_empty() || reservation.as_ref().is_some_and(|value| !value.append_after_seek.load(Ordering::Acquire))) => {
                            let Some(chunk) = chunk else { break; };
                            if !downloads_allowed.load(Ordering::Acquire) {
                                continue;
                            }
                            let (Some(value), Some(stream_format)) = (reservation.as_ref(), format.as_ref()) else { continue; };
                            if !crate::cache::store::slot_reservation_is_current(
                                    &slots,
                                    slot_index,
                                    value,
                                )
                            {
                                continue;
                            }
                            let Some((previous_timestamp, previous_data)) = pending_audio_chunk
                                .replace((chunk.timestamp, chunk.data)) else {
                                continue;
                            };
                            let duration = chunk
                                .timestamp
                                .saturating_sub(previous_timestamp)
                                .max(0) as u64
                                / 1_000;
                            last_chunk_duration_millis = duration;
                            if !value.append_after_seek.load(Ordering::Acquire) {
                                let deadline = watchdog_deadline(
                                    seek_confirmation_deadline.as_ref().map(|(lease_id, deadline)| (lease_id.as_str(), *deadline)),
                                    &value.lease_id,
                                    tokio::time::Instant::now(),
                                    SEEK_CONFIRMATION_TIMEOUT,
                                );
                                seek_confirmation_deadline = Some((value.lease_id.clone(), deadline));
                                if !seek_gate_reported {
                                    LOGGER.debug(format!(
                                        "download chunks are buffered until seek confirmation: slot {}, cache key {}",
                                        slot_index + 1,
                                        value.cache_content_key
                                    ));
                                    seek_gate_reported = true;
                                }
                                buffered_audio_bytes = buffered_audio_bytes.saturating_add(previous_data.len());
                                buffered_audio_chunks.push_back((duration, previous_data));
                                if buffered_audio_bytes > MAXIMUM_PENDING_SEEK_AUDIO_BYTES {
                                    downloads_allowed.store(false, Ordering::Release);
                                    retain_interrupted_reservation(
                                        &slots, &caches, &cache_change_signal, &states,
                                        &event_sender, slot_index, value,
                                    );
                                    report_connection_lost_if_current(
                                        &connection_generation, &connection_event,
                                        task_connection_generation, slot_index, "pending seek audio limit",
                                    );
                                    reservation = None;
                                    format = None;
                                    pending_audio_chunk = None;
                                    buffered_audio_chunks.clear();
                                    buffered_audio_bytes = 0;
                                }
                                continue;
                            }
                            let result = append_download_chunk(
                                &slots,
                                &downloads_allowed,
                                &caches,
                                &cache_change_signal,
                                &cache_touch_sequence,
                                &states,
                                &event_sender,
                                &download_status_refresh,
                                slot_index,
                                value,
                                stream_format,
                                duration,
                                &previous_data,
                                &mut cached_bytes,
                                &mut cached_duration_millis,
                                &mut first_chunk_reported,
                                &mut next_progress_millis,
                            );
                            let _ = result;
                        }
                    }
                }
                if connection_generation.load(Ordering::Acquire) == task_connection_generation {
                    downloads_allowed.store(false, Ordering::Release);
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
                        let _ = event_sender.send(DownloaderCommand::ReconcileCacheAndDownloads);
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
                    downloads_allowed.store(false, Ordering::Release);
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
            Arc::clone(&self.downloads_allowed),
            task_connection_generation,
            slot_index,
        )));
        Ok(())
    }

    // Disconnection and reconnection lifecycle.
    // Stops both transports and discards incomplete cache entries owned by their reservations.
    pub(crate) fn disconnect(&mut self) {
        self.downloads_allowed.store(false, Ordering::Release);
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
        self.player_ids = [None, None];
        self.queue_ids = [None, None];
    }

    pub(crate) fn reconnect(&mut self) -> Result<(), String> {
        self.prepare_reconnect();
        self.connect()
    }

    fn restart_for_codec_change(&mut self, config: SendspinDownloaderConfig) -> Result<(), String> {
        LOGGER.info(format!(
            "restarting Sendspin downloader for preferred codec {}",
            config.preferred_codec.as_str()
        ));
        self.prepare_reconnect();
        crate::cache::store::cache_clear_all(&self.caches);
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        self.config = Some(config);
        self.connect()
    }

    fn set_pending_current_seek(&mut self, seconds: i64) {
        self.pending_current_seek_seconds = Some(seconds.max(0));
    }

    // Invalidates active work before recreating the generation-scoped downloader transports.
    fn prepare_reconnect(&mut self) {
        LOGGER.warn("preparing Sendspin downloads for reconnection");
        self.downloads_allowed.store(false, Ordering::Release);
        self.connection_generation.fetch_add(1, Ordering::AcqRel);
        self.cancel_liveness_checks();
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
                    let _ = task.await;
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
        self.queue_ids = [None, None];
        // Keep these identities until connect stops their server-side queue flows.
    }

    // Download plan configuration.
    // Rebuilds downloader state from the authoritative queue snapshot and reconciles both slots.
    pub(crate) fn apply_download_plan(
        &mut self,
        request: SendspinDownloadPlanRequest,
    ) -> Result<(), String> {
        let plan = self.create_download_plan(request)?;
        LOGGER.debug(format!(
            "downloader plan accepted: version {}, {} items",
            plan.version,
            plan.items.len()
        ));
        if let Ok(mut states) = self.cache_states.lock() {
            states.clear();
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
        if let Ok(mut download_plan) = self.download_plan.lock() {
            *download_plan = self.plan.clone();
        }
        if matches!(self.capacity_mode, DownloadCapacityMode::Zero)
            && self.config.as_ref().is_some_and(|config| {
                crate::cache::store::cache_total_bytes(&self.caches).saturating_mul(100)
                    <= config
                        .cache_bytes
                        .saturating_mul(CACHE_CAPACITY_RECOVERY_PERCENT)
            })
        {
            self.set_capacity_mode(DownloadCapacityMode::Dual);
        }
        self.reconcile_cache_and_downloads()
    }

    fn create_download_plan(
        &self,
        request: SendspinDownloadPlanRequest,
    ) -> Result<SendspinDownloadPlan, String> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| "Sendspin downloader is not configured".to_string())?;
        let item_count = request.queue_items.len().max(1);
        let maximum_priority = download_priority_maximum(item_count);
        let current_content_key = request.current_index.and_then(|current_index| {
            request
                .queue_items
                .get(current_index)
                .and_then(|item| cache_content_key_for_item(item, config.preferred_codec).ok())
        });
        let items = request
            .queue_items
            .into_iter()
            .enumerate()
            .map(|(queue_index, item)| {
                let cache_content_key = cache_content_key_for_item(&item, config.preferred_codec)?;
                let expected_duration_millis = item
                    .media_item
                    .duration_seconds()
                    .filter(|duration| *duration > 0)
                    .map(|duration| u64::from(duration).saturating_mul(1_000));
                Ok(SendspinDownloadPlanItem {
                    queue_item_id: item.queue_item_id,
                    original_index: item.original_index,
                    queue_index,
                    priority: download_priority(
                        queue_index,
                        request.current_index,
                        maximum_priority,
                    ),
                    estimated_bytes: estimate_cache_size(
                        expected_duration_millis,
                        config.preferred_codec,
                    ),
                    cache_content_key,
                    expected_duration_millis,
                    media_item: item.media_item,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(SendspinDownloadPlan {
            version: request.queue_revision,
            current_content_key,
            items,
        })
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

    fn record_playback_position(&mut self, content_key: &str, position_millis: u64) {
        let is_current = self
            .plan
            .as_ref()
            .and_then(|plan| plan.current_content_key.as_ref())
            .map(|key| key.as_str() == content_key)
            .unwrap_or(false);
        let Some(target_bytes) = self.config.as_ref().map(|config| {
            config
                .cache_bytes
                .saturating_mul(CACHE_CAPACITY_RECOVERY_PERCENT)
                / 100
        }) else {
            return;
        };
        if matches!(self.capacity_mode, DownloadCapacityMode::Single { .. })
            && is_current
            && crate::cache::store::cache_trim_played_current_to_capacity(
                &self.caches,
                content_key,
                position_millis,
                target_bytes,
            ) > 0
        {
            crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        }
    }

    // Applies capacity mode, admission estimates, and plan priority to the two download slots.
    fn reconcile_cache_and_downloads(&mut self) -> Result<(), String> {
        if !self.downloads_allowed.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(plan) = self.plan.clone() else {
            return Ok(());
        };
        let content_keys = plan
            .items
            .iter()
            .map(|item| item.cache_content_key.as_str().to_string())
            .collect::<Vec<_>>();

        // Remove cache entries that are no longer part of the current download plan.
        self.discard_orphaned_cache_entries(&content_keys);

        // Remove lowest-priority entries until cache capacity is within the recovery target.
        self.enforce_soft_cache_capacity();

        // Restore two download slots only after the item that started single-slot mode completed.
        if let DownloadCapacityMode::Single { completion_key } = &self.capacity_mode {
            if crate::cache::store::cache_complete_duration_millis(&self.caches, completion_key)
                .is_some()
            {
                self.set_capacity_mode(DownloadCapacityMode::Dual);
            }
        }
        // Hold all new downloads until the next plan can recover capacity from zero-slot mode.
        if matches!(self.capacity_mode, DownloadCapacityMode::Zero) {
            return Ok(());
        }

        // Sort incomplete plan items by priority before applying slot and capacity admission limits.
        let mut candidates = plan.items.clone();
        candidates.sort_by(|left, right| right.priority.cmp(&left.priority));
        let Some(config) = self.config.clone() else {
            return Ok(());
        };
        let allowed_slot_count = match self.capacity_mode {
            DownloadCapacityMode::Dual => 2,
            DownloadCapacityMode::Single { .. } => 1,
            DownloadCapacityMode::Zero => 0,
        };
        let mut projected_cache_bytes = crate::cache::store::cache_total_bytes(&self.caches);
        let mut prioritized_items = Vec::with_capacity(allowed_slot_count);

        // Admit only items whose remaining download size fits the projected cache footprint.
        for item in candidates {
            if prioritized_items.len() == allowed_slot_count
                || crate::cache::store::cache_complete_duration_millis(
                    &self.caches,
                    &item.cache_content_key,
                )
                .is_some()
            {
                continue;
            }
            let cached_duration_millis = crate::cache::store::cache_cached_duration_millis(
                &self.caches,
                item.cache_content_key.as_str(),
            )
            .unwrap_or(0);
            let estimated_remaining_bytes = estimate_cache_size(
                remaining_download_duration_millis(
                    item.expected_duration_millis,
                    cached_duration_millis,
                ),
                config.preferred_codec,
            );
            if estimated_remaining_bytes > config.cache_bytes {
                continue;
            }

            // Free lower-priority cached entries when they prevent this higher-priority download.
            if projected_cache_bytes.saturating_add(estimated_remaining_bytes) > config.cache_bytes
            {
                let evicted_active_download = self
                    .evict_lower_priority_cache_entries_for_admission(
                        &plan,
                        &item,
                        estimated_remaining_bytes,
                        config.cache_bytes,
                    );
                projected_cache_bytes = crate::cache::store::cache_total_bytes(&self.caches);
                if evicted_active_download
                    && matches!(self.capacity_mode, DownloadCapacityMode::Dual)
                {
                    self.set_capacity_mode(DownloadCapacityMode::Single {
                        completion_key: item.cache_content_key.as_str().to_string(),
                    });
                }
            }
            if projected_cache_bytes.saturating_add(estimated_remaining_bytes) > config.cache_bytes
            {
                continue;
            }
            projected_cache_bytes = projected_cache_bytes.saturating_add(estimated_remaining_bytes);
            prioritized_items.push(item);
            if matches!(self.capacity_mode, DownloadCapacityMode::Single { .. }) {
                break;
            }
        }

        // A playback-priority change replaces the single-slot target but keeps the slot limit.
        if let (DownloadCapacityMode::Single { completion_key }, Some(item)) =
            (&self.capacity_mode, prioritized_items.first())
        {
            if completion_key != item.cache_content_key.as_str() {
                self.set_capacity_mode(DownloadCapacityMode::Single {
                    completion_key: item.cache_content_key.as_str().to_string(),
                });
            }
        }
        let should_admit_non_current_downloads = self.should_admit_non_current_downloads();
        let prioritized_content_keys = prioritized_items
            .iter()
            .map(|item| item.cache_content_key.as_str().to_string())
            .collect::<Vec<_>>();

        // Stop active reservations that no longer belong to the admitted priority set.
        self.preempt_deprioritized_downloads(&prioritized_content_keys);

        // Mark plan entries removed during preemption as discarded for UI status reporting.
        let discarded = cache_remove_orphaned_entries(&self.caches, &content_keys);
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

        // Start each admitted item only when a slot is free and no identical reservation exists.
        for item in prioritized_items {
            if !should_admit_non_current_downloads
                && plan.current_content_key.as_ref() != Some(&item.cache_content_key)
            {
                continue;
            }
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

    fn discard_orphaned_cache_entries(&mut self, content_keys: &[String]) {
        let discarded = cache_remove_orphaned_entries(&self.caches, content_keys);
        if discarded.is_empty() {
            return;
        }
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

    fn should_admit_non_current_downloads(&self) -> bool {
        let Some(current_content_key) = self
            .plan
            .as_ref()
            .and_then(|plan| plan.current_content_key.as_ref())
            .map(|key| key.as_str())
        else {
            return true;
        };
        let current_cache_is_incomplete =
            crate::cache::store::cache_complete_duration_millis(&self.caches, current_content_key)
                .is_none();
        let current_download_is_active = self
            .slots
            .lock()
            .map(|slots| {
                slots
                    .iter()
                    .flatten()
                    .any(|reservation| reservation.cache_content_key == current_content_key)
            })
            .unwrap_or(false);
        should_admit_non_current_downloads(current_cache_is_incomplete, current_download_is_active)
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
            let cached_duration_millis = crate::cache::store::cache_mark_interrupted(
                &self.caches,
                &reservation.cache_content_key,
            )
            .unwrap_or(0);
            if let Ok(mut states) = self.cache_states.lock() {
                if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                    state.state = CacheStatus::Interrupted;
                    state.cached_duration_millis = cached_duration_millis;
                }
            }
            LOGGER.trace(format!(
                "download retained for resume after playback-priority preemption: slot {}, cache key {}",
                slot_index + 1,
                reservation.cache_content_key
            ));
        }
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);

        let controllers = Arc::clone(&self.controllers);
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

    // Removes lower-priority cache entries that prevent admission of a higher-priority download.
    fn evict_lower_priority_cache_entries_for_admission(
        &mut self,
        plan: &SendspinDownloadPlan,
        candidate: &SendspinDownloadPlanItem,
        estimated_remaining_bytes: usize,
        cache_capacity_bytes: usize,
    ) -> bool {
        let mut eviction_candidates = plan
            .items
            .iter()
            .filter(|item| item.priority < candidate.priority)
            .collect::<Vec<_>>();
        eviction_candidates.sort_by(|left, right| left.priority.cmp(&right.priority));

        let mut evicted_content_keys = Vec::new();
        let mut evicted_active_download = false;
        for item in eviction_candidates {
            if crate::cache::store::cache_total_bytes(&self.caches)
                .saturating_add(estimated_remaining_bytes)
                <= cache_capacity_bytes
            {
                break;
            }
            let content_key = item.cache_content_key.as_str();
            if crate::cache::store::cache_remove_key(&self.caches, content_key) {
                evicted_active_download |= self.current_download_is_active(content_key);
                evicted_content_keys.push(content_key.to_string());
            }
        }
        if evicted_content_keys.is_empty() {
            return false;
        }

        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        self.abort_evicted_downloads(evicted_content_keys);
        evicted_active_download
    }

    // Cache-capacity pressure handling.
    fn enforce_soft_cache_capacity(&mut self) -> Option<String> {
        // Capacity pressure is meaningful only after the downloader received its configuration.
        let Some(cache_capacity_bytes) = self.config.as_ref().map(|config| config.cache_bytes)
        else {
            return None;
        };

        // Leave current reservations untouched while the cache remains below the pressure threshold.
        if !cache_reaches_capacity_pressure_threshold(
            crate::cache::store::cache_total_bytes(&self.caches),
            cache_capacity_bytes,
        ) {
            return None;
        }

        // Eviction priority is defined by the authoritative downloader plan.
        let Some(plan) = self.plan.clone() else {
            return None;
        };

        // Remove orphaned and lowest-priority entries until the cache reaches its recovery target.
        let target_capacity_bytes = cache_capacity_bytes
            .saturating_mul(CACHE_CAPACITY_RECOVERY_PERCENT)
            .saturating_div(100);
        let plan_keys = plan
            .items
            .iter()
            .map(|item| item.cache_content_key.as_str().to_string())
            .collect::<Vec<_>>();
        let mut evicted = cache_remove_orphaned_entries(&self.caches, &plan_keys);
        let mut evicted_active_download = false;
        let mut lowest_priority_items = plan.items.clone();
        lowest_priority_items.sort_by(|left, right| left.priority.cmp(&right.priority));
        for item in lowest_priority_items {
            if crate::cache::store::cache_total_bytes(&self.caches) <= target_capacity_bytes {
                break;
            }
            let content_key = item.cache_content_key.as_str();
            if crate::cache::store::cache_remove_key(&self.caches, content_key) {
                evicted.push(content_key.to_string());
                evicted_active_download |= self.current_download_is_active(content_key);
            }
        }

        // After evicting an active stream, let only the best remaining incomplete item restart.
        if evicted_active_download && matches!(self.capacity_mode, DownloadCapacityMode::Dual) {
            if let Some(completion_key) =
                highest_priority_incomplete_content_key(&plan, &self.caches, &evicted)
            {
                self.set_capacity_mode(DownloadCapacityMode::Single { completion_key });
            }
        }

        // Stop streams whose cache entries were removed before publishing their evicted status.
        self.abort_evicted_downloads(evicted);

        // Disable all slots when eviction cannot relieve the remaining capacity pressure.
        if cache_reaches_capacity_pressure_threshold(
            crate::cache::store::cache_total_bytes(&self.caches),
            cache_capacity_bytes,
        ) {
            self.set_capacity_mode(DownloadCapacityMode::Zero);
            plan.current_content_key.map(|key| key.0)
        } else {
            None
        }
    }

    fn current_download_is_active(&self, current_content_key: &str) -> bool {
        self.slots
            .lock()
            .map(|slots| {
                slots
                    .iter()
                    .flatten()
                    .any(|reservation| reservation.cache_content_key == current_content_key)
            })
            .unwrap_or(false)
    }

    fn set_capacity_mode(&mut self, capacity_mode: DownloadCapacityMode) {
        self.capacity_mode = capacity_mode.clone();
        if let Ok(mut download_capacity_mode) = self.download_capacity_mode.lock() {
            *download_capacity_mode = capacity_mode;
        }
    }

    fn abort_evicted_downloads(&mut self, evicted_content_keys: Vec<String>) {
        if evicted_content_keys.is_empty() {
            return;
        }
        let slots = Arc::clone(&self.slots);
        let controllers = Arc::clone(&self.controllers);
        self.runtime.block_on(stop_evicted_slot_downloads(
            slots,
            controllers,
            evicted_content_keys.clone(),
        ));
        if let Ok(mut states) = self.cache_states.lock() {
            for cache_content_key in evicted_content_keys {
                if let Some(state) = states.get_mut(&cache_content_key) {
                    state.state = CacheStatus::Evicted;
                    state.cached_duration_millis = 0;
                }
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
    fn next_reservation_id(&mut self) -> u64 {
        let reservation_id = self.next_reservation_id;
        self.next_reservation_id = self.next_reservation_id.saturating_add(1);
        reservation_id
    }

    // Reserves a slot, establishes its cache key, and submits or resumes its Music Assistant stream.
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
        let interrupted_resume_position_seconds =
            interrupted_resume_after_millis.map(|duration| duration.saturating_div(1_000) as i64);
        let requested_seek_seconds = if self
            .plan
            .as_ref()
            .and_then(|plan| plan.current_content_key.as_ref())
            == Some(&item.cache_content_key)
        {
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
        let reservation_id = self.next_reservation_id();
        let reservation = {
            let mut slots = self
                .slots
                .lock()
                .map_err(|_| "Sendspin downloader slot lock poisoned".to_string())?;
            let reservation = crate::cache::store::DownloadSlotReservation {
                cache_content_key: item.cache_content_key.as_str().to_string(),
                expected_duration_millis: item.expected_duration_millis,
                resume_after_millis,
                append_after_seek: Arc::new(AtomicBool::new(false)),
                lease_id: format!("slot-{}-{}", slot_index + 1, reservation_id),
            };
            slots[slot_index] = Some(reservation.clone());
            reservation
        };
        crate::cache::store::cache_add_key(
            &self.caches,
            reservation.cache_content_key.clone(),
            reservation.resume_after_millis,
        );
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
            "download request prepared: slot {}, priority {}, queue ID {}, cache key {}",
            slot_index + 1,
            item.priority,
            queue_id_for_log,
            reservation.cache_content_key
        ));
        let media_item = item.media_item.clone();
        let music_assistant_url = config.music_assistant_url.clone();
        let access_token = config.music_assistant_auth_token.clone();
        let slots = Arc::clone(&self.slots);
        let caches = Arc::clone(&self.caches);
        let cache_change_signal = Arc::clone(&self.cache_change_signal);
        let states = Arc::clone(&self.cache_states);
        let event_sender = self.event_sender.clone();
        let connection_event = Arc::clone(&self.connection_event);
        let connection_generation = Arc::clone(&self.connection_generation);
        let downloads_allowed = Arc::clone(&self.downloads_allowed);
        let request_connection_generation = connection_generation.load(Ordering::Acquire);
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
            if let Err(error) = client.player_stop(&player_id).await {
                if download_request_is_current(
                    &slots,
                    slot_index,
                    &reservation,
                    &connection_generation,
                    request_connection_generation,
                ) {
                    LOGGER.error(format!(
                        "failed to stop download slot before play_media: {error}"
                    ));
                    downloads_allowed.store(false, Ordering::Release);
                    retain_interrupted_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        &event_sender,
                        slot_index,
                        &reservation,
                    );
                    report_connection_lost_if_current(
                        &connection_generation,
                        &connection_event,
                        request_connection_generation,
                        slot_index,
                        "player stop failure",
                    );
                }
                return;
            }
            if !download_request_is_current(
                &slots,
                slot_index,
                &reservation,
                &connection_generation,
                request_connection_generation,
            ) {
                return;
            }
            if let Err(error) = client.queue_clear(&queue_id).await {
                if download_request_is_current(
                    &slots,
                    slot_index,
                    &reservation,
                    &connection_generation,
                    request_connection_generation,
                ) {
                    LOGGER.error(format!("failed to clear download slot queue: {error}"));
                    downloads_allowed.store(false, Ordering::Release);
                    retain_interrupted_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        &event_sender,
                        slot_index,
                        &reservation,
                    );
                    report_connection_lost_if_current(
                        &connection_generation,
                        &connection_event,
                        request_connection_generation,
                        slot_index,
                        "queue clear failure",
                    );
                }
                return;
            }
            if !download_request_is_current(
                &slots,
                slot_index,
                &reservation,
                &connection_generation,
                request_connection_generation,
            ) {
                return;
            }
            let result = client
                .queue_play_media(queue_id.clone(), vec![media_item], "replace_next", false)
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
                LOGGER.error(format!("failed to stage download slot media: {error}"));
                downloads_allowed.store(false, Ordering::Release);
                retain_interrupted_reservation(
                    &slots,
                    &caches,
                    &cache_change_signal,
                    &states,
                    &event_sender,
                    slot_index,
                    &reservation,
                );
                report_connection_lost_if_current(
                    &connection_generation,
                    &connection_event,
                    request_connection_generation,
                    slot_index,
                    "queue play_media failure",
                );
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
                if let Err(error) = client.player_seek(player_id, position).await {
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
                    downloads_allowed.store(false, Ordering::Release);
                    // The audio player may still consume cached chunks while the retry is pending.
                    retain_interrupted_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        &event_sender,
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
                if download_request_is_current(
                    &slots,
                    slot_index,
                    &reservation,
                    &connection_generation,
                    request_connection_generation,
                ) && downloads_allowed.load(Ordering::Acquire)
                {
                    reservation.append_after_seek.store(true, Ordering::Release);
                }
            } else if let Err(error) = client.queue_play_index(queue_id, 0).await {
                if download_request_is_current(
                    &slots,
                    slot_index,
                    &reservation,
                    &connection_generation,
                    request_connection_generation,
                ) {
                    LOGGER.error(format!("failed to start download slot: {error}"));
                    downloads_allowed.store(false, Ordering::Release);
                    retain_interrupted_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        &event_sender,
                        slot_index,
                        &reservation,
                    );
                    report_connection_lost_if_current(
                        &connection_generation,
                        &connection_event,
                        request_connection_generation,
                        slot_index,
                        "download seek failure",
                    );
                }
            } else if download_request_is_current(
                &slots,
                slot_index,
                &reservation,
                &connection_generation,
                request_connection_generation,
            ) && downloads_allowed.load(Ordering::Acquire)
            {
                reservation.append_after_seek.store(true, Ordering::Release);
            }
        }));
        Ok(())
    }

    // Cached-content seeking.
    // Replaces the current slot lease before seeking so stale stream chunks cannot reach the cache.
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
        let reservation_id = self.next_reservation_id();
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
                lease_id: format!("slot-{}-{}", slot_index + 1, reservation_id),
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
            tokio::time::timeout(
                SEEK_CONFIRMATION_TIMEOUT,
                seek_download_player(&client, &controller, player_id, position),
            )
            .await
            .map_err(|_| "Sendspin seek command timed out".to_string())?
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
                    &self.event_sender,
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
    ApplyDownloadPlan(SendspinDownloadPlanRequest),
    ReconcileCacheAndDownloads,
    MarkCacheBuffering(String),
    RecordPlaybackPosition {
        content_key: String,
        position_millis: u64,
    },
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
    download_status_refresh: Arc<AtomicBool>,
    confirmed_download_progress: Arc<AtomicU64>,
    connection_event: Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    slots: crate::cache::store::DownloadSlots,
    cache_bytes: Arc<AtomicUsize>,
    download_capacity_mode: Arc<Mutex<DownloadCapacityMode>>,
}

impl SendspinDownloader {
    // Construction and lifecycle commands.
    pub(crate) fn new() -> Self {
        let (commands, receiver) = mpsc::channel();
        let download_plan = Arc::new(Mutex::new(None));
        let download_capacity_mode = Arc::new(Mutex::new(DownloadCapacityMode::Dual));
        let worker = SendspinDownloaderWorker::new(
            commands.clone(),
            Arc::clone(&download_plan),
            Arc::clone(&download_capacity_mode),
        );
        let cache_states = Arc::clone(&worker.cache_states);
        let caches = Arc::clone(&worker.caches);
        let slots = Arc::clone(&worker.slots);
        let download_status_refresh = Arc::clone(&worker.download_status_refresh);
        let confirmed_download_progress = Arc::clone(&worker.confirmed_download_progress);
        let connection_event = Arc::clone(&worker.connection_event);
        let cache_bytes = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || run_downloader_worker(worker, receiver));
        Self {
            commands,
            download_plan,
            cache_states,
            caches,
            download_status_refresh,
            connection_event,
            slots,
            cache_bytes,
            download_capacity_mode,
            confirmed_download_progress,
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

    pub(crate) fn confirmed_download_progress(&self) -> u64 {
        self.confirmed_download_progress.load(Ordering::Acquire)
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
    pub(crate) fn apply_download_plan(
        &self,
        request: SendspinDownloadPlanRequest,
    ) -> Result<(), String> {
        self.send(DownloaderCommand::ApplyDownloadPlan(request))
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
    pub(crate) fn take_download_status_refresh(&self) -> bool {
        self.download_status_refresh.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn take_connection_event(&self) -> Option<DownloaderConnectionEvent> {
        self.connection_event
            .lock()
            .ok()
            .and_then(|mut event| event.take())
    }

    pub(crate) fn download_status(&self) -> (Vec<ActiveDownloadStatus>, usize, usize, String) {
        let states = self.cache_states();
        let plan = self.download_plan();
        let active_content_keys = self
            .slots
            .lock()
            .map(|slots| {
                slots
                    .iter()
                    .flatten()
                    .map(|reservation| reservation.cache_content_key.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let active = plan
            .as_ref()
            .map(|plan| {
                plan.items
                    .iter()
                    .filter_map(|item| {
                        active_content_keys
                            .iter()
                            .any(|content_key| content_key == item.cache_content_key.as_str())
                            .then_some(())?;
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
            self.download_capacity_mode
                .lock()
                .map(|mode| match &*mode {
                    DownloadCapacityMode::Dual => "dual",
                    DownloadCapacityMode::Single { .. } => "single",
                    DownloadCapacityMode::Zero => "zero",
                })
                .unwrap_or("unknown")
                .to_string(),
        )
    }

    pub(crate) fn cache_item_counts(&self, content_keys: &[String]) -> (usize, usize) {
        crate::cache::store::cache_item_counts(&self.caches, content_keys)
    }

    // Cache access commands and queries.
    pub(crate) fn mark_cache_buffering(&self, content_key: &str) -> Result<(), String> {
        self.send(DownloaderCommand::MarkCacheBuffering(
            content_key.to_string(),
        ))
    }

    pub(crate) fn record_playback_position(
        &self,
        content_key: &str,
        position_millis: u64,
    ) -> Result<(), String> {
        self.send(DownloaderCommand::RecordPlaybackPosition {
            content_key: content_key.to_string(),
            position_millis,
        })
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

    // Seeking commands.
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
            DownloaderCommand::ApplyDownloadPlan(request) => worker.apply_download_plan(request),
            DownloaderCommand::ReconcileCacheAndDownloads => worker.reconcile_cache_and_downloads(),
            DownloaderCommand::MarkCacheBuffering(content_key) => {
                worker.mark_cache_buffering(&content_key)
            }
            DownloaderCommand::RecordPlaybackPosition {
                content_key,
                position_millis,
            } => {
                worker.record_playback_position(&content_key, position_millis);
                Ok(())
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
            worker.downloads_allowed.store(false, Ordering::Release);
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
                worker.downloads_allowed.store(false, Ordering::Release);
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
async fn stop_existing_download_players(
    client: &MusicAssistantClient,
    client_ids: &[String; 2],
) -> Result<(), String> {
    let players = client.get_players(true, true).await?;
    for client_id in client_ids {
        if let Some(player_id) = find_download_player_id(&players, client_id) {
            client.player_stop(&player_id).await?;
        }
    }
    Ok(())
}

// Appends a validated stream chunk, publishes cache state, and schedules capacity reconciliation.
fn append_download_chunk(
    slots: &crate::cache::store::DownloadSlots,
    downloads_allowed: &Arc<AtomicBool>,
    caches: &crate::cache::store::EncodedCaches,
    cache_change_signal: &crate::cache::store::CacheChangeSignal,
    cache_touch_sequence: &Arc<AtomicU64>,
    states: &Arc<Mutex<HashMap<String, CacheItemState>>>,
    event_sender: &Sender<DownloaderCommand>,
    download_status_refresh: &Arc<AtomicBool>,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
    format: &AudioFormat,
    duration_millis: u64,
    data: &[u8],
    cached_bytes: &mut usize,
    cached_duration_millis: &mut u64,
    first_chunk_reported: &mut bool,
    next_progress_millis: &mut u64,
) -> crate::cache::store::CacheAppendResult {
    let Ok(mut slots) = slots.lock() else {
        return crate::cache::store::CacheAppendResult::IgnoredStaleLease;
    };
    if !downloads_allowed.load(Ordering::Acquire) {
        return crate::cache::store::CacheAppendResult::IgnoredConnectionLost;
    }
    if !slots[slot_index]
        .as_ref()
        .map(|current| current.lease_id == reservation.lease_id)
        .unwrap_or(false)
    {
        return crate::cache::store::CacheAppendResult::IgnoredStaleLease;
    }
    if !reservation.append_after_seek.load(Ordering::Acquire) {
        return crate::cache::store::CacheAppendResult::IgnoredPendingSeek;
    }
    let result = crate::cache::store::cache_append_encoded_chunk(
        caches,
        cache_touch_sequence,
        reservation,
        format,
        *cached_duration_millis,
        duration_millis,
        data,
    );
    if result == crate::cache::store::CacheAppendResult::IgnoredOverlap {
        *cached_duration_millis = cached_duration_millis.saturating_add(duration_millis);
        return result;
    }
    if !matches!(
        result,
        crate::cache::store::CacheAppendResult::Appended
            | crate::cache::store::CacheAppendResult::Completed
    ) {
        return result;
    }

    if !*first_chunk_reported {
        LOGGER.trace(format!(
            "download cache write started: slot {}, cache key {}",
            slot_index + 1,
            reservation.cache_content_key
        ));
        *first_chunk_reported = true;
    }
    *cached_bytes = cached_bytes.saturating_add(data.len());
    *cached_duration_millis = cached_duration_millis.saturating_add(duration_millis);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    if let Ok(mut states) = states.lock() {
        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
            state.state = CacheStatus::Downloading;
            state.cached_duration_millis =
                state.cached_duration_millis.saturating_add(duration_millis);
        }
    }
    if result == crate::cache::store::CacheAppendResult::Completed {
        slots[slot_index] = None;
        crate::cache::store::notify_cache_changed(cache_change_signal);
        if let Ok(mut states) = states.lock() {
            if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                state.state = CacheStatus::Complete;
            }
        }
        LOGGER.trace(format!(
            "download complete from stream timestamps: slot {}, cache key {}, {} ms, {} bytes",
            slot_index + 1,
            reservation.cache_content_key,
            *cached_duration_millis,
            *cached_bytes
        ));
    }
    if *cached_duration_millis >= *next_progress_millis {
        download_status_refresh.store(true, Ordering::Release);
        LOGGER.trace(format!(
            "download progress: slot {}, cache key {}, {} ms, {} bytes",
            slot_index + 1,
            reservation.cache_content_key,
            *cached_duration_millis,
            *cached_bytes
        ));
        *next_progress_millis = cached_duration_millis
            .saturating_div(5_000)
            .saturating_add(1)
            .saturating_mul(5_000);
    }
    let _ = event_sender.send(DownloaderCommand::ReconcileCacheAndDownloads);
    result
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
    event_sender: &Sender<DownloaderCommand>,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
) {
    if !crate::cache::store::slot_release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::cache_discard_incomplete(caches, &reservation.cache_content_key);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    let _ = event_sender.send(DownloaderCommand::ReconcileCacheAndDownloads);
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
    event_sender: &Sender<DownloaderCommand>,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
) {
    if !crate::cache::store::slot_release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::cache_mark_interrupted(caches, &reservation.cache_content_key);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    let _ = event_sender.send(DownloaderCommand::ReconcileCacheAndDownloads);
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

fn download_priority_maximum(item_count: usize) -> u32 {
    let mut exponent = 0_u32;
    let mut value = item_count;
    while value >= 10 {
        value /= 10;
        exponent = exponent.saturating_add(1);
    }
    10_u32.saturating_pow(exponent.saturating_add(2))
}

fn download_priority(queue_index: usize, current_index: Option<usize>, maximum: u32) -> u32 {
    let Some(current_index) = current_index else {
        return maximum.saturating_sub(queue_index as u32);
    };
    if queue_index == current_index {
        return maximum;
    }
    if queue_index == current_index.saturating_add(1) {
        return maximum.saturating_mul(9).saturating_div(10);
    }
    if current_index > 0 && queue_index == current_index - 1 {
        return maximum.saturating_mul(8).saturating_div(10);
    }
    if queue_index > current_index {
        return maximum
            .saturating_mul(7)
            .saturating_div(10)
            .saturating_sub((queue_index - current_index - 2) as u32);
    }
    maximum
        .saturating_div(2)
        .saturating_sub((current_index - queue_index - 1) as u32)
}

fn cache_content_key_for_item(
    item: &SendspinDownloadQueueItem,
    codec: SendspinAudioCodec,
) -> Result<CacheContentKey, String> {
    let fields = item.media_item.fields();
    let provider = fields.provider.as_str().trim();
    let item_id = fields.item_id.as_str().trim();
    if provider.is_empty() || item_id.is_empty() {
        return Err(format!(
            "queue item {} has no Music Assistant provider or item ID",
            item.queue_item_id.as_str()
        ));
    }
    Ok(CacheContentKey(format!(
        "{provider}:{item_id}:{}:48000:16:2",
        codec.as_str()
    )))
}

fn estimate_cache_size(duration_millis: Option<u64>, codec: SendspinAudioCodec) -> usize {
    let Some(duration_millis) = duration_millis else {
        return usize::MAX;
    };
    let bytes_per_second = match codec {
        SendspinAudioCodec::Pcm | SendspinAudioCodec::Flac => 48_000_u64 * 2 * 2,
        SendspinAudioCodec::Opus => 24_000,
    };
    duration_millis
        .saturating_mul(bytes_per_second)
        .saturating_div(1_000)
        .min(usize::MAX as u64) as usize
}

fn remaining_download_duration_millis(
    expected_duration_millis: Option<u64>,
    cached_duration_millis: u64,
) -> Option<u64> {
    expected_duration_millis.map(|expected| expected.saturating_sub(cached_duration_millis))
}

fn highest_priority_incomplete_content_key(
    plan: &SendspinDownloadPlan,
    caches: &crate::cache::store::EncodedCaches,
    evicted_content_keys: &[String],
) -> Option<String> {
    plan.items
        .iter()
        .filter(|item| {
            !evicted_content_keys
                .iter()
                .any(|key| key == item.cache_content_key.as_str())
        })
        .filter(|item| {
            crate::cache::store::cache_complete_duration_millis(caches, &item.cache_content_key)
                .is_none()
        })
        .max_by(|left, right| left.priority.cmp(&right.priority))
        .map(|item| item.cache_content_key.as_str().to_string())
}

fn cache_remove_orphaned_entries(
    caches: &crate::cache::store::EncodedCaches,
    plan_content_keys: &[String],
) -> Vec<String> {
    crate::cache::store::cache_keys(caches)
        .into_iter()
        .filter(|content_key| !plan_content_keys.contains(content_key))
        .filter(|content_key| crate::cache::store::cache_remove_key(caches, content_key))
        .collect()
}

fn normalize_client_id(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamEndOutcome {
    AwaitingSeekStream,
    Interrupted,
}

fn stream_end_outcome(seek_completed: bool) -> StreamEndOutcome {
    if !seek_completed {
        StreamEndOutcome::AwaitingSeekStream
    } else {
        StreamEndOutcome::Interrupted
    }
}

fn watchdog_deadline(
    previous: Option<(&str, tokio::time::Instant)>,
    identity: &str,
    now: tokio::time::Instant,
    timeout: std::time::Duration,
) -> tokio::time::Instant {
    previous
        .filter(|(previous_identity, _)| *previous_identity == identity)
        .map(|(_, deadline)| deadline)
        .unwrap_or(now + timeout)
}

fn terminal_chunk_duration_millis(
    last_chunk_duration_millis: u64,
    expected_duration_millis: Option<u64>,
    current_position_millis: u64,
) -> u64 {
    expected_duration_millis
        .map(|expected| {
            expected
                .saturating_sub(current_position_millis)
                .min(last_chunk_duration_millis)
        })
        .unwrap_or(last_chunk_duration_millis)
}

fn cache_reaches_capacity_pressure_threshold(cache_used_bytes: usize, cache_bytes: usize) -> bool {
    cache_bytes > 0
        && cache_used_bytes.saturating_mul(100)
            >= cache_bytes.saturating_mul(CACHE_CAPACITY_PRESSURE_PERCENT)
}

fn should_admit_non_current_downloads(
    current_cache_is_incomplete: bool,
    current_download_is_active: bool,
) -> bool {
    !current_cache_is_incomplete || !current_download_is_active
}

async fn run_slot_liveness_check(
    sender: WsSender,
    connection_event: Arc<Mutex<Option<DownloaderConnectionEvent>>>,
    connection_generation: Arc<AtomicU64>,
    downloads_allowed: Arc<AtomicBool>,
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
        if connection_generation.load(Ordering::Acquire) == task_connection_generation {
            downloads_allowed.store(false, Ordering::Release);
        }
        report_connection_lost_if_current(
            &connection_generation,
            &connection_event,
            task_connection_generation,
            slot_index,
            "ping timeout",
        );
        if connection_generation.load(Ordering::Acquire) == task_connection_generation {
            downloads_allowed.store(false, Ordering::Release);
        }
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
        append_download_chunk, cache_reaches_capacity_pressure_threshold, download_client_id,
        download_client_name, download_request_is_current, estimate_cache_size,
        normalize_client_id, publish_connection_event, remaining_download_duration_millis,
        report_connection_lost_if_current, retain_interrupted_reservation,
        should_admit_non_current_downloads, stream_end_outcome, terminal_chunk_duration_millis,
        watchdog_deadline, DownloaderConnectionEvent, DownloaderConnectionStatus, StreamEndOutcome,
        DOWNLOAD_PROGRESS_TIMEOUT, SEEK_CONFIRMATION_TIMEOUT,
    };
    use crate::cache::store::{self, CacheAppendResult, DownloadSlotReservation};
    use crate::sendspin::models::SendspinAudioCodec;
    use ::sendspin::audio::{AudioFormat, Codec};
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
    fn stream_end_waits_for_a_pending_seek_or_marks_the_download_interrupted() {
        assert_eq!(
            stream_end_outcome(false),
            StreamEndOutcome::AwaitingSeekStream
        );
        assert_eq!(stream_end_outcome(true), StreamEndOutcome::Interrupted);
    }

    #[test]
    fn repeated_stream_starts_do_not_extend_the_download_stall_deadline() {
        let now = tokio::time::Instant::now();
        let original = watchdog_deadline(None, "track", now, DOWNLOAD_PROGRESS_TIMEOUT);

        assert_eq!(
            watchdog_deadline(
                Some(("track", original)),
                "track",
                now + DOWNLOAD_PROGRESS_TIMEOUT,
                DOWNLOAD_PROGRESS_TIMEOUT
            ),
            original,
        );
        assert_eq!(
            watchdog_deadline(
                Some(("track", original)),
                "next",
                now,
                DOWNLOAD_PROGRESS_TIMEOUT
            ),
            now + DOWNLOAD_PROGRESS_TIMEOUT,
        );
    }

    #[test]
    fn repeated_seek_chunks_do_not_extend_the_lease_deadline() {
        let now = tokio::time::Instant::now();
        let original = watchdog_deadline(None, "lease-1", now, SEEK_CONFIRMATION_TIMEOUT);

        assert_eq!(
            watchdog_deadline(
                Some(("lease-1", original)),
                "lease-1",
                now + SEEK_CONFIRMATION_TIMEOUT,
                SEEK_CONFIRMATION_TIMEOUT
            ),
            original,
        );
        assert_eq!(
            watchdog_deadline(
                Some(("lease-1", original)),
                "lease-2",
                now,
                SEEK_CONFIRMATION_TIMEOUT
            ),
            now + SEEK_CONFIRMATION_TIMEOUT,
        );
    }

    #[test]
    fn terminal_chunk_does_not_claim_the_entire_missing_track_duration() {
        assert_eq!(terminal_chunk_duration_millis(20, Some(60_000), 5_000), 20);
        assert_eq!(terminal_chunk_duration_millis(20, Some(5_010), 5_000), 10);
        assert_eq!(terminal_chunk_duration_millis(0, Some(60_000), 5_000), 0);
    }

    #[test]
    fn recognizes_capacity_pressure_at_ninety_five_percent_cache_usage() {
        assert!(!cache_reaches_capacity_pressure_threshold(94, 100));
        assert!(cache_reaches_capacity_pressure_threshold(95, 100));
        assert!(cache_reaches_capacity_pressure_threshold(100, 100));
        assert!(!cache_reaches_capacity_pressure_threshold(0, 0));
    }

    #[test]
    fn does_not_admit_new_prefetch_while_current_download_is_incomplete_and_active() {
        assert!(!should_admit_non_current_downloads(true, true));
        assert!(should_admit_non_current_downloads(false, true));
        assert!(should_admit_non_current_downloads(true, false));
    }

    #[test]
    fn estimates_only_the_remaining_bytes_for_a_partially_cached_download() {
        let remaining_duration_millis = remaining_download_duration_millis(Some(10_000), 4_000);

        assert_eq!(remaining_duration_millis, Some(6_000));
        assert_eq!(
            estimate_cache_size(remaining_duration_millis, SendspinAudioCodec::Pcm),
            1_152_000
        );
    }

    #[test]
    fn superseded_download_request_cannot_modify_direct_seek_gate() {
        let reservation = DownloadSlotReservation {
            cache_content_key: "track".to_string(),
            expected_duration_millis: None,
            resume_after_millis: 2_000,
            append_after_seek: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            lease_id: "slot-1-old".to_string(),
        };
        let slots = Arc::new(Mutex::new([Some(reservation.clone()), None]));
        let generation = Arc::new(std::sync::atomic::AtomicU64::new(1));
        assert!(download_request_is_current(
            &slots,
            0,
            &reservation,
            &generation,
            1
        ));

        slots.lock().unwrap()[0].as_mut().unwrap().lease_id = "slot-1-new".to_string();
        assert!(!download_request_is_current(
            &slots,
            0,
            &reservation,
            &generation,
            1
        ));
        generation.store(2, Ordering::Release);
        assert!(!download_request_is_current(
            &slots,
            0,
            &reservation,
            &generation,
            1
        ));
        assert!(!reservation.append_after_seek.load(Ordering::Acquire));
    }

    #[test]
    fn resumed_chunks_commit_only_after_seek_and_stop_after_lost() {
        let reservation = DownloadSlotReservation {
            cache_content_key: "track".to_string(),
            expected_duration_millis: None,
            resume_after_millis: 3_000,
            append_after_seek: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            lease_id: "slot-1-1".to_string(),
        };
        let format = AudioFormat {
            codec: Codec::Pcm,
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec_header: None,
        };
        let slots = Arc::new(Mutex::new([Some(reservation.clone()), None]));
        let downloads_allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let caches = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let signal = store::new_cache_change_signal();
        let sequence = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let states = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let (commands, _) = std::sync::mpsc::channel();
        let refresh = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut cached_bytes = 0;
        let mut stream_position_millis = reservation.resume_after_millis;
        let mut first_chunk_reported = false;
        let mut next_progress_millis = 5_000;

        assert!(!reservation.append_after_seek.load(Ordering::Acquire));
        store::cache_add_key(&caches, "track".to_string(), 0);
        assert_eq!(
            store::cache_append_encoded_chunk(
                &caches,
                &sequence,
                &reservation,
                &format,
                0,
                3_500,
                b"old"
            ),
            CacheAppendResult::Appended
        );
        store::cache_mark_interrupted(&caches, "track");
        let mut append = |duration_millis, data: &[u8]| {
            append_download_chunk(
                &slots,
                &downloads_allowed,
                &caches,
                &signal,
                &sequence,
                &states,
                &commands,
                &refresh,
                0,
                &reservation,
                &format,
                duration_millis,
                data,
                &mut cached_bytes,
                &mut stream_position_millis,
                &mut first_chunk_reported,
                &mut next_progress_millis,
            )
        };

        assert_eq!(
            append(250, b"unconfirmed"),
            CacheAppendResult::IgnoredPendingSeek
        );
        assert_eq!(
            store::cache_cached_duration_millis(&caches, "track"),
            Some(3_500)
        );
        reservation.append_after_seek.store(true, Ordering::Release);
        assert_eq!(append(250, b"inside"), CacheAppendResult::IgnoredOverlap);
        assert_eq!(append(750, b"crossing"), CacheAppendResult::Appended);
        drop(append);
        assert_eq!(stream_position_millis, 4_000);
        assert_eq!(
            store::cache_cached_duration_millis(&caches, "track"),
            Some(4_000)
        );
        assert_eq!(
            store::cache_read_chunk_with_duration(&caches, "track", 1)
                .unwrap()
                .0
                .unwrap()
                .0
                .as_ref(),
            b"crossing"
        );
        downloads_allowed.store(false, Ordering::Release);
        assert_eq!(
            append_download_chunk(
                &slots,
                &downloads_allowed,
                &caches,
                &signal,
                &sequence,
                &states,
                &commands,
                &refresh,
                0,
                &reservation,
                &format,
                250,
                b"lost",
                &mut cached_bytes,
                &mut stream_position_millis,
                &mut first_chunk_reported,
                &mut next_progress_millis,
            ),
            CacheAppendResult::IgnoredConnectionLost
        );
        retain_interrupted_reservation(
            &slots,
            &caches,
            &signal,
            &states,
            &commands,
            0,
            &reservation,
        );
        downloads_allowed.store(true, Ordering::Release);
        assert_eq!(
            append_download_chunk(
                &slots,
                &downloads_allowed,
                &caches,
                &signal,
                &sequence,
                &states,
                &commands,
                &refresh,
                0,
                &reservation,
                &format,
                250,
                b"late",
                &mut cached_bytes,
                &mut stream_position_millis,
                &mut first_chunk_reported,
                &mut next_progress_millis,
            ),
            CacheAppendResult::IgnoredStaleLease
        );
        assert_eq!(
            store::cache_cached_duration_millis(&caches, "track"),
            Some(4_000)
        );
    }
}
