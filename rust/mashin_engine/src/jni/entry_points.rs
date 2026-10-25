//! Kotlin-to-Rust JNI entry points and Java value conversion.

use crate::jni::{android_context, commands, queries};
use jni::objects::{JObject, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

// Kotlin-to-Rust JNI entry points.
pub(crate) extern "system" fn initialize(
    mut env: JNIEnv,
    native_core_bridge: JObject,
    context: JObject,
    secure_session_store: JObject,
) {
    let native_core_bridge_class = match env.get_object_class(&native_core_bridge) {
        Ok(class) => class,
        Err(error) => {
            throw_core_error(
                &mut env,
                &format!("resolve Android native core bridge class failed: {error}"),
            );
            return;
        }
    };
    let secure_session_store_class = match env.get_object_class(&secure_session_store) {
        Ok(class) => class,
        Err(error) => {
            throw_core_error(
                &mut env,
                &format!("resolve Android secure session store class failed: {error}"),
            );
            return;
        }
    };
    if let Err(error) = android_context::initialize_android_context(
        &mut env,
        context,
        native_core_bridge_class,
        secure_session_store_class,
    ) {
        android_context::clear_pending_exception(&mut env);
        throw_core_error(&mut env, &error);
    } else {
        crate::logging::initialize();
    }
}

pub(crate) extern "system" fn restore_persisted_session(
    mut env: JNIEnv,
    _native_core_bridge: JObject,
) -> jboolean {
    match commands::restore_persisted_session() {
        Ok((has_restored_session, events)) => {
            commands::publish_media_session_events(events);
            if has_restored_session {
                JNI_TRUE
            } else {
                JNI_FALSE
            }
        }
        Err(error) => {
            throw_core_error(&mut env, &error.message);
            JNI_FALSE
        }
    }
}

// This barrier revokes only Flutter delivery; the service's JNI events and playback remain live.
pub(crate) extern "system" fn unregister_flutter_event_callback(
    mut env: JNIEnv,
    _rust_core_bridge: JObject,
) {
    if let Err(error) = crate::ffi::event_channel::unregister_flutter_event_callback() {
        let _ = env.throw_new("java/lang/IllegalStateException", error);
    }
}

pub(crate) extern "system" fn query(
    mut env: JNIEnv,
    _native_core_bridge: JObject,
    kind: JString,
    argument: JString,
) -> jstring {
    let result = read_java_string(&mut env, kind)
        .and_then(|kind| {
            read_nullable_java_string(&mut env, argument).map(|argument| (kind, argument))
        })
        .and_then(|(kind, argument)| queries::execute_query(&kind, argument.as_deref()));
    new_java_string(&mut env, result)
}

pub(crate) extern "system" fn command(
    mut env: JNIEnv,
    _native_core_bridge: JObject,
    kind: JString,
    argument: JString,
) -> jboolean {
    let result = read_java_string(&mut env, kind)
        .and_then(|kind| {
            read_nullable_java_string(&mut env, argument).map(|argument| (kind, argument))
        })
        .and_then(|(kind, argument)| commands::execute_command(&kind, argument.as_deref()));
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            android_context::clear_pending_exception(&mut env);
            let _ = env.throw_new("java/lang/IllegalStateException", error);
            JNI_FALSE
        }
    }
}

// Java value conversion and error mapping.
fn read_java_string(env: &mut JNIEnv, value: JString) -> Result<String, String> {
    env.get_string(&value)
        .map(|value| value.into())
        .map_err(|error| format!("read Android JNI string failed: {error}"))
}

fn read_nullable_java_string(env: &mut JNIEnv, value: JString) -> Result<Option<String>, String> {
    if value.is_null() {
        Ok(None)
    } else {
        read_java_string(env, value).map(Some)
    }
}

fn new_java_string(env: &mut JNIEnv, result: Result<String, String>) -> jstring {
    match result.and_then(|text| env.new_string(text).map_err(|error| error.to_string())) {
        Ok(text) => text.into_raw(),
        Err(error) => {
            android_context::clear_pending_exception(env);
            let _ = env.throw_new("java/lang/IllegalStateException", error);
            std::ptr::null_mut()
        }
    }
}

fn throw_core_error(env: &mut JNIEnv, operation: &str) {
    android_context::clear_pending_exception(env);
    let _ = env.throw_new(
        "java/lang/IllegalStateException",
        format!("Android core could not {operation}"),
    );
}
