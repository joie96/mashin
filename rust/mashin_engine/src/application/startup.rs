//! Session startup publishes metadata before optional hydration and never owns confirmed storage.

use super::recommendations::Recommendations;
use super::repositories::CatalogClient;
use super::repositories::{
    InitialUserDataUpdate, LibraryClient, SharedUserDataPublisher, UserDataPublisher,
};
use crate::application::repositories::{with_request_priority, RequestPriority};
use crate::domain::catalog::{CatalogLoadPhase, CatalogQuery, CatalogReadKnowledge, CatalogValue};
use crate::domain::library::PrivatePlaylist;
use futures_util::{stream, StreamExt};

pub(crate) struct InitialSessionDataResult {
    pub(crate) recommendation_data: Result<(), String>,
    pub(crate) user_data: Result<(), String>,
}

impl InitialSessionDataResult {
    pub(crate) fn into_result(self) -> Result<(), String> {
        self.user_data.and(self.recommendation_data)
    }
}

// Startup reservations order optional background reads without owning HTTP arguments or transport.
pub(crate) async fn load_initial_session_data(
    library: &dyn LibraryClient,
    catalog: &dyn CatalogClient,
    recommendations: &Recommendations,
    mut recommendations_changed: impl FnMut() -> Result<(), String> + Send,
    user_data_publisher: impl UserDataPublisher,
) -> InitialSessionDataResult {
    let _startup = catalog.begin_startup();
    let mut publication_error = None;
    let recommendation_load = with_request_priority(
        RequestPriority::Startup,
        recommendations.load_recommendation_data(|| {
            if let Err(error) = recommendations_changed() {
                publication_error = Some(error);
            }
        }),
    );
    let user_load = with_request_priority(
        RequestPriority::Startup,
        load_initial_user_data(library, catalog, user_data_publisher),
    );
    let (recommendation_data, mut user_data) = tokio::join!(recommendation_load, user_load);
    if let Some(error) = publication_error {
        user_data = Err(error);
    }
    InitialSessionDataResult {
        recommendation_data,
        user_data,
    }
}

// Identity precedes independent sections; optional hydration does not gate their publication.
pub(crate) async fn load_initial_user_data(
    library: &dyn LibraryClient,
    catalog: &dyn CatalogClient,
    mut publisher: impl UserDataPublisher,
) -> Result<(), String> {
    let user_id = library.get_current_user_id().await?;
    publisher
        .publish(InitialUserDataUpdate::UserIdentity(user_id.clone()))
        .await?;
    let publisher = tokio::sync::Mutex::new(&mut publisher);
    let mut playlist_publisher = SharedUserDataPublisher::new(&publisher);
    let mut favorite_publisher = SharedUserDataPublisher::new(&publisher);
    let playlists = async {
        let playlists = library
            .load_private_playlists(catalog, user_id.as_str(), false, &mut playlist_publisher)
            .await?;
        playlist_publisher
            .publish(InitialUserDataUpdate::Playlists(playlists))
            .await
    };
    let favorites = async {
        let favorites = library
            .load_favorite_pages(catalog, false, &mut favorite_publisher)
            .await?;
        favorite_publisher
            .publish(InitialUserDataUpdate::Favorites(favorites))
            .await
    };
    let (playlists, favorites) = tokio::join!(playlists, favorites);
    playlists.and(favorites)
}

