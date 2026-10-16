//! The authenticated native session owns its receiver independently of output selection.

use super::mashin::audio_adapter::MashinAudioAdapter;
use super::mashin::receiver::{MashinReceiver, ReceiverConfiguration};
use crate::music_assistant::connection::MusicAssistantConnection;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalSessionConfiguration {
    pub(crate) device_id: String,
    pub(crate) client_name: String,
    pub(crate) cache_bytes: usize,
    pub(crate) audio_profile: super::mashin::models::AudioProfileId,
    pub(crate) volume: u8,
}

pub(crate) struct AudioProfileChange {
    session: Arc<LocalSession>,
    profile: super::mashin::models::AudioProfileId,
    previous_profile: super::mashin::models::AudioProfileId,
}

impl AudioProfileChange {
    // Native changes and compensation remain on the original receiver, outside application locks.
    pub(crate) async fn execute(
        self,
        commit: impl FnOnce(&Self) -> Result<(), String>,
    ) -> Result<(), String> {
        self.session
            .receiver
            .set_audio_profile(self.profile)
            .await?;
        if let Err(error) = commit(&self) {
            self.session
                .receiver
                .set_audio_profile(self.previous_profile)
                .await
                .map_err(|rollback| format!("{error}; profile rollback failed: {rollback}"))?;
            return Err(error);
        }
        Ok(())
    }

    // Persisted preference and native session identity commit together, not negotiated server state.
    pub(crate) fn commit(
        &self,
        persist: impl FnOnce(super::mashin::models::AudioProfileId) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut configuration = self
            .session
            .configuration
            .lock()
            .map_err(|_| "local session configuration lock poisoned")?;
        persist(self.profile)?;
        configuration.audio_profile = self.profile;
        Ok(())
    }

    pub(crate) fn belongs_to(&self, session: &Arc<LocalSession>) -> bool {
        Arc::ptr_eq(&self.session, session)
    }
}

pub(crate) struct LocalSession {
    pub(super) output: MashinAudioAdapter,
    pub(super) receiver: MashinReceiver,
    pub(super) connection: Arc<MusicAssistantConnection>,
    configuration: Mutex<LocalSessionConfiguration>,
}

impl LocalSession {
    // The API token belongs only to MA API/EventHub, never to receiver sockets.
    pub(crate) fn new(
        runtime: &tokio::runtime::Runtime,
        configuration: &LocalSessionConfiguration,
        connection: Arc<MusicAssistantConnection>,
    ) -> Result<Self, String> {
        let output =
            MashinAudioAdapter::new(configuration.cache_bytes, configuration.volume, false)?;
        let music_assistant_url = connection.server_url.0.clone();
        let receiver = MashinReceiver::new(
            runtime,
            ReceiverConfiguration {
                music_assistant_url: music_assistant_url.clone(),
                device_id: configuration.device_id.clone(),
                client_name: configuration.client_name.clone(),
                audio_profile: configuration.audio_profile,
            },
            output.clone(),
        )?;
        Ok(Self {
            output,
            receiver,
            connection,
            configuration: Mutex::new(configuration.clone()),
        })
    }

    // The operation retains its receiver through mode changes and captures live rollback state.
    pub(crate) fn prepare_audio_profile_change(
        session: &Arc<Self>,
        profile: super::mashin::models::AudioProfileId,
    ) -> AudioProfileChange {
        AudioProfileChange {
            session: session.clone(),
            profile,
            previous_profile: session.receiver.snapshot().audio_profile,
        }
    }

    // Server-negotiated profiles and persisted volume must not recreate the user's receiver.
    pub(super) fn matches(
        &self,
        configuration: &LocalSessionConfiguration,
        connection: &Arc<MusicAssistantConnection>,
    ) -> Result<bool, String> {
        let mut current = self
            .configuration
            .lock()
            .map_err(|_| "local session configuration lock poisoned")?
            .clone();
        current.volume = configuration.volume;
        Ok(current == *configuration && Arc::ptr_eq(&self.connection, connection))
    }
}
