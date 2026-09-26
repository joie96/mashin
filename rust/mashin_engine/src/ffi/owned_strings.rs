//! CString ownership transfer for FFI responses and callback payloads.

use std::ffi::{c_char, CString};
use std::ptr;

// Strings returned to Flutter remain owned by Rust until explicitly released.
#[no_mangle]
pub extern "C" fn mashin_core_free_owned_string(pointer: *mut c_char) {
    if pointer.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(pointer));
    }
}

pub(crate) fn create_owned_string(text: String) -> *mut c_char {
    CString::new(text)
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
}
