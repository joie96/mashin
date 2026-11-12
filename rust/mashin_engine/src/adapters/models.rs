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
    #[serde(default)]
    pub(crate) is_catalog_observed: bool,
    #[serde(flatten)]
    pub(crate) operation: NativeOperation,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum NativeOperation {
    Command(ApplicationCommand),
    Query(ApplicationQuery),
}

impl NativeOperationRequest {
    pub(crate) fn catalog_completion_requested(&self) -> Result<bool, ApplicationError> {
        if !self.is_catalog_observed {
            return Ok(false);
        }
        let is_observable = matches!(
            self.operation,
            NativeOperation::Query(
                ApplicationQuery::ProviderManifests {}
                    | ApplicationQuery::RecentlyPlayedTracks { .. }
                    | ApplicationQuery::SearchMedia { .. }
                    | ApplicationQuery::Artist { .. }
                    | ApplicationQuery::Album { .. }
                    | ApplicationQuery::AlbumTracks { .. }
                    | ApplicationQuery::Playlist { .. }
                    | ApplicationQuery::PlaylistTracks {
                        force_refresh: false,
                        allow_dynamic_tracks: false,
                        ..
                    }
                    | ApplicationQuery::ArtistAlbums { .. }
                    | ApplicationQuery::ArtistTopTracks { .. }
                    | ApplicationQuery::SimilarArtists { .. }
                    | ApplicationQuery::SimilarTracks { .. }
                    | ApplicationQuery::TrackVersions { .. }
            )
        );
        if !is_observable {
            return Err(ApplicationError::command_failed(
                "catalog completion requires an observed non-dynamic catalog query",
            ));
        }
        Ok(true)
    }
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
    fn catalog_completion_requires_explicit_observation_and_rejects_strict_operations() {
        use serde_json::json;
        for kind in ["providerManifests", "recentlyPlayedTracks", "artistAlbums", "playlistTracks"] {
            let mut payload = match kind {
                "providerManifests" | "recentlyPlayedTracks" => json!({"kind":kind}),
                _ => json!({"kind":kind,"itemId":"one","provider":"library"}),
            };
            payload["requestId"] = json!(1);
            let plain: NativeOperationRequest = serde_json::from_value(payload.clone()).unwrap();
            assert!(!plain.catalog_completion_requested().unwrap());
            payload["isCatalogObserved"] = json!(true);
            let observed: NativeOperationRequest = serde_json::from_value(payload).unwrap();
            assert!(observed.catalog_completion_requested().unwrap());
        }
        for mut payload in [
            json!({"kind":"transportPlay"}),
            json!({"kind":"getUserData"}),
            json!({"kind":"recommendations","view":"home"}),
            json!({"kind":"providerIconData","provider":"source"}),
            json!({"kind":"playlistTracks","itemId":"one","provider":"library","forceRefresh":true}),
            json!({"kind":"playlistTracks","itemId":"one","provider":"library","allowDynamicTracks":true}),
        ] {
            payload["requestId"] = json!(1);
            payload["isCatalogObserved"] = json!(true);
            let request: NativeOperationRequest = serde_json::from_value(payload).unwrap();
            assert!(request.catalog_completion_requested().is_err());
        }
    }

    #[test]
    fn cache_usage_query_and_byte_counts_keep_the_native_contract() {
        let request: NativeOperationRequest = serde_json::from_value(
            serde_json::json!({"requestId":42,"kind":"getCacheUsage"}),
        ).unwrap();
        assert!(matches!(request.operation, NativeOperation::Query(ApplicationQuery::GetCacheUsage {})));
        assert!(super::decode_query("getCacheUsage", Some(r#"{"unexpected":true}"#)).is_err());
        let result = crate::application::queries::QueryResult::CacheUsage(
            crate::application::repositories::CacheUsage { metadata_bytes: 91, image_bytes: 72 },
        );
        assert_eq!(serde_json::to_value(result).unwrap(), serde_json::json!({"metadataBytes":91,"imageBytes":72}));
    }

    #[test]
    fn cache_maintenance_commands_and_completion_events_keep_separate_areas() {
        for (kind, area) in [
            ("clearDatabaseCache", "database"),
            ("clearImageCache", "images"),
        ] {
            assert!(super::decode_command(kind, None).is_ok());
            assert!(super::decode_command(kind, Some(r#"{"unexpected":true}"#)).is_err());
            let request: NativeOperationRequest =
                serde_json::from_value(serde_json::json!({"requestId":42,"kind":kind})).unwrap();
            assert!(matches!(request.operation, NativeOperation::Command(_)));
            let cache = if area == "database" {
                crate::application::events::CacheArea::Database
            } else {
                crate::application::events::CacheArea::Images
            };
            assert_eq!(
                serde_json::to_value(crate::application::events::ApplicationEvent::CacheCleared {
                    cache
                })
                .unwrap(),
                serde_json::json!({"type":"cache_cleared","cache":area})
            );
        }
    }

    #[test]
    fn startup_request_and_catalog_update_keep_the_native_contract_typed() {
        use crate::application::events::ApplicationEvent;
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        assert!(matches!(
            super::decode_query("startSessionDataLoad", None).unwrap(),
            ApplicationQuery::StartSessionDataLoad {}
        ));
        assert!(super::decode_query("loadSessionData", None).is_err());
        assert_eq!(
            serde_json::to_value(ApplicationEvent::ProviderInstancesChanged).unwrap(),
            serde_json::json!({"type":"provider_instances_changed"})
        );
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
