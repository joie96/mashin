use super::downloader::{SendspinDownloader, SendspinDownloaderConfig};
use super::models::{QueueItemCacheSnapshot, QueueItemSnapshot};
use crate::audio::player::{AudioPlayer, AudioPlayerState};
use crate::audio::source::{AudioSource, SourceReadResult};
use serde::Serialize;
use serde_json::Value;

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
    current_cache_content_key: Option<String>,
    queue_content_keys: Vec<String>,
    recently_played_content_keys: Vec<String>,
}

impl SendspinEngine {
    pub(crate) fn new() -> Self {
        Self {
            config: None,
            snapshot: SendspinSnapshot::default(),
            downloader: SendspinDownloader::new(),
            audio_player: AudioPlayer::new(),
            download_plan_version: 0,
            current_cache_content_key: None,
            queue_content_keys: Vec::new(),
            recently_played_content_keys: Vec::new(),
        }
    }

    pub(crate) fn init(&mut self, config: Value) {
        let downloader_config = SendspinDownloaderConfig {
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
        };
        self.config = Some(config);
        self.snapshot = SendspinSnapshot::default();
        self.download_plan_version = 0;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.recently_played_content_keys.clear();
        self.downloader = SendspinDownloader::new();
        self.downloader.configure(downloader_config);
        self.audio_player.stop();
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
        config["preferredCodec"] = Value::String(normalized);
        let was_online = self.snapshot.connection_state == "online";
        self.configure(config)?;
        if was_online {
            self.connect()?;
        }
        Ok(())
    }

    pub(crate) fn connect(&mut self) -> Result<(), String> {
        self.downloader.connect()?;
        self.snapshot.connection_state = "online".to_string();
        if self.snapshot.playback_state == PlaybackState::Uninitialized.as_str() {
            self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
        }
        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        self.downloader.disconnect();
        self.snapshot.connection_state = "offline".to_string();
        self.snapshot.playback_state = PlaybackState::Idle.as_str().to_string();
        self.current_cache_content_key = None;
    }

