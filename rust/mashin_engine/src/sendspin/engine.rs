use super::downloader::{DownloaderConnectionStatus, SendspinDownloader, SendspinDownloaderConfig};
use super::models::{
    SendspinAudioCodec, SendspinDownloadPlanRequest, SendspinDownloadQueueItem,
    SendspinEngineConfiguration,
};
use super::queue_store::{queue_snapshot_path, QueueSnapshot, QueueStore, SNAPSHOT_VERSION};
use crate::audio::player::{AudioPlayer, AudioPlayerState};
use crate::audio::source::{AudioSource, SourceReadResult};
use crate::logging::Logger;
use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::identifiers::{CacheContentKey, QueueItemId};
use crate::music_assistant::models::media::{MediaItem, Track};
use crate::music_assistant::models::queue::{
    CacheState, CacheStatus, PlayState, QueueItem, QueueOption, RepeatMode,
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
    queue_store: Option<QueueStore>,
    is_queue_restored: bool,
    current_cache_content_key: Option<String>,
    queue_content_keys: Vec<String>,
    last_download_status_log: Option<Instant>,
    next_reconnect_attempt: Option<Instant>,
    reconnect_delay: Duration,
    last_confirmed_download_progress: u64,
    is_reconnect_in_flight: bool,
    last_connection_event_generation: Option<u64>,
    lost_connection_generation: Option<u64>,
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
            queue_store: None,
            is_queue_restored: false,
            current_cache_content_key: None,
            queue_content_keys: Vec::new(),
            last_download_status_log: None,
            next_reconnect_attempt: None,
            reconnect_delay: INITIAL_RECONNECT_DELAY,
            last_confirmed_download_progress: 0,
            is_reconnect_in_flight: false,
            last_connection_event_generation: None,
            lost_connection_generation: None,
            playback_position_offset_millis: 0,
            autoplay: AutoplayController::new(),
            autoplay_result_sender,
            autoplay_result_receiver,
            background_worker: None,
            last_player_state_event_at: None,
        }
    }

    pub(crate) fn init(&mut self, config: SendspinEngineConfiguration) {
        if let Some(store) = self.queue_store.take() {
            store.finish();
        }
        let downloader_config = Self::downloader_config(&config);
        self.config = Some(config);
        self.queue = Queue::default();
        self.player_state = PlayerState::default();
        self.download_plan_version = 0;
        self.next_queue_item_id = 1;
        self.next_original_index = 0;
        self.is_queue_restored = false;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.last_download_status_log = None;
        self.next_reconnect_attempt = None;
        self.reconnect_delay = INITIAL_RECONNECT_DELAY;
        self.last_confirmed_download_progress = 0;
        self.is_reconnect_in_flight = false;
        self.last_connection_event_generation = None;
        self.lost_connection_generation = None;
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
        match queue_snapshot_path().and_then(QueueStore::start) {
            Ok((store, snapshot)) => {
                if let Some(snapshot) = snapshot {
                    self.restore_queue(snapshot);
                }
                self.queue_store = Some(store);
            }
            Err(error) => LOGGER.error(format!("queue snapshot unavailable: {error}")),
        }
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
        if let Some(current_index) = current_index.filter(|_| !self.is_queue_restored) {
            let plan = self.build_download_plan_request(queue_items, current_index)?;
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
        if let Some(store) = self.queue_store.take() {
            store.finish();
        }
        *self = Self::new();
    }

    // Queue commands.
    // Applies the queue snapshot after the downloader accepts the corresponding download plan.
    pub(crate) fn apply_queue_download_plan(
        &mut self,
        request: SendspinDownloadPlanRequest,
    ) -> Result<(), String> {
        self.downloader.apply_download_plan(request.clone())?;
        self.download_plan_version = request.queue_revision;
        self.queue.items = request
            .queue_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut queue_item = QueueItem::local(
                    self.queue.queue_id.clone(),
                    item.queue_item_id.clone(),
                    item.original_index,
                    item.media_item.clone(),
                );
                queue_item.index = index as u32;
                queue_item
            })
            .collect();
        self.queue.current_index = request.current_index;
        self.current_cache_content_key = request.current_index.and_then(|index| {
            request.queue_items.get(index).and_then(|item| {
                self.config.as_ref().and_then(|config| {
                    queue_cache_content_key(&item.media_item, config.preferred_codec).ok()
                })
            })
        });
        self.queue_content_keys = request
            .queue_items
            .iter()
            .filter_map(|item| {
                self.config.as_ref().and_then(|config| {
                    queue_cache_content_key(&item.media_item, config.preferred_codec).ok()
                })
            })
            .collect();
        LOGGER.debug(format!(
            "queue download plan applied: version {}, {} items, current index {:?}",
            request.queue_revision,
            request.queue_items.len(),
            self.queue.current_index
        ));
        self.queue_revision = self.queue_revision.saturating_add(1);
        self.refresh_queue_cache_states();
        self.is_queue_restored = false;
        self.persist_queue();
        Ok(())
    }

    pub(crate) fn queue_play_media(
        &mut self,
        mut media_items: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
        start_download: bool,
    ) -> Result<(), String> {
        if media_items.is_empty() {
            return if option == QueueOption::Replace {
                self.clear_queue()
            } else {
                Err("queue_play_media requires at least one media item".to_string())
            };
        }
        if option == QueueOption::Play && !start_download {
            return Err("play requires starting the download".to_string());
        }
        if matches!(option, QueueOption::Next | QueueOption::ReplaceNext | QueueOption::Add)
            && start_item.is_some()
        {
            return Err("start item is only valid for play or replace".to_string());
        }
        if matches!(option, QueueOption::Next | QueueOption::ReplaceNext | QueueOption::Add)
            && !start_download
            && !self.is_queue_restored
            && !self.queue.items.is_empty()
        {
            return Err("cannot defer a queue edit while downloading".to_string());
        }
        if shuffle && !self.queue.shuffle_enabled {
            Self::shuffle_items(&mut media_items);
        }
        let seed_tracks = tracks_from_media_items(&media_items);
        match option {
            QueueOption::Play => {
                let index = start_item.unwrap_or(0);
                if index >= media_items.len() {
                    return Err("queue start item is out of range".to_string());
                }
                self.next_original_index = 0;
                let new_items = self.new_queue_media_items(media_items);
                let plan = self.build_download_plan_request(new_items, index)?;
                self.apply_queue_download_plan(plan)?;
                let content_key = self
                    .current_cache_content_key
                    .clone()
                    .ok_or("queue_play_media plan has no current item")?;
                self.play_cached_content(&content_key)?;
                self.autoplay.replace_seed_tracks(seed_tracks);
            }
            QueueOption::Replace => {
                let index = start_item.unwrap_or(0);
                if index >= media_items.len() {
                    return Err("queue start index is out of range".to_string());
                }
                if !start_download {
                    self.stop_queue_downloads()?;
                }
                self.next_original_index = 0;
                let items = self.new_queue_media_items(media_items);
                self.audio_player.stop();
                if start_download {
                    self.apply_queue_download_plan(self.build_download_plan_request(items, index)?)?;
                } else {
                    self.queue.items = items.into_iter().enumerate().map(|(position, item)| {
                        let mut queue_item = QueueItem::local(
                            self.queue.queue_id.clone(), item.queue_item_id, item.original_index, item.media_item,
                        );
                        queue_item.index = position as u32;
                        queue_item.cache = None;
                        queue_item
                    }).collect();
                    self.queue.current_index = Some(index);
                    self.current_cache_content_key = None;
                    self.queue_content_keys.clear();
                    self.is_queue_restored = true;
                    self.queue_revision = self.queue_revision.saturating_add(1);
                    self.persist_queue();
                }
                self.player_state.playback_state = PlayState::Idle;
                self.player_state.position_millis = Some(0);
                self.playback_position_offset_millis = 0;
                self.autoplay.replace_seed_tracks(tracks_from_queue(&self.queue.items));
            }
            QueueOption::Next | QueueOption::ReplaceNext | QueueOption::Add => {
                if option == QueueOption::Add {
                    self.append_queue_items(media_items)?;
                } else if option == QueueOption::Next {
                    let insertion = self.queue.current_index.map(|index| index.saturating_add(1))
                        .unwrap_or(self.queue.items.len()).min(self.queue.items.len());
                    self.insert_queue_items(insertion, media_items)?;
                } else {
                    let new_items = self.new_queue_media_items(media_items);
                    let mut queue = self.queue_media_items();
                    let was_empty = queue.is_empty();
                    let insertion = self.queue.current_index.map(|index| index.saturating_add(1))
                        .unwrap_or(queue.len()).min(queue.len());
                    queue.truncate(insertion);
                    queue.extend(new_items);
                    if was_empty {
                        self.is_queue_restored = true;
                    }
                    self.replace_queue_media_items(queue)?;
                }
                if start_download && self.is_queue_restored {
                    let index = self.queue.current_index.unwrap_or(0);
                    self.apply_queue_download_plan(
                        self.build_download_plan_request(self.queue_media_items(), index)?,
                    )?;
                }
                if option == QueueOption::ReplaceNext {
                    self.autoplay.replace_seed_tracks(seed_tracks);
                } else {
                    self.autoplay.append_seed_tracks(seed_tracks);
                }
            }
            QueueOption::Unknown => return Err("unknown queue option".to_string()),
        }
        Ok(())
    }

    // Autoplay must not fill the new queue before radio recommendations arrive.
    pub(crate) fn start_radio(&mut self, started_tracks: Vec<MediaItem>) -> Result<(), String> {
        self.queue_play_media(started_tracks, QueueOption::Play, None, false, true)?;
        self.autoplay.replace_seed_tracks(Vec::new());
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
        self.stop_queue_downloads()?;
        self.audio_player.stop();
        self.queue.items.clear();
        self.queue.current_index = None;
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.next_original_index = 0;
        self.autoplay = AutoplayController::new();
        self.queue_revision = self.queue_revision.saturating_add(1);
        self.player_state.playback_state = PlayState::Idle;
        self.is_queue_restored = false;
        self.persist_queue();
        Ok(())
    }

    pub(crate) fn insert_queue_items(
        &mut self,
        index: usize,
        media_items: Vec<MediaItem>,
    ) -> Result<(), String> {
        if index > self.queue.items.len() {
            return Err("queue insertion index is out of range".to_string());
        }
        if media_items.iter().any(|item| !matches!(item, MediaItem::Track(_))) {
            return Err("queue items must be tracks".to_string());
        }
        if media_items.is_empty() {
            return Ok(());
        }
        let was_empty = self.queue.items.is_empty();
        let new_items = self.new_queue_media_items(media_items);
        let mut queue = self.queue_media_items();
        queue.splice(index..index, new_items);
        if was_empty {
            self.is_queue_restored = true;
        }
        self.replace_queue_media_items(queue)?;
        if was_empty {
            self.player_state.playback_state = PlayState::Idle;
            self.player_state.position_millis = Some(0);
        }
        Ok(())
    }

    pub(crate) fn append_queue_items(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        self.insert_queue_items(self.queue.items.len(), media_items)
    }

    // Moves existing entries in queue order; the target index refers to the queue after removal.
    pub(crate) fn move_queue_items(
        &mut self,
        item_ids: Vec<QueueItemId>,
        target_index: usize,
    ) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        if item_ids.is_empty() {
            return Ok(());
        }
        if item_ids.iter().enumerate().any(|(index, id)| {
            item_ids[..index].contains(id) || !queue.iter().any(|item| &item.queue_item_id == id)
        }) {
            return Err("queue items must be distinct existing entries".to_string());
        }
        if target_index > queue.len() - item_ids.len() {
            return Err("queue move target is out of range".to_string());
        }
        let mut moved = Vec::with_capacity(item_ids.len());
        queue.retain(|item| {
            if item_ids.contains(&item.queue_item_id) {
                moved.push(item.clone());
                false
            } else {
                true
            }
        });
        queue.splice(target_index..target_index, moved);
        self.replace_queue_media_items(queue)
    }

    pub(crate) fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String> {
        let mut queue = self.queue_media_items();
        if item_ids.is_empty() {
            return Ok(());
        }
        if item_ids.iter().enumerate().any(|(index, id)| {
            item_ids[..index].contains(id) || !queue.iter().any(|item| &item.queue_item_id == id)
        }) {
            return Err("queue items must be distinct existing entries".to_string());
        }
        queue.retain(|item| !item_ids.contains(&item.queue_item_id));
        if queue.is_empty() {
            return self.clear_queue();
        }
        self.replace_queue_media_items(queue)
    }

    // Selection leaves the queue idle until a transport command starts playback.
    pub(crate) fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        let index = self
            .queue
            .items
            .iter()
            .position(|item| item.queue_item_id == item_id)
            .ok_or("queue item is no longer available")?;
        self.stop_queue_downloads()?;
        self.audio_player.stop();
        self.queue.current_index = Some(index);
        for item in &mut self.queue.items {
            item.cache = None;
        }
        self.current_cache_content_key = None;
        self.queue_content_keys.clear();
        self.is_queue_restored = true;
        self.player_state.playback_state = PlayState::Idle;
        self.player_state.position_millis = Some(0);
        self.playback_position_offset_millis = 0;
        self.queue_revision = self.queue_revision.saturating_add(1);
        self.persist_queue();
        Ok(())
    }

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        if self.is_queue_restored {
            if index >= self.queue.items.len() {
                return Err("queue index is out of range".to_string());
            }
            let start_position_millis = self.player_state.position_millis.unwrap_or(0);
            self.apply_queue_download_plan(
                self.build_download_plan_request(self.queue_media_items(), index)?,
            )?;
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or("queue has no current item")?;
            self.play_cached_content(&content_key)?;
            self.playback_position_offset_millis = start_position_millis;
            self.player_state.position_millis = Some(start_position_millis);
            return Ok(());
        }
        let Some(content_key) = self.queue_content_keys.get(index) else {
            return Err("queue index is out of range".to_string());
        };
        let content_key = content_key.clone();
        self.queue.current_index = Some(index);
        self.play_cached_content(&content_key)?;
        self.apply_queue_download_plan(
            self.build_download_plan_request(self.queue_media_items(), index)?,
        )
    }

    // Queue planning helpers.
    // Invalidate the downloader's old plan before leaving an active queue idle.
    fn stop_queue_downloads(&mut self) -> Result<(), String> {
        if self.current_cache_content_key.is_none() {
            return Ok(());
        }
        let version = self.download_plan_version.saturating_add(1);
        self.downloader.apply_download_plan(SendspinDownloadPlanRequest {
            queue_revision: version,
            queue_items: Vec::new(),
            current_index: None,
        })?;
        self.download_plan_version = version;
        Ok(())
    }

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
        let mut queue_media_items = media_items
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
            .collect::<Vec<_>>();
        if self.queue.shuffle_enabled {
            Self::shuffle_items(&mut queue_media_items);
        }
        queue_media_items
    }

    // Preserves the current item when possible, then rebuilds the authoritative queue snapshot.
    fn replace_queue_media_items(
        &mut self,
        mut media_items: Vec<QueueMediaItem>,
    ) -> Result<(), String> {
        if media_items.is_empty() {
            return self.clear_queue();
        }
        if !self.queue.shuffle_enabled {
            for (index, item) in media_items.iter_mut().enumerate() {
                item.original_index = index;
            }
            self.next_original_index = media_items.len();
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
        if self.is_queue_restored {
            self.queue.items = media_items
                .into_iter()
                .enumerate()
                .map(|(index, item)| {
                    let mut queue_item = QueueItem::local(
                        self.queue.queue_id.clone(),
                        item.queue_item_id,
                        item.original_index,
                        item.media_item,
                    );
                    queue_item.index = index as u32;
                    queue_item.cache = None;
                    queue_item
                })
                .collect();
            self.queue.current_index = Some(current_index);
            if previous_current_queue_item_id.as_ref() != self.current_queue_item_id() {
                self.player_state.position_millis = Some(0);
                self.playback_position_offset_millis = 0;
            }
            self.persist_queue();
            return Ok(());
        }
        let plan = self.build_download_plan_request(media_items, current_index)?;
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

    fn build_download_plan_request(
        &self,
        media_items: Vec<QueueMediaItem>,
        current_index: usize,
    ) -> Result<SendspinDownloadPlanRequest, String> {
        Ok(SendspinDownloadPlanRequest {
            queue_revision: self.download_plan_version.saturating_add(1),
            queue_items: media_items
                .into_iter()
                .map(|item| SendspinDownloadQueueItem {
                    queue_item_id: item.queue_item_id,
                    original_index: item.original_index,
                    media_item: item.media_item,
                })
                .collect(),
            current_index: Some(current_index),
        })
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
        let (active_downloads, cache_used_bytes, cache_bytes, capacity_mode) =
            self.downloader.download_status();
        let (completed_cache_items, partly_cached_items) =
            self.downloader.cache_item_counts(&self.queue_content_keys);
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
                    "{track_name} {}/{} ms ({:.1}%)",
                    download.cached_duration_millis,
                    expected_duration_millis,
                    Self::percentage(download.cached_duration_millis, expected_duration_millis,)
                )
            })
            .collect::<Vec<_>>();
        let cache_usage_percent = Self::percentage(cache_used_bytes as u64, cache_bytes as u64);
        LOGGER.info(format!(
            "download status: mode {capacity_mode}, cached items: {completed_cache_items} completed, {partly_cached_items} partly, [{}], cache {} KiB/{} KiB ({:.1}%)",
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

    // Consumes downloader transport events and schedules a generation-safe reconnect when needed.
    fn process_connection_state(&mut self) {
        if let Some(event) = self.downloader.take_connection_event() {
            if self
                .last_connection_event_generation
                .map(|generation| event.generation < generation)
                .unwrap_or(false)
            {
                return;
            }
            match event.status {
                DownloaderConnectionStatus::Lost => {
                    self.last_connection_event_generation = Some(event.generation);
                    self.lost_connection_generation = Some(event.generation);
                    LOGGER.trace(format!(
                        "Sendspin downloader connection lost: generation {}",
                        event.generation
                    ));
                    if self.player_state.connection_state != PlaybackConnectionState::Reconnecting {
                        self.player_state.connection_state = PlaybackConnectionState::Reconnecting;
                        self.schedule_reconnect_attempt();
                    } else if self.is_reconnect_in_flight {
                        self.is_reconnect_in_flight = false;
                        self.schedule_reconnect_attempt();
                    }
                }
                DownloaderConnectionStatus::Ready => {
                    if self.lost_connection_generation == Some(event.generation) {
                        return;
                    }
                    self.last_connection_event_generation = Some(event.generation);
                    LOGGER.trace(format!(
                        "Sendspin downloaders reconnected: generation {}",
                        event.generation
                    ));
                    self.player_state.connection_state = PlaybackConnectionState::Online;
                    self.next_reconnect_attempt = None;
                    self.last_confirmed_download_progress =
                        self.downloader.confirmed_download_progress();
                    self.is_reconnect_in_flight = false;
                    self.resume_downloads_if_active();
                }
            }
        }
        self.reset_reconnect_backoff_after_download_progress(
            self.downloader.confirmed_download_progress(),
        );
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

    // A ready transport alone does not prove that a repeatedly stalled download recovered.
    fn reset_reconnect_backoff_after_download_progress(&mut self, progress: u64) {
        if self.player_state.connection_state == PlaybackConnectionState::Online
            && progress > self.last_confirmed_download_progress
        {
            self.last_confirmed_download_progress = progress;
            self.reconnect_delay = INITIAL_RECONNECT_DELAY;
        }
    }

    // A restored queue is visible before playback but must not create a download plan on connect.
    fn resume_downloads_if_active(&mut self) {
        if self.is_queue_restored || self.current_cache_content_key.is_none() {
            return;
        }
        let request = self.build_download_plan_request(
            self.queue_media_items(),
            self.queue.current_index.unwrap_or(0),
        );
        if let Err(error) = request.and_then(|request| self.apply_queue_download_plan(request)) {
            LOGGER.error(format!("failed to resume Sendspin downloads: {error}"));
        }
    }

    fn process_download_status(&mut self) {
        if self.downloader.take_download_status_refresh() {
            self.refresh_queue_cache_states();
        }
        self.refresh_and_log_download_status();
    }

    // Advances local cached playback, updates player state, and selects the next queue item on end.
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
            crate::core::event_channel::publish_background_event(
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
            crate::core::event_channel::publish_background_event(
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
        let position_millis = self
            .playback_position_offset_millis
            .saturating_add(self.audio_player.playback_position_millis())
            .max(0) as u64;
        if let Err(error) = self
            .downloader
            .record_playback_position(content_key, position_millis)
        {
            LOGGER.error(format!(
                "failed to update Sendspin playback progress: {error}"
            ));
        }
    }

    fn start_prepared_audio_if_available(&mut self, content_key: &str) {
        if self.audio_player.is_started() || !self.audio_player.has_prepared_source() {
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
        if self.is_queue_restored {
            let index = self
                .queue
                .current_index
                .ok_or("cannot play without an active track")?;
            return self.play_queue_index(index);
        }
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
        if self.is_queue_restored {
            let index = self
                .queue
                .current_index
                .ok_or("cannot seek without an active track")?;
            // The restored downloader has no slot; start its first plan at the seek position.
            self.downloader.set_pending_current_seek(position / 1_000)?;
            self.apply_queue_download_plan(
                self.build_download_plan_request(self.queue_media_items(), index)?,
            )?;
            let content_key = self
                .current_cache_content_key
                .clone()
                .ok_or("queue has no current item")?;
            self.play_cached_content(&content_key)?;
            self.playback_position_offset_millis = position;
            self.player_state.position_millis = Some(position);
            return Ok(());
        }
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
            if self.queue.repeat_mode == RepeatMode::All {
                return self.play_queue_index(0);
            }
            self.transport_stop();
            return Ok(());
        }
        self.play_queue_index(next_index)
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        let Some(current_index) = self.queue.current_index else {
            return Ok(());
        };
        if current_index == 0 && self.queue.repeat_mode == RepeatMode::All {
            return self.play_queue_index(self.queue.items.len() - 1);
        }
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
            self.persist_queue();
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
        self.queue.shuffle_enabled = enabled;
        if let Err(error) = self.replace_queue_media_items(queue) {
            self.queue.shuffle_enabled = !enabled;
            LOGGER.error(format!("failed to update queue for shuffle: {error}"));
            return;
        }
        self.persist_queue();
    }

    fn shuffle_items<T>(items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        let mut original_positions = (0..items.len()).collect::<Vec<_>>();
        let mut state = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0)
            ^ items.len() as u64;
        for index in (1..items.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let target = (state as usize) % (index + 1);
            items.swap(index, target);
            original_positions.swap(index, target);
        }
        if original_positions.iter().enumerate().all(|(index, position)| index == *position) {
            items.swap(0, 1);
        }
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: RepeatMode) {
        if self.queue.repeat_mode == mode {
            return;
        }
        self.queue.repeat_mode = mode;
        self.persist_queue();
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

    // Queue snapshot mapping stays separate from playback and downloader commands.
    fn restore_queue(&mut self, snapshot: QueueSnapshot) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        if snapshot.music_assistant_url != config.music_assistant_url
            || snapshot.username != config.username
        {
            return;
        }
        let mut items = snapshot.items;
        if items.iter().any(|item| item.media_item.is_none()) {
            LOGGER.error("queue snapshot contains an item without media");
            return;
        }
        // Preserve the original order of a shuffled snapshot while rebasing old position numbers.
        if snapshot.shuffle_enabled {
            let mut original_order = (0..items.len()).collect::<Vec<_>>();
            original_order.sort_by_key(|&index| items[index].sort_index);
            for (original_index, queue_index) in original_order.into_iter().enumerate() {
                items[queue_index].sort_index = original_index as u32;
            }
        } else {
            for (index, item) in items.iter_mut().enumerate() {
                item.sort_index = index as u32;
            }
        }
        for (index, item) in items.iter_mut().enumerate() {
            item.index = index as u32;
            item.cache = None;
        }
        self.queue.current_index = snapshot
            .current_index
            .filter(|index| *index < items.len())
            .or_else(|| (!items.is_empty()).then_some(0));
        self.next_original_index = items.len();
        self.next_queue_item_id = items
            .iter()
            .filter_map(|item| item.queue_item_id.as_str().strip_prefix("local-"))
            .filter_map(|id| id.parse::<u64>().ok())
            .max()
            .map_or(1, |id| id.saturating_add(1));
        self.queue.items = items;
        self.queue.shuffle_enabled = snapshot.shuffle_enabled;
        self.queue.repeat_mode = snapshot.repeat_mode;
        self.is_queue_restored = !self.queue.items.is_empty();
    }

    fn persist_queue(&self) {
        let (Some(store), Some(config)) = (&self.queue_store, &self.config) else {
            return;
        };
        let mut items = self.queue.items.clone();
        for item in &mut items {
            item.cache = None;
        }
        store.save(QueueSnapshot {
            version: SNAPSHOT_VERSION,
            music_assistant_url: config.music_assistant_url.clone(),
            username: config.username.clone(),
            current_index: self.queue.current_index,
            items,
            shuffle_enabled: self.queue.shuffle_enabled,
            repeat_mode: self.queue.repeat_mode,
        });
    }
}

fn queue_cache_content_key(
    media_item: &MediaItem,
    codec: SendspinAudioCodec,
) -> Result<String, String> {
    let fields = media_item.fields();
    let provider = fields.provider.as_str().trim();
    let item_id = fields.item_id.as_str().trim();
    if provider.is_empty() || item_id.is_empty() {
        return Err("queue item has no Music Assistant provider or item ID".to_string());
    }
    Ok(format!(
        "{provider}:{item_id}:{}:48000:16:2",
        codec.as_str()
    ))
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

    fn queue_play_media(
        &mut self,
        media_items: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
        start_download: bool,
    ) -> Result<(), String> {
        SendspinEngine::queue_play_media(self, media_items, option, start_item, shuffle, start_download)
    }

    fn queue_start_radio(&mut self, started_tracks: Vec<MediaItem>) -> Result<(), String> {
        SendspinEngine::start_radio(self, started_tracks)
    }

    fn queue_append_radio_tracks(
        &mut self,
        seed_tracks: Vec<Track>,
        radio_tracks: Vec<Track>,
    ) -> Result<(), String> {
        SendspinEngine::append_radio_tracks(self, seed_tracks, radio_tracks)
    }

    fn insert_queue_items(&mut self, index: usize, media_items: Vec<MediaItem>) -> Result<(), String> {
        SendspinEngine::insert_queue_items(self, index, media_items)
    }

    fn append_queue_items(&mut self, media_items: Vec<MediaItem>) -> Result<(), String> {
        SendspinEngine::append_queue_items(self, media_items)
    }

    fn move_queue_items(&mut self, item_ids: Vec<QueueItemId>, target_index: usize) -> Result<(), String> {
        SendspinEngine::move_queue_items(self, item_ids, target_index)
    }

    fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String> {
        SendspinEngine::delete_queue_items(self, item_ids)
    }

    fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        SendspinEngine::select_queue_item(self, item_id)
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
            self.process_download_status();
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

    #[test]
    fn queue_item_operations_preserve_ids_and_selected_item() {
        let mut engine = SendspinEngine::new();
        engine
            .queue_play_media(vec![track("test", "first"), track("test", "second")], QueueOption::Replace, Some(1), false, false)
            .expect("set queue");
        assert_eq!(engine.queue.current_index, Some(1));
        assert_eq!(engine.player_state().playback_state, PlayState::Idle);
        assert!(engine.is_queue_restored);
        assert_eq!(engine.download_plan_version, 0);
        let first_id = engine.queue.items[0].queue_item_id.clone();
        let second_id = engine.queue.items[1].queue_item_id.clone();
        engine
            .insert_queue_items(1, vec![track("test", "third"), track("test", "fourth")])
            .expect("insert before current item");
        assert_eq!(engine.queue.items[0].queue_item_id, first_id);
        assert_eq!(engine.queue.items[3].queue_item_id, second_id);
        assert_eq!(engine.queue.current_index, Some(3));
        let third_id = engine.queue.items[1].queue_item_id.clone();
        let fourth_id = engine.queue.items[2].queue_item_id.clone();
        engine.move_queue_items(vec![third_id.clone(), fourth_id.clone()], 2).expect("move both items");
        assert_eq!(engine.queue.items[1].queue_item_id, second_id);
        assert_eq!(engine.queue.items[2].queue_item_id, third_id);
        assert_eq!(engine.queue.items[3].queue_item_id, fourth_id);
        assert_eq!(engine.queue.current_index, Some(1));
        assert!(engine
            .move_queue_items(vec![first_id.clone(), first_id.clone()], 0)
            .is_err());
        assert!(engine.move_queue_items(vec![first_id.clone()], 4).is_err());
        let next_queue_item_id = engine.next_queue_item_id;
        assert!(engine.insert_queue_items(5, vec![track("test", "unused")]).is_err());
        assert_eq!(engine.next_queue_item_id, next_queue_item_id);
        assert!(engine.delete_queue_items(vec![first_id.clone(), QueueItemId("missing".into())]).is_err());
        engine
            .delete_queue_items(vec![first_id, second_id])
            .expect("delete multiple entries");
        assert_eq!(engine.queue.current_index, Some(1));
        assert_eq!(engine.queue.items.len(), 2);
        assert_eq!(engine.download_plan_version, 0);
    }

    #[test]
    fn queue_positions_start_at_zero_after_replacement_and_edits() {
        let mut engine = SendspinEngine::new();
        engine.queue_play_media(
            vec![track("test", "old-first"), track("test", "old-second")],
            QueueOption::Replace, None, false, false,
        ).expect("initial queue");
        engine.queue_play_media(
            vec![track("test", "first"), track("test", "second"), track("test", "third")],
            QueueOption::Replace, None, false, false,
        ).expect("replace queue");
        assert_eq!(engine.queue.items[0].sort_index, 0);
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1, 2]);

        engine.insert_queue_items(1, vec![track("test", "inserted")]).expect("insert item");
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        let moved_id = engine.queue.items[3].queue_item_id.clone();
        engine.move_queue_items(vec![moved_id], 0).expect("move item");
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        let deleted_id = engine.queue.items[1].queue_item_id.clone();
        engine.delete_queue_items(vec![deleted_id]).expect("delete item");
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn selecting_queue_item_defers_playback_and_preserves_queue_ids() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine
            .queue_play_media(vec![track("test", "first"), track("test", "second")], QueueOption::Play, None, false, true)
            .expect("start playback");
        let ids = engine.queue.items.iter().map(|item| item.queue_item_id.clone()).collect::<Vec<_>>();

        engine.select_queue_item(ids[1].clone()).expect("select second item");

        assert_eq!(engine.queue.current_index, Some(1));
        assert_eq!(engine.queue.items.iter().map(|item| item.queue_item_id.clone()).collect::<Vec<_>>(), ids);
        assert_eq!(engine.player_state().playback_state, PlayState::Idle);
        assert_eq!(engine.player_state().position_millis, Some(0));
        assert!(engine.is_queue_restored);
        assert!(engine.current_cache_content_key.is_none());
        assert_eq!(engine.select_queue_item(QueueItemId("missing".into())).unwrap_err(), "queue item is no longer available");
        engine.transport_play().expect("play selected item");
        assert_eq!(engine.queue.current_index, Some(1));
        assert!(!engine.is_queue_restored);
        assert!(engine.current_cache_content_key.is_some());
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn queue_play_media_can_clear_queue() {
        let mut engine = SendspinEngine::new();
        engine
            .queue_play_media(vec![track("test", "first")], QueueOption::Replace, None, false, false)
            .expect("set queue");
        engine
            .queue_play_media(Vec::new(), QueueOption::Replace, None, false, false)
            .expect("clear queue with play media");
        assert!(engine.queue.items.is_empty());
    }

    #[test]
    fn inserting_into_empty_queue_defers_playback_and_append_preserves_ids() {
        let mut engine = SendspinEngine::new();
        let album: MediaItem = serde_json::from_value(serde_json::json!({
            "media_type": "album", "item_id": "album", "provider": "test", "name": "Album"
        })).expect("album media item");
        assert_eq!(engine.insert_queue_items(0, vec![track("test", "unused"), album]).unwrap_err(), "queue items must be tracks");
        assert!(engine.queue.items.is_empty());
        assert_eq!(engine.next_queue_item_id, 1);
        engine.insert_queue_items(0, vec![track("test", "first")]).expect("insert first item");
        let first_id = engine.queue.items[0].queue_item_id.clone();
        assert_eq!(engine.player_state().playback_state, PlayState::Idle);
        assert!(engine.is_queue_restored);
        assert_eq!(engine.download_plan_version, 0);
        engine.append_queue_items(vec![track("test", "second"), track("test", "third")]).expect("append items");
        assert_eq!(engine.queue.items[0].queue_item_id, first_id);
        assert_eq!(engine.queue.items.len(), 3);
        assert_eq!(engine.queue.current_index, Some(0));
    }

    #[test]
    fn active_queue_edits_update_the_download_plan() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine
            .queue_play_media(vec![track("test", "first")], QueueOption::Replace, None, false, true)
            .expect("start queue download");
        assert_eq!(engine.queue.items[0].index, 0);
        let first_id = engine.queue.items[0].queue_item_id.clone();
        engine
            .append_queue_items(vec![track("test", "second")])
            .expect("refresh download plan");
        assert_eq!(engine.queue.items.len(), 2);
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(engine.queue.items[0].queue_item_id, first_id);
        assert_eq!(engine.download_plan_version, 2);
        let second_id = engine.queue.items[1].queue_item_id.clone();
        engine.move_queue_items(vec![second_id.clone()], 0).expect("move active item");
        assert_eq!(engine.queue.current_index, Some(1));
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(engine.download_plan_version, 3);
        engine.delete_queue_items(vec![second_id]).expect("delete active item");
        assert_eq!(engine.queue.items[0].queue_item_id, first_id);
        assert_eq!(engine.queue.items[0].index, 0);
        assert_eq!(engine.download_plan_version, 4);
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn replace_queue_starts_download_without_playback() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine
            .queue_play_media(
                vec![track("test", "first"), track("test", "second")],
                QueueOption::Replace,
                Some(1),
                false,
                true,
            )
            .expect("replace queue");

        assert_eq!(engine.queue.current_index, Some(1));
        assert_eq!(engine.player_state().playback_state, PlayState::Idle);
        assert!(!engine.is_queue_restored);
        assert_eq!(engine.download_plan_version, 1);
        assert!(engine.current_cache_content_key.is_some());
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn replace_queue_can_defer_download_until_media_is_added() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine
            .queue_play_media(
                vec![track("test", "first"), track("test", "second")],
                QueueOption::Replace,
                Some(1),
                false,
                false,
            )
            .expect("defer queue download");

        assert!(engine.is_queue_restored);
        assert_eq!(engine.download_plan_version, 0);
        assert_eq!(engine.queue.current_index, Some(1));
        engine
            .queue_play_media(vec![track("test", "third")], QueueOption::Add, None, false, true)
            .expect("start download for extended queue");
        assert!(!engine.is_queue_restored);
        assert_eq!(engine.download_plan_version, 1);
        assert_eq!(engine.queue.items.len(), 3);
        assert_eq!(engine.player_state().playback_state, PlayState::Idle);
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn deferring_or_clearing_an_active_queue_invalidates_its_download_plan() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.queue_play_media(vec![track("test", "first")], QueueOption::Replace, None, false, true)
            .expect("start download");
        engine.queue_play_media(vec![track("test", "second")], QueueOption::Replace, None, false, false)
            .expect("defer replacement");
        assert_eq!(engine.download_plan_version, 2);
        assert!(engine.is_queue_restored);
        assert!(engine.current_cache_content_key.is_none());
        engine.transport_play().expect("start replacement");
        engine.clear_queue().expect("clear active queue");
        assert_eq!(engine.download_plan_version, 4);
        assert!(engine.queue.items.is_empty());
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn active_queue_cannot_skip_download_when_adding_media() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine
            .queue_play_media(vec![track("test", "first")], QueueOption::Replace, None, false, true)
            .expect("start queue download");
        let next_queue_item_id = engine.next_queue_item_id;
        assert!(engine
            .queue_play_media(vec![track("test", "second")], QueueOption::Next, None, false, false)
            .is_err());
        assert_eq!(engine.queue.items.len(), 1);
        assert_eq!(engine.next_queue_item_id, next_queue_item_id);
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    fn configuration() -> SendspinEngineConfiguration {
        SendspinEngineConfiguration {
            sendspin_url: "ws://localhost".to_string(),
            music_assistant_url: "http://localhost".to_string(),
            username: "test".to_string(),
            music_assistant_auth_token: None,
            client_id: "test".to_string(),
            client_name: "Test".to_string(),
            downloader_buffer_bytes: 1024,
            cache_bytes: 1024,
            preferred_codec: SendspinAudioCodec::Pcm,
        }
    }

    #[test]
    fn radio_replaces_the_queue_with_started_tracks_before_appending_recommendations() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.set_autoplay(true);
        engine
            .queue_play_media(vec![track("test", "old")], QueueOption::Play, None, false, true)
            .expect("seed old queue");
        engine
            .start_radio(vec![track("test", "started")])
            .expect("start radio");
        assert!(engine.autoplay.reserve_refill(1, Some(0)).is_none());
        let seed_track = match track("test", "seed") {
            MediaItem::Track(track) => track,
            _ => unreachable!(),
        };
        let radio_track = match track("test", "radio") {
            MediaItem::Track(track) => track,
            _ => unreachable!(),
        };
        engine
            .append_radio_tracks(vec![seed_track], vec![radio_track])
            .expect("append radio tracks");

        assert_eq!(
            engine
                .queue
                .items
                .iter()
                .filter_map(|item| item.media_item.as_ref())
                .map(|item| item.fields().item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["started", "radio"]
        );
        assert_eq!(engine.queue.current_index, Some(0));
        assert!(engine.autoplay.reserve_refill(2, Some(0)).is_some());
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn restored_queue_does_not_download_until_play() {
        let path = queue_snapshot_path().expect("test queue path");
        let config = configuration();
        let snapshot = QueueSnapshot {
            version: SNAPSHOT_VERSION,
            music_assistant_url: config.music_assistant_url.clone(),
            username: config.username.clone(),
            items: vec![
                QueueItem::local(
                    Queue::default().queue_id,
                    QueueItemId("local-1".into()),
                    0,
                    track("test", "first"),
                ),
                QueueItem::local(
                    Queue::default().queue_id,
                    QueueItemId("local-2".into()),
                    1,
                    track("test", "second"),
                ),
            ],
            current_index: Some(0),
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
        };
        for (action, expected_index) in
            [("seek", 0), ("next", 1), ("previous", 0), ("play_index", 1)]
        {
            snapshot.save(&path).expect("reset stored queue");
            let mut engine = SendspinEngine::new();
            engine.configure(config.clone()).expect("restore queue");
            engine.transport_pause();
            engine.transport_stop();
            assert!(engine.downloader.download_plan().is_none());
            assert!(engine.current_cache_content_key.is_none());

            match action {
                "seek" => engine.transport_seek(15.0).expect("seek restored queue"),
                "next" => engine.transport_next().expect("skip restored queue"),
                "previous" => engine.transport_previous().expect("restart restored queue"),
                "play_index" => engine.play_queue_index(1).expect("select restored track"),
                _ => unreachable!(),
            }
            assert_eq!(engine.queue.current_index, Some(expected_index));
            assert!(engine.current_cache_content_key.is_some());
            if action == "seek" {
                assert_eq!(engine.player_state.position_millis, Some(15_000));
            }
            engine.shutdown();
        }
        std::fs::remove_file(path).expect("remove test snapshot");
    }

    #[test]
    fn restored_repeat_all_queue_wraps_on_next_and_previous() {
        let path = queue_snapshot_path().expect("test queue path");
        let config = configuration();
        let queue_id = Queue::default().queue_id;
        let snapshot = QueueSnapshot {
            version: SNAPSHOT_VERSION,
            music_assistant_url: config.music_assistant_url.clone(),
            username: config.username.clone(),
            items: vec![
                QueueItem::local(
                    queue_id.clone(),
                    QueueItemId("repeat-first".into()),
                    0,
                    track("test", "first"),
                ),
                QueueItem::local(
                    queue_id,
                    QueueItemId("repeat-last".into()),
                    1,
                    track("test", "last"),
                ),
            ],
            current_index: Some(0),
            shuffle_enabled: false,
            repeat_mode: RepeatMode::All,
        };
        snapshot.save(&path).expect("store repeat queue");
        let mut engine = SendspinEngine::new();
        engine.configure(config).expect("restore repeat queue");
        engine.transport_previous().expect("wrap to last item");
        assert_eq!(engine.queue.current_index, Some(1));
        engine.transport_next().expect("wrap to first item");
        assert_eq!(engine.queue.current_index, Some(0));
        engine.shutdown();
        std::fs::remove_file(path).expect("remove repeat queue snapshot");
    }

    #[test]
    fn snapshot_with_duplicate_tracks_restores_selected_occurrence() {
        let path = queue_snapshot_path().expect("test queue path");
        let config = configuration();
        let mut items = vec![
            QueueItem::local(
                Queue::default().queue_id,
                QueueItemId("local-1".into()),
                0,
                track("test", "same"),
            ),
            QueueItem::local(
                Queue::default().queue_id,
                QueueItemId("local-2".into()),
                1,
                track("test", "same"),
            ),
        ];
        for (position, item) in items.iter_mut().enumerate() {
            item.sort_index = 61 + position as u32;
            item.index = 61 + position as u32;
        }
        QueueSnapshot {
            version: SNAPSHOT_VERSION,
            music_assistant_url: config.music_assistant_url,
            username: config.username,
            items,
            current_index: Some(1),
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
        }
        .save(&path)
        .expect("save queue snapshot");

        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("restore queue");
        assert_eq!(engine.queue.current_index, Some(1));
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(
            engine.queue.items[0].media_item,
            engine.queue.items[1].media_item
        );
        assert!(engine.current_cache_content_key.is_none());
        engine.transport_play().expect("play selected occurrence");
        engine.shutdown();

        let snapshot = QueueSnapshot::load(&path)
            .expect("load queue snapshot")
            .expect("saved snapshot");
        assert_eq!(snapshot.version, SNAPSHOT_VERSION);
        assert_eq!(snapshot.current_index, Some(1));
        std::fs::remove_file(path).expect("remove test snapshot");
    }

    #[test]
    fn restored_shuffled_queue_preserves_original_position_order() {
        let path = queue_snapshot_path().expect("test queue path");
        let config = configuration();
        let mut items = ["first", "third", "second"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| QueueItem::local(
                Queue::default().queue_id,
                QueueItemId(format!("local-{}", index + 1)),
                [61, 63, 62][index],
                track("test", name),
            ))
            .collect::<Vec<_>>();
        for (index, item) in items.iter_mut().enumerate() {
            item.index = (61 + index) as u32;
        }
        QueueSnapshot {
            version: SNAPSHOT_VERSION,
            music_assistant_url: config.music_assistant_url.clone(),
            username: config.username.clone(),
            current_index: Some(0),
            items,
            shuffle_enabled: true,
            repeat_mode: RepeatMode::Off,
        }.save(&path).expect("save shuffled queue");

        let mut engine = SendspinEngine::new();
        engine.configure(config).expect("restore shuffled queue");
        assert_eq!(engine.queue.current_index, Some(0));
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 2, 1]);
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        engine.set_shuffle(false);
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(engine.queue.items.iter().map(|item| item.media_item.as_ref().unwrap().fields().item_id.as_str()).collect::<Vec<_>>(), vec!["first", "second", "third"]);
        engine.shutdown();
        std::fs::remove_file(path).expect("remove test snapshot");
    }

    #[test]
    fn queue_changes_and_current_track_survive_engine_restart() {
        let path = queue_snapshot_path().expect("test queue path");
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        let first_track: MediaItem = serde_json::from_value(serde_json::json!({
            "media_type": "track",
            "item_id": "first",
            "provider": "test",
            "name": "first",
            "artists": [{
                "item_id": "artist-1",
                "provider": "test",
                "name": "Track artist",
                "artist_type": "group",
                "metadata": { "images": [{ "type": "thumb", "path": "artist.jpg", "provider": "test", "proxy_id": "artist-image" }] }
            }],
            "album": {
                "item_id": "album-1",
                "provider": "test",
                "name": "Complete album",
                "year": 2014,
                "artists": [{ "item_id": "artist-2", "provider": "test", "name": "Album artist" }],
                "metadata": { "description": "Complete metadata", "images": [{ "type": "thumb", "path": "album.jpg", "provider": "test", "proxy_id": "album-image" }] }
            }
        }))
        .expect("track with complete album");
        engine
            .queue_play_media(vec![
                first_track,
                track("test", "second"),
                track("test", "third"),
                track("test", "fourth"),
            ], QueueOption::Play, None, false, true)
            .expect("start queue");
        engine.play_queue_index(1).expect("select second track");
        engine.set_shuffle(true);
        engine.set_repeat_mode(RepeatMode::All);
        let mut saved_items = engine.queue.items.clone();
        for item in &mut saved_items {
            item.cache = None;
        }
        engine.shutdown();

        let saved_snapshot = QueueSnapshot::load(&path)
            .expect("load saved queue")
            .expect("saved queue snapshot");
        let saved_media = serde_json::to_value(&saved_snapshot.items[0].media_item)
            .expect("serialize saved media");
        assert_eq!(
            saved_media["artists"][0]["metadata"]["images"][0]["proxy_id"],
            "artist-image"
        );
        assert_eq!(saved_media["album"]["artists"][0]["name"], "Album artist");
        assert_eq!(
            saved_media["album"]["metadata"]["description"],
            "Complete metadata"
        );

        let mut restarted = SendspinEngine::new();
        restarted.configure(configuration()).expect("restore queue");
        assert_eq!(restarted.queue.items, saved_items);
        assert_eq!(restarted.queue.current_index, Some(1));
        assert!(restarted.queue.shuffle_enabled);
        assert_eq!(restarted.queue.repeat_mode, RepeatMode::All);
        let MediaItem::Track(restored_track) = restarted.queue.items[0]
            .media_item
            .as_ref()
            .expect("restored track")
        else {
            panic!("expected restored track");
        };
        assert_eq!(
            restored_track.album.as_ref().and_then(|album| album.year),
            Some(2014)
        );
        assert_eq!(
            restored_track.artists[0].artist_type.as_deref(),
            Some("group")
        );
        assert_eq!(
            restored_track.artists[0]
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images[0]
                .proxy_id
                .as_deref(),
            Some("artist-image")
        );
        assert_eq!(
            restored_track.album.as_ref().unwrap().artists[0].media.name,
            "Album artist"
        );
        assert_eq!(
            restored_track
                .album
                .as_ref()
                .unwrap()
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images[0]
                .proxy_id
                .as_deref(),
            Some("album-image")
        );
        assert!(restarted.current_cache_content_key.is_none());
        restarted.clear_queue().expect("clear restored queue");
        restarted.shutdown();

        let mut after_clear = SendspinEngine::new();
        after_clear
            .configure(configuration())
            .expect("restore empty queue");
        assert!(after_clear.queue.items.is_empty());
        after_clear.shutdown();
        std::fs::remove_file(path).expect("remove test snapshot");
    }

    #[test]
    fn playback_modes_survive_restart_without_queue_items() {
        let path = queue_snapshot_path().expect("test queue path");
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.set_shuffle(true);
        engine.set_repeat_mode(RepeatMode::One);
        engine.shutdown();

        let mut restarted = SendspinEngine::new();
        restarted
            .configure(configuration())
            .expect("restore playback modes");
        assert!(restarted.queue.items.is_empty());
        assert!(restarted.queue.shuffle_enabled);
        assert_eq!(restarted.queue.repeat_mode, RepeatMode::One);
        restarted.shutdown();
        std::fs::remove_file(path).expect("remove test snapshot");
    }

    #[test]
    fn another_account_does_not_restore_the_saved_queue() {
        let path = queue_snapshot_path().expect("test queue path");
        let config = configuration();
        QueueSnapshot {
            version: SNAPSHOT_VERSION,
            music_assistant_url: config.music_assistant_url.clone(),
            username: "another-listener".into(),
            items: vec![QueueItem::local(
                Queue::default().queue_id,
                QueueItemId("local-1".into()),
                0,
                track("test", "private-track"),
            )],
            current_index: Some(0),
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
        }
        .save(&path)
        .expect("save another user's queue");

        let mut engine = SendspinEngine::new();
        engine.configure(config).expect("configure current user");
        assert!(engine.queue.items.is_empty());
        assert!(engine.current_cache_content_key.is_none());
        engine.shutdown();
        std::fs::remove_file(path).expect("remove test snapshot");
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
    fn resets_reconnect_backoff_only_after_online_download_progress() {
        let mut engine = SendspinEngine::new();
        engine.schedule_reconnect_attempt();
        engine.player_state.connection_state = PlaybackConnectionState::Online;

        engine.reset_reconnect_backoff_after_download_progress(0);
        assert_eq!(engine.reconnect_delay, Duration::from_secs(4));

        engine.player_state.connection_state = PlaybackConnectionState::Reconnecting;
        engine.reset_reconnect_backoff_after_download_progress(1);
        assert_eq!(engine.reconnect_delay, Duration::from_secs(4));

        engine.player_state.connection_state = PlaybackConnectionState::Online;
        engine.reset_reconnect_backoff_after_download_progress(1);
        assert_eq!(engine.reconnect_delay, INITIAL_RECONNECT_DELAY);
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
    fn queue_download_snapshot_keeps_the_selected_item_current() {
        let mut engine = SendspinEngine::new();
        engine.config = Some(configuration());
        let queue_items = engine.new_queue_media_items(vec![
            track("test", "first"),
            track("test", "second"),
            track("test", "third"),
        ]);

        let plan = engine
            .build_download_plan_request(queue_items, 1)
            .expect("build queue plan");
        assert_eq!(plan.current_index, Some(1));
        assert_eq!(plan.queue_items.len(), 3);
        assert_eq!(plan.queue_items[1].queue_item_id.as_str(), "local-2");
    }

    #[test]
    fn queue_cache_keys_use_music_assistant_item_ids() {
        let mut engine = SendspinEngine::new();
        engine.config = Some(configuration());
        let queue_items =
            engine.new_queue_media_items(vec![track("deezer", "first"), track("deezer", "second")]);

        let plan = engine
            .build_download_plan_request(queue_items, 0)
            .expect("build queue plan");
        assert_eq!(
            queue_cache_content_key(&plan.queue_items[0].media_item, SendspinAudioCodec::Pcm)
                .expect("first cache key"),
            "deezer:first:pcm:48000:16:2"
        );
        assert_eq!(
            queue_cache_content_key(&plan.queue_items[1].media_item, SendspinAudioCodec::Pcm)
                .expect("second cache key"),
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
        engine.configure(configuration()).expect("configure engine");
        engine
            .queue_play_media(vec![
                track("test", "first"),
                track("test", "second"),
                track("test", "third"),
                track("test", "fourth"),
            ], QueueOption::Play, None, false, true)
            .expect("start playback");
        let original_upcoming_ids = engine.queue.items[1..]
            .iter()
            .map(|item| item.queue_item_id.clone())
            .collect::<Vec<_>>();

        engine.set_shuffle(true);

        let deadline = Instant::now() + Duration::from_secs(2);
        while engine.downloader.download_plan().as_ref().map(|plan| plan.version) != Some(2)
            && Instant::now() < deadline
        {
            std::thread::yield_now();
        }
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
        assert_eq!(
            downloader_plan
                .current_content_key
                .as_ref()
                .map(|key| key.as_str()),
            Some("test:first:pcm:48000:16:2")
        );
        assert!(downloader_plan.items[0].priority > downloader_plan.items[1].priority);
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn queue_play_media_shuffles_inputs_without_enabling_shuffle_mode() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.queue_play_media(
            vec![track("test", "first"), track("test", "second"), track("test", "third")],
            QueueOption::Play,
            None,
            true,
            true,
        ).expect("play shuffled inputs");

        let items = &engine.queue.items;
        assert!(!engine.queue.shuffle_enabled);
        assert_ne!(items.iter().map(|item| item.media_item.as_ref().unwrap().fields().item_id.as_str()).collect::<Vec<_>>(), vec!["first", "second", "third"]);
        assert_eq!(engine.queue.current_index, Some(0));
        assert_eq!(engine.download_plan_version, 1);

        engine.queue_play_media(
            vec![track("test", "fourth"), track("test", "fifth"), track("test", "sixth")],
            QueueOption::Add,
            None,
            true,
            true,
        ).expect("append shuffled inputs");
        assert!(!engine.queue.shuffle_enabled);
        assert_ne!(engine.queue.items[3..].iter().map(|item| item.media_item.as_ref().unwrap().fields().item_id.as_str()).collect::<Vec<_>>(), vec!["fourth", "fifth", "sixth"]);

        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn queue_play_media_uses_enabled_shuffle_mode_for_new_items() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.set_shuffle(true);
        engine
            .queue_play_media(
                vec![
                    track("test", "first"),
                    track("test", "second"),
                    track("test", "third"),
                    track("test", "fourth"),
                ],
                QueueOption::Play,
                None,
                false,
                true,
            )
            .expect("play items with shuffle mode enabled");

        assert!(engine.queue.shuffle_enabled);
        assert_ne!(
            engine
                .queue
                .items
                .iter()
                .map(|item| item.media_item.as_ref().unwrap().fields().item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second", "third", "fourth"],
        );
        let mut original_positions = engine
            .queue
            .items
            .iter()
            .map(|item| item.sort_index)
            .collect::<Vec<_>>();
        original_positions.sort_unstable();
        assert_eq!(original_positions, vec![0, 1, 2, 3]);

        let current_item_id = engine.queue.items[engine.queue.current_index.unwrap()]
            .media_item
            .as_ref()
            .unwrap()
            .fields()
            .item_id
            .clone();
        engine.set_shuffle(false);

        assert!(!engine.queue.shuffle_enabled);
        assert_eq!(
            engine.queue.items[0]
                .media_item
                .as_ref()
                .unwrap()
                .fields()
                .item_id,
            current_item_id,
        );
        assert!(engine.queue.items[1..]
            .windows(2)
            .all(|items| items[0].sort_index <= items[1].sort_index));
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn shuffled_play_keeps_the_requested_start_index() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.queue_play_media(
            vec![
                track("test", "first"),
                track("test", "second"),
                track("test", "selected"),
                track("test", "fourth"),
            ],
            QueueOption::Play,
            Some(2),
            true,
            true,
        ).expect("play shuffled item at requested index");

        assert_eq!(engine.queue.current_index, Some(2));
        assert_eq!(engine.queue.items[0].sort_index, 0);
        assert!(!engine.queue.shuffle_enabled);
        assert_ne!(
            engine.queue.items.iter()
                .map(|item| item.media_item.as_ref().unwrap().fields().item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second", "selected", "fourth"],
        );
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn queue_shuffle_restores_remaining_shuffled_play_order() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.queue_play_media(
            vec![
                track("test", "first"),
                track("test", "second"),
                track("test", "third"),
                track("test", "fourth"),
            ],
            QueueOption::Play,
            None,
            true,
            true,
        ).expect("shuffle played items");
        let original_ids = engine.queue.items.iter()
            .map(|item| item.queue_item_id.clone())
            .collect::<Vec<_>>();
        assert!(!engine.queue.shuffle_enabled);

        engine.set_shuffle(true);
        assert!(engine.queue.shuffle_enabled);
        let removed_id = engine.queue.items[1].queue_item_id.clone();
        engine.delete_queue_items(vec![removed_id.clone()]).expect("remove upcoming item");
        engine.set_shuffle(false);

        assert!(!engine.queue.shuffle_enabled);
        assert_eq!(
            engine.queue.items.iter().map(|item| item.queue_item_id.clone()).collect::<Vec<_>>(),
            original_ids.into_iter().filter(|item_id| item_id != &removed_id).collect::<Vec<_>>(),
        );
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }

    #[test]
    fn queue_shuffle_keeps_original_display_positions() {
        let mut engine = SendspinEngine::new();
        engine.configure(configuration()).expect("configure engine");
        engine.queue_play_media(
            vec![track("test", "first"), track("test", "second"), track("test", "third")],
            QueueOption::Replace, None, false, false,
        ).expect("create queue");

        engine.set_shuffle(true);

        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_ne!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1, 2]);
        let mut original_positions = engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>();
        original_positions.sort_unstable();
        assert_eq!(original_positions, vec![0, 1, 2]);
        engine.set_shuffle(false);
        assert_eq!(engine.queue.items.iter().map(|item| item.sort_index).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(engine.queue.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        engine.shutdown();
        std::fs::remove_file(queue_snapshot_path().expect("test queue path"))
            .expect("remove test snapshot");
    }
}
