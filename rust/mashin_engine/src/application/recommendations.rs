//! Session-bound view assembly from observed reads; working values live only for one request.
use super::errors::ApplicationError;
use super::repositories::CatalogClient;
use super::Application;
use crate::domain::catalog::{CatalogLoadPhase, CatalogLoadSnapshot, CatalogQuery, CatalogValue};
use crate::domain::identifiers::MediaId;
use crate::domain::media::{MediaItem, Track};
use crate::domain::recommendations::{
    RecommendationFolder, RecommendationSnapshot, RecommendationView,
};
use futures_util::future::AbortHandle;
use futures_util::stream::{Abortable, BoxStream, SelectAll};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
pub(crate) struct Recommendations {
    catalog: Arc<dyn CatalogClient>,
    view_publication_revisions: [AtomicU64; 2],
}

type FolderKey = (String, String);
type ReadStreams<'read> =
    SelectAll<BoxStream<'read, (ReadTarget, CatalogQuery, CatalogLoadSnapshot)>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ReadTarget {
    Folders,
    Items(FolderKey),
    Playlist(FolderKey, u64, usize),
}

struct PlaylistRead {
    tracks: Option<Vec<Track>>,
    is_finished: bool,
    has_error: bool,
}

struct FolderRead {
    value: RecommendationFolder,
    items_are_finished: bool,
    revision: u64,
    playlists: HashMap<usize, PlaylistRead>,
}

