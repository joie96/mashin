//! FFI commands and routing into the application core.

use super::errors::record_and_publish_error;
use super::event_channel::publish_media_session_event;
use super::strings::{read_required_c_string, read_required_json_array, read_required_uri_array};
use crate::core;
use crate::core::models::{
    CoreError, LoginConfig, MediaSessionEvent, OfflineOutputConfig, RemoteOutputConfig,
    SelectedOutput,
};
use crate::core::CoreCoordinator;
use crate::music_assistant::models::identifiers::PlayerId;
use crate::music_assistant::models::identifiers::QueueItemId;
use crate::music_assistant::models::media::{MediaItem, Track};
use crate::music_assistant::models::queue::RepeatMode;
use std::ffi::c_char;

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
            core::mutate_user_data(|user_data| {
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
            core::mutate_user_data(|user_data| {
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
pub extern "C" fn mashin_core_sendspin_queue_radio_next(media_items_json: *const c_char) -> i32 {
    match read_required_c_string(media_items_json, "media_items_json") {
        Ok(media_items) => match parse_media_items(&media_items) {
            Ok(media_items) => resolve_sendspin_radio_and_execute_core_command(media_items),
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

fn resolve_sendspin_radio_and_execute_core_command(media_items: Vec<MediaItem>) -> i32 {
    publish_command_result(core::resolve_sendspin_radio_and_execute_command(
        media_items,
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
