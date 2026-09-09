use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadSlotSnapshot {
    pub(crate) slot: u8,
    pub(crate) connection_state: String,
    pub(crate) cache_content_key: Option<String>,
    pub(crate) lease_id: Option<String>,
    pub(crate) state: String,
    pub(crate) cached_duration_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CacheItemSnapshot {
    pub(crate) cache_content_key: String,
    pub(crate) queue_item_id: Option<String>,
    pub(crate) state: String,
    pub(crate) cached_duration_millis: u64,
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
    #[allow(dead_code)]
    pub(crate) role: String,
    #[allow(dead_code)]
    pub(crate) priority: i32,
    pub(crate) cache_content_key: String,
    #[allow(dead_code)]
    pub(crate) media_item: serde_json::Value,
}

impl DownloadSlotSnapshot {
    pub(crate) fn idle(slot: u8) -> Self {
        Self {
            slot,
            connection_state: "offline".to_string(),
            cache_content_key: None,
            lease_id: None,
            state: "idle".to_string(),
            cached_duration_millis: 0,
        }
    }
}
