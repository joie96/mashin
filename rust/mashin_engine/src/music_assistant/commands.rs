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
    },
    PlayIndex(usize),
    PlayItem(QueueItemId),
    MoveItem {
        item_id: QueueItemId,
        shift: i64,
    },
    DeleteItem(QueueItemId),
    MoveItemToEnd(QueueItemId),
    Shuffle(bool),
    Repeat(RepeatMode),
    Autoplay(bool),
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
                json!({"queue_id":queue_id,"media":media,"option":option,"start_item":start,"shuffle":shuffle,"sort_by":"original"}),
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
mod tests {
    use super::*;

    #[test]
    fn queue_contract_uses_ma_item_ids_and_separate_clear_command() {
        let queue = QueueId("room".into());
        for (command, expected_name, expected) in [
            (QueueCommand::Clear, "clear", json!({"queue_id":"room"})),
            (
                QueueCommand::MoveItemToEnd(QueueItemId("stable-item".into())),
                "move_item_end",
                json!({"queue_id":"room", "queue_item_id":"stable-item"}),
            ),
            (
                QueueCommand::MoveItem {
                    item_id: QueueItemId("stable-item".into()),
                    shift: 0,
                },
                "move_item",
                json!({"queue_id":"room", "queue_item_id":"stable-item", "pos_shift":0}),
            ),
            (
                QueueCommand::Autoplay(true),
                "autoplay",
                json!({"queue_id":"room", "autoplay_enabled":true}),
            ),
            (
                QueueCommand::Autoplay(false),
                "autoplay",
                json!({"queue_id":"room", "autoplay_enabled":false}),
            ),
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
    fn media_and_shuffle_are_forwarded_without_radio_mode_or_reordering() {
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
            },
        )
        .unwrap();
        assert_eq!(name, "play_media");
        assert_eq!(arguments["media"][0]["media_type"], "album");
        assert_eq!(arguments["media"][1]["item_id"], "track");
        assert_eq!(arguments["start_item"], arguments["media"][1]);
        assert_eq!(arguments["shuffle"], true);
        assert!(arguments.get("radio_mode").is_none());
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
                }
            )
            .await
            .unwrap_err()
            .contains("outside"));
    }
}
