//! FFI error retention and publication through the core event channel.

use super::event_channel::publish_json_value;
use super::owned_strings::create_owned_string;
use crate::core::models::CoreError;
use once_cell::sync::Lazy;
use serde_json::json;
use std::ffi::c_char;
use std::sync::Mutex;

static LAST_ERROR: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

// Error reads and event publication.
#[no_mangle]
pub extern "C" fn mashin_core_get_last_error() -> *mut c_char {
    let error = LAST_ERROR
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| "last error lock poisoned".to_string());
    create_owned_string(error)
}

pub(crate) fn record_and_publish_error(error: impl Into<CoreError>) {
    let error = error.into();
    if let Ok(mut last_error) = LAST_ERROR.lock() {
        *last_error = error.message.clone();
    }
    publish_json_value(json!({
        "type": "core_error",
        "error": error,
    }));
}
