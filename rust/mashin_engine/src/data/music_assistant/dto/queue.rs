use crate::domain::identifiers::QueueId;
use crate::playback::models::{PlayState, QueueItem, RepeatMode};
use serde::{Deserialize, Serialize};

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
            queue_id: QueueId(String::new()),
            active: false,
            display_name: String::new(),
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
    use super::MusicAssistantQueue;

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
}
