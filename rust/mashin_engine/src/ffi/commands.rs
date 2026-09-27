//! FFI commands and routing into the application core.

use super::errors::record_and_publish_error;
use super::event_channel::publish_media_session_event;
use super::strings::{read_required_c_string, read_required_json_array, read_required_uri_array};
use crate::core;
use crate::core::models::{
    CoreError, CoreOperationRequest, LoginConfig, MediaSessionEvent, OfflineOutputConfig,
    RemoteOutputConfig, SelectedOutput,
};
use crate::core::CoreCoordinator;
use crate::music_assistant::models::identifiers::PlayerId;
use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::{MediaItem, Track};
use crate::music_assistant::models::queue::RepeatMode;
use serde_json::{json, Value};
use std::ffi::c_char;

// Asynchronous Flutter operation dispatch.
#[no_mangle]
pub extern "C" fn mashin_core_start_operation(request_json: *const c_char) -> i32 {
    let request = match read_required_c_string(request_json, "operation request")
        .map_err(CoreError::command_failed)
        .and_then(|value| {
            serde_json::from_str::<CoreOperationRequest>(&value).map_err(|error| {
                CoreError::command_failed(format!("decode operation request failed: {error}"))
            })
        }) {
        Ok(request) => request,
        Err(error) => {
            record_and_publish_error(error);
            return -1;
        }
    };

    if is_music_assistant_query(&request.kind) {
        let mut query_payload = request.arguments.clone();
        query_payload.insert("requestId".to_string(), json!(request.request_id));
        query_payload.insert("kind".to_string(), json!(request.kind));
        return match serde_json::from_value(
            query_payload
                .into_iter()
                .collect::<serde_json::Map<_, _>>()
                .into(),
        )
        .map_err(|error| {
            CoreError::command_failed(format!("decode query operation failed: {error}"))
        })
        .and_then(core::start_music_assistant_query)
        {
            Ok(()) => 0,
            Err(error) => {
                record_and_publish_error(error);
                -1
            }
        };
    }

    core::spawn_blocking_operation(move || {
        let request_id = request.request_id;
        match execute_async_operation(request) {
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

fn is_music_assistant_query(kind: &str) -> bool {
    matches!(
        kind,
        "recommendations"
            | "providerManifests"
            | "providerIconData"
            | "recentlyPlayedTracks"
            | "searchMedia"
            | "artist"
            | "album"
            | "albumTracks"
            | "playlist"
            | "playlistTracks"
            | "artistAlbums"
            | "artistTopTracks"
            | "similarArtists"
            | "similarTracks"
            | "trackVersions"
    )
}

fn execute_async_operation(request: CoreOperationRequest) -> Result<Value, CoreError> {
    let arguments = request.arguments;
    match request.kind.as_str() {
        "login" => execute_json_command(mashin_core_login, &arguments),
        "restorePersistedSession" => execute_restore_persisted_session(),
        "selectDefaultSendspinOutput" => {
            execute_void_command(mashin_core_select_default_sendspin_output)
        }
        "selectOfflineLibrary" => execute_text_command(
            mashin_core_select_offline_library,
            &arguments,
            "libraryRoot",
        ),
        "selectRemotePlayer" => {
            execute_text_command(mashin_core_select_remote_player, &arguments, "playerId")
        }
        "activateEngine" => execute_void_command(mashin_core_activate_engine),
        "deactivateEngine" => execute_void_command(mashin_core_deactivate_engine),
        "shutdown" => execute_void_command(mashin_core_shutdown),
        "transportPlay" => execute_void_command(mashin_core_transport_play),
        "transportPause" => execute_void_command(mashin_core_transport_pause),
        "transportStop" => execute_void_command(mashin_core_transport_stop),
        "transportNext" => execute_void_command(mashin_core_transport_next),
        "transportPrevious" => execute_void_command(mashin_core_transport_previous),
        "transportSeek" => {
            execute_number_command(mashin_core_transport_seek, &arguments, "seconds")
        }
        "transportRepeatMode" => {
            execute_integer_command(mashin_core_set_repeat_mode, &arguments, "mode")
        }
        "transportShuffle" => {
            execute_boolean_command(mashin_core_set_shuffle, &arguments, "enabled")
        }
        "setAutoplay" => execute_boolean_command(mashin_core_set_autoplay, &arguments, "enabled"),
        "setVolume" => execute_integer_command(mashin_core_set_volume, &arguments, "volume"),
        "setMuted" => execute_boolean_command(mashin_core_set_muted, &arguments, "muted"),
        "setTheme" => execute_text_command(mashin_core_set_theme, &arguments, "theme"),
        "setLogLevel" => {
            execute_text_command(mashin_core_set_session_log_level, &arguments, "logLevel")
        }
        "saveLogSnapshot" => serialize_operation_result(
            crate::logging::save_log_snapshot().map_err(CoreError::internal),
        ),
        "playMedia" => {
            execute_json_text_command(mashin_core_queue_replace, &arguments, "mediaItems")
        }
        "playShuffledMedia" => {
            execute_json_text_command(mashin_core_queue_replace_shuffled, &arguments, "mediaItems")
        }
        "startSendspinRadio" => {
            execute_json_text_command(mashin_core_start_sendspin_radio, &arguments, "mediaItems")
        }
        "playMediaNext" => {
            execute_json_text_command(mashin_core_queue_insert_next, &arguments, "mediaItems")
        }
        "playMediaReplaceNext" => {
            execute_json_text_command(mashin_core_queue_replace_next, &arguments, "mediaItems")
        }
        "playMediaLast" => {
            execute_json_text_command(mashin_core_queue_append, &arguments, "mediaItems")
        }
        "clearQueue" => execute_void_command(mashin_core_queue_clear),
        "moveQueueItem" => {
            execute_text_integer_command(mashin_core_queue_move, &arguments, "itemId", "shift")
        }
        "deleteQueueItem" => execute_text_command(mashin_core_queue_remove, &arguments, "itemId"),
        "playQueueIndex" => execute_integer_command(mashin_core_queue_play, &arguments, "index"),
        "requestCodec" => execute_text_command(mashin_core_set_codec, &arguments, "codec"),
        "setLocalCacheSizeBytes" => execute_integer_command(
            mashin_core_set_local_cache_size_bytes,
            &arguments,
            "cacheSizeBytes",
        ),
        "addPlaylistTracks" => execute_two_json_text_command(
            mashin_core_add_playlist_tracks,
            &arguments,
            "playlistId",
            "uris",
        ),
        "removePlaylistTracks" => execute_two_json_text_command(
            mashin_core_remove_playlist_tracks,
            &arguments,
            "playlistId",
            "positions",
        ),
        "setUserDataFavorites" => execute_json_boolean_command(
            mashin_core_set_user_data_favorites,
            &arguments,
            "mediaItems",
            "favorite",
        ),
        "createLocalPlaylist" => {
            execute_text_command(mashin_core_create_local_playlist, &arguments, "name")
        }
        "renameLocalPlaylist" => execute_two_text_command(
            mashin_core_rename_local_playlist,
            &arguments,
            "playlistId",
            "name",
        ),
        "removeLocalPlaylist" => {
            execute_text_command(mashin_core_remove_local_playlist, &arguments, "playlistId")
        }
        "addLocalPlaylistTracks" => execute_two_json_text_command(
            mashin_core_add_local_playlist_tracks,
            &arguments,
            "playlistId",
            "tracks",
        ),
        "removeLocalPlaylistTracks" => execute_two_json_text_command(
            mashin_core_remove_local_playlist_tracks,
            &arguments,
            "playlistId",
            "uris",
        ),
        "getSessionSettings" => serialize_operation_result(core::read_session_settings()),
        "refreshQueue" => serialize_operation_result(core::read_queue()),
        "refreshPlayerState" => serialize_operation_result(core::read_player_state()),
        "loadUserData" => serialize_operation_result(core::load_user_data()),
        "getUserData" => serialize_operation_result(core::read_user_data()),
        _ => Err(CoreError::command_failed(format!(
            "unsupported async operation '{}'",
            request.kind
        ))),
    }
}

fn serialize_operation_result<T: serde::Serialize>(
    result: Result<T, CoreError>,
) -> Result<Value, CoreError> {
    result.and_then(|value| {
        serde_json::to_value(value).map_err(|error| {
            CoreError::internal(format!("serialize operation result failed: {error}"))
        })
    })
}

fn execute_restore_persisted_session() -> Result<Value, CoreError> {
    match mashin_core_restore_persisted_session() {
        0 => Ok(Value::Bool(true)),
        1 => Ok(Value::Bool(false)),
        _ => Err(CoreError::command_failed(
            super::errors::last_error_message(),
        )),
    }
}

fn execute_void_command(command: extern "C" fn() -> i32) -> Result<Value, CoreError> {
    command_result(command())
}
fn execute_text_command(
    command: extern "C" fn(*const c_char) -> i32,
    arguments: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Value, CoreError> {
    with_text(arguments, key, |value| {
        command_result(command(value.as_ptr()))
    })
}
fn execute_json_command(
    command: extern "C" fn(*const c_char) -> i32,
    arguments: &serde_json::Map<String, Value>,
) -> Result<Value, CoreError> {
    with_json(arguments, |value| command_result(command(value.as_ptr())))
}
fn execute_json_text_command(
    command: extern "C" fn(*const c_char) -> i32,
    arguments: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Value, CoreError> {
    with_json_value(arguments, key, |value| {
        command_result(command(value.as_ptr()))
    })
}
fn execute_number_command(
    command: extern "C" fn(f64) -> i32,
    arguments: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Value, CoreError> {
    command_result(command(required_f64(arguments, key)?))
}
fn execute_integer_command(
    command: extern "C" fn(i32) -> i32,
    arguments: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Value, CoreError> {
    command_result(command(required_i32(arguments, key)?))
}
fn execute_boolean_command(
    command: extern "C" fn(i32) -> i32,
    arguments: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Value, CoreError> {
    command_result(command(if required_bool(arguments, key)? { 1 } else { 0 }))
}
fn execute_text_integer_command(
    command: extern "C" fn(*const c_char, i32) -> i32,
    arguments: &serde_json::Map<String, Value>,
    text_key: &str,
    integer_key: &str,
) -> Result<Value, CoreError> {
    with_text(arguments, text_key, |value| {
        command_result(command(
            value.as_ptr(),
            required_i32(arguments, integer_key)?,
        ))
    })
}
fn execute_json_boolean_command(
    command: extern "C" fn(*const c_char, i32) -> i32,
    arguments: &serde_json::Map<String, Value>,
    json_key: &str,
    boolean_key: &str,
) -> Result<Value, CoreError> {
    with_json_value(arguments, json_key, |value| {
        command_result(command(
            value.as_ptr(),
            if required_bool(arguments, boolean_key)? {
                1
            } else {
                0
            },
        ))
    })
}
fn execute_two_text_command(
    command: extern "C" fn(*const c_char, *const c_char) -> i32,
    arguments: &serde_json::Map<String, Value>,
    first_key: &str,
    second_key: &str,
) -> Result<Value, CoreError> {
    with_text(arguments, first_key, |first| {
        with_text(arguments, second_key, |second| {
            command_result(command(first.as_ptr(), second.as_ptr()))
        })
    })
}
fn execute_two_json_text_command(
    command: extern "C" fn(*const c_char, *const c_char) -> i32,
    arguments: &serde_json::Map<String, Value>,
    text_key: &str,
    json_key: &str,
) -> Result<Value, CoreError> {
    with_text(arguments, text_key, |text| {
        with_json_value(arguments, json_key, |json| {
            command_result(command(text.as_ptr(), json.as_ptr()))
        })
    })
}
fn command_result(code: i32) -> Result<Value, CoreError> {
    if code == 0 {
        Ok(Value::Null)
    } else {
        Err(CoreError::command_failed(
            super::errors::last_error_message(),
        ))
    }
}
fn with_text<T>(
    arguments: &serde_json::Map<String, Value>,
    key: &str,
    operation: impl FnOnce(std::ffi::CString) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let value = arguments.get(key).and_then(Value::as_str).ok_or_else(|| {
        CoreError::command_failed(format!("operation argument '{key}' must be a string"))
    })?;
    let value = std::ffi::CString::new(value).map_err(|_| {
        CoreError::command_failed(format!("operation argument '{key}' contains a null byte"))
    })?;
    operation(value)
}
fn with_json<T>(
    arguments: &serde_json::Map<String, Value>,
    operation: impl FnOnce(std::ffi::CString) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let value = serde_json::to_string(arguments).map_err(|error| {
        CoreError::internal(format!("serialize operation arguments failed: {error}"))
    })?;
    let value = std::ffi::CString::new(value)
        .map_err(|_| CoreError::command_failed("operation arguments contain a null byte"))?;
    operation(value)
}
fn with_json_value<T>(
    arguments: &serde_json::Map<String, Value>,
    key: &str,
    operation: impl FnOnce(std::ffi::CString) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let value = arguments
        .get(key)
        .ok_or_else(|| CoreError::command_failed(format!("missing operation argument '{key}'")))?;
    let value = serde_json::to_string(value).map_err(|error| {
        CoreError::internal(format!("serialize operation argument failed: {error}"))
    })?;
    let value = std::ffi::CString::new(value).map_err(|_| {
        CoreError::command_failed(format!("operation argument '{key}' contains a null byte"))
    })?;
    operation(value)
}
fn required_i32(arguments: &serde_json::Map<String, Value>, key: &str) -> Result<i32, CoreError> {
    arguments
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| {
            CoreError::command_failed(format!("operation argument '{key}' must be an integer"))
        })
}
fn required_f64(arguments: &serde_json::Map<String, Value>, key: &str) -> Result<f64, CoreError> {
    arguments.get(key).and_then(Value::as_f64).ok_or_else(|| {
        CoreError::command_failed(format!("operation argument '{key}' must be a number"))
    })
}
fn required_bool(arguments: &serde_json::Map<String, Value>, key: &str) -> Result<bool, CoreError> {
    arguments.get(key).and_then(Value::as_bool).ok_or_else(|| {
        CoreError::command_failed(format!("operation argument '{key}' must be a boolean"))
    })
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
    execute_core_command(|coordinator| coordinator.authenticate_and_configure_session(login))
}

#[no_mangle]
pub extern "C" fn mashin_core_restore_persisted_session() -> i32 {
    match core::restore_persisted_session_and_collect_events() {
        Ok((has_restored_session, events)) => {
            for event in events {
                publish_media_session_event(event);
            }
            if has_restored_session {
                0
            } else {
                1
            }
        }
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_select_default_sendspin_output() -> i32 {
    execute_core_command(|coordinator| coordinator.select_default_sendspin_output())
}

#[no_mangle]
pub extern "C" fn mashin_core_select_offline_library(library_root: *const c_char) -> i32 {
    let library_root = match read_required_c_string(library_root, "library_root") {
        Ok(value) => value,
        Err(error) => {
            record_and_publish_error(error);
            return -1;
        }
    };
    execute_core_command(|coordinator| {
        coordinator.select_output(SelectedOutput::Offline(OfflineOutputConfig {
            library_root,
        }))
    })
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
    execute_core_command(|coordinator| {
        coordinator.select_output(SelectedOutput::Remote(RemoteOutputConfig {
            player_id: PlayerId(player_id),
        }))
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_activate_engine() -> i32 {
    execute_core_command(|coordinator| coordinator.activate_selected_engine())
}

#[no_mangle]
pub extern "C" fn mashin_core_deactivate_engine() -> i32 {
    execute_core_command(|coordinator| {
        coordinator.deactivate_active_engine();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_shutdown() -> i32 {
    execute_playlist_mutation(core::shutdown())
}

// Playlist mutation commands.
#[no_mangle]
pub extern "C" fn mashin_core_add_playlist_tracks(
    playlist_id: *const c_char,
    uris_json: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| read_required_uri_array(uris_json).map(|uris| (playlist_id, uris)))
        .map_err(CoreError::command_failed)
        .and_then(|(playlist_id, uris)| core::add_playlist_tracks(playlist_id, uris));
    execute_playlist_mutation(result)
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
        .map_err(CoreError::command_failed)
        .and_then(|(playlist_id, positions)| core::remove_playlist_tracks(playlist_id, positions));
    execute_playlist_mutation(result)
}

fn execute_playlist_mutation(result: Result<(), CoreError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

// User-data commands.
#[no_mangle]
pub extern "C" fn mashin_core_set_user_data_favorites(
    media_items_json: *const c_char,
    is_favorite: i32,
) -> i32 {
    let result = read_required_c_string(media_items_json, "media_items_json")
        .and_then(|items| parse_media_items(&items))
        .map_err(CoreError::command_failed)
        .and_then(|items| {
            core::mutate_user_data(|user_data| {
                user_data.set_favorites(items, is_favorite != 0);
                Ok(())
            })
        });
    execute_user_data_mutation(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_create_local_playlist(name: *const c_char) -> i32 {
    let result = read_required_c_string(name, "name")
        .map_err(CoreError::command_failed)
        .and_then(|name| {
            core::mutate_user_data(|user_data| user_data.create_local_playlist(name).map(|_| ()))
        });
    execute_user_data_mutation(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_rename_local_playlist(
    playlist_id: *const c_char,
    name: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| {
            read_required_c_string(name, "name").map(|name| (playlist_id, name))
        })
        .map_err(CoreError::command_failed)
        .and_then(|(playlist_id, name)| {
            core::mutate_user_data(|user_data| user_data.rename_local_playlist(&playlist_id, name))
        });
    execute_user_data_mutation(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_remove_local_playlist(playlist_id: *const c_char) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .map_err(CoreError::command_failed)
        .and_then(|playlist_id| {
            core::mutate_user_data(|user_data| user_data.remove_local_playlist(&playlist_id))
        });
    execute_user_data_mutation(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_add_local_playlist_tracks(
    playlist_id: *const c_char,
    tracks_json: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| parse_tracks(tracks_json).map(|tracks| (playlist_id, tracks)))
        .map_err(CoreError::command_failed)
        .and_then(|(playlist_id, tracks)| {
            core::mutate_local_playlist_tracks(&playlist_id, false, |user_data| {
                user_data.add_local_playlist_tracks(&playlist_id, tracks)
            })
        });
    execute_user_data_mutation(result)
}

#[no_mangle]
pub extern "C" fn mashin_core_remove_local_playlist_tracks(
    playlist_id: *const c_char,
    track_uris_json: *const c_char,
) -> i32 {
    let result = read_required_c_string(playlist_id, "playlist_id")
        .and_then(|playlist_id| {
            read_required_uri_array(track_uris_json).map(|uris| (playlist_id, uris))
        })
        .map_err(CoreError::command_failed)
        .and_then(|(playlist_id, uris)| {
            core::mutate_local_playlist_tracks(&playlist_id, true, |user_data| {
                user_data.remove_local_playlist_tracks(&playlist_id, uris)
            })
        });
    execute_user_data_mutation(result)
}

fn execute_user_data_mutation(
    result: Result<crate::music_assistant::user_data::UserDataSnapshot, CoreError>,
) -> i32 {
    match result {
        Ok(_) => 0,
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

// Queue commands.
#[no_mangle]
pub extern "C" fn mashin_core_queue_replace(media_items_json: *const c_char) -> i32 {
    match read_required_c_string(media_items_json, "media_items_json") {
        Ok(media_items) => match parse_media_items(&media_items) {
            Ok(media_items) => {
                resolve_media_and_execute_core_command(media_items, false, |coordinator, tracks| {
                    coordinator.play_media(tracks)
                })
            }
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

fn execute_queue_next_command(items_json: *const c_char, replace_following: bool) -> i32 {
    match read_required_c_string(items_json, "media_items_json") {
        Ok(items) => match parse_media_items(&items) {
            Ok(items) => {
                resolve_media_and_execute_core_command(items, false, |coordinator, tracks| {
                    coordinator.play_media_next(tracks, replace_following)
                })
            }
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
pub extern "C" fn mashin_core_queue_insert_next(items: *const c_char) -> i32 {
    execute_queue_next_command(items, false)
}

#[no_mangle]
pub extern "C" fn mashin_core_queue_replace_next(items: *const c_char) -> i32 {
    execute_queue_next_command(items, true)
}

#[no_mangle]
pub extern "C" fn mashin_core_queue_append(items: *const c_char) -> i32 {
    match read_required_c_string(items, "media_items_json") {
        Ok(items) => match parse_media_items(&items) {
            Ok(items) => {
                resolve_media_and_execute_core_command(items, false, |coordinator, tracks| {
                    coordinator.play_media_last(tracks)
                })
            }
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
pub extern "C" fn mashin_core_queue_replace_shuffled(media_items_json: *const c_char) -> i32 {
    match read_required_c_string(media_items_json, "media_items_json") {
        Ok(media_items) => match parse_media_items(&media_items) {
            Ok(media_items) => {
                resolve_media_and_execute_core_command(media_items, true, |coordinator, tracks| {
                    coordinator.play_media(tracks)
                })
            }
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
pub extern "C" fn mashin_core_start_sendspin_radio(media_items_json: *const c_char) -> i32 {
    match read_required_c_string(media_items_json, "media_items_json") {
        Ok(media_items) => match parse_media_items(&media_items) {
            Ok(media_items) => start_sendspin_radio_and_execute_core_command(media_items),
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
pub extern "C" fn mashin_core_set_codec(codec: *const c_char) -> i32 {
    match read_required_c_string(codec, "codec") {
        Ok(codec) => execute_core_command(|coordinator| coordinator.set_preferred_codec(&codec)),
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_set_local_cache_size_bytes(cache_size_bytes: i32) -> i32 {
    match usize::try_from(cache_size_bytes) {
        Ok(cache_size_bytes) => execute_core_command(|coordinator| {
            coordinator.set_local_cache_size_bytes(cache_size_bytes)
        }),
        Err(_) => {
            record_and_publish_error("local cache size must not be negative".to_string());
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_set_theme(theme: *const c_char) -> i32 {
    match read_required_c_string(theme, "theme") {
        Ok(theme) => match theme.as_str() {
            "dark" => execute_core_command(|coordinator| {
                coordinator.set_theme(crate::core::models::ThemePreference::Dark)
            }),
            "light" => execute_core_command(|coordinator| {
                coordinator.set_theme(crate::core::models::ThemePreference::Light)
            }),
            _ => {
                record_and_publish_error("theme must be 'dark' or 'light'".to_string());
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
pub extern "C" fn mashin_core_set_session_log_level(log_level: *const c_char) -> i32 {
    match read_required_c_string(log_level, "log_level") {
        Ok(log_level) => {
            let log_level = match log_level.as_str() {
                "trace" => crate::core::models::LogLevelPreference::Trace,
                "debug" => crate::core::models::LogLevelPreference::Debug,
                "info" => crate::core::models::LogLevelPreference::Info,
                "warn" => crate::core::models::LogLevelPreference::Warn,
                "error" => crate::core::models::LogLevelPreference::Error,
                _ => {
                    record_and_publish_error(
                        "log level must be trace, debug, info, warn, or error".to_string(),
                    );
                    return -1;
                }
            };
            execute_core_command(|coordinator| coordinator.set_log_level(log_level))
        }
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_queue_clear() -> i32 {
    execute_core_command(|coordinator| coordinator.clear_queue())
}

#[no_mangle]
pub extern "C" fn mashin_core_queue_move(item_id: *const c_char, shift: i32) -> i32 {
    match read_required_c_string(item_id, "queue_item_id") {
        Ok(item_id) => execute_core_command(|coordinator| {
            coordinator.move_queue_item(QueueItemId(item_id), shift)
        }),
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn mashin_core_queue_remove(item_id: *const c_char) -> i32 {
    match read_required_c_string(item_id, "queue_item_id") {
        Ok(item_id) => {
            execute_core_command(|coordinator| coordinator.delete_queue_item(QueueItemId(item_id)))
        }
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
    execute_core_command(|coordinator| coordinator.play_queue_index(index as usize))
}

// Playback transport and preference commands.
#[no_mangle]
pub extern "C" fn mashin_core_transport_play() -> i32 {
    execute_core_command(|coordinator| coordinator.transport_play())
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_pause() -> i32 {
    execute_core_command(|coordinator| {
        coordinator.transport_pause();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_stop() -> i32 {
    execute_core_command(|coordinator| {
        coordinator.transport_stop();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_next() -> i32 {
    execute_core_command(|coordinator| coordinator.transport_next())
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_previous() -> i32 {
    execute_core_command(|coordinator| coordinator.transport_previous())
}

#[no_mangle]
pub extern "C" fn mashin_core_transport_seek(seconds: f64) -> i32 {
    execute_core_command(|coordinator| coordinator.transport_seek(seconds))
}

#[no_mangle]
pub extern "C" fn mashin_core_set_shuffle(is_enabled: i32) -> i32 {
    execute_core_command(|coordinator| {
        coordinator.set_shuffle(is_enabled != 0);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_set_autoplay(is_enabled: i32) -> i32 {
    execute_core_command(|coordinator| {
        coordinator.set_autoplay(is_enabled != 0);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_set_repeat_mode(mode: i32) -> i32 {
    let mode = match mode {
        1 => RepeatMode::One,
        2 => RepeatMode::All,
        _ => RepeatMode::Off,
    };
    execute_core_command(|coordinator| {
        coordinator.set_repeat_mode(mode);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn mashin_core_set_volume(volume: i32) -> i32 {
    execute_core_command(|coordinator| coordinator.set_volume(volume.clamp(0, 100) as u8))
}

#[no_mangle]
pub extern "C" fn mashin_core_set_muted(is_muted: i32) -> i32 {
    execute_core_command(|coordinator| {
        coordinator.set_muted(is_muted != 0);
        Ok(())
    })
}

// Core command execution and playback-event publication.
fn execute_core_command<F>(operation: F) -> i32
where
    F: FnOnce(&mut CoreCoordinator) -> Result<(), String>,
{
    publish_command_result(core::execute_command_and_collect_events(operation))
}

fn resolve_media_and_execute_core_command<F>(
    media_items: Vec<MediaItem>,
    should_shuffle: bool,
    operation: F,
) -> i32
where
    F: FnOnce(&mut CoreCoordinator, Vec<MediaItem>) -> Result<(), String>,
{
    publish_command_result(core::resolve_media_and_execute_command(
        media_items,
        should_shuffle,
        operation,
    ))
}

fn start_sendspin_radio_and_execute_core_command(media_items: Vec<MediaItem>) -> i32 {
    publish_command_result(core::start_sendspin_radio_and_execute_command(
        media_items,
        |events| {
            for event in events {
                publish_media_session_event(event);
            }
        },
    ))
}

fn publish_command_result(result: Result<Vec<MediaSessionEvent>, CoreError>) -> i32 {
    match result {
        Ok(events) => {
            for event in events {
                publish_media_session_event(event);
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
