//! Rust-to-Kotlin media-session event delivery.

use crate::jni::android_context;
use jni::objects::{JObject, JValue};

// Rust-to-Kotlin event transport.
pub(crate) fn publish_json_text(text: &str) {
    let _ = android_context::with_attached_environment(|env| {
        let native_core_bridge = android_context::native_core_bridge_global_class()?;
        let text = env
            .new_string(text)
            .map_err(|error| format!("encode Android core event failed: {error}"))?;
        env.call_static_method(
            android_context::global_class_as_jclass(&native_core_bridge),
            "deliverCoreEvent",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&JObject::from(text))],
        )
        .map_err(|error| format!("deliver Android core event failed: {error}"))?;
        Ok(())
    });
}
