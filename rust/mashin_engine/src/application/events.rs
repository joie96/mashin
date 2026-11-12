//! Changes to application projections; native routing is owned by adapters.

use super::errors::ApplicationError;
use crate::playback::models::{PlayerState, Queue};
use serde::Serialize;
use std::sync::Arc;

/// Status notifications never duplicate the media value carried by CatalogUpdated.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CatalogReadStatus {
    pub(crate) is_server_complete: bool,
    pub(crate) is_persisting: bool,
    pub(crate) is_persistence_complete: bool,
    pub(crate) source: Option<crate::domain::catalog::CatalogDataSource>,
    pub(crate) phase: crate::domain::catalog::CatalogLoadPhase,
    pub(crate) knowledge: crate::domain::catalog::CatalogReadKnowledge,
    pub(crate) freshness: crate::domain::catalog::CatalogFreshness,
    pub(crate) observed_at: Option<i64>,
    pub(crate) error: Option<String>,
    pub(crate) cache_error: Option<String>,
}

impl From<&crate::domain::catalog::CatalogLoadSnapshot> for CatalogReadStatus {
    fn from(snapshot: &crate::domain::catalog::CatalogLoadSnapshot) -> Self {
        use crate::domain::catalog::{CatalogDataSource, CatalogLoadPhase, CatalogReadKnowledge};
        let is_server_complete = snapshot.source == Some(CatalogDataSource::Server)
            && snapshot.knowledge == CatalogReadKnowledge::ObservedResponse;
        Self {
            is_server_complete,
            is_persisting: is_server_complete && snapshot.phase == CatalogLoadPhase::Refreshing,
            is_persistence_complete: is_server_complete && snapshot.phase == CatalogLoadPhase::Ready,
            source: snapshot.source,
            phase: snapshot.phase,
            knowledge: snapshot.knowledge,
            freshness: snapshot.freshness,
            observed_at: snapshot.observed_at,
            error: snapshot.error.clone(),
            cache_error: snapshot.cache_error.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CacheArea {
    Database,
    Images,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ApplicationEvent {
    SessionChanged,
    QueueChanged {
        queue: Queue,
    },
    PlayerStateChanged {
        player_state: PlayerState,
    },
    RecommendationsChanged,
    RecommendationsUpdated {
        view: crate::domain::recommendations::RecommendationView,
        snapshot: crate::domain::recommendations::RecommendationSnapshot,
    },
    UserDataChanged {
        #[serde(skip_serializing_if = "Option::is_none")]
        playlist: Option<crate::domain::library::PrivatePlaylist>,
        #[serde(skip_serializing_if = "Option::is_none")]
        revision: Option<u64>,
    },
    UserDataFavoritesChanged {
        snapshot: crate::domain::library::UserDataFavoritesSnapshot,
    },
    UserDataMetadataChanged {
        snapshot: crate::domain::library::UserDataMetadataSnapshot,
    },
    ProviderInstancesChanged,
    CacheCleared {
        cache: CacheArea,
    },
    ImageSourceChanged {
        proxy_id: String,
    },
    ImageReadUpdated {
        observation_id: u64,
        snapshot: super::queries::ImageReadSnapshot,
    },
    CatalogUpdated {
        query: crate::domain::catalog::CatalogQuery,
        value: crate::domain::catalog::CatalogValue,
    },
    CatalogReadChanged {
        query: crate::domain::catalog::CatalogQuery,
        snapshot: CatalogReadStatus,
    },
    #[serde(rename = "core_error")]
    Error {
        error: ApplicationError,
    },
}

#[derive(Clone)]
pub(crate) struct EventPublisher {
    sink: Arc<dyn Fn(ApplicationEvent) + Send + Sync>,
}

#[derive(Default)]
pub(crate) struct CatalogPublication {
    state: std::sync::Mutex<(u64, Option<Arc<crate::domain::catalog::CatalogValue>>)>,
}

impl CatalogPublication {
    // Replay serves a new observer without moving the shared publication backwards.
    pub(crate) fn value_for_publication(&self, revision: u64, value: &Arc<crate::domain::catalog::CatalogValue>, is_replay: bool) -> Option<Arc<crate::domain::catalog::CatalogValue>> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if revision != 0 && revision <= state.0 {
            return if is_replay { state.1.clone() } else { None };
        }
        let is_changed = !state.1.as_ref().is_some_and(|previous| Arc::ptr_eq(previous, value) || previous == value);
        state.0 = revision;
        if is_changed { state.1 = Some(value.clone()); }
        (is_changed || is_replay).then(|| value.clone())
    }
}

impl EventPublisher {
    // Sink ownership and delivery.
    pub(crate) fn new(sink: impl Fn(ApplicationEvent) + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
        }
    }

    pub(crate) fn publish(&self, event: ApplicationEvent) {
        (self.sink)(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_data_metadata_publication_does_not_serialize_playlist_tracks() {
        let playlist = serde_json::from_value(serde_json::json!({
            "item_id":"playlist", "provider":"library", "name":"Private",
            "is_loading_tracks":true,
            "items":[{"item_id":"track", "provider":"library", "name":"Track"}]
        })).unwrap();
        let state = crate::domain::library::UserDataSnapshot {
            revision: 7,
            playlists: vec![playlist],
            is_loading_playlists: true,
            ..Default::default()
        };
        let event = ApplicationEvent::UserDataMetadataChanged { snapshot: (&state).into() };
        let encoded = serde_json::to_value(event).unwrap();
        assert_eq!(encoded["type"], "user_data_metadata_changed");
        assert_eq!(encoded["snapshot"]["revision"], 7);
        assert_eq!(encoded["snapshot"]["is_loading_playlists"], true);
        assert_eq!(encoded["snapshot"]["playlists"][0]["is_loading_tracks"], true);
        assert!(encoded["snapshot"]["playlists"][0].get("items").is_none());
        assert_eq!(state.playlists[0].items.len(), 1);
    }

    #[test]
    fn catalog_publication_deduplicates_updates_but_replays_without_regressing_revision() {
        use crate::domain::catalog::CatalogValue;
        let publication = CatalogPublication::default();
        let cached = Arc::new(CatalogValue::Tracks(Vec::new()));
        let server = Arc::new(CatalogValue::Tracks(serde_json::from_value(serde_json::json!([
            {"provider":"library","item_id":"track","name":"Server"}
        ])).unwrap()));
        assert_eq!(publication.value_for_publication(1, &cached, true), Some(cached.clone()));
        assert_eq!(publication.value_for_publication(2, &server, false), Some(server.clone()));
        assert_eq!(publication.value_for_publication(2, &server, false), None);
        assert_eq!(publication.value_for_publication(1, &cached, true), Some(server.clone()));
        assert_eq!(publication.value_for_publication(1, &cached, false), None);
        assert_eq!(publication.value_for_publication(2, &server, false), None);
        assert_eq!(publication.value_for_publication(3, &server, false), None);
        assert_eq!(publication.value_for_publication(4, &cached, false), Some(cached));
    }

    #[test]
    fn catalog_status_preserves_read_state_without_media_payloads() {
        use crate::domain::catalog::*;
        let snapshot = CatalogLoadSnapshot {
            publication_revision: 0,
            value: Some(Arc::new(CatalogValue::Tracks(Vec::new()))),
            source: Some(CatalogDataSource::Cache),
            phase: CatalogLoadPhase::Failed,
            knowledge: CatalogReadKnowledge::ObservedResponse,
            freshness: CatalogFreshness::Stale,
            observed_at: Some(123),
            error: Some("server unavailable".into()),
            cache_error: Some("cache read warning".into()),
        };
        let event = serde_json::to_value(ApplicationEvent::CatalogReadChanged {
            query: CatalogQuery::RecentlyPlayed { limit: 40 },
            snapshot: (&snapshot).into(),
        }).unwrap();
        assert!(event["snapshot"].get("value").is_none());
        assert_eq!(event["snapshot"]["source"], "cache");
        assert_eq!(event["snapshot"]["phase"], "failed");
        assert_eq!(event["snapshot"]["knowledge"], "observed_response");
        assert_eq!(event["snapshot"]["freshness"], "stale");
        assert_eq!(event["snapshot"]["observed_at"], 123);
        assert_eq!(event["snapshot"]["error"], "server unavailable");
        assert_eq!(event["snapshot"]["cache_error"], "cache read warning");
        assert!(snapshot.value.is_some());
        let mut server = snapshot.clone();
        server.source = Some(CatalogDataSource::Server);
        server.phase = CatalogLoadPhase::Refreshing;
        let pending = CatalogReadStatus::from(&server);
        assert!(pending.is_server_complete);
        assert!(pending.is_persisting);
        assert!(!pending.is_persistence_complete);
        server.phase = CatalogLoadPhase::Ready;
        let completed = CatalogReadStatus::from(&server);
        assert!(completed.is_server_complete);
        assert!(!completed.is_persisting);
        assert!(completed.is_persistence_complete);
    }

    #[test]
    fn user_data_changes_preserve_general_and_targeted_event_contracts() {
        let favorites = serde_json::to_value(ApplicationEvent::UserDataFavoritesChanged {
            snapshot: crate::domain::library::UserDataSnapshot::default().into(),
        }).unwrap();
        assert!(favorites["snapshot"].get("playlists").is_none());
        assert!(favorites["snapshot"].get("is_loading_playlists").is_none());
        assert!(favorites["snapshot"].get("favorites").is_some());
        assert_eq!(
            serde_json::to_value(ApplicationEvent::UserDataChanged {
                playlist: None,
                revision: None
            })
            .unwrap(),
            serde_json::json!({"type": "user_data_changed"})
        );
        let playlist = serde_json::from_value(serde_json::json!({
            "provider": "library", "item_id": "42", "name": "Playlist",
            "items": [{"provider": "library", "item_id": "7", "name": "Track"}],
            "is_loading_tracks": false
        }))
        .unwrap();
        let event = serde_json::to_value(ApplicationEvent::UserDataChanged {
            playlist: Some(playlist),
            revision: Some(7),
        })
        .unwrap();
        assert_eq!(event["type"], "user_data_changed");
        assert_eq!(event["playlist"]["item_id"], "42");
        assert_eq!(event["playlist"]["items"][0]["item_id"], "7");
        assert_eq!(event["playlist"]["is_loading_tracks"], false);
        assert_eq!(event["revision"], 7);
    }
}
