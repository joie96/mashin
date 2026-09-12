use super::downloader::{SendspinDownloader, SendspinDownloaderConfig};
use super::models::{QueueCacheUpdate, QueueItemCacheSnapshot, QueueItemSnapshot};
use crate::audio::player::{AudioPlayer, AudioPlayerState};
use crate::audio::source::{AudioSource, SourceReadResult};
use crate::logging::Logger;
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static LOGGER: Logger = Logger::new(module_path!());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaybackState {
    Uninitialized,
    Idle,
    Buffering,
    Playing,
    Paused,
    Error,
}

impl PlaybackState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Idle => "idle",
            Self::Buffering => "buffering",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendspinSnapshot {
    pub(crate) connection_state: String,
    pub(crate) playback_state: String,
    pub(crate) local_playback_position_millis: Option<i64>,
    pub(crate) volume: u8,
    pub(crate) muted: bool,
    pub(crate) shuffle_enabled: bool,
    pub(crate) repeat_mode: String,
    pub(crate) queue: Vec<QueueItemSnapshot>,
    pub(crate) current_index: Option<usize>,
}

impl Default for SendspinSnapshot {
    fn default() -> Self {
        Self {
            connection_state: "offline".to_string(),
            playback_state: PlaybackState::Uninitialized.as_str().to_string(),
            local_playback_position_millis: None,
            volume: 50,
            muted: false,
            shuffle_enabled: false,
            repeat_mode: "off".to_string(),
            queue: Vec::new(),
            current_index: None,
        }
    }
}

pub(crate) struct SendspinEngine {
    config: Option<Value>,
    snapshot: SendspinSnapshot,
    downloader: SendspinDownloader,
    audio_player: AudioPlayer,
    download_plan_version: u64,
    next_queue_item_id: u64,
    next_original_index: usize,
    queue_revision: u64,
    current_cache_content_key: Option<String>,
    queue_content_keys: Vec<String>,
    recently_played_content_keys: Vec<String>,
    last_download_status_log: Option<Instant>,
    next_reconnect_attempt: Option<Instant>,
    reconnect_in_flight: bool,
    playback_position_offset_millis: i64,
}

#[derive(Clone)]
struct QueueMediaItem {
    queue_item_id: String,
    original_index: usize,
    media_item: Value,
}

impl SendspinEngine {
    pub(crate) fn new() -> Self {
        Self {
            config: None,
            snapshot: SendspinSnapshot::default(),
            downloader: SendspinDownloader::new(),
            audio_player: AudioPlayer::new(),
            download_plan_version: 0,
            next_queue_item_id: 1,
            next_original_index: 0,
            queue_revision: 0,
            current_cache_content_key: None,
            queue_content_keys: Vec::new(),
            recently_played_content_keys: Vec::new(),
            last_download_status_log: None,
            next_reconnect_attempt: None,
            reconnect_in_flight: false,
            playback_position_offset_millis: 0,
        }
    }

    pub(crate) fn init(&mut self, config: Value) {
        let downloader_config = Self::downloader_config(&config);
        self.config = Some(config);
        self.snapshot = SendspinSnapshot::default();
        self.download_plan_version = 0;
        self.next_queue_item_id = 1;
        self.next_original_index = 0;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.recently_played_content_keys.clear();
        self.last_download_status_log = None;
        self.next_reconnect_attempt = None;
        self.reconnect_in_flight = false;
        self.playback_position_offset_millis = 0;
        self.downloader = SendspinDownloader::new();
        self.downloader.configure(downloader_config);
        self.audio_player.stop();
    }

