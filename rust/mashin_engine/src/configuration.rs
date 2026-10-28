//! Owns the non-secret configuration file; credentials remain in platform secure storage.

use crate::core::models::{
    LocalOutputConfig, LogLevelPreference, PreferredProvider, SelectedOutput, SessionConfig,
    ThemePreference,
};
use crate::music_assistant::models::identifiers::{AccessToken, UserId};
use crate::music_assistant::models::session::MusicAssistantServerUrl;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

const CONFIGURATION_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionPreferences {
    pub(crate) music_assistant_url: MusicAssistantServerUrl,
    pub(crate) user_id: Option<UserId>,
    pub(crate) username: String,
    pub(crate) selected_output: SelectedOutput,
    pub(crate) local: LocalOutputConfig,
    pub(crate) volume: u8,
    pub(crate) theme: ThemePreference,
    pub(crate) log_level: LogLevelPreference,
    pub(crate) should_show_multi_device_button: bool,
    pub(crate) preferred_provider: Option<PreferredProvider>,
}

impl SessionPreferences {
    // Secure storage supplies the token only when restoring a matching identity.
    pub(crate) fn into_session(self, access_token: Option<AccessToken>) -> SessionConfig {
        SessionConfig {
            music_assistant_url: self.music_assistant_url,
            access_token,
            user_id: self.user_id,
            username: self.username,
            selected_output: self.selected_output,
            local: self.local,
            volume: self.volume,
            theme: self.theme,
            log_level: self.log_level,
            should_show_multi_device_button: self.should_show_multi_device_button,
            preferred_provider: self.preferred_provider,
        }
    }
}

impl From<&SessionConfig> for SessionPreferences {
    fn from(session: &SessionConfig) -> Self {
        Self {
            music_assistant_url: session.music_assistant_url.clone(),
            user_id: session.user_id.clone(),
            username: session.username.clone(),
            selected_output: session.selected_output.clone(),
            local: session.local.clone(),
            volume: session.volume,
            theme: session.theme,
            log_level: session.log_level,
            should_show_multi_device_button: session.should_show_multi_device_button,
            preferred_provider: session.preferred_provider.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigurationDocument {
    format_version: u8,
    session: SessionPreferences,
}

#[derive(Default)]
pub(crate) struct ConfigurationFile {
    path: Option<PathBuf>,
}

impl ConfigurationFile {
    // Explicit paths isolate tests from the user's installed configuration.
    #[cfg(test)]
    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    // Configuration reads and atomic replacement.
    pub(crate) fn load(&self) -> Result<Option<SessionPreferences>, String> {
        let contents = match fs::read(self.path()?) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read configuration failed: {error}")),
        };
        let document: ConfigurationDocument = serde_json::from_slice(&contents)
            .map_err(|_| "configuration is invalid; the file was preserved".to_string())?;
        if document.format_version != CONFIGURATION_VERSION {
            return Err("unsupported configuration version; the file was preserved".into());
        }
        Ok(Some(document.session))
    }

    pub(crate) fn save(&self, preferences: &SessionPreferences) -> Result<(), String> {
        let contents = serde_json::to_vec_pretty(&ConfigurationDocument {
            format_version: CONFIGURATION_VERSION,
            session: preferences.clone(),
        })
        .map_err(|_| "encode configuration failed".to_string())?;
        let path = self.path()?;
        let directory = path.parent().ok_or("configuration path has no parent")?;
        fs::create_dir_all(directory)
            .map_err(|error| format!("create configuration directory failed: {error}"))?;
        let temporary_path = directory.join(format!(".config-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary_path)
                .map_err(|error| format!("create configuration replacement failed: {error}"))?;
            file.write_all(&contents)
                .and_then(|_| file.sync_all())
                .map_err(|error| format!("write configuration replacement failed: {error}"))?;
            drop(file);
            fs::rename(&temporary_path, &path)
                .map_err(|error| format!("replace configuration failed: {error}"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary_path);
        }
        result
    }

    // A failed secure-store update restores the previous non-secret configuration.
    pub(crate) fn restore(&self, previous: Option<&SessionPreferences>) -> Result<(), String> {
        if let Some(previous) = previous {
            return self.save(previous);
        }
        match fs::remove_file(self.path()?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("remove uncommitted configuration failed: {error}")),
        }
    }

    // Platform paths are resolved lazily after Android supplies its application context.
    fn path(&self) -> Result<PathBuf, String> {
        match &self.path {
            Some(path) => Ok(path.clone()),
            None => Ok(application_data_directory()?.join("config.json")),
        }
    }
}

// App-owned data roots; executable installation and log rotation never own config.json.
#[cfg(target_os = "windows")]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("Mashin"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_string())
}

#[cfg(target_os = "android")]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    crate::jni::android_context::external_application_files_directory()
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(|path| PathBuf::from(path).join("Library/Application Support/Mashin"))
        .ok_or_else(|| "HOME is not set".to_string())
}

#[cfg(target_os = "linux")]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/state")))
        .map(|path| path.join("mashin"))
        .ok_or_else(|| "neither XDG_STATE_HOME nor HOME is set".to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) struct TestDirectory(pub(crate) PathBuf);

