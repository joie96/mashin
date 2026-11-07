//! The authenticated native session owns its receiver independently of output selection.

use super::mashin::audio_adapter::MashinAudioAdapter;
use super::mashin::receiver::{MashinReceiver, ReceiverConfiguration};
use crate::domain::identifiers::{QueueId, QueueItemId};
use crate::playback::models::MediaSessionState;
use crate::playback::server::PlaybackConnection;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(super) enum QueueBootstrap {
    #[default]
    Pending,
    Loading,
    Complete(Option<InitialQueueSelection>),
    Local,
}

pub(super) struct InitialQueueSelection {
    queue_id: QueueId,
    item_id: QueueItemId,
    position_millis: Option<i64>,
    duration_millis: Option<i64>,
}

impl QueueBootstrap {
    // Registration starts loading once; reconnects never reset session authority.
    pub(super) fn start_loading(&mut self) {
        if matches!(self, Self::Pending) {
            *self = Self::Loading;
        }
    }

    pub(super) fn take_local_authority(&mut self) {
        *self = Self::Local;
    }

    // Retain only the initial selection, not a second copy of the server-owned queue.
    pub(super) fn project(
        &mut self,
        state: &mut MediaSessionState,
        synchronized: Option<&MediaSessionState>,
        has_local_activity: bool,
    ) {
        if has_local_activity {
            self.take_local_authority();
        }
        if matches!(self, Self::Loading) {
            if let Some(snapshot) = synchronized {
                let selection = snapshot
                    .queue
                    .current_index
                    .and_then(|index| snapshot.queue.items.get(index))
                    .map(|item| InitialQueueSelection {
                        queue_id: snapshot.queue.queue_id.clone(),
                        item_id: item.queue_item_id.clone(),
                        position_millis: snapshot.player_state.position_millis,
                        duration_millis: item.duration.map(|seconds| i64::from(seconds) * 1000),
                    });
                *self = Self::Complete(selection);
            }
        }
        let Self::Complete(Some(selection)) = self else {
            return;
        };
        if state.queue.queue_id != selection.queue_id {
            return;
        }
        if let Some(index) = state
            .queue
            .items
            .iter()
            .position(|item| item.queue_item_id == selection.item_id)
        {
            state.queue.current_index = Some(index);
            state.player_state.current_index = Some(index);
            state.player_state.current_queue_item_id = Some(selection.item_id.clone());
            state.player_state.position_millis = selection.position_millis;
            state.player_state.duration_millis = selection.duration_millis;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalSessionConfiguration {
    pub(crate) device_id: String,
    pub(crate) client_name: String,
    pub(crate) cache_bytes: usize,
    pub(crate) audio_profile: crate::domain::settings::AudioProfileId,
    pub(crate) volume: u8,
}

pub(crate) struct AudioProfileChange {
    session: Arc<LocalSession>,
    profile: crate::domain::settings::AudioProfileId,
    previous_profile: crate::domain::settings::AudioProfileId,
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
        persist: impl FnOnce(crate::domain::settings::AudioProfileId) -> Result<(), String>,
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
    runtime: tokio::runtime::Handle,
    pub(super) output: MashinAudioAdapter,
    pub(super) receiver: MashinReceiver,
    pub(super) connection: Arc<dyn PlaybackConnection>,
    pub(super) queue_bootstrap: Mutex<QueueBootstrap>,
    configuration: Mutex<LocalSessionConfiguration>,
}

impl LocalSession {
    // The API token belongs only to MA API/EventHub, never to receiver sockets.
    pub(crate) fn new(
        runtime: &tokio::runtime::Handle,
        configuration: &LocalSessionConfiguration,
        connection: Arc<dyn PlaybackConnection>,
    ) -> Result<Self, String> {
        let output =
            MashinAudioAdapter::new(configuration.cache_bytes, configuration.volume, false)?;
        let music_assistant_url = connection.server_url();
        let receiver = MashinReceiver::new(
            runtime,
            ReceiverConfiguration {
                music_assistant_url: music_assistant_url.clone(),
                device_id: configuration.device_id.clone(),
                client_name: configuration.client_name.clone(),
                audio_profile: configuration.audio_profile,
            },
            output.clone(),
            connection.client(),
        )?;
        Ok(Self {
            runtime: runtime.clone(),
            output,
            receiver,
            connection,
            queue_bootstrap: Mutex::new(QueueBootstrap::default()),
            configuration: Mutex::new(configuration.clone()),
        })
    }

    // Join receiver/recovery before audio on the blocking executor, independent of retained Arcs.
    pub(crate) fn shutdown(session: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let runtime = session.runtime.clone();
        let worker = session.receiver.shutdown();
        runtime.clone().spawn_blocking(move || {
            let result = worker.map(|worker| runtime.block_on(worker));
            let audio_result = session.output.shutdown();
            drop(session);
            if let Some(Err(error)) = result {
                if error.is_panic() {
                    std::panic::resume_unwind(error.into_panic());
                }
            }
            if let Err(error) = audio_result {
                std::panic::resume_unwind(error);
            }
        })
    }

    // The operation retains its receiver through mode changes and captures live rollback state.
    pub(crate) fn prepare_audio_profile_change(
        session: &Arc<Self>,
        profile: crate::domain::settings::AudioProfileId,
    ) -> AudioProfileChange {
        AudioProfileChange {
            session: session.clone(),
            profile,
            previous_profile: session.receiver.snapshot().audio_profile,
        }
    }

    // Runtime capability and committed session identity.
    pub(crate) fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }

    // Server-negotiated profiles and persisted volume must not recreate the user's receiver.
    pub(super) fn matches(
        &self,
        configuration: &LocalSessionConfiguration,
        connection: &Arc<dyn PlaybackConnection>,
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
