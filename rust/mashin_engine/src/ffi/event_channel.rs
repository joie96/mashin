//! FFI callback registration and typed core-event delivery.

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

pub(crate) fn publish_json_text(text: &str) {
    publish_flutter_json_text(text);
}

pub(crate) fn publish_flutter_json_text(text: &str) {
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

// Rust logs are intended for Flutter only and must not reach Android's media-session callback.
pub(crate) fn publish_flutter_log_event(line: &str) -> bool {
    let callback = EVENT_CALLBACK.lock().ok().and_then(|callback| *callback);
    let Some(callback) = callback else {
        return false;
    };
    let text = json!({
        "type": "log",
        "line": line,
    })
    .to_string();
    let Ok(pointer) = CString::new(text) else {
        return false;
    };
    callback(pointer.into_raw());
    true
}
