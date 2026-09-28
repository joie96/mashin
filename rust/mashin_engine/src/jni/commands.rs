//! Synchronous Android JNI command adapter for the Rust-owned playback core.

use crate::core;
use crate::core::models::{CoreError, MediaSessionEvent};
use crate::ffi::event_channel;
use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::RepeatMode;

// Session lifecycle commands.
pub(crate) fn restore_persisted_session() -> Result<(bool, Vec<MediaSessionEvent>), CoreError> {
    core::restore_persisted_session_and_collect_events()
}

pub(crate) fn activate_engine() -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| coordinator.activate_selected_engine())
}

// Queue commands.
pub(crate) fn replace_queue(serialized_items: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let media_items = parse_media_items(serialized_items)?;
    core::resolve_media_and_execute_command(media_items, false, |coordinator, tracks| {
        coordinator.play_media(tracks)
    })
}

pub(crate) fn replace_queue_shuffled(
    serialized_items: &str,
) -> Result<Vec<MediaSessionEvent>, CoreError> {
    let media_items = parse_media_items(serialized_items)?;
    core::resolve_media_and_execute_command(media_items, true, |coordinator, tracks| {
        coordinator.play_media(tracks)
    })
}

pub(crate) fn play_queue_item(item_id: &str) -> Result<Vec<MediaSessionEvent>, CoreError> {
    execute_core_command(|coordinator| {
        coordinator.play_queue_item(QueueItemId(item_id.to_string()))
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
        "playMediaItems" => replace_queue(required_text_argument(argument)?),
        "playShuffledMedia" => replace_queue_shuffled(required_text_argument(argument)?),
        "playQueueItem" => play_queue_item(required_text_argument(argument)?),
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
    for event in events {
        if let Ok(event) = serde_json::to_string(&event) {
            event_channel::publish_json_text(&event);
        }
    }
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
    use super::execute_command;

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
    fn android_shuffle_routes_to_the_core_queue_replacement() {
        assert_eq!(
            execute_command("playShuffledMedia", None).unwrap_err(),
            "missing Android core command argument"
        );
        assert!(execute_command("playShuffledMedia", Some("not JSON"))
            .unwrap_err()
            .starts_with("invalid Android media items:"));
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
    }
}
