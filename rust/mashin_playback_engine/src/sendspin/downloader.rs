use super::connection::{audio_format_from_stream, player_support};
use super::models::{CacheItemSnapshot, DownloadPlan, DownloadPlanItem};
use crate::cache::store::CachePlan;
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
    cache_states: Arc<Mutex<HashMap<String, CacheItemSnapshot>>>,
    caches: crate::cache::store::EncodedCaches,
    cache_change_signal: crate::cache::store::CacheChangeSignal,
    cache_sequence: Arc<std::sync::atomic::AtomicU64>,
    slots: crate::cache::store::DownloadSlots,
    controllers: [Option<Controller>; 2],
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
            cache_sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            slots: Arc::new(Mutex::new([None, None])),
            controllers: [None, None],
            guards: [None, None],
            io_tasks: [None, None],
        }
    }

    pub(crate) fn configure(&mut self, config: SendspinDownloaderConfig) {
        self.config = Some(config);
    }

    pub(crate) fn connect(&mut self) -> Result<(), String> {
        if self.controllers.iter().all(Option::is_some) {
            return Ok(());
        }
        self.connect_slot(0)?;
        self.connect_slot(1)?;
        Ok(())
    }

    fn connect_slot(&mut self, slot_index: usize) -> Result<(), String> {
        if self.controllers[slot_index].is_some() {
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
                                        crate::cache::store::mark_cache_complete(&caches, value);
                                        crate::cache::store::notify_cache_changed(&cache_change_signal);
                                        if let Ok(mut states) = states.lock() {
                                            if let Some(state) = states.get_mut(&value.cache_content_key) {
                                                state.state = "complete".to_string();
                                            }
                                        }
                                    }
                                    reservation = None;
                                    format = None;
                                    last_timestamp = None;
                                }
                                Message::StreamClear(_) => {
                                    if let Some(value) = reservation.as_ref() {
                                        crate::cache::store::discard_incomplete_cache(&caches, &value.cache_content_key);
                                    }
                                    crate::cache::store::notify_cache_changed(&cache_change_signal);
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
                            let result = crate::cache::store::cache_encoded_chunk(
                                &caches,
                                &sequence,
                                capacity,
                                capacity.saturating_mul(90) / 100,
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
                    crate::cache::store::discard_incomplete_cache(
                        &caches,
                        &value.cache_content_key,
                    );
                    crate::cache::store::notify_cache_changed(&cache_change_signal);
                    if let Ok(mut states) = states.lock() {
                        if let Some(state) = states.get_mut(&value.cache_content_key) {
                            state.state = "incomplete".to_string();
                        }
                    }
                }
            });
            Ok::<_, String>((controller, connection.guard, task))
        })?;
        self.controllers[slot_index] = Some(controller);
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
        self.controllers = [None, None];
        if let Ok(mut slots) = self.slots.lock() {
            *slots = [None, None];
        }
    }

    pub(crate) fn apply_plan(&mut self, plan_json: &str) -> Result<CachePlan, String> {
        let plan = parse_plan(plan_json)?;
        let cache_plan = cache_plan(&plan);
        if let Ok(mut states) = self.cache_states.lock() {
            states.clear();
        }
        for item in prioritized_items(&plan) {
            if let Ok(mut states) = self.cache_states.lock() {
                states.insert(
                    item.cache_content_key.clone(),
                    CacheItemSnapshot {
                        cache_content_key: item.cache_content_key,
                        queue_item_id: Some(item.queue_item_id),
                        state: "queued".to_string(),
                        cached_duration_millis: 0,
                    },
                );
            }
        }
        self.plan = Some(plan);
        self.start_plan_downloads()?;
        Ok(cache_plan)
    }

    fn start_plan_downloads(&mut self) -> Result<(), String> {
        let Some(plan) = self.plan.clone() else {
            return Ok(());
        };
        let Some(config) = self.config.clone() else {
            return Ok(());
        };
        for (slot_index, item) in prioritized_items(&plan).into_iter().take(2).enumerate() {
            self.start_slot_download(slot_index, &item, &config)?;
        }
        Ok(())
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
                let _ = self.release_reservation(slot_index, &reservation);
                Err(format!(
                    "Music Assistant download request returned HTTP {}",
                    response.status()
                ))
            }
            Err(error) => {
                let _ = self.release_reservation(slot_index, &reservation);
                Err(format!("Music Assistant download request failed: {error}"))
            }
        }
    }

    fn release_reservation(
        &mut self,
        slot_index: usize,
        reservation: &crate::cache::store::DownloadSlotReservation,
    ) -> Result<(), String> {
        reservation
            .warm_keepalive_cancel
            .store(true, Ordering::Relaxed);
        crate::cache::store::discard_incomplete_cache(&self.caches, &reservation.cache_content_key);
        if let Ok(mut slots) = self.slots.lock() {
            slots[slot_index] = None;
        }
        Ok(())
    }

    pub(crate) fn plan(&self) -> Option<&DownloadPlan> {
        self.plan.as_ref()
    }

    pub(crate) fn cache_states(&self) -> Vec<CacheItemSnapshot> {
        self.cache_states
            .lock()
            .map(|states| states.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn mark_buffering(&mut self, content_key: &str) -> Result<(), String> {
        let Ok(mut states) = self.cache_states.lock() else {
            return Err("Sendspin downloader state lock poisoned".to_string());
        };
        let Some(item) = states.get_mut(content_key) else {
            return Err("track is not present in the current download plan".to_string());
        };
        item.state = "buffering".to_string();
        Ok(())
    }

    pub(crate) fn seek_via_music_assistant(
        &mut self,
        content_key: &str,
        seconds: i64,
    ) -> Result<(), String> {
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
        let controller = self.controllers[slot_index]
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

    pub(crate) fn cache_change_generation(&self) -> u64 {
        self.cache_change_signal
            .0
            .lock()
            .map(|generation| *generation)
            .unwrap_or(0)
    }

    pub(crate) fn wait_for_cache_change(&self, generation: u64) -> u64 {
        crate::cache::store::wait_for_cache_change(&self.cache_change_signal, generation)
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

pub(crate) fn cache_plan(plan: &DownloadPlan) -> CachePlan {
    CachePlan {
        current_cache_content_key: plan
            .items
            .iter()
            .find(|item| item.role == "current")
            .map(|item| item.cache_content_key.clone()),
        next_cache_content_key: plan
            .items
            .iter()
            .find(|item| item.role == "next")
            .map(|item| item.cache_content_key.clone()),
        last_played_cache_content_key: None,
        ordered_cache_content_keys: plan
            .items
            .iter()
            .map(|item| item.cache_content_key.clone())
            .collect(),
    }
}

pub(crate) fn prioritized_items(plan: &DownloadPlan) -> Vec<DownloadPlanItem> {
    let mut items = plan.items.clone();
    items.sort_by_key(|item| std::cmp::Reverse(item.priority));
    items
}
