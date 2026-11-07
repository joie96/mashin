//! Protected credential I/O through the initialized Android class references.

use super::android_context::{
    global_class_as_jclass, secure_session_store_global_class, with_attached_environment,
};
use crate::credentials::store::CredentialStore;
use jni::objects::{JObject, JString, JValue};

pub(crate) struct AndroidCredentialStore;

impl CredentialStore for AndroidCredentialStore {
    // Only protected credentials cross JNI; configuration remains Rust-owned.
    fn read(&self) -> Result<Option<String>, String> {
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

    fn write(&self, serialized_session: &str) -> Result<(), String> {
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

    fn clear(&self) -> Result<(), String> {
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
}
