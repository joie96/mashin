//! Shared player commands and server-owned queue projection; never owns local audio.

use crate::core::core::CORE_NETWORK_RUNTIME;
use crate::music_assistant::client::{PlayerCommand, QueueCommand};
use crate::music_assistant::connection::MusicAssistantConnection;
use crate::music_assistant::event_hub::EventHubLeaseReason;
use crate::music_assistant::models::identifiers::{PlayerId, QueueItemId};
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{QueueOption, RepeatMode};
use crate::music_assistant::queue_controller::MusicAssistantQueueController;
use crate::music_assistant::queue_projection::QueueSnapshotRequest;
use crate::playback::backend::PlaybackCommand;
use crate::playback::mode::PlaybackMode;
use crate::playback::models::{MediaSessionState, PlayerState, Queue};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct MusicAssistantPlaybackTargetConfiguration {
    pub(crate) player_id: PlayerId,
    pub(crate) connection: Arc<MusicAssistantConnection>,
}

pub(crate) struct MusicAssistantPlaybackTarget {
    configuration: Option<MusicAssistantPlaybackTargetConfiguration>,
    projection: Option<MusicAssistantQueueController>,
    pending_commands: Vec<PlaybackCommand>,
}

impl MusicAssistantPlaybackTarget {
    // Construction and configuration.
    pub(crate) fn new() -> Self {
        Self {
            configuration: None,
            projection: None,
            pending_commands: Vec::new(),
        }
    }

    pub(crate) fn configure(
        &mut self,
        configuration: MusicAssistantPlaybackTargetConfiguration,
    ) -> Result<(), String> {
        self.shutdown();
        self.configuration = Some(configuration);
        Ok(())
    }

    // Projection lifecycle and EventHub lease ownership.
    pub(crate) fn activate(&mut self) -> Result<(), String> {
        if self.projection.is_some() {
            return Ok(());
        }
        let configuration = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?;
        let mut hub = configuration
            .connection
            .event_hub
            .lock()
            .map_err(|_| "MA event hub lock poisoned")?;
        let events = hub.subscribe();
        let _runtime = CORE_NETWORK_RUNTIME.enter();
        hub.acquire(EventHubLeaseReason::Playback);
        self.projection = Some(MusicAssistantQueueController::new(
            &CORE_NETWORK_RUNTIME,
            configuration.connection.client.clone(),
            events,
            configuration.player_id.clone(),
            hub.is_connected(),
        ));
        Ok(())
    }

    pub(crate) fn deactivate(&mut self) {
        if self.projection.take().is_some() {
            if let Some(configuration) = &self.configuration {
                if let Ok(mut hub) = configuration.connection.event_hub.lock() {
                    hub.release(EventHubLeaseReason::Playback);
                }
            }
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.deactivate();
        self.configuration = None;
        self.pending_commands.clear();
    }

    // MA queue commands.
    pub(crate) fn queue_play_media(
        &mut self,
        media: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    ) -> Result<(), String> {
        if media.is_empty() && option == QueueOption::Replace {
            return self.queue_command(QueueCommand::Clear);
        }
        self.queue_command(QueueCommand::PlayMedia {
            media,
            option,
            start_item,
            shuffle,
        })
    }

    pub(crate) fn move_queue_item(
        &mut self,
        item_id: QueueItemId,
        position_shift: i32,
    ) -> Result<(), String> {
        self.queue_command(QueueCommand::MoveItem {
            item_id,
            shift: position_shift.into(),
        })
    }

    pub(crate) fn move_queue_item_to_end(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.queue_command(QueueCommand::MoveItemToEnd(item_id))
    }

    pub(crate) fn delete_queue_items(&mut self, item_ids: Vec<QueueItemId>) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        let item_ids = item_ids
            .into_iter()
            .filter(|item_id| seen.insert(item_id.clone()))
            .collect::<Vec<_>>();
        if item_ids.is_empty()
            || item_ids
                .iter()
                .any(|item_id| item_id.as_str().trim().is_empty())
        {
            return Err("queue item IDs must not be empty".into());
        }
        let projection = self
            .projection
            .as_ref()
            .ok_or("remote queue is not active")?;
        let client = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?
            .connection
            .client
            .clone();
        let mutation = projection.begin_mutation()?;
        self.pending_commands.push(Box::pin(async move {
            let total = item_ids.len();
            for (completed, item_id) in item_ids.into_iter().enumerate() {
                mutation.check_connection().map_err(|error| {
                    format!("Queue deletion stopped after {completed}/{total} requests: {error}")
                })?;
                client
                    .execute_queue_command(mutation.queue_id(), QueueCommand::DeleteItem(item_id))
                    .await
                    .map_err(|error| {
                        format!(
                            "Queue deletion stopped after {completed}/{total} requests: {error}"
                        )
                    })?;
            }
            mutation.check_connection()
        }));
        Ok(())
    }

