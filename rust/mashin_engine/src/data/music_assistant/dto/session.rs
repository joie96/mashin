use crate::domain::identifiers::{ProviderInstanceId, UserId};
use crate::domain::media::deserialize_nullable_vec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MusicAssistantUser {
    pub(crate) user_id: UserId,
    pub(crate) username: String,
    pub(crate) role: String,
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) display_name: Option<String>,
    #[serde(default)]
    pub(crate) avatar_url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) provider_filter: Vec<ProviderInstanceId>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) player_filter: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ServerInfo {
    pub(crate) server_id: String,
    pub(crate) server_version: String,
    pub(crate) schema_version: u32,
    pub(crate) min_supported_schema_version: u32,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) homeassistant_addon: bool,
    #[serde(default)]
    pub(crate) onboard_done: bool,
}
