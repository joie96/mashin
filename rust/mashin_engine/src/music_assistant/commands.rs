//! Typed playback commands; HTTP serialization stays at the MA boundary.

use super::client::MusicAssistantClient;
use super::models::identifiers::{PlayerId, QueueId, QueueItemId};
use super::models::media::MediaItem;
use super::models::queue::{QueueOption, RepeatMode};
use serde_json::{json, Value};

pub(crate) enum PlayerCommand {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Seek(f64),
    Volume(u8),
    Mute(bool),
}

pub(crate) enum QueueCommand {
    Clear,
    PlayMedia {
        media: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
        radio_mode: bool,
    },
    PlayIndex(usize),
    PlayItem(QueueItemId),
    MoveItem {
        item_id: QueueItemId,
        shift: i64,
    },
    DeleteItem(QueueItemId),
    Shuffle(bool),
    Repeat(RepeatMode),
    Autoplay(bool),
}

impl MusicAssistantClient {
    // Playback and queue commands.
    pub(crate) async fn move_queue_items_to(
        &self,
        queue_id: &QueueId,
        item_ids: Vec<QueueItemId>,
        target: usize,
    ) -> Result<(), String> {
        let mut items = Vec::new();
        loop {
            let page = self
                .get_queue_items(&queue_id.0, 500, items.len() as u32)
                .await?;
            let has_more = page.len() == 500;
            items.extend(page.into_iter().map(|item| item.queue_item_id));
            if !has_more {
                break;
            }
        }
        let selected = items
            .iter()
            .filter(|item| item_ids.contains(item))
            .cloned()
            .collect::<Vec<_>>();
        if selected.len() != item_ids.len() {
            return Err("one or more queue items no longer exist".into());
        }
        let mut desired = items
            .iter()
            .filter(|item| !item_ids.contains(item))
            .cloned()
            .collect::<Vec<_>>();
        let target = target.min(desired.len());
        desired.splice(target..target, selected);
        for (destination, item_id) in desired.iter().enumerate() {
            let source = items
                .iter()
                .position(|item| item == item_id)
                .ok_or("queue item disappeared")?;
            if source == destination {
                continue;
            }
            self.execute_queue_command(
                queue_id,
                QueueCommand::MoveItem {
                    item_id: item_id.clone(),
                    shift: destination as i64 - source as i64,
                },
            )
            .await?;
            let item = items.remove(source);
            items.insert(destination, item);
        }
        Ok(())
    }

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
    ) -> Result<(), String> {
        let (name, arguments) = queue_command_request(queue_id, command)?;
        self.execute_command::<Value>(&format!("player_queues/{name}"), arguments)
            .await
            .map(|_| ())
    }
}

fn queue_command_request(
    queue_id: &QueueId,
    command: QueueCommand,
) -> Result<(&'static str, Value), String> {
    let (name, arguments) = match command {
        QueueCommand::Clear => ("clear", json!({"queue_id": queue_id})),
        QueueCommand::PlayMedia {
            media,
            option,
            start_item,
            shuffle,
            radio_mode,
        } => {
            let start = start_item
                .map(|index| {
                    media
                        .get(index)
                        .cloned()
                        .ok_or_else(|| "start item is outside the media request".to_string())
                })
                .transpose()?
                .map(super::client::queue_media_payload);
            let media = media
                .into_iter()
                .map(super::client::queue_media_payload)
                .collect::<Vec<_>>();
            (
                "play_media",
                json!({"queue_id":queue_id,"media":media,"option":option,"start_item":start,"shuffle":shuffle,"radio_mode":radio_mode,"sort_by":"original"}),
            )
        }
        QueueCommand::PlayIndex(index) => {
            ("play_index", json!({"queue_id":queue_id,"index":index}))
        }
        QueueCommand::PlayItem(item_id) => {
            ("play_index", json!({"queue_id":queue_id,"index":item_id}))
        }
        QueueCommand::MoveItem { item_id, shift } => (
            "move_item",
            json!({"queue_id":queue_id,"queue_item_id":item_id,"pos_shift":shift}),
        ),
        QueueCommand::DeleteItem(item_id) => (
            "delete_item",
            json!({"queue_id":queue_id,"item_id_or_index":item_id}),
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
mod tests {
    use super::*;

    #[test]
    fn queue_contract_uses_ma_item_ids_and_separate_clear_command() {
        let queue = QueueId("room".into());
        for (command, expected_name, expected) in [
            (QueueCommand::Clear, "clear", json!({"queue_id":"room"})),
            (
                QueueCommand::PlayItem(QueueItemId("stable-item".into())),
                "play_index",
                json!({"queue_id":"room", "index":"stable-item"}),
            ),
            (
                QueueCommand::DeleteItem(QueueItemId("stable-item".into())),
                "delete_item",
                json!({"queue_id":"room", "item_id_or_index":"stable-item"}),
            ),
            (
                QueueCommand::MoveItem {
                    item_id: QueueItemId("stable-item".into()),
                    shift: -2,
                },
                "move_item",
                json!({"queue_id":"room", "queue_item_id":"stable-item", "pos_shift":-2}),
            ),
        ] {
            let (name, arguments) = queue_command_request(&queue, command).unwrap();
            assert_eq!(name, expected_name);
            assert_eq!(arguments, expected);
        }
    }

    #[test]
    fn radio_and_shuffle_are_forwarded_without_local_expansion_or_reordering() {
        let media: Vec<MediaItem> = serde_json::from_value(json!([
            {"media_type":"album", "item_id":"album", "provider":"library", "name":"Album"},
            {"media_type":"track", "item_id":"track", "provider":"library", "name":"Track"}
        ]))
        .unwrap();
        let (name, arguments) = queue_command_request(
            &QueueId("room".into()),
            QueueCommand::PlayMedia {
                media,
                option: QueueOption::Play,
                start_item: Some(1),
                shuffle: true,
                radio_mode: true,
            },
        )
        .unwrap();
        assert_eq!(name, "play_media");
        assert_eq!(arguments["media"][0]["media_type"], "album");
        assert_eq!(arguments["media"][1]["item_id"], "track");
        assert_eq!(arguments["start_item"], arguments["media"][1]);
        assert_eq!(arguments["shuffle"], true);
        assert_eq!(arguments["radio_mode"], true);
        assert!(arguments.get("startDownload").is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_seek_and_start_without_sending_a_request() {
        let client = MusicAssistantClient::new("http://127.0.0.1:1", None);
        assert!(client
            .execute_player_command(&PlayerId("player".into()), PlayerCommand::Seek(f64::NAN))
            .await
            .unwrap_err()
            .contains("finite"));
        assert!(client
            .execute_queue_command(
                &QueueId("queue".into()),
                QueueCommand::PlayMedia {
                    media: Vec::new(),
                    option: QueueOption::Play,
                    start_item: Some(1),
                    shuffle: false,
                    radio_mode: false,
                }
            )
            .await
            .unwrap_err()
            .contains("outside"));
    }
}