    pub(crate) fn apply_download_plan(&mut self, plan_json: &str) -> Result<(), String> {
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
                media_item: item.media_item.clone(),
                cache: QueueItemCacheSnapshot {
                    state: "queued".to_string(),
                    cached_duration_millis: 0,
                },
            })
            .collect();
        self.snapshot.current_index = plan.items.iter().position(|item| item.role == "current");
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

        let plan = self.build_download_plan(media_items)?;
        let plan_json = serde_json::to_string(&plan)
            .map_err(|error| format!("serialize Sendspin download plan failed: {error}"))?;
        self.apply_download_plan(&plan_json)?;
        let content_key = self
            .current_cache_content_key
            .clone()
            .ok_or_else(|| "play_media plan has no current item".to_string())?;
        self.play(&content_key)
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
        queue.splice(insertion..insertion, additions);
        self.update_media_queue(queue, false)
    }

    pub(crate) fn play_media_last(&mut self, media_items_json: &str) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        let additions: Vec<Value> = serde_json::from_str(media_items_json)
            .map_err(|error| format!("invalid media items json: {error}"))?;
        queue.extend(additions);
        self.update_media_queue(queue, false)
    }

    pub(crate) fn clear_queue(&mut self) -> Result<(), String> {
        self.snapshot.queue.clear();
        self.snapshot.current_index = None;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
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
        self.update_media_queue(queue, false)
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
        self.update_media_queue(queue, false)
    }

    fn queue_media_items(&self) -> Vec<Value> {
        self.snapshot
            .queue
            .iter()
            .map(|item| item.media_item.clone())
            .collect()
    }

    fn update_media_queue(&mut self, media_items: Vec<Value>, play: bool) -> Result<(), String> {
        if media_items.is_empty() {
            return self.clear_queue();
        }
        let plan = self.build_download_plan(media_items)?;
        let plan_json = serde_json::to_string(&plan)
            .map_err(|error| format!("serialize Sendspin download plan failed: {error}"))?;
        self.apply_download_plan(&plan_json)?;
        if play {
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "queue has no current item".to_string())?;
            self.play(&content_key)?;
        }
        Ok(())
    }

    fn build_download_plan(&self, media_items: Vec<Value>) -> Result<Value, String> {
        let preferred_codec = self
            .config
            .as_ref()
            .and_then(|config| config.get("preferredCodec"))
            .and_then(Value::as_str)
            .unwrap_or("pcm");

        let items = media_items
            .into_iter()
            .enumerate()
            .map(|(index, media_item)| {
                let queue_item_id = format!("local-{}", index + 1);
                let provider = media_item
                    .get("provider")
                    .or_else(|| media_item.get("providerId"))
                    .and_then(Value::as_str)
                    .unwrap_or("media");
                let item_id = media_item
                    .get("itemId")
                    .or_else(|| media_item.get("mediaItemId"))
                    .or_else(|| media_item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let cache_content_key =
                    format!("{provider}:{item_id}:{preferred_codec}:48000:16:2");
                let role = if index == 0 {
                    "current"
                } else if index == 1 {
                    "next"
                } else {
                    "future"
                };
                let priority = match role {
                    "current" => 100,
                    "next" => 90,
                    _ => 50,
                };
                serde_json::json!({
                    "queueItemId": queue_item_id,
                    "role": role,
                    "priority": priority,
                    "cacheContentKey": cache_content_key,
                    "mediaItem": media_item,
                })
            })
            .collect::<Vec<Value>>();

        Ok(serde_json::json!({
            "version": self.download_plan_version.saturating_add(1),
            "items": items,
        }))
    }

    pub(crate) fn play(&mut self, content_key: &str) -> Result<(), String> {
        let key = content_key.trim();
        if key.is_empty() {
            return Err("play content key is empty".to_string());
        }
        self.downloader.mark_cache_buffering(key)?;
        self.audio_player
            .prepare(AudioSource::cached_content(key.to_string()));
        let format = self.wait_for_cache_format(key)?;
        self.audio_player
            .start(&format, self.snapshot.volume, self.snapshot.muted)?;
        self.pump_audio(key);
        self.refresh_queue_cache_states();
        self.current_cache_content_key = Some(key.to_string());
        self.snapshot.playback_state = if self.audio_player.buffered_duration_millis() > 0 {
            PlaybackState::Playing.as_str().to_string()
        } else {
            PlaybackState::Buffering.as_str().to_string()
        };
        self.snapshot.local_playback_position_millis =
            Some(self.audio_player.playback_position_millis());
        Ok(())
    }

    pub(crate) fn pump_playback(&mut self) {
        let Some(content_key) = self.current_cache_content_key.clone() else {
            return;
        };
        self.pump_audio(&content_key);
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
        self.snapshot.local_playback_position_millis =
            Some(self.audio_player.playback_position_millis());
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

    fn pump_audio(&mut self, content_key: &str) {
        self.audio_player.pump_until(750, |chunk_index| {
            match self.downloader.cache_chunk(content_key, chunk_index) {
                Ok((Some(chunk), _)) => SourceReadResult::Data(chunk),
                Ok((None, true)) => SourceReadResult::EndOfStream,
                Ok((None, false)) => SourceReadResult::Waiting,
                Err(error) => SourceReadResult::Error(error),
            }
        });
    }

    fn wait_for_cache_format(
        &self,
        content_key: &str,
    ) -> Result<::sendspin::audio::AudioFormat, String> {
        for _ in 0..500 {
            if let Ok(format) = self.downloader.cache_format(content_key) {
                return Ok(format);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Err("timed out waiting for Sendspin cache format".to_string())
    }

    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        if self.audio_player.state() == AudioPlayerState::Idle {
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "cannot play without an active track".to_string())?;
            return self.play(&content_key);
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

        // TODO: Seek directly in the local cache when chunk/time indexing exists.
        self.downloader.seek(&content_key, position / 1000)?;
        self.snapshot.local_playback_position_millis = Some(position);
        self.snapshot.playback_state = PlaybackState::Buffering.as_str().to_string();
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

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        let Some(content_key) = self.queue_content_keys.get(index) else {
            return Err("queue index is out of range".to_string());
        };
        let content_key = content_key.clone();
        self.remember_current_track(&content_key);
        self.snapshot.current_index = Some(index);
        self.play(&content_key)?;
        self.sync_download_window()
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

    fn remember_current_track(&mut self, next_content_key: &str) {
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

    pub(crate) fn set_volume(&mut self, volume: u8) {
        self.snapshot.volume = volume.min(100);
        self.audio_player.set_volume(self.snapshot.volume);
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.snapshot.muted = muted;
        self.audio_player.set_muted(muted);
    }

    pub(crate) fn set_shuffle(&mut self, enabled: bool) {
        self.snapshot.shuffle_enabled = enabled;
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: &str) {
        self.snapshot.repeat_mode = mode.to_string();
    }

    pub(crate) fn snapshot(&self) -> &SendspinSnapshot {
        &self.snapshot
    }
}
