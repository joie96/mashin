//! Persistent session storage contract for platform secure-storage adapters.

use crate::core::models::SessionConfig;

pub(crate) trait AuthStore {
    // Session values include credentials and must only be provided by secure platform storage.
    fn load_session(&self) -> Result<Option<SessionConfig>, String>;

    fn save_session(&self, session: SessionConfig) -> Result<(), String>;

    fn clear_session(&self) -> Result<(), String>;
}

// Platform-specific secure-session storage.
pub(crate) fn platform_auth_store() -> Box<dyn AuthStore + Send> {
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "linux"
    ))]
    return Box::new(KeyringAuthStore::new());

    #[cfg(target_os = "android")]
    return Box::new(AndroidAuthStore);

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    )))]
    Box::new(UnsupportedAuthStore)
}

#[cfg(target_os = "android")]
struct AndroidAuthStore;

#[cfg(target_os = "android")]
impl AuthStore for AndroidAuthStore {
    fn load_session(&self) -> Result<Option<SessionConfig>, String> {
        let serialized_session = crate::jni::android_context::read_secure_session()?;
        serialized_session
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| format!("decode Android secure session failed: {error}"))
    }

    fn save_session(&self, session: SessionConfig) -> Result<(), String> {
        let serialized_session = serde_json::to_string(&session)
            .map_err(|error| format!("encode Android secure session failed: {error}"))?;
        crate::jni::android_context::write_secure_session(&serialized_session)
    }

    fn clear_session(&self) -> Result<(), String> {
        crate::jni::android_context::clear_secure_session()
    }
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "linux"
)))]
struct UnsupportedAuthStore;

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "linux"
)))]
impl AuthStore for UnsupportedAuthStore {
    fn load_session(&self) -> Result<Option<SessionConfig>, String> {
        Err("secure session storage is not implemented on this platform".to_string())
    }

    fn save_session(&self, _session: SessionConfig) -> Result<(), String> {
        Err("secure session storage is not implemented on this platform".to_string())
    }

    fn clear_session(&self) -> Result<(), String> {
        Err("secure session storage is not implemented on this platform".to_string())
    }
}

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "linux"
))]
pub(crate) struct KeyringAuthStore;

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "linux"
))]
impl KeyringAuthStore {
    const SERVICE_NAME: &'static str = "Mashin";
    const ACCOUNT_NAME: &'static str = "music-assistant-session";

    // Construction.
    pub(crate) fn new() -> Self {
        Self
    }

    fn credential_entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(Self::SERVICE_NAME, Self::ACCOUNT_NAME)
            .map_err(|error| format!("create secure session credential failed: {error}"))
    }
}

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "linux"
))]
impl AuthStore for KeyringAuthStore {
    // Session persistence commands.
    fn load_session(&self) -> Result<Option<SessionConfig>, String> {
        let entry = self.credential_entry()?;
        let serialized_session = match entry.get_password() {
            Ok(value) => value,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(error) => return Err(format!("read secure session credential failed: {error}")),
        };
        serde_json::from_str(&serialized_session)
            .map(Some)
            .map_err(|error| format!("decode secure session credential failed: {error}"))
    }

    fn save_session(&self, session: SessionConfig) -> Result<(), String> {
        let serialized_session = serde_json::to_string(&session)
            .map_err(|error| format!("encode secure session credential failed: {error}"))?;
        self.credential_entry()?
            .set_password(&serialized_session)
            .map_err(|error| format!("save secure session credential failed: {error}"))
    }

    fn clear_session(&self) -> Result<(), String> {
        match self.credential_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("clear secure session credential failed: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AuthStore;
    use crate::core::models::{
        LogLevelPreference, SelectedOutput, SendspinOutputConfig, SessionConfig, ThemePreference,
    };
    use crate::music_assistant::models::identifiers::AccessToken;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use crate::sendspin::models::SendspinAudioCodec;
    use std::sync::Mutex;

    struct InMemoryAuthStore {
        session: Mutex<Option<SessionConfig>>,
    }

    impl InMemoryAuthStore {
        // Construction.
        fn new() -> Self {
            Self {
                session: Mutex::new(None),
            }
        }
    }

    impl AuthStore for InMemoryAuthStore {
        // Session persistence commands.
        fn load_session(&self) -> Result<Option<SessionConfig>, String> {
            self.session
                .lock()
                .map(|session| session.clone())
                .map_err(|_| "test auth store lock poisoned".to_string())
        }

        fn save_session(&self, session: SessionConfig) -> Result<(), String> {
            self.session
                .lock()
                .map(|mut current| *current = Some(session))
                .map_err(|_| "test auth store lock poisoned".to_string())
        }

        fn clear_session(&self) -> Result<(), String> {
            self.session
                .lock()
                .map(|mut current| *current = None)
                .map_err(|_| "test auth store lock poisoned".to_string())
        }
    }

    fn session_config() -> SessionConfig {
        SessionConfig {
            music_assistant_url: MusicAssistantServerUrl("https://music-assistant.example".into()),
            access_token: Some(AccessToken("secret-token".into())),
            user_id: None,
            username: "mashin".into(),
            selected_output: SelectedOutput::Sendspin(SendspinOutputConfig {
                sendspin_url: "ws://sendspin.example".into(),
                client_id: "mashin-test".into(),
                client_name: "Mashin Test".into(),
                downloader_buffer_bytes: 1_024,
                cache_bytes: 2_048,
                preferred_codec: SendspinAudioCodec::Pcm,
            }),
            volume: 100,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
        }
    }

    #[test]
    fn auth_store_round_trips_and_clears_session_configurations() {
        let store = InMemoryAuthStore::new();
        let session = session_config();

        assert_eq!(store.load_session().expect("load empty session"), None);
        store
            .save_session(session.clone())
            .expect("save session configuration");
        assert_eq!(
            store.load_session().expect("load saved session"),
            Some(session)
        );

        store.clear_session().expect("clear session configuration");
        assert_eq!(store.load_session().expect("load cleared session"), None);
    }
}
