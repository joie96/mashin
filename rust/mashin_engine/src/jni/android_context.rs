//! Android application-context ownership and platform queries for native worker threads.

use jni::objects::{JClass, JObject, JString, JValue};
use jni::JavaVM;
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// JVM and application-class references retained for native worker threads.
static ANDROID_APP_CONTEXT: Lazy<Mutex<Option<jni::objects::GlobalRef>>> =
    Lazy::new(|| Mutex::new(None));
static ANDROID_NATIVE_CORE_BRIDGE: Lazy<Mutex<Option<jni::objects::GlobalRef>>> =
    Lazy::new(|| Mutex::new(None));
static ANDROID_SECURE_SESSION_STORE: Lazy<Mutex<Option<jni::objects::GlobalRef>>> =
    Lazy::new(|| Mutex::new(None));
static ANDROID_JAVA_VM: Lazy<Mutex<Option<Arc<JavaVM>>>> = Lazy::new(|| Mutex::new(None));
static ANDROID_NDK_CONTEXT_INITIALIZED: AtomicBool = AtomicBool::new(false);

// Android JNI lifecycle initialization.
pub(crate) fn initialize_android_context(
    env: &mut jni::JNIEnv,
    context: JObject,
    native_core_bridge_class: JClass,
    secure_session_store_class: JClass,
) -> Result<(), String> {
    let java_vm = env
        .get_java_vm()
        .map_err(|error| format!("access Android JavaVM failed: {error}"))?;
    let global_ref = env
        .new_global_ref(context)
        .map_err(|error| format!("retain Android application context failed: {error}"))?;
    let native_core_bridge = env
        .new_global_ref(native_core_bridge_class)
        .map_err(|error| format!("retain Android native core bridge failed: {error}"))?;
    let secure_session_store = env
        .new_global_ref(secure_session_store_class)
        .map_err(|error| format!("retain Android secure session store failed: {error}"))?;
    validate_static_method(
        env,
        &native_core_bridge,
        "deliverCoreEvent",
        "(Ljava/lang/String;)V",
    )?;
    validate_static_method(env, &secure_session_store, "read", "()Ljava/lang/String;")?;
    validate_static_method(env, &secure_session_store, "write", "(Ljava/lang/String;)V")?;
    validate_static_method(env, &secure_session_store, "clear", "()V")?;
    let context_pointer = global_ref.as_obj().as_raw().cast();
    let java_vm_pointer = java_vm.get_java_vm_pointer().cast();
    *ANDROID_APP_CONTEXT
        .lock()
        .map_err(|_| "Android application context lock poisoned".to_string())? = Some(global_ref);
    *ANDROID_NATIVE_CORE_BRIDGE
        .lock()
        .map_err(|_| "Android native core bridge lock poisoned".to_string())? =
        Some(native_core_bridge);
    *ANDROID_SECURE_SESSION_STORE
        .lock()
        .map_err(|_| "Android secure session store lock poisoned".to_string())? =
        Some(secure_session_store);
    *ANDROID_JAVA_VM
        .lock()
        .map_err(|_| "Android JavaVM lock poisoned".to_string())? = Some(Arc::new(java_vm));
    // CPAL accesses `ndk_context` after AAudio opens, so initialize it exactly once.
    if ANDROID_NDK_CONTEXT_INITIALIZED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        unsafe {
            ndk_context::initialize_android_context(java_vm_pointer, context_pointer);
        }
    }
    Ok(())
}

// Android device identity uses the user-assigned name, or the model if unavailable.
pub(crate) fn device_name() -> Result<String, String> {
    let configured_name = with_attached_environment(|env| {
        let context = ANDROID_APP_CONTEXT
            .lock()
            .map_err(|_| "Android application context lock poisoned".to_string())?
            .as_ref()
            .cloned()
            .ok_or_else(|| "Android application context is not initialized".to_string())?;
        let resolver = env
            .call_method(
                context.as_obj(),
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("read Android content resolver failed: {error}"))?;
        let key = env
            .new_string("device_name")
            .map_err(|error| format!("encode Android device name key failed: {error}"))?;
        let value = env
            .call_static_method(
                "android/provider/Settings$Global",
                "getString",
                "(Landroid/content/ContentResolver;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(&resolver),
                    JValue::Object(&JObject::from(key)),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("read Android device name failed: {error}"))?;
        if value.is_null() {
            return Ok(None);
        }
        let name: String = env
            .get_string(&JString::from(value))
            .map_err(|error| format!("decode Android device name failed: {error}"))?
            .into();
        Ok(Some(name))
    });
    if let Ok(Some(name)) = configured_name {
        if !name.trim().is_empty() {
            return Ok(name);
        }
    }
    with_attached_environment(|env| {
        let model = env
            .get_static_field("android/os/Build", "MODEL", "Ljava/lang/String;")
            .and_then(|value| value.l())
            .map_err(|error| format!("read Android device model failed: {error}"))?;
        env.get_string(&JString::from(model))
            .map(|value| value.into())
            .map_err(|error| format!("decode Android device model failed: {error}"))
    })
}

