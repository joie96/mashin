//! Typed Android commands into the Rust-owned playback core.
//! JNI callers refresh their snapshot synchronously; events also notify Flutter.

use crate::core;
use crate::core::models::{
    CoreError, LogLevelPreference, LoginConfig, MediaSessionEvent, OfflineOutputConfig,
    QueuePlayMediaRequest, RemoteOutputConfig, SelectedOutput, ThemePreference,
};
use crate::music_assistant::models::identifiers::{PlayerId, QueueItemId};
use crate::music_assistant::models::media::MediaItem;
use crate::music_assistant::models::queue::{QueueOption, RepeatMode};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum PlaybackCommand {
    Login {
        music_assistant_url: String,
        username: String,
        password: String,
    },
    ActivateEngine,
    DeactivateEngine,
    Shutdown,
    SelectDefaultSendspinOutput,
    SelectOfflineLibrary {
        library_root: String,
    },
    SelectRemotePlayer {
        player_id: PlayerId,
    },
    TransportPlay,
    TransportPause,
    TransportStop,
    TransportNext,
    TransportPrevious,
    TransportSeek {
        seconds: f64,
    },
    TransportRepeatMode {
        mode: u8,
    },
    TransportShuffle {
        enabled: bool,
    },
    SetAutoplay {
        enabled: bool,
    },
    SetVolume {
        volume: u8,
    },
    SetMuted {
        muted: bool,
    },
    SetTheme {
        theme: ThemePreference,
    },
    SetLogLevel {
        log_level: LogLevelPreference,
    },
    SetUserDataFavorites {
        media_items: Vec<MediaItem>,
        favorite: bool,
    },
    QueuePlayMedia {
        request: QueuePlayMediaRequest,
    },
    StartSendspinRadio {
        media_items: Vec<MediaItem>,
    },
    InsertQueueItems {
        index: usize,
        media_items: Vec<MediaItem>,
    },
    AppendQueueItems {
        media_items: Vec<MediaItem>,
    },
    MoveQueueItems {
        item_ids: Vec<QueueItemId>,
        target_index: usize,
    },
    DeleteQueueItems {
        item_ids: Vec<QueueItemId>,
    },
    SelectQueueItem {
        item_id: QueueItemId,
    },
    PlayQueueIndex {
        index: usize,
    },
    RequestCodec {
        codec: String,
    },
    SetLocalCacheSizeBytes {
        cache_size_bytes: usize,
    },
}

// Session restoration precedes snapshot synchronization in the service.
pub(crate) fn restore_persisted_session() -> Result<(bool, Vec<MediaSessionEvent>), CoreError> {
    core::restore_persisted_session_and_collect_events()
}

