//! Detached availability projection of visible session data.
use crate::domain::library::UserDataSnapshot;
use crate::domain::providers::{
    project_album_availability, project_fields_availability, project_item_availability,
    project_track_availability, ProviderInstance,
};

#[derive(Default)]
pub(crate) struct ProviderAvailabilityProjection {
    current: std::sync::RwLock<Option<std::sync::Arc<Vec<ProviderInstance>>>>,
}

impl ProviderAvailabilityProjection {
    pub(crate) fn replace(&self, providers: Option<Vec<ProviderInstance>>) {
        *self.current.write().unwrap_or_else(|error| error.into_inner()) = providers.map(std::sync::Arc::new);
    }

    pub(crate) fn project_event(&self, event: &mut super::events::ApplicationEvent) {
        let current = self.current.read().unwrap_or_else(|error| error.into_inner()).clone();
        let providers = current.as_deref().map(Vec::as_slice);
        match event {
            super::events::ApplicationEvent::CatalogUpdated { value, .. } => {
                super::queries::project_catalog_availability(value, providers);
            }
            super::events::ApplicationEvent::RecommendationsUpdated { snapshot, .. } => {
                for folder in &mut snapshot.folders {
                    for item in &mut folder.items {
                        project_item_availability(item, providers);
                    }
                    for track in &mut folder.playlist_tracks {
                        project_track_availability(track, providers);
                    }
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn project_library_availability(
    snapshot: &mut UserDataSnapshot,
    providers: Option<&[ProviderInstance]>,
) {
    for item in &mut snapshot.favorites.tracks {
        project_track_availability(item, providers);
    }
    for item in &mut snapshot.favorites.albums {
        project_album_availability(item, providers);
    }
    for item in &mut snapshot.favorites.artists {
        project_fields_availability(&mut item.media, providers);
    }
    for item in &mut snapshot.favorites.playlists {
        project_fields_availability(&mut item.media, providers);
    }
    for playlist in &mut snapshot.playlists {
        project_fields_availability(&mut playlist.playlist.media, providers);
        for item in &mut playlist.items {
            project_track_availability(item, providers);
        }
    }
    for entry in &mut snapshot.favorite_overrides {
        project_item_availability(&mut entry.item, providers);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn provider_projection_matches_early_and_terminal_values_without_changing_canonical_data() {
        use crate::application::events::ApplicationEvent;
        use crate::application::queries::QueryResult;
        use crate::domain::catalog::{CatalogQuery, CatalogValue};
        let tracks: Vec<crate::domain::media::Track> = serde_json::from_value(serde_json::json!([{
            "item_id":"track", "provider":"library", "name":"Track", "provider_mappings":[{
                "item_id":"track", "provider_domain":"test", "provider_instance":"source", "available":true
            }]
        }])).unwrap();
        let availability = super::ProviderAvailabilityProjection::default();
        availability.replace(Some(Vec::new()));
        let mut event = ApplicationEvent::CatalogUpdated {
            query: CatalogQuery::RecentlyPlayed { limit: 40 },
            value: CatalogValue::Tracks(tracks.clone()),
        };
        availability.project_event(&mut event);
        let mut terminal = QueryResult::Tracks(tracks.clone());
        terminal.project_availability(Some(&[]));
        let ApplicationEvent::CatalogUpdated { value: CatalogValue::Tracks(early), .. } = event else { panic!("expected tracks") };
        let QueryResult::Tracks(completed) = terminal else { panic!("expected tracks") };
        assert_eq!(early, completed);
        assert_eq!(early[0].media.available, Some(false));
        assert_ne!(early, tracks);
        availability.replace(None);
        let mut unknown = ApplicationEvent::CatalogUpdated {
            query: CatalogQuery::RecentlyPlayed { limit: 40 },
            value: CatalogValue::Tracks(tracks.clone()),
        };
        availability.project_event(&mut unknown);
        let mut terminal = QueryResult::Tracks(tracks.clone());
        terminal.project_availability(None);
        let ApplicationEvent::CatalogUpdated { value: CatalogValue::Tracks(early), .. } = unknown else { panic!("expected tracks") };
        let QueryResult::Tracks(completed) = terminal else { panic!("expected tracks") };
        assert_eq!(early, completed);
        assert_eq!(tracks[0].media.available, None);
    }
}
