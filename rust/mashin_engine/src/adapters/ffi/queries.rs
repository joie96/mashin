//! FFI data queries and routing into the application core.

use super::errors::record_and_publish_error;
use super::owned_strings::create_owned_string;
use crate::application::errors::ApplicationError;
use crate::application::queries::ApplicationQuery;
use serde::Serialize;
use std::ffi::c_char;

// Initial state reads.
#[no_mangle]
pub extern "C" fn mashin_core_get_queue() -> *mut c_char {
    serialize_query_response(
        "get queue",
        crate::bootstrap::application().execute_query_sync(ApplicationQuery::RefreshQueue {}),
    )
}

#[no_mangle]
pub extern "C" fn mashin_core_get_player_state() -> *mut c_char {
    serialize_query_response(
        "get player state",
        crate::bootstrap::application().execute_query_sync(ApplicationQuery::RefreshPlayerState {}),
    )
}

#[no_mangle]
pub extern "C" fn mashin_core_get_session_settings() -> *mut c_char {
    serialize_query_response(
        "get session settings",
        crate::bootstrap::application().execute_query_sync(ApplicationQuery::GetSessionSettings {}),
    )
}

#[no_mangle]
pub extern "C" fn mashin_core_load_user_data() -> *mut c_char {
    serialize_query_response(
        "load user data",
        crate::bootstrap::application().execute_query_sync(ApplicationQuery::LoadUserData {}),
    )
}

#[no_mangle]
pub extern "C" fn mashin_core_get_user_data() -> *mut c_char {
    serialize_query_response(
        "get user data",
        crate::bootstrap::application().execute_query_sync(ApplicationQuery::GetUserData {}),
    )
}

fn serialize_query_response<Response>(
    operation: &str,
    result: Result<Response, ApplicationError>,
) -> *mut c_char
where
    Response: Serialize,
{
    match result.and_then(|response| {
        serde_json::to_string(&response).map_err(|error| {
            ApplicationError::internal(format!("serialize {operation} failed: {error}"))
        })
    }) {
        Ok(response) => create_owned_string(response),
        Err(error) => {
            record_and_publish_error(error);
            std::ptr::null_mut()
        }
    }
}
