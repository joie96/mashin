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

#[cfg(test)]
mod tests {
    use super::{read_required_c_string, read_required_json_array, read_required_uri_array};
    use std::ffi::CString;

    #[test]
    fn rejects_missing_empty_and_invalid_utf8_inputs() {
        assert_eq!(
            read_required_c_string(std::ptr::null(), "item_id"),
            Err("item_id is null".to_string())
        );
        let empty = CString::new(" ").expect("create empty input");
        assert_eq!(
            read_required_c_string(empty.as_ptr(), "item_id"),
            Err("item_id is empty".to_string())
        );
        let invalid_utf8 = CString::new(vec![0xff]).expect("create invalid UTF-8 input");
        assert!(read_required_c_string(invalid_utf8.as_ptr(), "item_id").is_err());
    }

    #[test]
    fn decodes_nonempty_uri_and_position_arrays() {
        let uris = CString::new(r#"["ma://track/1"]"#).expect("create URI input");
        assert_eq!(
            read_required_uri_array(uris.as_ptr()),
            Ok(vec!["ma://track/1".to_string()])
        );

        let positions = CString::new("[1,2]").expect("create position input");
        assert_eq!(
            read_required_json_array::<u32>(positions.as_ptr(), "positions_json"),
            Ok(vec![1, 2])
        );
    }

    #[test]
    fn rejects_empty_or_invalid_playlist_mutation_arrays() {
        let empty = CString::new("[]").expect("create empty array input");
        assert_eq!(
            read_required_json_array::<u32>(empty.as_ptr(), "positions_json"),
            Err("positions_json is empty".to_string())
        );

        let empty_uri = CString::new(r#"[" "]"#).expect("create empty URI input");
        assert_eq!(
            read_required_uri_array(empty_uri.as_ptr()),
            Err("uris_json contains an empty URI".to_string())
        );
    }
}
