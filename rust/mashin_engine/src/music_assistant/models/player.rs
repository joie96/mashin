use super::identifiers::{PlayerId, QueueItemId};
use super::media::{deserialize_nullable_vec, MediaType};
use super::queue::PlayState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DeviceInfo {
    pub(crate) model: String,
    pub(crate) manufacturer: String,
    #[serde(default)]
    pub(crate) software_version: Option<String>,
    #[serde(default)]
    pub(crate) model_id: Option<String>,
    #[serde(default)]
    pub(crate) manufacturer_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PlayerMedia {
    pub(crate) uri: String,
    #[serde(default)]
    pub(crate) media_type: Option<MediaType>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) artist: Option<String>,
    #[serde(default)]
    pub(crate) album: Option<String>,
    #[serde(default)]
    pub(crate) image_url: Option<String>,
    #[serde(default)]
    pub(crate) duration: Option<f64>,
    #[serde(default)]
    pub(crate) queue_item_id: Option<QueueItemId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MusicAssistantOutputProtocol {
    pub(crate) output_protocol_id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) protocol_domain: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MusicAssistantPlayer {
    pub(crate) player_id: PlayerId,
    pub(crate) provider: String,
    #[serde(rename = "type")]
    pub(crate) player_type: String,
    pub(crate) name: String,
    pub(crate) available: bool,
    pub(crate) device_info: DeviceInfo,
    #[serde(default)]
    pub(crate) playback_state: PlayState,
    #[serde(default)]
    pub(crate) elapsed_time: Option<f64>,
    #[serde(default)]
    pub(crate) powered: Option<bool>,
    #[serde(default)]
    pub(crate) volume_level: Option<u8>,
    #[serde(default)]
    pub(crate) volume_muted: Option<bool>,
    #[serde(default)]
    pub(crate) active_source: Option<String>,
    #[serde(default)]
    pub(crate) current_media: Option<PlayerMedia>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) output_protocols: Vec<MusicAssistantOutputProtocol>,
}
