//! Authoritative default configuration for core-owned playback settings.

use super::models::{SelectedOutput, SendspinOutputConfig, SessionConfig};
use crate::sendspin::models::SendspinAudioCodec;
use rand::Rng;
use serde::Serialize;

pub(crate) const DEFAULT_MUSIC_ASSISTANT_URL: &str = "http://192.168.1.2:8095";
pub(crate) const DEFAULT_LOCAL_VOLUME: u8 = 100;
const DEFAULT_SENDSPIN_DOWNLOADER_BUFFER_BYTES: u64 = 50 * 1024 * 1024;
const DEFAULT_SENDSPIN_CACHE_BYTES: usize = 250 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSettings {
    pub(crate) music_assistant_url: String,
    pub(crate) username: Option<String>,
    pub(crate) preferred_codec: String,
}

pub(crate) fn session_settings(session: Option<&SessionConfig>) -> SessionSettings {
    let (music_assistant_url, username, preferred_codec) = match session {
        Some(session) => (
            session.music_assistant_url.0.clone(),
            Some(session.username.clone()),
            match &session.selected_output {
                SelectedOutput::Sendspin(configuration) => configuration.preferred_codec.as_str(),
                SelectedOutput::Offline(_) | SelectedOutput::Remote(_) => {
                    SendspinAudioCodec::Pcm.as_str()
                }
            },
        ),
        None => (
            DEFAULT_MUSIC_ASSISTANT_URL.to_string(),
            None,
            SendspinAudioCodec::Pcm.as_str(),
        ),
    };
    SessionSettings {
        music_assistant_url,
        username,
        preferred_codec: preferred_codec.to_string(),
    }
}

// Derivate sendspin url from the Music Assistant URL
pub(crate) fn default_sendspin_output(music_assistant_url: &str) -> Result<SelectedOutput, String> {
    let mut url = reqwest::Url::parse(music_assistant_url.trim())
        .map_err(|error| format!("invalid Music Assistant URL for Sendspin: {error}"))?;
    let sendspin_scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        scheme => {
            return Err(format!(
                "unsupported Music Assistant URL scheme for Sendspin: {scheme}"
            ));
        }
    };
    url.set_scheme(sendspin_scheme)
        .map_err(|_| "set Sendspin URL scheme failed".to_string())?;
    url.set_port(Some(8927))
        .map_err(|_| "set Sendspin URL port failed".to_string())?;
    url.set_path("/sendspin");
    url.set_query(None);
    url.set_fragment(None);

    let platform = std::env::consts::OS;
    let device_suffix = rand::thread_rng().gen::<u64>();
    Ok(SelectedOutput::Sendspin(SendspinOutputConfig {
        sendspin_url: url.into(),
        client_id: format!("mashin-{platform}-{device_suffix:016x}"),
        client_name: format!("Mashin ({platform})"),
        downloader_buffer_bytes: DEFAULT_SENDSPIN_DOWNLOADER_BUFFER_BYTES,
        cache_bytes: DEFAULT_SENDSPIN_CACHE_BYTES,
        preferred_codec: SendspinAudioCodec::Pcm,
    }))
}

#[cfg(test)]
mod tests {
    use super::{default_sendspin_output, session_settings, DEFAULT_MUSIC_ASSISTANT_URL};
    use crate::core::models::{SelectedOutput, SendspinOutputConfig, SessionConfig};
    use crate::music_assistant::models::identifiers::AccessToken;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use crate::sendspin::models::SendspinAudioCodec;

    #[test]
    fn default_sendspin_output_derives_the_secure_sendspin_endpoint() {
        let output = default_sendspin_output("https://music-assistant.example:8095/api?debug=true")
            .expect("derive Sendspin output");
        let SelectedOutput::Sendspin(configuration) = output else {
            panic!("default output must select Sendspin");
        };

        assert_eq!(
            configuration.sendspin_url,
            "wss://music-assistant.example:8927/sendspin"
        );
        assert_eq!(configuration.downloader_buffer_bytes, 50 * 1024 * 1024);
        assert_eq!(configuration.cache_bytes, 250 * 1024 * 1024);
        assert_eq!(configuration.preferred_codec, SendspinAudioCodec::Pcm);
    }

    #[test]
    fn session_settings_use_rust_defaults_without_a_session() {
        assert_eq!(
            session_settings(None),
            super::SessionSettings {
                music_assistant_url: DEFAULT_MUSIC_ASSISTANT_URL.to_string(),
                username: None,
                preferred_codec: "pcm".to_string(),
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
            selected_output: SelectedOutput::Sendspin(SendspinOutputConfig {
                sendspin_url: "wss://stored.example:8927/sendspin".into(),
                client_id: "mashin-test".into(),
                client_name: "Mashin Test".into(),
                downloader_buffer_bytes: 1_024,
                cache_bytes: 1_024,
                preferred_codec: SendspinAudioCodec::Flac,
            }),
            volume: 100,
        };

        let configuration = session_settings(Some(&session));

        assert_eq!(configuration.music_assistant_url, "https://stored.example");
        assert_eq!(configuration.username.as_deref(), Some("mashin"));
        assert_eq!(configuration.preferred_codec, "flac");
    }
}
