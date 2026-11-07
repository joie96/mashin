//! Platform delivery is independent from the application publisher's lifetime.

use crate::application::events::ApplicationEvent;

// Kotlin refreshes its snapshot from the synchronous result; Flutter still needs the change.
pub(crate) fn publish_jni_command_events(events: Vec<ApplicationEvent>) {
    for event in events {
        publish_event(
            &event,
            false,
            publish_android,
            super::ffi::event_channel::publish_flutter_json_text,
        );
    }
}

// Background changes have no command caller to refresh either platform.
pub(crate) fn publish_background_event(event: ApplicationEvent) {
    publish_event(
        &event,
        true,
        publish_android,
        super::ffi::event_channel::publish_flutter_json_text,
    );
}

// Logs belong to Flutter diagnostics, not the Android media-session callback.
pub(crate) fn publish_log(line: &str) {
    super::ffi::event_channel::publish_flutter_log_event(line);
}

fn publish_android(text: &str) {
    #[cfg(not(target_os = "android"))]
    let _ = text;
    #[cfg(target_os = "android")]
    super::jni::event_channel::publish_json_text(text);
}

fn publish_event(
    event: &ApplicationEvent,
    should_publish_to_android: bool,
    publish_to_android: impl FnOnce(&str),
    publish_to_flutter: impl FnOnce(&str),
) {
    if let Ok(text) = serde_json::to_string(event) {
        if should_publish_to_android {
            publish_to_android(&text);
        }
        publish_to_flutter(&text);
    }
}
