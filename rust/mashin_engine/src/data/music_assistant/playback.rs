//! MA HTTP and event-hub implementation of the playback server port.

use crate::data::music_assistant::client::{HttpCommandError, MusicAssistantClient};
use crate::data::music_assistant::connection::MusicAssistantConnection;
use crate::data::music_assistant::dto::player::MusicAssistantPlayer;
use crate::data::music_assistant::dto::queue::MusicAssistantQueue;
use crate::data::music_assistant::event_hub::EventHubLeaseReason;
use crate::data::music_assistant::mapping::queue_media_payload;
use crate::domain::identifiers::{PlayerId, QueueId};
use crate::playback::models::QueueItem;
use crate::playback::server::{
    PlaybackConnection, PlaybackServer, PlaybackServerError, PlayerCommand, QueueCommand,
    ServerEvent,
};
use crate::playback::server::{ServerPlayer, ServerQueue};
use futures_util::future::BoxFuture;
use rand::seq::SliceRandom;
use serde_json::{json, Value};
use std::sync::Arc;

impl PlaybackServer for MusicAssistantClient {
    fn execute_player_command<'a>(
        &'a self,
        player: &'a PlayerId,
        command: PlayerCommand,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(MusicAssistantClient::execute_player_command(
            self, player, command,
        ))
    }
    fn execute_queue_command<'a>(
        &'a self,
        queue: &'a QueueId,
        command: QueueCommand,
    ) -> BoxFuture<'a, Result<(), PlaybackServerError>> {
        Box::pin(async move {
            MusicAssistantClient::execute_queue_command(self, queue, command)
                .await
                .map_err(map_error)
        })
    }
    fn get_player<'a>(
        &'a self,
        player: &'a PlayerId,
    ) -> BoxFuture<'a, Result<ServerPlayer, String>> {
        Box::pin(async move {
            MusicAssistantClient::get_player(self, player)
                .await
                .map(Into::into)
        })
    }
    fn get_active_queue<'a>(
        &'a self,
        player: &'a PlayerId,
    ) -> BoxFuture<'a, Result<Option<ServerQueue>, String>> {
        Box::pin(async move {
            MusicAssistantClient::get_active_queue(self, player)
                .await
                .map(|queue| queue.map(Into::into))
        })
    }
    fn get_player_queues(&self) -> BoxFuture<'_, Result<Vec<ServerQueue>, PlaybackServerError>> {
        Box::pin(async move {
            MusicAssistantClient::get_player_queues(self)
                .await
                .map(|queues| queues.into_iter().map(Into::into).collect())
                .map_err(map_error)
        })
    }
    fn get_queue_items<'a>(
        &'a self,
        queue: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Vec<QueueItem>, String>> {
        Box::pin(MusicAssistantClient::get_queue_items(
            self, queue, limit, offset,
        ))
    }
}

impl PlaybackConnection for MusicAssistantConnection {
    fn server_url(&self) -> String {
        self.server_url.0.clone()
    }
    fn client(&self) -> Arc<dyn PlaybackServer> {
        self.client.clone()
    }

    fn acquire_playback(
        &self,
        runtime: &tokio::runtime::Handle,
    ) -> Result<(tokio::sync::broadcast::Receiver<ServerEvent>, bool), String> {
        let mut hub = self
            .event_hub
            .lock()
            .map_err(|_| "MA event hub lock poisoned")?;
        let events = hub.subscribe();
        let _runtime = runtime.enter();
        hub.acquire(EventHubLeaseReason::Playback);
        Ok((events, hub.is_connected()))
    }

    fn release_playback(&self) {
        self.event_hub
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .release(EventHubLeaseReason::Playback);
    }
}

fn map_error(error: HttpCommandError) -> PlaybackServerError {
    if matches!(
        error,
        HttpCommandError::Http {
            status: 401 | 403,
            ..
        }
    ) {
        PlaybackServerError::Denied(error.to_string())
    } else {
        PlaybackServerError::Failed(error.to_string())
    }
}

