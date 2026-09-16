//! Direct Android JNI facade for the Rust-owned playback core.

use crate::ffi::{commands, queries};
use crate::jni::android_context;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use std::ffi::{c_char, CStr, CString};

const NATIVE_CORE_BRIDGE: &str = "com/example/mashin_flutter/media/NativeCoreBridge";

pub(crate) extern "system" fn initialize(env: JNIEnv, _class: JClass, context: JObject) {
    android_context::initialize_android_context(env, context);
}

pub(crate) extern "system" fn restore_persisted_session(
    mut env: JNIEnv,
    _class: JClass,
) -> jboolean {
    match commands::mashin_core_restore_persisted_session() {
        0 => JNI_TRUE,
        1 => JNI_FALSE,
        _ => {
            throw_core_error(&mut env, "restore persisted session");
            JNI_FALSE
        }
    }
}

pub(crate) extern "system" fn query(
    mut env: JNIEnv,
    _class: JClass,
    kind: JString,
    argument: JString,
) -> jstring {
    let result = read_java_string(&mut env, kind)
        .and_then(|kind| {
            read_nullable_java_string(&mut env, argument).map(|argument| (kind, argument))
        })
        .and_then(|(kind, argument)| execute_query(&kind, argument.as_deref()));
    new_java_string(&mut env, result)
}

pub(crate) extern "system" fn command(
    mut env: JNIEnv,
    _class: JClass,
    kind: JString,
    argument: JString,
) -> jboolean {
    let result = read_java_string(&mut env, kind)
        .and_then(|kind| {
            read_nullable_java_string(&mut env, argument).map(|argument| (kind, argument))
        })
        .and_then(|(kind, argument)| execute_command(&kind, argument.as_deref()));
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalStateException", error);
            JNI_FALSE
        }
    }
}

pub(crate) fn publish_json_text(text: &str) {
    let _ = android_context::with_attached_environment(|env| {
        let text = env
            .new_string(text)
            .map_err(|error| format!("encode Android core event failed: {error}"))?;
        env.call_static_method(
            NATIVE_CORE_BRIDGE,
            "deliverCoreEvent",
            "(Ljava/lang/String;)V",
            &[jni::objects::JValue::Object(&JObject::from(text))],
        )
        .map_err(|error| format!("deliver Android core event failed: {error}"))?;
        Ok(())
    });
}

fn execute_query(kind: &str, argument: Option<&str>) -> Result<String, String> {
    let response = match kind {
        "queue" => queries::mashin_core_get_queue(),
        "playerState" => queries::mashin_core_get_player_state(),
        "recommendations" => queries::mashin_core_get_recommendations(),
        "recentlyPlayed" => queries::mashin_core_get_recently_played_tracks(50),
        "search" => queries::mashin_core_search_music(required_c_argument(argument)?.as_ptr(), 50),
        _ => return Err(format!("unsupported Android core query '{kind}'")),
    };
    take_owned_string(response)
}

fn execute_command(kind: &str, argument: Option<&str>) -> Result<(), String> {
    let result = match kind {
        "activate" => commands::mashin_core_activate_engine(),
        "playMediaItems" => {
            commands::mashin_core_queue_replace(required_c_argument(argument)?.as_ptr())
        }
        "play" => commands::mashin_core_transport_play(),
        "pause" => commands::mashin_core_transport_pause(),
        "stop" => commands::mashin_core_transport_stop(),
        "next" => commands::mashin_core_transport_next(),
        "previous" => commands::mashin_core_transport_previous(),
        "seekSeconds" => commands::mashin_core_transport_seek(
            required_text_argument(argument)?
                .parse()
                .map_err(|_| "invalid seek seconds")?,
        ),
        "setShuffle" => {
            commands::mashin_core_set_shuffle(if required_text_argument(argument)? == "true" {
                1
            } else {
                0
            })
        }
        "setRepeatMode" => {
            commands::mashin_core_set_repeat_mode(match required_text_argument(argument)? {
                "one" => 1,
                "all" => 2,
                _ => 0,
            })
        }
        _ => return Err(format!("unsupported Android core command '{kind}'")),
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("Android core command '{kind}' failed"))
    }
}

fn required_c_argument(argument: Option<&str>) -> Result<CString, String> {
    CString::new(argument.ok_or("missing Android core command argument")?)
        .map_err(|_| "Android core command argument contains a null byte".to_string())
}

fn required_text_argument(argument: Option<&str>) -> Result<&str, String> {
    argument.ok_or_else(|| "missing Android core command argument".to_string())
}

fn take_owned_string(pointer: *mut c_char) -> Result<String, String> {
    if pointer.is_null() {
        return Err("Android core query failed".to_string());
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|error| format!("decode Android core query failed: {error}"))?
        .to_string();
    crate::ffi::owned_strings::mashin_core_free_owned_string(pointer);
    Ok(text)
}

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
            let _ = env.throw_new("java/lang/IllegalStateException", error);
            std::ptr::null_mut()
        }
    }
}

fn throw_core_error(env: &mut JNIEnv, operation: &str) {
    let _ = env.throw_new(
        "java/lang/IllegalStateException",
        format!("Android core could not {operation}"),
    );
}