// JNI command validation and execution.
pub(crate) fn execute_command(kind: &str, argument: Option<&str>) -> Result<(), String> {
    let command = decode_command(kind, argument)?;
    let events = match command {
        PlaybackCommand::Login {
            music_assistant_url,
            username,
            password,
        } => execute_core_command(|coordinator| {
            coordinator.authenticate_and_configure_session(LoginConfig {
                music_assistant_url:
                    crate::music_assistant::models::session::MusicAssistantServerUrl(
                        music_assistant_url,
                    ),
                username,
                password,
            })
        }),
        PlaybackCommand::ActivateEngine => {
            execute_core_command(|coordinator| coordinator.activate_selected_engine())
        }
        PlaybackCommand::DeactivateEngine => execute_core_command(|coordinator| {
            coordinator.deactivate_active_engine();
            Ok(())
        }),
        PlaybackCommand::Shutdown => return core::shutdown().map_err(|error| error.message),
        PlaybackCommand::SelectDefaultSendspinOutput => {
            execute_core_command(|coordinator| coordinator.select_default_sendspin_output())
        }
        PlaybackCommand::SelectOfflineLibrary { library_root } => {
            execute_core_command(|coordinator| {
                coordinator.select_output(SelectedOutput::Offline(OfflineOutputConfig {
                    library_root,
                }))
            })
        }
        PlaybackCommand::SelectRemotePlayer { player_id } => execute_core_command(|coordinator| {
            coordinator.select_output(SelectedOutput::Remote(RemoteOutputConfig { player_id }))
        }),
        PlaybackCommand::TransportPlay => {
            execute_core_command(|coordinator| coordinator.transport_play())
        }
        PlaybackCommand::TransportPause => {
            execute_core_command(|coordinator| coordinator.transport_pause())
        }
        PlaybackCommand::TransportStop => execute_core_command(|coordinator| {
            coordinator.transport_stop();
            Ok(())
        }),
        PlaybackCommand::TransportNext => {
            execute_core_command(|coordinator| coordinator.transport_next())
        }
        PlaybackCommand::TransportPrevious => {
            execute_core_command(|coordinator| coordinator.transport_previous())
        }
        PlaybackCommand::TransportSeek { seconds } => {
            execute_core_command(|coordinator| coordinator.transport_seek(seconds))
        }
        PlaybackCommand::TransportRepeatMode { mode } => execute_core_command(|coordinator| {
            coordinator.set_repeat_mode(match mode {
                0 => RepeatMode::Off,
                1 => RepeatMode::One,
                2 => RepeatMode::All,
                _ => return Err("invalid repeat mode".into()),
            });
            Ok(())
        }),
        PlaybackCommand::TransportShuffle { enabled } => execute_core_command(|coordinator| {
            coordinator.set_shuffle(enabled);
            Ok(())
        }),
        PlaybackCommand::SetAutoplay { enabled } => execute_core_command(|coordinator| {
            coordinator.set_autoplay(enabled);
            Ok(())
        }),
        PlaybackCommand::SetVolume { volume } => {
            execute_core_command(|coordinator| coordinator.set_volume(volume.min(100)))
        }
        PlaybackCommand::SetMuted { muted } => execute_core_command(|coordinator| {
            coordinator.set_muted(muted);
            Ok(())
        }),
        PlaybackCommand::SetTheme { theme } => {
            execute_core_command(|coordinator| coordinator.set_theme(theme))
        }
        PlaybackCommand::SetLogLevel { log_level } => {
            execute_core_command(|coordinator| coordinator.set_log_level(log_level))
        }
        PlaybackCommand::SetUserDataFavorites {
            media_items,
            favorite,
        } => {
            core::mutate_user_data(|user_data| {
                user_data.set_favorites(media_items, favorite);
                Ok(())
            })
            .map_err(|error| error.message)?;
            return Ok(());
        }
        PlaybackCommand::QueuePlayMedia { request } => {
            if request.option == QueueOption::Unknown {
                return Err("unknown queue option".into());
            }
            if request.media_items.is_empty() && request.option == QueueOption::Replace {
                execute_core_command(|coordinator| {
                    coordinator.queue_play_media(Vec::new(), request.option, None, false, false)
                })
            } else {
                core::resolve_media_and_execute_command(
                    request.media_items,
                    false,
                    |coordinator, tracks| {
                        coordinator.queue_play_media(
                            tracks,
                            request.option,
                            request.start_item,
                            request.shuffle,
                            request.start_download,
                        )
                    },
                )
            }
        }
        PlaybackCommand::StartSendspinRadio { media_items } => {
            core::start_sendspin_radio_and_execute_command(
                media_items,
                publish_media_session_events,
            )
        }
        PlaybackCommand::InsertQueueItems { index, media_items } => {
            execute_core_command(|coordinator| coordinator.insert_queue_items(index, media_items))
        }
        PlaybackCommand::AppendQueueItems { media_items } => {
            execute_core_command(|coordinator| coordinator.append_queue_items(media_items))
        }
        PlaybackCommand::MoveQueueItems {
            item_ids,
            target_index,
        } => {
            execute_core_command(|coordinator| coordinator.move_queue_items(item_ids, target_index))
        }
        PlaybackCommand::DeleteQueueItems { item_ids } => {
            execute_core_command(|coordinator| coordinator.delete_queue_items(item_ids))
        }
        PlaybackCommand::SelectQueueItem { item_id } => {
            execute_core_command(|coordinator| coordinator.select_queue_item(item_id))
        }
        PlaybackCommand::PlayQueueIndex { index } => {
            execute_core_command(|coordinator| coordinator.play_queue_index(index))
        }
        PlaybackCommand::RequestCodec { codec } => {
            execute_core_command(|coordinator| coordinator.set_preferred_codec(&codec))
        }
        PlaybackCommand::SetLocalCacheSizeBytes { cache_size_bytes } => {
            execute_core_command(|coordinator| {
                coordinator.set_local_cache_size_bytes(cache_size_bytes)
            })
        }
    };
    events
        .map(publish_media_session_events)
        .map_err(|error| error.message)
}

// Event delivery and core coordination.
pub(crate) fn publish_media_session_events(events: Vec<MediaSessionEvent>) {
    core::event_channel::publish_jni_command_events(events);
}

