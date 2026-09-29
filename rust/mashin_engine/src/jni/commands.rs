//! Synchronous Android JNI command adapter for the Rust-owned playback core.

use crate::core;
use crate::core::models::{CoreError, MediaSessionEvent, QueuePlayMediaRequest};
use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{QueueOption, RepeatMode};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueInsertRequest {
    index: usize,
    media_items: Vec<MediaItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueMoveRequest {
    item_ids: Vec<QueueItemId>,
    target_index: usize,
}

// Session lifecycle commands.
pub(crate) fn restore_persisted_session() -> Result<(bool, Vec<MediaSessionEvent>), CoreError> {
    core::restore_persisted_session_and_collect_events()
}

pub(crate) fn activate_engine() -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| coordinator.activate_selected_engine())
}

// Queue commands.
pub(crate) fn queue_play_media(argument: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let request: QueuePlayMediaRequest = serde_json::from_str(argument).map_err(|error| {
        CoreError::command_failed(format!("invalid Android play media request: {error}"))
    })?;
    if request.option == QueueOption::Unknown {
        return Err(CoreError::command_failed("unknown queue option"));
    }
    if request.media_items.is_empty() && request.option == QueueOption::Replace {
        return execute_core_command(|coordinator| {
            coordinator.queue_play_media(Vec::new(), request.option, None, false, false)
        });
    }
    core::resolve_media_and_execute_command(request.media_items, false, |coordinator, tracks| {
        coordinator.queue_play_media(tracks, request.option, request.start_item, request.shuffle, request.start_download)
    })
}

pub(crate) fn insert_queue_items(argument: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let request: QueueInsertRequest = serde_json::from_str(argument)
        .map_err(|error| CoreError::command_failed(format!("invalid queue insertion: {error}")))?;
    execute_core_command(|coordinator| coordinator.insert_queue_items(request.index, request.media_items))
}

pub(crate) fn append_queue_items(argument: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let items = parse_media_items(argument)?;
    execute_core_command(|coordinator| coordinator.append_queue_items(items))
}

pub(crate) fn move_queue_items(argument: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let request: QueueMoveRequest = serde_json::from_str(argument)
        .map_err(|error| CoreError::command_failed(format!("invalid queue move: {error}")))?;
    execute_core_command(|coordinator| coordinator.move_queue_items(request.item_ids, request.target_index))
}

pub(crate) fn delete_queue_items(argument: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let item_ids: Vec<QueueItemId> = serde_json::from_str(argument)
        .map_err(|error| CoreError::command_failed(format!("invalid queue deletion: {error}")))?;
    execute_core_command(|coordinator| coordinator.delete_queue_items(item_ids))
}

pub(crate) fn play_queue_item(item_id: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.play_queue_item(QueueItemId(item_id.to_string()))
    })
}

pub(crate) fn select_queue_item(item_id: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.select_queue_item(QueueItemId(item_id.to_string()))
    })
}

// Playback transport commands.
pub(crate) fn play() -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| coordinator.transport_play())
}

pub(crate) fn pause() -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.transport_pause();
        Ok(())
    })
}

pub(crate) fn stop() -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.transport_stop();
        Ok(())
    })
}

pub(crate) fn next() -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| coordinator.transport_next())
}

pub(crate) fn previous() -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| coordinator.transport_previous())
}

pub(crate) fn seek(seconds: f64) -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| coordinator.transport_seek(seconds))
}

// Playback preference commands.
pub(crate) fn set_volume(volume: u8) -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.set_volume(volume)?;
        if volume > 0 {
            coordinator.set_muted(false);
        }
        Ok(())
    })
}

pub(crate) fn set_shuffle(is_enabled: bool) -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.set_shuffle(is_enabled);
        Ok(())
    })
}

pub(crate) fn set_repeat_mode(mode: RepeatMode) -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.set_repeat_mode(mode);
        Ok(())
    })
}

// JNI command routing.
pub(crate) fn execute_command(kind: &str, argument: Option<&str>) -> Result<(), String> {
    let events = match kind {
        "activate" => activate_engine(),
        "queuePlayMedia" => queue_play_media(required_text_argument(argument)?),
        "insertQueueItems" => insert_queue_items(required_text_argument(argument)?),
        "appendQueueItems" => append_queue_items(required_text_argument(argument)?),
        "moveQueueItems" => move_queue_items(required_text_argument(argument)?),
        "deleteQueueItems" => delete_queue_items(required_text_argument(argument)?),
        "playQueueItem" => play_queue_item(required_text_argument(argument)?),
        "selectQueueItem" => select_queue_item(required_text_argument(argument)?),
        "startSendspinRadio" => {
            let media_items = parse_media_items(required_text_argument(argument)?)
                .map_err(|error| error.message)?;
            let events = core::start_sendspin_radio_and_execute_command(
                media_items,
                publish_media_session_events,
            )
            .map_err(|error| error.message)?;
            publish_media_session_events(events);
            return Ok(());
        }
        "play" => play(),
        "pause" => pause(),
        "stop" => stop(),
        "next" => next(),
        "previous" => previous(),
        "seekSeconds" => seek(
            required_text_argument(argument)?
                .parse()
                .map_err(|_| "invalid seek seconds")?,
        ),
        "setVolume" => set_volume(
            required_text_argument(argument)?
                .parse::<u8>()
                .map_err(|_| "invalid Android volume")?
                .min(100),
        ),
        "setShuffle" => set_shuffle(required_text_argument(argument)? == "true"),
        "setRepeatMode" => set_repeat_mode(match required_text_argument(argument)? {
            "one" => RepeatMode::One,
            "all" => RepeatMode::All,
            _ => RepeatMode::Off,
        }),
        "shutdown" => return core::shutdown().map_err(|error| error.message),
        _ => return Err(format!("unsupported Android core command '{kind}'")),
    };
    events
        .map(publish_media_session_events)
        .map_err(|error| error.message)
}

