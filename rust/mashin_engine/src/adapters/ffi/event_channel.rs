//! Flutter owns the listener trampoline; Rust registration and invocation share a lifetime gate.
//! Unregistration waits for native calls to return before Flutter may destroy its engine.
//! Dart listener processing is asynchronous and must not reenter this gate from the native call.

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::sync::Mutex;

type EventCallback = extern "C" fn(*mut c_char);

static FLUTTER_EVENT_CHANNEL: Lazy<FlutterEventChannel> = Lazy::new(FlutterEventChannel::new);

struct FlutterEventChannel {
    callback: Mutex<Option<EventCallback>>,
}

impl FlutterEventChannel {
    // Listener registration and lifetime.
    fn new() -> Self {
        Self {
            callback: Mutex::new(None),
        }
    }

    fn set_callback(&self, callback: Option<EventCallback>) -> Result<(), &'static str> {
        let mut stored_callback = self
            .callback
            .lock()
            .map_err(|_| "Flutter callback lock poisoned")?;
        *stored_callback = callback;
        Ok(())
    }

    // Hold the gate through the synchronous trampoline, not through asynchronous Dart processing.
    fn publish_json_text(&self, text: &str) -> bool {
        let Ok(pointer) = CString::new(text) else {
            return false;
        };
        let Ok(stored_callback) = self.callback.lock() else {
            return false;
        };
        let Some(callback) = *stored_callback else {
            return false;
        };
        callback(pointer.into_raw());
        true
    }

    fn publish_log_event(&self, line: &str) -> bool {
        self.publish_json_text(&json!({ "type": "log", "line": line }).to_string())
    }
}

// Event callback registration.
#[no_mangle]
pub extern "C" fn mashin_core_set_event_callback(callback: Option<EventCallback>) -> i32 {
    if callback.is_some() {
        crate::bootstrap::initialize_logging();
    }
    FLUTTER_EVENT_CHANNEL
        .set_callback(callback)
        .map_or(-1, |_| 0)
}

// Android must revoke the listener before destroying Flutter, without stopping service-owned playback.
#[cfg(target_os = "android")]
pub(crate) fn unregister_flutter_event_callback() -> Result<(), &'static str> {
    FLUTTER_EVENT_CHANNEL.set_callback(None)
}

pub(crate) fn publish_json_text(text: &str) {
    publish_flutter_json_text(text);
}

pub(crate) fn publish_flutter_json_text(text: &str) {
    FLUTTER_EVENT_CHANNEL.publish_json_text(text);
}

pub(crate) fn publish_json_value(value: Value) {
    if let Ok(text) = serde_json::to_string(&value) {
        publish_json_text(&text);
    }
}

// Rust logs are intended for Flutter only and must not reach Android's media-session callback.
pub(crate) fn publish_flutter_log_event(line: &str) -> bool {
    FLUTTER_EVENT_CHANNEL.publish_log_event(line)
}