// Every sidebar playlist observes local data independently of queued background HTTP reads.
pub(crate) async fn load_initial_playlist_tracks(
    catalog: &dyn CatalogClient,
    playlists: Vec<PrivatePlaylist>,
    should_load: impl Fn(&str) -> bool + Sync,
    mut publisher: impl UserDataPublisher,
) -> Result<(), String> {
    with_request_priority(RequestPriority::Background, async {
        let observations = playlists
            .into_iter()
            .filter(|playlist| should_load(playlist.playlist.media.item_id.as_str()))
            .map(|playlist| {
                let item_id = playlist.playlist.media.item_id;
                let query = CatalogQuery::PlaylistTracks {
                    item_id: item_id.clone(),
                    provider: "library".into(),
                    allow_dynamic_tracks: false,
                };
                let mut previous_items = playlist.items;
                let mut has_published = !playlist.is_loading_tracks;
                catalog
                    .observe_catalog(query, false)
                    .filter_map(move |mut snapshot| {
                        if snapshot.knowledge == CatalogReadKnowledge::ObservedResponse {
                            if let Some(CatalogValue::Tracks(items)) = snapshot.value.as_deref() {
                                if has_published && previous_items == *items {
                                    snapshot.value = None;
                                } else {
                                    previous_items = items.clone();
                                    has_published = true;
                                }
                            }
                        }
                        futures_util::future::ready(Some(snapshot))
                    })
                    .map(move |snapshot| (item_id.clone(), snapshot))
            });
        let mut pending = stream::select_all(observations);
        let mut load_error = None;
        while let Some((item_id, snapshot)) = pending.next().await {
            if snapshot.phase == CatalogLoadPhase::Invalidated {
                load_error = Some(
                    snapshot
                        .error
                        .unwrap_or_else(|| "playlist observation invalidated".into()),
                );
                continue;
            }
            if snapshot.knowledge == CatalogReadKnowledge::ObservedResponse {
                if let Some(CatalogValue::Tracks(items)) = snapshot.value.map(std::sync::Arc::unwrap_or_clone) {
                    publisher
                        .publish(InitialUserDataUpdate::PlaylistTracks { item_id, items })
                        .await?;
                }
            }
            if snapshot.phase == CatalogLoadPhase::Failed {
                load_error = Some(
                    snapshot
                        .error
                        .unwrap_or_else(|| "playlist refresh failed".into()),
                );
            }
        }
        load_error.map_or(Ok(()), Err)
    })
    .await
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::domain::catalog::{
        CatalogDataSource, CatalogFreshness, CatalogLoadPhase, CatalogLoadSnapshot,
        CatalogReadKnowledge,
    };
    use futures_util::future::BoxFuture;
    use futures_util::stream::BoxStream;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub(crate) struct SidebarCatalog {
        terminal: Option<CatalogLoadPhase>,
        pub(crate) release: tokio::sync::Notify,
        pub(crate) reads: std::sync::atomic::AtomicUsize,
    }

    impl SidebarCatalog {
        // Controlled cache publication and remote completion.
        pub(crate) fn new(terminal: Option<CatalogLoadPhase>) -> Self {
            Self {
                terminal,
                release: Default::default(),
                reads: Default::default(),
            }
        }
    }

    impl CatalogClient for SidebarCatalog {
        fn begin_startup(&self) -> Box<dyn Send> {
            Box::new(())
        }

        fn load_catalog(
            &self,
            _query: CatalogQuery,
            _force_refresh: bool,
        ) -> BoxFuture<'_, Result<CatalogValue, String>> {
            Box::pin(async { Err("completion-only reads hide cached tracks".into()) })
        }

        fn observe_catalog(
            &self,
            _query: CatalogQuery,
            _force_refresh: bool,
        ) -> BoxStream<'_, CatalogLoadSnapshot> {
            assert_eq!(
                super::super::repositories::current_priority(),
                RequestPriority::Background
            );
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let cached = CatalogLoadSnapshot {
                publication_revision: 0,
                value: Some(std::sync::Arc::new(CatalogValue::Tracks(serde_json::from_value(serde_json::json!([
                    {"provider":"library","item_id":"track","name":"Cached track","duration":120}
                ])).unwrap()))),
                source: Some(CatalogDataSource::Cache),
                phase: CatalogLoadPhase::Refreshing,
                knowledge: CatalogReadKnowledge::ObservedResponse,
                freshness: CatalogFreshness::Stale,
                observed_at: Some(1),
                error: None,
                cache_error: None,
            };
            let mut completed = cached.clone();
            completed.phase = self.terminal.unwrap_or(CatalogLoadPhase::Ready);
            if completed.phase == CatalogLoadPhase::Ready {
                completed.value = Some(std::sync::Arc::new(CatalogValue::Tracks(Vec::new())));
                completed.source = Some(CatalogDataSource::Server);
            } else {
                completed.error = Some("controlled refresh failure".into());
            }
            let remote = stream::once(async move {
                self.release.notified().await;
                completed
            });
            Box::pin(stream::iter([cached.clone(), cached]).chain(remote))
        }
    }

    #[tokio::test]
    async fn sidebar_cached_tracks_publish_for_all_playlists_before_remote_completion() {
        let playlists = (0..4)
            .map(|index| PrivatePlaylist {
                playlist: serde_json::from_value(serde_json::json!({
                    "provider":"library", "item_id":index.to_string(), "name":"Cached"
                }))
                .unwrap(),
                items: Vec::new(),
                is_loading_tracks: true,
            })
            .collect();
        let published = Arc::new(Mutex::new(Vec::new()));
        let received = published.clone();
        let publisher = move |update| {
            if let InitialUserDataUpdate::PlaylistTracks { item_id, .. } = update {
                received.lock().unwrap().push(item_id);
            }
            Ok(())
        };
        let catalog = SidebarCatalog::new(None);
        let loading = load_initial_playlist_tracks(&catalog, playlists, |_| true, publisher);
        assert!(tokio::time::timeout(Duration::from_millis(100), loading)
            .await
            .is_err());
        assert_eq!(published.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn sidebar_cached_tracks_survive_failed_refresh_and_accept_confirmed_empty() {
        for phase in [
            CatalogLoadPhase::Ready,
            CatalogLoadPhase::Failed,
            CatalogLoadPhase::Invalidated,
        ] {
            let catalog = SidebarCatalog::new(Some(phase));
            let playlists = vec![PrivatePlaylist {
                playlist: serde_json::from_value(
                    serde_json::json!({"provider":"library","item_id":"42","name":"Cached"}),
                )
                .unwrap(),
                items: Vec::new(),
                is_loading_tracks: true,
            }];
            let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
            let publisher = move |update| {
                if let InitialUserDataUpdate::PlaylistTracks { items, .. } = update {
                    sender.send(items).unwrap();
                }
                Ok(())
            };
            let loading = load_initial_playlist_tracks(&catalog, playlists, |_| true, publisher);
            tokio::pin!(loading);
            let cached = tokio::select! {
                result = &mut loading => panic!("hydration completed before publishing cache: {result:?}"),
                items = received.recv() => items.unwrap(),
            };
            assert_eq!(cached.len(), 1);
            assert_eq!(cached[0].duration, 120);
            catalog.release.notify_one();
            let result = tokio::time::timeout(Duration::from_secs(2), &mut loading)
                .await
                .unwrap();
            assert_eq!(result.is_ok(), phase == CatalogLoadPhase::Ready);
            if phase == CatalogLoadPhase::Ready {
                assert!(received.recv().await.unwrap().is_empty());
            }
            assert!(received.recv().await.is_none());
        }
    }
}
