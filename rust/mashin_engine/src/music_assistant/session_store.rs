//! Owns session persistence across a non-secret file and platform-protected credentials.
//! Version-3 secure sessions migrate file-first; failures retain the original secure entry.

use crate::configuration::{ConfigurationFile, SessionPreferences};
use crate::core::models::{SessionConfig, UpdateChannel};
use crate::music_assistant::models::identifiers::{AccessToken, UserId};
use crate::music_assistant::models::session::MusicAssistantServerUrl;
use serde::{Deserialize, Serialize};

pub(crate) trait SessionStore {
    fn load_session(&self) -> Result<Option<SessionConfig>, String>;
    fn save_session(&self, session: SessionConfig) -> Result<(), String>;
    fn clear_session(&self) -> Result<(), String>;
    fn get_update_channel(&self, default_channel: UpdateChannel) -> Result<UpdateChannel, String>;
    fn save_update_channel(&self, channel: UpdateChannel) -> Result<(), String>;
}

trait CredentialStore {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, serialized: &str) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionCredentials {
    format_version: u8,
    music_assistant_url: MusicAssistantServerUrl,
    username: String,
    user_id: Option<UserId>,
    access_token: Option<AccessToken>,
}

impl SessionCredentials {
    // Identity binds the protected token to its intended server and user.
    fn matches(&self, preferences: &SessionPreferences) -> bool {
        self.music_assistant_url == preferences.music_assistant_url
            && self.username == preferences.username
            && self.user_id == preferences.user_id
    }
}

impl From<&SessionConfig> for SessionCredentials {
    fn from(session: &SessionConfig) -> Self {
        Self {
            format_version: 1,
            music_assistant_url: session.music_assistant_url.clone(),
            username: session.username.clone(),
            user_id: session.user_id.clone(),
            access_token: session.access_token.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacySecureSession {
    format_version: u8,
    session: SessionConfig,
}

struct SplitSessionStore {
    configuration: ConfigurationFile,
    credentials: Box<dyn CredentialStore + Send>,
}

impl SessionStore for SplitSessionStore {
    // Restore only matching identities; corrupt files never fall back to stale secure preferences.
    fn load_session(&self) -> Result<Option<SessionConfig>, String> {
        let Some(serialized) = self.credentials.read()? else {
            return Ok(None);
        };
        if let Ok(credentials) = serde_json::from_str::<SessionCredentials>(&serialized) {
            if credentials.format_version != 1 {
                return Err("unsupported secure credential version".into());
            }
            let preferences = self
                .configuration
                .load()?
                .ok_or("configuration is missing; secure credentials were preserved")?;
            if !credentials.matches(&preferences) {
                return Err("configuration identity does not match secure credentials".into());
            }
            return Ok(Some(preferences.into_session(credentials.access_token)));
        }
        let legacy = serde_json::from_str::<LegacySecureSession>(&serialized)
            .map_err(|_| "secure session is invalid; the entry was preserved".to_string())?;
        if legacy.format_version != 3 {
            return Err("unsupported secure session version; the entry was preserved".into());
        }
        let session = match self.configuration.load()? {
            Some(preferences) => {
                if !SessionCredentials::from(&legacy.session).matches(&preferences) {
                    return Err("configuration identity does not match the legacy session".into());
                }
                preferences.into_session(legacy.session.access_token)
            }
            None => legacy.session,
        };
        self.save_session(session.clone())?;
        Ok(Some(session))
    }

    // A secure-store failure rolls back the file before the coordinator commits runtime state.
    fn save_session(&self, session: SessionConfig) -> Result<(), String> {
        let credentials = serde_json::to_string(&SessionCredentials::from(&session))
            .map_err(|_| "encode secure credentials failed".to_string())?;
        let previous = self.configuration.load()?;
        self.configuration
            .save(&SessionPreferences::from(&session))?;
        if let Err(error) = self.credentials.write(&credentials) {
            self.configuration
                .restore(previous.as_ref())
                .map_err(|rollback| {
                    format!("{error}; configuration rollback failed: {rollback}")
                })?;
            return Err(error);
        }
        Ok(())
    }

    // Signing out removes credentials, not the user's non-secret settings.
    fn clear_session(&self) -> Result<(), String> {
        self.credentials.clear()
    }

    // App preferences do not require credentials or an active session.
    fn get_update_channel(&self, default_channel: UpdateChannel) -> Result<UpdateChannel, String> {
        self.configuration.get_update_channel(default_channel)
    }

    fn save_update_channel(&self, channel: UpdateChannel) -> Result<(), String> {
        self.configuration.save_update_channel(channel)
    }
}

// Platform construction retains the existing secure-store service and account identifiers.
pub(crate) fn platform_session_store() -> Box<dyn SessionStore + Send> {
    Box::new(SplitSessionStore {
        configuration: ConfigurationFile::default(),
        credentials: Box::new(PlatformCredentialStore),
    })
}

struct PlatformCredentialStore;

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "linux"
))]
impl PlatformCredentialStore {
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new("Mashin", "music-assistant-session")
            .map_err(|error| format!("create secure session credential failed: {error}"))
    }
}

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "linux"
))]
impl CredentialStore for PlatformCredentialStore {
    // Platform credential commands.
    fn read(&self) -> Result<Option<String>, String> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("read secure credentials failed: {error}")),
        }
    }

    fn write(&self, serialized: &str) -> Result<(), String> {
        self.entry()?
            .set_password(serialized)
            .map_err(|error| format!("save secure credentials failed: {error}"))
    }

    fn clear(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("clear secure credentials failed: {error}")),
        }
    }
}

