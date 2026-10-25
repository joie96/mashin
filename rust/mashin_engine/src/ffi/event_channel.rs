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
        crate::logging::initialize();
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

#[cfg(test)]
mod tests {
    use super::{c_char, CString, FlutterEventChannel, Mutex};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    static CALLBACK_COUNT: AtomicUsize = AtomicUsize::new(0);
    static CALLBACK_SIGNALS: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> =
        Mutex::new(None);

    extern "C" fn recording_callback(pointer: *mut c_char) {
        unsafe {
            drop(CString::from_raw(pointer));
        }
        CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    extern "C" fn blocking_callback(pointer: *mut c_char) {
        recording_callback(pointer);
        if let Some((entered, release)) = CALLBACK_SIGNALS.lock().unwrap().take() {
            let _ = entered.send(());
            let _ = release.recv_timeout(Duration::from_secs(5));
        }
    }

    #[test]
    fn unregister_waits_for_delivery_and_allows_a_new_listener() {
        for is_log_event in [false, true] {
            CALLBACK_COUNT.store(0, Ordering::SeqCst);
            let channel = Arc::new(FlutterEventChannel::new());
            let (entered_sender, entered_receiver) = mpsc::channel();
            let (release_sender, release_receiver) = mpsc::channel();
            *CALLBACK_SIGNALS.lock().unwrap() = Some((entered_sender, release_receiver));
            channel.set_callback(Some(blocking_callback)).unwrap();
            let publishing_channel = Arc::clone(&channel);
            let publisher = thread::spawn(move || {
                if is_log_event {
                    publishing_channel.publish_log_event("shutdown")
                } else {
                    publishing_channel.publish_json_text("{}")
                }
            });
            entered_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(
                channel.callback.try_lock().is_err(),
                "invocation must retain the lifetime gate"
            );
            let (started_sender, started_receiver) = mpsc::channel();
            let (finished_sender, finished_receiver) = mpsc::channel();
            let unregistering_channel = Arc::clone(&channel);
            let unregister = thread::spawn(move || {
                started_sender.send(()).unwrap();
                unregistering_channel.set_callback(None).unwrap();
                finished_sender.send(()).unwrap();
            });
            started_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(finished_receiver.try_recv().is_err());
            release_sender.send(()).unwrap();
            assert!(publisher.join().unwrap());
            unregister.join().unwrap();
            finished_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(!channel.publish_json_text("{}"));
            assert!(!channel.publish_log_event("after engine detach"));
            assert_eq!(CALLBACK_COUNT.load(Ordering::SeqCst), 1);
            channel.set_callback(Some(recording_callback)).unwrap();
            assert!(channel.publish_log_event("new engine"));
            assert_eq!(CALLBACK_COUNT.load(Ordering::SeqCst), 2);
            channel.set_callback(None).unwrap();
        }
    }

    #[test]
    fn unregistered_or_invalid_payloads_are_not_delivered() {
        let channel = FlutterEventChannel::new();
        assert!(!channel.publish_json_text("{}"));
        assert!(!channel.publish_log_event("without Flutter"));
        channel.set_callback(Some(recording_callback)).unwrap();
        assert!(!channel.publish_json_text("invalid\0payload"));
        channel.set_callback(None).unwrap();
    }
}
