use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CacheItemState {
    pub(crate) queue_item_id: String,
    pub(crate) state: String,
    pub(crate) cached_duration_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueItemCacheSnapshot {
    pub(crate) state: String,
    pub(crate) cached_duration_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueItemSnapshot {
    pub(crate) queue_item_id: String,
    pub(crate) original_index: usize,
    pub(crate) media_item: serde_json::Value,
    pub(crate) cache: QueueItemCacheSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueCacheUpdate {
    pub(crate) queue_item_id: String,
    pub(crate) cache: QueueItemCacheSnapshot,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadPlan {
    pub(crate) version: u64,
    pub(crate) items: Vec<DownloadPlanItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadPlanItem {
    pub(crate) queue_item_id: String,
    #[serde(default)]
    pub(crate) original_index: usize,
    #[allow(dead_code)]
    pub(crate) role: String,
    #[allow(dead_code)]
    pub(crate) priority: i32,
    pub(crate) cache_content_key: String,
    #[serde(default)]
    pub(crate) expected_duration_millis: Option<u64>,
    #[allow(dead_code)]
    pub(crate) media_item: serde_json::Value,
}
