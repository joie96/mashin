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
    UserDataChanged,
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
