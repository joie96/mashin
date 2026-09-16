use super::identifiers::{QueueId, QueueItemId};
use super::media::{MediaItem, MediaItemImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepeatMode {
    #[default]
    Off,
    One,
    All,
    #[serde(other)]
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlayState {
    Uninitialized,
    #[default]
    Idle,
    Buffering,
    Paused,
    Playing,
    Seeking,
    Error,
    #[serde(other)]
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueueOption {
    Play,
    Replace,
    Next,
    ReplaceNext,
    Add,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StreamDetails {
    pub(crate) provider: String,
    pub(crate) item_id: String,
    #[serde(default)]
    pub(crate) duration: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CacheStatus {
    Queued,
    Prepared,
    Buffering,
    Downloading,
    Complete,
    Interrupted,
    Evicted,
    Discarded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct CacheState {
    pub(crate) status: CacheStatus,
    pub(crate) cached_duration_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct QueueItem {
    pub(crate) queue_id: QueueId,
    pub(crate) queue_item_id: QueueItemId,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) duration: Option<u32>,
    #[serde(default)]
    pub(crate) sort_index: u32,
    #[serde(default)]
    pub(crate) streamdetails: Option<StreamDetails>,
    #[serde(default)]
    pub(crate) media_item: Option<MediaItem>,
    #[serde(default)]
    pub(crate) image: Option<MediaItemImage>,
    #[serde(default)]
    pub(crate) index: u32,
    #[serde(default = "default_true")]
    pub(crate) available: bool,
    #[serde(default)]
    pub(crate) cache: Option<CacheState>,
}

impl QueueItem {
    pub(crate) fn local(
        queue_id: QueueId,
        queue_item_id: QueueItemId,
        sort_index: usize,
        media_item: MediaItem,
    ) -> Self {
        let name = media_item.fields().name.clone();
        let duration = media_item.duration_seconds();
        Self {
            queue_id,
            queue_item_id,
            name,
            duration,
            sort_index: sort_index as u32,
            streamdetails: None,
            media_item: Some(media_item),
            image: None,
            index: sort_index as u32,
            available: true,
            cache: Some(CacheState {
                status: CacheStatus::Queued,
                cached_duration_millis: 0,
            }),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct MusicAssistantQueue {
    pub(crate) queue_id: QueueId,
    #[serde(default)]
    pub(crate) active: bool,
    #[serde(default)]
    pub(crate) display_name: String,
    #[serde(default = "default_true")]
    pub(crate) available: bool,
    #[serde(default, deserialize_with = "deserialize_queue_items")]
    pub(crate) items: Vec<QueueItem>,
    #[serde(default)]
    pub(crate) shuffle_enabled: bool,
    #[serde(default)]
    pub(crate) repeat_mode: RepeatMode,
    #[serde(default)]
    pub(crate) autoplay_enabled: bool,
    #[serde(default)]
    pub(crate) current_index: Option<usize>,
    #[serde(default)]
    pub(crate) elapsed_time: f64,
    #[serde(default)]
    pub(crate) state: PlayState,
    #[serde(default)]
    pub(crate) current_item: Option<QueueItem>,
    #[serde(default)]
    pub(crate) next_item: Option<QueueItem>,
    #[serde(default)]
    pub(crate) is_dynamic: bool,
}

impl Default for MusicAssistantQueue {
    fn default() -> Self {
        Self {
            queue_id: QueueId("local-sendspin".to_string()),
            active: false,
            display_name: "Local Sendspin".to_string(),
            available: true,
            items: Vec::new(),
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
            autoplay_enabled: false,
            current_index: None,
            elapsed_time: 0.0,
            state: PlayState::Uninitialized,
            current_item: None,
            next_item: None,
            is_dynamic: false,
        }
    }
}

fn deserialize_queue_items<'de, D>(deserializer: D) -> Result<Vec<QueueItem>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum QueueItems {
        Items(Vec<QueueItem>),
        ItemCount(u32),
        Empty(Option<()>),
    }

    match QueueItems::deserialize(deserializer)? {
        QueueItems::Items(items) => Ok(items),
        QueueItems::ItemCount(_) | QueueItems::Empty(_) => Ok(Vec::new()),
    }
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{CacheState, CacheStatus, MusicAssistantQueue, QueueItem};
    use crate::music_assistant::models::identifiers::{QueueId, QueueItemId};
    use crate::music_assistant::models::media::{MediaItem, MediaItemFields, Track};

    #[test]
    fn deserializes_music_assistant_queue_metadata_with_item_count() {
        let queue: MusicAssistantQueue = serde_json::from_str(
            r#"{
                "queue_id": "living-room",
                "display_name": "Living Room",
                "items": 12,
                "shuffle_enabled": true,
                "repeat_mode": "all",
                "current_index": 3,
                "state": "playing"
            }"#,
        )
        .expect("deserialize Music Assistant queue");

        assert_eq!(queue.queue_id.as_str(), "living-room");
        assert_eq!(queue.display_name, "Living Room");
        assert!(queue.items.is_empty());
        assert!(queue.shuffle_enabled);
        assert_eq!(queue.current_index, Some(3));
    }

    #[test]
    fn deserializes_music_assistant_queue_with_null_items() {
        let queue: MusicAssistantQueue = serde_json::from_str(
            r#"{
                "queue_id": "up8d689fb7",
                "display_name": "Sendspin downloader",
                "items": null,
                "state": "idle"
            }"#,
        )
        .expect("deserialize Music Assistant queue with null items");

        assert_eq!(queue.queue_id.as_str(), "up8d689fb7");
        assert!(queue.items.is_empty());
    }

    #[test]
    fn deserializes_music_assistant_queue_item_without_media_item() {
        let item: QueueItem = serde_json::from_str(
            r#"{
                "queue_id": "upda1f28c7",
                "queue_item_id": "ee609fd141b44306b1e8e475b9ea61b4",
                "name": "Unavailable item",
                "media_item": null
            }"#,
        )
        .expect("deserialize Music Assistant queue item without media item");

        assert_eq!(item.queue_id.as_str(), "upda1f28c7");
        assert!(item.media_item.is_none());
    }

    #[test]
    fn serializes_queue_item_for_ffi_payload() {
        let item = QueueItem::local(
            QueueId("local-sendspin".to_string()),
            QueueItemId("local-1".to_string()),
            2,
            MediaItem::Track(Track {
                media: MediaItemFields {
                    item_id: crate::music_assistant::models::identifiers::MediaId(
                        "track-1".to_string(),
                    ),
                    provider: crate::music_assistant::models::identifiers::ProviderInstanceId(
                        "library".to_string(),
                    ),
                    name: "A Track".to_string(),
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
                duration: 120,
                artists: Vec::new(),
                album: None,
                image: None,
                disc_number: 0,
                track_number: 0,
            }),
        );
        let queue = MusicAssistantQueue {
            items: vec![item],
            ..MusicAssistantQueue::default()
        };

        let serialized = serde_json::to_value(queue).expect("serialize FFI queue payload");

        assert_eq!(serialized["queue_id"], "local-sendspin");
        assert_eq!(serialized["items"][0]["queue_item_id"], "local-1");
        assert_eq!(serialized["items"][0]["sort_index"], 2);
        assert_eq!(serialized["items"][0]["cache"]["status"], "queued");
    }

    #[test]
    fn cache_state_uses_snake_case_status_values() {
        let serialized = serde_json::to_value(CacheState {
            status: CacheStatus::Downloading,
            cached_duration_millis: 1_000,
        })
        .expect("serialize cache state");

        assert_eq!(serialized["status"], "downloading");
        assert_eq!(serialized["cached_duration_millis"], 1_000);
    }
}
