//! Owns session persistence across a non-secret file and injected protected credentials.
//! Unsupported entries remain untouched; neither format nor account scope changes here.

use super::store::CredentialStore;
use crate::application::session::SessionStorage;
use crate::configuration::{ConfigurationFile, SessionPreferences};
use crate::domain::identifiers::UserId;
use crate::domain::session::{AccessToken, MusicAssistantServerUrl};
use crate::domain::settings::SessionConfig;
use serde::{Deserialize, Serialize};

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

pub(crate) struct SplitSessionStorage {
    configuration: ConfigurationFile,
    credentials: Box<dyn CredentialStore>,
}

impl SplitSessionStorage {
    // Bootstrap supplies both persistence capabilities without platform imports here.
    pub(crate) fn new(
        configuration: ConfigurationFile,
        credentials: Box<dyn CredentialStore>,
    ) -> Self {
        Self {
            configuration,
            credentials,
        }
    }
}

impl SessionStorage for SplitSessionStorage {
    // Restore only credentials bound to the configured server and user.
    fn load_session(&self) -> Result<Option<SessionConfig>, String> {
        let Some(serialized) = self.credentials.read()? else {
            return Ok(None);
        };
        let credentials = serde_json::from_str::<SessionCredentials>(&serialized)
            .map_err(|_| "secure credentials are invalid; the entry was preserved".to_string())?;
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
        Ok(Some(preferences.into_session(credentials.access_token)))
    }

    // A secure-store failure rolls back the file before runtime state can be committed.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::tests::{session_config, TestDirectory};
    use crate::domain::settings::UpdateChannel;
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

    fn store(
        directory: &TestDirectory,
        credentials: &Arc<MemoryCredentials>,
    ) -> SplitSessionStorage {
        SplitSessionStorage::new(
            ConfigurationFile::with_path(directory.0.join("config.json")),
            Box::new(credentials.clone()),
        )
    }

    #[test]
    fn existing_version_one_credential_shape_is_unchanged() {
        let directory = TestDirectory::new();
        let credentials = Arc::new(MemoryCredentials::default());
        let store = store(&directory, &credentials);
        let session = session_config();
        let expected = serde_json::json!({
            "formatVersion": 1,
            "musicAssistantUrl": session.music_assistant_url,
            "username": session.username,
            "userId": session.user_id,
            "accessToken": session.access_token,
        });
        store.save_session(session.clone()).unwrap();
        let serialized = credentials.read().unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialized).unwrap(),
            expected
        );
        credentials.write(&expected.to_string()).unwrap();
        assert_eq!(store.load_session().unwrap(), Some(session));
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
        assert_eq!(
            store
                .configuration
                .get_update_channel(UpdateChannel::Dev)
                .unwrap(),
            UpdateChannel::Dev
        );
        store
            .configuration
            .save_update_channel(UpdateChannel::Beta)
            .unwrap();
        store.save_session(session_config()).unwrap();
        store.clear_session().unwrap();
        assert_eq!(
            store
                .configuration
                .get_update_channel(UpdateChannel::Stable)
                .unwrap(),
            UpdateChannel::Beta
        );
        *credentials.should_fail_write.lock().unwrap() = true;
        assert!(store.save_session(session_config()).is_err());
        assert_eq!(
            store
                .configuration
                .get_update_channel(UpdateChannel::Stable)
                .unwrap(),
            UpdateChannel::Beta
        );
        assert!(credentials.read().unwrap().is_none());
    }

    #[test]
    fn failed_secure_write_rolls_back_configuration_and_preserves_credentials() {
        let directory = TestDirectory::new();
        let session = session_config();
        let credentials = Arc::new(MemoryCredentials::default());
        *credentials.should_fail_write.lock().unwrap() = true;
        let store = store(&directory, &credentials);
        assert!(store.save_session(session.clone()).is_err());
        assert_eq!(store.configuration.load().unwrap(), None);
        assert!(credentials.read().unwrap().is_none());
        *credentials.should_fail_write.lock().unwrap() = false;
        store.save_session(session.clone()).unwrap();
        let original = credentials.read().unwrap();
        *credentials.should_fail_write.lock().unwrap() = true;
        let mut updated = session.clone();
        updated.volume = 10;
        assert!(store.save_session(updated).is_err());
        assert_eq!(credentials.read().unwrap(), original);
        assert_eq!(store.load_session().unwrap(), Some(session));
    }

    #[test]
    fn corrupt_configuration_does_not_destroy_credentials() {
        let directory = TestDirectory::new();
        let credentials = Arc::new(MemoryCredentials::default());
        let store = store(&directory, &credentials);
        store.save_session(session_config()).unwrap();
        let path = directory.0.join("config.json");
        std::fs::write(&path, "broken").unwrap();
        let original = credentials.read().unwrap();
        assert!(store.load_session().is_err());
        assert_eq!(credentials.read().unwrap(), original);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
    }

    #[test]
    fn failed_configuration_write_never_replaces_credentials() {
        let directory = TestDirectory::new();
        std::fs::create_dir_all(&directory.0).unwrap();
        let blocked = directory.0.join("not-a-directory");
        std::fs::write(&blocked, "keep").unwrap();
        let credentials = Arc::new(MemoryCredentials::default());
        credentials
            .write(&serde_json::to_string(&SessionCredentials::from(&session_config())).unwrap())
            .unwrap();
        let original = credentials.read().unwrap();
        let store = SplitSessionStorage::new(
            ConfigurationFile::with_path(blocked.join("config.json")),
            Box::new(credentials.clone()),
        );
        assert!(store.save_session(session_config()).is_err());
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
            serde_json::json!({"formatVersion": 3, "session": session_config()}).to_string(),
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
