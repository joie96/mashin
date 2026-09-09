use super::connection::{audio_format_from_stream, player_support};
use super::models::{CacheItemState, DownloadPlan, DownloadPlanItem};
use crate::cache::store::CacheRetentionPolicy;
use ::sendspin::audio::AudioFormat;
use ::sendspin::protocol::client::{ConnectionGuard, Controller};
use ::sendspin::protocol::messages::{GoodbyeReason, Message, PlayerState};
use ::sendspin::ProtocolClientBuilder;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

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

pub(crate) struct SendspinDownloader {
    runtime: Runtime,
    config: Option<SendspinDownloaderConfig>,
    plan: Option<DownloadPlan>,
    cache_states: Arc<Mutex<HashMap<String, CacheItemState>>>,
    caches: crate::cache::store::EncodedCaches,
    cache_change_signal: crate::cache::store::CacheChangeSignal,
    retention_policy: Arc<Mutex<CacheRetentionPolicy>>,
    cache_sequence: Arc<std::sync::atomic::AtomicU64>,
    slots: crate::cache::store::DownloadSlots,
    controllers: Arc<Mutex<[Option<Controller>; 2]>>,
    guards: [Option<ConnectionGuard>; 2],
    io_tasks: [Option<JoinHandle<()>>; 2],
}

