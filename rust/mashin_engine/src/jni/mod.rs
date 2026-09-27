//! Android JNI boundaries for context, commands, queries, events, and entry points.

#[cfg(target_os = "android")]
pub(crate) mod android_context;
#[cfg(any(target_os = "android", test))]
pub(crate) mod commands;
#[cfg(target_os = "android")]
pub(crate) mod entry_points;
#[cfg(target_os = "android")]
pub(crate) mod event_channel;
#[cfg(target_os = "android")]
pub(crate) mod queries;

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
            sig: "(Landroid/content/Context;Ljava/lang/Object;)V".into(),
            fn_ptr: entry_points::initialize as *mut c_void,
        },
        NativeMethod {
            name: "restorePersistedSession".into(),
            sig: "()Z".into(),
            fn_ptr: entry_points::restore_persisted_session as *mut c_void,
        },
        NativeMethod {
            name: "query".into(),
            sig: "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".into(),
            fn_ptr: entry_points::query as *mut c_void,
        },
        NativeMethod {
            name: "command".into(),
            sig: "(Ljava/lang/String;Ljava/lang/String;)Z".into(),
            fn_ptr: entry_points::command as *mut c_void,
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