#[cfg(target_os = "android")]
impl CredentialStore for PlatformCredentialStore {
    // Android retains encrypted SharedPreferences through the existing JNI boundary.
    fn read(&self) -> Result<Option<String>, String> {
        crate::jni::android_context::read_secure_session()
    }

    fn write(&self, serialized: &str) -> Result<(), String> {
        crate::jni::android_context::write_secure_session(serialized)
    }

    fn clear(&self) -> Result<(), String> {
        crate::jni::android_context::clear_secure_session()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::tests::{session_config, TestDirectory};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryCredentials {
        value: Mutex<Option<String>>,
        should_fail_write: Mutex<bool>,
    }

    impl CredentialStore for Arc<MemoryCredentials> {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.value.lock().unwrap().clone())
        }

        fn write(&self, serialized: &str) -> Result<(), String> {
            if *self.should_fail_write.lock().unwrap() {
                return Err("test credential write failed".into());
            }
            *self.value.lock().unwrap() = Some(serialized.into());
            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    fn store(directory: &TestDirectory, credentials: &Arc<MemoryCredentials>) -> SplitSessionStore {
        SplitSessionStore {
            configuration: ConfigurationFile::with_path(directory.0.join("config.json")),
            credentials: Box::new(credentials.clone()),
        }
    }

    fn legacy_credentials(session: &SessionConfig) -> Arc<MemoryCredentials> {
        Arc::new(MemoryCredentials {
            value: Mutex::new(Some(
                serde_json::to_string(&LegacySecureSession {
                    format_version: 3,
                    session: session.clone(),
                })
                .unwrap(),
            )),
            should_fail_write: Mutex::new(false),
        })
    }

    #[test]
    fn session_round_trips_with_token_only_in_secure_storage() {
        let directory = TestDirectory::new();
        let credentials = Arc::new(MemoryCredentials::default());
        let store = store(&directory, &credentials);
        assert_eq!(store.load_session().unwrap(), None);
        let session = session_config();
        store.save_session(session.clone()).unwrap();
        assert_eq!(store.load_session().unwrap(), Some(session));
        let secure = credentials.read().unwrap().unwrap();
        assert!(secure.contains("secret-token"));
        assert!(!secure.contains("cacheBytes"));
        assert!(!secure.contains("theme"));
        let config = std::fs::read_to_string(directory.0.join("config.json")).unwrap();
        assert!(!config.contains("secret-token"));
        assert!(!config.contains("accessToken"));
        store.clear_session().unwrap();
        assert_eq!(store.load_session().unwrap(), None);
        assert!(store.configuration.load().unwrap().is_some());
    }

    #[test]
    fn update_channel_survives_login_logout_and_failed_session_commit() {
        let directory = TestDirectory::new();
        let credentials = Arc::new(MemoryCredentials::default());
        let store = store(&directory, &credentials);
        assert_eq!(store.get_update_channel(UpdateChannel::Dev).unwrap(), UpdateChannel::Dev);
        store.save_update_channel(UpdateChannel::Beta).unwrap();
        store.save_session(session_config()).unwrap();
        store.clear_session().unwrap();
        assert_eq!(store.get_update_channel(UpdateChannel::Stable).unwrap(), UpdateChannel::Beta);
        *credentials.should_fail_write.lock().unwrap() = true;
        assert!(store.save_session(session_config()).is_err());
        assert_eq!(store.get_update_channel(UpdateChannel::Stable).unwrap(), UpdateChannel::Beta);
        assert!(credentials.read().unwrap().is_none());
    }

    #[test]
    fn version_three_session_migrates_once_without_changing_preferences() {
        let directory = TestDirectory::new();
        let session = session_config();
        let credentials = legacy_credentials(&session);
        let store = store(&directory, &credentials);
        assert_eq!(store.load_session().unwrap(), Some(session.clone()));
        assert!(!credentials.read().unwrap().unwrap().contains("theme"));
        assert_eq!(store.load_session().unwrap(), Some(session));
    }

    #[test]
    fn interrupted_migration_keeps_the_existing_configuration() {
        let directory = TestDirectory::new();
        let session = session_config();
        let credentials = legacy_credentials(&session);
        let store = store(&directory, &credentials);
        let mut preferences = SessionPreferences::from(&session);
        preferences.volume = 15;
        store.configuration.save(&preferences).unwrap();
        assert_eq!(store.load_session().unwrap().unwrap().volume, 15);
        assert_eq!(store.configuration.load().unwrap(), Some(preferences));
    }

    #[test]
    fn failed_secure_write_rolls_back_configuration_and_preserves_legacy_entry() {
        let directory = TestDirectory::new();
        let session = session_config();
        let credentials = legacy_credentials(&session);
        let legacy = credentials.read().unwrap();
        *credentials.should_fail_write.lock().unwrap() = true;
        let store = store(&directory, &credentials);
        assert!(store.load_session().is_err());
        assert_eq!(store.configuration.load().unwrap(), None);
        assert_eq!(credentials.read().unwrap(), legacy);
        *credentials.should_fail_write.lock().unwrap() = false;
        store.save_session(session.clone()).unwrap();
        *credentials.should_fail_write.lock().unwrap() = true;
        let mut updated = session.clone();
        updated.volume = 10;
        assert!(store.save_session(updated).is_err());
        assert_eq!(store.load_session().unwrap(), Some(session));
    }

    #[test]
    fn corrupt_configuration_does_not_destroy_legacy_credentials() {
        let directory = TestDirectory::new();
        std::fs::create_dir_all(&directory.0).unwrap();
        let path = directory.0.join("config.json");
        std::fs::write(&path, "broken").unwrap();
        let credentials = legacy_credentials(&session_config());
        let original = credentials.read().unwrap();
        let store = store(&directory, &credentials);
        assert!(store.load_session().is_err());
        assert_eq!(credentials.read().unwrap(), original);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
    }

    #[test]
    fn failed_configuration_write_never_replaces_legacy_credentials() {
        let directory = TestDirectory::new();
        std::fs::create_dir_all(&directory.0).unwrap();
        let blocked = directory.0.join("not-a-directory");
        std::fs::write(&blocked, "keep").unwrap();
        let credentials = legacy_credentials(&session_config());
        let original = credentials.read().unwrap();
        let store = SplitSessionStore {
            configuration: ConfigurationFile::with_path(blocked.join("config.json")),
            credentials: Box::new(credentials.clone()),
        };
        assert!(store.load_session().is_err());
        assert_eq!(credentials.read().unwrap(), original);
        assert_eq!(std::fs::read_to_string(blocked).unwrap(), "keep");
    }

    #[test]
    fn unsupported_and_corrupt_secure_entries_are_preserved() {
        let directory = TestDirectory::new();
        let credentials = Arc::new(MemoryCredentials::default());
        let store = store(&directory, &credentials);
        for value in [
            "not-json".to_string(),
            serde_json::to_string(&LegacySecureSession {
                format_version: 2,
                session: session_config(),
            })
            .unwrap(),
            serde_json::to_string(&SessionCredentials {
                format_version: 255,
                ..SessionCredentials::from(&session_config())
            })
            .unwrap(),
        ] {
            credentials.write(&value).unwrap();
            assert!(store.load_session().is_err());
            assert_eq!(credentials.read().unwrap().as_deref(), Some(value.as_str()));
            assert_eq!(store.configuration.load().unwrap(), None);
        }
    }

    #[test]
    fn changing_configured_server_or_user_never_reuses_another_identitys_token() {
        let directory = TestDirectory::new();
        let credentials = Arc::new(MemoryCredentials::default());
        let store = store(&directory, &credentials);
        let session = session_config();
        store.save_session(session.clone()).unwrap();
        let mut preferences = SessionPreferences::from(&session);
        preferences.music_assistant_url = MusicAssistantServerUrl("https://other.example".into());
        store.configuration.save(&preferences).unwrap();
        assert!(store.load_session().is_err());
        preferences = SessionPreferences::from(&session);
        preferences.username = "another-user".into();
        store.configuration.save(&preferences).unwrap();
        assert!(store.load_session().is_err());
        std::fs::remove_file(directory.0.join("config.json")).unwrap();
        assert!(store.load_session().is_err());
        assert!(credentials.read().unwrap().is_some());
    }
}