    pub(crate) fn select_queue_item(&mut self, item_id: QueueItemId) -> Result<(), String> {
        self.queue_command(QueueCommand::PlayItem(item_id))
    }

    pub(crate) fn play_queue_index(&mut self, index: usize) -> Result<(), String> {
        self.queue_command(QueueCommand::PlayIndex(index))
    }

    // Player transport and queue preferences.
    pub(crate) fn transport_play(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Play)
    }

    pub(crate) fn transport_pause(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Pause)
    }

    pub(crate) fn transport_stop(&mut self) {
        let result = self.player_command(PlayerCommand::Stop);
        self.record_failure(result);
    }

    pub(crate) fn transport_next(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Next)
    }

    pub(crate) fn transport_previous(&mut self) -> Result<(), String> {
        self.player_command(PlayerCommand::Previous)
    }

    pub(crate) fn transport_seek(&mut self, seconds: f64) -> Result<(), String> {
        self.player_command(PlayerCommand::Seek(seconds))
    }

    pub(crate) fn set_volume(&mut self, volume: u8) {
        let result = self.player_command(PlayerCommand::Volume(volume));
        self.record_failure(result);
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        let result = self.player_command(PlayerCommand::Mute(muted));
        self.record_failure(result);
    }

    pub(crate) fn set_shuffle(&mut self, enabled: bool) {
        let result = self.queue_command(QueueCommand::Shuffle(enabled));
        self.record_failure(result);
    }

    pub(crate) fn set_repeat_mode(&mut self, mode: RepeatMode) {
        let result = self.queue_command(QueueCommand::Repeat(mode));
        self.record_failure(result);
    }

    pub(crate) fn set_autoplay(&mut self, enabled: bool) {
        let result = self.queue_command(QueueCommand::Autoplay(enabled));
        self.record_failure(result);
    }

    pub(crate) fn take_pending_commands(&mut self) -> Vec<PlaybackCommand> {
        std::mem::take(&mut self.pending_commands)
    }

    // Read-only server projections and revision metadata.
    pub(crate) fn queue(&self) -> Queue {
        self.projection
            .as_ref()
            .map(|projection| projection.queue())
            .unwrap_or_default()
    }

    pub(crate) fn player_state(&self) -> PlayerState {
        self.projection
            .as_ref()
            .map(|projection| projection.remote_player_state())
            .unwrap_or_else(|| PlayerState {
                active_mode: PlaybackMode::Remote,
                ..PlayerState::default()
            })
    }

    pub(crate) fn queue_revision(&self) -> Option<(QueueSnapshotRequest, bool)> {
        self.projection
            .as_ref()
            .map(MusicAssistantQueueController::revision)
    }

    pub(crate) fn synchronized_snapshot(&self) -> Option<MediaSessionState> {
        self.projection.as_ref()?.synchronized_snapshot()
    }

    // Command serialization and asynchronous error delivery.
    fn player_command(&mut self, command: PlayerCommand) -> Result<(), String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?;
        let client = configuration.connection.client.clone();
        let player_id = configuration.player_id.clone();
        self.pending_commands.push(Box::pin(async move {
            client.execute_player_command(&player_id, command).await
        }));
        Ok(())
    }

    fn queue_command(&mut self, command: QueueCommand) -> Result<(), String> {
        let projection = self
            .projection
            .as_ref()
            .ok_or("remote queue is not active")?;
        if !projection.is_synchronized() {
            return Err("remote queue is synchronizing".into());
        }
        let client = self
            .configuration
            .as_ref()
            .ok_or("remote player is not configured")?
            .connection
            .client
            .clone();
        let queue_id = projection.queue().queue_id;
        self.pending_commands.push(Box::pin(async move {
            client.execute_queue_command(&queue_id, command).await
        }));
        Ok(())
    }

    fn record_failure(&mut self, result: Result<(), String>) {
        if let Err(error) = result {
            self.pending_commands
                .push(Box::pin(async move { Err(error) }));
        }
    }
}

