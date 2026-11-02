//! Session-local recommendation cache and recovery, independent of user data.
//! Loading is serialized; the session orchestrator owns the retry worker's lifetime.

use super::client::MusicAssistantClient;
use super::models::identifiers::MediaId;
use super::models::media::{MediaItem, Playlist, Track};
use super::models::recommendation::{
    MusicAssistantRecommendationFolder, RecommendationSnapshot, RecommendationView,
};
use crate::logging::Logger;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

type CachedPlaylistTracks = Arc<tokio::sync::Mutex<Option<Vec<Track>>>>;
static LOGGER: Logger = Logger::new(module_path!());

#[derive(Default)]
pub(super) struct RecommendationData {
    cache: tokio::sync::Mutex<RecommendationCache>,
    load: tokio::sync::Mutex<()>,
    retry: tokio::sync::Notify,
    playlist_tracks: tokio::sync::Mutex<HashMap<(String, String), CachedPlaylistTracks>>,
}

#[derive(Default)]
struct RecommendationCache {
    folders: Vec<MusicAssistantRecommendationFolder>,
    completed_views: HashSet<RecommendationView>,
    has_loaded_successfully: bool,
    has_loaded_folders: bool,
    has_folder_list_error: bool,
    loaded_items: HashSet<usize>,
    completed_folders: HashSet<usize>,
    failure_count: u32,
    next_retry_at: Option<tokio::time::Instant>,
}

impl RecommendationData {
    // Generic playlist reads share only cache entries registered by recommendation loading.
    pub(super) async fn load_registered_playlist_tracks<F, Load>(
        &self,
        item_id: &MediaId,
        provider: &str,
        load: Load,
    ) -> Option<Result<Vec<Track>, String>>
    where
        F: Future<Output = Result<Vec<Track>, String>>,
        Load: FnOnce() -> F,
    {
        let cached_tracks = self
            .playlist_tracks
            .lock()
            .await
            .get(&(provider.to_owned(), item_id.as_str().to_owned()))
            .cloned()?;
        let mut snapshot = cached_tracks.lock().await;
        if let Some(tracks) = snapshot.as_ref() {
            return Some(Ok(tracks.clone()));
        }
        let result = load().await;
        if let Ok(tracks) = &result {
            *snapshot = Some(tracks.clone());
        }
        Some(result)
    }
}

impl MusicAssistantClient {
    // The session serializes loading; successful snapshots survive subsequent reads.
    pub(crate) async fn load_recommendation_data(
        &self,
        mut on_update: impl FnMut(),
    ) -> Result<(), String> {
        let _load_guard = self.recommendation_data.load.lock().await;
        if self
            .recommendation_data
            .cache
            .lock()
            .await
            .has_loaded_successfully
        {
            return Ok(());
        }
        let started = Instant::now();
        LOGGER.debug("Recommendation data load started");
        let result = self.load_recommendation_folders(&mut on_update).await;
        LOGGER.info(format_args!(
            "Recommendation data load finished: elapsed_ms={} success={}",
            started.elapsed().as_millis(),
            result.is_ok()
        ));
        let mut cache = self.recommendation_data.cache.lock().await;
        cache.has_loaded_successfully = result.is_ok();
        if result.is_err() {
            cache
                .completed_views
                .extend([RecommendationView::Home, RecommendationView::Explore]);
            cache.failure_count = cache.failure_count.saturating_add(1);
            let delay = Duration::from_secs((2 << (cache.failure_count - 1).min(5)).min(60));
            cache.next_retry_at = Some(tokio::time::Instant::now() + delay);
            LOGGER.warn(format_args!(
                "Recommendation data retry scheduled: delay_secs={}",
                delay.as_secs()
            ));
        } else {
            cache.failure_count = 0;
            cache.next_retry_at = None;
        }
        drop(cache);
        self.recommendation_data.retry.notify_one();
        on_update();
        result
    }

    pub(crate) async fn retry_recommendation_data_after_reconnect(&self) {
        let mut cache = self.recommendation_data.cache.lock().await;
        if cache.next_retry_at.is_some() {
            cache.next_retry_at = Some(tokio::time::Instant::now());
        }
        drop(cache);
        self.recommendation_data.retry.notify_one();
    }