fn execute_core_command<F>(operation: F) -> Result<Vec<MediaSessionEvent>, CoreError>
where
    F: FnOnce(&mut core::CoreCoordinator) -> Result<(), String>,
{
    core::execute_command_and_collect_events(operation)
}

// Serialization is confined to the JNI boundary.
fn decode_command(kind: &str, argument: Option<&str>) -> Result<PlaybackCommand, String> {
    let mut arguments: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(argument.unwrap_or("{}"))
            .map_err(|error| format!("invalid Android command arguments: {error}"))?;
    if arguments.contains_key("kind") {
        return Err("command arguments must not override kind".into());
    }
    arguments.insert("kind".into(), serde_json::Value::String(kind.into()));
    serde_json::from_value(serde_json::Value::Object(arguments))
        .map_err(|error| format!("invalid Android command: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{decode_command, execute_command};

    // Platform command contracts.
    #[test]
    fn favorite_commands_accept_flutter_payloads_and_reject_invalid_types() {
        for favorite in [true, false] {
            let arguments = serde_json::json!({
                "mediaItems": [{"media_type":"track", "item_id":"track-1", "provider":"library", "name":"Track", "uri":"library://track/track-1"}],
                "favorite": favorite,
            });
            assert!(decode_command("setUserDataFavorites", Some(&arguments.to_string())).is_ok());
        }
        for arguments in [
            r#"{"mediaItems":[],"favorite":"true"}"#,
            r#"{"mediaItems":{ },"favorite":true}"#,
            r#"{"mediaItems":[]}"#,
        ] {
            assert!(decode_command("setUserDataFavorites", Some(arguments)).is_err());
        }
    }

    #[test]
    fn transport_uses_the_same_names_as_flutter() {
        for kind in [
            "transportPlay",
            "transportPause",
            "transportStop",
            "transportNext",
            "transportPrevious",
            "activateEngine",
            "deactivateEngine",
        ] {
            assert!(decode_command(kind, Some("{}")).is_ok(), "{kind}");
        }
        for kind in [
            "play",
            "pause",
            "stop",
            "next",
            "previous",
            "activate",
            "seekSeconds",
        ] {
            assert!(decode_command(kind, None).is_err(), "{kind}");
        }
    }

    #[test]
    fn queue_commands_reject_invalid_contracts() {
        for (kind, arguments) in [
            (
                "queuePlayMedia",
                r#"{"request":{"mediaItems":[],"option":"replace","startPositionMillis":2500,"startDownload":false}}"#,
            ),
            ("insertQueueItems", r#"{"index":-1,"mediaItems":[]}"#),
            ("appendQueueItems", r#"{"mediaItems":{}}"#),
            ("moveQueueItems", r#"{"targetIndex":0,"itemIds":"invalid"}"#),
            ("deleteQueueItems", r#"{"itemIds":{}}"#),
            ("playQueueIndex", r#"{"index":-1}"#),
        ] {
            assert!(decode_command(kind, Some(arguments)).is_err(), "{kind}");
        }
    }

    #[test]
    fn queue_commands_accept_flutter_payloads() {
        assert!(decode_command("queuePlayMedia", Some(r#"{"request":{"mediaItems":[{"media_type":"track","item_id":"track-1","provider":"test","name":"First"}],"option":"play","startItem":0,"shuffle":false,"startDownload":true}}"#)).is_ok());
        assert!(decode_command(
            "moveQueueItems",
            Some(r#"{"itemIds":["item-1"],"targetIndex":0}"#)
        )
        .is_ok());
        assert!(decode_command("transportRepeatMode", Some(r#"{"mode":2}"#)).is_ok());
    }

    #[test]
    fn arguments_cannot_change_the_command_or_coerce_types() {
        assert!(decode_command("transportPause", Some(r#"{"kind":"transportPlay"}"#)).is_err());
        assert!(decode_command("setVolume", Some(r#"{"volume":"50"}"#)).is_err());
        assert!(decode_command("transportShuffle", Some(r#"{"enabled":"true"}"#)).is_err());
        assert!(decode_command("setMuted", Some(r#"{"muted":true}"#)).is_ok());
    }

    #[test]
    fn unknown_queue_option_and_missing_selection_fail() {
        assert_eq!(
            execute_command(
                "queuePlayMedia",
                Some(r#"{"request":{"mediaItems":[],"option":"invalid","startDownload":true}}"#)
            )
            .unwrap_err(),
            "unknown queue option"
        );
        assert_eq!(
            execute_command("selectQueueItem", Some(r#"{"itemId":"missing-item"}"#)).unwrap_err(),
            "queue item is no longer available"
        );
    }
}
