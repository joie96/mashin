//! Changes to application projections; native routing is owned by adapters.

use super::errors::ApplicationError;
use crate::playback::models::{PlayerState, Queue};
use serde::Serialize;
use std::sync::Arc;

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
    ProviderInstancesChanged,
    CacheCleared {
        cache: CacheArea,
    },
    ImageSourceChanged {
        proxy_id: String,
    },
    CatalogUpdated {
        query: crate::domain::catalog::CatalogQuery,
        value: crate::domain::catalog::CatalogValue,
    },
    CatalogReadChanged {
        query: crate::domain::catalog::CatalogQuery,
        snapshot: crate::domain::catalog::CatalogLoadSnapshot,
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
    fn user_data_changes_preserve_general_and_targeted_event_contracts() {
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
