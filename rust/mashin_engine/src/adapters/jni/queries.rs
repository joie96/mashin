//! Android snapshot queries and session-owned asynchronous catalog requests.

use crate::adapters::models::decode_query;
use crate::application::errors::ApplicationError;
use serde::Serialize;
use serde_json::json;

// JNI query routing.
pub(crate) fn execute_query(kind: &str, argument: Option<&str>) -> Result<String, String> {
    let query = decode_query(kind, argument)?;
    crate::bootstrap::application()
        .execute_query_sync(query)
        .map_err(|error| error.message)
        .and_then(|response| serde_json::to_string(&response).map_err(|error| error.to_string()))
}

// Negative JNI IDs cannot collide with Flutter's positive request sequence.
#[cfg(target_os = "android")]
pub(crate) fn start_query(
    request_id: i64,
    kind: &str,
    argument: Option<&str>,
) -> Result<(), String> {
    let native_id = native_request_id(request_id)?;
    let query = decode_query(kind, argument)?;
    crate::bootstrap::application()
        .start_query(native_id, query, move |result| {
            super::event_channel::publish_json_text(&completion_event(request_id, result));
        })
        .map_err(|error| error.message)
}

#[cfg(target_os = "android")]
pub(crate) fn cancel_query(request_id: i64) -> Result<(), String> {
    crate::bootstrap::application()
        .start_query(
            native_request_id(request_id)?,
            crate::application::queries::ApplicationQuery::CancelQuery {},
            |_| {},
        )
        .map_err(|error| error.message)
}

// Android-only responses do not enter Flutter's operation channel.
fn completion_event<T: Serialize>(request_id: i64, result: Result<T, ApplicationError>) -> String {
    match result {
        Ok(result) => {
            json!({"type": "android_query_completed", "request_id": request_id, "result": result})
        }
        Err(error) => {
            json!({"type": "android_query_failed", "request_id": request_id, "error": error})
        }
    }
    .to_string()
}

fn native_request_id(request_id: i64) -> Result<u64, String> {
    if request_id >= 0 {
        return Err("Android query IDs must be negative".to_owned());
    }
    Ok(request_id as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_query_ids_and_completion_envelopes_remain_separate() {
        assert_eq!(
            serde_json::to_value(crate::domain::catalog::CatalogQuery::RecommendationFolders).unwrap(),
            json!("RecommendationFolders"),
        );
        assert!(native_request_id(0).is_err());
        assert!(native_request_id(1).is_err());
        assert_eq!(native_request_id(-1).unwrap(), u64::MAX);
        let success: serde_json::Value =
            serde_json::from_str(&completion_event(-2, Ok(json!({"tracks": []})))).unwrap();
        assert_eq!(
            success,
            json!({"type":"android_query_completed","request_id":-2,"result":{"tracks":[]}})
        );
        let failure: serde_json::Value =
            serde_json::from_str(&completion_event::<serde_json::Value>(
                -3,
                Err(ApplicationError::command_failed("offline")),
            ))
            .unwrap();
        assert_eq!(failure["type"], "android_query_failed");
        assert_eq!(failure["request_id"], -3);
        assert_eq!(failure["error"]["message"], "offline");
    }
}