    // Cache reads wake the session worker without bypassing its retry backoff.
    pub(crate) async fn get_recommendation_data(
        &self,
        view: RecommendationView,
    ) -> Result<RecommendationSnapshot, String> {
        self.recommendation_data.retry.notify_one();
        let cache = self.recommendation_data.cache.lock().await;
        Ok(RecommendationSnapshot {
            folders: cache
                .folders
                .iter()
                .filter(|folder| view.includes(folder.item_id.as_str()))
                .cloned()
                .collect(),
            is_loading: !cache.completed_views.contains(&view),
            has_load_error: cache.has_folder_list_error
                || cache
                    .folders
                    .iter()
                    .any(|folder| view.includes(folder.item_id.as_str()) && folder.has_load_error),
        })
    }

    // The session owns this worker and aborts it on logout, including pending HTTP work.
    pub(crate) async fn run_recommendation_data_retries(&self, mut on_update: impl FnMut()) {
        loop {
            let next_retry_at = self.recommendation_data.cache.lock().await.next_retry_at;
            if let Some(deadline) = next_retry_at {
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => {
                        let _ = self.load_recommendation_data(&mut on_update).await;
                    }
                    _ = self.recommendation_data.retry.notified() => {}
                }
            } else {
                self.recommendation_data.retry.notified().await;
            }
        }
    }

    // Folder hydration preserves successful rows and playlist cache entries.
    async fn load_recommendation_folders(
        &self,
        on_update: &mut impl FnMut(),
    ) -> Result<(), String> {
        let has_loaded_folders = self
            .recommendation_data
            .cache
            .lock()
            .await
            .has_loaded_folders;
        if !has_loaded_folders {
            let result = self
                .execute_command::<Vec<MusicAssistantRecommendationFolder>>(
                    "music/recommendations",
                    json!({}),
                )
                .await;
            let mut cache = self.recommendation_data.cache.lock().await;
            cache.has_folder_list_error = result.is_err();
            cache.folders = result?;
            cache.has_loaded_folders = true;
            cache.loaded_items = cache
                .folders
                .iter()
                .enumerate()
                .filter(|(_, folder)| !folder.items.is_empty())
                .map(|(index, _)| index)
                .collect();
        }
        let mut cache = self.recommendation_data.cache.lock().await;
        let completed_folders = cache.completed_folders.clone();
        for (index, folder) in cache.folders.iter_mut().enumerate() {
            if !completed_folders.contains(&index) {
                folder.is_loading = true;
                folder.has_load_error = false;
            }
        }
        for view in [RecommendationView::Home, RecommendationView::Explore] {
            if cache.folders.iter().enumerate().any(|(index, folder)| {
                view.includes(folder.item_id.as_str()) && !completed_folders.contains(&index)
            }) {
                cache.completed_views.remove(&view);
            }
        }
        let mut folders = cache.folders.clone();
        drop(cache);
        on_update();
        let mut load_error = None;
        for view in [RecommendationView::Home, RecommendationView::Explore] {
            for (index, folder) in folders
                .iter_mut()
                .enumerate()
                .filter(|(_, folder)| view.includes(folder.item_id.as_str()))
            {
                if completed_folders.contains(&index) {
                    continue;
                }
                if !self
                    .recommendation_data
                    .cache
                    .lock()
                    .await
                    .loaded_items
                    .contains(&index)
                {
                    let started = Instant::now();
                    LOGGER.trace(format_args!(
                        "Initial recommendation items load started: provider={} folder={}",
                        folder.provider.as_str(),
                        folder.item_id.as_str()
                    ));
                    let result = self
                        .execute_command(
                            "music/recommendations/items",
                            json!({"provider": folder.provider, "item_id": folder.item_id}),
                        )
                        .await;
                    LOGGER.trace(format_args!(
                        "Initial recommendation items load finished: provider={} folder={} elapsed_ms={} success={}",
                        folder.provider.as_str(),
                        folder.item_id.as_str(),
                        started.elapsed().as_millis(),
                        result.is_ok()
                    ));
                    match result {
                        Ok(items) => {
                            folder.items = items;
                            self.recommendation_data
                                .cache
                                .lock()
                                .await
                                .loaded_items
                                .insert(index);
                        }
                        Err(error) => {
                            folder.has_load_error = true;
                            LOGGER.warn(format!("load recommendation folder failed: {error}"));
                            load_error = Some(error);
                        }
                    }
                }
                folder.playlist_tracks.clear();
                for playlist in recommendation_playlists(std::slice::from_ref(folder)) {
                    self.recommendation_data
                        .playlist_tracks
                        .lock()
                        .await
                        .entry((
                            playlist.media.provider.as_str().to_owned(),
                            playlist.media.item_id.as_str().to_owned(),
                        ))
                        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)));
                    let started = Instant::now();
                    LOGGER.trace(format_args!(
                        "Initial recommendation playlist tracks load started: provider={} playlist={}",
                        playlist.media.provider.as_str(),
                        playlist.media.item_id.as_str()
                    ));
                    let result = self
                        .get_playlist_tracks(
                            &playlist.media.item_id,
                            playlist.media.provider.as_str(),
                            false,
                            false,
                        )
                        .await;
                    LOGGER.trace(format_args!(
                        "Initial recommendation playlist tracks load finished: provider={} playlist={} elapsed_ms={} success={}",
                        playlist.media.provider.as_str(),
                        playlist.media.item_id.as_str(),
                        started.elapsed().as_millis(),
                        result.is_ok()
                    ));
                    match result {
                        Ok(tracks) => folder.playlist_tracks.extend(tracks),
                        Err(error) => {
                            folder.has_load_error = true;
                            LOGGER.warn(format!("load recommendation playlist failed: {error}"));
                            load_error = Some(error);
                        }
                    }
                }
                folder.is_loading = false;
                let mut cache = self.recommendation_data.cache.lock().await;
                cache.folders[index] = folder.clone();
                if !folder.has_load_error {
                    cache.completed_folders.insert(index);
                }
                drop(cache);
                on_update();
            }
            self.recommendation_data
                .cache
                .lock()
                .await
                .completed_views
                .insert(view);
            on_update();
        }
        load_error.map_or(Ok(()), Err)
    }
}