impl FolderRead {
    // Request-local content and load-state projection.
    fn project(&self) -> RecommendationFolder {
        let mut value = self.value.clone();
        value.is_loading = (!value.knowledge.items && !self.items_are_finished)
            || (!self.playlists.is_empty()
                && self.playlists.values().all(|read| read.tracks.is_none())
                && self.playlists.values().any(|read| !read.is_finished));
        value.has_load_error |= self.playlists.values().any(|read| read.has_error);
        if !self.playlists.is_empty() {
            let mut positions = self.playlists.keys().copied().collect::<Vec<_>>();
            positions.sort_unstable();
            value.playlist_tracks = positions
                .into_iter()
                .flat_map(|position| self.playlists[&position].tracks.clone().unwrap_or_default())
                .collect();
            value.knowledge.playlist_tracks =
                self.playlists.values().all(|read| read.tracks.is_some());
        }
        value
    }
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
        Self {
            catalog,
            view_publication_revisions: [AtomicU64::new(0), AtomicU64::new(0)],
        }
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
    // View assembly publishes each changed projection without waiting for sibling reads.
    pub(crate) async fn get_recommendation_data(
        &self,
        view: RecommendationView,
        force_refresh: bool,
        mut on_update: impl FnMut(RecommendationSnapshot) + Send,
        mut on_read: impl FnMut(CatalogQuery, CatalogLoadSnapshot) + Send,
    ) -> Result<RecommendationSnapshot, String> {
        let publication_revision = &self.view_publication_revisions[match view {
            RecommendationView::Home => 0,
            RecommendationView::Explore => 1,
        }];
        let expected_publication_revision = publication_revision.fetch_add(1, Ordering::AcqRel) + 1;
        let mut streams = ReadStreams::new();
        let mut handles = HashMap::new();
        self.start_read(
            &mut streams,
            &mut handles,
            ReadTarget::Folders,
            CatalogQuery::RecommendationFolders,
            force_refresh,
        );
        let mut selected = HashMap::<FolderKey, FolderRead>::new();
        let mut order = Vec::new();
        let mut has_folders = false;
        let mut has_folder_error = false;
        let mut published = None;
        while let Some((target, query, observation)) = streams.next().await {
            on_read(query, observation.clone());
            if observation.phase == CatalogLoadPhase::Invalidated {
                return Err(observation
                    .error
                    .unwrap_or_else(|| "recommendation read invalidated".into()));
            }
            let is_finished = matches!(
                observation.phase,
                CatalogLoadPhase::Ready | CatalogLoadPhase::Failed
            );
            let has_error = observation.phase == CatalogLoadPhase::Failed;
            if is_finished {
                handles.remove(&target);
            }
            match target {
                ReadTarget::Folders => {
                    has_folder_error = has_error;
                    if let Some(CatalogValue::RecommendationFolders(folders)) = observation.value {
                        has_folders = true;
                        let mut previous = std::mem::take(&mut selected);
                        order.clear();
                        for folder in folders
                            .into_iter()
                            .filter(|folder| view.includes(folder.item_id.as_str()))
                        {
                            let key = (
                                folder.provider.as_str().to_owned(),
                                folder.item_id.as_str().to_owned(),
                            );
                            if selected.contains_key(&key) {
                                continue;
                            }
                            let read = if let Some(mut read) = previous.remove(&key) {
                                if folder.knowledge.title {
                                    read.value.title = folder.title;
                                    read.value.knowledge.title = true;
                                }
                                if folder.knowledge.subtitle {
                                    read.value.subtitle = folder.subtitle;
                                    read.value.knowledge.subtitle = true;
                                }
                                read
                            } else {
                                let needs_items = folder.items.is_empty();
                                let mut read = FolderRead {
                                    value: folder,
                                    items_are_finished: !needs_items,
                                    revision: 0,
                                    playlists: HashMap::new(),
                                };
                                if needs_items {
                                    read.value.knowledge.items = false;
                                    self.start_read(
                                        &mut streams,
                                        &mut handles,
                                        ReadTarget::Items(key.clone()),
                                        CatalogQuery::RecommendationItems {
                                            provider: key.0.clone().into(),
                                            item_id: MediaId(key.1.clone()),
                                        },
                                        force_refresh,
                                    );
                                } else {
                                    self.start_playlist_reads(
                                        &key,
                                        &mut read,
                                        &mut streams,
                                        &mut handles,
                                        force_refresh,
                                    );
                                }
                                read
                            };
                            order.push(key.clone());
                            selected.insert(key, read);
                        }
                        handles.retain(|target, handle| {
                            let retained = match target {
                                ReadTarget::Folders => true,
                                ReadTarget::Items(key) | ReadTarget::Playlist(key, _, _) => {
                                    selected.contains_key(key)
                                }
                            };
                            if !retained {
                                handle.abort();
                            }
                            retained
                        });
                    }
                }
                ReadTarget::Items(key) => {
                    if let Some(read) = selected.get_mut(&key) {
                        if let Some(CatalogValue::RecommendationItems(items)) = observation.value {
                            if !read.value.knowledge.items || read.value.items != items {
                                read.value.items = items;
                                read.value.knowledge.items = true;
                                self.start_playlist_reads(
                                    &key,
                                    read,
                                    &mut streams,
                                    &mut handles,
                                    force_refresh,
                                );
                            }
                        }
                        read.items_are_finished = is_finished;
                        read.value.has_load_error = has_error;
                    }
                }
                ReadTarget::Playlist(key, revision, position) => {
                    if let Some(read) = selected
                        .get_mut(&key)
                        .filter(|read| read.revision == revision)
                    {
                        if let Some(playlist) = read.playlists.get_mut(&position) {
                            if let Some(CatalogValue::Tracks(tracks)) = observation.value {
                                playlist.tracks = Some(tracks);
                            }
                            playlist.is_finished = is_finished;
                            playlist.has_error = has_error;
                        }
                    }
                }
            }
            let folders = order
                .iter()
                .filter_map(|key| selected.get(key).map(FolderRead::project))
                .collect::<Vec<_>>();
            let snapshot = RecommendationSnapshot {
                has_load_error: has_folder_error
                    || folders.iter().any(|folder| folder.has_load_error),
                folders,
                is_loading: !handles.is_empty(),
            };
            if published.as_ref() != Some(&snapshot) {
                if publication_revision.load(Ordering::Acquire) == expected_publication_revision {
                    on_update(snapshot.clone());
                }
                published = Some(snapshot);
            }
        }
        if !has_folders {
            return Err("recommendation folders unavailable".into());
        }
        published.ok_or_else(|| "recommendations ended without a projection".into())
    }