    fn downloader_config(config: &Value) -> SendspinDownloaderConfig {
        SendspinDownloaderConfig {
            sendspin_url: config
                .get("sendspinUrl")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            music_assistant_url: config
                .get("musicAssistantUrl")
                .or_else(|| config.get("baseUrl"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            music_assistant_auth_token: config
                .get("musicAssistantAuthToken")
                .or_else(|| config.get("authToken"))
                .and_then(Value::as_str)
                .map(str::to_string),
            client_id: config
                .get("clientId")
                .and_then(Value::as_str)
                .unwrap_or("mashin-playback")
                .to_string(),
            client_name: config
                .get("clientName")
                .and_then(Value::as_str)
                .unwrap_or("Mashin Playback")
                .to_string(),
            buffer_capacity_bytes: config
                .get("bufferCapacityBytes")
                .and_then(Value::as_u64)
                .unwrap_or(8 * 1024 * 1024),
            cache_capacity_bytes: config
                .get("cacheCapacityBytes")
                .and_then(Value::as_u64)
                .unwrap_or(250 * 1024 * 1024) as usize,
            preferred_codec: config
                .get("preferredCodec")
                .and_then(Value::as_str)
                .unwrap_or("pcm")
                .to_string(),
            }
    }

    pub(crate) fn configure(&mut self, config: Value) -> Result<(), String> {
        let url = config
            .get("sendspinUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if url.is_empty() {
            return Err("Sendspin URL is required".to_string());
        }
        LOGGER.info(format!(
            "Sendspin configured with preferred codec {}",
            config
                .get("preferredCodec")
                .and_then(Value::as_str)
                .unwrap_or("pcm")
        ));
        self.init(config);
        Ok(())
    }

    pub(crate) fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String> {
        let normalized = match codec.trim().to_ascii_lowercase().as_str() {
            "pcm" | "opus" | "flac" => codec.trim().to_ascii_lowercase(),
            value => return Err(format!("unsupported Sendspin codec: {value}")),
        };
        let mut config = self
            .config
            .clone()
            .ok_or_else(|| "Sendspin engine is not configured".to_string())?;
        if config
            .get("preferredCodec")
            .and_then(Value::as_str)
            .map(|value| value.eq_ignore_ascii_case(&normalized))
            .unwrap_or(false)
        {
            return Ok(());
        }
        config["preferredCodec"] = Value::String(normalized);
        let playback_position_millis = self
            .snapshot
            .local_playback_position_millis
            .unwrap_or_else(|| self.audio_player.playback_position_millis())
            .max(0);
        let queue_items = self.queue_media_items();
        let current_index = self.snapshot.current_index;
        let was_connected = matches!(self.snapshot.connection_state.as_str(), "online" | "reconnecting");
        self.config = Some(config.clone());
        let downloader_config = Self::downloader_config(&config);
        if was_connected {
            self.downloader.restart_for_codec_change(downloader_config)?;
        } else {
            self.downloader.configure(downloader_config);
        }
        if let Some(current_index) = current_index {
            let plan = self.build_queue_download_plan(queue_items, current_index)?;
            let plan_json = serde_json::to_string(&plan)
                .map_err(|error| format!("serialize Sendspin codec-change plan failed: {error}"))?;
            self.downloader
                .set_pending_current_seek(playback_position_millis / 1_000)?;
            self.apply_queue_download_plan(&plan_json)?;
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "codec-change plan has no current item".to_string())?;
            self.play_cached_content(&content_key)?;
            self.playback_position_offset_millis = playback_position_millis;
        }
        Ok(())
    }

    pub(crate) fn connect(&mut self) -> Result<(), String> {
        LOGGER.info("connecting Sendspin engine");
        self.downloader.connect()?;
        self.snapshot.connection_state = "online".to_string();
        if self.snapshot.playback_state == PlaybackState::Uninitialized.as_str() {
            self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
        }
        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        LOGGER.info("disconnecting Sendspin engine");
        self.downloader.disconnect();
        self.snapshot.connection_state = "offline".to_string();
        self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
        self.current_cache_content_key = None;
        self.next_reconnect_attempt = None;
        self.reconnect_in_flight = false;
    }

    pub(crate) fn apply_queue_download_plan(&mut self, plan_json: &str) -> Result<(), String> {
        self.downloader.apply_download_plan(plan_json)?;
        let plan = self
            .downloader
            .download_plan()
            .ok_or_else(|| "download plan was not stored".to_string())?;
        self.download_plan_version = plan.version;
        self.current_cache_content_key = plan
            .items
            .iter()
            .find(|item| item.role == "current")
            .map(|item| item.cache_content_key.clone());
        self.queue_content_keys = plan
            .items
            .iter()
            .map(|item| item.cache_content_key.clone())
            .collect();
        self.snapshot.queue = plan
            .items
            .iter()
            .map(|item| QueueItemSnapshot {
                queue_item_id: item.queue_item_id.clone(),
                original_index: item.original_index,
                media_item: item.media_item.clone(),
                cache: QueueItemCacheSnapshot {
                    state: "queued".to_string(),
                    cached_duration_millis: 0,
                },
            })
            .collect();
        self.snapshot.current_index = plan.items.iter().position(|item| item.role == "current");
        LOGGER.debug(format!(
            "queue download plan applied: version {}, {} items, current index {:?}",
            plan.version,
            plan.items.len(),
            self.snapshot.current_index
        ));
        self.queue_revision = self.queue_revision.saturating_add(1);
        self.sync_download_window()?;
        self.refresh_queue_cache_states();
        Ok(())
    }

    pub(crate) fn play_media(&mut self, plan_json: &str) -> Result<(), String> {
        let media_items: Vec<Value> = serde_json::from_str(plan_json)
            .map_err(|error| format!("invalid media items json: {error}"))?;
        if media_items.is_empty() {
            return Err("play_media requires at least one media item".to_string());
        }
        LOGGER.info(format!(
            "starting playback for {} media items",
            media_items.len()
        ));

        let queue_items = self.new_queue_media_items(media_items);
        let plan = self.build_queue_download_plan(queue_items, 0)?;
        let plan_json = serde_json::to_string(&plan)
            .map_err(|error| format!("serialize Sendspin download plan failed: {error}"))?;
        self.apply_queue_download_plan(&plan_json)?;
        let content_key = self
            .current_cache_content_key
            .clone()
            .ok_or_else(|| "play_media plan has no current item".to_string())?;
        self.play_cached_content(&content_key)
    }

    pub(crate) fn play_media_next(
        &mut self,
        media_items_json: &str,
        replace_following: bool,
    ) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        let additions: Vec<Value> = serde_json::from_str(media_items_json)
            .map_err(|error| format!("invalid media items json: {error}"))?;
        let insertion = self
            .snapshot
            .current_index
            .map(|index| index.saturating_add(1))
            .unwrap_or(queue.len())
            .min(queue.len());
        if replace_following {
            queue.truncate(insertion);
        }
        queue.splice(insertion..insertion, self.new_queue_media_items(additions));
        self.replace_queue_media_items(queue)
    }

