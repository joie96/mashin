//! Flutter owns the listener trampoline; Rust registration and invocation share a lifetime gate.
//! Unregistration waits for native calls to return before Flutter may destroy its engine.
//! Dart listener processing is asynchronous and must not reenter this gate from the native call.

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::sync::Mutex;

type EventCallback = extern "C" fn(*mut c_char, *mut u8, usize);

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
        self.publish(text, None)
    }

    fn publish(&self, text: &str, bytes: Option<Vec<u8>>) -> bool {
        let Ok(pointer) = CString::new(text) else {
            return false;
        };
        let Ok(stored_callback) = self.callback.lock() else {
            return false;
        };
        let Some(callback) = *stored_callback else {
            return false;
        };
        let (bytes, length) = match bytes {
            Some(bytes) => {
                let bytes = bytes.into_boxed_slice();
                let length = bytes.len();
                (Box::into_raw(bytes) as *mut u8, length)
            }
            None => (std::ptr::null_mut(), 0),
        };
        callback(pointer.into_raw(), bytes, length);
        true
    }

    fn publish_log_event(&self, line: &str) -> bool {
        self.publish_json_text(&json!({ "type": "log", "line": line }).to_string())
    }

    fn publish_image_snapshot(&self, mut identity: Value, snapshot: crate::application::queries::ImageReadSnapshot) -> bool {
        let field = if identity["type"] == "query_completed" { "result" } else { "snapshot" };
        identity[field] = json!({"bytes": null, "cacheError": snapshot.cache_error, "hasRefresh": snapshot.has_refresh});
        self.publish(&identity.to_string(), snapshot.bytes)
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

/// The callback transfers this allocation to Dart; release it exactly once after copying.
#[no_mangle]
pub unsafe extern "C" fn mashin_core_free_owned_bytes(pointer: *mut u8, length: usize) {
    if !pointer.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(pointer, length)) });
    }
}

pub(crate) fn publish_image_snapshot(
    identity: Value,
    snapshot: crate::application::queries::ImageReadSnapshot,
) {
    FLUTTER_EVENT_CHANNEL.publish_image_snapshot(identity, snapshot);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    thread_local! { static RECEIVED: RefCell<Vec<(Value, Option<Vec<u8>>)>> = const { RefCell::new(Vec::new()) }; }

    extern "C" fn receive(text: *mut c_char, bytes: *mut u8, length: usize) {
        let text = unsafe { CString::from_raw(text) };
        let image = if bytes.is_null() { None } else {
            let image = unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec();
            unsafe { mashin_core_free_owned_bytes(bytes, length); }
            Some(image)
        };
        RECEIVED.with(|received| received.borrow_mut().push((serde_json::from_slice(text.as_bytes()).unwrap(), image)));
    }

    #[test]
    fn image_callbacks_transfer_binary_ownership_without_base64_and_preserve_jni_json() {
        let channel = FlutterEventChannel::new();
        channel.set_callback(Some(receive)).unwrap();
        for bytes in [Some(vec![0, 1, 255]), Some(Vec::new()), None] {
            let snapshot = crate::application::queries::ImageReadSnapshot {
                did_publish_change: false, bytes: bytes.clone(), cache_error: Some("cache failure".into()), has_refresh: true,
            };
            let jni = serde_json::to_value(&snapshot).unwrap();
            assert_eq!(jni["bytes"], match &bytes { Some(bytes) => {
                use base64::Engine;
                json!(base64::engine::general_purpose::STANDARD.encode(bytes))
            }, None => Value::Null });
            assert!(channel.publish_image_snapshot(json!({"type":"query_completed","requestId":7}), snapshot));
            let (metadata, actual) = RECEIVED.with(|received| received.borrow_mut().pop().unwrap());
            assert_eq!(metadata["result"]["bytes"], Value::Null);
            assert_eq!(metadata["result"]["cacheError"], "cache failure");
            assert_eq!(actual, bytes);
        }
        channel.set_callback(None).unwrap();
        assert!(!channel.publish("{}", Some(vec![1])));
    }
}