pub(crate) fn publish_media_session_events(events: Vec<MediaSessionEvent>) {
    core::event_channel::publish_jni_command_events(events);
}

fn execute_core_command<F>(operation: F) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut core::CoreCoordinator) -> Result<(), String>,
{
    core::execute_command_and_collect_events(operation)
}

fn required_text_argument(argument: Option<&str>) -> Result<&str, String> {
    argument.ok_or_else(|| "missing Android core command argument".to_string())
}

fn parse_media_items(serialized_items: &str) -> Result<Vec<MediaItem>, CoreError> {
    serde_json::from_str(serialized_items)
        .map_err(|error| CoreError::command_failed(format!("invalid Android media items: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{execute_command, QueuePlayMediaRequest};

    #[test]
    fn android_artist_radio_requires_valid_media_items() {
        assert_eq!(
            execute_command("startSendspinRadio", None).unwrap_err(),
            "missing Android core command argument"
        );
        assert!(execute_command("startSendspinRadio", Some("not JSON"))
            .unwrap_err()
            .starts_with("invalid Android media items:"));
    }

    #[test]
    fn android_queue_play_media_requires_a_valid_queue_option() {
        assert_eq!(
            execute_command("queuePlayMedia", None).unwrap_err(),
            "missing Android core command argument"
        );
        assert!(execute_command("queuePlayMedia", Some("not JSON"))
            .unwrap_err()
            .starts_with("invalid Android play media request:"));
        assert!(execute_command("queuePlayMedia", Some(r#"{"mediaItems":[],"option":"invalid","startDownload":true}"#))
            .unwrap_err()
            .contains("unknown queue option"));
    }

    #[test]
    fn android_queue_selection_requires_an_item_id() {
        assert_eq!(
            execute_command("playQueueItem", None).unwrap_err(),
            "missing Android core command argument"
        );
        assert_eq!(
            execute_command("playQueueItem", Some("missing-item")).unwrap_err(),
            "queue item is no longer available"
        );
        assert_eq!(
            execute_command("selectQueueItem", None).unwrap_err(),
            "missing Android core command argument"
        );
        assert_eq!(
            execute_command("selectQueueItem", Some("missing-item")).unwrap_err(),
            "queue item is no longer available"
        );
    }

    #[test]
    fn queue_play_media_rejects_removed_media3_fields() {
        assert!(
            execute_command("queuePlayMedia", Some(r#"{"queueEntries":[],"option":"replace","startDownload":false}"#))
                .unwrap_err()
                .starts_with("invalid Android play media request:")
        );
        assert!(
            execute_command("queuePlayMedia", Some(r#"{"mediaItems":[],"option":"replace","startPositionMillis":2500,"startDownload":false}"#))
                .unwrap_err()
                .starts_with("invalid Android play media request:")
        );
    }

    #[test]
    fn queue_play_media_accepts_regular_media_items() {
        let request: QueuePlayMediaRequest = serde_json::from_str(
            r#"{"mediaItems":[{"media_type":"track","item_id":"track-1","provider":"test","name":"First"}],"option":"replace","startItem":0,"startDownload":false}"#,
        )
        .expect("deserialize queue request");
        assert_eq!(request.start_item, Some(0));
        assert_eq!(request.media_items.len(), 1);
    }

    #[test]
    fn queue_edit_commands_validate_payloads() {
        assert!(execute_command("insertQueueItems", Some(r#"{"index":-1,"mediaItems":[]}"#))
            .unwrap_err().starts_with("invalid queue insertion:"));
        assert!(execute_command("appendQueueItems", Some("{}"))
            .unwrap_err().starts_with("invalid Android media items:"));
        assert!(execute_command("moveQueueItems", Some(r#"{"targetIndex":0,"itemIds":"invalid"}"#))
            .unwrap_err().starts_with("invalid queue move:"));
        assert!(execute_command("deleteQueueItems", Some("{}"))
            .unwrap_err().starts_with("invalid queue deletion:"));
    }

    #[test]
    fn android_volume_requires_a_number() {
        assert_eq!(
            execute_command("setVolume", None).unwrap_err(),
            "missing Android core command argument"
        );
        assert_eq!(
            execute_command("setVolume", Some("NaN")).unwrap_err(),
            "invalid Android volume"
        );
    }
}
