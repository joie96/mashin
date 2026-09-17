//! FFI callback registration and typed core-event delivery.

use crate::core::models::MediaSessionEvent;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::sync::Mutex;

type EventCallback = extern "C" fn(*mut c_char);

static EVENT_CALLBACK: Lazy<Mutex<Option<EventCallback>>> = Lazy::new(|| Mutex::new(None));

// Event callback registration.
#[no_mangle]
pub extern "C" fn mashin_core_set_event_callback(callback: Option<EventCallback>) -> i32 {
    match EVENT_CALLBACK.lock() {
        Ok(mut stored_callback) => {
            *stored_callback = callback;
            0
        }
        Err(_) => -1,
    }
}

pub(crate) fn publish_media_session_event(event: MediaSessionEvent) {
    match serde_json::to_string(&event) {
        Ok(text) => publish_json_text(&text),
        Err(error) => {
            crate::ffi::errors::record_and_publish_error(crate::core::models::CoreError::internal(
                format!("serialize media session event failed: {error}"),
            ))
        }
    }
}

pub(crate) fn publish_json_text(text: &str) {
    #[cfg(target_os = "android")]
    crate::jni::event_channel::publish_json_text(text);
    let callback = EVENT_CALLBACK.lock().ok().and_then(|callback| *callback);
    let Some(callback) = callback else {
        return;
    };
    let Ok(pointer) = CString::new(text) else {
        return;
    };
    callback(pointer.into_raw());
}

pub(crate) fn publish_json_value(value: Value) {
    if let Ok(text) = serde_json::to_string(&value) {
        publish_json_text(&text);
    }
}
