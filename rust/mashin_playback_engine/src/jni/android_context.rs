//! Android application-context initialization required by CPAL and AAudio.

use jni::objects::{JObject, JString, JValue};
use jni::JavaVM;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

static ANDROID_APP_CONTEXT: Lazy<Mutex<Option<jni::objects::GlobalRef>>> =
    Lazy::new(|| Mutex::new(None));
static ANDROID_JAVA_VM: Lazy<Mutex<Option<Arc<JavaVM>>>> = Lazy::new(|| Mutex::new(None));
static ANDROID_NDK_CONTEXT_INITIALIZED: AtomicBool = AtomicBool::new(false);

pub(crate) fn initialize_android_context(env: jni::JNIEnv, context: JObject) {
    let Ok(java_vm) = env.get_java_vm() else {
        return;
    };
    let Ok(global_ref) = env.new_global_ref(context) else {
        return;
    };
    let context_pointer = global_ref.as_obj().as_raw().cast();
    let java_vm_pointer = java_vm.get_java_vm_pointer().cast();
    {
        let Ok(mut stored) = ANDROID_APP_CONTEXT.lock() else {
            return;
        };
        *stored = Some(global_ref);
    }
    if let Ok(mut stored) = ANDROID_JAVA_VM.lock() {
        *stored = Some(Arc::new(java_vm));
    }
    // CPAL accesses `ndk_context` after AAudio opens, so initialize it exactly once.
    if ANDROID_NDK_CONTEXT_INITIALIZED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        unsafe {
            ndk_context::initialize_android_context(java_vm_pointer, context_pointer);
        }
    }
}

// Android session persistence crosses the boundary only as one serialized SessionConfig value.
pub(crate) fn read_secure_session() -> Result<Option<String>, String> {
    with_attached_environment(|env| {
        let value = env
            .call_static_method(
                "com/example/mashin_flutter/SecureSessionStore",
                "read",
                "()Ljava/lang/String;",
                &[],
            )
            .map_err(|error| format!("read Android secure session failed: {error}"))?
            .l()
            .map_err(|error| format!("read Android secure session result failed: {error}"))?;
        if value.is_null() {
            return Ok(None);
        }
        env.get_string(&JString::from(value))
            .map(|value| Some(value.into()))
            .map_err(|error| format!("decode Android secure session failed: {error}"))
    })
}

pub(crate) fn write_secure_session(serialized_session: &str) -> Result<(), String> {
    with_attached_environment(|env| {
        let value = env
            .new_string(serialized_session)
            .map_err(|error| format!("encode Android secure session failed: {error}"))?;
        env.call_static_method(
            "com/example/mashin_flutter/SecureSessionStore",
            "write",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&JObject::from(value))],
        )
        .map_err(|error| format!("write Android secure session failed: {error}"))?;
        Ok(())
    })
}

pub(crate) fn clear_secure_session() -> Result<(), String> {
    with_attached_environment(|env| {
        env.call_static_method(
            "com/example/mashin_flutter/SecureSessionStore",
            "clear",
            "()V",
            &[],
        )
        .map_err(|error| format!("clear Android secure session failed: {error}"))?;
        Ok(())
    })
}

pub(crate) fn with_attached_environment<T>(
    operation: impl FnOnce(&mut jni::JNIEnv) -> Result<T, String>,
) -> Result<T, String> {
    let java_vm = ANDROID_JAVA_VM
        .lock()
        .map_err(|_| "Android JavaVM lock poisoned".to_string())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "Android application context is not initialized".to_string())?;
    let mut env = java_vm
        .attach_current_thread()
        .map_err(|error| format!("attach Android JNI thread failed: {error}"))?;
    operation(&mut env)
}