    // Observation participation is owned by this request, not a spawned feed worker.
    fn start_read<'read>(
        &'read self,
        streams: &mut ReadStreams<'read>,
        handles: &mut HashMap<ReadTarget, AbortHandle>,
        target: ReadTarget,
        query: CatalogQuery,
        force_refresh: bool,
    ) {
        let (handle, registration) = AbortHandle::new_pair();
        handles.insert(target.clone(), handle);
        streams.push(
            Abortable::new(
                self.catalog.observe_catalog(query.clone(), force_refresh),
                registration,
            )
            .map(move |snapshot| (target.clone(), query.clone(), snapshot))
            .boxed(),
        );
    }

    fn start_playlist_reads<'read>(
        &'read self,
        key: &FolderKey,
        read: &mut FolderRead,
        streams: &mut ReadStreams<'read>,
        handles: &mut HashMap<ReadTarget, AbortHandle>,
        force_refresh: bool,
    ) {
        handles.retain(|target, handle| {
            let retained = !matches!(target, ReadTarget::Playlist(folder, _, _) if folder == key);
            if !retained {
                handle.abort();
            }
            retained
        });
        read.revision += 1;
        read.playlists.clear();
        read.value.playlist_tracks.clear();
        read.value.knowledge.playlist_tracks = true;
        if !matches!(
            key.1.as_str(),
            "user_exploration_weekly" | "similarusers_jam_weekly"
        ) {
            return;
        }
        for (position, item) in read.value.items.iter().enumerate() {
            if let MediaItem::Playlist(playlist) = item {
                read.playlists.insert(
                    position,
                    PlaylistRead {
                        tracks: None,
                        is_finished: false,
                        has_error: false,
                    },
                );
                self.start_read(
                    streams,
                    handles,
                    ReadTarget::Playlist(key.clone(), read.revision, position),
                    CatalogQuery::PlaylistTracks {
                        item_id: playlist.media.item_id.clone(),
                        provider: playlist.media.provider.as_str().into(),
                        allow_dynamic_tracks: false,
                    },
                    force_refresh,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::catalog::{CatalogDataSource, CatalogFreshness, CatalogReadKnowledge};
    use futures_util::future::BoxFuture;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    struct ObservedCatalog {
        reads: Mutex<
            HashMap<
                String,
                std::collections::VecDeque<mpsc::UnboundedReceiver<CatalogLoadSnapshot>>,
            >,
        >,
        starts: Mutex<Vec<(CatalogQuery, bool)>>,
    }
    impl CatalogClient for ObservedCatalog {
        fn begin_startup(&self) -> Box<dyn Send> {
            Box::new(())
        }
        fn load_catalog(
            &self,
            _: CatalogQuery,
            _: bool,
        ) -> BoxFuture<'_, Result<CatalogValue, String>> {
            Box::pin(async { panic!("view must consume observations") })
        }
        fn observe_catalog(
            &self,
            query: CatalogQuery,
            force_refresh: bool,
        ) -> BoxStream<'_, CatalogLoadSnapshot> {
            let receiver = self
                .reads
                .lock()
                .unwrap()
                .get_mut(&format!("{query:?}"))
                .unwrap()
                .pop_front()
                .unwrap();
            self.starts.lock().unwrap().push((query, force_refresh));
            futures_util::stream::unfold(receiver, |mut receiver| async move {
                receiver.recv().await.map(|value| (value, receiver))
            })
            .boxed()
        }
    }

    fn observation(value: Option<CatalogValue>, phase: CatalogLoadPhase) -> CatalogLoadSnapshot {
        CatalogLoadSnapshot {
            value,
            phase,
            source: Some(CatalogDataSource::Cache),
            cache_error: None,
            knowledge: CatalogReadKnowledge::ObservedResponse,
            freshness: CatalogFreshness::Stale,
            observed_at: Some(100),
            error: None,
        }
    }

    #[tokio::test]
    async fn recommendation_sections_publish_cache_in_parallel_retain_failure_and_accept_empty() {
        let catalog = Arc::new(ObservedCatalog {
            reads: Mutex::new(HashMap::new()),
            starts: Mutex::new(Vec::new()),
        });
        let mut senders = Vec::new();
        for query in [
            CatalogQuery::RecommendationFolders,
            CatalogQuery::RecommendationItems {
                provider: "source".into(),
                item_id: MediaId("user_recommendations".into()),
            },
            CatalogQuery::RecommendationItems {
                provider: "source".into(),
                item_id: MediaId("user_top_tracks_monthly".into()),
            },
        ] {
            let (sender, receiver) = mpsc::unbounded_channel();
            catalog
                .reads
                .lock()
                .unwrap()
                .entry(format!("{query:?}"))
                .or_default()
                .push_back(receiver);
            senders.push(sender);
        }
        let folders = serde_json::from_value(serde_json::json!([
            {"provider":"source","item_id":"user_recommendations","name":"Slow"},
            {"provider":"source","item_id":"user_top_tracks_monthly","name":"Fast"}
        ]))
        .unwrap();
        senders[0]
            .send(observation(
                Some(CatalogValue::RecommendationFolders(folders)),
                CatalogLoadPhase::Refreshing,
            ))
            .unwrap();
        let item: MediaItem = serde_json::from_value(serde_json::json!({"media_type":"track","provider":"source","item_id":"cached","name":"Cached"})).unwrap();
        senders[2]
            .send(observation(
                Some(CatalogValue::RecommendationItems(vec![item.clone()])),
                CatalogLoadPhase::Refreshing,
            ))
            .unwrap();
        let service = Recommendations::new(catalog.clone());
        let (updates, mut observed) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            service
                .get_recommendation_data(
                    RecommendationView::Home,
                    true,
                    |snapshot| {
                        updates.send(snapshot).unwrap();
                    },
                    |_, _| {},
                )
                .await
        });
        let early = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let snapshot = observed.recv().await.unwrap();
                if snapshot
                    .folders
                    .get(1)
                    .is_some_and(|folder| !folder.items.is_empty())
                {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(early.folders[1].items, vec![item.clone()]);
        assert!(!early.folders[1].is_loading);
        assert!(early.folders[0].is_loading);
        assert!(!task.is_finished());
        assert_eq!(catalog.starts.lock().unwrap().len(), 3);
        assert!(catalog
            .starts
            .lock()
            .unwrap()
            .iter()
            .all(|(_, force)| *force));
        senders[1]
            .send(observation(
                Some(CatalogValue::RecommendationItems(vec![item.clone()])),
                CatalogLoadPhase::Ready,
            ))
            .unwrap();
        senders[2]
            .send(observation(
                Some(CatalogValue::RecommendationItems(vec![item])),
                CatalogLoadPhase::Failed,
            ))
            .unwrap();
        let failed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let snapshot = observed.recv().await.unwrap();
                if snapshot
                    .folders
                    .get(1)
                    .is_some_and(|folder| folder.has_load_error)
                {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        assert!(!failed.folders[1].items.is_empty());
        assert!(failed.has_load_error);
        senders[0]
            .send(observation(
                Some(CatalogValue::RecommendationFolders(Vec::new())),
                CatalogLoadPhase::Ready,
            ))
            .unwrap();
        drop(senders);
        let final_value = task.await.unwrap().unwrap();
        assert!(final_value.folders.is_empty());
        assert!(!final_value.is_loading);
    }

    #[tokio::test]
    async fn recommendation_playlist_reads_publish_out_of_order_without_losing_occurrences() {
        let catalog = Arc::new(ObservedCatalog {
            reads: Mutex::new(HashMap::new()),
            starts: Mutex::new(Vec::new()),
        });
        let mut senders = Vec::new();
        for query in [
            CatalogQuery::RecommendationFolders,
            CatalogQuery::PlaylistTracks {
                provider: "source".into(),
                item_id: MediaId("slow".into()),
                allow_dynamic_tracks: false,
            },
            CatalogQuery::PlaylistTracks {
                provider: "source".into(),
                item_id: MediaId("fast".into()),
                allow_dynamic_tracks: false,
            },
        ] {
            let (sender, receiver) = mpsc::unbounded_channel();
            catalog
                .reads
                .lock()
                .unwrap()
                .entry(format!("{query:?}"))
                .or_default()
                .push_back(receiver);
            senders.push(sender);
        }
        let folders = serde_json::from_value(serde_json::json!([{
            "provider":"source", "item_id":"user_exploration_weekly", "name":"Explore",
            "items":[
                {"media_type":"playlist", "provider":"source", "item_id":"slow", "name":"Slow"},
                {"media_type":"playlist", "provider":"source", "item_id":"fast", "name":"Fast"}
            ]
        }]))
        .unwrap();
        senders[0]
            .send(observation(
                Some(CatalogValue::RecommendationFolders(folders)),
                CatalogLoadPhase::Ready,
            ))
            .unwrap();
        let fast: Track = serde_json::from_value(serde_json::json!({"provider":"source", "item_id":"duplicate", "name":"Fast", "position":9})).unwrap();
        senders[2]
            .send(observation(
                Some(CatalogValue::Tracks(vec![fast.clone()])),
                CatalogLoadPhase::Ready,
            ))
            .unwrap();
        let service = Recommendations::new(catalog.clone());
        let (updates, mut observed) = mpsc::unbounded_channel();
        let read_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = read_count.clone();
        let task = tokio::spawn(async move {
            service
                .get_recommendation_data(
                    RecommendationView::Explore,
                    false,
                    |snapshot| {
                        updates.send(snapshot).unwrap();
                    },
                    |_, _| {
                        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    },
                )
                .await
        });
        let early = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let snapshot = observed.recv().await.unwrap();
                if snapshot
                    .folders
                    .first()
                    .is_some_and(|folder| !folder.playlist_tracks.is_empty())
                {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(early.folders[0].playlist_tracks, vec![fast.clone()]);
        assert!(!early.folders[0].is_loading);
        assert!(early.is_loading);
        assert!(!task.is_finished());
        assert_eq!(catalog.starts.lock().unwrap().len(), 3);
        let slow: Track = serde_json::from_value(serde_json::json!({"provider":"source", "item_id":"duplicate", "name":"Slow", "position":1})).unwrap();
        senders[1]
            .send(observation(
                Some(CatalogValue::Tracks(vec![slow.clone()])),
                CatalogLoadPhase::Failed,
            ))
            .unwrap();
        drop(senders);
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.folders[0].playlist_tracks, vec![slow, fast]);
        assert!(result.has_load_error);
        assert!(!result.is_loading);
        assert_eq!(read_count.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn recommendation_view_replacement_rejects_older_publications() {
        let catalog = Arc::new(ObservedCatalog {
            reads: Mutex::new(HashMap::new()),
            starts: Mutex::new(Vec::new()),
        });
        let (old_sender, old_receiver) = mpsc::unbounded_channel();
        let (new_sender, new_receiver) = mpsc::unbounded_channel();
        catalog.reads.lock().unwrap().insert(
            format!("{:?}", CatalogQuery::RecommendationFolders),
            std::collections::VecDeque::from([old_receiver, new_receiver]),
        );
        old_sender
            .send(observation(
                Some(CatalogValue::RecommendationFolders(Vec::new())),
                CatalogLoadPhase::Refreshing,
            ))
            .unwrap();
        let service = Recommendations::new(catalog);
        let (old_updates, mut old_observed) = mpsc::unbounded_channel();
        let mut old_request = Box::pin(service.get_recommendation_data(
            RecommendationView::Home,
            false,
            |snapshot| {
                old_updates.send(snapshot).unwrap();
            },
            |_, _| {},
        ));
        tokio::select! {
            result = &mut old_request => panic!("old request completed early: {result:?}"),
            snapshot = old_observed.recv() => assert!(snapshot.unwrap().is_loading),
        }
        new_sender
            .send(observation(
                Some(CatalogValue::RecommendationFolders(Vec::new())),
                CatalogLoadPhase::Ready,
            ))
            .unwrap();
        drop(new_sender);
        let mut new_updates = Vec::new();
        let current = service
            .get_recommendation_data(
                RecommendationView::Home,
                true,
                |snapshot| new_updates.push(snapshot),
                |_, _| {},
            )
            .await
            .unwrap();
        assert_eq!(new_updates, vec![current]);
        let folders = serde_json::from_value(serde_json::json!([{
            "provider":"source","item_id":"user_recommendations","name":"Old",
            "items":[{"media_type":"track","provider":"source","item_id":"old","name":"Old"}]
        }]))
        .unwrap();
        old_sender
            .send(observation(
                Some(CatalogValue::RecommendationFolders(folders)),
                CatalogLoadPhase::Ready,
            ))
            .unwrap();
        drop(old_sender);
        assert_eq!(old_request.await.unwrap().folders.len(), 1);
        assert!(old_observed.try_recv().is_err());
    }
}
