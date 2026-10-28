//! Persistent session storage contract for platform secure-storage adapters.

use crate::core::models::SessionConfig;
use serde::{Deserialize, Serialize};

const SESSION_STORAGE_VERSION: u8 = 3;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredSession {
    format_version: u8,
    session: SessionConfig,
}

fn decode_session(serialized: &str) -> Option<SessionConfig> {
    let stored = serde_json::from_str::<StoredSession>(serialized).ok()?;
    (stored.format_version == SESSION_STORAGE_VERSION).then_some(stored.session)
}

fn encode_session(session: SessionConfig) -> Result<String, String> {
    serde_json::to_string(&StoredSession {
        format_version: SESSION_STORAGE_VERSION,
        session,
    })
    .map_err(|_| "encode secure session failed".to_string())
}

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
        let Some(serialized_session) = serialized_session else {
            return Ok(None);
        };
        if let Some(session) = decode_session(&serialized_session) {
            return Ok(Some(session));
        }
        self.clear_session()?;
        Ok(None)
    }

    fn save_session(&self, session: SessionConfig) -> Result<(), String> {
        let serialized_session = encode_session(session)?;
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
        if let Some(session) = decode_session(&serialized_session) {
            return Ok(Some(session));
        }
        self.clear_session()?;
        Ok(None)
    }

    fn save_session(&self, session: SessionConfig) -> Result<(), String> {
        let serialized_session = encode_session(session)?;
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
    use super::{decode_session, encode_session, AuthStore};
    use crate::core::models::{LogLevelPreference, SelectedOutput, SessionConfig, ThemePreference};
    use crate::music_assistant::models::identifiers::AccessToken;
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
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
            selected_output: SelectedOutput::Local,
            local: crate::core::config::default_local_output(),
            volume: 100,
            theme: ThemePreference::Dark,
            log_level: LogLevelPreference::Info,
            should_show_multi_device_button: true,
            preferred_provider: None,
        }
    }

    #[test]
    fn unversioned_and_corrupt_sessions_require_fresh_login() {
        let legacy = serde_json::to_string(&session_config()).unwrap();
        assert!(decode_session(&legacy).is_none());
        assert!(decode_session("not-json").is_none());
        assert!(decode_session(r#"{"formatVersion":255,"session":{}}"#).is_none());
    }

    #[test]
    fn versioned_sessions_round_trip_without_a_legacy_decoder() {
        let session = session_config();
        let serialized = encode_session(session.clone()).unwrap();
        assert_eq!(decode_session(&serialized), Some(session));
    }

    #[test]
    fn multi_device_visibility_defaults_for_existing_sessions_and_round_trips() {
        let mut session = session_config();
        let mut existing: serde_json::Value =
            serde_json::from_str(&encode_session(session.clone()).unwrap()).unwrap();
        existing["session"]
            .as_object_mut()
            .unwrap()
            .remove("shouldShowMultiDeviceButton");
        assert_eq!(decode_session(&existing.to_string()), Some(session.clone()));
        session.should_show_multi_device_button = false;
        assert_eq!(
            decode_session(&encode_session(session.clone()).unwrap()),
            Some(session)
        );
    }

    #[test]
    fn preferred_provider_defaults_for_existing_sessions_and_round_trips() {
        let mut session = session_config();
        let mut existing = serde_json::to_value(super::StoredSession {
            format_version: super::SESSION_STORAGE_VERSION,
            session: session.clone(),
        })
        .unwrap();
        existing["session"]
            .as_object_mut()
            .unwrap()
            .remove("preferredProvider");
        assert_eq!(decode_session(&existing.to_string()), Some(session.clone()));
        session.preferred_provider = Some(crate::core::models::PreferredProvider {
            provider_instance: "tidal-instance".into(),
            provider_domain: "tidal".into(),
        });
        assert_eq!(
            decode_session(&encode_session(session.clone()).unwrap()),
            Some(session)
        );
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

    #[test]
    fn previous_schema_and_legacy_outputs_require_a_fresh_login() {
        let serialized = encode_session(session_config()).unwrap();
        let mut previous: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        previous["formatVersion"] = serde_json::json!(1);
        assert!(decode_session(&previous.to_string()).is_none());
        previous["formatVersion"] = serde_json::json!(2);
        assert!(decode_session(&previous.to_string()).is_none());
        previous["formatVersion"] = serde_json::json!(3);
        previous["session"]["selectedOutput"] =
            serde_json::json!({"offline":{"libraryRoot":"old"}});
        assert!(decode_session(&previous.to_string()).is_none());
    }
}
