//! Synchronous Android JNI query adapter for the Rust-owned playback core.

use crate::adapters::models::decode_query;

// JNI query routing.
pub(crate) fn execute_query(kind: &str, argument: Option<&str>) -> Result<String, String> {
    let query = decode_query(kind, argument)?;
    crate::bootstrap::application()
        .execute_query_sync(query)
        .map_err(|error| error.message)
        .and_then(|response| serde_json::to_string(&response).map_err(|error| error.to_string()))
}
