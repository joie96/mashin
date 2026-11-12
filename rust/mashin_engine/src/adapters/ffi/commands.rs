//! FFI commands and routing into the application core.

use super::errors::record_and_publish_error;
use super::strings::{read_required_c_string, read_required_json_array, read_required_uri_array};
use crate::adapters::event_router::publish_background_event;
use crate::adapters::models::{serialize_command_value, NativeOperation, NativeOperationRequest};
use crate::application::commands::ApplicationCommand;
use crate::application::commands::QueuePlayMediaRequest;
use crate::application::dispatcher::{CommandResult, CommandValue};
use crate::application::errors::ApplicationError;
use crate::domain::identifiers::PlayerId;
use crate::domain::identifiers::QueueItemId;
use crate::domain::media::{MediaItem, Track};
use crate::domain::settings::LoginConfig;
use serde_json::{json, Value};
use std::ffi::c_char;

// Asynchronous Flutter operation dispatch.
#[no_mangle]
pub extern "C" fn mashin_core_start_operation(request_json: *const c_char) -> i32 {
    let request = match read_required_c_string(request_json, "operation request")
        .map_err(ApplicationError::command_failed)
        .and_then(|value| {
            serde_json::from_str::<NativeOperationRequest>(&value).map_err(|error| {
                ApplicationError::command_failed(format!(
                    "decode operation request failed: {error}"
                ))
            })
        }) {
        Ok(request) => request,
        Err(error) => {
            record_and_publish_error(error);
            return -1;
        }
    };

    let is_catalog_observed = match request.catalog_completion_requested() {
        Ok(observed) => observed,
        Err(error) => {
            record_and_publish_error(error);
            return -1;
        }
    };
    if let NativeOperation::Query(query) = request.operation {
        let request_id = request.request_id;
        return match crate::bootstrap::application().start_query(request_id, query, move |result| {
            let result = match result {
                Ok(crate::application::queries::QueryResult::ImageRead(snapshot)) => {
                    super::event_channel::publish_image_snapshot(
                        json!({"type":"query_completed", "requestId":request_id}), snapshot,
                    );
                    return;
                }
                result => result,
            };
            let result = serialize_query_completion(result, is_catalog_observed);
            let payload = match result {
                Ok(result) => {
                    json!({"type": "query_completed", "requestId": request_id, "result": result})
                }
                Err(error) => {
                    json!({"type": "query_failed", "requestId": request_id, "error": error})
                }
            };
            super::event_channel::publish_json_value(payload);
        }) {
            Ok(()) => 0,
            Err(error) => {
                record_and_publish_error(error);
                -1
            }
        };
    }

    crate::bootstrap::application().spawn_blocking_operation(move || {
        let request_id = request.request_id;
        match execute_native_operation(request) {
            Ok(result) => super::event_channel::publish_json_value(json!({
                "type": "operation_completed",
                "requestId": request_id,
                "result": result,
            })),
            Err(error) => super::event_channel::publish_json_value(json!({
                "type": "operation_failed",
                "requestId": request_id,
                "error": error,
            })),
        }
    });
    0
}

fn execute_native_operation(request: NativeOperationRequest) -> Result<Value, ApplicationError> {
    match request.operation {
        NativeOperation::Command(command) => {
            let result = crate::bootstrap::application().execute_command(command)?;
            for event in result.events {
                publish_background_event(event);
            }
            serialize_command_value(result.value)
        }
        NativeOperation::Query(_) => Err(ApplicationError::internal(
            "query must use cancellable dispatch",
        )),
    }
}

fn serialize_operation_result<T: serde::Serialize>(
    result: Result<T, ApplicationError>,
) -> Result<Value, ApplicationError> {
    result.and_then(|value| {
        serde_json::to_value(value).map_err(|error| {
            ApplicationError::internal(format!("serialize operation result failed: {error}"))
        })
    })
}

fn serialize_query_completion(
    result: Result<crate::application::queries::QueryResult, ApplicationError>,
    is_catalog_observed: bool,
) -> Result<Value, ApplicationError> {
    result.and_then(|value| {
        if is_catalog_observed {
            Ok(json!({"catalogResultDelivered": true}))
        } else {
            serialize_operation_result(Ok(value))
        }
    })
}

