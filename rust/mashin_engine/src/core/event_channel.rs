//! Media-session events are routed by origin to the independent platform listeners.

use crate::core::models::MediaSessionEvent;

// Kotlin reads the new snapshot from the synchronous JNI command result but needs to inform Flutter asynchronously.
pub(crate) fn publish_jni_command_events(events: Vec<MediaSessionEvent>) {
    for event in events {
        publish_event(
            &event,
            false,
            publish_android,
            crate::ffi::event_channel::publish_flutter_json_text,
        );
    }
}

// Background changes have no command caller to refresh either platform.
pub(crate) fn publish_background_event(event: MediaSessionEvent) {
    publish_event(
        &event,
        true,
        publish_android,
        crate::ffi::event_channel::publish_flutter_json_text,
    );
}

fn publish_android(text: &str) {
    #[cfg(not(target_os = "android"))]
    let _ = text;
    #[cfg(target_os = "android")]
    crate::jni::event_channel::publish_json_text(text);
}

fn publish_event(
    event: &MediaSessionEvent,
    should_publish_to_android: bool,
    publish_to_android: impl FnOnce(&str),
    publish_to_flutter: impl FnOnce(&str),
) {
    if let Ok(text) = serde_json::to_string(&event) {
        if should_publish_to_android {
            publish_to_android(&text);
        }
        publish_to_flutter(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::{publish_event, publish_jni_command_events};
    use crate::core::models::MediaSessionEvent;
    use crate::music_assistant::models::identifiers::QueueId;
    use crate::playback::models::{PlayerState, Queue};
    use once_cell::sync::Lazy;
    use std::cell::RefCell;
    use std::ffi::{c_char, CString};
    use std::sync::Mutex;

    static FLUTTER_EVENTS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

    extern "C" fn capture_flutter_event(pointer: *mut c_char) {
        let text = unsafe { CString::from_raw(pointer) };
        FLUTTER_EVENTS
            .lock()
            .unwrap()
            .push(text.to_string_lossy().into_owned());
    }

    // Platform event routing contracts.
    #[test]
    fn user_data_changes_reach_both_platform_listeners() {
        let android_messages = RefCell::new(Vec::new());
        let flutter_messages = RefCell::new(Vec::new());
        publish_event(
            &MediaSessionEvent::UserDataChanged,
            true,
            |text| android_messages.borrow_mut().push(text.to_string()),
            |text| flutter_messages.borrow_mut().push(text.to_string()),
        );
        assert_eq!(*android_messages.borrow(), vec![r#"{"type":"user_data_changed"}"#]);
        assert_eq!(*android_messages.borrow(), *flutter_messages.borrow());
    }

    #[test]
    fn jni_command_updates_only_flutter_while_background_updates_both() {
        let event = MediaSessionEvent::PlayerStateChanged {
            player_state: PlayerState::default(),
        };
        for (should_publish_to_android, expected_android_count) in [(false, 0), (true, 1)] {
            let android_messages = RefCell::new(Vec::new());
            let flutter_messages = RefCell::new(Vec::new());
            publish_event(
                &event,
                should_publish_to_android,
                |text| android_messages.borrow_mut().push(text.to_string()),
                |text| flutter_messages.borrow_mut().push(text.to_string()),
            );
            assert_eq!(android_messages.borrow().len(), expected_android_count);
            assert_eq!(flutter_messages.borrow().len(), 1);
            if should_publish_to_android {
                assert_eq!(*android_messages.borrow(), *flutter_messages.borrow());
            }
        }
    }

    #[test]
    fn jni_command_event_reaches_registered_flutter_listener() {
        FLUTTER_EVENTS.lock().unwrap().clear();
        assert_eq!(
            crate::ffi::event_channel::mashin_core_set_event_callback(Some(capture_flutter_event)),
            0
        );
        let mut queue = Queue::default();
        queue.queue_id = QueueId("jni-command-event-test".to_string());
        publish_jni_command_events(vec![
            MediaSessionEvent::QueueChanged { queue },
            MediaSessionEvent::PlayerStateChanged {
                player_state: PlayerState::default(),
            },
        ]);
        crate::ffi::event_channel::mashin_core_set_event_callback(None);
        let events = FLUTTER_EVENTS.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("\"type\":\"queue_changed\""));
        assert!(events[0].contains("jni-command-event-test"));
        assert!(events[1].contains("\"type\":\"player_state_changed\""));
    }
}
