//! Owns the non-secret configuration file; credentials remain in platform secure storage.

use crate::domain::identifiers::UserId;
use crate::domain::session::{AccessToken, MusicAssistantServerUrl};
use crate::domain::settings::{
    LocalOutputConfig, LogLevelPreference, PreferredProvider, SelectedOutput, SessionConfig,
    ThemePreference, UpdateChannel,
};
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
    #[serde(default)]
    session: Option<SessionPreferences>,
    #[serde(default)]
    update_channel: Option<UpdateChannel>,
}

impl Default for ConfigurationDocument {
    fn default() -> Self {
        Self {
            format_version: CONFIGURATION_VERSION,
            session: None,
            update_channel: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ConfigurationFile {
    location: ConfigurationLocation,
}

#[derive(Clone)]
enum ConfigurationLocation {
    #[cfg(test)]
    Fixed(PathBuf),
    Resolver(fn() -> Result<PathBuf, String>),
}

impl crate::application::settings::AppSettingsStorage for ConfigurationFile {
    fn get_update_channel(&self, default_channel: UpdateChannel) -> Result<UpdateChannel, String> {
        ConfigurationFile::get_update_channel(self, default_channel)
    }

    fn save_update_channel(&self, channel: UpdateChannel) -> Result<(), String> {
        ConfigurationFile::save_update_channel(self, channel)
    }
}

impl ConfigurationFile {
    pub(crate) fn new(resolve_path: fn() -> Result<PathBuf, String>) -> Self {
        Self {
            location: ConfigurationLocation::Resolver(resolve_path),
        }
    }

    // Explicit paths isolate tests from the user's installed configuration.
    #[cfg(test)]
    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self {
            location: ConfigurationLocation::Fixed(path),
        }
    }

    // Configuration reads and atomic replacement.
    pub(crate) fn load(&self) -> Result<Option<SessionPreferences>, String> {
        Ok(self.read_document()?.session)
    }

    pub(crate) fn save(&self, preferences: &SessionPreferences) -> Result<(), String> {
        let mut document = self.read_document()?;
        document.session = Some(preferences.clone());
        self.write_document(&document)
    }

    // App preferences survive session replacement, rollback, and logout.
    pub(crate) fn get_update_channel(
        &self,
        default_channel: UpdateChannel,
    ) -> Result<UpdateChannel, String> {
        let mut document = self.read_document()?;
        if let Some(channel) = document.update_channel {
            return Ok(channel);
        }
        document.update_channel = Some(default_channel);
        self.write_document(&document)?;
        Ok(default_channel)
    }

    pub(crate) fn save_update_channel(&self, channel: UpdateChannel) -> Result<(), String> {
        let mut document = self.read_document()?;
        document.update_channel = Some(channel);
        self.write_document(&document)
    }

    // A failed secure-store update restores only session preferences.
    pub(crate) fn restore(&self, previous: Option<&SessionPreferences>) -> Result<(), String> {
        if let Some(previous) = previous {
            return self.save(previous);
        }
        let mut document = self.read_document()?;
        if document.update_channel.is_some() {
            document.session = None;
            return self.write_document(&document);
        }
        match fs::remove_file(self.path()?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("remove uncommitted configuration failed: {error}")),
        }
    }

    // Serialization and replacement preserve unreadable or unsupported documents.
    fn read_document(&self) -> Result<ConfigurationDocument, String> {
        let contents = match fs::read(self.path()?) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConfigurationDocument::default());
            }
            Err(error) => return Err(format!("read configuration failed: {error}")),
        };
        let document: ConfigurationDocument = serde_json::from_slice(&contents)
            .map_err(|_| "configuration is invalid; the file was preserved".to_string())?;
        if document.format_version != CONFIGURATION_VERSION {
            return Err("unsupported configuration version; the file was preserved".into());
        }
        Ok(document)
    }

    fn write_document(&self, document: &ConfigurationDocument) -> Result<(), String> {
        let contents = serde_json::to_vec_pretty(document)
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

    // Platform paths are resolved lazily after Android supplies its application context.
    fn path(&self) -> Result<PathBuf, String> {
        match &self.location {
            #[cfg(test)]
            ConfigurationLocation::Fixed(path) => Ok(path.clone()),
            ConfigurationLocation::Resolver(resolve_path) => resolve_path(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn theme_preferences_survive_configuration_reopen() {
        let directory = TestDirectory::new();
        let path = directory.0.join("configuration.json");
        let mut session = session_config();
        for (theme, wire_value) in [
            (ThemePreference::Light, "light"),
            (ThemePreference::Dark, "dark"),
            (ThemePreference::System, "system"),
        ] {
            let update: crate::domain::settings::SessionSettingsUpdate =
                serde_json::from_value(serde_json::json!({
                    "theme": wire_value,
                    "logLevel": "info",
                    "shouldShowMultiDeviceButton": false,
                }))
                .unwrap();
            assert_eq!(update.theme, theme);
            session.theme = update.theme;
            let preferences = SessionPreferences::from(&session);
            ConfigurationFile::with_path(path.clone())
                .save(&preferences)
                .unwrap();
            let restored = ConfigurationFile::with_path(path.clone())
                .load()
                .unwrap()
                .unwrap();
            assert_eq!(restored, preferences);
            assert_eq!(
                serde_json::to_value(&restored).unwrap()["theme"],
                wire_value
            );
            assert_eq!(restored.into_session(None).theme, theme);
        }
    }

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
            local: crate::domain::settings::default_local_output("Mashin (Test)".into()),
            volume: 70,
            theme: ThemePreference::Light,
            log_level: LogLevelPreference::Warn,
            should_show_multi_device_button: false,
            preferred_provider: None,
        }
    }
}
