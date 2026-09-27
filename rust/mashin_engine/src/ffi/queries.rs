//! FFI data queries and routing into the application core.

use super::errors::record_and_publish_error;
use super::owned_strings::create_owned_string;
use super::strings::read_required_c_string;
use crate::core;
use crate::core::models::CoreError;
use crate::core::models::MusicAssistantQueryRequest;
use serde::Serialize;
use std::ffi::c_char;

// Initial state reads.
#[no_mangle]
pub extern "C" fn mashin_core_get_queue() -> *mut c_char {
    serialize_query_response("get queue", core::read_queue())
}

#[no_mangle]
pub extern "C" fn mashin_core_get_player_state() -> *mut c_char {
    serialize_query_response("get player state", core::read_player_state())
}

#[no_mangle]
pub extern "C" fn mashin_core_get_session_settings() -> *mut c_char {
    serialize_query_response("get session settings", core::read_session_settings())
}

#[no_mangle]
pub extern "C" fn mashin_core_load_user_data() -> *mut c_char {
    serialize_query_response("load user data", core::read_or_load_user_data())
}

#[no_mangle]
pub extern "C" fn mashin_core_get_user_data() -> *mut c_char {
    serialize_query_response("get user data", core::read_user_data())
}

// Music Assistant queries are dispatched asynchronously and return through the event channel.
#[no_mangle]
pub extern "C" fn mashin_core_start_music_assistant_query(request: *const c_char) -> i32 {
    let result = read_required_c_string(request, "query request")
        .map_err(CoreError::command_failed)
        .and_then(|request| {
            serde_json::from_str::<MusicAssistantQueryRequest>(&request).map_err(|error| {
                CoreError::command_failed(format!("decode query request failed: {error}"))
            })
        })
        .and_then(core::start_music_assistant_query);
    match result {
        Ok(()) => 0,
        Err(error) => {
            record_and_publish_error(error);
            -1
        }
    }
}

fn serialize_query_response<Response>(
    operation: &str,
    result: Result<Response, CoreError>,
) -> *mut c_char
where
    Response: Serialize,
{
    match result.and_then(|response| {
        serde_json::to_string(&response)
            .map_err(|error| CoreError::internal(format!("serialize {operation} failed: {error}")))
    }) {
        Ok(response) => create_owned_string(response),
        Err(error) => {
            record_and_publish_error(error);
            std::ptr::null_mut()
        }
    }
}