impl MusicAssistantClient {
    // Playback and queue commands.
    pub(crate) async fn execute_player_command(
        &self,
        player_id: &PlayerId,
        command: PlayerCommand,
    ) -> Result<(), String> {
        let (name, arguments) = match command {
            PlayerCommand::Play => ("play", json!({"player_id":player_id})),
            PlayerCommand::Pause => ("pause", json!({"player_id":player_id})),
            PlayerCommand::Stop => ("stop", json!({"player_id":player_id})),
            PlayerCommand::Next => ("next", json!({"player_id":player_id})),
            PlayerCommand::Previous => ("previous", json!({"player_id":player_id})),
            PlayerCommand::Seek(position) => {
                if !position.is_finite() || position < 0.0 {
                    return Err("seek position must be finite and nonnegative".into());
                }
                (
                    "seek",
                    json!({"player_id":player_id,"position":position.round() as i64}),
                )
            }
            PlayerCommand::Volume(volume) => (
                "volume_set",
                json!({"player_id":player_id,"volume_level":volume.min(100)}),
            ),
            PlayerCommand::Mute(muted) => {
                ("volume_mute", json!({"player_id":player_id,"muted":muted}))
            }
        };
        self.execute_command::<Value>(&format!("players/cmd/{name}"), arguments)
            .await
            .map(|_| ())
    }

    pub(crate) async fn execute_queue_command(
        &self,
        queue_id: &QueueId,
        command: QueueCommand,
    ) -> Result<(), HttpCommandError> {
        let (name, arguments) = queue_command_request(queue_id, command).map_err(|details| {
            HttpCommandError::InvalidArguments {
                command: "player_queues".into(),
                details,
            }
        })?;
        self.execute_http_command::<Value>(&format!("player_queues/{name}"), arguments)
            .await
            .map(|_| ())
    }

    // Remote player and queue queries.
    pub(crate) async fn get_player(
        &self,
        player_id: &PlayerId,
    ) -> Result<MusicAssistantPlayer, String> {
        self.execute_command("players/get", json!({"player_id": player_id}))
            .await
    }

    pub(crate) async fn get_active_queue(
        &self,
        player_id: &PlayerId,
    ) -> Result<Option<MusicAssistantQueue>, String> {
        self.execute_command(
            "player_queues/get_active_queue",
            json!({"player_id": player_id}),
        )
        .await
    }

    pub(crate) async fn get_player_queues(
        &self,
    ) -> Result<Vec<MusicAssistantQueue>, HttpCommandError> {
        self.execute_http_command("player_queues/all", json!({}))
            .await
    }

    pub(crate) async fn get_queue_items(
        &self,
        queue_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<QueueItem>, String> {
        self.execute_command(
            "player_queues/items",
            json!({ "queue_id": queue_id, "limit": limit, "offset": offset }),
        )
        .await
    }
}
fn queue_command_request(
    queue_id: &QueueId,
    command: QueueCommand,
) -> Result<(&'static str, Value), String> {
    let (name, arguments) = match command {
        QueueCommand::Clear => ("clear", json!({"queue_id": queue_id})),
        QueueCommand::PlayMedia {
            mut media,
            option,
            start_item,
            shuffle,
        } => {
            let start = start_item
                .map(|index| {
                    media
                        .get(index)
                        .cloned()
                        .ok_or_else(|| "start item is outside the media request".to_string())
                })
                .transpose()?
                .map(queue_media_payload);
            if shuffle {
                media.shuffle(&mut rand::rng());
            }
            let media = media
                .into_iter()
                .map(queue_media_payload)
                .collect::<Vec<_>>();
            (
                "play_media",
                json!({"queue_id":queue_id,"media":media,"option":option,"start_item":start}),
            )
        }
        QueueCommand::PlayIndex(index) => {
            ("play_index", json!({"queue_id":queue_id,"index":index}))
        }
        QueueCommand::PlayItem {
            item_id,
            position_seconds,
        } => (
            "play_index",
            json!({"queue_id":queue_id,"index":item_id,"seek_position":position_seconds}),
        ),
        QueueCommand::MoveItem { item_id, shift } => (
            "move_item",
            json!({"queue_id":queue_id,"queue_item_id":item_id,"pos_shift":shift}),
        ),
        QueueCommand::DeleteItem(item_id) => (
            "delete_item",
            json!({"queue_id":queue_id,"item_id_or_index":item_id}),
        ),
        QueueCommand::MoveItemToEnd(item_id) => (
            "move_item_end",
            json!({"queue_id":queue_id,"queue_item_id":item_id}),
        ),
        QueueCommand::Shuffle(enabled) => (
            "shuffle",
            json!({"queue_id":queue_id,"shuffle_enabled":enabled}),
        ),
        QueueCommand::Repeat(mode) => ("repeat", json!({"queue_id":queue_id,"repeat_mode":mode})),
        QueueCommand::Autoplay(enabled) => (
            "autoplay",
            json!({"queue_id":queue_id,"autoplay_enabled":enabled}),
        ),
    };
    Ok((name, arguments))
}

#[cfg(test)]
mod command_tests {
    
    
    
    
}
