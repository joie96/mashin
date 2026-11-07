//! Remote mode projects and controls an MA target without local audio resources.

use crate::application::events::{ApplicationEvent, EventPublisher};
use crate::domain::identifiers::QueueItemId;
use crate::domain::media::MediaItem;
use crate::playback::backend::{PlaybackBackend, PlaybackCommand};
use crate::playback::models::{PlayerState, Queue};
use crate::playback::models::{QueueOption, RepeatMode};
use crate::playback::queue_projection::QueueSnapshotRequest;
use crate::playback::server_target::ServerTarget;
use crate::playback::start::{PlaybackStart, StartIdentity, StartObservation};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) struct RemotePlayback {
    event_publisher: EventPublisher,
    target: ServerTarget,
    last_queue: Option<Queue>,
    last_queue_revision: Option<(QueueSnapshotRequest, bool)>,
    last_player: Option<PlayerState>,
    start: Arc<Mutex<PlaybackStart>>,
}

impl RemotePlayback {
    // Construction and target configuration.
    pub(crate) fn configure(
        &mut self,
        configuration: crate::playback::server_target::ServerTargetConfiguration,
    ) -> Result<(), String> {
        self.shutdown();
        self.target.configure(configuration)
    }

    pub(crate) fn new(event_publisher: EventPublisher) -> Self {
        Self {
            event_publisher,
            target: ServerTarget::new(),
            last_queue: None,
            last_queue_revision: None,
            last_player: None,
            start: Arc::new(Mutex::new(PlaybackStart::default())),
        }
    }
}

impl PlaybackBackend for RemotePlayback {
    // Remote presentation waits for authoritative MA playback observations.
    fn start_presentation(&self) -> Arc<Mutex<PlaybackStart>> {
        self.start.clone()
    }

    fn start_observation(&self) -> StartObservation {
        let player = self.target.player_state();
        StartObservation {
            identity: StartIdentity::Remote(player.current_queue_item_id),
            revision: 0,
            state: player.playback_state,
        }
    }

    // Configuration and lifecycle.
    fn activate(&mut self) -> Result<(), String> {
        self.target.activate()
    }
    fn deactivate(&mut self) {
        self.start
            .lock()
            .expect("playback start lock poisoned")
            .cancel();
        self.target.deactivate();
        self.last_queue = None;
        self.last_queue_revision = None;
        self.last_player = None;
    }
    fn shutdown(&mut self) {
        self.deactivate();
        self.target.shutdown();
    }

    fn take_retired_workers(&mut self) -> Vec<tokio::task::JoinHandle<()>> {
        self.target.take_retired_workers()
    }

    // Queue commands.
    fn queue_play_media(
        &mut self,
        media: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    ) -> Result<(), String> {
        self.target
            .queue_play_media(media, option, start_item, shuffle)
    }
    fn move_queue_item(&mut self, item_id: QueueItemId, position_shift: i32) -> Result<(), String> {
        self.target.move_queue_item(item_id, position_shift)
    }
    fn move_queue_item_to_end(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.target.move_queue_item_to_end(item_id)
    }
    fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String> {
        self.target.delete_queue_items(item_ids)
    }
    fn select_queue_item(&mut self, id: QueueItemId) -> Result<(), String> {
        self.target.select_queue_item(id)
    }
    fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.target.play_queue_index(index)
    }

    // Transport and preferences.
    fn transport_play(&mut self) -> Result<(), String> {
        self.target.transport_play()
    }
    fn transport_pause(&mut self) -> Result<(), String> {
        self.target.transport_pause()
    }
    fn transport_stop(&mut self) {
        self.target.transport_stop();
    }
    fn transport_next(&mut self) -> Result<(), String> {
        self.target.transport_next()
    }
    fn transport_previous(&mut self) -> Result<(), String> {
        self.target.transport_previous()
    }
    fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        self.target.transport_seek(seconds)
    }
    fn set_volume(&mut self, volume: u8) {
        self.target.set_volume(volume);
    }
    fn set_muted(&mut self, muted: bool) {
        self.target.set_muted(muted);
    }
    fn set_shuffle(&mut self, enabled: bool) {
        self.target.set_shuffle(enabled);
    }
    fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.target.set_repeat_mode(mode);
    }
    fn set_autoplay(&mut self, enabled: bool) {
        self.target.set_autoplay(enabled);
    }

    // Snapshots and command completion.
    fn queue(&self) -> Queue {
        self.target.queue()
    }
    fn player_state(&self) -> PlayerState {
        let mut player = self.target.player_state();
        player.playback_state = self
            .start
            .lock()
            .expect("playback start lock poisoned")
            .project(self.start_observation(), Instant::now());
        player
    }
    fn take_pending_commands(&mut self) -> Vec<PlaybackCommand> {
        self.target.take_pending_commands()
    }

    // The facade polls the selected mode independently of Flutter widgets.
    fn collect_presentation_events(&mut self) -> crate::playback::backend::PresentationEvents {
        let mut events = Vec::new();
        let revision = self.target.queue_revision();
        if self.last_queue_revision != revision {
            self.last_queue_revision = revision;
            let queue = self.target.queue();
            if self.last_queue.as_ref() != Some(&queue) {
                self.last_queue = Some(queue.clone());
                events.push(ApplicationEvent::QueueChanged { queue });
            }
        }
        let player = self.player_state();
        if self.last_player.as_ref() != Some(&player) {
            self.last_player = Some(player.clone());
            events.push(ApplicationEvent::PlayerStateChanged {
                player_state: player,
            });
        }
        crate::playback::backend::PresentationEvents {
            publisher: self.event_publisher.clone(),
            events,
        }
    }
}
