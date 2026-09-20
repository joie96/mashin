use super::downloader::{SendspinDownloader, SendspinDownloaderConfig};
use super::models::{
    SendspinAudioCodec, SendspinDownloadPlan, SendspinDownloadPlanItem, SendspinDownloadRole,
    SendspinEngineConfiguration,
};
use crate::audio::player::{AudioPlayer, AudioPlayerState};
use crate::audio::source::{AudioSource, SourceReadResult};
use crate::logging::Logger;
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::identifiers::{CacheContentKey, QueueItemId};
use crate::music_assistant::models::media::{MediaItem, Track};
use crate::music_assistant::models::queue::{
    CacheState, CacheStatus, PlayState, QueueItem, RepeatMode,
};
use crate::playback::autoplay::{get_autoplay_refill_tracks, AutoplayController};
use crate::playback::engine::{PlaybackEngine, PreferredCodecConfigurablePlaybackEngine};
use crate::playback::models::{
    PlaybackConnectionState, PlaybackEngineConfiguration, PlaybackEngineKind, PlayerState, Queue,
};
use crate::playback::runtime::PeriodicWorker;
use once_cell::sync::Lazy;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

static LOGGER: Logger = Logger::new(module_path!());
static AUTOPLAY_NETWORK_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("create Sendspin autoplay network runtime")
});
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(2);
const MAXIMUM_RECONNECT_DELAY: Duration = Duration::from_secs(8);

pub(crate) struct SendspinEngine {
    config: Option<SendspinEngineConfiguration>,
    queue: Queue,
    player_state: PlayerState,
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
    reconnect_delay: Duration,
    is_reconnect_in_flight: bool,
    playback_position_offset_millis: i64,
    autoplay: AutoplayController,
    autoplay_result_sender: Sender<Vec<Track>>,
    autoplay_result_receiver: Receiver<Vec<Track>>,
    background_worker: Option<PeriodicWorker>,
    last_player_state_event_at: Option<Instant>,
}

#[derive(Clone)]
struct QueueMediaItem {
    queue_item_id: QueueItemId,
    original_index: usize,
    media_item: MediaItem,
}

impl SendspinEngine {
    // Construction and configuration.
    pub(crate) fn new() -> Self {
        let (autoplay_result_sender, autoplay_result_receiver) = mpsc::channel();
        Self {
            config: None,
            queue: Queue::default(),
            player_state: PlayerState::default(),
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
            reconnect_delay: INITIAL_RECONNECT_DELAY,
            is_reconnect_in_flight: false,
            playback_position_offset_millis: 0,
            autoplay: AutoplayController::new(),
            autoplay_result_sender,
            autoplay_result_receiver,
            background_worker: None,
            last_player_state_event_at: None,
        }
    }

    pub(crate) fn init(&mut self, config: SendspinEngineConfiguration) {
        let downloader_config = Self::downloader_config(&config);
        self.config = Some(config);
        self.queue = Queue::default();
        self.player_state = PlayerState::default();
        self.download_plan_version = 0;
        self.next_queue_item_id = 1;
        self.next_original_index = 0;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.recently_played_content_keys.clear();
        self.last_download_status_log = None;
        self.next_reconnect_attempt = None;
        self.reconnect_delay = INITIAL_RECONNECT_DELAY;
        self.is_reconnect_in_flight = false;
        self.playback_position_offset_millis = 0;
        self.autoplay = AutoplayController::new();
        self.last_player_state_event_at = None;
        self.downloader = SendspinDownloader::new();
        self.downloader.configure(downloader_config);
        self.audio_player.stop();
    }

    fn downloader_config(config: &SendspinEngineConfiguration) -> SendspinDownloaderConfig {
        SendspinDownloaderConfig {
            sendspin_url: config.sendspin_url.clone(),
            music_assistant_url: config.music_assistant_url.clone(),
            music_assistant_auth_token: config.music_assistant_auth_token.clone(),
            client_id: config.client_id.clone(),
            client_name: config.client_name.clone(),
            downloader_buffer_bytes: config.downloader_buffer_bytes,
            cache_bytes: config.cache_bytes,
            preferred_codec: config.preferred_codec,
        }
    }

    pub(crate) fn configure(&mut self, config: SendspinEngineConfiguration) -> Result<(), String> {
        if config.sendspin_url.trim().is_empty() {
            return Err("Sendspin URL is required".to_string());
        }
        LOGGER.info(format!(
            "Sendspin configured with preferred codec {}",
            config.preferred_codec.as_str()
        ));
        self.init(config);
        Ok(())
    }

