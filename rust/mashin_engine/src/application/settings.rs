//! Application settings persistence; native playback operations retain their own lifecycle.

use crate::application::errors::ApplicationError;
use crate::application::state::ApplicationState;
use crate::application::Application;
use crate::domain::settings::UpdateChannel;

pub(crate) trait AppSettingsStorage: Send + Sync {
    fn get_update_channel(&self, default_channel: UpdateChannel) -> Result<UpdateChannel, String>;
    fn save_update_channel(&self, channel: UpdateChannel) -> Result<(), String>;
}

impl ApplicationState {
    // Provider preference changes do not reconfigure playback or overwrite other settings.
    pub(crate) fn set_preferred_provider(
        &mut self,
        preferred_provider: Option<crate::domain::settings::PreferredProvider>,
    ) -> Result<(), String> {
        if preferred_provider.as_ref().is_some_and(|preferred| {
            preferred.provider_instance.trim().is_empty()
                || preferred.provider_domain.trim().is_empty()
                || preferred.provider_instance == "library"
                || preferred.provider_domain == "library"
        }) {
            return Err("preferred provider must identify a music provider".to_string());
        }
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required before saving settings".to_string())?;
        session.preferred_provider = preferred_provider;
        self.session_storage.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    // Commit all settings together; failed persistence restores the previous playback configuration.
    pub(crate) fn save_session_settings(
        &mut self,
        update: crate::domain::settings::SessionSettingsUpdate,
    ) -> Result<(), String> {
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required before saving settings".to_string())?;
        session.theme = update.theme;
        session.log_level = update.log_level;
        session.should_show_multi_device_button = update.should_show_multi_device_button;
        self.configure_and_persist_session(&session)?;
        crate::logging::set_minimum_level(session.log_level.as_str())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    // Persisted playback preferences; transport-only commands bypass application settings.
    pub(crate) fn set_volume(&mut self, volume: u8) -> Result<(), String> {
        if volume > 100 {
            return Err("volume must be between 0 and 100".into());
        }
        if self.playback.active_mode() == crate::playback::mode::PlaybackMode::Remote {
            self.playback.set_volume(volume);
            return Ok(());
        }
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required before changing volume".to_string())?;
        session.volume = volume;
        self.session_storage.save_session(session.clone())?;
        self.session_configuration = Some(session);
        self.playback.set_volume(volume);
        Ok(())
    }

    pub(crate) fn set_audio_profile(
        &mut self,
        profile: &str,
        owner: std::sync::Weak<std::sync::Mutex<ApplicationState>>,
    ) -> Result<(), String> {
        let profile = profile.parse::<crate::domain::settings::AudioProfileId>()?;
        let change = self.playback.prepare_audio_profile_change(profile)?;
        self.pending_commands.push(Box::pin(async move {
            change
                .execute(|change| {
                    let owner = owner
                        .upgrade()
                        .ok_or("application ended during profile request")?;
                    let mut coordinator =
                        owner.lock().map_err(|_| "core coordinator lock poisoned")?;
                    let ApplicationState {
                        playback,
                        session_storage,
                        session_configuration,
                        ..
                    } = &mut *coordinator;
                    playback.commit_audio_profile_change(change, |profile| {
                        let mut configuration = session_configuration
                            .clone()
                            .ok_or("session ended during profile request")?;
                        configuration.local.audio_profile = profile;
                        session_storage.save_session(configuration.clone())?;
                        *session_configuration = Some(configuration);
                        Ok(())
                    })
                })
                .await
        }));
        Ok(())
    }
}

impl Application {
    // App preferences share the coordinator lock with session configuration writes.
    pub(crate) fn get_update_channel(
        &self,
        default_channel: UpdateChannel,
    ) -> Result<UpdateChannel, ApplicationError> {
        let _coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        self.environment
            .app_settings
            .get_update_channel(default_channel)
            .map_err(ApplicationError::command_failed)
    }

    pub(crate) fn save_update_channel(
        &self,
        channel: UpdateChannel,
    ) -> Result<(), ApplicationError> {
        let _coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        self.environment
            .app_settings
            .save_update_channel(channel)
            .map_err(ApplicationError::command_failed)
    }
}
