use crate::music_assistant::models::identifiers::{AccessToken, CacheContentKey, QueueItemId};
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::CacheStatus;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendspinEngineConfiguration {
    pub(crate) sendspin_url: String,
    pub(crate) music_assistant_url: String,
    #[serde(default)]
    pub(crate) music_assistant_auth_token: Option<AccessToken>,
    pub(crate) client_id: String,
    pub(crate) client_name: String,
    pub(crate) downloader_buffer_bytes: u64,
    pub(crate) cache_bytes: usize,
    pub(crate) preferred_codec: SendspinAudioCodec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SendspinAudioCodec {
    Pcm,
    Opus,
    Flac,
}
impl SendspinAudioCodec {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pcm => "pcm",
            Self::Opus => "opus",
            Self::Flac => "flac",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CacheItemState {
    pub(crate) queue_item_id: QueueItemId,
    pub(crate) state: CacheStatus,
    pub(crate) cached_duration_millis: u64,
}
#[derive(Debug, Clone)]
pub(crate) struct SendspinDownloadPlanRequest {
    pub(crate) queue_revision: u64,
    pub(crate) queue_items: Vec<SendspinDownloadQueueItem>,
    pub(crate) current_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct SendspinDownloadQueueItem {
    pub(crate) queue_item_id: QueueItemId,
    pub(crate) original_index: usize,
    pub(crate) media_item: MediaItem,
}

#[derive(Debug, Clone)]
pub(crate) struct SendspinDownloadPlan {
    pub(crate) version: u64,
    pub(crate) current_content_key: Option<CacheContentKey>,
    pub(crate) items: Vec<SendspinDownloadPlanItem>,
}
#[derive(Debug, Clone)]
pub(crate) struct SendspinDownloadPlanItem {
    pub(crate) queue_item_id: QueueItemId,
    pub(crate) original_index: usize,
    pub(crate) queue_index: usize,
    pub(crate) priority: u32,
    pub(crate) cache_content_key: CacheContentKey,
    pub(crate) expected_duration_millis: Option<u64>,
    pub(crate) estimated_bytes: usize,
    pub(crate) media_item: MediaItem,
}
