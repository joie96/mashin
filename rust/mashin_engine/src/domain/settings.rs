//! Typed application-core configuration, event, and error contracts.

use crate::domain::identifiers::{PlayerId, UserId};
use crate::domain::session::{AccessToken, MusicAssistantServerUrl};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioProfileId {
    Flac,
    #[serde(rename = "opus_96k")]
    Opus96k,
    #[serde(rename = "opus_128k")]
    Opus128k,
    #[serde(rename = "opus_192k")]
    Opus192k,
    #[default]
    #[serde(rename = "opus_256k")]
    Opus256k,
}

impl AudioProfileId {
    pub(crate) const ALL: [Self; 5] = [
        Self::Flac,
        Self::Opus96k,
        Self::Opus128k,
        Self::Opus192k,
        Self::Opus256k,
    ];

    pub(crate) fn bitrate(self) -> Option<u32> {
        match self {
            Self::Flac => None,
            Self::Opus96k => Some(96_000),
            Self::Opus128k => Some(128_000),
            Self::Opus192k => Some(192_000),
            Self::Opus256k => Some(256_000),
        }
    }
}

impl std::str::FromStr for AudioProfileId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "flac" => Ok(Self::Flac),
            "opus_96k" => Ok(Self::Opus96k),
            "opus_128k" => Ok(Self::Opus128k),
            "opus_192k" => Ok(Self::Opus192k),
            "opus_256k" => Ok(Self::Opus256k),
            _ => Err("invalid Mashin audio profile".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SelectedOutput {
    Local,
    Remote(RemoteOutputConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalOutputConfig {
    pub(crate) device_id: String,
    pub(crate) client_name: String,
    pub(crate) cache_bytes: usize,
    #[serde(default)]
    pub(crate) audio_profile: AudioProfileId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteOutputConfig {
    pub(crate) player_id: PlayerId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThemePreference {
    Dark,
    Light,
}

impl Default for ThemePreference {
    fn default() -> Self {
        Self::Dark
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum UpdateChannel {
    Stable,
    Beta,
    Dev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LogLevelPreference {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevelPreference {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl Default for LogLevelPreference {
    fn default() -> Self {
        Self::Info
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreferredProvider {
    pub(crate) provider_instance: String,
    pub(crate) provider_domain: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionConfig {
    pub(crate) music_assistant_url: MusicAssistantServerUrl,
    pub(crate) access_token: Option<AccessToken>,
    pub(crate) user_id: Option<UserId>,
    pub(crate) username: String,
    pub(crate) selected_output: SelectedOutput,
    pub(crate) local: LocalOutputConfig,
    pub(crate) volume: u8,
    #[serde(default)]
    pub(crate) theme: ThemePreference,
    #[serde(default)]
    pub(crate) log_level: LogLevelPreference,
    #[serde(default = "default_should_show_multi_device_button")]
    pub(crate) should_show_multi_device_button: bool,
    #[serde(default)]
    pub(crate) preferred_provider: Option<PreferredProvider>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionSettingsUpdate {
    pub(crate) theme: ThemePreference,
    pub(crate) log_level: LogLevelPreference,
    pub(crate) should_show_multi_device_button: bool,
}

fn default_should_show_multi_device_button() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginConfig {
    pub(crate) music_assistant_url: MusicAssistantServerUrl,
    pub(crate) username: String,
    pub(crate) password: String,
}

#[cfg(test)]
mod tests {
    use crate::adapters::models::{NativeOperation, NativeOperationRequest};
    use crate::application::queries::ApplicationQuery;

    #[test]
    fn decodes_optional_artwork_query_from_the_dart_contract() {
        let request = serde_json::from_value::<NativeOperationRequest>(serde_json::json!({
            "requestId": 43,
            "kind": "mediaArtwork",
            "item": {
                "media_type": "artist",
                "item_id": "artist-1",
                "provider": "library",
                "name": "Artist",
                "provider_mappings": [],
                "metadata": {}
            }
        }))
        .expect("decode optional artwork query");

        assert_eq!(request.request_id, 43);
        assert!(
            matches!(request.operation, NativeOperation::Query(ApplicationQuery::MediaArtwork { item })
            if item.fields().item_id.as_str() == "artist-1")
        );
    }
}

pub(crate) const DEFAULT_MUSIC_ASSISTANT_URL: &str = "http://192.168.1.2:8095";
pub(crate) const DEFAULT_LOCAL_VOLUME: u8 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSettings {
    pub(crate) music_assistant_url: String,
    pub(crate) username: Option<String>,
    pub(crate) audio_profile: AudioProfileId,
    pub(crate) theme: ThemePreference,
    pub(crate) log_level: LogLevelPreference,
    pub(crate) should_show_multi_device_button: bool,
    pub(crate) preferred_provider: Option<PreferredProvider>,
}

pub(crate) fn session_settings(session: Option<&SessionConfig>) -> SessionSettings {
    let (music_assistant_url, username, audio_profile, theme, log_level) = match session {
        Some(session) => (
            session.music_assistant_url.0.clone(),
            Some(session.username.clone()),
            session.local.audio_profile,
            session.theme,
            session.log_level,
        ),
        None => (
            DEFAULT_MUSIC_ASSISTANT_URL.to_string(),
            None,
            AudioProfileId::default(),
            ThemePreference::Dark,
            LogLevelPreference::Info,
        ),
    };
    SessionSettings {
        music_assistant_url,
        username,
        audio_profile,
        theme,
        log_level,
        should_show_multi_device_button: session
            .map_or(true, |session| session.should_show_multi_device_button),
        preferred_provider: session.and_then(|session| session.preferred_provider.clone()),
    }
}

// A device identity is generated only for a fresh login, never for output selection.
pub(crate) fn default_local_output(client_name: String) -> LocalOutputConfig {
    let platform = std::env::consts::OS;
    LocalOutputConfig {
        device_id: format!("mashin-{platform}-{}", uuid::Uuid::new_v4()),
        client_name,
        cache_bytes: 512 * 1024 * 1024,
        audio_profile: AudioProfileId::default(),
    }
}

// Device display names are refreshed independently of the persisted receiver identity.
pub(crate) fn refresh_local_client_name(
    configuration: &mut LocalOutputConfig,
    client_name: String,
) {
    configuration.client_name = client_name;
}

pub(crate) fn format_client_name(device_name: Option<&str>) -> String {
    let name = device_name.map(str::trim).filter(|name| !name.is_empty());
    match name {
        Some(name) => {
            let mut end = name.len().min(503);
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            format!("Mashin ({})", &name[..end])
        }
        None => "Mashin".to_string(),
    }
}

#[cfg(test)]
mod defaults_tests {}