// Only recommendation-owned playlists are frozen; normal library playlists stay live.
fn recommendation_playlists(folders: &[MusicAssistantRecommendationFolder]) -> Vec<Playlist> {
    folders
        .iter()
        .filter(|folder| {
            matches!(
                folder.item_id.as_str(),
                "user_exploration_weekly" | "similarusers_jam_weekly"
            )
        })
        .flat_map(|folder| &folder.items)
        .filter_map(|item| match item {
            MediaItem::Playlist(playlist) => Some(playlist.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_assistant::client::tests::server;
    use serde_json::Value;

    #[tokio::test]
    async fn recommendation_cache_does_not_intercept_unregistered_playlists() {
        let recommendations = RecommendationData::default();
        assert!(recommendations
            .load_registered_playlist_tracks(&MediaId("private".into()), "library", || async {
                panic!("unregistered playlists must be loaded by the transport client")
            })
            .await
            .is_none());
    }

    #[tokio::test]
    async fn recommendation_cache_does_not_intercept_refresh_or_dynamic_tracks() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([{
                    "item_id":"user_exploration_weekly", "provider":"listenbrainz",
                    "items":[{"media_type":"playlist", "item_id":"weekly", "provider":"listenbrainz", "name":"Weekly"}]
                }]),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([{"item_id":"cached", "provider":"library", "name":"Cached"}]),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([{"item_id":"fresh", "provider":"library", "name":"Fresh"}]),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([{"item_id":"dynamic", "provider":"library", "name":"Dynamic"}]),
            ),
        ]);
        client.load_recommendation_data(|| {}).await.unwrap();
        let playlist_id = MediaId("weekly".into());
        for (force_refresh, allow_dynamic_tracks, expected_id) in [
            (true, false, "fresh"),
            (false, true, "dynamic"),
            (false, false, "cached"),
        ] {
            let tracks = client
                .get_playlist_tracks(
                    &playlist_id,
                    "listenbrainz",
                    force_refresh,
                    allow_dynamic_tracks,
                )
                .await
                .unwrap();
            assert_eq!(tracks[0].media.item_id.as_str(), expected_id);
        }
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[2]["args"]["force_refresh"], true);
        assert_eq!(requests[3]["args"]["allow_dynamic_tracks"], true);
    }
    #[tokio::test]
    async fn recommendations_are_isolated_between_sessions() {
        let (first, first_worker) = server(vec![("music/recommendations", json!([]))]);
        let (second, second_worker) = server(vec![(
            "music/recommendations",
            json!([{
                "item_id":"user_recommendations", "provider":"listenbrainz",
                "items":[{"media_type":"track", "item_id":"other", "provider":"library", "name":"Other"}]
            }]),
        )]);
        first.load_recommendation_data(|| {}).await.unwrap();
        second.load_recommendation_data(|| {}).await.unwrap();
        assert!(first
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap()
            .folders
            .is_empty());
        assert_eq!(
            second
                .get_recommendation_data(RecommendationView::Home)
                .await
                .unwrap()
                .folders
                .len(),
            1
        );
        assert!(first
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap()
            .folders
            .is_empty());
        assert_eq!(first_worker.join().unwrap().len(), 1);
        assert_eq!(second_worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recommendations_load_explore_playlist_tracks_once_without_a_page() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([{
                    "item_id":"user_exploration_weekly", "provider":"listenbrainz",
                    "items":[{"media_type":"playlist", "item_id":"weekly", "provider":"listenbrainz", "name":"Weekly"}]
                }]),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([{
                    "item_id":"track", "provider":"library", "name":"Track"
                }]),
            ),
        ]);
        client.load_recommendation_data(|| {}).await.unwrap();
        let snapshot = client
            .get_recommendation_data(RecommendationView::Explore)
            .await
            .unwrap();
        assert_eq!(snapshot.folders[0].playlist_tracks.len(), 1);
        assert!(!snapshot.is_loading);
        let weekly = MediaId("weekly".into());
        let (first, second) = tokio::join!(
            client.get_playlist_tracks(&weekly, "listenbrainz", false, false),
            client.get_playlist_tracks(&weekly, "listenbrainz", false, false),
        );
        assert_eq!(first.unwrap().len(), 1);
        assert_eq!(second.unwrap().len(), 1);
        client
            .get_recommendation_data(RecommendationView::Explore)
            .await
            .unwrap();
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn recommendations_retry_only_failed_playlists_without_duplicating_tracks() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([{
                    "item_id":"user_exploration_weekly", "provider":"listenbrainz",
                    "items":[
                        {"media_type":"playlist", "item_id":"ready", "provider":"listenbrainz", "name":"Ready"},
                        {"media_type":"playlist", "item_id":"failed", "provider":"listenbrainz", "name":"Failed"}
                    ]
                }]),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([{"item_id":"first", "provider":"library", "name":"First"}]),
            ),
            (
                "music/playlists/playlist_tracks",
                json!({"test_http_status":500}),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([{"item_id":"second", "provider":"library", "name":"Second"}]),
            ),
        ]);
        assert!(client.load_recommendation_data(|| {}).await.is_err());
        let partial = client
            .get_recommendation_data(RecommendationView::Explore)
            .await
            .unwrap();
        assert!(partial.has_load_error);
        assert_eq!(partial.folders[0].playlist_tracks.len(), 1);
        client.load_recommendation_data(|| {}).await.unwrap();
        let recovered = client
            .get_recommendation_data(RecommendationView::Explore)
            .await
            .unwrap();
        assert!(!recovered.has_load_error);
        assert_eq!(
            recovered.folders[0]
                .playlist_tracks
                .iter()
                .map(|track| track.media.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[3]["args"]["item_id"], "failed");
    }

    #[tokio::test]
    async fn recommendations_backoff_increases_and_is_capped() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!({"test_http_status":500})
            );
            7
        ]);
        for expected_delay in [2, 4, 8, 16, 32, 60, 60] {
            assert!(client.load_recommendation_data(|| {}).await.is_err());
            let cache = client.recommendation_data.cache.lock().await;
            let remaining = cache
                .next_retry_at
                .unwrap()
                .saturating_duration_since(tokio::time::Instant::now());
            assert!(remaining <= Duration::from_secs(expected_delay));
            assert!(remaining > Duration::from_secs(expected_delay - 1));
        }
        assert_eq!(worker.join().unwrap().len(), 7);
    }

    #[tokio::test]
    async fn recommendations_share_inflight_and_completed_requests() {
        let (client, worker) = server(vec![("music/recommendations", json!([]))]);
        assert!(
            client
                .get_recommendation_data(RecommendationView::Home)
                .await
                .unwrap()
                .is_loading
        );
        let (first, second) = tokio::join!(
            client.load_recommendation_data(|| {}),
            client.load_recommendation_data(|| {})
        );
        first.unwrap();
        second.unwrap();
        let (home, explore) = tokio::join!(
            client.get_recommendation_data(RecommendationView::Home),
            client.get_recommendation_data(RecommendationView::Explore)
        );
        assert!(home.unwrap().folders.is_empty());
        assert!(explore.unwrap().folders.is_empty());
        assert!(client
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap()
            .folders
            .is_empty());
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recommendations_retry_failed_startup_without_reloading_successful_data() {
        let (client, worker) = server(vec![
            ("music/recommendations", json!({"test_http_status":500})),
            ("music/recommendations", json!([])),
        ]);
        assert!(client.load_recommendation_data(|| {}).await.is_err());
        client.load_recommendation_data(|| {}).await.unwrap();
        client.load_recommendation_data(|| {}).await.unwrap();
        let snapshot = client
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap();
        assert!(snapshot.folders.is_empty());
        assert!(!snapshot.is_loading);
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn recommendations_retry_worker_recovers_after_backoff_without_a_page() {
        let (client, worker) = server(vec![
            ("music/recommendations", json!({"test_http_status":500})),
            ("music/recommendations", json!([])),
        ]);
        let client = Arc::new(client);
        let retry_client = client.clone();
        let (updates, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let retry_worker = tokio::spawn(async move {
            retry_client
                .run_recommendation_data_retries(|| {
                    let _ = updates.send(());
                })
                .await;
        });
        tokio::task::yield_now().await;
        assert!(client.load_recommendation_data(|| {}).await.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), receiver.recv())
                .await
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(8), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            !client
                .get_recommendation_data(RecommendationView::Home)
                .await
                .unwrap()
                .has_load_error
        );
        retry_worker.abort();
        assert!(retry_worker.await.unwrap_err().is_cancelled());
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn recommendations_reconnect_wakes_one_worker_and_reads_preserve_backoff() {
        let (client, worker) = server(vec![
            ("music/recommendations", json!({"test_http_status":500})),
            ("music/recommendations", json!([])),
        ]);
        assert!(client.load_recommendation_data(|| {}).await.is_err());
        client.recommendation_data.cache.lock().await.next_retry_at =
            Some(tokio::time::Instant::now() + Duration::from_secs(60));
        let client = Arc::new(client);
        let retry_client = client.clone();
        let (updates, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let retry_worker = tokio::spawn(async move {
            retry_client
                .run_recommendation_data_retries(|| {
                    let _ = updates.send(());
                })
                .await;
        });
        for _ in 0..10 {
            client
                .get_recommendation_data(RecommendationView::Home)
                .await
                .unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), receiver.recv())
                .await
                .is_err()
        );
        tokio::join!(
            client.retry_recommendation_data_after_reconnect(),
            client.retry_recommendation_data_after_reconnect()
        );
        tokio::time::timeout(Duration::from_secs(8), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        retry_worker.abort();
        assert!(retry_worker.await.unwrap_err().is_cancelled());
        assert!(
            !client
                .get_recommendation_data(RecommendationView::Explore)
                .await
                .unwrap()
                .has_load_error
        );
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn recommendations_skip_rows_not_used_by_home() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([
                    {"item_id":"user_recommendations", "provider":"listenbrainz", "items":[]},
                    {"item_id":"unused_row", "provider":"other", "items":[]}
                ]),
            ),
            ("music/recommendations/items", json!([])),
            ("test/finish", json!(null)),
        ]);
        client.load_recommendation_data(|| {}).await.unwrap();
        client
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap();
        client
            .execute_command::<Value>("test/finish", json!({}))
            .await
            .unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1]["args"]["item_id"], "user_recommendations");
    }

    #[tokio::test]
    async fn recommendations_publish_home_before_explore_and_cache_empty_rows() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([
                    {"item_id":"user_exploration_weekly", "provider":"listenbrainz"},
                    {"item_id":"user_recommendations", "provider":"listenbrainz"},
                    {"item_id":"unused_row", "provider":"other"}
                ]),
            ),
            ("music/recommendations/items", json!([])),
            ("music/recommendations/items", json!([])),
            ("test/finish", json!(null)),
        ]);
        let mut updates = Vec::new();
        client
            .load_recommendation_data(|| {
                updates.push(
                    client
                        .recommendation_data
                        .cache
                        .try_lock()
                        .unwrap()
                        .folders
                        .clone(),
                );
            })
            .await
            .unwrap();
        assert!(updates[0][0].is_loading);
        assert!(updates[0][1].is_loading);
        assert!(updates[1][0].is_loading);
        assert!(!updates[1][1].is_loading);
        client
            .load_recommendation_data(|| panic!("startup was repeated"))
            .await
            .unwrap();
        for view in [
            RecommendationView::Home,
            RecommendationView::Explore,
            RecommendationView::Home,
            RecommendationView::Explore,
        ] {
            let snapshot = client.get_recommendation_data(view).await.unwrap();
            assert!(!snapshot.is_loading);
            assert_eq!(snapshot.folders.len(), 1);
            assert!(view.includes(snapshot.folders[0].item_id.as_str()));
        }
        client
            .execute_command::<Value>("test/finish", json!({}))
            .await
            .unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[1]["args"]["item_id"], "user_recommendations");
        assert_eq!(requests[2]["args"]["item_id"], "user_exploration_weekly");
    }

    #[tokio::test]
    async fn recommendations_retry_failed_rows_and_publish_recovery() {
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([
                    {"item_id":"user_recommendations", "provider":"listenbrainz"}
                ]),
            ),
            (
                "music/recommendations/items",
                json!({"test_http_status":500}),
            ),
            ("music/recommendations/items", json!([])),
        ]);
        assert!(client.load_recommendation_data(|| {}).await.is_err());
        let failed = client
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap();
        assert!(failed.has_load_error);
        assert!(failed.folders[0].has_load_error);
        assert!(!failed.is_loading);
        let mut updates = 0;
        client
            .load_recommendation_data(|| updates += 1)
            .await
            .unwrap();
        assert!(updates > 0);
        let recovered = client
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap();
        assert!(!recovered.has_load_error);
        assert!(!recovered.folders[0].has_load_error);
        assert!(!recovered.is_loading);
        client
            .load_recommendation_data(|| panic!("empty success must stay cached"))
            .await
            .unwrap();
        assert_eq!(worker.join().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn recommendations_hydrate_empty_rows_and_preserve_embedded_items() {
        let track =
            json!({"media_type":"track", "item_id":"track", "provider":"library", "name":"Track"});
        let (client, worker) = server(vec![
            (
                "music/recommendations",
                json!([
                    {"item_id":"user_recommendations", "provider":"listenbrainz", "items":[track.clone()]},
                    {"item_id":"user_top_tracks_monthly", "provider":"listenbrainz"},
                    {"item_id":"user_top_artists_monthly", "provider":"listenbrainz"}
                ]),
            ),
            (
                "music/recommendations/items",
                json!({"test_http_status":500}),
            ),
            ("music/recommendations/items", json!([track.clone()])),
            ("music/recommendations/items", json!([track])),
        ]);
        assert!(client.load_recommendation_data(|| {}).await.is_err());
        let folders = client
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap()
            .folders;
        assert_eq!(folders[0].items.len(), 1);
        assert!(folders[1].items.is_empty());
        assert_eq!(folders[2].items.len(), 1);
        client.load_recommendation_data(|| {}).await.unwrap();
        let recovered = client
            .get_recommendation_data(RecommendationView::Home)
            .await
            .unwrap();
        assert!(!recovered.has_load_error);
        assert_eq!(recovered.folders[0], folders[0]);
        assert_eq!(recovered.folders[1].items.len(), 1);
        assert_eq!(recovered.folders[2], folders[2]);
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[3]["args"]["item_id"], "user_top_tracks_monthly");
        assert_eq!(
            requests[1]["args"],
            json!({"provider":"listenbrainz", "item_id":"user_top_tracks_monthly"})
        );
        assert_eq!(
            requests[2]["args"],
            json!({"provider":"listenbrainz", "item_id":"user_top_artists_monthly"})
        );
    }
}
