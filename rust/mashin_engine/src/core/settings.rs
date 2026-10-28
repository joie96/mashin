//! Application settings persistence; native playback operations retain their own lifecycle.

use super::{CoreCoordinator, CORE_COORDINATOR};

impl CoreCoordinator {
    // Provider preference changes do not reconfigure playback or overwrite other settings.
    pub(crate) fn set_preferred_provider(
        &mut self,
        preferred_provider: Option<crate::core::models::PreferredProvider>,
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
        self.session_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    // Commit all settings together; failed persistence restores the previous playback configuration.
    pub(crate) fn save_session_settings(
        &mut self,
        update: crate::core::models::SessionSettingsUpdate,
    ) -> Result<(), String> {
        if !(25 * 1024 * 1024..=1_000 * 1024 * 1024).contains(&update.local_cache_size_bytes) {
            return Err("local cache size must be between 25 and 1000 MB".to_string());
        }
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required before saving settings".to_string())?;
        session.local.cache_bytes = update.local_cache_size_bytes;
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
        self.playback.set_volume(volume);
        if self.playback.active_mode() == crate::playback::mode::PlaybackMode::Remote {
            return Ok(());
        }
        let mut session = self
            .session_configuration
            .clone()
            .ok_or_else(|| "a restored session is required before changing volume".to_string())?;
        session.volume = volume;
        self.session_store.save_session(session.clone())?;
        self.session_configuration = Some(session);
        Ok(())
    }

    pub(crate) fn set_audio_profile(&mut self, profile: &str) -> Result<(), String> {
        let profile = profile.parse::<crate::playback::local::mashin::models::AudioProfileId>()?;
        let change = self.playback.prepare_audio_profile_change(profile)?;
        self.pending_commands.push(Box::pin(async move {
            change
                .execute(|change| {
                    let mut coordinator = CORE_COORDINATOR
                        .lock()
                        .map_err(|_| "core coordinator lock poisoned")?;
                    let CoreCoordinator {
                        playback,
                        session_store,
                        session_configuration,
                        ..
                    } = &mut *coordinator;
                    playback.commit_audio_profile_change(change, |profile| {
                        let mut configuration = session_configuration
                            .clone()
                            .ok_or("session ended during profile request")?;
                        configuration.local.audio_profile = profile;
                        session_store.save_session(configuration.clone())?;
                        *session_configuration = Some(configuration);
                        Ok(())
                    })
                })
                .await
        }));
        Ok(())
    }
}