// Only protected credentials cross JNI; the Rust session store owns non-secret configuration.
pub(crate) fn read_secure_session() -> Result<Option<String>, String> {
    with_attached_environment(|env| {
        let secure_session_store = secure_session_store_global_class()?;
        let value = env
            .call_static_method(
                global_class_as_jclass(&secure_session_store),
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
        let secure_session_store = secure_session_store_global_class()?;
        let value = env
            .new_string(serialized_session)
            .map_err(|error| format!("encode Android secure session failed: {error}"))?;
        env.call_static_method(
            global_class_as_jclass(&secure_session_store),
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
        let secure_session_store = secure_session_store_global_class()?;
        env.call_static_method(
            global_class_as_jclass(&secure_session_store),
            "clear",
            "()V",
            &[],
        )
        .map_err(|error| format!("clear Android secure session failed: {error}"))?;
        Ok(())
    })
}

// FilesDir belongs to the installed application and needs no storage permission.
pub(crate) fn application_files_directory() -> Result<PathBuf, String> {
    with_attached_environment(|env| {
        let context = ANDROID_APP_CONTEXT
            .lock()
            .map_err(|_| "Android application context lock poisoned".to_string())?
            .as_ref()
            .cloned()
            .ok_or_else(|| "Android application context is not initialized".to_string())?;
        let directory = env
            .call_method(context.as_obj(), "getFilesDir", "()Ljava/io/File;", &[])
            .map_err(|error| format!("read Android files directory failed: {error}"))?
            .l()
            .map_err(|error| format!("read Android files directory result failed: {error}"))?;
        let path = env
            .call_method(directory, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .map_err(|error| format!("read Android files path failed: {error}"))?
            .l()
            .map_err(|error| format!("read Android files path result failed: {error}"))?;
        let path: String = env
            .get_string(&JString::from(path))
            .map_err(|error| format!("decode Android files path failed: {error}"))?
            .into();
        Ok(PathBuf::from(path))
    })
}

// ExternalFilesDir is app-specific shared storage and requires no storage permission.
pub(crate) fn external_application_files_directory() -> Result<PathBuf, String> {
    with_attached_environment(|env| {
        let context = ANDROID_APP_CONTEXT
            .lock()
            .map_err(|_| "Android application context lock poisoned".to_string())?
            .as_ref()
            .cloned()
            .ok_or_else(|| "Android application context is not initialized".to_string())?;
        let directory = env
            .call_method(
                context.as_obj(),
                "getExternalFilesDir",
                "(Ljava/lang/String;)Ljava/io/File;",
                &[JValue::Object(&JObject::null())],
            )
            .map_err(|error| format!("read Android external files directory failed: {error}"))?
            .l()
            .map_err(|error| format!("read Android external files directory result failed: {error}"))?;
        if directory.is_null() {
            return Err("Android external files directory is unavailable".to_string());
        }
        let path = env
            .call_method(directory, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .map_err(|error| format!("read Android external files path failed: {error}"))?
            .l()
            .map_err(|error| format!("read Android external files path result failed: {error}"))?;
        let path: String = env
            .get_string(&JString::from(path))
            .map_err(|error| format!("decode Android external files path failed: {error}"))?
            .into();
        Ok(PathBuf::from(path))
    })
}

// Cached application-class references and JNI error handling.
pub(crate) fn native_core_bridge_global_class() -> Result<jni::objects::GlobalRef, String> {
    ANDROID_NATIVE_CORE_BRIDGE
        .lock()
        .map_err(|_| "Android native core bridge lock poisoned".to_string())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "Android native core bridge is not initialized".to_string())
}

fn secure_session_store_global_class() -> Result<jni::objects::GlobalRef, String> {
    ANDROID_SECURE_SESSION_STORE
        .lock()
        .map_err(|_| "Android secure session store lock poisoned".to_string())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "Android secure session store is not initialized".to_string())
}

// A GlobalRef retains the application class across thread attachments.
pub(crate) fn global_class_as_jclass(global_ref: &jni::objects::GlobalRef) -> JClass<'_> {
    unsafe { JClass::from_raw(global_ref.as_obj().as_raw()) }
}

fn validate_static_method(
    env: &mut jni::JNIEnv,
    class: &jni::objects::GlobalRef,
    name: &str,
    signature: &str,
) -> Result<(), String> {
    env.get_static_method_id(global_class_as_jclass(class), name, signature)
        .map(|_| ())
        .map_err(|error| format!("resolve Android JNI method {name}{signature} failed: {error}"))
}

pub(crate) fn clear_pending_exception(env: &mut jni::JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

// Worker-thread attachment and controlled JNI error cleanup.
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
    let result = operation(&mut env);
    if result.is_err() {
        clear_pending_exception(&mut env);
    }
    result
}
