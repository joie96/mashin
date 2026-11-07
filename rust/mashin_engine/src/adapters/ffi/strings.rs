//! Validated string inputs received from the C FFI boundary.

use serde::de::DeserializeOwned;
use std::ffi::{c_char, CStr};

pub(crate) fn read_required_c_string(pointer: *const c_char, name: &str) -> Result<String, String> {
    if pointer.is_null() {
        return Err(format!("{name} is null"));
    }
    // The caller retains ownership and guarantees a valid NUL-terminated pointer during this call.
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map(str::to_owned)
        .map_err(|error| format!("{name} is not valid UTF-8: {error}"))?;
    if value.trim().is_empty() {
        return Err(format!("{name} is empty"));
    }
    Ok(value)
}

pub(crate) fn read_required_json_array<T>(
    pointer: *const c_char,
    name: &str,
) -> Result<Vec<T>, String>
where
    T: DeserializeOwned,
{
    let json = read_required_c_string(pointer, name)?;
    let values: Vec<T> = serde_json::from_str(&json)
        .map_err(|error| format!("{name} must be a JSON array: {error}"))?;
    if values.is_empty() {
        return Err(format!("{name} is empty"));
    }
    Ok(values)
}

pub(crate) fn read_required_uri_array(pointer: *const c_char) -> Result<Vec<String>, String> {
    let uris = read_required_json_array::<String>(pointer, "uris_json")?;
    if uris.iter().any(|uri| uri.trim().is_empty()) {
        return Err("uris_json contains an empty URI".to_string());
    }
    Ok(uris)
}