impl Drop for MusicAssistantPlaybackTarget {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_assistant::client::tests::server_with_request_handler;
    use crate::music_assistant::event_hub::MusicAssistantEvent;
    use crate::music_assistant::models::identifiers::{AccessToken, QueueId};
    use crate::music_assistant::models::session::MusicAssistantServerUrl;
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};
    use tokio::sync::broadcast;

    #[test]
    pub(crate) fn unconfigured_remote_has_no_local_audio_and_rejects_commands() {
        let mut engine = MusicAssistantPlaybackTarget::new();
        assert_eq!(engine.player_state().active_mode, PlaybackMode::Remote);
        assert_eq!(engine.player_state().buffered_position_millis, None);
        assert!(engine.transport_play().is_err());
        assert!(engine.play_queue_index(0).is_err());
        assert!(engine
            .delete_queue_items(Vec::new())
            .unwrap_err()
            .contains("must not be empty"));
        assert!(engine
            .delete_queue_items(vec![QueueItemId(" ".into())])
            .unwrap_err()
            .contains("must not be empty"));
        engine.shutdown();
    }

    // HTTP responses deliver queue events before acknowledging each mutation.
    #[test]
    fn deletion_batches_survive_item_events_and_stop_on_failure_or_reconnect() {
        for scenario in ["success", "failure", "reconnect"] {
            CORE_NETWORK_RUNTIME.block_on(async {
                let items = (1..=3)
                    .map(|index| {
                        json!({
                            "queue_id":"queue", "queue_item_id":format!("item-{index}"),
                            "name":format!("Item {index}")
                        })
                    })
                    .collect::<Vec<_>>();
                let mut responses = snapshot_responses(json!(items));
                responses.push((
                    "player_queues/delete_item",
                    if scenario == "failure" {
                        json!({"test_http_status":400})
                    } else {
                        Value::Null
                    },
                ));
                if scenario == "success" {
                    responses.push(("player_queues/delete_item", Value::Null));
                }
                let remaining = match scenario {
                    "success" => items[2..].to_vec(),
                    "failure" => items.clone(),
                    _ => items[1..].to_vec(),
                };
                responses.extend(snapshot_responses(json!(remaining)));
                let (event_sender, receiver) = broadcast::channel(64);
                let events = event_sender.clone();
                let (client, worker) = server_with_request_handler(responses, move |request| {
                    if request["command"] != "player_queues/delete_item" {
                        return;
                    }
                    events
                        .send(MusicAssistantEvent::QueueItemsUpdated(QueueId(
                            "queue".into(),
                        )))
                        .unwrap();
                    if scenario == "reconnect" {
                        events.send(MusicAssistantEvent::Disconnected).unwrap();
                        events.send(MusicAssistantEvent::Connected).unwrap();
                    }
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while events.len() > 0 {
                        assert!(
                            Instant::now() < deadline,
                            "projection did not consume queue events"
                        );
                        std::thread::yield_now();
                    }
                });
                let client = Arc::new(client);
                let mut connection = MusicAssistantConnection::new(
                    MusicAssistantServerUrl("http://127.0.0.1:1".into()),
                    Some(AccessToken("test-token".into())),
                )
                .unwrap();
                connection.client = client.clone();
                let mut target = MusicAssistantPlaybackTarget {
                    configuration: Some(MusicAssistantPlaybackTargetConfiguration {
                        player_id: PlayerId("queue".into()),
                        connection: Arc::new(connection),
                    }),
                    projection: Some(MusicAssistantQueueController::new(
                        &CORE_NETWORK_RUNTIME,
                        client,
                        receiver,
                        PlayerId("queue".into()),
                        true,
                    )),
                    pending_commands: Vec::new(),
                };
                wait_for_synchronization(&target).await;
                target
                    .delete_queue_items(vec![
                        QueueItemId("item-1".into()),
                        QueueItemId("item-2".into()),
                        QueueItemId("item-1".into()),
                    ])
                    .unwrap();
                let mut commands = target.take_pending_commands();
                assert_eq!(commands.len(), 1);
                let result = commands.pop().unwrap().await;
                assert_eq!(
                    result.is_ok(),
                    scenario == "success",
                    "{scenario}: {result:?}"
                );
                if scenario == "failure" {
                    assert!(result.unwrap_err().contains("0/2"));
                } else if scenario == "reconnect" {
                    assert!(result.unwrap_err().contains("1/2"));
                }
                wait_for_synchronization(&target).await;
                assert_eq!(target.queue().items.len(), remaining.len());
                let requests = worker.join().unwrap();
                let deletions = requests
                    .iter()
                    .filter(|request| request["command"] == "player_queues/delete_item")
                    .map(|request| request["args"]["item_id_or_index"].clone())
                    .collect::<Vec<_>>();
                assert_eq!(
                    deletions,
                    if scenario == "success" {
                        vec![json!("item-1"), json!("item-2")]
                    } else {
                        vec![json!("item-1")]
                    }
                );
                assert!(requests.iter().all(|request| request["args"]
                    .get("queue_id")
                    .is_none_or(|queue_id| queue_id == "queue")));
                assert_eq!(
                    requests
                        .iter()
                        .filter(|request| request["command"] == "player_queues/items")
                        .count(),
                    2
                );
                drop(event_sender);
            });
        }
    }

    // Shared snapshot fixtures retain stable queue identities across the batch.
    fn snapshot_responses(items: Value) -> Vec<(&'static str, Value)> {
        vec![
            (
                "player_queues/get_active_queue",
                json!({"queue_id":"queue"}),
            ),
            (
                "players/get",
                json!({
                    "player_id":"queue", "provider":"test", "type":"player", "name":"Player",
                    "available":true, "device_info":{"model":"test","manufacturer":"test"}
                }),
            ),
            ("player_queues/items", items),
        ]
    }

    async fn wait_for_synchronization(target: &MusicAssistantPlaybackTarget) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !target.projection.as_ref().unwrap().is_synchronized() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("queue snapshot did not synchronize");
    }
}
