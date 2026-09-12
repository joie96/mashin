use super::connection::{audio_format_from_stream, player_support};
use super::models::{CacheItemState, DownloadPlan, DownloadPlanItem};
use crate::cache::store::CacheRetentionPolicy;
use crate::logging::Logger;
use ::sendspin::audio::AudioFormat;
use ::sendspin::protocol::client::{ConnectionGuard, Controller};
use ::sendspin::protocol::messages::{GoodbyeReason, Message, PlayerState};
use ::sendspin::ProtocolClientBuilder;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

static LOGGER: Logger = Logger::new(module_path!());

const CACHE_COMPLETION_TOLERANCE_MILLIS: u64 = 500;
const CACHE_DOWNLOAD_PAUSE_PERCENT: usize = 95;
const DOWNLOAD_KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);
const DOWNLOAD_KEEPALIVE_PLAY_DURATION: std::time::Duration = std::time::Duration::from_secs(1);
const RESUME_SEEK_PREROLL_MILLIS: u64 = 3_000;

#[derive(Clone)]
pub(crate) struct SendspinDownloaderConfig {
    pub(crate) sendspin_url: String,
    pub(crate) music_assistant_url: String,
    pub(crate) music_assistant_auth_token: Option<String>,
    pub(crate) client_id: String,
    pub(crate) client_name: String,
    pub(crate) buffer_capacity_bytes: u64,
    pub(crate) cache_capacity_bytes: usize,
    pub(crate) preferred_codec: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveDownloadStatus {
    pub(crate) queue_item_id: String,
    pub(crate) cached_duration_millis: u64,
    pub(crate) expected_duration_millis: Option<u64>,
}

struct SendspinDownloaderWorker {
    runtime: Runtime,
    config: Option<SendspinDownloaderConfig>,
    plan: Option<DownloadPlan>,
    cache_states: Arc<Mutex<HashMap<String, CacheItemState>>>,
    caches: crate::cache::store::EncodedCaches,
    cache_change_signal: crate::cache::store::CacheChangeSignal,
    retention_policy: Arc<Mutex<CacheRetentionPolicy>>,
    cache_sequence: Arc<std::sync::atomic::AtomicU64>,
    download_window_refresh: Arc<AtomicBool>,
    download_status_refresh: Arc<AtomicBool>,
    connection_lost: Arc<AtomicBool>,
    connection_ready: Arc<AtomicBool>,
    slots: crate::cache::store::DownloadSlots,
    queue_ids: [Option<String>; 2],
    controllers: Arc<Mutex<[Option<Controller>; 2]>>,
    guards: [Option<ConnectionGuard>; 2],
    io_tasks: [Option<JoinHandle<()>>; 2],
    keepalive_tasks: [Option<JoinHandle<()>>; 2],
}

impl SendspinDownloaderWorker {
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
            cache_sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            download_window_refresh: Arc::new(AtomicBool::new(false)),
            download_status_refresh: Arc::new(AtomicBool::new(false)),
            connection_lost: Arc::new(AtomicBool::new(false)),
            connection_ready: Arc::new(AtomicBool::new(false)),
            slots: Arc::new(Mutex::new([None, None])),
            queue_ids: [None, None],
            controllers: Arc::new(Mutex::new([None, None])),
            guards: [None, None],
            io_tasks: [None, None],
            keepalive_tasks: [None, None],
        }
    }

    pub(crate) fn configure(&mut self, config: SendspinDownloaderConfig) {
        LOGGER.debug(format!(
            "downloader configured: preferred codec {}, cache capacity {} bytes",
            config.preferred_codec, config.cache_capacity_bytes
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
        self.connect_slot(0)?;
        self.connect_slot(1)?;
        let config = self
            .config
            .clone()
            .ok_or_else(|| "Sendspin downloader is not configured".to_string())?;
        let endpoint = format!("{}/api", config.music_assistant_url.trim_end_matches('/'));
        let client_ids = [
            download_client_id(&config.client_id, 1),
            download_client_id(&config.client_id, 2),
        ];
        let queue_ids = self.runtime.block_on(resolve_queue_ids(
            &endpoint,
            config.music_assistant_auth_token.as_deref(),
            &client_ids,
        ))?;
        self.runtime.block_on(disable_queue_autoplay(
            &endpoint,
            config.music_assistant_auth_token.as_deref(),
            &queue_ids,
        ))?;
        LOGGER.info(format!(
            "resolved Music Assistant queue IDs and disabled autoplay for download slots: {}, {}",
            queue_ids[0], queue_ids[1]
        ));
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
        let support = player_support(&config.preferred_codec, config.buffer_capacity_bytes);
        let slots = Arc::clone(&self.slots);
        let caches = Arc::clone(&self.caches);
        let states = Arc::clone(&self.cache_states);
        let cache_change_signal = Arc::clone(&self.cache_change_signal);
        let retention_policy = Arc::clone(&self.retention_policy);
        let controllers = Arc::clone(&self.controllers);
        let sequence = Arc::clone(&self.cache_sequence);
        let download_window_refresh = Arc::clone(&self.download_window_refresh);
        let download_status_refresh = Arc::clone(&self.download_status_refresh);
        let connection_lost = Arc::clone(&self.connection_lost);
        let capacity = config.cache_capacity_bytes;
        let (controller, guard, task) = self.runtime.block_on(async move {
            let client = ProtocolClientBuilder::builder()
                .client_id(client_id)
                .name(client_name)
                .player_v1_support(support)
                .initial_player_state(PlayerState::default())
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
            let task = tokio::spawn(async move {
                let mut format = None;
                let mut reservation = None;
                let mut last_timestamp = None;
                let mut cached_bytes = 0_usize;
                let mut cached_duration_millis = 0_u64;
                let mut next_progress_millis = 5_000_u64;
                let mut hard_limit_reported = false;
                loop {
                    tokio::select! {
                        message = messages.recv() => {
                            let Some(message) = message else { break; };
                            match message {
                                Message::StreamStart(start) => {
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
                                    if let Some(format) = format.as_ref() {
                                        LOGGER.info(format!(
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
                                    }
                                }
                                Message::StreamEnd(_) => {
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
                                }
                                Message::StreamClear(_) => {
                                    LOGGER.warn(format!("stream cleared on slot {}", slot_index + 1));
                                    if let Some(value) = reservation.as_ref() {
                                        release_incomplete_reservation(
                                            &slots,
                                            &caches,
                                            &cache_change_signal,
                                            &states,
                                            slot_index,
                                            value,
                                        );
                                    }
                                    reservation = None;
                                    format = None;
                                    last_timestamp = None;
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
                                continue;
                            }
                            if crate::cache::store::slot_reservation_is_current(&slots, slot_index, reservation) {
                                let policy = retention_policy.lock().map(|policy| policy.clone()).unwrap_or_default();
                                let evicted = crate::cache::store::cache_evict_to_fit(
                                    &caches,
                                    &policy,
                                    capacity,
                                    chunk.data.len(),
                                );
                                if !evicted.is_empty() {
                                    LOGGER.trace(format!(
                                        "cache eviction on slot {}: {} item(s) for {} incoming bytes",
                                        slot_index + 1,
                                        evicted.len(),
                                        chunk.data.len()
                                    ));
                                }
                                stop_evicted_slot_downloads(
                                    Arc::clone(&slots),
                                    Arc::clone(&controllers),
                                    evicted.clone(),
                                )
                                .await;
                                if let Ok(mut states) = states.lock() {
                                    for cache_content_key in evicted {
                                        if let Some(state) = states.get_mut(&cache_content_key) {
                                            state.state = "evicted".to_string();
                                        }
                                    }
                                }
                            }
                            if !crate::cache::store::slot_reservation_is_current(&slots, slot_index, reservation) {
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
                                cached_bytes = cached_bytes.saturating_add(chunk.data.len());
                                cached_duration_millis = cached_duration_millis.saturating_add(duration);
                                crate::cache::store::notify_cache_changed(&cache_change_signal);
                                if let Ok(mut states) = states.lock() {
                                    if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                                        state.state = "downloading".to_string();
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
                                                state.state = "complete".to_string();
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
                                    "cache capacity reached: slot {}, cache key {}, rejecting further chunks",
                                    slot_index + 1,
                                    reservation.cache_content_key
                                ));
                                hard_limit_reported = true;
                            }
                        }
                    }
                }
                if let Some(value) = reservation {
                    if crate::cache::store::slot_reservation_is_current(&slots, slot_index, &value) {
                        crate::cache::store::cache_mark_corrupted(
                            &caches,
                            &value.cache_content_key,
                        );
                        crate::cache::store::notify_cache_changed(&cache_change_signal);
                        if let Ok(mut states) = states.lock() {
                            if let Some(state) = states.get_mut(&value.cache_content_key) {
                                state.state = "corrupted".to_string();
                            }
                        }
                        connection_lost.store(true, Ordering::Release);
                    }
                }
            });
            Ok::<_, String>((controller, connection.guard, task))
        })?;
        if let Ok(mut controllers) = self.controllers.lock() {
            controllers[slot_index] = Some(controller);
        }
        self.guards[slot_index] = Some(guard);
        self.io_tasks[slot_index] = Some(task);
        LOGGER.info(format!(
            "Sendspin downloader slot {} connected",
            slot_index + 1
        ));
        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        LOGGER.info("disconnecting Sendspin downloader slots");
        let active_reservations = self
            .slots
            .lock()
            .map(|slots| slots.iter().flatten().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for reservation in active_reservations {
            reservation
                .warm_keepalive_cancel
                .store(true, Ordering::Relaxed);
            crate::cache::store::cache_discard_incomplete(
                &self.caches,
                &reservation.cache_content_key,
            );
        }
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        self.runtime.block_on(async {
            for task in self.io_tasks.iter_mut() {
                if let Some(task) = task.take() {
                    task.abort();
                }
            }
            for task in self.keepalive_tasks.iter_mut() {
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
        self.queue_ids = [None, None];
    }

    pub(crate) fn reconnect(&mut self) -> Result<(), String> {
        self.pause_for_reconnect();
        self.connect()
    }

    fn pause_for_reconnect(&mut self) {
        LOGGER.warn("pausing Sendspin downloads for reconnection");
        self.cancel_warm_keepalives();
        let reservations = self
            .slots
            .lock()
            .map(|slots| slots.iter().flatten().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for reservation in reservations {
            let cached_duration = crate::cache::store::cache_mark_corrupted(
                &self.caches,
                &reservation.cache_content_key,
            )
            .unwrap_or(0);
            if let Ok(mut states) = self.cache_states.lock() {
                if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                    state.state = "corrupted".to_string();
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
        self.queue_ids = [None, None];
    }

    pub(crate) fn apply_download_plan(&mut self, plan_json: &str) -> Result<(), String> {
        let plan = parse_plan(plan_json)?;
        self.cancel_warm_keepalives();
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
                    item.cache_content_key.clone(),
                    CacheItemState {
                        queue_item_id: item.queue_item_id.clone(),
                        state: "queued".to_string(),
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
        item.state = "buffering".to_string();
        Ok(())
    }

    pub(crate) fn update_playback_window(
        &mut self,
        current: Option<String>,
        upcoming: Vec<String>,
        recently_played: Vec<String>,
    ) {
        let next = upcoming.first().cloned();
        let mut protected = current.iter().cloned().collect::<Vec<_>>();
        if let Some(next) = next.as_ref() {
            protected.push(next.clone());
        }
        if let Ok(mut policy) = self.retention_policy.lock() {
            *policy = CacheRetentionPolicy {
                current,
                protected,
                prefetch: upcoming.into_iter().skip(1).collect(),
                recently_played,
            };
        }
        self.evict_to_capacity(0);
    }

    pub(crate) fn ensure_download_window(&mut self, content_keys: &[String]) -> Result<(), String> {
        self.evict_to_capacity(0);
        self.pause_downloads_near_capacity();
        let Some(plan) = self.plan.as_ref() else {
            return Ok(());
        };
        let candidates = content_keys
            .iter()
            .filter_map(|content_key| {
                plan.items
                    .iter()
                    .find(|item| item.cache_content_key == *content_key)
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
            .map(|item| item.cache_content_key.clone())
            .collect::<Vec<_>>();
        self.preempt_deprioritized_downloads(&prioritized_content_keys);
        let discarded = crate::cache::store::cache_discard_corrupted_outside_window(
            &self.caches,
            content_keys,
        );
        if !discarded.is_empty() {
            crate::cache::store::notify_cache_changed(&self.cache_change_signal);
            if let Ok(mut states) = self.cache_states.lock() {
                for content_key in discarded {
                    if let Some(state) = states.get_mut(&content_key) {
                        state.state = "discarded".to_string();
                        state.cached_duration_millis = 0;
                    }
                }
            }
        }

        for item in candidates {
            if cache_reaches_download_pause_threshold(
                crate::cache::store::cache_total_bytes(&self.caches),
                config.cache_capacity_bytes,
            ) {
                self.pause_downloads_near_capacity();
                break;
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
                    .any(|slot| slot.cache_content_key == item.cache_content_key)
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
            reservation
                .warm_keepalive_cancel
                .store(true, Ordering::Relaxed);
            crate::cache::store::cache_discard_incomplete(
                &self.caches,
                &reservation.cache_content_key,
            );
            if let Ok(mut states) = self.cache_states.lock() {
                if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                    state.state = "queued".to_string();
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

    fn evict_to_capacity(&mut self, incoming_bytes: usize) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let policy = self
            .retention_policy
            .lock()
            .map(|policy| policy.clone())
            .unwrap_or_default();
        let evicted = crate::cache::store::cache_evict_to_fit(
            &self.caches,
            &policy,
            config.cache_capacity_bytes,
            incoming_bytes,
        );
        if !evicted.is_empty() {
            let slots = Arc::clone(&self.slots);
            let controllers = Arc::clone(&self.controllers);
            self.runtime.spawn(stop_evicted_slot_downloads(
                slots,
                controllers,
                evicted.clone(),
            ));
        }
        if let Ok(mut states) = self.cache_states.lock() {
            for cache_content_key in evicted {
                if let Some(state) = states.get_mut(&cache_content_key) {
                    state.state = "evicted".to_string();
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
            config.cache_capacity_bytes,
        ) {
            return;
        }

        let reservations = self
            .slots
            .lock()
            .map(|slots| slots.clone())
            .unwrap_or([None, None]);
        for (slot_index, reservation) in reservations.iter().enumerate() {
            let Some(reservation) = reservation else {
                continue;
            };
            if self.keepalive_tasks[slot_index].is_some() {
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
            let cancel = Arc::clone(&reservation.warm_keepalive_cancel);
            let cache_content_key = reservation.cache_content_key.clone();
            let lease = reservation.clone();
            let slots = Arc::clone(&self.slots);
            self.keepalive_tasks[slot_index] = Some(self.runtime.spawn(async move {
                if !warm_keepalive_is_current(&slots, slot_index, &lease) {
                    return;
                }
                if controller.pause().await.is_err() {
                    return;
                }
                LOGGER.trace(format!(
                    "download paused near cache capacity: slot {}, cache key {}",
                    slot_index + 1,
                    cache_content_key
                ));
                loop {
                    tokio::time::sleep(DOWNLOAD_KEEPALIVE_INTERVAL).await;
                    if cancel.load(Ordering::Relaxed)
                        || !warm_keepalive_is_current(&slots, slot_index, &lease)
                    {
                        break;
                    }
                    if controller.play().await.is_err() {
                        break;
                    }
                    tokio::time::sleep(DOWNLOAD_KEEPALIVE_PLAY_DURATION).await;
                    if cancel.load(Ordering::Relaxed)
                        || !warm_keepalive_is_current(&slots, slot_index, &lease)
                    {
                        break;
                    }
                    if controller.pause().await.is_err() {
                        break;
                    }
                }
            }));
        }
    }

    fn cancel_warm_keepalives(&mut self) {
        if let Ok(slots) = self.slots.lock() {
            for reservation in slots.iter().flatten() {
                reservation
                    .warm_keepalive_cancel
                    .store(true, Ordering::Relaxed);
            }
        }
        for task in self.keepalive_tasks.iter_mut() {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
    }

    fn start_slot_download(
        &mut self,
        slot_index: usize,
        item: &DownloadPlanItem,
        config: &SendspinDownloaderConfig,
    ) -> Result<(), String> {
        if crate::cache::store::cache_complete_duration_millis(
            &self.caches,
            &item.cache_content_key,
        )
        .is_some()
        {
            LOGGER.trace(format!(
                "download skipped: cache hit for {}",
                item.cache_content_key
            ));
            return Ok(());
        }
        let resume_after_millis = crate::cache::store::cache_corrupted_duration_millis(
            &self.caches,
            &item.cache_content_key,
        );
        let resume_position_seconds = resume_after_millis.map(|duration| {
            duration
                .saturating_sub(RESUME_SEEK_PREROLL_MILLIS)
                .saturating_div(1_000) as i64
        });
        if resume_after_millis.is_none() {
            crate::cache::store::cache_discard_incomplete(&self.caches, &item.cache_content_key);
        }
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        let reservation = {
            let mut slots = self
                .slots
                .lock()
                .map_err(|_| "Sendspin downloader slot lock poisoned".to_string())?;
            let reservation = crate::cache::store::DownloadSlotReservation {
                cache_content_key: item.cache_content_key.clone(),
                expected_duration_millis: item.expected_duration_millis,
                resume_after_millis: resume_after_millis.unwrap_or(0),
                append_after_seek: Arc::new(AtomicBool::new(resume_position_seconds.is_none())),
                lease_id: format!(
                    "slot-{}-{}",
                    slot_index + 1,
                    self.cache_sequence.fetch_add(1, Ordering::Relaxed)
                ),
                warm_keepalive_cancel: Arc::new(AtomicBool::new(false)),
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
            if let Some(state) = states.get_mut(&item.cache_content_key) {
                state.state = "prepared".to_string();
            }
        }

        let endpoint = format!("{}/api", config.music_assistant_url.trim_end_matches('/'));
        let queue_id = self.queue_ids[slot_index]
            .clone()
            .ok_or_else(|| "Music Assistant queue ID is not resolved".to_string())?;
        let media_item = item.media_item.clone();
        let auth_token = config.music_assistant_auth_token.clone();
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
        let append_after_seek = Arc::clone(&reservation.append_after_seek);
        self.runtime.spawn(async move {
            let client = reqwest::Client::new();
            let payload = serde_json::json!({
                "command": "player_queues/play_media",
                "args": {
                    "queue_id": queue_id,
                    "media": [media_item],
                    "option": "replace",
                    "radio_mode": false,
                    "sort_by": "original"
                }
            });
            let mut request = client.post(&endpoint).json(&payload);
            if let Some(token) = auth_token
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                request = request.bearer_auth(token);
            }
            let result = match request.send().await {
                Ok(response) if response.status().is_success() => {
                    LOGGER.trace(format!(
                        "download requested: slot {}, cache key {}",
                        slot_index + 1,
                        reservation.cache_content_key
                    ));
                    Ok(())
                }
                Ok(response) => Err(format!(
                    "Music Assistant download request returned HTTP {}",
                    response.status()
                )),
                Err(error) => Err(format!("Music Assistant download request failed: {error}")),
            };
            if let Err(error) = result {
                LOGGER.error(error);
                release_incomplete_reservation(
                    &slots,
                    &caches,
                    &cache_change_signal,
                    &states,
                    slot_index,
                    &reservation,
                );
                return;
            }
            if let Some(position) = resume_position_seconds {
                if let Err(error) = seek_download_player(
                    &client,
                    &controller,
                    &endpoint,
                    auth_token.as_deref(),
                    &queue_id,
                    position,
                )
                .await
                {
                    LOGGER.error(format!("failed to resume interrupted download: {error}"));
                    release_incomplete_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        slot_index,
                        &reservation,
                    );
                    return;
                }
                append_after_seek.store(true, Ordering::Release);
            }
        });
        Ok(())
    }

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
        let player_id = self.queue_ids[slot_index]
            .clone()
            .ok_or_else(|| "Music Assistant queue ID is not resolved".to_string())?;
        let endpoint = format!("{}/api", config.music_assistant_url.trim_end_matches('/'));
        let auth_token = config.music_assistant_auth_token.clone();

        self.runtime.block_on(async {
            let client = reqwest::Client::new();
            controller
                .play()
                .await
                .map_err(|error| format!("Sendspin seek slot play failed: {error}"))?;
            let payload = serde_json::json!({
                "command": "players/cmd/seek",
                "args": {
                    "player_id": player_id,
                    "position": position,
                }
            });
            let mut request = client.post(endpoint).json(&payload);
            if let Some(token) = auth_token
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .map_err(|error| format!("Music Assistant seek failed: {error}"))?;
            if !response.status().is_success() {
                return Err(format!(
                    "Music Assistant seek returned HTTP {}",
                    response.status()
                ));
            }
            Ok(())
        })
    }
}

enum DownloaderCommand {
    Configure(SendspinDownloaderConfig),
    Connect,
    Reconnect,
    Disconnect,
    ApplyDownloadPlan(String),
    UpdatePlaybackWindow {
        current: Option<String>,
        upcoming: Vec<String>,
        recently_played: Vec<String>,
    },
    EnsureDownloadWindow(Vec<String>),
    MarkCacheBuffering(String),
    Seek { content_key: String, seconds: i64 },
    Shutdown,
}

pub(crate) struct SendspinDownloader {
    commands: Sender<DownloaderCommand>,
    download_plan: Arc<Mutex<Option<DownloadPlan>>>,
    cache_states: Arc<Mutex<HashMap<String, CacheItemState>>>,
    caches: crate::cache::store::EncodedCaches,
    slots: crate::cache::store::DownloadSlots,
    download_window_refresh: Arc<AtomicBool>,
    download_status_refresh: Arc<AtomicBool>,
    connection_lost: Arc<AtomicBool>,
    connection_ready: Arc<AtomicBool>,
    cache_capacity_bytes: Arc<AtomicUsize>,
}

impl SendspinDownloader {
    pub(crate) fn new() -> Self {
        let worker = SendspinDownloaderWorker::new();
        let cache_states = Arc::clone(&worker.cache_states);
        let caches = Arc::clone(&worker.caches);
        let slots = Arc::clone(&worker.slots);
        let download_window_refresh = Arc::clone(&worker.download_window_refresh);
        let download_status_refresh = Arc::clone(&worker.download_status_refresh);
        let connection_lost = Arc::clone(&worker.connection_lost);
        let connection_ready = Arc::clone(&worker.connection_ready);
        let cache_capacity_bytes = Arc::new(AtomicUsize::new(0));
        let download_plan = Arc::new(Mutex::new(None));
        let (commands, receiver) = mpsc::channel();
        std::thread::spawn(move || run_downloader_worker(worker, receiver));
        Self {
            commands,
            download_plan,
            cache_states,
            caches,
            slots,
            download_window_refresh,
            download_status_refresh,
            connection_lost,
            connection_ready,
            cache_capacity_bytes,
        }
    }

    pub(crate) fn configure(&self, config: SendspinDownloaderConfig) {
        self.cache_capacity_bytes
            .store(config.cache_capacity_bytes, Ordering::Release);
        let _ = self.commands.send(DownloaderCommand::Configure(config));
    }

    pub(crate) fn connect(&self) -> Result<(), String> {
        self.send(DownloaderCommand::Connect)
    }

    pub(crate) fn reconnect(&self) -> Result<(), String> {
        self.send(DownloaderCommand::Reconnect)
    }

    pub(crate) fn disconnect(&self) {
        let _ = self.commands.send(DownloaderCommand::Disconnect);
    }

    pub(crate) fn apply_download_plan(&self, plan_json: &str) -> Result<(), String> {
        let plan = parse_plan(plan_json)?;
        if let Ok(mut current_plan) = self.download_plan.lock() {
            *current_plan = Some(plan);
        }
        self.send(DownloaderCommand::ApplyDownloadPlan(plan_json.to_string()))
    }

    pub(crate) fn download_plan(&self) -> Option<DownloadPlan> {
        self.download_plan
            .lock()
            .ok()
            .and_then(|plan| plan.clone())
    }

    pub(crate) fn cache_states(&self) -> Vec<CacheItemState> {
        self.cache_states
            .lock()
            .map(|states| states.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn take_download_window_refresh(&self) -> bool {
        self.download_window_refresh.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn take_download_status_refresh(&self) -> bool {
        self.download_status_refresh.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn take_connection_lost(&self) -> bool {
        self.connection_lost.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn take_connection_ready(&self) -> bool {
        self.connection_ready.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn download_status(&self) -> (Vec<ActiveDownloadStatus>, usize, usize) {
        let states = self.cache_states();
        let reservations = self
            .slots
            .lock()
            .map(|slots| slots.iter().flatten().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let plan = self.download_plan();
        let active = reservations
            .iter()
            .map(|reservation| {
                let queue_item_id = plan
                    .as_ref()
                    .and_then(|plan| {
                        plan.items
                            .iter()
                            .find(|item| item.cache_content_key == reservation.cache_content_key)
                    })
                    .map(|item| item.queue_item_id.as_str());
                let state = states
                    .iter()
                    .find(|state| Some(state.queue_item_id.as_str()) == queue_item_id);
                ActiveDownloadStatus {
                    queue_item_id: state
                        .map(|state| state.queue_item_id.clone())
                        .unwrap_or_else(|| reservation.cache_content_key.clone()),
                    cached_duration_millis: state
                        .map(|state| state.cached_duration_millis)
                        .unwrap_or(0),
                    expected_duration_millis: reservation.expected_duration_millis,
                }
            })
            .collect();
        (
            active,
            crate::cache::store::cache_total_bytes(&self.caches),
            self.cache_capacity_bytes.load(Ordering::Acquire),
        )
    }

    pub(crate) fn completed_cache_item_count(&self, content_keys: &[String]) -> usize {
        content_keys
            .iter()
            .filter(|key| crate::cache::store::cache_complete_duration_millis(&self.caches, key).is_some())
            .count()
    }

    pub(crate) fn mark_cache_buffering(&self, content_key: &str) -> Result<(), String> {
        self.send(DownloaderCommand::MarkCacheBuffering(content_key.to_string()))
    }

    pub(crate) fn cache_format(&self, content_key: &str) -> Result<AudioFormat, String> {
        crate::cache::store::cache_read_format(&self.caches, content_key)
    }

    pub(crate) fn cache_chunk(&self, content_key: &str, chunk_index: usize) -> Result<(Option<Arc<[u8]>>, bool), String> {
        crate::cache::store::cache_read_chunk(&self.caches, content_key, chunk_index)
    }

    pub(crate) fn update_playback_window(&self, current: Option<String>, upcoming: Vec<String>, recently_played: Vec<String>) {
        let _ = self.commands.send(DownloaderCommand::UpdatePlaybackWindow { current, upcoming, recently_played });
    }

    pub(crate) fn ensure_download_window(&self, content_keys: &[String]) -> Result<(), String> {
        self.send(DownloaderCommand::EnsureDownloadWindow(content_keys.to_vec()))
    }

    pub(crate) fn seek(&self, content_key: &str, seconds: i64) -> Result<(), String> {
        self.send(DownloaderCommand::Seek { content_key: content_key.to_string(), seconds })
    }

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

fn run_downloader_worker(mut worker: SendspinDownloaderWorker, receiver: mpsc::Receiver<DownloaderCommand>) {
    while let Ok(command) = receiver.recv() {
        let is_connection_command = matches!(command, DownloaderCommand::Connect | DownloaderCommand::Reconnect);
        let result = match command {
            DownloaderCommand::Configure(config) => {
                worker.configure(config);
                Ok(())
            }
            DownloaderCommand::Connect => worker.connect(),
            DownloaderCommand::Reconnect => worker.reconnect(),
            DownloaderCommand::Disconnect => {
                worker.disconnect();
                Ok(())
            }
            DownloaderCommand::ApplyDownloadPlan(plan) => worker.apply_download_plan(&plan),
            DownloaderCommand::UpdatePlaybackWindow { current, upcoming, recently_played } => {
                worker.update_playback_window(current, upcoming, recently_played);
                Ok(())
            }
            DownloaderCommand::EnsureDownloadWindow(content_keys) => worker.ensure_download_window(&content_keys),
            DownloaderCommand::MarkCacheBuffering(content_key) => worker.mark_cache_buffering(&content_key),
            DownloaderCommand::Seek { content_key, seconds } => worker.seek(&content_key, seconds),
            DownloaderCommand::Shutdown => {
                worker.disconnect();
                break;
            }
        };
        if let Err(error) = result {
            LOGGER.error(format!("Sendspin downloader worker command failed: {error}"));
            worker.connection_ready.store(false, Ordering::Release);
            worker.connection_lost.store(true, Ordering::Release);
        } else if is_connection_command {
            worker.connection_lost.store(false, Ordering::Release);
            worker.connection_ready.store(true, Ordering::Release);
        }
    }
}

async fn seek_download_player(
    client: &reqwest::Client,
    controller: &Controller,
    endpoint: &str,
    auth_token: Option<&str>,
    player_id: &str,
    position: i64,
) -> Result<(), String> {
    controller
        .play()
        .await
        .map_err(|error| format!("Sendspin seek slot play failed: {error}"))?;
    let payload = serde_json::json!({
        "command": "players/cmd/seek",
        "args": { "player_id": player_id, "position": position.max(0) }
    });
    let mut request = client.post(endpoint).json(&payload);
    if let Some(token) = auth_token.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("Music Assistant seek failed: {error}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("Music Assistant seek returned HTTP {}", response.status()))
    }
}

async fn resolve_queue_ids(
    endpoint: &str,
    auth_token: Option<&str>,
    sendspin_client_ids: &[String; 2],
) -> Result<[String; 2], String> {
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "command": "players/all",
        "args": {
            "return_unavailable": true,
            "return_protocol_players": true
        }
    });
    let mut request = client.post(endpoint).json(&payload);
    if let Some(token) = auth_token.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("Music Assistant player lookup failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Music Assistant player lookup returned HTTP {}",
            response.status()
        ));
    }
    let players: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("Music Assistant player lookup returned invalid JSON: {error}"))?;
    let first = find_queue_id(&players, &sendspin_client_ids[0]).ok_or_else(|| {
        format!(
            "Music Assistant player for Sendspin client {} is not available",
            sendspin_client_ids[0]
        )
    })?;
    let second = find_queue_id(&players, &sendspin_client_ids[1]).ok_or_else(|| {
        format!(
            "Music Assistant player for Sendspin client {} is not available",
            sendspin_client_ids[1]
        )
    })?;
    Ok([first, second])
}

async fn disable_queue_autoplay(
    endpoint: &str,
    auth_token: Option<&str>,
    queue_ids: &[String; 2],
) -> Result<(), String> {
    let client = reqwest::Client::new();
    for queue_id in queue_ids {
        let payload = serde_json::json!({
            "command": "player_queues/autoplay",
            "args": {
                "queue_id": queue_id,
                "autoplay_enabled": false,
            }
        });
        let mut request = client.post(endpoint).json(&payload);
        if let Some(token) = auth_token.filter(|value| !value.trim().is_empty()) {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("Music Assistant autoplay disable failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Music Assistant autoplay disable returned HTTP {} for queue {queue_id}",
                response.status()
            ));
        }
    }
    Ok(())
}

fn find_queue_id(players: &serde_json::Value, sendspin_client_id: &str) -> Option<String> {
    let players = players.as_array()?;
    for player in players {
        let Some(output_protocols) = player
            .get("output_protocols")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let matches_client_id = output_protocols.iter().any(|protocol| {
            protocol
                .get("output_protocol_id")
                .and_then(serde_json::Value::as_str)
                == Some(sendspin_client_id)
        });
        if matches_client_id {
            return player
                .get("player_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
        }
    }
    None
}

fn release_incomplete_reservation(
    slots: &crate::cache::store::DownloadSlots,
    caches: &crate::cache::store::EncodedCaches,
    cache_change_signal: &crate::cache::store::CacheChangeSignal,
    states: &Arc<Mutex<HashMap<String, CacheItemState>>>,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
) {
    reservation
        .warm_keepalive_cancel
        .store(true, Ordering::Relaxed);
    if !crate::cache::store::slot_release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::cache_discard_incomplete(caches, &reservation.cache_content_key);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    if let Ok(mut states) = states.lock() {
        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
            state.state = "incomplete".to_string();
            state.cached_duration_millis = 0;
        }
    }
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
    reservation
        .warm_keepalive_cancel
        .store(true, Ordering::Relaxed);
    if !crate::cache::store::slot_release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::cache_mark_corrupted(caches, &reservation.cache_content_key);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    download_window_refresh.store(true, Ordering::Release);
    if let Ok(mut states) = states.lock() {
        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
            state.state = "corrupted".to_string();
        }
    }
    LOGGER.warn(format!(
        "stream ended before download completion on slot {}; retaining cache for resume",
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
    for (slot_index, reservation) in slot_indexes {
        reservation
            .warm_keepalive_cancel
            .store(true, Ordering::Relaxed);
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

fn cache_reaches_download_pause_threshold(
    cache_used_bytes: usize,
    cache_capacity_bytes: usize,
) -> bool {
    cache_capacity_bytes > 0
        && cache_used_bytes.saturating_mul(100)
            >= cache_capacity_bytes.saturating_mul(CACHE_DOWNLOAD_PAUSE_PERCENT)
}

fn warm_keepalive_is_current(
    slots: &crate::cache::store::DownloadSlots,
    slot_index: usize,
    reservation: &crate::cache::store::DownloadSlotReservation,
) -> bool {
    !reservation.warm_keepalive_cancel.load(Ordering::Relaxed)
        && crate::cache::store::slot_reservation_is_current(slots, slot_index, reservation)
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

pub(crate) fn parse_plan(plan_json: &str) -> Result<DownloadPlan, String> {
    serde_json::from_str(plan_json).map_err(|error| format!("invalid download plan json: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        cache_reaches_download_pause_threshold, download_client_id, download_client_name,
        download_duration_reached, find_queue_id, normalize_client_id, warm_keepalive_is_current,
        SendspinDownloader,
    };
    use crate::cache::store::{DownloadSlotReservation, DownloadSlots};
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::Ordering;

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
    fn resolves_dynamic_universal_player_id_from_output_protocol() {
        let players = serde_json::json!([
            {
                "player_id": "mashinsurfacepro11dl2",
                "output_protocols": []
            },
            {
                "player_id": "upcc05aedd",
                "output_protocols": [
                    {
                        "output_protocol_id": "mashinsurfacepro11dl2",
                        "protocol_domain": "sendspin"
                    }
                ]
            }
        ]);

        assert_eq!(
            find_queue_id(&players, "mashinsurfacepro11dl2"),
            Some("upcc05aedd".to_string())
        );
    }

    #[test]
    fn returns_no_id_when_protocol_id_is_not_present() {
        let players = serde_json::json!([
            {
                "player_id": "upcc05aedd",
                "output_protocols": []
            }
        ]);

        assert_eq!(find_queue_id(&players, "mashinsurfacepro11dl1"), None);
    }

    #[test]
    fn completes_download_when_cached_duration_reaches_tolerance_window() {
        assert!(!download_duration_reached(Some(180_000), 179_499));
        assert!(download_duration_reached(Some(180_000), 179_500));
        assert!(!download_duration_reached(None, 999_999));
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
            warm_keepalive_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        assert!(!reservation.append_after_seek.load(Ordering::Acquire));
        reservation.append_after_seek.store(true, Ordering::Release);
        assert!(reservation.append_after_seek.load(Ordering::Acquire));
    }

    #[test]
    fn warm_keepalive_stops_when_its_slot_lease_is_replaced() {
        let first = DownloadSlotReservation {
            cache_content_key: "provider:track:first".to_string(),
            expected_duration_millis: None,
            resume_after_millis: 0,
            append_after_seek: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            lease_id: "slot-1-1".to_string(),
            warm_keepalive_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let replacement = DownloadSlotReservation {
            cache_content_key: "provider:track:second".to_string(),
            expected_duration_millis: None,
            resume_after_millis: 0,
            append_after_seek: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            lease_id: "slot-1-2".to_string(),
            warm_keepalive_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let slots: DownloadSlots = Arc::new(Mutex::new([Some(first.clone()), None]));

        assert!(warm_keepalive_is_current(&slots, 0, &first));
        slots.lock().expect("slot lock")[0] = Some(replacement);
        assert!(!warm_keepalive_is_current(&slots, 0, &first));
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