impl SendspinDownloader {
    pub(crate) fn new() -> Self {
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
            slots: Arc::new(Mutex::new([None, None])),
            controllers: Arc::new(Mutex::new([None, None])),
            guards: [None, None],
            io_tasks: [None, None],
        }
    }

    pub(crate) fn configure(&mut self, config: SendspinDownloaderConfig) {
        self.config = Some(config);
    }

    pub(crate) fn connect(&mut self) -> Result<(), String> {
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
        let download_suffix = format!("dl{slot_number}");
        let client_id = format!(
            "mashin-{}-{download_suffix}",
            normalize_client_id(&config.client_id),
        );
        let client_name = format!("Mashin {} {download_suffix}", config.client_name.trim());
        let support = player_support(&config.preferred_codec, config.buffer_capacity_bytes);
        let slots = Arc::clone(&self.slots);
        let caches = Arc::clone(&self.caches);
        let states = Arc::clone(&self.cache_states);
        let cache_change_signal = Arc::clone(&self.cache_change_signal);
        let retention_policy = Arc::clone(&self.retention_policy);
        let controllers = Arc::clone(&self.controllers);
        let sequence = Arc::clone(&self.cache_sequence);
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
                                }
                                Message::StreamEnd(_) => {
                                    if let Some(value) = reservation.as_ref() {
                                        if crate::cache::store::release_reservation_if_current(
                                            &slots,
                                            slot_index,
                                            value,
                                        ) {
                                            crate::cache::store::mark_cache_complete(&caches, value);
                                            crate::cache::store::notify_cache_changed(&cache_change_signal);
                                            if let Ok(mut states) = states.lock() {
                                                if let Some(state) = states.get_mut(&value.cache_content_key) {
                                                    state.state = "complete".to_string();
                                                }
                                            }
                                        }
                                    }
                                    reservation = None;
                                    format = None;
                                    last_timestamp = None;
                                }
                                Message::StreamClear(_) => {
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
                            if crate::cache::store::reservation_is_current(&slots, slot_index, reservation) {
                                let policy = retention_policy.lock().map(|policy| policy.clone()).unwrap_or_default();
                                let evicted = crate::cache::store::evict_to_fit(
                                    &caches,
                                    &policy,
                                    capacity,
                                    chunk.data.len(),
                                );
                                stop_evicted_slot_downloads(&slots, &controllers, &evicted).await;
                                if let Ok(mut states) = states.lock() {
                                    for cache_content_key in evicted {
                                        if let Some(state) = states.get_mut(&cache_content_key) {
                                            state.state = "evicted".to_string();
                                        }
                                    }
                                }
                            }
                            if !crate::cache::store::reservation_is_current(&slots, slot_index, reservation) {
                                continue;
                            }
                            let result = crate::cache::store::cache_encoded_chunk(
                                &caches,
                                &sequence,
                                capacity,
                                reservation,
                                format,
                                duration,
                                &chunk.data,
                            );
                            if result != crate::cache::store::CacheAppendResult::HardLimitReached {
                                crate::cache::store::notify_cache_changed(&cache_change_signal);
                                if let Ok(mut states) = states.lock() {
                                    if let Some(state) = states.get_mut(&reservation.cache_content_key) {
                                        state.state = "downloading".to_string();
                                        state.cached_duration_millis = state.cached_duration_millis.saturating_add(duration);
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(value) = reservation {
                    release_incomplete_reservation(
                        &slots,
                        &caches,
                        &cache_change_signal,
                        &states,
                        slot_index,
                        &value,
                    );
                }
            });
            Ok::<_, String>((controller, connection.guard, task))
        })?;
        if let Ok(mut controllers) = self.controllers.lock() {
            controllers[slot_index] = Some(controller);
        }
        self.guards[slot_index] = Some(guard);
        self.io_tasks[slot_index] = Some(task);
        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        let active_reservations = self
            .slots
            .lock()
            .map(|slots| slots.iter().flatten().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for reservation in active_reservations {
            reservation
                .warm_keepalive_cancel
                .store(true, Ordering::Relaxed);
            crate::cache::store::discard_incomplete_cache(
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
    }

    pub(crate) fn apply_download_plan(&mut self, plan_json: &str) -> Result<(), String> {
        let plan = parse_plan(plan_json)?;
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

    pub(crate) fn download_plan(&self) -> Option<&DownloadPlan> {
        self.plan.as_ref()
    }

    pub(crate) fn cache_states(&self) -> Vec<CacheItemState> {
        self.cache_states
            .lock()
            .map(|states| states.values().cloned().collect())
            .unwrap_or_default()
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

    pub(crate) fn cache_format(&self, content_key: &str) -> Result<AudioFormat, String> {
        crate::cache::store::read_cache_format(&self.caches, content_key)
    }

    pub(crate) fn cache_chunk(
        &self,
        content_key: &str,
        chunk_index: usize,
    ) -> Result<(Option<Arc<[u8]>>, bool), String> {
        crate::cache::store::read_cache_chunk(&self.caches, content_key, chunk_index)
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
                next,
                protected,
                prefetch: upcoming.into_iter().skip(1).collect(),
                recently_played,
            };
        }
        self.evict_to_capacity(0);
    }

    pub(crate) fn ensure_download_window(&mut self, content_keys: &[String]) -> Result<(), String> {
        self.evict_to_capacity(0);
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

        for item in candidates {
            if crate::cache::store::complete_cache_duration_millis(
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

    fn evict_to_capacity(&mut self, incoming_bytes: usize) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let policy = self
            .retention_policy
            .lock()
            .map(|policy| policy.clone())
            .unwrap_or_default();
        let evicted = crate::cache::store::evict_to_fit(
            &self.caches,
            &policy,
            config.cache_capacity_bytes,
            incoming_bytes,
        );
        self.runtime.block_on(stop_evicted_slot_downloads(
            &self.slots,
            &self.controllers,
            &evicted,
        ));
        if let Ok(mut states) = self.cache_states.lock() {
            for cache_content_key in evicted {
                if let Some(state) = states.get_mut(&cache_content_key) {
                    state.state = "evicted".to_string();
                }
            }
        }
    }

    fn start_slot_download(
        &mut self,
        slot_index: usize,
        item: &DownloadPlanItem,
        config: &SendspinDownloaderConfig,
    ) -> Result<(), String> {
        if crate::cache::store::complete_cache_duration_millis(
            &self.caches,
            &item.cache_content_key,
        )
        .is_some()
        {
            return Ok(());
        }
        crate::cache::store::discard_incomplete_cache(&self.caches, &item.cache_content_key);
        crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        let reservation = {
            let mut slots = self
                .slots
                .lock()
                .map_err(|_| "Sendspin downloader slot lock poisoned".to_string())?;
            let reservation = crate::cache::store::DownloadSlotReservation {
                cache_content_key: item.cache_content_key.clone(),
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
        if let Ok(mut states) = self.cache_states.lock() {
            if let Some(state) = states.get_mut(&item.cache_content_key) {
                state.state = "prepared".to_string();
            }
        }

        let endpoint = format!("{}/api", config.music_assistant_url.trim_end_matches('/'));
        let payload = serde_json::json!({
            "command": "player_queues/play_media",
            "args": {
                "queue_id": format!(
                    "upmashin{}-dl{}",
                    normalize_client_id(&config.client_id).replace('-', ""),
                    slot_index + 1
                ),
                "media": [item.media_item],
                "option": "replace",
                "radio_mode": false,
                "sort_by": "original"
            }
        });
        let auth_token = config.music_assistant_auth_token.clone();
        let response = self.runtime.block_on(async move {
            let client = reqwest::Client::new();
            let mut request = client.post(endpoint).json(&payload);
            if let Some(token) = auth_token.filter(|value| !value.trim().is_empty()) {
                request = request.bearer_auth(token);
            }
            request.send().await.map_err(|error| error.to_string())
        });
        match response {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => {
                self.release_reservation(slot_index, &reservation);
                Err(format!(
                    "Music Assistant download request returned HTTP {}",
                    response.status()
                ))
            }
            Err(error) => {
                self.release_reservation(slot_index, &reservation);
                Err(format!("Music Assistant download request failed: {error}"))
            }
        }
    }

    fn release_reservation(
        &mut self,
        slot_index: usize,
        reservation: &crate::cache::store::DownloadSlotReservation,
    ) {
        reservation
            .warm_keepalive_cancel
            .store(true, Ordering::Relaxed);
        if crate::cache::store::release_reservation_if_current(&self.slots, slot_index, reservation)
        {
            crate::cache::store::discard_incomplete_cache(
                &self.caches,
                &reservation.cache_content_key,
            );
            crate::cache::store::notify_cache_changed(&self.cache_change_signal);
        }
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
        let player_id = format!(
            "upmashin{}-dl{}",
            normalize_client_id(&config.client_id).replace('-', ""),
            slot_index + 1
        );

        self.runtime.block_on(async {
            controller
                .play()
                .await
                .map_err(|error| format!("Sendspin seek slot play failed: {error}"))?;
            let endpoint = format!("{}/api", config.music_assistant_url.trim_end_matches('/'));
            let payload = serde_json::json!({
                "command": "players/cmd/seek",
                "args": {
                    "player_id": player_id,
                    "position": position,
                }
            });
            let client = reqwest::Client::new();
            let mut request = client.post(endpoint).json(&payload);
            if let Some(token) = config
                .music_assistant_auth_token
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
    if !crate::cache::store::release_reservation_if_current(slots, slot_index, reservation) {
        return;
    }
    crate::cache::store::discard_incomplete_cache(caches, &reservation.cache_content_key);
    crate::cache::store::notify_cache_changed(cache_change_signal);
    if let Ok(mut states) = states.lock() {
        if let Some(state) = states.get_mut(&reservation.cache_content_key) {
            state.state = "incomplete".to_string();
        }
    }
}

async fn stop_evicted_slot_downloads(
    slots: &crate::cache::store::DownloadSlots,
    controllers: &Arc<Mutex<[Option<Controller>; 2]>>,
    evicted_content_keys: &[String],
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
    let normalized: String = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        "device".to_string()
    } else {
        normalized.to_string()
    }
}

pub(crate) fn parse_plan(plan_json: &str) -> Result<DownloadPlan, String> {
    serde_json::from_str(plan_json).map_err(|error| format!("invalid download plan json: {error}"))
}
