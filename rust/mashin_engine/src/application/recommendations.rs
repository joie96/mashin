//! Stateless online recommendations; no stored feed or refresh coordination.
use super::errors::ApplicationError;
use super::repositories::CatalogClient;
use super::Application;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::media::MediaItem;
use crate::domain::recommendations::{RecommendationSnapshot, RecommendationView};
use std::sync::Arc;
pub(crate) struct Recommendations {
    catalog: Arc<dyn CatalogClient>,
}
impl Application {
    // Session-bound service lookup.
    pub(crate) fn recommendation_service(
        &self,
        generation: u64,
    ) -> Result<Arc<Recommendations>, ApplicationError> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("coordinator lock poisoned"))?;
        if coordinator.session_generation != generation {
            return Err(ApplicationError::command_failed(
                "session changed before recommendations",
            ));
        }
        coordinator
            .repositories
            .as_ref()
            .map(|services| services.recommendations.clone())
            .ok_or_else(|| ApplicationError::internal("session services unavailable"))
    }
}
impl Recommendations {
    // Construction.
    pub(crate) fn new(catalog: Arc<dyn CatalogClient>) -> Self {
        Self { catalog }
    }
    // Startup checks the endpoint without retaining a second recommendation inventory.
    pub(crate) async fn load_recommendation_data(
        &self,
        mut on_update: impl FnMut(),
    ) -> Result<(), String> {
        self.catalog
            .load_catalog(CatalogQuery::RecommendationFolders, false)
            .await?;
        on_update();
        Ok(())
    }
    // Each selected view is assembled from online results without a retained feed.
    pub(crate) async fn get_recommendation_data(
        &self,
        view: RecommendationView,
    ) -> Result<RecommendationSnapshot, String> {
        let CatalogValue::RecommendationFolders(folders) = self
            .catalog
            .load_catalog(CatalogQuery::RecommendationFolders, false)
            .await?
        else {
            return Err("recommendation folders unavailable".into());
        };
        let mut selected = Vec::new();
        for mut folder in folders
            .into_iter()
            .filter(|folder| view.includes(folder.item_id.as_str()))
        {
            if folder.items.is_empty() {
                match self
                    .catalog
                    .load_catalog(
                        CatalogQuery::RecommendationItems {
                            item_id: folder.item_id.clone(),
                            provider: folder.provider.as_str().into(),
                        },
                        false,
                    )
                    .await
                {
                    Ok(CatalogValue::RecommendationItems(items)) => folder.items = items,
                    _ => folder.has_load_error = true,
                }
            }
            if matches!(
                folder.item_id.as_str(),
                "user_exploration_weekly" | "similarusers_jam_weekly"
            ) {
                for item in &folder.items {
                    if let MediaItem::Playlist(playlist) = item {
                        match self
                            .catalog
                            .load_catalog(
                                CatalogQuery::PlaylistTracks {
                                    item_id: playlist.media.item_id.clone(),
                                    provider: playlist.media.provider.as_str().into(),
                                    allow_dynamic_tracks: false,
                                },
                                false,
                            )
                            .await
                        {
                            Ok(CatalogValue::Tracks(items)) => folder.playlist_tracks.extend(items),
                            _ => folder.has_load_error = true,
                        }
                    }
                }
            }
            folder.is_loading = false;
            selected.push(folder);
        }
        Ok(RecommendationSnapshot {
            has_load_error: selected.iter().any(|folder| folder.has_load_error),
            folders: selected,
            is_loading: false,
        })
    }
}
