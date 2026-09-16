//! Android JNI boundaries for platform services and application context setup.

#[cfg(target_os = "android")]
pub(crate) mod android_context;
#[cfg(target_os = "android")]
pub(crate) mod android_core;

#[cfg(target_os = "android")]
use jni::{JavaVM, NativeMethod};
#[cfg(target_os = "android")]
use std::ffi::c_void;

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn JNI_OnLoad(java_vm: JavaVM, _: *mut c_void) -> jni::sys::jint {
    let Ok(mut env) = java_vm.get_env() else {
        return 0;
    };
    let methods = [
        NativeMethod {
            name: "initialize".into(),
            sig: "(Landroid/content/Context;)V".into(),
            fn_ptr: android_core::initialize as *mut c_void,
        },
        NativeMethod {
            name: "restorePersistedSession".into(),
            sig: "()Z".into(),
            fn_ptr: android_core::restore_persisted_session as *mut c_void,
        },
        NativeMethod {
            name: "query".into(),
            sig: "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".into(),
            fn_ptr: android_core::query as *mut c_void,
        },
        NativeMethod {
            name: "command".into(),
            sig: "(Ljava/lang/String;Ljava/lang/String;)Z".into(),
            fn_ptr: android_core::command as *mut c_void,
        },
    ];
    if env
        .register_native_methods(
            "com/example/mashin_flutter/media/NativeCoreBridge",
            &methods,
        )
        .is_err()
    {
        return 0;
    }
    jni::sys::JNI_VERSION_1_6
}