    pub(crate) fn play_media_last(&mut self, media_items_json: &str) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        let additions: Vec<Value> = serde_json::from_str(media_items_json)
            .map_err(|error| format!("invalid media items json: {error}"))?;
        queue.extend(self.new_queue_media_items(additions));
        self.replace_queue_media_items(queue)
    }

    pub(crate) fn clear_queue(&mut self) -> Result<(), String> {
        LOGGER.info("clearing playback queue");
        self.audio_player.stop();
        self.snapshot.queue.clear();
        self.snapshot.current_index = None;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.next_original_index = 0;
        self.queue_revision = self.queue_revision.saturating_add(1);
        self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
        Ok(())
    }

    pub(crate) fn move_queue_item(&mut self, item_id: &str, shift: i32) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        let Some(source) = self
            .snapshot
            .queue
            .iter()
            .position(|item| item.queue_item_id == item_id)
        else {
            return Ok(());
        };
        let item = queue.remove(source);
        let target = (source as i32 + shift).clamp(0, queue.len() as i32) as usize;
        queue.insert(target, item);
        self.replace_queue_media_items(queue)
    }

    pub(crate) fn delete_queue_item(&mut self, item_id: &str) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        let Some(index) = self
            .snapshot
            .queue
            .iter()
            .position(|item| item.queue_item_id == item_id)
        else {
            return Ok(());
        };
        queue.remove(index);
        if queue.is_empty() {
            return self.clear_queue();
        }
        self.replace_queue_media_items(queue)
    }

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        let Some(content_key) = self.queue_content_keys.get(index) else {
            return Err("queue index is out of range".to_string());
        };
        let content_key = content_key.clone();
        self.record_recently_played_track(&content_key);
        self.snapshot.current_index = Some(index);
        self.play_cached_content(&content_key)?;
        self.sync_download_window()
    }

    fn queue_media_items(&self) -> Vec<QueueMediaItem> {
        self.snapshot
            .queue
            .iter()
            .map(|item| QueueMediaItem {
                queue_item_id: item.queue_item_id.clone(),
                original_index: item.original_index,
                media_item: item.media_item.clone(),
            })
            .collect()
    }

    fn new_queue_media_items(&mut self, media_items: Vec<Value>) -> Vec<QueueMediaItem> {
        media_items
            .into_iter()
            .map(|media_item| {
                let queue_item_id = format!("local-{}", self.next_queue_item_id);
                self.next_queue_item_id = self.next_queue_item_id.saturating_add(1);
                let original_index = self.next_original_index;
                self.next_original_index = self.next_original_index.saturating_add(1);
                QueueMediaItem {
                    queue_item_id,
                    original_index,
                    media_item,
                }
            })
            .collect()
    }

    fn replace_queue_media_items(
        &mut self,
        media_items: Vec<QueueMediaItem>,
    ) -> Result<(), String> {
        if media_items.is_empty() {
            return self.clear_queue();
        }
        let previous_current_index = self.snapshot.current_index;
        let previous_current_queue_item_id = self.current_queue_item_id().map(str::to_owned);
        let current_index = previous_current_queue_item_id
            .as_deref()
            .and_then(|queue_item_id| {
                media_items
                    .iter()
                    .position(|item| item.queue_item_id == queue_item_id)
            })
            .or_else(|| {
                previous_current_index.map(|index| index.min(media_items.len().saturating_sub(1)))
            })
            .unwrap_or(0);
        let plan = self.build_queue_download_plan(media_items, current_index)?;
        let plan_json = serde_json::to_string(&plan)
            .map_err(|error| format!("serialize Sendspin download plan failed: {error}"))?;
        self.apply_queue_download_plan(&plan_json)?;
        let current_queue_item_id = self.current_queue_item_id();
        if previous_current_queue_item_id.as_deref() != current_queue_item_id {
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "queue has no current item".to_string())?;
            self.play_cached_content(&content_key)?;
        }
        Ok(())
    }

    fn current_queue_item_id(&self) -> Option<&str> {
        self.snapshot
            .current_index
            .and_then(|index| self.snapshot.queue.get(index))
            .map(|item| item.queue_item_id.as_str())
    }

    fn build_queue_download_plan(
        &self,
        media_items: Vec<QueueMediaItem>,
        current_index: usize,
    ) -> Result<Value, String> {
        let preferred_codec = self
            .config
            .as_ref()
            .and_then(|config| config.get("preferredCodec"))
            .and_then(Value::as_str)
            .unwrap_or("pcm");

        let items = media_items
            .into_iter()
            .enumerate()
            .map(|(index, queue_item)| -> Result<Value, String> {
                let QueueMediaItem {
                    queue_item_id,
                    original_index,
                    media_item,
                } = queue_item;
                let provider = media_item
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        format!("queue item {queue_item_id} has no Music Assistant provider")
                    })?;
                let item_id = media_item
                    .get("item_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        format!("queue item {queue_item_id} has no Music Assistant item_id")
                    })?;
                let cache_content_key =
                    format!("{provider}:{item_id}:{preferred_codec}:48000:16:2");
                let expected_duration_millis = media_item
                    .get("duration")
                    .and_then(Value::as_u64)
                    .filter(|duration| *duration > 0)
                    .map(|duration| duration.saturating_mul(1_000));
                let role = if index == current_index {
                    "current"
                } else if index == current_index.saturating_add(1) {
                    "next"
                } else {
                    "future"
                };
                let priority = match role {
                    "current" => 100,
                    "next" => 90,
                    _ => 50,
                };
                Ok(serde_json::json!({
                    "queueItemId": queue_item_id,
                    "originalIndex": original_index,
                    "role": role,
                    "priority": priority,
                    "cacheContentKey": cache_content_key,
                    "expectedDurationMillis": expected_duration_millis,
                    "mediaItem": media_item,
                }))
            })
            .collect::<Result<Vec<Value>, String>>()?;

        Ok(serde_json::json!({
            "version": self.download_plan_version.saturating_add(1),
            "items": items,
        }))
    }

    fn sync_download_window(&mut self) -> Result<(), String> {
        let download_window = self
            .queue_content_keys
            .iter()
            .skip(self.snapshot.current_index.unwrap_or(0))
            .map(String::clone)
            .collect::<Vec<_>>();
        let current = download_window.first().cloned();
        let upcoming = download_window.iter().skip(1).cloned().collect::<Vec<_>>();
        self.downloader.update_playback_window(
            current,
            upcoming.clone(),
            self.recently_played_content_keys.clone(),
        );
        self.downloader.ensure_download_window(&download_window)
    }

    fn record_recently_played_track(&mut self, next_content_key: &str) {
        let Some(current) = self.current_cache_content_key.as_ref() else {
            return;
        };
        if current == next_content_key {
            return;
        }
        self.recently_played_content_keys
            .retain(|key| key != current);
        self.recently_played_content_keys.push(current.clone());
    }

    fn refresh_queue_cache_states(&mut self) {
        let states = self.downloader.cache_states();
        for item in &mut self.snapshot.queue {
            if let Some(state) = states
                .iter()
                .find(|state| state.queue_item_id == item.queue_item_id)
            {
                item.cache.state = state.state.clone();
                item.cache.cached_duration_millis = state.cached_duration_millis;
            }
        }
    }

    fn refresh_and_log_download_status(&mut self) {
        let now = Instant::now();
        if self
            .last_download_status_log
            .map(|last_log| now.duration_since(last_log) < Duration::from_secs(5))
            .unwrap_or(false)
        {
            return;
        }
        self.refresh_queue_cache_states();
        let (active_downloads, cache_used_bytes, cache_capacity_bytes) =
            self.downloader.download_status();
        let completed_cache_items = self
            .downloader
            .completed_cache_item_count(&self.queue_content_keys);
        let active_downloads = active_downloads
            .iter()
            .map(|download| {
                let track_name = self
                    .snapshot
                    .queue
                    .iter()
                    .find(|item| item.queue_item_id == download.queue_item_id)
                    .and_then(|item| item.media_item.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or(&download.queue_item_id);
                let expected_duration_millis = download.expected_duration_millis.unwrap_or(0);
                format!(
                    "{track_name} {}/{} ms ({:.1}%)",
                    download.cached_duration_millis,
                    expected_duration_millis,
                    Self::percentage(
                        download.cached_duration_millis,
                        expected_duration_millis,
                    )
                )
            })
            .collect::<Vec<_>>();
        let cache_usage_percent =
            Self::percentage(cache_used_bytes as u64, cache_capacity_bytes as u64);
        LOGGER.info(format!(
            "download status: {}/{} complete queue item(s), [{}], cache {} KiB/{} KiB ({:.1}%)",
            completed_cache_items,
            self.queue_content_keys.len(),
            active_downloads.join(", "),
            cache_used_bytes / 1024,
            cache_capacity_bytes / 1024,
            cache_usage_percent,
        ));
        self.last_download_status_log = Some(now);
    }

    fn percentage(value: u64, total: u64) -> f64 {
        if total == 0 {
            return 0.0;
        }
        value as f64 * 100.0 / total as f64
    }

    pub(crate) fn play_cached_content(&mut self, content_key: &str) -> Result<(), String> {
        self.play_cached_content_with_seek(content_key, 0, 0)
    }

    fn play_cached_content_with_seek(
        &mut self,
        content_key: &str,
        chunk_index: usize,
        skip_millis: u64,
    ) -> Result<(), String> {
        let key = content_key.trim();
        if key.is_empty() {
            return Err("play content key is empty".to_string());
        }
        LOGGER.debug("preparing cached content for playback");
        self.downloader.mark_cache_buffering(key)?;
        self.audio_player.prepare_with_seek(
            AudioSource::cached_content(key.to_string()),
            chunk_index,
            skip_millis,
        );
        self.current_cache_content_key = Some(key.to_string());
        self.playback_position_offset_millis = 0;
        self.snapshot.playback_state = PlaybackState::Buffering.as_str().to_string();
        self.snapshot.local_playback_position_millis = Some(0);
        self.start_prepared_audio_if_available(key);
        self.refresh_queue_cache_states();
        Ok(())
    }

    pub(crate) fn pump_playback(&mut self) {
        if self.downloader.take_connection_lost() {
            if self.snapshot.connection_state != "reconnecting" {
                self.snapshot.connection_state = "reconnecting".to_string();
                self.next_reconnect_attempt = Some(Instant::now());
            } else if self.reconnect_in_flight {
                self.reconnect_in_flight = false;
                self.next_reconnect_attempt = Some(Instant::now() + Duration::from_secs(2));
            }
        }
        if self.downloader.take_connection_ready() {
            self.snapshot.connection_state = "online".to_string();
            self.next_reconnect_attempt = None;
            self.reconnect_in_flight = false;
            if let Err(error) = self.sync_download_window() {
                LOGGER.error(format!("failed to resume Sendspin downloads: {error}"));
            }
        }
        if self.snapshot.connection_state == "reconnecting"
            && !self.reconnect_in_flight
            && self
                .next_reconnect_attempt
                .map(|attempt| Instant::now() >= attempt)
                .unwrap_or(false)
        {
            match self.downloader.reconnect() {
                Ok(()) => self.reconnect_in_flight = true,
                Err(error) => {
                    LOGGER.error(format!("failed to request Sendspin reconnect: {error}"));
                    self.next_reconnect_attempt = Some(Instant::now() + Duration::from_secs(2));
                }
            }
        }
        let Some(content_key) = self.current_cache_content_key.clone() else {
            return;
        };
        if self.downloader.take_download_window_refresh() {
            if let Err(error) = self.sync_download_window() {
                LOGGER.error(format!("failed to refresh Sendspin download window: {error}"));
            }
        }
        if self.downloader.take_download_status_refresh() {
            self.refresh_queue_cache_states();
        }
        self.refresh_and_log_download_status();
        self.start_prepared_audio_if_available(&content_key);
        if !self.audio_player.is_started() {
            self.refresh_queue_cache_states();
            return;
        }
        self.pump_audio_player(&content_key);
        if self.audio_player.state() == AudioPlayerState::Ended {
            let Some(current_index) = self.snapshot.current_index else {
                self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
                return;
            };
            let next_index = match self.snapshot.repeat_mode.as_str() {
                "one" => Some(current_index),
                "all" if !self.snapshot.queue.is_empty() => {
                    Some((current_index + 1) % self.snapshot.queue.len())
                }
                _ => current_index
                    .checked_add(1)
                    .filter(|index| *index < self.snapshot.queue.len()),
            };
            if let Some(next_index) = next_index {
                if self.play_queue_index(next_index).is_ok() {
                    return;
                }
            }
            self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
            return;
        }
        self.refresh_queue_cache_states();
        self.snapshot.local_playback_position_millis = Some(
            self.playback_position_offset_millis
                .saturating_add(self.audio_player.playback_position_millis()),
        );
        self.snapshot.playback_state = match self.audio_player.state() {
            AudioPlayerState::Buffering => PlaybackState::Buffering,
            AudioPlayerState::Playing => PlaybackState::Playing,
            AudioPlayerState::Paused => PlaybackState::Paused,
            AudioPlayerState::Ended => PlaybackState::Idle,
            AudioPlayerState::Error => PlaybackState::Error,
            AudioPlayerState::Idle => PlaybackState::Idle,
        }
        .as_str()
        .to_string();
    }

    fn pump_audio_player(&mut self, content_key: &str) {
        self.audio_player.pump_until(750, |chunk_index| {
            match self.downloader.cache_chunk_with_duration(content_key, chunk_index) {
                Ok((Some((data, duration_millis)), _)) => SourceReadResult::Data {
                    data,
                    duration_millis,
                },
                Ok((None, true)) => SourceReadResult::EndOfStream,
                Ok((None, false)) => SourceReadResult::Waiting,
                Err(error) => SourceReadResult::Error(error),
            }
        });
    }

    fn start_prepared_audio_if_available(&mut self, content_key: &str) {
        if self.audio_player.is_started() {
            return;
        }
        let Ok(format) = self.downloader.cache_format(content_key) else {
            return;
        };
        if let Err(error) =
            self.audio_player
                .start(&format, self.snapshot.volume, self.snapshot.muted)
        {
            LOGGER.error(format!("failed to start audio playback: {error}"));
            self.snapshot.playback_state = PlaybackState::Error.as_str().to_string();
        }
    }

    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        if self.audio_player.state() == AudioPlayerState::Idle {
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "cannot play without an active track".to_string())?;
            return self.play_cached_content(&content_key);
        }
        self.audio_player.resume();
        self.snapshot.playback_state = PlaybackState::Playing.as_str().to_string();
        Ok(())
    }

    pub(crate) fn transport_pause(&mut self) {
        self.audio_player.pause();
        self.snapshot.playback_state = PlaybackState::Paused.as_str().to_string();
    }

    pub(crate) fn transport_stop(&mut self) {
        self.audio_player.stop();
        self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
        self.snapshot.local_playback_position_millis = Some(0);
    }

    pub(crate) fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        let position = if seconds.is_finite() {
            (seconds.max(0.0) * 1000.0) as i64
        } else {
            0
        };
        let content_key = self
            .current_cache_content_key
            .clone()
            .ok_or_else(|| "cannot seek without an active track".to_string())?;

        if let Some(target) = self
            .downloader
            .local_cache_seek_target(&content_key, position as u64)
        {
            LOGGER.debug(format!(
                "transport seek served from local cache: target {} ms, chunk {}, chunk start {} ms",
                position, target.chunk_index, target.chunk_start_position_millis
            ));
            self.play_cached_content_with_seek(
                &content_key,
                target.chunk_index,
                target.skip_within_chunk_millis,
            )?;
            self.playback_position_offset_millis = position;
            self.snapshot.local_playback_position_millis = Some(position);
            return Ok(());
        }
        LOGGER.debug(format!(
            "transport seek delegated to downloader: target {} ms is not cached",
            position
        ));
        self.downloader.seek(&content_key, position / 1000)?;
        self.play_cached_content(&content_key)?;
        self.playback_position_offset_millis = position;
        Ok(())
    }

    pub(crate) fn transport_next(&mut self) -> Result<(), String> {
        let Some(current_index) = self.snapshot.current_index else {
            return Ok(());
        };
        let next_index = current_index.saturating_add(1);
        if next_index >= self.snapshot.queue.len() {
            self.transport_stop();
            return Ok(());
        }
        self.play_queue_index(next_index)
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        let Some(current_index) = self.snapshot.current_index else {
            return Ok(());
        };
        self.play_queue_index(current_index.saturating_sub(1))
    }

    pub(crate) fn set_volume(&mut self, volume: u8) {
        self.snapshot.volume = volume.min(100);
        self.audio_player.set_volume(self.snapshot.volume);
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.snapshot.muted = muted;
        self.audio_player.set_muted(muted);
    }

    pub(crate) fn set_shuffle(&mut self, enabled: bool) {
        if self.snapshot.shuffle_enabled == enabled {
            return;
        }
        let Some(current_index) = self.snapshot.current_index else {
            self.snapshot.shuffle_enabled = enabled;
            return;
        };
        let mut queue = self.queue_media_items();
        let upcoming = queue.get_mut(current_index.saturating_add(1)..);
        if let Some(upcoming) = upcoming {
            if enabled {
                Self::shuffle_items(upcoming);
            } else {
                upcoming.sort_by_key(|item| item.original_index);
            }
        }
        if let Err(error) = self.replace_queue_media_items(queue) {
            LOGGER.error(format!("failed to update queue for shuffle: {error}"));
            return;
        }
        self.snapshot.shuffle_enabled = enabled;
    }

    fn shuffle_items(items: &mut [QueueMediaItem]) {
        if items.len() < 2 {
            return;
        }
        let original_order = items
            .iter()
            .map(|item| item.queue_item_id.clone())
            .collect::<Vec<_>>();
        let mut state = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0)
            ^ items.len() as u64;
        for index in (1..items.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let target = (state as usize) % (index + 1);
            items.swap(index, target);
        }
        if items
            .iter()
            .map(|item| &item.queue_item_id)
            .eq(original_order.iter())
        {
            items.swap(0, 1);
        }
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: &str) {
        self.snapshot.repeat_mode = mode.to_string();
    }

    pub(crate) fn snapshot(&self) -> &SendspinSnapshot {
        &self.snapshot
    }

    pub(crate) fn queue_revision(&self) -> u64 {
        self.queue_revision
    }

    pub(crate) fn cache_updates(&self) -> Vec<QueueCacheUpdate> {
        self.snapshot
            .queue
            .iter()
            .map(|item| QueueCacheUpdate {
                queue_item_id: item.queue_item_id.clone(),
                cache: item.cache.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn queue_item_ids_stay_stable_when_inserting_media() {
        let mut engine = SendspinEngine::new();
        let initial_items =
            engine.new_queue_media_items(vec![json!({"id": "first"}), json!({"id": "second"})]);
        engine.snapshot.queue = initial_items
            .iter()
            .map(|item| QueueItemSnapshot {
                queue_item_id: item.queue_item_id.clone(),
                original_index: item.original_index,
                media_item: item.media_item.clone(),
                cache: QueueItemCacheSnapshot {
                    state: "queued".to_string(),
                    cached_duration_millis: 0,
                },
            })
            .collect();

        let mut rebuilt_items = engine.queue_media_items();
        rebuilt_items.splice(
            1..1,
            engine.new_queue_media_items(vec![json!({"id": "inserted"})]),
        );

        assert_eq!(
            rebuilt_items
                .iter()
                .map(|item| item.queue_item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["local-1", "local-3", "local-2"],
        );
    }

    #[test]
    fn queue_download_plan_keeps_the_selected_item_current() {
        let mut engine = SendspinEngine::new();
        let queue_items = engine.new_queue_media_items(vec![
            json!({"provider": "test", "item_id": "first"}),
            json!({"provider": "test", "item_id": "second"}),
            json!({"provider": "test", "item_id": "third"}),
        ]);

        let plan = engine
            .build_queue_download_plan(queue_items, 1)
            .expect("build queue plan");
        let items = plan["items"].as_array().expect("plan items");

        assert_eq!(items[0]["role"], "future");
        assert_eq!(items[1]["role"], "current");
        assert_eq!(items[2]["role"], "next");
    }

    #[test]
    fn queue_download_plan_uses_music_assistant_item_ids_for_cache_keys() {
        let mut engine = SendspinEngine::new();
        let queue_items = engine.new_queue_media_items(vec![
            json!({"provider": "deezer", "item_id": "first"}),
            json!({"provider": "deezer", "item_id": "second"}),
        ]);

        let plan = engine
            .build_queue_download_plan(queue_items, 0)
            .expect("build queue plan");
        let items = plan["items"].as_array().expect("plan items");

        assert_eq!(items[0]["cacheContentKey"], "deezer:first:pcm:48000:16:2");
        assert_eq!(items[1]["cacheContentKey"], "deezer:second:pcm:48000:16:2");
    }

    #[test]
    fn shuffling_upcoming_items_keeps_the_current_item_in_place() {
        let mut engine = SendspinEngine::new();
        let mut queue = engine.new_queue_media_items(vec![
            json!({"id": "played"}),
            json!({"id": "current"}),
            json!({"id": "third"}),
            json!({"id": "fourth"}),
        ]);
        let current_id = queue[1].queue_item_id.clone();
        let played_id = queue[0].queue_item_id.clone();
        let upcoming_ids = queue[2..]
            .iter()
            .map(|item| item.queue_item_id.clone())
            .collect::<Vec<_>>();

        SendspinEngine::shuffle_items(&mut queue[2..]);

        assert_eq!(queue[0].queue_item_id, played_id);
        assert_eq!(queue[1].queue_item_id, current_id);
        assert_ne!(
            queue[2..]
                .iter()
                .map(|item| item.queue_item_id.clone())
                .collect::<Vec<_>>(),
            upcoming_ids
        );
        assert_eq!(
            {
                let mut original_indexes = queue[2..]
                .iter()
                .map(|item| item.original_index)
                .collect::<Vec<_>>();
                original_indexes.sort_unstable();
                original_indexes
            },
            vec![2, 3]
        );
    }

    #[test]
    fn disabling_shuffle_restores_upcoming_items_to_original_order() {
        let mut engine = SendspinEngine::new();
        let mut queue = engine.new_queue_media_items(vec![
            json!({"id": "played"}),
            json!({"id": "current"}),
            json!({"id": "third"}),
            json!({"id": "fourth"}),
            json!({"id": "fifth"}),
        ]);
        queue[2..].reverse();

        queue[2..].sort_by_key(|item| item.original_index);

        assert_eq!(
            queue.iter().map(|item| item.original_index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn shuffling_replaces_the_downloader_plan_with_the_new_queue_order() {
        let mut engine = SendspinEngine::new();
        engine
            .play_media(
                &serde_json::to_string(&vec![
                    json!({"provider": "test", "item_id": "first"}),
                    json!({"provider": "test", "item_id": "second"}),
                    json!({"provider": "test", "item_id": "third"}),
                    json!({"provider": "test", "item_id": "fourth"}),
                ])
                .expect("serialize media items"),
            )
            .expect("start playback");
        let original_upcoming_ids = engine.snapshot.queue[1..]
            .iter()
            .map(|item| item.queue_item_id.clone())
            .collect::<Vec<_>>();

        engine.set_shuffle(true);

        let downloader_plan = engine.downloader.download_plan().expect("download plan");
        assert_eq!(engine.snapshot.shuffle_enabled, true);
        assert_eq!(downloader_plan.version, 2);
        assert_eq!(
            downloader_plan
                .items
                .iter()
                .map(|item| item.queue_item_id.clone())
                .collect::<Vec<_>>(),
            engine
                .snapshot
                .queue
                .iter()
                .map(|item| item.queue_item_id.clone())
                .collect::<Vec<_>>(),
        );
        assert_ne!(
            engine.snapshot.queue[1..]
                .iter()
                .map(|item| item.queue_item_id.clone())
                .collect::<Vec<_>>(),
            original_upcoming_ids,
        );
        assert_eq!(downloader_plan.items[0].role, "current");
        assert_eq!(downloader_plan.items[1].role, "next");
    }
}