#[cfg(test)]
mod query_completion_tests {
    use super::*;
    use crate::application::queries::QueryResult;

    #[test]
    fn observed_completion_omits_inventory_but_unobserved_reads_and_failures_remain_complete() {
        let tracks: Vec<Track> = serde_json::from_value(json!([{
            "item_id":"track", "provider":"library", "name":"Retained track"
        }])).unwrap();
        let completed = serialize_query_completion(Ok(QueryResult::Tracks(tracks.clone())), true).unwrap();
        assert_eq!(completed, json!({"catalogResultDelivered":true}));
        let full = serialize_query_completion(Ok(QueryResult::Tracks(tracks)), false).unwrap();
        assert_eq!(full[0]["item_id"], "track");
        assert_eq!(full[0]["name"], "Retained track");
        assert!(serialize_query_completion(Err(ApplicationError::command_failed("failed")), true).is_err());
    }
}

// Core configuration and engine lifecycle commands.
#[no_mangle]
pub extern "C" fn mashin_core_set_log_level(level: *const c_char) -> i32 {
    match read_required_c_string(level, "log_level")
        .and_then(|level| crate::logging::set_minimum_level(&level))
    {
        Ok(()) => 0,
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_log(
    level: *const c_char,
    category: *const c_char,
    message: *const c_char,
) -> i32 {
    let result = read_required_c_string(level, "log_level").and_then(|level| {
        read_required_c_string(category, "log_category").and_then(|category| {
            read_required_c_string(message, "log_message")
                .and_then(|message| crate::logging::log_external(&level, &category, &message))
        })
    });
    match result {
        Ok(()) => 0,
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_login(login_json: *const c_char) -> i32 {
    let login_json = match read_required_c_string(login_json, "login_json") {
        Ok(value) => value,
        Err(error) => {
            record_and_publish_error(error);
            return -1;
        }
    };
    let login = match parse_login_configuration(&login_json) {
        Ok(value) => value,
        Err(error) => {
            record_and_publish_error(error);
            return -1;
        }
    };
    execute_command(ApplicationCommand::Login {
        music_assistant_url: login.music_assistant_url.0,
        username: login.username,
        password: login.password,
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_logout() -> i32 {
    execute_command(ApplicationCommand::Logout {})
}

#[no_mangle]
pub extern "C" fn mashin_core_restore_persisted_session() -> i32 {
    match crate::bootstrap::application()
        .execute_command(ApplicationCommand::RestorePersistedSession {})
    {
        Ok(CommandResult {
            value: CommandValue::RestoredSession(has_restored_session),
            events,
        }) => {
            for event in events {
                publish_background_event(event);
            }
            if has_restored_session {
                0
            } else {
                1
            }
        }
        Ok(_) => {
            record_and_publish_error(ApplicationError::internal(
                "unexpected session restoration result",
            ));
            -1
        }
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_select_local_output() -> i32 {
    execute_command(ApplicationCommand::SelectLocalOutput {})
}

#[no_mangle]
pub extern "C" fn mashin_core_select_remote_player(player_id: *const c_char) -> i32 {
    let player_id = match read_required_c_string(player_id, "player_id") {
        Ok(value) => value,
        Err(error) => {
            record_and_publish_error(error);
            return -1;
        }
    };
    execute_command(ApplicationCommand::SelectRemotePlayer {
        player_id: PlayerId(player_id),
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_activate_engine() -> i32 {
    execute_command(ApplicationCommand::ActivateEngine {})
}

#[no_mangle]
pub extern "C" fn mashin_core_deactivate_engine() -> i32 {
    execute_command(ApplicationCommand::DeactivateEngine {})
}

#[no_mangle]
pub extern "C" fn mashin_core_shutdown() -> i32 {
    execute_command(ApplicationCommand::Shutdown {})
}

// Playlist mutation commands.
#[no_mangle]
pub extern "C" fn mashin_core_add_playlist_tracks(
    playlist_id: *const c_char,
    uris_json: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| read_required_uri_array(uris_json).map(|uris| (playlist_id, uris)))
        .map_err(ApplicationError::command_failed)
        .and_then(|(playlist_id, uris)| {
            crate::bootstrap::application()
                .execute_command(ApplicationCommand::AddPlaylistTracks { playlist_id, uris })
        });
    publish_command_result(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_remove_playlist_tracks(
    playlist_id: *const c_char,
    positions_json: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| {
            read_required_json_array(positions_json, "positions_json")
                .map(|positions| (playlist_id, positions))
        })
        .map_err(ApplicationError::command_failed)
        .and_then(|(playlist_id, positions)| {
            crate::bootstrap::application().execute_command(
                ApplicationCommand::RemovePlaylistTracks {
                    playlist_id,
                    positions,
                },
            )
        });
    publish_command_result(result)
}

// User-data commands.
#[no_mangle]
pub extern "C" fn mashin_core_set_user_data_favorites(
    media_items_json: *const c_char,
    is_favorite: i32,
) -> i32 {
    let result = read_required_c_string(media_items_json, "media_items_json")
        .and_then(|items| parse_media_items(&items))
        .map_err(ApplicationError::command_failed)
        .and_then(|media_items| {
            crate::bootstrap::application().execute_command(
                ApplicationCommand::SetUserDataFavorites {
                    media_items,
                    favorite: is_favorite != 0,
                },
            )
        });
    publish_command_result(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_create_private_playlist(name: *const c_char) -> i32 {
    let result = read_required_c_string(name, "name")
        .map_err(ApplicationError::command_failed)
        .and_then(|name| {
            crate::bootstrap::application()
                .execute_command(ApplicationCommand::CreatePrivatePlaylist { name })
        });
    publish_command_result(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_rename_private_playlist(
    playlist_id: *const c_char,
    name: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| {
            read_required_c_string(name, "name").map(|name| (playlist_id, name))
        })
        .map_err(ApplicationError::command_failed)
        .and_then(|(playlist_id, name)| {
            crate::bootstrap::application()
                .execute_command(ApplicationCommand::RenamePrivatePlaylist { playlist_id, name })
        });
    publish_command_result(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_remove_private_playlist(playlist_id: *const c_char) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .map_err(ApplicationError::command_failed)
        .and_then(|playlist_id| {
            crate::bootstrap::application()
                .execute_command(ApplicationCommand::RemovePrivatePlaylist { playlist_id })
        });
    publish_command_result(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_add_private_playlist_tracks(
    playlist_id: *const c_char,
    tracks_json: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| parse_tracks(tracks_json).map(|tracks| (playlist_id, tracks)))
        .map_err(ApplicationError::command_failed)
        .and_then(|(playlist_id, tracks)| {
            crate::bootstrap::application().execute_command(
                ApplicationCommand::AddPrivatePlaylistTracks {
                    playlist_id,
                    tracks,
                },
            )
        });
    publish_command_result(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_remove_private_playlist_tracks(
    playlist_id: *const c_char,
    track_uris_json: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| {
            read_required_uri_array(track_uris_json).map(|uris| (playlist_id, uris))
        })
        .map_err(ApplicationError::command_failed)
        .and_then(|(playlist_id, uris)| {
            crate::bootstrap::application().execute_command(
                ApplicationCommand::RemovePrivatePlaylistTracks { playlist_id, uris },
            )
        });
    publish_command_result(result)
}

// Queue commands.
#[no_mangle]
pub extern "C" fn mashin_core_queue_play_media(request_json: *const c_char) -> i32 {
    let request = read_required_c_string(request_json, "play media request")
        .map_err(ApplicationError::command_failed)
        .and_then(|value| {
            serde_json::from_str::<QueuePlayMediaRequest>(&value).map_err(|error| {
                ApplicationError::command_failed(format!("invalid play media request: {error}"))
            })
        });
    publish_command_result(request.and_then(|request| {
        crate::bootstrap::application()
            .execute_command(ApplicationCommand::QueuePlayMedia { request })
    }))
}

#[no_mangle]
pub extern "C" fn mashin_core_start_radio(media_items_json: *const c_char) -> i32 {
    match read_required_c_string(media_items_json, "media_items_json") {
        Ok(media_items) => match parse_media_items(&media_items) {
            Ok(media_items) => execute_command(ApplicationCommand::StartRadio { media_items }),
            Err(error) => {
                record_and_publish_error(error);
                -1
            }
        },
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_set_audio_profile(audio_profile: *const c_char) -> i32 {
    match read_required_c_string(audio_profile, "audioProfile") {
        Ok(audio_profile) => execute_command(ApplicationCommand::SetAudioProfile { audio_profile }),
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_move_queue_item(item_id: *const c_char, position_shift: i32) -> i32 {
    match read_required_c_string(item_id, "itemId") {
        Ok(item_id) => execute_command(ApplicationCommand::MoveQueueItem {
            item_id: QueueItemId(item_id),
            position_shift,
        }),
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_move_queue_item_to_end(item_id: *const c_char) -> i32 {
    match read_required_c_string(item_id, "itemId") {
        Ok(item_id) => execute_command(ApplicationCommand::MoveQueueItemToEnd {
            item_id: QueueItemId(item_id),
        }),
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_delete_queue_items(item_ids: *const c_char) -> i32 {
    match read_required_json_array::<QueueItemId>(item_ids, "itemIds") {
        Ok(item_ids) => execute_command(ApplicationCommand::DeleteQueueItems { item_ids }),
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_select_queue_item(item_id: *const c_char) -> i32 {
    match read_required_c_string(item_id, "queue_item_id") {
        Ok(item_id) => execute_command(ApplicationCommand::SelectQueueItem {
            item_id: QueueItemId(item_id),
        }),
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_queue_play(index: i32) -> i32 {
    if index < 0 {
        record_and_publish_error("queue index must not be negative".to_string());
        return -1;
    }
    execute_command(ApplicationCommand::PlayQueueIndex {
        index: index as usize,
    })
}

// Playback transport and preference commands.
#[no_mangle]
pub extern "C" fn mashin_core_transport_play() -> i32 {
    execute_command(ApplicationCommand::TransportPlay {})
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_pause() -> i32 {
    execute_command(ApplicationCommand::TransportPause {})
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_stop() -> i32 {
    execute_command(ApplicationCommand::TransportStop {})
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_next() -> i32 {
    execute_command(ApplicationCommand::TransportNext {})
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_previous() -> i32 {
    execute_command(ApplicationCommand::TransportPrevious {})
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_seek(seconds: f64) -> i32 {
    execute_command(ApplicationCommand::TransportSeek { seconds })
}

#[no_mangle]
pub extern "C" fn mashin_core_set_shuffle(is_enabled: i32) -> i32 {
    execute_command(ApplicationCommand::TransportShuffle {
        enabled: is_enabled != 0,
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_set_autoplay(is_enabled: i32) -> i32 {
    execute_command(ApplicationCommand::SetAutoplay {
        enabled: is_enabled != 0,
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_set_repeat_mode(mode: i32) -> i32 {
    match u8::try_from(mode) {
        Ok(mode) => execute_command(ApplicationCommand::TransportRepeatMode { mode }),
        Err(_) => {
            record_and_publish_error(ApplicationError::command_failed("invalid repeat mode"));
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_set_volume(volume: i32) -> i32 {
    execute_command(ApplicationCommand::SetVolume {
        volume: volume.clamp(0, 100) as u8,
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_set_muted(is_muted: i32) -> i32 {
    execute_command(ApplicationCommand::SetMuted {
        muted: is_muted != 0,
    })
}

// Command execution and platform event delivery.
fn execute_command(command: ApplicationCommand) -> i32 {
    publish_command_result(crate::bootstrap::application().execute_command(command))
}

fn publish_command_result(result: Result<CommandResult, ApplicationError>) -> i32 {
    match result {
        Ok(result) => {
            for event in result.events {
                publish_background_event(event);
            }
            0
        }
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

// JSON decoding.
fn parse_login_configuration(login_json: &str) -> Result<LoginConfig, String> {
    serde_json::from_str(login_json)
        .map_err(|error| format!("invalid core login configuration: {error}"))
}

fn parse_media_items(media_items_json: &str) -> Result<Vec<MediaItem>, String> {
    serde_json::from_str(media_items_json)
        .map_err(|error| format!("invalid Music Assistant media items: {error}"))
}

fn parse_tracks(tracks_json: *const c_char) -> Result<Vec<Track>, String> {
    let tracks_json = read_required_c_string(tracks_json, "tracks_json")?;
    serde_json::from_str(&tracks_json).map_err(|error| format!("invalid track array: {error}"))
}
