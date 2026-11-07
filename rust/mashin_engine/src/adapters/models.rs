//! Serialization belongs at the native boundary, not in command execution.

use crate::application::commands::ApplicationCommand;
use crate::application::dispatcher::CommandValue;
use crate::application::errors::ApplicationError;
use crate::application::queries::ApplicationQuery;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeOperationRequest {
    pub(crate) request_id: u64,
    #[serde(flatten)]
    pub(crate) operation: NativeOperation,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum NativeOperation {
    Command(ApplicationCommand),
    Query(ApplicationQuery),
}

// Request decoding and result serialization.
pub(crate) fn decode_command(
    kind: &str,
    argument: Option<&str>,
) -> Result<ApplicationCommand, String> {
    decode_request(kind, argument)
}

pub(crate) fn decode_query(kind: &str, argument: Option<&str>) -> Result<ApplicationQuery, String> {
    decode_request(kind, argument)
}

fn decode_request<Request: serde::de::DeserializeOwned>(
    kind: &str,
    argument: Option<&str>,
) -> Result<Request, String> {
    let mut arguments: Map<String, Value> = serde_json::from_str(argument.unwrap_or("{}"))
        .map_err(|error| format!("invalid request arguments: {error}"))?;
    if arguments.contains_key("kind") {
        return Err("request arguments must not override kind".into());
    }
    arguments.insert("kind".into(), Value::String(kind.into()));
    serde_json::from_value(Value::Object(arguments))
        .map_err(|error| format!("invalid request '{kind}': {error}"))
}

pub(crate) fn serialize_command_value(value: CommandValue) -> Result<Value, ApplicationError> {
    match value {
        CommandValue::Unit => Ok(Value::Null),
        CommandValue::RestoredSession(restored) => Ok(Value::Bool(restored)),
        CommandValue::UserData(snapshot) => serde_json::to_value(snapshot).map_err(|error| {
            ApplicationError::internal(format!("serialize command result failed: {error}"))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeOperation, NativeOperationRequest};
    use crate::application::commands::ApplicationCommand;
    use crate::application::queries::ApplicationQuery;

    #[test]
    fn startup_request_and_catalog_update_keep_the_native_contract_typed() {
        use crate::application::events::ApplicationEvent;
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        assert!(matches!(
            super::decode_query("startSessionDataLoad", None).unwrap(),
            ApplicationQuery::StartSessionDataLoad {}
        ));
        assert!(super::decode_query("loadSessionData", None).is_err());
        assert!(matches!(
            super::decode_query(
                "recommendations",
                Some(r#"{"view":"home","refresh":false}"#)
            )
            .unwrap(),
            ApplicationQuery::Recommendations { refresh: false, .. }
        ));
        let value = serde_json::to_value(ApplicationEvent::CatalogUpdated {
            query: CatalogQuery::RecentlyPlayed { limit: 40 },
            value: CatalogValue::Tracks(Vec::new()),
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"type":"catalog_updated", "query":{"RecentlyPlayed":{"limit":40}}, "value":{"Tracks":[]}})
        );
    }

    #[test]
    fn operation_envelopes_select_typed_requests_without_a_query_whitelist() {
        for (kind, expected) in [
            ("transportPlay", "command"),
            ("getUserData", "state"),
            ("cancelQuery", "query"),
        ] {
            let request: NativeOperationRequest =
                serde_json::from_value(serde_json::json!({"requestId": 42, "kind": kind})).unwrap();
            assert_eq!(request.request_id, 42);
            match (request.operation, expected) {
                (NativeOperation::Command(ApplicationCommand::TransportPlay {}), "command")
                | (NativeOperation::Query(ApplicationQuery::GetUserData {}), "state")
                | (NativeOperation::Query(ApplicationQuery::CancelQuery {}), "query") => {}
                _ => panic!("wrong native request classification"),
            }
        }
    }

    #[test]
    fn native_commands_reject_missing_fields_and_invalid_scalar_types() {
        for payload in [
            serde_json::json!({"requestId": 1, "kind": "setMuted", "muted": 1}),
            serde_json::json!({"requestId": 1, "kind": "transportRepeatMode", "mode": -1}),
            serde_json::json!({"requestId": 1, "kind": "setPreferredProvider"}),
            serde_json::json!({"requestId": 1, "kind": "saveUpdateChannel", "channel": "unknown"}),
            serde_json::json!({"kind": "transportPlay"}),
            serde_json::json!({"requestId": 1, "kind": "transportPlay", "unexpected": true}),
            serde_json::json!({"requestId": 1, "kind": "cancelQuery", "unexpected": true}),
        ] {
            assert!(serde_json::from_value::<NativeOperationRequest>(payload).is_err());
        }
    }
}
