use super::identifiers::{AccessToken, ProviderInstanceId, UserId};
use super::media::deserialize_nullable_vec;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct MusicAssistantServerUrl(pub(crate) String);

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MusicAssistantSession {
    pub(crate) server_url: MusicAssistantServerUrl,
    pub(crate) access_token: AccessToken,
    pub(crate) user_id: UserId,
    pub(crate) username: String,
}

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
    #[serde(default)]
    pub(crate) preferences: Map<String, Value>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProviderManifest {
    pub(crate) domain: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) icon: Option<String>,
    #[serde(default)]
    pub(crate) icon_svg: Option<String>,
    #[serde(default)]
    pub(crate) icon_svg_dark: Option<String>,
    #[serde(default)]
    pub(crate) icon_svg_monochrome: Option<String>,
    #[serde(default)]
    pub(crate) icon_images: Vec<String>,
    #[serde(default, rename = "type")]
    pub(crate) provider_type: Option<String>,
    #[serde(default)]
    pub(crate) builtin: Option<bool>,
}
