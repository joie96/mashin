//! Authoritative default configuration for core-owned playback settings.

use super::models::{
    LocalOutputConfig, LogLevelPreference, PreferredProvider, SessionConfig, ThemePreference,
};
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
    pub(crate) preferred_provider: Option<PreferredProvider>,
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
        preferred_provider: session.and_then(|session| session.preferred_provider.clone()),
    }
}

// A device identity is generated only for a fresh login, never for output selection.
pub(crate) fn default_local_output() -> LocalOutputConfig {
    let platform = std::env::consts::OS;
    LocalOutputConfig {
        device_id: format!("mashin-{platform}-{}", uuid::Uuid::new_v4()),
        client_name: local_client_name(),
        cache_bytes: DEFAULT_LOCAL_CACHE_SIZE_BYTES,
        audio_profile: AudioProfileId::Flac,
    }
}

// Device display names are refreshed independently of the persisted receiver identity.
pub(crate) fn refresh_local_client_name(configuration: &mut LocalOutputConfig) {
    configuration.client_name = local_client_name();
}

fn local_client_name() -> String {
    format_client_name(device_name().as_deref())
}

fn format_client_name(device_name: Option<&str>) -> String {
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

#[cfg(target_os = "windows")]
fn device_name() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetComputerNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }

    let mut buffer = [0u16; 256];
    let mut size = buffer.len() as u32;
    if unsafe { GetComputerNameW(buffer.as_mut_ptr(), &mut size) } == 0 {
        return None;
    }
    String::from_utf16(&buffer[..size as usize]).ok()
}

#[cfg(target_os = "android")]
fn device_name() -> Option<String> {
    crate::jni::android_context::device_name().ok()
}

#[cfg(not(any(target_os = "windows", target_os = "android")))]
fn device_name() -> Option<String> {
    std::env::var("HOSTNAME").ok()
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
        assert_eq!(configuration.client_name, super::local_client_name());
        assert_eq!(configuration.cache_bytes, 250 * 1024 * 1024);
        assert_eq!(configuration.audio_profile, AudioProfileId::Flac);
    }

    #[test]
    fn client_name_uses_the_device_name_and_handles_missing_or_long_names() {
        assert_eq!(
            super::format_client_name(Some(" ERIC-PC ")),
            "Mashin (ERIC-PC)"
        );
        assert_eq!(
            super::format_client_name(Some("Pixel 9")),
            "Mashin (Pixel 9)"
        );
        assert_eq!(super::format_client_name(Some("  ")), "Mashin");
        assert_eq!(super::format_client_name(None), "Mashin");
        let name = "\u{00e9}".repeat(300);
        let formatted = super::format_client_name(Some(&name));
        assert!(formatted.len() <= 512);
        assert!(formatted.ends_with(')'));
    }

    #[test]
    fn refreshing_a_stored_name_preserves_identity_and_playback_preferences() {
        let mut configuration = default_local_output();
        let original = configuration.clone();
        configuration.client_name = "Mashin (windows)".into();

        super::refresh_local_client_name(&mut configuration);

        assert_eq!(configuration, original);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_device_name_matches_the_computer_name() {
        assert_eq!(
            super::device_name().unwrap().to_uppercase(),
            std::env::var("COMPUTERNAME").unwrap().to_uppercase()
        );
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
                preferred_provider: None,
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
            preferred_provider: None,
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