    impl TestDirectory {
        pub(crate) fn new() -> Self {
            Self(std::env::temp_dir().join(format!("mashin-config-test-{}", uuid::Uuid::new_v4())))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) fn session_config() -> SessionConfig {
        SessionConfig {
            music_assistant_url: MusicAssistantServerUrl("https://music-assistant.example".into()),
            access_token: Some(AccessToken("secret-token".into())),
            user_id: None,
            username: "mashin".into(),
            selected_output: SelectedOutput::Local,
            local: crate::core::config::default_local_output(),
            volume: 70,
            theme: ThemePreference::Light,
            log_level: LogLevelPreference::Warn,
            should_show_multi_device_button: false,
            preferred_provider: None,
        }
    }

    #[test]
    fn configuration_round_trips_without_credentials() {
        let directory = TestDirectory::new();
        let path = directory.0.join("config.json");
        let store = ConfigurationFile::with_path(path.clone());
        assert_eq!(store.load().unwrap(), None);
        let session = session_config();
        let preferences = SessionPreferences::from(&session);
        store.save(&preferences).unwrap();
        let contents = fs::read_to_string(path).unwrap();
        assert!(!contents.contains("secret-token"));
        assert!(!contents.contains("accessToken"));
        assert_eq!(store.load().unwrap(), Some(preferences));
        assert_eq!(
            store
                .load()
                .unwrap()
                .unwrap()
                .into_session(session.access_token.clone()),
            session
        );
    }

    #[test]
    fn configuration_replaces_existing_file_and_can_roll_back() {
        let directory = TestDirectory::new();
        let store = ConfigurationFile::with_path(directory.0.join("config.json"));
        let previous = SessionPreferences::from(&session_config());
        store.save(&previous).unwrap();
        let mut updated = previous.clone();
        updated.volume = 25;
        store.save(&updated).unwrap();
        assert_eq!(store.load().unwrap(), Some(updated));
        store.restore(Some(&previous)).unwrap();
        assert_eq!(store.load().unwrap(), Some(previous));
        store.restore(None).unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn corrupt_and_newer_configuration_files_are_preserved() {
        let directory = TestDirectory::new();
        fs::create_dir_all(&directory.0).unwrap();
        let path = directory.0.join("config.json");
        let store = ConfigurationFile::with_path(path.clone());
        fs::write(&path, "invalid JSON").unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "invalid JSON");
        let document = ConfigurationDocument {
            format_version: 255,
            session: SessionPreferences::from(&session_config()),
        };
        let contents = serde_json::to_vec(&document).unwrap();
        fs::write(&path, &contents).unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read(path).unwrap(), contents);
    }

    #[test]
    fn failed_replacement_keeps_the_existing_configuration() {
        let directory = TestDirectory::new();
        let store = ConfigurationFile::with_path(directory.0.join("config.json"));
        let preferences = SessionPreferences::from(&session_config());
        store.save(&preferences).unwrap();
        let blocked = ConfigurationFile::with_path(directory.0.join("config.json/child.json"));
        assert!(blocked.save(&preferences).is_err());
        assert_eq!(store.load().unwrap(), Some(preferences));
    }
}
