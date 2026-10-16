//! Authoritative default configuration for core-owned playback settings.

use super::models::{LocalOutputConfig, LogLevelPreference, SessionConfig, ThemePreference};
use crate::playback::local::mashin::models::AudioProfileId;
use serde::Serialize;

pub(crate) const DEFAULT_MUSIC_ASSISTANT_URL: &str = "http://192.168.1.2:8095";
pub(crate) const DEFAULT_LOCAL_VOLUME: u8 = 100;
pub(crate) const DEFAULT_LOCAL_CACHE_SIZE_BYTES: usize = 250 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSettings {
    pub(crate) music_assistant_url: String,
    pub(crate) username: Option<String>,
    pub(crate) audio_profile: AudioProfileId,
    pub(crate) local_cache_size_bytes: usize,
    pub(crate) theme: ThemePreference,
    pub(crate) log_level: LogLevelPreference,
}

pub(crate) fn session_settings(session: Option<&SessionConfig>) -> SessionSettings {
    let (music_assistant_url, username, audio_profile, local_cache_size_bytes, theme, log_level) =
        match session {
            Some(session) => (
                session.music_assistant_url.0.clone(),
                Some(session.username.clone()),
                session.local.audio_profile,
                session.local.cache_bytes,
                session.theme,
                session.log_level,
            ),
            None => (
                DEFAULT_MUSIC_ASSISTANT_URL.to_string(),
                None,
                AudioProfileId::Flac,
                DEFAULT_LOCAL_CACHE_SIZE_BYTES,
                ThemePreference::Dark,
                LogLevelPreference::Info,
            ),
        };
    SessionSettings {
        music_assistant_url,
        username,
        audio_profile,
        local_cache_size_bytes,
        theme,
        log_level,
    }
}

// A device identity is generated only for a fresh login, never for output selection.
pub(crate) fn default_local_output() -> LocalOutputConfig {
    let platform = std::env::consts::OS;
    LocalOutputConfig {
        device_id: format!("mashin-{platform}-{}", uuid::Uuid::new_v4()),
        client_name: format!("Mashin ({platform})"),
        cache_bytes: DEFAULT_LOCAL_CACHE_SIZE_BYTES,
        audio_profile: AudioProfileId::Flac,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        default_local_output, session_settings, DEFAULT_LOCAL_CACHE_SIZE_BYTES,
        DEFAULT_MUSIC_ASSISTANT_URL,
    };
    use crate::core::models::{LogLevelPreference, SelectedOutput, SessionConfig, ThemePreference};
    use crate::music_assistant::models::identifiers::AccessToken;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use crate::playback::local::mashin::models::AudioProfileId;

    #[test]
    fn fresh_login_creates_a_device_identity_and_all_v1_profiles_are_supported() {
        let configuration = default_local_output();
        assert_ne!(configuration.device_id, default_local_output().device_id);
        assert_eq!(configuration.cache_bytes, 250 * 1024 * 1024);
        assert_eq!(configuration.audio_profile, AudioProfileId::Flac);
    }

    #[test]
    fn session_settings_use_rust_defaults_without_a_session() {
        assert_eq!(
            session_settings(None),
            super::SessionSettings {
                music_assistant_url: DEFAULT_MUSIC_ASSISTANT_URL.to_string(),
                username: None,
                audio_profile: AudioProfileId::Flac,
                local_cache_size_bytes: DEFAULT_LOCAL_CACHE_SIZE_BYTES,
                theme: ThemePreference::Dark,
                log_level: LogLevelPreference::Info,
            }
        );
    }

    #[test]
    fn session_settings_project_stored_session_values() {
        let session = SessionConfig {
            music_assistant_url: MusicAssistantServerUrl("https://stored.example".into()),
            access_token: Some(AccessToken("secret-token".into())),
            user_id: None,
            username: "mashin".into(),
            selected_output: SelectedOutput::Local,
            local: super::LocalOutputConfig {
                cache_bytes: 1024,
                ..default_local_output()
            },
            volume: 100,
            theme: ThemePreference::Light,
            log_level: LogLevelPreference::Error,
        };

        let configuration = session_settings(Some(&session));

        assert_eq!(configuration.music_assistant_url, "https://stored.example");
        assert_eq!(configuration.username.as_deref(), Some("mashin"));
        assert_eq!(configuration.audio_profile, AudioProfileId::Flac);
        assert_eq!(configuration.local_cache_size_bytes, 1_024);
        assert_eq!(configuration.theme, ThemePreference::Light);
        assert_eq!(configuration.log_level, LogLevelPreference::Error);
    }
}
