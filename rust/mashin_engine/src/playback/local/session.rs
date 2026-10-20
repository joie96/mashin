//! The authenticated native session owns its receiver independently of output selection.

use super::mashin::audio_adapter::MashinAudioAdapter;
use super::mashin::receiver::{MashinReceiver, ReceiverConfiguration};
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::models::identifiers::{QueueId, QueueItemId};
use crate::playback::models::MediaSessionState;
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
                let selection = snapshot.queue.current_index
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
        if let Some(index) = state.queue.items.iter().position(|item| item.queue_item_id == selection.item_id) {
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
    pub(super) queue_bootstrap: Mutex<QueueBootstrap>,
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
            queue_bootstrap: Mutex::new(QueueBootstrap::default()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_assistant::models::queue::PlayState;
    use crate::playback::models::Queue;

    fn server_snapshot() -> MediaSessionState {
        let item = serde_json::from_value(serde_json::json!({
            "queue_id":"registered-player", "queue_item_id":"saved-item", "name":"Saved",
            "duration":180, "media_item":{"item_id":"track", "provider":"test", "name":"Saved", "media_type":"track"}
        })).unwrap();
        MediaSessionState {
            queue: Queue {
                queue_id: QueueId("registered-player".into()),
                items: vec![item],
                current_index: Some(0),
                shuffle_enabled: true,
                autoplay_enabled: true,
                ..Queue::default()
            },
            player_state: crate::playback::models::PlayerState {
                playback_state: PlayState::Playing,
                position_millis: Some(12_500),
                ..Default::default()
            },
        }
    }

    fn local_state(snapshot: &MediaSessionState) -> MediaSessionState {
        let mut queue = snapshot.queue.clone();
        queue.current_index = None;
        MediaSessionState { queue, player_state: Default::default() }
    }

    #[test]
    fn bootstrap_waits_for_registration_and_a_complete_snapshot() {
        let snapshot = server_snapshot();
        let mut bootstrap = QueueBootstrap::default();
        let mut local = local_state(&snapshot);
        bootstrap.project(&mut local, Some(&snapshot), false);
        assert_eq!(local.queue.current_index, None);
        bootstrap.start_loading();
        bootstrap.project(&mut local, None, false);
        assert!(matches!(bootstrap, QueueBootstrap::Loading));
        bootstrap.project(&mut local, Some(&snapshot), false);
        assert_eq!(local.queue.current_index, Some(0));
        assert_eq!(local.player_state.position_millis, Some(12_500));
        assert_eq!(local.player_state.duration_millis, Some(180_000));
        assert_ne!(local.player_state.playback_state, PlayState::Playing);
        assert!(local.queue.shuffle_enabled && local.queue.autoplay_enabled);
    }

    #[test]
    fn reconnect_does_not_reimport_server_selection_or_position() {
        let mut snapshot = server_snapshot();
        let mut bootstrap = QueueBootstrap::default();
        bootstrap.start_loading();
        bootstrap.project(&mut local_state(&snapshot), Some(&snapshot), false);
        snapshot.queue.current_index = None;
        snapshot.player_state.position_millis = Some(99_000);
        bootstrap.start_loading();
        let mut local = local_state(&snapshot);
        bootstrap.project(&mut local, Some(&snapshot), false);
        assert_eq!(local.queue.current_index, Some(0));
        assert_eq!(local.player_state.position_millis, Some(12_500));
    }

    #[test]
    fn local_activity_rejects_a_late_initial_snapshot_permanently() {
        let snapshot = server_snapshot();
        let mut bootstrap = QueueBootstrap::default();
        bootstrap.start_loading();
        let mut local = local_state(&snapshot);
        bootstrap.project(&mut local, Some(&snapshot), true);
        bootstrap.start_loading();
        bootstrap.project(&mut local, Some(&snapshot), false);
        assert!(matches!(bootstrap, QueueBootstrap::Local));
        assert_eq!(local.queue.current_index, None);
        assert_eq!(local.player_state.position_millis, None);
    }

    #[test]
    fn queue_mutation_keeps_local_selection_and_position_after_reconnect() {
        let snapshot = server_snapshot();
        let mut bootstrap = QueueBootstrap::default();
        bootstrap.start_loading();
        let mut local = local_state(&snapshot);
        bootstrap.project(&mut local, Some(&snapshot), false);
        bootstrap.take_local_authority();
        local.queue.current_index = None;
        local.player_state.current_index = None;
        local.player_state.current_queue_item_id = Some(QueueItemId("local-stream".into()));
        local.player_state.position_millis = Some(42_000);
        let expected = local.clone();
        bootstrap.project(&mut local, None, false);
        bootstrap.start_loading();
        bootstrap.project(&mut local, Some(&snapshot), false);
        assert!(matches!(bootstrap, QueueBootstrap::Local));
        assert_eq!(local, expected);
    }

    #[test]
    fn an_empty_initial_queue_completes_bootstrap() {
        let mut bootstrap = QueueBootstrap::default();
        bootstrap.start_loading();
        let empty = MediaSessionState { queue: Queue::default(), player_state: Default::default() };
        bootstrap.project(&mut local_state(&empty), Some(&empty), false);
        assert!(matches!(bootstrap, QueueBootstrap::Complete(None)));
        let snapshot = server_snapshot();
        let mut local = local_state(&snapshot);
        bootstrap.project(&mut local, Some(&snapshot), false);
        assert_eq!(local.queue.current_index, None);
    }
}