    pub(crate) fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String> {
        let preferred_codec = match codec.trim().to_ascii_lowercase().as_str() {
            "pcm" => SendspinAudioCodec::Pcm,
            "opus" => SendspinAudioCodec::Opus,
            "flac" => SendspinAudioCodec::Flac,
            value => return Err(format!("unsupported Sendspin codec: {value}")),
        };
        let mut config = self
            .config
            .clone()
            .ok_or_else(|| "Sendspin engine is not configured".to_string())?;
        if config.preferred_codec == preferred_codec {
            return Ok(());
        }
        config.preferred_codec = preferred_codec;
        let playback_position_millis = self
            .player_state
            .position_millis
            .unwrap_or_else(|| self.audio_player.playback_position_millis())
            .max(0);
        let queue_items = self.queue_media_items();
        let current_index = self.queue.current_index;
        let was_connected = matches!(
            self.player_state.connection_state,
            PlaybackConnectionState::Online | PlaybackConnectionState::Reconnecting
        );
        self.config = Some(config.clone());
        let downloader_config = Self::downloader_config(&config);
        if was_connected {
            self.downloader
                .restart_for_codec_change(downloader_config)?;
        } else {
            self.downloader.configure(downloader_config);
        }
        if let Some(current_index) = current_index {
            let plan = self.build_queue_download_plan(queue_items, current_index)?;
            self.downloader
                .set_pending_current_seek(playback_position_millis / 1_000)?;
            self.apply_queue_download_plan(plan)?;
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "codec-change plan has no current item".to_string())?;
            self.play_cached_content(&content_key)?;
            self.playback_position_offset_millis = playback_position_millis;
        }
        Ok(())
    }

    // Connection lifecycle.
    pub(crate) fn connect(&mut self) -> Result<(), String> {
        LOGGER.info("connecting Sendspin engine");
        self.downloader.connect()?;
        self.player_state.connection_state = PlaybackConnectionState::Online;
        if self.player_state.playback_state == PlayState::Uninitialized {
            self.player_state.playback_state = PlayState::Idle;
        }
        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        LOGGER.info("disconnecting Sendspin engine");
        self.downloader.disconnect();
        self.player_state.connection_state = PlaybackConnectionState::Offline;
        self.player_state.playback_state = PlayState::Idle;
        self.current_cache_content_key = None;
        self.next_reconnect_attempt = None;
        self.reconnect_delay = INITIAL_RECONNECT_DELAY;
        self.is_reconnect_in_flight = false;
    }

    // Shutdown clears the in-memory Music Assistant session and stops all local playback work.
    pub(crate) fn shutdown(&mut self) {
        self.stop_background_work();
        self.disconnect();
        *self = Self::new();
    }

    // Queue commands.
    pub(crate) fn apply_queue_download_plan(
        &mut self,
        plan: SendspinDownloadPlan,
    ) -> Result<(), String> {
        self.downloader.apply_download_plan(plan.clone())?;
        self.download_plan_version = plan.version;
        self.current_cache_content_key = plan
            .items
            .iter()
            .find(|item| item.role == SendspinDownloadRole::Current)
            .map(|item| item.cache_content_key.as_str().to_string());
        self.queue_content_keys = plan
            .items
            .iter()
            .map(|item| item.cache_content_key.as_str().to_string())
            .collect();
        self.queue.items = plan
            .items
            .iter()
            .map(|item| {
                QueueItem::local(
                    self.queue.queue_id.clone(),
                    item.queue_item_id.clone(),
                    item.original_index,
                    item.media_item.clone(),
                )
            })
            .collect();
        self.queue.current_index = plan
            .items
            .iter()
            .position(|item| item.role == SendspinDownloadRole::Current);
        LOGGER.debug(format!(
            "queue download plan applied: version {}, {} items, current index {:?}",
            plan.version,
            plan.items.len(),
            self.queue.current_index
        ));
        self.queue_revision = self.queue_revision.saturating_add(1);
        self.sync_download_window()?;
        self.refresh_queue_cache_states();
        Ok(())
    }

    pub(crate) fn play_media(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        if media_items.is_empty() {
            return Err("play_media requires at least one media item".to_string());
        }
        LOGGER.info(format!(
            "starting playback for {} media items",
            media_items.len()
        ));

        let seed_tracks = tracks_from_media_items(&media_items);
        let queue_items = self.new_queue_media_items(media_items);
        let plan = self.build_queue_download_plan(queue_items, 0)?;
        self.apply_queue_download_plan(plan)?;
        let content_key = self
            .current_cache_content_key
            .clone()
            .ok_or_else(|| "play_media plan has no current item".to_string())?;
        self.play_cached_content(&content_key)?;
        self.autoplay.replace_seed_tracks(seed_tracks);
        Ok(())
    }

    pub(crate) fn play_media_next(
        &mut self,
        media_items: Vec<MediaItem>,
        replace_following: bool,
    ) -> Result<(), String> {
        let seed_tracks = tracks_from_media_items(&media_items);
        let mut queue = self.queue_media_items();
        let insertion = self
            .queue
            .current_index
            .map(|index| index.saturating_add(1))
            .unwrap_or(queue.len())
            .min(queue.len());
        if replace_following {
            queue.truncate(insertion);
        }
        queue.splice(
            insertion..insertion,
            self.new_queue_media_items(media_items),
        );
        self.replace_queue_media_items(queue)?;
        if replace_following {
            self.autoplay.replace_seed_tracks(seed_tracks);
        } else {
            self.autoplay.append_seed_tracks(seed_tracks);
        }
        Ok(())
    }

    pub(crate) fn play_media_last(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        let seed_tracks = tracks_from_media_items(&media_items);
        let mut queue = self.queue_media_items();
        queue.extend(self.new_queue_media_items(media_items));
        self.replace_queue_media_items(queue)?;
        self.autoplay.append_seed_tracks(seed_tracks);
        Ok(())
    }

    pub(crate) fn append_radio_tracks(
        &mut self,
        seed_tracks: Vec<Track>,
        radio_tracks: Vec<Track>,
    ) -> Result<(), String> {
        let radio_media_items = radio_tracks.into_iter().map(MediaItem::Track).collect();
        let mut queue = self.queue_media_items();
        queue.extend(self.new_queue_media_items(radio_media_items));
        self.replace_queue_media_items(queue)?;
        self.autoplay.append_seed_tracks(seed_tracks);
        Ok(())
    }

    pub(crate) fn clear_queue(&mut self) -> Result<(), String> {
        LOGGER.info("clearing playback queue");
        self.audio_player.stop();
        self.queue.items.clear();
        self.queue.current_index = None;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.next_original_index = 0;
        self.autoplay = AutoplayController::new();
        self.queue_revision = self.queue_revision.saturating_add(1);
        self.player_state.playback_state = PlayState::Idle;
        Ok(())
    }

    pub(crate) fn move_queue_item(
        &mut self,
        item_id: QueueItemId,
        shift: i32,
    ) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        let Some(source) = self
            .queue
            .items
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

    pub(crate) fn delete_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        let Some(index) = self
            .queue
            .items
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
        self.queue.current_index = Some(index);
        self.play_cached_content(&content_key)?;
        self.sync_download_window()
    }

    // Queue planning helpers.
    fn queue_media_items(&self) -> Vec<QueueMediaItem> {
        self.queue
            .items
            .iter()
            .filter_map(|item| {
                Some(QueueMediaItem {
                    queue_item_id: item.queue_item_id.clone(),
                    original_index: item.sort_index as usize,
                    media_item: item.media_item.clone()?,
                })
            })
            .collect()
    }

    fn new_queue_media_items(&mut self, media_items: Vec<MediaItem>) -> Vec<QueueMediaItem> {
        media_items
            .into_iter()
            .map(|media_item| {
                let queue_item_id = QueueItemId(format!("local-{}", self.next_queue_item_id));
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
        let previous_current_index = self.queue.current_index;
        let previous_current_queue_item_id = self.current_queue_item_id().cloned();
        let current_index = previous_current_queue_item_id
            .as_ref()
            .and_then(|queue_item_id| {
                media_items
                    .iter()
                    .position(|item| &item.queue_item_id == queue_item_id)
            })
            .or_else(|| {
                previous_current_index.map(|index| index.min(media_items.len().saturating_sub(1)))
            })
            .unwrap_or(0);
        let plan = self.build_queue_download_plan(media_items, current_index)?;
        self.apply_queue_download_plan(plan)?;
        let current_queue_item_id = self.current_queue_item_id();
        if previous_current_queue_item_id.as_ref() != current_queue_item_id {
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "queue has no current item".to_string())?;
            self.play_cached_content(&content_key)?;
        }
        Ok(())
    }

    fn current_queue_item_id(&self) -> Option<&QueueItemId> {
        self.queue
            .current_index
            .and_then(|index| self.queue.items.get(index))
            .map(|item| &item.queue_item_id)
    }

    fn build_queue_download_plan(
        &self,
        media_items: Vec<QueueMediaItem>,
        current_index: usize,
    ) -> Result<SendspinDownloadPlan, String> {
        let preferred_codec = self
            .config
            .as_ref()
            .ok_or_else(|| "Sendspin engine is not configured".to_string())?
            .preferred_codec;

        let items = media_items
            .into_iter()
            .enumerate()
            .map(
                |(index, queue_item)| -> Result<SendspinDownloadPlanItem, String> {
                    let QueueMediaItem {
                        queue_item_id,
                        original_index,
                        media_item,
                    } = queue_item;
                    let fields = media_item.fields();
                    let provider = fields.provider.as_str().trim();
                    let item_id = fields.item_id.as_str().trim();
                    if provider.is_empty() || item_id.is_empty() {
                        return Err(format!(
                            "queue item {} has no Music Assistant provider or item ID",
                            queue_item_id.as_str()
                        ));
                    }
                    let cache_content_key = CacheContentKey(format!(
                        "{provider}:{item_id}:{}:48000:16:2",
                        preferred_codec.as_str()
                    ));
                    let expected_duration_millis = media_item
                        .duration_seconds()
                        .filter(|duration| *duration > 0)
                        .map(|duration| u64::from(duration).saturating_mul(1_000));
                    let role = if index == current_index {
                        SendspinDownloadRole::Current
                    } else if index == current_index.saturating_add(1) {
                        SendspinDownloadRole::Next
                    } else {
                        SendspinDownloadRole::Future
                    };
                    let priority = match role {
                        SendspinDownloadRole::Current => 100,
                        SendspinDownloadRole::Next => 90,
                        SendspinDownloadRole::Future => 50,
                    };
                    Ok(SendspinDownloadPlanItem {
                        queue_item_id,
                        original_index,
                        role,
                        priority,
                        cache_content_key,
                        expected_duration_millis,
                        media_item,
                    })
                },
            )
            .collect::<Result<Vec<_>, String>>()?;

        Ok(SendspinDownloadPlan {
            version: self.download_plan_version.saturating_add(1),
            items,
        })
    }

    // Download-window and cache synchronization.
    fn sync_download_window(&mut self) -> Result<(), String> {
        let download_window = self
            .queue_content_keys
            .iter()
            .skip(self.queue.current_index.unwrap_or(0))
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
        for item in &mut self.queue.items {
            if let Some(state) = states
                .iter()
                .find(|state| state.queue_item_id == item.queue_item_id)
            {
                item.cache = Some(CacheState {
                    status: state.state,
                    cached_duration_millis: state.cached_duration_millis,
                });
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
        let (active_downloads, cache_used_bytes, cache_bytes) = self.downloader.download_status();
        let completed_cache_items = self
            .downloader
            .completed_cache_item_count(&self.queue_content_keys);
        let active_downloads = active_downloads
            .iter()
            .map(|download| {
                let track_name = self
                    .queue
                    .items
                    .iter()
                    .find(|item| item.queue_item_id == download.queue_item_id)
                    .and_then(|item| item.media_item.as_ref())
                    .map(|media_item| media_item.fields().name.as_str())
                    .unwrap_or_else(|| download.queue_item_id.as_str());
                let expected_duration_millis = download.expected_duration_millis.unwrap_or(0);
                format!(
                    "{} {track_name} {}/{} ms ({:.1}%)",
                    download_status_symbol(download.state),
                    download.cached_duration_millis,
                    expected_duration_millis,
                    Self::percentage(download.cached_duration_millis, expected_duration_millis,)
                )
            })
            .collect::<Vec<_>>();
        let cache_usage_percent = Self::percentage(cache_used_bytes as u64, cache_bytes as u64);
        LOGGER.info(format!(
            "download status: {}/{} complete queue item(s), [{}], cache {} KiB/{} KiB ({:.1}%)",
            completed_cache_items,
            self.queue_content_keys.len(),
            active_downloads.join(", "),
            cache_used_bytes / 1024,
            cache_bytes / 1024,
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

    // Cached playback and background processing.
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
        self.player_state.playback_state = PlayState::Buffering;
        self.player_state.position_millis = Some(0);
        self.start_prepared_audio_if_available(key);
        self.refresh_queue_cache_states();
        Ok(())
    }

    // Background playback processing.
    fn schedule_reconnect_attempt(&mut self) {
        let delay = self.reconnect_delay;
        self.next_reconnect_attempt = Some(Instant::now() + delay);
        LOGGER.trace(format!(
            "Sendspin reconnect scheduled in {} seconds",
            delay.as_secs()
        ));
        self.reconnect_delay = self
            .reconnect_delay
            .saturating_mul(2)
            .min(MAXIMUM_RECONNECT_DELAY);
    }

    fn process_connection_state(&mut self) {
        if self.downloader.take_connection_lost() {
            LOGGER.trace("Sendspin downloader connection lost");
            if self.player_state.connection_state != PlaybackConnectionState::Reconnecting {
                self.player_state.connection_state = PlaybackConnectionState::Reconnecting;
                self.schedule_reconnect_attempt();
            } else if self.is_reconnect_in_flight {
                self.is_reconnect_in_flight = false;
                self.schedule_reconnect_attempt();
            }
        }
        if self.downloader.take_connection_ready() {
            LOGGER.trace("Sendspin downloaders reconnected");
            self.player_state.connection_state = PlaybackConnectionState::Online;
            self.next_reconnect_attempt = None;
            self.reconnect_delay = INITIAL_RECONNECT_DELAY;
            self.is_reconnect_in_flight = false;
            if let Err(error) = self.sync_download_window() {
                LOGGER.error(format!("failed to resume Sendspin downloads: {error}"));
            }
        }
        if self.player_state.connection_state == PlaybackConnectionState::Reconnecting
            && !self.is_reconnect_in_flight
            && self
                .next_reconnect_attempt
                .map(|attempt| Instant::now() >= attempt)
                .unwrap_or(false)
        {
            LOGGER.trace("starting Sendspin downloader reconnect attempt");
            match self.downloader.reconnect() {
                Ok(()) => self.is_reconnect_in_flight = true,
                Err(error) => {
                    LOGGER.error(format!("failed to request Sendspin reconnect: {error}"));
                    self.schedule_reconnect_attempt();
                }
            }
        }
    }

    fn process_download_window(&mut self) {
        if self.downloader.take_download_window_refresh() {
            if let Err(error) = self.sync_download_window() {
                LOGGER.error(format!(
                    "failed to refresh Sendspin download window: {error}"
                ));
            }
        }
        if self.downloader.take_download_status_refresh() {
            self.refresh_queue_cache_states();
        }
        self.refresh_and_log_download_status();
    }

    fn process_cached_playback(&mut self, content_key: &str) {
        self.start_prepared_audio_if_available(&content_key);
        if !self.audio_player.is_started() {
            self.refresh_queue_cache_states();
            return;
        }
        self.pump_audio_player(content_key);
        if self.audio_player.state() == AudioPlayerState::Ended {
            let Some(current_index) = self.queue.current_index else {
                self.player_state.playback_state = PlayState::Idle;
                return;
            };
            let next_index = match self.queue.repeat_mode {
                RepeatMode::One => Some(current_index),
                RepeatMode::All if !self.queue.items.is_empty() => {
                    Some((current_index + 1) % self.queue.items.len())
                }
                _ => current_index
                    .checked_add(1)
                    .filter(|index| *index < self.queue.items.len()),
            };
            if let Some(next_index) = next_index {
                if self.play_queue_index(next_index).is_ok() {
                    return;
                }
            }
            self.player_state.playback_state = PlayState::Idle;
            return;
        }
        self.refresh_queue_cache_states();
        self.player_state.position_millis = Some(
            self.playback_position_offset_millis
                .saturating_add(self.audio_player.playback_position_millis()),
        );
        self.player_state.playback_state = match self.audio_player.state() {
            AudioPlayerState::Buffering => PlayState::Buffering,
            AudioPlayerState::Playing => PlayState::Playing,
            AudioPlayerState::Paused => PlayState::Paused,
            AudioPlayerState::Ended => PlayState::Idle,
            AudioPlayerState::Error => PlayState::Error,
            AudioPlayerState::Idle => PlayState::Idle,
        };
    }

    fn process_autoplay_results(&mut self) {
        while let Ok(tracks) = self.autoplay_result_receiver.try_recv() {
            self.autoplay.finish_refill();
            if tracks.is_empty() {
                continue;
            }
            let mut queue = self.queue_media_items();
            queue.extend(
                self.new_queue_media_items(tracks.into_iter().map(MediaItem::Track).collect()),
            );
            if let Err(error) = self.replace_queue_media_items(queue) {
                LOGGER.error(format!("failed to append autoplay tracks: {error}"));
            }
        }
    }

    fn request_autoplay_refill_if_needed(&mut self) {
        let Some(seed_tracks) = self
            .autoplay
            .reserve_refill(self.queue.items.len(), self.queue.current_index)
        else {
            return;
        };
        let Some(config) = self.config.as_ref() else {
            self.autoplay.finish_refill();
            return;
        };
        let queued_tracks = tracks_from_queue(&self.queue.items);
        let music_assistant_url = config.music_assistant_url.clone();
        let access_token = config.music_assistant_auth_token.clone();
        let result_sender = self.autoplay_result_sender.clone();
        std::thread::spawn(move || {
            let client = MusicAssistantClient::new(&music_assistant_url, access_token);
            match AUTOPLAY_NETWORK_RUNTIME.block_on(get_autoplay_refill_tracks(
                &client,
                seed_tracks,
                queued_tracks,
            )) {
                Ok(tracks) => {
                    let _ = result_sender.send(tracks);
                }
                Err(error) => {
                    LOGGER.error(format!("failed to load autoplay tracks: {error}"));
                    let _ = result_sender.send(Vec::new());
                }
            }
        });
    }

    fn publish_background_events(&mut self, queue_before: Queue, player_state_before: PlayerState) {
        let queue = self.queue().clone();
        if !queue.has_same_structure_as(&queue_before) {
            crate::ffi::event_channel::publish_media_session_event(
                crate::core::models::MediaSessionEvent::QueueChanged { queue },
            );
        }
        let player_state = self.player_state();
        let should_publish_position = self
            .last_player_state_event_at
            .map(|last_event_at| last_event_at.elapsed() >= Duration::from_millis(250))
            .unwrap_or(true);
        if player_state != player_state_before
            && (!player_state.has_same_non_position_as(&player_state_before)
                || should_publish_position)
        {
            crate::ffi::event_channel::publish_media_session_event(
                crate::core::models::MediaSessionEvent::PlayerStateChanged { player_state },
            );
            self.last_player_state_event_at = Some(Instant::now());
        }
    }

    fn pump_audio_player(&mut self, content_key: &str) {
        self.audio_player.pump_until(750, |chunk_index| {
            match self
                .downloader
                .cache_chunk_with_duration(content_key, chunk_index)
            {
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
                .start(&format, self.player_state.volume, self.player_state.muted)
        {
            LOGGER.error(format!("failed to start audio playback: {error}"));
            self.player_state.playback_state = PlayState::Error;
        }
    }

    // Playback transport commands.
    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        if self.audio_player.state() == AudioPlayerState::Idle {
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or_else(|| "cannot play without an active track".to_string())?;
            return self.play_cached_content(&content_key);
        }
        self.audio_player.resume();
        self.player_state.playback_state = PlayState::Playing;
        Ok(())
    }

    pub(crate) fn transport_pause(&mut self) {
        self.audio_player.pause();
        self.player_state.playback_state = PlayState::Paused;
    }

    pub(crate) fn transport_stop(&mut self) {
        self.audio_player.stop();
        self.player_state.playback_state = PlayState::Idle;
        self.player_state.position_millis = Some(0);
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
            self.player_state.position_millis = Some(position);
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
        let Some(current_index) = self.queue.current_index else {
            return Ok(());
        };
        let next_index = current_index.saturating_add(1);
        if next_index >= self.queue.items.len() {
            self.transport_stop();
            return Ok(());
        }
        self.play_queue_index(next_index)
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        let Some(current_index) = self.queue.current_index else {
            return Ok(());
        };
        self.play_queue_index(current_index.saturating_sub(1))
    }

    // Playback preference commands.
    pub(crate) fn set_volume(&mut self, volume: u8) {
        self.player_state.volume = volume.min(100);
        self.audio_player.set_volume(self.player_state.volume);
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.player_state.muted = muted;
        self.audio_player.set_muted(muted);
    }

    pub(crate) fn set_shuffle(&mut self, enabled: bool) {
        if self.queue.shuffle_enabled == enabled {
            return;
        }
        let Some(current_index) = self.queue.current_index else {
            self.queue.shuffle_enabled = enabled;
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
        self.queue.shuffle_enabled = enabled;
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

    pub(crate) fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.queue.repeat_mode = mode;
    }

    pub(crate) fn set_autoplay(&mut self, is_enabled: bool) {
        self.autoplay.set_enabled(is_enabled);
        self.queue.autoplay_enabled = is_enabled;
    }

    // Platform state queries.
    pub(crate) fn queue(&self) -> &Queue {
        &self.queue
    }

    pub(crate) fn player_state(&self) -> PlayerState {
        let mut player_state = self.player_state.clone();
        player_state.active_engine_kind = PlaybackEngineKind::Sendspin;
        player_state.current_queue_item_id = self.current_queue_item_id().cloned();
        player_state.current_index = self.queue.current_index;
        player_state
    }

    pub(crate) fn queue_revision(&self) -> u64 {
        self.queue_revision
    }
}

fn download_status_symbol(status: CacheStatus) -> &'static str {
    match status {
        CacheStatus::Interrupted => "⏸",
        _ => "▶",
    }
}

impl PlaybackEngine for SendspinEngine {
    fn configure(&mut self, configuration: PlaybackEngineConfiguration) -> Result<(), String> {
        match configuration {
            PlaybackEngineConfiguration::Sendspin(configuration) => {
                SendspinEngine::configure(self, configuration)
            }
            unsupported_configuration => Err(format!(
                "Sendspin cached local engine does not support '{}' configuration",
                unsupported_configuration.kind().display_name()
            )),
        }
    }

    fn activate(&mut self) -> Result<(), String> {
        SendspinEngine::connect(self)
    }

    fn deactivate(&mut self) {
        SendspinEngine::disconnect(self);
    }

    fn shutdown(&mut self) {
        SendspinEngine::shutdown(self);
    }

    fn queue_replace(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        SendspinEngine::play_media(self, media_items)
    }

    fn queue_insert_next(
        &mut self,
        media_items: Vec<MediaItem>,
        replace_following: bool,
    ) -> Result<(), String> {
        SendspinEngine::play_media_next(self, media_items, replace_following)
    }

    fn queue_append(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        SendspinEngine::play_media_last(self, media_items)
    }

    fn queue_append_radio_tracks(
        &mut self,
        seed_tracks: Vec<Track>,
        radio_tracks: Vec<Track>,
    ) -> Result<(), String> {
        SendspinEngine::append_radio_tracks(self, seed_tracks, radio_tracks)
    }

    fn queue_clear(&mut self) -> Result<(), String> {
        SendspinEngine::clear_queue(self)
    }

    fn queue_move(&mut self, item_id: QueueItemId, shift: i32) -> Result<(), String> {
        SendspinEngine::move_queue_item(self, item_id, shift)
    }

    fn queue_remove(&mut self, item_id: QueueItemId) -> Result<(), String> {
        SendspinEngine::delete_queue_item(self, item_id)
    }

    fn queue_play(&mut self, index: usize) -> Result<(), String> {
        SendspinEngine::play_queue_index(self, index)
    }

    fn transport_play(&mut self) -> Result<(), String> {
        SendspinEngine::transport_play(self)
    }

    fn transport_pause(&mut self) {
        SendspinEngine::transport_pause(self);
    }

    fn transport_stop(&mut self) {
        SendspinEngine::transport_stop(self);
    }

    fn transport_next(&mut self) -> Result<(), String> {
        SendspinEngine::transport_next(self)
    }

    fn transport_previous(&mut self) -> Result<(), String> {
        SendspinEngine::transport_previous(self)
    }

    fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        SendspinEngine::transport_seek(self, seconds)
    }

    fn set_volume(&mut self, volume: u8) {
        SendspinEngine::set_volume(self, volume);
    }

    fn set_muted(&mut self, muted: bool) {
        SendspinEngine::set_muted(self, muted);
    }

    fn set_shuffle(&mut self, enabled: bool) {
        SendspinEngine::set_shuffle(self, enabled);
    }

    fn set_repeat_mode(&mut self, mode: RepeatMode) {
        SendspinEngine::set_repeat_mode(self, mode);
    }

    fn queue(&self) -> Queue {
        SendspinEngine::queue(self).clone()
    }

    fn player_state(&self) -> PlayerState {
        SendspinEngine::player_state(self)
    }

    fn preferred_codec_configuration(
        &mut self,
    ) -> Option<&mut dyn PreferredCodecConfigurablePlaybackEngine> {
        Some(self)
    }

    fn start_background_work(&mut self, engine: Arc<Mutex<Box<dyn PlaybackEngine>>>) {
        if self.background_worker.is_some() {
            return;
        }
        let engine = Arc::downgrade(&engine);
        self.background_worker = Some(PeriodicWorker::start(move || {
            let Some(engine) = Weak::upgrade(&engine) else {
                return;
            };
            let Ok(mut engine) = engine.try_lock() else {
                return;
            };
            engine.process_playback_cycle();
        }));
    }

    fn stop_background_work(&mut self) {
        if let Some(mut worker) = self.background_worker.take() {
            worker.stop();
        }
    }

    fn process_playback_cycle(&mut self) {
        let queue_before = self.queue().clone();
        let player_state_before = self.player_state();
        self.process_autoplay_results();
        self.process_connection_state();
        if let Some(content_key) = self.current_cache_content_key.clone() {
            self.process_download_window();
            self.process_cached_playback(&content_key);
        }
        self.request_autoplay_refill_if_needed();
        self.publish_background_events(queue_before, player_state_before);
    }

    fn set_autoplay(&mut self, is_enabled: bool) {
        SendspinEngine::set_autoplay(self, is_enabled);
    }
}

impl PreferredCodecConfigurablePlaybackEngine for SendspinEngine {
    fn set_preferred_codec(&mut self, codec: &str) -> Result<(), String> {
        SendspinEngine::set_preferred_codec(self, codec)
    }
}

fn tracks_from_media_items(media_items: &[MediaItem]) -> Vec<Track> {
    media_items
        .iter()
        .filter_map(|media_item| match media_item {
            MediaItem::Track(track) => Some(track.clone()),
            _ => None,
        })
        .collect()
}

fn tracks_from_queue(queue_items: &[QueueItem]) -> Vec<Track> {
    queue_items
        .iter()
        .filter_map(|item| item.media_item.as_ref())
        .filter_map(|media_item| match media_item {
            MediaItem::Track(track) => Some(track.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_assistant::models::identifiers::{MediaId, ProviderInstanceId};
    use crate::music_assistant::models::media::{MediaItemFields, Track};

    fn track(provider: &str, item_id: &str) -> MediaItem {
        MediaItem::Track(Track {
            media: MediaItemFields {
                item_id: MediaId(item_id.to_string()),
                provider: ProviderInstanceId(provider.to_string()),
                name: item_id.to_string(),
                version: String::new(),
                sort_name: None,
                uri: None,
                external_ids: Vec::new(),
                is_playable: true,
                provider_mappings: Vec::new(),
                metadata: None,
                favorite: false,
                position: None,
                date_added: None,
            },
            duration: 0,
            artists: Vec::new(),
            album: None,
            image: None,
            disc_number: 0,
            track_number: 0,
        })
    }

    fn configuration() -> SendspinEngineConfiguration {
        SendspinEngineConfiguration {
            sendspin_url: "ws://localhost".to_string(),
            music_assistant_url: "http://localhost".to_string(),
            music_assistant_auth_token: None,
            client_id: "test".to_string(),
            client_name: "Test".to_string(),
            downloader_buffer_bytes: 1024,
            cache_bytes: 1024,
            preferred_codec: SendspinAudioCodec::Pcm,
        }
    }

    #[test]
    fn implements_the_output_engine_contract() {
        let mut engine = SendspinEngine::new();

        let queue = PlaybackEngine::queue(&engine);
        let player_state = PlaybackEngine::player_state(&engine);

        assert_eq!(player_state.playback_state, PlayState::Uninitialized);
        assert!(queue.items.is_empty());
        assert!(engine.preferred_codec_configuration().is_some());
    }

    #[test]
    fn limits_reconnect_delay_to_eight_seconds() {
        let mut engine = SendspinEngine::new();

        engine.schedule_reconnect_attempt();
        assert_eq!(engine.reconnect_delay, Duration::from_secs(4));
        engine.schedule_reconnect_attempt();
        assert_eq!(engine.reconnect_delay, Duration::from_secs(8));
        engine.schedule_reconnect_attempt();
        assert_eq!(engine.reconnect_delay, Duration::from_secs(8));
    }

    #[test]
    fn rejects_another_output_engines_configuration() {
        let mut engine = SendspinEngine::new();

        let error = PlaybackEngine::configure(
            &mut engine,
            PlaybackEngineConfiguration::Offline(
                crate::playback::models::OfflinePlaybackConfiguration {
                    library_root: std::path::PathBuf::from("C:/Music"),
                },
            ),
        )
        .expect_err("Sendspin accepts only Sendspin configuration");

        assert_eq!(
            error,
            "Sendspin cached local engine does not support 'offline' configuration"
        );
    }

    #[test]
    fn queue_item_ids_stay_stable_when_inserting_media() {
        let mut engine = SendspinEngine::new();
        let initial_items =
            engine.new_queue_media_items(vec![track("test", "first"), track("test", "second")]);
        engine.queue.items = initial_items
            .iter()
            .map(|item| {
                QueueItem::local(
                    engine.queue.queue_id.clone(),
                    item.queue_item_id.clone(),
                    item.original_index,
                    item.media_item.clone(),
                )
            })
            .collect();

        let mut rebuilt_items = engine.queue_media_items();
        rebuilt_items.splice(
            1..1,
            engine.new_queue_media_items(vec![track("test", "inserted")]),
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
        engine.config = Some(configuration());
        let queue_items = engine.new_queue_media_items(vec![
            track("test", "first"),
            track("test", "second"),
            track("test", "third"),
        ]);

        let plan = engine
            .build_queue_download_plan(queue_items, 1)
            .expect("build queue plan");
        assert_eq!(plan.items[0].role, SendspinDownloadRole::Future);
        assert_eq!(plan.items[1].role, SendspinDownloadRole::Current);
        assert_eq!(plan.items[2].role, SendspinDownloadRole::Next);
    }

    #[test]
    fn queue_download_plan_uses_music_assistant_item_ids_for_cache_keys() {
        let mut engine = SendspinEngine::new();
        engine.config = Some(configuration());
        let queue_items =
            engine.new_queue_media_items(vec![track("deezer", "first"), track("deezer", "second")]);

        let plan = engine
            .build_queue_download_plan(queue_items, 0)
            .expect("build queue plan");
        assert_eq!(
            plan.items[0].cache_content_key.as_str(),
            "deezer:first:pcm:48000:16:2"
        );
        assert_eq!(
            plan.items[1].cache_content_key.as_str(),
            "deezer:second:pcm:48000:16:2"
        );
    }

    #[test]
    fn shuffling_upcoming_items_keeps_the_current_item_in_place() {
        let mut engine = SendspinEngine::new();
        let mut queue = engine.new_queue_media_items(vec![
            track("test", "played"),
            track("test", "current"),
            track("test", "third"),
            track("test", "fourth"),
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
            track("test", "played"),
            track("test", "current"),
            track("test", "third"),
            track("test", "fourth"),
            track("test", "fifth"),
        ]);
        queue[2..].reverse();

        queue[2..].sort_by_key(|item| item.original_index);

        assert_eq!(
            queue
                .iter()
                .map(|item| item.original_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn shuffling_replaces_the_downloader_plan_with_the_new_queue_order() {
        let mut engine = SendspinEngine::new();
        engine.config = Some(configuration());
        engine
            .play_media(vec![
                track("test", "first"),
                track("test", "second"),
                track("test", "third"),
                track("test", "fourth"),
            ])
            .expect("start playback");
        let original_upcoming_ids = engine.queue.items[1..]
            .iter()
            .map(|item| item.queue_item_id.clone())
            .collect::<Vec<_>>();

        engine.set_shuffle(true);

        let downloader_plan = engine.downloader.download_plan().expect("download plan");
        assert_eq!(engine.queue.shuffle_enabled, true);
        assert_eq!(downloader_plan.version, 2);
        assert_eq!(
            downloader_plan
                .items
                .iter()
                .map(|item| item.queue_item_id.clone())
                .collect::<Vec<_>>(),
            engine
                .queue
                .items
                .iter()
                .map(|item| item.queue_item_id.clone())
                .collect::<Vec<_>>(),
        );
        assert_ne!(
            engine.queue.items[1..]
                .iter()
                .map(|item| item.queue_item_id.clone())
                .collect::<Vec<_>>(),
            original_upcoming_ids,
        );
        assert_eq!(downloader_plan.items[0].role, SendspinDownloadRole::Current);
        assert_eq!(downloader_plan.items[1].role, SendspinDownloadRole::Next);
    }
}
