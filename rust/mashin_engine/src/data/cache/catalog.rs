//! Parallel migrated-media reads over the existing MA read-group scheduler.
//! Only active refreshes live in RAM. Last-consumer drop aborts unconfirmed network work;
//! accepted SQLite writes retain worker durability and leases reject old generations.
//! Confirmed mutations reserve owner-wide FIFO order before asynchronous cache prerequisites.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use futures_util::{stream::BoxStream, StreamExt};
use tokio::sync::watch;

use crate::application::repositories::{current_priority, with_request_priority, CatalogClient};
use crate::data::music_assistant::client::MusicAssistantClient;
use crate::data::music_assistant::request_scheduler::ReadGroup;
use crate::domain::catalog::{
    CatalogFreshness, CatalogLoadPhase, CatalogLoadSnapshot, CatalogQuery, CatalogReadKnowledge,
    CatalogValue,
};
use crate::domain::identifiers::UserId;

#[cfg(test)]
use super::artist_reads::{read_artist_read, read_partial_artist, write_artist_read};
use super::lifecycle::{DatabaseLease, ServerCache};
use super::reads::{
    read_cached_read, read_partial_cached, supports_cached_read, write_cached_read,
};

static LOGGER: crate::logging::Logger = crate::logging::Logger::new(module_path!());

pub(crate) struct CachedCatalog {
    inner: Arc<CatalogRuntime>,
}
struct CatalogRuntime {
    client: Arc<MusicAssistantClient>,
    access: CacheAccess,
    fresh_for: Duration,
    confirmed_revision: Arc<std::sync::atomic::AtomicU64>,
    confirmed_writes: Arc<tokio::sync::Mutex<()>>,
    refreshes: Mutex<HashMap<(CatalogQuery, bool, u64), Weak<Refresh>>>,
}
enum CacheAccess {
    Active(Arc<CacheContext>),
    Session(Arc<SessionCache>),
}
struct CacheContext {
    cache: ServerCache,
    database: DatabaseLease,
    user_id: UserId,
    expected_server_id: Option<String>,
    is_identity_verified: std::sync::atomic::AtomicBool,
}
struct Refresh {
    read_id: u64,
    started: Instant,
    query_kind: &'static str,
    group: Arc<ReadGroup>,
    changes: watch::Sender<CatalogLoadSnapshot>,
    updates: tokio::sync::broadcast::Sender<CatalogLoadSnapshot>,
    worker: Mutex<Option<tokio::task::AbortHandle>>,
}

enum RefreshError {
    Failed(String),
    Invalidated(String),
}

#[derive(Default)]
struct RefreshTimings {
    cache_status: Option<&'static str>,
    cache_ms: Option<u128>,
    cache_used_before_server: bool,
    server_succeeded: Option<bool>,
    server_ms: Option<u128>,
}

impl RefreshTimings {
    // One result per shared refresh; source transitions remain trace diagnostics.
    fn log_result(&self, refresh: &Refresh, state: &CatalogLoadSnapshot) {
        LOGGER.debug(format_args!(
            "{}",
            self.result_message(
                refresh.read_id,
                refresh.query_kind,
                state,
                refresh.started.elapsed().as_millis(),
            )
        ));
    }

    fn result_message(
        &self,
        read_id: u64,
        query_kind: &str,
        state: &CatalogLoadSnapshot,
        total_ms: u128,
    ) -> String {
        let cache_ms = self
            .cache_ms
            .map(|elapsed| elapsed.to_string())
            .unwrap_or_else(|| "none".into());
        let server_ms = self
            .server_ms
            .map(|elapsed| elapsed.to_string())
            .unwrap_or_else(|| "none".into());
        let server = self
            .server_succeeded
            .map(|succeeded| if succeeded { "ok" } else { "error" })
            .unwrap_or("not_completed");
        format!("read_id={read_id} query={query_kind} cache={} cache_ms={cache_ms} used_before_server={} server={server} server_ms={server_ms} phase={:?} cache_error={} total_ms={total_ms}", self.cache_status.unwrap_or("not_completed"), self.cache_used_before_server, state.phase, state.cache_error.is_some())
    }
}

impl RefreshError {
    fn from_cache(error: super::database::CacheError) -> Self {
        match error {
            super::database::CacheError::Invalidated => Self::Invalidated(error.to_string()),
            _ => Self::Failed(error.to_string()),
        }
    }
}

#[derive(serde::Deserialize)]
pub(super) struct ServerIdentity {
    pub(super) server_id: String,
}
impl Drop for Refresh {
    fn drop(&mut self) {
        if matches!(self.changes.borrow().phase, CatalogLoadPhase::ReadingLocal | CatalogLoadPhase::Refreshing) {
            LOGGER.trace(format_args!("read_id={} query={} stage=canceled elapsed_ms={}", self.read_id, self.query_kind, self.started.elapsed().as_millis()));
        }
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            worker.abort();
        }
    }
}

impl Refresh {
    // Keep the current state and preserve each source transition for active subscribers.
    fn publish(&self, mut snapshot: CatalogLoadSnapshot) {
        static NEXT_PUBLICATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        snapshot.publication_revision = NEXT_PUBLICATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.changes.send_replace(snapshot.clone());
        let _ = self.updates.send(snapshot);
    }
}

impl CachedCatalog {
    // Session-bound construction and observation.
    pub(crate) async fn new(
        client: Arc<MusicAssistantClient>,
        cache: ServerCache,
        user_id: UserId,
        fresh_for: Duration,
    ) -> Result<Self, String> {
        let database = cache.database().await.map_err(|error| error.to_string())?;
        Ok(Self {
            inner: Arc::new(CatalogRuntime {
                client,
                access: CacheAccess::Active(Arc::new(CacheContext {
                    cache,
                    database,
                    user_id,
                    expected_server_id: None,
                    is_identity_verified: std::sync::atomic::AtomicBool::new(true),
                })),
                fresh_for,
                confirmed_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                confirmed_writes: Arc::new(tokio::sync::Mutex::new(())),
                refreshes: Mutex::new(HashMap::new()),
            }),
        })
    }

    fn observe_cached(
        &self,
        query: CatalogQuery,
        force_refresh: bool,
    ) -> BoxStream<'static, CatalogLoadSnapshot> {
        let runtime = self.inner.clone();
        Box::pin(
            futures_util::stream::once(async move {
                let refresh = runtime.refresh(query, force_refresh);
                let receiver = refresh.updates.subscribe();
                let initial = refresh.changes.borrow().clone();
                Box::pin(futures_util::stream::unfold(
                    (refresh, receiver, Some(initial), false),
                    |(refresh, mut receiver, initial, is_done)| async move {
                        if is_done {
                            return None;
                        }
                        let snapshot = match initial
                            .filter(|snapshot| snapshot.phase != CatalogLoadPhase::ReadingLocal)
                        {
                            Some(snapshot) => snapshot,
                            None => match receiver.recv().await {
                                Ok(snapshot) => snapshot,
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    refresh.changes.borrow().clone()
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                    return None
                                }
                            },
                        };
                        let is_done = matches!(
                            snapshot.phase,
                            CatalogLoadPhase::Ready
                                | CatalogLoadPhase::Failed
                                | CatalogLoadPhase::Invalidated
                        );
                        Some((snapshot, (refresh, receiver, None, is_done)))
                    },
                )) as BoxStream<'static, CatalogLoadSnapshot>
            })
            .flatten(),
        )
    }
}

impl CachedCatalog {
    // Keep the remaining observation owned by the caller after the first local result.
    async fn read_providers<T: Send + 'static>(
        &self,
        query: CatalogQuery,
        extract: fn(CatalogValue) -> Option<Vec<T>>,
    ) -> Result<crate::application::repositories::ProviderRead<T>, String> {
        use crate::application::repositories::{
            ProviderRead, ProviderRefreshError, ProviderUpdate,
        };
        let mut updates = self.observe_cached(query, false);
        let first = updates
            .next()
            .await
            .ok_or("provider observation ended before local read")?;
        if first.phase == CatalogLoadPhase::Invalidated {
            return Err(first
                .error
                .unwrap_or_else(|| "provider session invalidated".into()));
        }
        let current = ProviderUpdate {
            items: first.value.map(Arc::unwrap_or_clone).and_then(extract),
            cache_error: first.cache_error,
        };
        let refresh = match first.phase {
            CatalogLoadPhase::Ready => None,
            CatalogLoadPhase::Failed => {
                let error = first
                    .error
                    .unwrap_or_else(|| "provider refresh failed".into());
                Some(
                    Box::pin(async move { Err(ProviderRefreshError::Failed(error)) })
                        as futures_util::future::BoxFuture<'static, _>,
                )
            }
            _ => Some(Box::pin(async move {
                while let Some(state) = updates.next().await {
                    match state.phase {
                        CatalogLoadPhase::Ready => {
                            return Ok(ProviderUpdate {
                                items: state.value.map(Arc::unwrap_or_clone).and_then(extract),
                                cache_error: state.cache_error,
                            })
                        }
                        CatalogLoadPhase::Failed => {
                            return Err(ProviderRefreshError::Failed(
                                state
                                    .error
                                    .unwrap_or_else(|| "provider refresh failed".into()),
                            ));
                        }
                        CatalogLoadPhase::Invalidated => {
                            return Err(ProviderRefreshError::Invalidated(
                                state
                                    .error
                                    .unwrap_or_else(|| "provider session invalidated".into()),
                            ))
                        }
                        _ => {}
                    }
                }
                Err(ProviderRefreshError::Failed(
                    "provider observation ended before completion".into(),
                ))
            })
                as futures_util::future::BoxFuture<'static, _>),
        };
        Ok(ProviderRead { current, refresh })
    }
}

impl crate::application::repositories::ProviderClient for CachedCatalog {
    // Provider reads share catalog ownership, scheduling and durable observations.
    fn read_manifests(
        &self,
    ) -> futures_util::future::BoxFuture<
        '_,
        Result<
            crate::application::repositories::ProviderRead<
                crate::domain::providers::ProviderManifest,
            >,
            String,
        >,
    > {
        Box::pin(
            self.read_providers(CatalogQuery::ProviderManifests {}, |value| match value {
                CatalogValue::ProviderManifests(items) => Some(items),
                _ => None,
            }),
        )
    }

    fn read_instances(
        &self,
    ) -> futures_util::future::BoxFuture<
        '_,
        Result<
            crate::application::repositories::ProviderRead<
                crate::domain::providers::ProviderInstance,
            >,
            String,
        >,
    > {
        Box::pin(
            self.read_providers(CatalogQuery::ProviderInstances, |value| match value {
                CatalogValue::ProviderInstances(items) => Some(items),
                _ => None,
            }),
        )
    }

    fn get_icon_data<'request>(
        &'request self,
        provider: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Option<String>, String>> {
        Box::pin(async move {
            let runtime = &self.inner;
            runtime
                .require_active()
                .map_err(|_| "provider session invalidated".to_owned())?;
            let context = runtime.cache_context().await;
            let images = if let Ok(context) = &context {
                let server_id = context.cache.observe().borrow().server_id.clone();
                if let Some(server_id) = server_id {
                    match context.cache.activate_images(server_id).await {
                        Ok(()) => context.cache.images().await.ok(),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let cached = if let Some(images) = &images {
                super::image_loader::read_provider_icon(images, provider)
                    .await
                    .ok()
            } else {
                None
            };
            let result = runtime.client.get_icon_data(provider).await;
            runtime
                .require_active()
                .map_err(|_| "provider session invalidated".to_owned())?;
            match result {
                Ok(icon) => {
                    if let (Ok(context), Some(images)) = (context, images.as_ref()) {
                        runtime
                            .verify_server_identity(&context, false)
                            .await
                            .map_err(|error| match error {
                                RefreshError::Failed(error) | RefreshError::Invalidated(error) => {
                                    error
                                }
                            })?;
                        if let Err(error) = super::image_loader::persist_provider_icon(
                            images,
                            provider,
                            icon.clone(),
                        )
                        .await
                        {
                            LOGGER.debug(format_args!("Provider icon persistence failed: {error}"));
                        }
                    }
                    runtime
                        .require_active()
                        .map_err(|_| "provider session invalidated".to_owned())?;
                    Ok(icon)
                }
                Err(error) => {
                    if let Some(images) = &images {
                        images.require_active().map_err(|error| error.to_string())?;
                    }
                    cached.ok_or(error)
                }
            }
        })
    }
}

impl CatalogRuntime {
    // Reserve confirmation order before waits; only inline cache publications share this guard.
    async fn begin_confirmed_write(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.confirmed_revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.confirmed_writes.lock().await
    }

    // Image files precede metadata; older refreshes cannot publish after a newer confirmation.
    async fn persist_inline_images(
        &self,
        context: &CacheContext,
        value: &mut CatalogValue,
        expected_revision: Option<u64>,
    ) -> Result<(), String> {
        use crate::domain::media::MediaItem;
        let has_manifest_icons = matches!(value,CatalogValue::ProviderManifests(manifests) if manifests.iter().any(|manifest| [&manifest.icon,&manifest.icon_svg,&manifest.icon_svg_dark,&manifest.icon_svg_monochrome].into_iter().flatten().any(|value|crate::domain::artwork::is_inline_source(value))));
        let playlists: Vec<_> = match value {
            CatalogValue::Playlist(playlist)
            | CatalogValue::Item(MediaItem::Playlist(playlist)) => vec![playlist],
            CatalogValue::Playlists(playlists) => playlists.iter_mut().collect(),
            CatalogValue::Search(results) => results.playlists.iter_mut().collect(),
            CatalogValue::RecommendationItems(items) => items
                .iter_mut()
                .filter_map(|item| match item {
                    MediaItem::Playlist(playlist) => Some(playlist),
                    _ => None,
                })
                .collect(),
            CatalogValue::RecommendationFolders(folders) => folders
                .iter_mut()
                .flat_map(|folder| folder.items.iter_mut())
                .filter_map(|item| match item {
                    MediaItem::Playlist(playlist) => Some(playlist),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let playlists: Vec<_> = playlists
            .into_iter()
            .filter(|playlist| {
                playlist.media.metadata.as_ref().is_some_and(|metadata| {
                    metadata
                        .images
                        .iter()
                        .any(crate::domain::artwork::is_inline_image)
                })
            })
            .collect();
        if playlists.is_empty() && !has_manifest_icons {
            return Ok(());
        }
        let _publication_order = if expected_revision.is_some() {
            Some(self.confirmed_writes.lock().await)
        } else {
            None
        };
        if expected_revision.is_some_and(|expected| {
            self.confirmed_revision
                .load(std::sync::atomic::Ordering::Acquire)
                != expected
        }) {
            return Err(super::database::CacheError::SupersededObservation.to_string());
        }
        context
            .database
            .require_active()
            .map_err(|error| error.to_string())?;
        let server_id = context
            .cache
            .observe()
            .borrow()
            .server_id
            .clone()
            .ok_or("image cache requires an observed server identity")?;
        context
            .cache
            .activate_images(server_id)
            .await
            .map_err(|error| error.to_string())?;
        let images = context
            .cache
            .images()
            .await
            .map_err(|error| error.to_string())?;
        context
            .database
            .require_active()
            .map_err(|error| error.to_string())?;
        for playlist in playlists {
            super::image_loader::persist_playlist_images(&images, playlist).await?;
        }
        if let CatalogValue::ProviderManifests(manifests) = value {
            for manifest in manifests {
                super::image_loader::persist_manifest_icons(&images, manifest).await?;
            }
        }
        Ok(())
    }

    // Retired sessions and leases cannot start or publish a new observation.
    fn require_active(&self) -> Result<(), RefreshError> {
        match &self.access {
            CacheAccess::Active(context) => context
                .database
                .require_active()
                .map_err(RefreshError::from_cache),
            CacheAccess::Session(session) => session
                .owner
                .require_generation(session.generation)
                .map_err(RefreshError::Invalidated),
        }
    }

    // Resolve storage independently from the network read owned by this coordinator.
    async fn cache_context(&self) -> Result<Arc<CacheContext>, RefreshError> {
        match &self.access {
            CacheAccess::Active(context) => Ok(context.clone()),
            CacheAccess::Session(session) => session.resolve().await.map_err(RefreshError::Failed),
        }
    }

    // Server identity, not its address, authorizes access to the generation-bound database.
    async fn verify_server_identity(
        &self,
        context: &CacheContext,
        force_verification: bool,
    ) -> Result<(), RefreshError> {
        if let Some(expected) = &context.expected_server_id {
            if force_verification
                || !context
                    .is_identity_verified
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                let identity: ServerIdentity = self
                    .client
                    .execute_command("info", serde_json::json!({}))
                    .await
                    .map_err(RefreshError::Failed)?;
                if identity.server_id != *expected {
                    context
                        .cache
                        .activate(identity.server_id)
                        .await
                        .map_err(RefreshError::from_cache)?;
                    return Err(RefreshError::Invalidated(
                        "server identity changed; reopen the session".to_owned(),
                    ));
                }
                context
                    .is_identity_verified
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }
        Ok(())
    }

    // Parallel reads share one publication and one atomic persistence attempt.
    fn refresh(self: &Arc<Self>, query: CatalogQuery, force_refresh: bool) -> Arc<Refresh> {
        let mut refreshes = self
            .refreshes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        refreshes.retain(|_, refresh| {
            refresh.upgrade().is_some_and(|refresh| {
                matches!(
                    refresh.changes.borrow().phase,
                    CatalogLoadPhase::ReadingLocal | CatalogLoadPhase::Refreshing
                )
            })
        });
        let expected_revision = self
            .confirmed_revision
            .load(std::sync::atomic::Ordering::Acquire);
        let key = (query.clone(), force_refresh, expected_revision);
        let requires_independent_refresh = match &query {
            CatalogQuery::LibraryArtists { filters, .. } => filters.requires_new_random_sample(),
            CatalogQuery::LibraryAlbums { filters, .. } => filters.requires_new_random_sample(),
            CatalogQuery::PlaylistTracks {
                allow_dynamic_tracks,
                ..
            } => *allow_dynamic_tracks,
            CatalogQuery::RecommendationItems { .. } => true,
            CatalogQuery::SimilarTracks {
                allow_lookup: true, ..
            } => true,
            _ => false,
        };
        if let Some(refresh) = refreshes
            .get(&key)
            .and_then(Weak::upgrade)
            .filter(|_| !requires_independent_refresh)
        {
            refresh.group.promote_to_current_priority();
            LOGGER.trace(format_args!("read_id={} query={} stage=joined", refresh.read_id, query.kind_name()));
            return refresh;
        }
        static NEXT_READ_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let read_id = NEXT_READ_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = Instant::now();
        LOGGER.trace(format_args!("read_id={read_id} query={} stage=started force_refresh={force_refresh}", query.kind_name()));
        let group = self.client.create_read_group();
        let (changes, _) = watch::channel(CatalogLoadSnapshot {
            publication_revision: 0,
            value: None,
            source: None,
            phase: CatalogLoadPhase::ReadingLocal,
            knowledge: CatalogReadKnowledge::Unknown,
            freshness: CatalogFreshness::Unknown,
            observed_at: None,
            error: None,
            cache_error: None,
        });
        let refresh = Arc::new(Refresh {
            read_id,
            started,
            query_kind: query.kind_name(),
            group: group.clone(),
            changes,
            updates: tokio::sync::broadcast::channel(8).0,
            worker: Mutex::new(None),
        });
        if let Err(RefreshError::Failed(error) | RefreshError::Invalidated(error)) =
            self.require_active()
        {
            let mut state = refresh.changes.borrow().clone();
            state.phase = CatalogLoadPhase::Invalidated;
            state.error = Some(error);
            RefreshTimings::default().log_result(&refresh, &state);
            refresh.publish(state);
            return refresh;
        }
        if !requires_independent_refresh {
            refreshes.insert(key, Arc::downgrade(&refresh));
        }
        let publication = Arc::downgrade(&refresh);
        let runtime = self.clone();
        let priority = current_priority();
        let worker = tokio::spawn(async move {
            with_request_priority(
                priority,
                group.scope(async {
                    let mut local = Box::pin(async {
                        tokio::time::timeout(Duration::from_secs(5), runtime.read_local(&query))
                            .await.map_err(|_| RefreshError::Failed("cache initialization/read exceeded its deadline".into()))?
                    });
                    let mut remote = Box::pin(crate::data::music_assistant::catalog::load_catalog(&runtime.client, &query, force_refresh));
                    let mut local_result = None;
                    let mut remote_result = None;
                    // A confirmed server response owns its pending persistence independently of display waiters.
                    let mut _persistence_owner = None;
                    let mut timings = RefreshTimings::default();
                    while local_result.is_none() || remote_result.is_none() {
                        tokio::select! {
                            result = &mut local, if local_result.is_none() => {
                                if runtime.require_active().is_err() { break; }
                                let (cache, knowledge, is_used, cache_error) = match &result {
                                    Ok((_, cached)) => (
                                        if cached.value.is_some() { "hit" } else { "miss" },
                                        cached.knowledge,
                                        cached.value.is_some() && publication.upgrade().is_some_and(|publication| publication.changes.borrow().source != Some(crate::domain::catalog::CatalogDataSource::Server)),
                                        cached.cache_error.is_some(),
                                    ),
                                    Err(_) => ("error", CatalogReadKnowledge::Unknown, false, true),
                                };
                                timings.cache_ms = Some(started.elapsed().as_millis());
                                timings.cache_status = Some(cache);
                                timings.cache_used_before_server = is_used;
                                LOGGER.trace(format_args!("read_id={read_id} query={} stage=local cache={cache} used_before_server={is_used} knowledge={knowledge:?} elapsed_ms={} cache_error={cache_error}", query.kind_name(), started.elapsed().as_millis()));
                                if let Some(publication) = publication.upgrade() {
                                    let mut state = publication.changes.borrow().clone();
                                    state.phase = CatalogLoadPhase::Refreshing;
                                    match &result {
                                        Ok((_, cached)) if state.source != Some(crate::domain::catalog::CatalogDataSource::Server) => {
                                            state.cache_error = cached.cache_error.clone();
                                            state.value = cached.value.clone();
                                            state.source = cached.source;
                                            state.knowledge = cached.knowledge;
                                            state.freshness = cached.freshness;
                                            state.observed_at = cached.observed_at;
                                        }
                                        Ok((_, cached)) => {
                                            state.cache_error = cached.cache_error.clone();
                                            if let Some(server_value) = state.value.as_ref() {
                                                state.value = Some(Arc::new(merge_server_value(cached.value.clone().map(Arc::unwrap_or_clone), server_value.as_ref().clone())));
                                            }
                                        }
                                        Err(RefreshError::Failed(error) | RefreshError::Invalidated(error)) => state.cache_error = Some(error.clone()),
                                    }
                                    publication.publish(state);
                                }
                                local_result = Some(result);
                            }
                            result = &mut remote, if remote_result.is_none() => {
                                if runtime.require_active().is_err() { break; }
                                timings.server_ms = Some(started.elapsed().as_millis());
                                timings.server_succeeded = Some(result.is_ok());
                                LOGGER.trace(format_args!("read_id={read_id} query={} stage=server elapsed_ms={} success={}", query.kind_name(), started.elapsed().as_millis(), result.is_ok()));
                                if let (Ok(value), Some(publication)) = (&result, publication.upgrade()) {
                                    _persistence_owner = Some(publication.clone());
                                    if runtime.confirmed_revision.load(std::sync::atomic::Ordering::Acquire) == expected_revision {
                                        let mut state = publication.changes.borrow().clone();
                                        state.phase = CatalogLoadPhase::Refreshing;
                                        state.value = Some(Arc::new(merge_server_value(state.value.take().map(Arc::unwrap_or_clone), value.clone())));
                                        state.source = Some(crate::domain::catalog::CatalogDataSource::Server);
                                        state.knowledge = CatalogReadKnowledge::ObservedResponse;
                                        state.freshness = CatalogFreshness::Fresh;
                                        state.observed_at = Some(chrono::Utc::now().timestamp());
                                        publication.publish(state);
                                    }
                                }
                                remote_result = Some(result);
                            }
                        }
                    }
                    if runtime.require_active().is_err() {
                        if let Some(publication) = publication.upgrade() {
                            let mut state = publication.changes.borrow().clone();
                            state.phase = CatalogLoadPhase::Invalidated;
                            state.value = None;
                            state.source = None;
                            state.error = Some("catalog observation invalidated".into());
                            state.knowledge = CatalogReadKnowledge::Unknown;
                            state.freshness = CatalogFreshness::Unknown;
                            state.observed_at = None;
                            timings.log_result(&publication, &state);
                            publication.publish(state);
                        }
                        return;
                    }
                    let local_result = local_result.unwrap();
                    let Some(publication) = publication.upgrade() else { return; };
                    let mut state = publication.changes.borrow().clone();
                    let mut value = match remote_result.unwrap() {
                        Ok(value) => value,
                        Err(error) => {
                            state.phase = CatalogLoadPhase::Failed;
                            state.error = Some(error);
                            timings.log_result(&publication, &state);
                            publication.publish(state);
                            return;
                        }
                    };
                    let persistence_started = Instant::now();
                    let persistence = match local_result {
                        Ok((context, _)) => {
                            let verify = runtime.verify_server_identity(&context, false);
                            match tokio::time::timeout(Duration::from_secs(5), verify).await {
                                Ok(Ok(())) => {
                                    match runtime.persist_inline_images(&context, &mut value, Some(expected_revision)).await {
                                    Err(error) => Err(RefreshError::Failed(error)),
                                    Ok(()) => {
                                    let write_query = query.clone();
                                    let user_id = context.user_id.clone();
                                    let revision = runtime.confirmed_revision.clone();
                                    context.database.write_transaction(move |transaction| {
                                        if revision.load(std::sync::atomic::Ordering::Acquire) != expected_revision {
                                            return Err(super::database::CacheError::SupersededObservation);
                                        }
                                        write_cached_read(transaction, &write_query, &user_id, &value, chrono::Utc::now().timestamp())
                                    }).await.map_err(RefreshError::from_cache)
                                    }
                                    }
                                }
                                Ok(Err(error)) => Err(error),
                                Err(_) => Err(RefreshError::Failed("cache identity verification exceeded its deadline".into())),
                            }
                        }
                        Err(error) => Err(error),
                    };
                    LOGGER.trace(format_args!("read_id={read_id} query={} stage=persistence_finished elapsed_ms={} success={}", query.kind_name(), persistence_started.elapsed().as_millis(), persistence.is_ok()));
                    if runtime.confirmed_revision.load(std::sync::atomic::Ordering::Acquire) != expected_revision {
                        state.phase = CatalogLoadPhase::Failed;
                        state.error = Some("catalog refresh superseded by confirmed mutation".into());
                        if let Ok(context) = runtime.cache_context().await {
                            let user_id = context.user_id.clone();
                            let latest_query = query.clone();
                            state.value = context.database.read_snapshot(move |snapshot| read_partial_cached(snapshot, &latest_query, &user_id)).await.ok().flatten().map(Arc::new);
                        } else { state.value = None; }
                        state.knowledge = if state.value.is_some() { CatalogReadKnowledge::Partial } else { CatalogReadKnowledge::Unknown };
                        state.freshness = CatalogFreshness::Unknown;
                        state.observed_at = None;
                    } else {
                        state.phase = CatalogLoadPhase::Ready;
                        match persistence {
                            Ok(()) => {}
                            Err(RefreshError::Failed(error)) => {
                                LOGGER.trace(format_args!("Cache persistence failed: {error}"));
                                state.cache_error = Some(error);
                            }
                            Err(RefreshError::Invalidated(error)) => {
                                state.phase = CatalogLoadPhase::Invalidated;
                                state.error = Some(error);
                                state.value = None;
                                state.source = None;
                                state.knowledge = CatalogReadKnowledge::Unknown;
                                state.freshness = CatalogFreshness::Unknown;
                                state.observed_at = None;
                            }
                        }
                    }
                    timings.log_result(&publication, &state);
                    publication.publish(state);
                }),
            )
            .await;
        });
        *refresh
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(worker.abort_handle());
        refresh
    }

    // Local observations never decide whether the online operation is needed.
    async fn read_local(
        &self,
        query: &CatalogQuery,
    ) -> Result<(Arc<CacheContext>, CatalogLoadSnapshot), RefreshError> {
        let context = self.cache_context().await?;
        let read_query = query.clone();
        let user_id = context.user_id.clone();
        let (observed, partial, manifest_slots) = context
            .database
            .read_snapshot(move |snapshot| {
                if matches!(read_query, CatalogQuery::ProviderManifests {}) {
                    let stored = super::providers::read_manifest_observation(snapshot, &user_id)?;
                    let observed = stored.as_ref().map(|(manifests, observed_at)| {
                        super::reads::CatalogReadObservation {
                            value: CatalogValue::ProviderManifests(
                                manifests
                                    .iter()
                                    .map(|stored| stored.manifest.clone())
                                    .collect(),
                            ),
                            observed_at: *observed_at,
                        }
                    });
                    return Ok((observed, None, stored.map(|(manifests, _)| manifests)));
                }
                let observed = read_cached_read(snapshot, &read_query, &user_id)?;
                let partial = if observed.is_none() {
                    read_partial_cached(snapshot, &read_query, &user_id)?
                } else {
                    None
                };
                Ok((observed, partial, None))
            })
            .await
            .map_err(RefreshError::from_cache)?;
        let now = chrono::Utc::now().timestamp();
        let is_fresh = context
            .is_identity_verified
            .load(std::sync::atomic::Ordering::Acquire)
            && observed.as_ref().is_some_and(|value| {
                value.observed_at <= now
                    && now - value.observed_at < self.fresh_for.as_secs() as i64
            });
        let mut value = observed
            .as_ref()
            .map(|value| value.value.clone())
            .or(partial);
        let mut cache_error = None;
        if let Some(stored) =
            manifest_slots.filter(|stored| stored.iter().any(|manifest| !manifest.icons.is_empty()))
        {
            let restore = async {
                let server_id = context
                    .cache
                    .observe()
                    .borrow()
                    .server_id
                    .clone()
                    .ok_or("image cache requires server identity")?;
                context
                    .cache
                    .activate_images(server_id)
                    .await
                    .map_err(|error| error.to_string())?;
                let images = context
                    .cache
                    .images()
                    .await
                    .map_err(|error| error.to_string())?;
                let mut manifests = Vec::new();
                for manifest in stored {
                    manifests.push(
                        super::image_loader::restore_manifest_icons(&images, manifest).await?,
                    );
                }
                Ok::<_, String>(CatalogValue::ProviderManifests(manifests))
            }
            .await;
            match restore {
                Ok(restored) => value = Some(restored),
                Err(error) => cache_error = Some(error),
            }
        }
        let snapshot = CatalogLoadSnapshot {
            publication_revision: 0,
            source: value
                .as_ref()
                .map(|_| crate::domain::catalog::CatalogDataSource::Cache),
            knowledge: if observed.is_some() {
                CatalogReadKnowledge::ObservedResponse
            } else if value.is_some() {
                CatalogReadKnowledge::Partial
            } else {
                CatalogReadKnowledge::Unknown
            },
            freshness: if is_fresh {
                CatalogFreshness::Fresh
            } else if observed.is_some() {
                CatalogFreshness::Stale
            } else {
                CatalogFreshness::Unknown
            },
            value: value.map(Arc::new),
            phase: CatalogLoadPhase::Refreshing,
            observed_at: observed.map(|value| value.observed_at),
            error: None,
            cache_error,
        };
        Ok((context, snapshot))
    }
}

// Preserve known fields only for matching occurrences; the server owns order and membership.
fn merge_server_value(previous: Option<CatalogValue>, incoming: CatalogValue) -> CatalogValue {
    use crate::domain::media::MediaItem;
    fn merge(previous: MediaItem, incoming: MediaItem) -> MediaItem {
        if previous.fields().provider != incoming.fields().provider
            || previous.fields().item_id != incoming.fields().item_id
        {
            return incoming;
        }
        let mut previous = previous;
        previous.merge_known(incoming);
        previous
    }
    macro_rules! single {
        ($variant:ident, $previous:expr, $incoming:expr) => {{
            let MediaItem::$variant(item) = merge(
                MediaItem::$variant($previous),
                MediaItem::$variant($incoming),
            ) else {
                unreachable!()
            };
            CatalogValue::$variant(item)
        }};
    }
    macro_rules! list {
        ($variant:ident, $item:ident, $previous:expr, $incoming:expr) => {{
            CatalogValue::$variant(
                $incoming
                    .into_iter()
                    .enumerate()
                    .map(|(position, incoming)| {
                        let Some(previous) = $previous.get(position) else {
                            return incoming;
                        };
                        let MediaItem::$item(item) = merge(
                            MediaItem::$item(previous.clone()),
                            MediaItem::$item(incoming),
                        ) else {
                            unreachable!()
                        };
                        item
                    })
                    .collect(),
            )
        }};
    }
    match (previous, incoming) {
        (
            Some(CatalogValue::ProviderManifests(previous)),
            CatalogValue::ProviderManifests(incoming),
        ) => {
            let mut known: HashMap<_, _> = previous
                .into_iter()
                .map(|manifest| (manifest.domain.clone(), manifest))
                .collect();
            CatalogValue::ProviderManifests(
                incoming
                    .into_iter()
                    .map(|manifest| {
                        if let Some(mut previous) = known.remove(&manifest.domain) {
                            previous.merge_known(manifest);
                            previous
                        } else {
                            manifest
                        }
                    })
                    .collect(),
            )
        }
        (Some(CatalogValue::Artist(previous)), CatalogValue::Artist(incoming)) => {
            single!(Artist, previous, incoming)
        }
        (Some(CatalogValue::Album(previous)), CatalogValue::Album(incoming)) => {
            single!(Album, previous, incoming)
        }
        (Some(CatalogValue::Item(previous)), CatalogValue::Item(incoming)) => {
            CatalogValue::Item(merge(previous, incoming))
        }
        (Some(CatalogValue::Tracks(previous)), CatalogValue::Tracks(incoming)) => {
            list!(Tracks, Track, previous, incoming)
        }
        (Some(CatalogValue::Albums(previous)), CatalogValue::Albums(incoming)) => {
            list!(Albums, Album, previous, incoming)
        }
        (Some(CatalogValue::Artists(previous)), CatalogValue::Artists(incoming)) => {
            list!(Artists, Artist, previous, incoming)
        }
        (Some(CatalogValue::Search(previous)), CatalogValue::Search(mut incoming)) => {
            if let CatalogValue::Artists(artists) = merge_server_value(
                Some(CatalogValue::Artists(previous.artists)),
                CatalogValue::Artists(std::mem::take(&mut incoming.artists)),
            ) {
                incoming.artists = artists;
            }
            if let CatalogValue::Albums(albums) = merge_server_value(
                Some(CatalogValue::Albums(previous.albums)),
                CatalogValue::Albums(std::mem::take(&mut incoming.albums)),
            ) {
                incoming.albums = albums;
            }
            if let CatalogValue::Tracks(tracks) = merge_server_value(
                Some(CatalogValue::Tracks(previous.tracks)),
                CatalogValue::Tracks(std::mem::take(&mut incoming.tracks)),
            ) {
                incoming.tracks = tracks;
            }
            CatalogValue::Search(incoming)
        }
        (_, incoming) => incoming,
    }
}

impl CatalogClient for CachedCatalog {
    fn cache_usage(&self) -> futures_util::future::BoxFuture<'_, Result<crate::application::repositories::CacheUsage, String>> {
        Box::pin(async move {
            match &self.inner.access {
                CacheAccess::Session(session) => {
                    let mut owner = session.owner.cache.lock().await;
                    session.owner.require_generation(session.generation)?;
                    if owner.is_none() {
                        *owner = Some(ServerCache::initialize((session.owner.directory)()?).await.map_err(|error| error.to_string())?);
                    }
                    owner.as_ref().unwrap().usage().await.map_err(|error| error.to_string())
                }
                CacheAccess::Active(context) => context.cache.usage().await.map_err(|error| error.to_string()),
            }
        })
    }

    fn clear_database_cache(&self) -> futures_util::future::BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let _order = self.inner.begin_confirmed_write().await;
            let context = self
                .inner
                .cache_context()
                .await
                .map_err(|error| match error {
                    RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
                })?;
            self.inner.require_active().map_err(|error| match error {
                RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
            })?;
            match &self.inner.access {
                CacheAccess::Session(session) => {
                    session
                        .owner
                        .clear_cache_for_generation(
                            session.generation,
                            super::lifecycle::CacheMaintenance::ClearDatabase,
                        )
                        .await
                }
                CacheAccess::Active(_) => context
                    .cache
                    .clear_database_cache()
                    .await
                    .map_err(|error| error.to_string()),
            }
        })
    }

    // Confirmed online writes retire older in-flight observations before cache publication.
    fn commit_confirmed_media(
        &self,
        item: crate::domain::media::MediaItem,
    ) -> futures_util::future::BoxFuture<'_, Result<(), String>> {
        let runtime = self.inner.clone();
        Box::pin(async move {
            if !matches!(
                &item,
                crate::domain::media::MediaItem::Artist(_)
                    | crate::domain::media::MediaItem::Album(_)
                    | crate::domain::media::MediaItem::Track(_)
                    | crate::domain::media::MediaItem::Playlist(_)
            ) {
                return Err(super::database::CacheError::InvalidMediaObservation(
                    "confirmed media cache commit requires an artist, album, track or playlist",
                )
                .to_string());
            }
            let _write_order = runtime.begin_confirmed_write().await;
            let context = runtime.cache_context().await.map_err(|error| match error {
                RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
            })?;
            runtime
                .verify_server_identity(&context, true)
                .await
                .map_err(|error| match error {
                    RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
                })?;
            let mut value = CatalogValue::Item(item);
            runtime
                .persist_inline_images(&context, &mut value, None)
                .await?;
            let CatalogValue::Item(item) = value else {
                unreachable!()
            };
            let user_id = context.user_id.clone();
            context
                .database
                .write_transaction(move |transaction| {
                    match item {
                        crate::domain::media::MediaItem::Artist(artist) => {
                            super::artist_reads::commit_confirmed_artist(
                                transaction,
                                &artist,
                                &user_id,
                            )?;
                        }
                        crate::domain::media::MediaItem::Album(album) => {
                            super::album_reads::commit_confirmed_album(
                                transaction,
                                &album,
                                &user_id,
                            )?;
                        }
                        crate::domain::media::MediaItem::Track(track) => {
                            super::track_reads::commit_confirmed_track(
                                transaction,
                                &track,
                                &user_id,
                            )?;
                        }
                        crate::domain::media::MediaItem::Playlist(playlist) => {
                            super::playlist_reads::commit_confirmed_playlist(
                                transaction,
                                &playlist,
                                &user_id,
                            )?;
                        }
                        _ => unreachable!(),
                    }
                    Ok(())
                })
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn commit_confirmed_playlist_removal(
        &self,
        playlist: crate::domain::media::Playlist,
    ) -> futures_util::future::BoxFuture<'_, Result<(), String>> {
        let runtime = self.inner.clone();
        Box::pin(async move {
            if playlist.media.provider.as_str() != "library"
                || playlist.media.item_id.as_str().is_empty()
            {
                return Err("playlist removal requires a library identity".into());
            }
            let _write_order = runtime.begin_confirmed_write().await;
            let context = runtime.cache_context().await.map_err(|error| match error {
                RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
            })?;
            runtime
                .verify_server_identity(&context, true)
                .await
                .map_err(|error| match error {
                    RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
                })?;
            let user_id = context.user_id.clone();
            context
                .database
                .write_transaction(move |transaction| {
                    super::playlist_reads::commit_confirmed_playlist_removal(
                        transaction,
                        &playlist,
                        &user_id,
                    )
                })
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn read_confirmed_playlist_contents(
        &self,
        playlist: crate::domain::media::Playlist,
    ) -> futures_util::future::BoxFuture<'_, Result<Vec<crate::domain::media::Track>, String>> {
        let runtime = self.inner.clone();
        Box::pin(async move {
            runtime.require_active().map_err(|error| match error {
                RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
            })?;
            let query = CatalogQuery::PlaylistTracks {
                item_id: playlist.media.item_id.clone(),
                provider: playlist.media.provider.as_str().to_owned(),
                allow_dynamic_tracks: false,
            };
            let CatalogValue::Tracks(tracks) = runtime.client.load_catalog(query, true).await?
            else {
                return Err("playlist tracks unavailable".into());
            };
            runtime.require_active().map_err(|error| match error {
                RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
            })?;
            Ok(tracks)
        })
    }

    fn commit_confirmed_playlist_contents(
        &self,
        playlist: crate::domain::media::Playlist,
        tracks: Vec<crate::domain::media::Track>,
    ) -> futures_util::future::BoxFuture<'_, Result<(), String>> {
        let runtime = self.inner.clone();
        Box::pin(async move {
            let _write_order = runtime.begin_confirmed_write().await;
            let query = CatalogQuery::PlaylistTracks {
                item_id: playlist.media.item_id.clone(),
                provider: playlist.media.provider.as_str().to_owned(),
                allow_dynamic_tracks: false,
            };
            let context = runtime.cache_context().await.map_err(|error| match error {
                RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
            })?;
            runtime
                .verify_server_identity(&context, true)
                .await
                .map_err(|error| match error {
                    RefreshError::Failed(error) | RefreshError::Invalidated(error) => error,
                })?;
            let mut value = CatalogValue::Playlist(playlist);
            runtime
                .persist_inline_images(&context, &mut value, None)
                .await?;
            let CatalogValue::Playlist(playlist) = value else {
                unreachable!()
            };
            let user_id = context.user_id.clone();
            let observed_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_secs();
            let observed_at = i64::try_from(observed_at).map_err(|error| error.to_string())?;
            context
                .database
                .write_transaction(move |transaction| {
                    super::track_reads::invalidate_confirmed_playlist_content_reads(
                        transaction,
                        &playlist,
                    )?;
                    super::playlist_reads::commit_confirmed_playlist(
                        transaction,
                        &playlist,
                        &user_id,
                    )?;
                    super::track_reads::invalidate_confirmed_playlist_content_reads(
                        transaction,
                        &playlist,
                    )?;
                    write_cached_read(
                        transaction,
                        &query,
                        &user_id,
                        &CatalogValue::Tracks(tracks),
                        observed_at,
                    )
                })
                .await
                .map_err(|error| error.to_string())
        })
    }

    // Command preparation and confirmed readbacks await completion; displays observe local data first.
    fn begin_startup(&self) -> Box<dyn Send> {
        self.inner.client.begin_startup()
    }
    fn load_catalog(
        &self,
        query: CatalogQuery,
        force_refresh: bool,
    ) -> futures_util::future::BoxFuture<'_, Result<CatalogValue, String>> {
        Box::pin(async move {
            let mut updates = self.observe_catalog(query, force_refresh);
            while let Some(state) = updates.next().await {
                match state.phase {
                    CatalogLoadPhase::Ready => {
                        return state
                            .value
                            .map(Arc::unwrap_or_clone)
                            .ok_or_else(|| "ready catalog read has no value".to_owned())
                    }
                    CatalogLoadPhase::Failed | CatalogLoadPhase::Invalidated => {
                        return Err(state
                            .error
                            .unwrap_or_else(|| "catalog refresh failed".to_owned()))
                    }
                    _ => {}
                }
            }
            Err("catalog observation ended before completion".to_owned())
        })
    }
    fn observe_catalog(
        &self,
        query: CatalogQuery,
        force_refresh: bool,
    ) -> BoxStream<'_, CatalogLoadSnapshot> {
        if supports_cached_read(&query) {
            self.observe_cached(query, force_refresh)
        } else {
            LOGGER.trace(format_args!("query={} cache=bypass reason=unsupported force_refresh={force_refresh}", query.kind_name()));
            self.inner.client.observe_catalog(query, force_refresh)
        }
    }
}

pub(crate) struct SessionCacheOwner {
    directory: Arc<dyn Fn() -> Result<std::path::PathBuf, String> + Send + Sync>,
    generation: std::sync::atomic::AtomicU64,
    retired_generation: std::sync::atomic::AtomicU64,
    cache: tokio::sync::Mutex<Option<ServerCache>>,
    confirmed_writes: Arc<tokio::sync::Mutex<()>>,
}

struct SessionCache {
    owner: Arc<SessionCacheOwner>,
    client: Arc<MusicAssistantClient>,
    user_id: Option<UserId>,
    generation: u64,
    resolved: tokio::sync::Mutex<Option<Arc<CacheContext>>>,
}

impl SessionCacheOwner {
    // A single app-session factory owns the cache across user and address changes.
    pub(crate) fn new(
        directory: impl Fn() -> Result<std::path::PathBuf, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            directory: Arc::new(directory),
            generation: std::sync::atomic::AtomicU64::new(0),
            retired_generation: std::sync::atomic::AtomicU64::new(u64::MAX),
            cache: tokio::sync::Mutex::new(None),
            confirmed_writes: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
    pub(crate) fn session(
        self: &Arc<Self>,
        client: Arc<MusicAssistantClient>,
        user_id: Option<UserId>,
        generation: u64,
    ) -> CachedCatalog {
        self.generation
            .fetch_max(generation, std::sync::atomic::Ordering::AcqRel);
        CachedCatalog {
            inner: Arc::new(CatalogRuntime {
                client: client.clone(),
                access: CacheAccess::Session(Arc::new(SessionCache {
                    owner: self.clone(),
                    client,
                    user_id,
                    generation,
                    resolved: tokio::sync::Mutex::new(None),
                })),
                fresh_for: Duration::from_secs(300),
                confirmed_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                confirmed_writes: self.confirmed_writes.clone(),
                refreshes: Mutex::new(HashMap::new()),
            }),
        }
    }
    pub(crate) async fn shutdown(&self, generation: u64) -> Result<(), String> {
        self.retired_generation
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |retired| {
                    Some(if retired == u64::MAX {
                        generation
                    } else {
                        retired.max(generation)
                    })
                },
            )
            .ok();
        self.generation
            .fetch_max(generation, std::sync::atomic::Ordering::AcqRel);
        let mut owner = self.cache.lock().await;
        if self.generation.load(std::sync::atomic::Ordering::Acquire) > generation {
            return Ok(());
        }
        if let Some(cache) = owner.take() {
            cache.shutdown().await.map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) async fn clear_cache_for_generation(
        &self,
        generation: u64,
        operation: super::lifecycle::CacheMaintenance,
    ) -> Result<(), String> {
        let owner = self.cache.lock().await;
        self.require_generation(generation)?;
        if let Some(cache) = owner.as_ref() {
            match operation {
                super::lifecycle::CacheMaintenance::ClearDatabase => {
                    cache.clear_database_cache().await
                }
                super::lifecycle::CacheMaintenance::ClearImages => cache.clear_image_cache().await,
                _ => return Err("unsupported session cache maintenance".into()),
            }
            .map_err(|error| error.to_string())?;
        }
        self.require_generation(generation)
    }

    // Shared cache-root admission does not require opening the media database.
    pub(super) fn require_generation(&self, generation: u64) -> Result<(), String> {
        let retired = self
            .retired_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if self.generation.load(std::sync::atomic::Ordering::Acquire) != generation
            || (retired != u64::MAX && generation <= retired)
        {
            return Err("cache belongs to a retired session".into());
        }
        Ok(())
    }

    pub(super) async fn cache_for_generation(
        &self,
        generation: u64,
    ) -> Result<ServerCache, String> {
        let mut owner = self.cache.lock().await;
        self.require_generation(generation)?;
        if owner.is_none() {
            *owner = Some(
                ServerCache::initialize((self.directory)()?)
                    .await
                    .map_err(|error| error.to_string())?,
            );
        }
        self.require_generation(generation)?;
        Ok(owner.as_ref().unwrap().clone())
    }

    pub(super) async fn image_lease(
        &self,
        client: &MusicAssistantClient,
        generation: u64,
    ) -> Result<(super::lifecycle::ImageLease, String, bool), String> {
        let cache = self.cache_for_generation(generation).await?;
        let observed_id = cache
            .server_id_for_address(client.base_url().to_owned())
            .await
            .map_err(|error| error.to_string())?;
        let (server_id, is_verified) = match observed_id {
            Some(server_id) => (server_id, false),
            None => {
                let identity: ServerIdentity = client
                    .execute_command("info", serde_json::json!({}))
                    .await?;
                (identity.server_id, true)
            }
        };
        let _owner = self.cache.lock().await;
        self.require_generation(generation)?;
        cache
            .activate_images(server_id.clone())
            .await
            .map_err(|error| error.to_string())?;
        if is_verified {
            cache
                .bind_server_address(server_id.clone(), client.base_url().to_owned())
                .await
                .map_err(|error| error.to_string())?;
        }
        let images = cache.images().await.map_err(|error| error.to_string())?;
        self.require_generation(generation)?;
        Ok((images, server_id, is_verified))
    }
}

impl SessionCache {
    // Identity lookup and filesystem initialization happen only for migrated media reads.
    async fn resolve(&self) -> Result<Arc<CacheContext>, String> {
        self.owner.require_generation(self.generation)?;
        let mut resolved = self.resolved.lock().await;
        if let Some(catalog) = resolved
            .as_ref()
            .filter(|catalog| catalog.database.require_active().is_ok())
        {
            return Ok(catalog.clone());
        }
        let catalog = async {
            LOGGER.trace(format_args!("Cache initialization: resolving user"));
            let user_id = match self.user_id.clone() {
                Some(user_id) => user_id,
                None => {
                    crate::application::repositories::LibraryClient::get_current_user_id(
                        self.client.as_ref(),
                    )
                    .await?
                }
            };
            let cache = self.owner.cache_for_generation(self.generation).await?;
            let observed_id = cache
                .server_id_for_address(self.client.base_url().to_owned())
                .await
                .map_err(|error| error.to_string())?;
            let (server_id, is_verified) = if let Some(server_id) = observed_id {
                (server_id, false)
            } else {
                LOGGER.debug(format_args!(
                    "Cache initialization: requesting server identity"
                ));
                let identity: ServerIdentity = self
                    .client
                    .execute_command("info", serde_json::json!({}))
                    .await?;
                (identity.server_id, true)
            };
            let _owner = self.owner.cache.lock().await;
            self.owner.require_generation(self.generation)?;
            LOGGER.trace(format_args!("Cache initialization: activating database"));
            cache
                .activate(server_id.clone())
                .await
                .map_err(|error| error.to_string())?;
            if is_verified {
                cache
                    .bind_server_address(server_id.clone(), self.client.base_url().to_owned())
                    .await
                    .map_err(|error| error.to_string())?;
            }
            let database = cache.database().await.map_err(|error| error.to_string())?;
            self.owner.require_generation(self.generation)?;
            LOGGER.trace(format_args!("Cache initialization: database ready"));
            Ok::<_, String>(Arc::new(CacheContext {
                cache,
                database,
                user_id,
                expected_server_id: Some(server_id),
                is_identity_verified: std::sync::atomic::AtomicBool::new(is_verified),
            }))
        }
        .await?;
        self.owner.require_generation(self.generation)?;
        *resolved = Some(catalog.clone());
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::music_assistant::client::tests::{server, server_with_request_handler};
    use crate::domain::identifiers::MediaId;
    use serde_json::json;

    #[test]
    fn refresh_result_reports_cache_and_server_timings_in_one_line() {
        let timings = RefreshTimings {
            cache_status: Some("hit"),
            cache_ms: Some(375),
            cache_used_before_server: true,
            server_succeeded: Some(true),
            server_ms: Some(16370),
        };
        let state = CatalogLoadSnapshot {
            publication_revision: 0,
            value: None,
            source: None,
            phase: CatalogLoadPhase::Ready,
            knowledge: CatalogReadKnowledge::ObservedResponse,
            freshness: CatalogFreshness::Fresh,
            observed_at: None,
            error: None,
            cache_error: None,
        };
        let message = timings.result_message(12, "PlaylistTracks", &state, 17231);
        assert_eq!(message, "read_id=12 query=PlaylistTracks cache=hit cache_ms=375 used_before_server=true server=ok server_ms=16370 phase=Ready cache_error=false total_ms=17231");
        assert_eq!(message.lines().count(), 1);
    }

    #[test]
    fn refresh_result_distinguishes_cache_misses_and_source_errors() {
        let mut state = CatalogLoadSnapshot {
            publication_revision: 0,
            value: None,
            source: None,
            phase: CatalogLoadPhase::Ready,
            knowledge: CatalogReadKnowledge::Unknown,
            freshness: CatalogFreshness::Unknown,
            observed_at: None,
            error: None,
            cache_error: None,
        };
        let mut timings = RefreshTimings {
            cache_status: Some("miss"),
            cache_ms: Some(2),
            cache_used_before_server: false,
            server_succeeded: Some(true),
            server_ms: Some(1486),
        };
        assert_eq!(timings.result_message(55, "ArtistAlbums", &state, 1511), "read_id=55 query=ArtistAlbums cache=miss cache_ms=2 used_before_server=false server=ok server_ms=1486 phase=Ready cache_error=false total_ms=1511");
        timings.cache_status = Some("error");
        timings.server_succeeded = Some(false);
        state.phase = CatalogLoadPhase::Failed;
        state.error = Some("synthetic server failure".into());
        state.cache_error = Some("synthetic cache failure".into());
        assert_eq!(timings.result_message(55, "ArtistAlbums", &state, 1511), "read_id=55 query=ArtistAlbums cache=error cache_ms=2 used_before_server=false server=error server_ms=1486 phase=Failed cache_error=true total_ms=1511");
    }

    #[test]
    fn refresh_result_does_not_fabricate_timings_for_unfinished_sources() {
        let state = CatalogLoadSnapshot {
            publication_revision: 0,
            value: None,
            source: None,
            phase: CatalogLoadPhase::Invalidated,
            knowledge: CatalogReadKnowledge::Unknown,
            freshness: CatalogFreshness::Unknown,
            observed_at: None,
            error: Some("session invalidated".into()),
            cache_error: None,
        };
        assert_eq!(RefreshTimings::default().result_message(1, "Album", &state, 0), "read_id=1 query=Album cache=not_completed cache_ms=none used_before_server=false server=not_completed server_ms=none phase=Invalidated cache_error=false total_ms=0");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_provider_icon_survives_offline_reopen_and_confirmed_absence() {
        use crate::application::repositories::ProviderClient;
        use base64::Engine;
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let bytes = super::super::image_files::tests::bitmap(11);
        let icon = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        let (client, worker) = server(vec![("providers/icon", json!(icon))]);
        let client = Arc::new(client);
        let catalog = CachedCatalog::new(
            client.clone(),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(catalog.get_icon_data("builtin").await.unwrap().is_some());
        drop(catalog);
        worker.join().unwrap();
        cache.shutdown().await.unwrap();
        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".into()).await.unwrap();
        let offline = CachedCatalog::new(
            client,
            reopened.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let cached = offline.get_icon_data("builtin").await.unwrap().unwrap();
        assert_eq!(
            super::super::image_files::ImagePayload::decode_inline(&cached)
                .unwrap()
                .bytes,
            bytes
        );
        drop(offline);
        let (client, worker) = server(vec![("providers/icon", json!(null))]);
        let catalog = CachedCatalog::new(
            Arc::new(client),
            reopened.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(catalog.get_icon_data("builtin").await.unwrap().is_none());
        drop(catalog);
        worker.join().unwrap();
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_manifests_and_icons_survive_offline_reopen_and_merge_partial_refresh() {
        use crate::application::repositories::ProviderClient;
        use base64::Engine;
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let bytes = super::super::image_files::tests::bitmap(9);
        let icon = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        let (client, worker) = server(vec![(
            "providers/manifests",
            json!([{"domain":"builtin","name":"Original","icon_svg":icon,"builtin":true}]),
        )]);
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(
            catalog
                .read_manifests()
                .await
                .unwrap()
                .resolve()
                .await
                .unwrap()
                .items
                .unwrap()[0]
                .name
                .as_deref(),
            Some("Original")
        );
        drop(catalog);
        worker.join().unwrap();
        cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|transaction| {
                let metadata: String = transaction.query_row(
                    "SELECT metadata FROM provider_manifests",
                    [],
                    |row| row.get(0),
                )?;
                assert!(!metadata.contains("base64"));
                assert!(!metadata.contains("data:"));
                Ok(())
            })
            .await
            .unwrap();
        cache.shutdown().await.unwrap();
        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".into()).await.unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker_entered = entered.clone();
        let worker_release = release.clone();
        let (client, worker) = server_with_request_handler(
            vec![(
                "providers/manifests",
                json!([{"domain":"builtin","name":"Updated"}]),
            )],
            move |_| {
                worker_entered.notify_one();
                worker_release.wait();
            },
        );
        let client = Arc::new(client);
        let catalog = CachedCatalog::new(
            client.clone(),
            reopened.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut updates = catalog.observe_catalog(CatalogQuery::ProviderManifests {}, false);
        let local = updates.next().await.unwrap();
        let Some(CatalogValue::ProviderManifests(manifests)) = local.value.as_deref().cloned() else {
            panic!("local manifests missing")
        };
        assert_eq!(manifests[0].name.as_deref(), Some("Original"));
        assert_eq!(
            super::super::image_files::ImagePayload::decode_inline(
                manifests[0].icon_svg.as_deref().unwrap()
            )
            .unwrap()
            .bytes,
            bytes
        );
        entered.notified().await;
        release.wait();
        let terminal = loop {
            let state = updates.next().await.unwrap();
            if state.phase == CatalogLoadPhase::Ready {
                break state;
            }
        };
        let Some(CatalogValue::ProviderManifests(manifests)) = terminal.value.as_deref().cloned() else {
            panic!("server manifests missing")
        };
        assert_eq!(manifests[0].name.as_deref(), Some("Updated"));
        assert_eq!(manifests[0].builtin, Some(true));
        assert!(manifests[0].icon_svg.is_some());
        drop(updates);
        drop(catalog);
        worker.join().unwrap();
        let offline = CachedCatalog::new(
            client,
            reopened.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let manifests = offline
            .read_manifests()
            .await
            .unwrap()
            .resolve()
            .await
            .unwrap()
            .items
            .unwrap();
        assert_eq!(manifests[0].name.as_deref(), Some("Updated"));
        assert!(manifests[0].icon_svg.is_some());
        drop(offline);
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_instances_publish_cache_before_remote_preserve_errors_and_reopen_per_user() {
        use crate::application::repositories::ProviderClient;
        use crate::domain::providers::ProviderInstance;
        for should_fail in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let old: Vec<ProviderInstance> = serde_json::from_value(
                json!([{"instance_id":"visible-instance","available":true}]),
            )
            .unwrap();
            let seed = old.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    super::super::providers::write_instances(
                        transaction,
                        &UserId("user".into()),
                        &seed,
                        100,
                    )?;
                    super::super::providers::write_instances(
                        transaction,
                        &UserId("other".into()),
                        &seed,
                        100,
                    )
                })
                .await
                .unwrap();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let worker_entered = entered.clone();
            let worker_release = release.clone();
            let (client, worker) = server_with_request_handler(
                vec![(
                    "providers",
                    if should_fail {
                        json!({"test_http_status":503})
                    } else {
                        json!([])
                    },
                )],
                move |request| {
                    assert_eq!(request["command"], "providers");
                    assert_eq!(request["args"], json!({}));
                    worker_entered.notify_one();
                    worker_release.wait();
                },
            );
            let client = Arc::new(client);
            let catalog = CachedCatalog::new(
                client.clone(),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let mut updates = catalog.observe_catalog(CatalogQuery::ProviderInstances, false);
            let local = updates.next().await.unwrap();
            assert_eq!(
                local.value,
                Some(Arc::new(CatalogValue::ProviderInstances(old.clone())))
            );
            assert_eq!(local.phase, CatalogLoadPhase::Refreshing);
            entered.notified().await;
            let early_instances =
                tokio::time::timeout(Duration::from_millis(100), catalog.read_instances()).await;
            drop(updates);
            release.wait();
            let early_instances = early_instances.unwrap().unwrap();
            assert_eq!(early_instances.current.items, Some(old.clone()));
            let expected = if should_fail { old.clone() } else { Vec::new() };
            let refreshed = early_instances.refresh.unwrap().await;
            if should_fail {
                assert!(refreshed.is_err());
            } else {
                assert_eq!(refreshed.unwrap().items, Some(expected.clone()));
            }
            drop(catalog);
            cache.shutdown().await.unwrap();
            let requests = worker.join().unwrap();
            assert_eq!(requests.len(), 1);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            let expected_other = old.clone();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |transaction| {
                    assert_eq!(
                        super::super::providers::read_instances(
                            transaction,
                            &UserId("user".into())
                        )?,
                        Some(expected)
                    );
                    assert_eq!(
                        super::super::providers::read_instances(
                            transaction,
                            &UserId("other".into())
                        )?,
                        Some(expected_other)
                    );
                    Ok(())
                })
                .await
                .unwrap();
            let offline = CachedCatalog::new(
                client,
                reopened.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            assert_eq!(
                offline
                    .read_instances()
                    .await
                    .unwrap()
                    .resolve()
                    .await
                    .unwrap()
                    .items
                    .unwrap(),
                if should_fail { old } else { Vec::new() }
            );
            drop(offline);
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn confirmed_playlist_contents_commit_atomically_and_preserve_server_success() {
        use crate::domain::media::Playlist;
        for (should_fail_commit, should_fail_readback, is_empty) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let original: Playlist = serde_json::from_value(json!({
                "provider":"library","item_id":"42","name":"Old playlist",
                "provider_mappings":[{"provider_instance":"source-a","provider_domain":"domain-a","item_id":"remote"}]
            })).unwrap();
            let mut confirmed = original.clone();
            confirmed.media.name = "Confirmed playlist".into();
            confirmed.media.provider_mappings = serde_json::from_value(json!([
                {"provider_instance":"source-b","provider_domain":"domain-b","item_id":"remote-new"}
            ]))
            .unwrap();
            let playlist_query = CatalogQuery::Playlist {
                provider: "library".into(),
                item_id: MediaId("42".into()),
            };
            let tracks_query = CatalogQuery::PlaylistTracks {
                provider: "library".into(),
                item_id: MediaId("42".into()),
                allow_dynamic_tracks: false,
            };
            let neighbor_query = CatalogQuery::PlaylistTracks {
                provider: "library".into(),
                item_id: MediaId("neighbor".into()),
                allow_dynamic_tracks: false,
            };
            let aliases: Vec<_> = [
                ("library", "42"),
                ("source-a", "remote"),
                ("domain-a", "remote"),
                ("source-b", "remote-new"),
                ("domain-b", "remote-new"),
            ]
            .into_iter()
            .map(|(provider, item_id)| CatalogQuery::PlaylistTracks {
                provider: provider.into(),
                item_id: MediaId(item_id.into()),
                allow_dynamic_tracks: true,
            })
            .collect();
            let old_track =
                json!({"provider":"library","item_id":"old","name":"Old track","position":0});
            let old_tracks =
                CatalogValue::Tracks(vec![serde_json::from_value(old_track.clone()).unwrap(); 2]);
            let response = if should_fail_readback {
                json!({"invalid":"not a track list"})
            } else if is_empty {
                json!([])
            } else {
                json!([
                    {"media_type":"track","provider":"library","item_id":"new","name":"New track","position":0},
                    {"media_type":"track","provider":"library","item_id":"new","name":"New track","position":1},
                    {"media_type":"track","provider":"library","item_id":"old","name":"Old track","position":2}
                ])
            };
            let (client, worker) = server(vec![("music/playlists/playlist_tracks", response)]);
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let seed_playlist = playlist_query.clone();
            let seed_tracks = tracks_query.clone();
            let seed_neighbor = neighbor_query.clone();
            let seed_aliases = aliases.clone();
            cache.database().await.unwrap().write_transaction(move |transaction| {
                let user = UserId("user".into());
                write_cached_read(transaction, &seed_playlist, &user, &CatalogValue::Playlist(original), 1)?;
                write_cached_read(transaction, &seed_tracks, &user, &old_tracks, 1)?;
                write_cached_read(transaction, &seed_neighbor, &user, &old_tracks, 1)?;
                for alias in seed_aliases { write_cached_read(transaction, &alias, &user, &old_tracks, 1)?; }
                if should_fail_commit {
                    transaction.execute_batch("CREATE TEMP TRIGGER fail_content BEFORE INSERT ON track_list_states BEGIN SELECT RAISE(ABORT,'synthetic content failure'); END;")?;
                }
                Ok(())
            }).await.unwrap();
            let result = catalog
                .read_confirmed_playlist_contents(confirmed.clone())
                .await;
            if should_fail_readback {
                assert!(result.is_err());
            } else {
                let tracks = result.unwrap();
                assert_eq!(tracks.len(), if is_empty { 0 } else { 3 });
                if !is_empty {
                    assert_eq!(tracks[0].media.item_id.as_str(), "new");
                    assert_eq!(tracks[1].media.item_id.as_str(), "new");
                    assert_eq!(tracks[1].media.position, Some(1));
                }
                assert_eq!(
                    catalog
                        .commit_confirmed_playlist_contents(confirmed, tracks)
                        .await
                        .is_err(),
                    should_fail_commit
                );
            }
            assert_eq!(worker.join().unwrap().len(), 1);
            drop(catalog);
            cache.shutdown().await.unwrap();
            drop(cache);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    let user = UserId("user".into());
                    let Some(CatalogValue::Playlist(playlist)) =
                        read_partial_cached(snapshot, &playlist_query, &user)?
                    else {
                        panic!("playlist metadata missing")
                    };
                    let kept_old = should_fail_commit || should_fail_readback;
                    assert_eq!(
                        playlist.media.name,
                        if kept_old {
                            "Old playlist"
                        } else {
                            "Confirmed playlist"
                        }
                    );
                    let CatalogValue::Tracks(tracks) =
                        read_cached_read(snapshot, &tracks_query, &user)?
                            .unwrap()
                            .value
                    else {
                        panic!("playlist tracks missing")
                    };
                    assert_eq!(
                        tracks.len(),
                        if kept_old {
                            2
                        } else if is_empty {
                            0
                        } else {
                            3
                        }
                    );
                    if !tracks.is_empty() {
                        assert_eq!(
                            tracks[0].media.item_id.as_str(),
                            if kept_old { "old" } else { "new" }
                        );
                    }
                    let CatalogValue::Tracks(neighbor) =
                        read_cached_read(snapshot, &neighbor_query, &user)?
                            .unwrap()
                            .value
                    else {
                        panic!("neighbor missing")
                    };
                    assert_eq!(neighbor.len(), 2);
                    assert_eq!(neighbor[0].media.item_id.as_str(), "old");
                    for alias in aliases {
                        assert_eq!(
                            read_cached_read(snapshot, &alias, &user)?.is_some(),
                            kept_old
                        );
                    }
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
            drop(reopened);
            directory.close().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn database_clear_retires_old_context_and_reopens_session_without_touching_images() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_owned();
        let owner = Arc::new(SessionCacheOwner::new(move || Ok(path.clone())));
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        let catalog = owner.session(client.clone(), Some(UserId("user".into())), 1);
        let cache = owner.cache_for_generation(1).await.unwrap();
        cache.activate("server".into()).await.unwrap();
        cache
            .bind_server_address("server".into(), client.base_url().into())
            .await
            .unwrap();
        let previous = catalog.inner.cache_context().await.ok().unwrap();
        let images = cache.images().await.unwrap();
        let revision = catalog
            .inner
            .confirmed_revision
            .load(std::sync::atomic::Ordering::Acquire);
        catalog.clear_database_cache().await.unwrap();
        assert!(previous.database.require_active().is_err());
        assert!(images.require_active().is_ok());
        let current = catalog.inner.cache_context().await.ok().unwrap();
        assert!(current.database.require_active().is_ok());
        assert!(!Arc::ptr_eq(&previous, &current));
        assert!(
            catalog
                .inner
                .confirmed_revision
                .load(std::sync::atomic::Ordering::Acquire)
                > revision
        );
        owner.shutdown(1).await.unwrap();
        assert!(catalog.clear_database_cache().await.is_err());
        drop((previous, current, catalog, owner, cache, images));
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inline_playlist_refresh_reuses_the_persistent_reference_projection() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let response = json!({"provider":"library","item_id":"42","uri":"library://playlist/42","name":"Private","metadata":{"images":[{"type":"thumb","provider":"builtin","path":format!("data:image/png;base64,{}",base64::Engine::encode(&base64::engine::general_purpose::STANDARD,super::super::image_files::tests::bitmap(6)))}]}});
        let (client, worker) = server(vec![
            ("music/playlists/get", response.clone()),
            ("music/playlists/get", response),
        ]);
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::from_secs(300),
        )
        .await
        .unwrap();
        let query = CatalogQuery::Playlist {
            provider: "library".into(),
            item_id: MediaId("42".into()),
        };
        for _ in 0..2 {
            let mut updates = catalog.observe_catalog(query.clone(), true);
            let completed = completed_snapshot(&mut updates).await;
            assert_eq!(completed.phase, CatalogLoadPhase::Ready);
            assert!(
                completed.cache_error.is_none(),
                "{:?}",
                completed.cache_error
            );
            cache
                .database()
                .await
                .unwrap()
                .read_snapshot(|snapshot| {
                    let metadata: String = snapshot.query_row(
                        "SELECT metadata FROM playlists WHERE provider='library' AND item_id='42'",
                        [],
                        |row| row.get(0),
                    )?;
                    assert!(metadata.contains("cached_inline"));
                    assert!(!metadata.contains("data:image"));
                    Ok(())
                })
                .await
                .unwrap();
        }
        assert_eq!(worker.join().unwrap().len(), 2);
        drop(catalog);
        cache.shutdown().await.unwrap();
        drop(cache);
        directory.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retired_owner_rejects_recreated_catalogs_without_initializing_storage() {
        let owner = Arc::new(SessionCacheOwner::new(|| {
            panic!("retired catalog opened storage")
        }));
        let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
        owner.shutdown(1).await.unwrap();
        let catalog = owner.session(client.clone(), None, 1);
        assert!(catalog.inner.require_active().is_err());
        assert!(catalog
            .read_confirmed_playlist_contents(
                serde_json::from_value(json!({
                    "provider":"library","item_id":"42","name":"Retired"
                }))
                .unwrap()
            )
            .await
            .is_err());
        if let CacheAccess::Session(session) = &catalog.inner.access {
            assert!(session.resolve().await.is_err());
        } else {
            panic!("expected a session-bound catalog");
        }
        let current = owner.session(client, None, 2);
        assert!(current.inner.require_active().is_ok());
        owner.shutdown(2).await.unwrap();
        assert!(current.inner.require_active().is_err());
    }

    async fn completed_snapshot(
        updates: &mut BoxStream<'_, CatalogLoadSnapshot>,
    ) -> CatalogLoadSnapshot {
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(snapshot) = updates.next().await {
                if matches!(
                    snapshot.phase,
                    CatalogLoadPhase::Ready
                        | CatalogLoadPhase::Failed
                        | CatalogLoadPhase::Invalidated
                ) {
                    return snapshot;
                }
            }
            panic!("catalog stream ended without completion")
        })
        .await
        .expect("catalog observation stalled")
    }

    // Isolated server identity and canonical cache fixtures.
    fn query() -> CatalogQuery {
        CatalogQuery::Artist {
            provider: "library".to_owned(),
            item_id: MediaId("007".to_owned()),
        }
    }
    fn value(name: &str) -> CatalogValue {
        CatalogValue::Artist(
            serde_json::from_value(json!({"provider":"library","item_id":"007","name":name}))
                .unwrap(),
        )
    }
    async fn cache(directory: &std::path::Path) -> ServerCache {
        let cache = ServerCache::initialize(directory.to_owned()).await.unwrap();
        cache.activate("server".to_owned()).await.unwrap();
        let previous = value("Old");
        cache
            .database()
            .await
            .unwrap()
            .write_transaction(move |transaction| {
                write_artist_read(
                    transaction,
                    &query(),
                    &UserId("user".to_owned()),
                    &previous,
                    100,
                )
            })
            .await
            .unwrap();
        cache
    }

    // Local-first delivery, shared refresh, commit and error retention.
    #[tokio::test(flavor = "current_thread")]
    async fn confirmed_inline_playlist_cover_persists_without_sql_payload_and_preserves_tracks_after_reopen(
    ) {
        use super::super::image_sources::{
            ImageOutputFormat, ImageSize, ImageSourceKey, ImageVariantKey, ImageVariantParameters,
        };
        use crate::domain::media::{MediaItem, Playlist, Track};
        for (proxy_id, should_fail_sql) in [
            (None, false),
            (Some("a".repeat(64)), false),
            (Some("b".repeat(64)), true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let playlist_query = CatalogQuery::Playlist {
                provider: "library".into(),
                item_id: MediaId("42".into()),
            };
            let tracks_query = CatalogQuery::PlaylistTracks {
                provider: "library".into(),
                item_id: MediaId("42".into()),
                allow_dynamic_tracks: false,
            };
            let track: Track = serde_json::from_value(
                json!({"provider":"library","item_id":"track","name":"Repeated"}),
            )
            .unwrap();
            let tracks = CatalogValue::Tracks(vec![track.clone(), track]);
            let old: Playlist = serde_json::from_value(json!({"provider":"library","item_id":"42","uri":"library://playlist/42","name":"Private","metadata":{"description":"Retained","images":[{"type":"thumb","provider":"builtin","path":"old.jpg"}]}})).unwrap();
            let seed_playlist = playlist_query.clone();
            let seed_tracks = tracks_query.clone();
            let seed_value = tracks.clone();
            cache.database().await.unwrap().write_transaction(move |transaction| {
                let user = UserId("user".into());
                write_cached_read(transaction, &seed_playlist, &user, &CatalogValue::Playlist(old), 100)?;
                write_cached_read(transaction, &seed_tracks, &user, &seed_value, 100)?;
                if should_fail_sql { transaction.execute_batch("CREATE TEMP TRIGGER fail_inline_cover BEFORE INSERT ON playlists BEGIN SELECT RAISE(ABORT,'synthetic inline cover commit failure'); END;")?; }
                Ok(())
            }).await.unwrap();
            let client = Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None));
            let catalog = CachedCatalog::new(
                client,
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(300),
            )
            .await
            .unwrap();
            let bytes = super::super::image_files::tests::bitmap(4);
            let confirmed: Playlist = serde_json::from_value(json!({"provider":"library","item_id":"42","uri":"library://playlist/42","name":"Private","metadata":{"images":[{"type":"thumb","provider":"builtin","path":format!("data:image/png;base64,{}",base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)),"proxy_id":proxy_id}]}})).unwrap();
            let result = catalog
                .commit_confirmed_media(MediaItem::Playlist(confirmed))
                .await;
            assert_eq!(result.is_err(), should_fail_sql);
            drop(catalog);
            cache.shutdown().await.unwrap();
            drop(cache);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            let image = reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    let user = UserId("user".into());
                    let metadata: String = snapshot.query_row(
                        "SELECT metadata FROM playlists WHERE provider='library' AND item_id='42'",
                        [],
                        |row| row.get(0),
                    )?;
                    assert!(!metadata.contains("base64"));
                    assert!(!metadata.contains("data:image"));
                    let CatalogValue::Playlist(playlist) =
                        read_partial_cached(snapshot, &playlist_query, &user)?.unwrap()
                    else {
                        panic!("playlist missing after reopen")
                    };
                    let metadata = playlist.media.metadata.unwrap();
                    assert_eq!(metadata.description.as_deref(), Some("Retained"));
                    assert_eq!(
                        read_cached_read(snapshot, &tracks_query, &user)?
                            .unwrap()
                            .value,
                        tracks
                    );
                    let image = metadata.images.into_iter().next().unwrap();
                    assert_eq!(image.cached_inline.is_some(), !should_fail_sql);
                    assert_eq!(image.path, if should_fail_sql { "old.jpg" } else { "" });
                    Ok(image)
                })
                .await
                .unwrap();
            if !should_fail_sql {
                assert_eq!(image.proxy_id, proxy_id);
                let reference = image.cached_inline.as_ref().unwrap();
                let source = ImageSourceKey::from_inline(
                    &image,
                    reference.owner_uri.as_ref().unwrap().as_str(),
                    reference.position,
                )
                .unwrap();
                reopened.activate_images("server".into()).await.unwrap();
                let key = ImageVariantKey {
                    source,
                    parameters: ImageVariantParameters {
                        size: ImageSize::Original,
                        output_format: ImageOutputFormat::Automatic,
                    },
                };
                assert_eq!(
                    reopened
                        .images()
                        .await
                        .unwrap()
                        .read_variant(key)
                        .await
                        .unwrap()
                        .unwrap()
                        .read
                        .payload
                        .bytes,
                    bytes
                );
            }
            reopened.shutdown().await.unwrap();
            drop(reopened);
            directory.close().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn track_single_reads_share_refresh_retain_failure_and_reopen() {
        use crate::domain::media::{FieldKnowledge, MediaItem, MediaType, Track};
        for read_query in [
            CatalogQuery::Track {
                provider: "source-a".into(),
                item_id: MediaId("remote".into()),
            },
            CatalogQuery::ProviderItem {
                media_type: MediaType::Track,
                provider: "source-a".into(),
                item_id: MediaId("remote".into()),
            },
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let old_json = json!({
                "media_type":"track","provider":"library","item_id":"007","name":"Old track","favorite":true,
                "duration":240,"artists":[],"metadata":{"description":"Retained"},
                "album":{"provider":"library","item_id":"album-a","name":"A"},"disc_number":1,"track_number":4,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote","in_library":false}]
            });
            let mut new_json = old_json.clone();
            new_json["name"] = json!("Server track");
            new_json["favorite"] = json!(null);
            new_json["album"] = json!({"provider":"library","item_id":"album-b","name":"B"});
            new_json["track_number"] = json!(9);
            let value_for = |raw| {
                let track: Track = serde_json::from_value(raw).unwrap();
                if matches!(&read_query, CatalogQuery::ProviderItem { .. }) {
                    CatalogValue::Item(MediaItem::Track(track))
                } else {
                    CatalogValue::Track(track)
                }
            };
            let old_value = value_for(old_json);
            let new_value = value_for(new_json.clone());
            let seed_query = read_query.clone();
            let seed_value = old_value.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_cached_read(
                        transaction,
                        &seed_query,
                        &UserId("user".into()),
                        &seed_value,
                        chrono::Utc::now().timestamp(),
                    )
                })
                .await
                .unwrap();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let signal = entered.clone();
            let barrier = release.clone();
            let request_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counter = request_count.clone();
            let is_provider_item = matches!(&read_query, CatalogQuery::ProviderItem { .. });
            let command = if is_provider_item {
                "music/item"
            } else {
                "music/tracks/get"
            };
            let (client, worker) = server_with_request_handler(
                vec![
                    (command, new_json),
                    (command, json!({"test_http_status":503})),
                ],
                move |request| {
                    assert_eq!(request["args"]["item_id"], "remote");
                    assert_eq!(
                        request["args"]["provider_instance_id_or_domain"],
                        "source-a"
                    );
                    if is_provider_item {
                        assert_eq!(request["args"]["media_type"], "track");
                        assert_eq!(request["args"]["allow_update_metadata"], false);
                    }
                    if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        signal.notify_one();
                        barrier.wait();
                    }
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(3600),
            )
            .await
            .unwrap();
            let mut first = catalog.observe_catalog(read_query.clone(), false);
            let initial = first.next().await.unwrap();
            assert_eq!(initial.value, Some(Arc::new(old_value.clone())));
            assert_eq!(initial.knowledge, CatalogReadKnowledge::ObservedResponse);
            assert_eq!(initial.freshness, CatalogFreshness::Fresh);
            entered.notified().await;
            let mut second = catalog.observe_catalog(read_query.clone(), false);
            assert_eq!(second.next().await.unwrap().value, Some(Arc::new(old_value)));
            release.wait();
            for updates in [&mut first, &mut second] {
                let completed = completed_snapshot(updates).await;
                assert_eq!(completed.phase, CatalogLoadPhase::Ready);
                assert_eq!(
                    completed.source,
                    Some(crate::domain::catalog::CatalogDataSource::Server)
                );
                assert_eq!(completed.value, Some(Arc::new(new_value.clone())));
                assert!(completed.cache_error.is_none());
            }
            assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);
            let mut forced = catalog.observe_catalog(read_query.clone(), true);
            assert_eq!(forced.next().await.unwrap().value, Some(Arc::new(new_value.clone())));
            let failed = completed_snapshot(&mut forced).await;
            assert_eq!(failed.phase, CatalogLoadPhase::Failed);
            assert_eq!(failed.value, Some(Arc::new(new_value.clone())));
            assert!(failed.error.is_some());
            drop(first);
            drop(second);
            drop(forced);
            drop(catalog);
            cache.shutdown().await.unwrap();
            assert_eq!(worker.join().unwrap().len(), 2);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    let exact =
                        read_cached_read(snapshot, &read_query, &UserId("user".into()))?.unwrap();
                    assert_eq!(exact.value, new_value);
                    assert!(
                        read_cached_read(snapshot, &read_query, &UserId("other".into()))?.is_none()
                    );
                    let partial =
                        read_partial_cached(snapshot, &read_query, &UserId("other".into()))?
                            .unwrap();
                    let track = match partial {
                        CatalogValue::Track(track)
                        | CatalogValue::Item(MediaItem::Track(track)) => track,
                        _ => panic!("track missing"),
                    };
                    assert_eq!(track.media.favorite, FieldKnowledge::Unknown);
                    assert_eq!(track.album.unwrap().media.item_id.as_str(), "album-b");
                    assert_eq!(track.track_number, 9);
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn album_cache_first_shares_ma_refresh_retains_failure_and_reopens_all_query_kinds() {
        use super::super::album_reads::{read_album_read, write_album_read};
        use crate::domain::media::{Album, MediaItem, MediaType, MusicAssistantSearchResults};
        let queries = vec![
            CatalogQuery::LibraryAlbums {
                favorite: Some(false),
                limit: 25,
                offset: 0,
                summary: false,
                filters: Default::default(),
            },
            CatalogQuery::LibraryAlbums {
                favorite: Some(false),
                limit: 25,
                offset: 0,
                summary: true,
                filters: Default::default(),
            },
            CatalogQuery::LibraryAlbums {
                favorite: Some(false),
                limit: 25,
                offset: 0,
                summary: true,
                filters: serde_json::from_value(json!({"search":"Artist - Album%","provider":["source-a","source-b"],"genre":[7,9],"played_only":true,"album_types":["album","ep"],"sort_field":"year","sort_direction":"desc","reachable_via":["source-b"],"random_seed":42})).unwrap(),
            },
            CatalogQuery::LibraryAlbums {
                favorite: None, limit: 25, offset: 0, summary: true,
                filters: serde_json::from_value(json!({"sort_field":"random","random_seed":42})).unwrap(),
            },
            CatalogQuery::Album {
                provider: "source-a".into(),
                item_id: MediaId("007".into()),
            },
            CatalogQuery::AlbumTracks {
                provider: "source-a".into(),
                item_id: MediaId("007".into()),
            },
            CatalogQuery::ProviderItem {
                provider: "source-a".into(),
                item_id: MediaId("007".into()),
                media_type: MediaType::Album,
            },
            CatalogQuery::Search {
                query: "needle".into(),
                media_type: MediaType::Album,
                limit: 25,
            },
        ];
        for read_query in queries {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let previous: Album = serde_json::from_value(json!({"provider":"library","item_id":"44","name":"Cached","year":2020,"favorite":false,
                "metadata":{"description":"Preserved details"},"provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"007","in_library":false}]})).unwrap();
            let project = |mut album: Album| {
                if matches!(
                    read_query,
                    CatalogQuery::LibraryAlbums { summary: true, .. }
                ) {
                    if let Some(metadata) = album.media.metadata.as_mut() {
                        metadata.description = None;
                        metadata.knowledge.description = false;
                    }
                }
                match &read_query {
                    CatalogQuery::LibraryAlbums { .. } => CatalogValue::Albums(vec![album]),
                    CatalogQuery::AlbumTracks { .. } => {
                        CatalogValue::Tracks(vec![serde_json::from_value(json!({
                    "provider":"source-b","item_id":"track","name":album.media.name,"duration":125,
                    "album":album,"disc_number":1,"track_number":4
                })).unwrap()])
                    }
                    CatalogQuery::ProviderItem { .. } => {
                        CatalogValue::Item(MediaItem::Album(album))
                    }
                    CatalogQuery::Search { .. } => {
                        CatalogValue::Search(MusicAssistantSearchResults {
                            albums: vec![album],
                            ..Default::default()
                        })
                    }
                    _ => CatalogValue::Album(album),
                }
            };
            let previous_value = project(previous.clone());
            let mut refreshed = previous.clone();
            refreshed.media.name = "Refreshed".into();
            let refreshed_value = project(refreshed);
            let stored_query = read_query.clone();
            let stored_value = previous_value.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    super::super::albums::write_album_observation(
                        transaction,
                        &previous,
                        &UserId("user".into()),
                    )?;
                    if matches!(
                        stored_query,
                        CatalogQuery::Search { .. }
                            | CatalogQuery::AlbumTracks { .. }
                            | CatalogQuery::LibraryAlbums { .. }
                    ) {
                        write_album_read(
                            transaction,
                            &stored_query,
                            &UserId("user".into()),
                            &stored_value,
                            100,
                        )?;
                    }
                    Ok(())
                })
                .await
                .unwrap();
            let command = match &read_query {
                CatalogQuery::Album { .. } => "music/albums/get",
                CatalogQuery::AlbumTracks { .. } => "music/albums/album_tracks",
                CatalogQuery::ProviderItem { .. } => "music/item",
                CatalogQuery::LibraryAlbums { .. } => "music/albums/library_items",
                _ => "music/search",
            };
            let album_response = json!({"media_type":"album","provider":"library","item_id":"44","name":"Refreshed"});
            let response = if matches!(read_query, CatalogQuery::Search { .. }) {
                json!({"albums":[album_response]})
            } else if matches!(read_query, CatalogQuery::AlbumTracks { .. }) {
                json!([{"provider":"source-b","item_id":"track","name":"Refreshed","album":album_response}])
            } else if matches!(read_query, CatalogQuery::LibraryAlbums { .. }) {
                json!([album_response])
            } else {
                album_response
            };
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let request_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let signal = entered.clone();
            let barrier = release.clone();
            let counter = request_count.clone();
            let request_query = read_query.clone();
            let (client, worker) = server_with_request_handler(
                vec![(command, response), (command, json!(null))],
                move |request| {
                    match &request_query {
                        CatalogQuery::LibraryAlbums {
                            favorite,
                            limit,
                            offset,
                            summary,
                            filters,
                        } => {
                            assert_eq!(request["args"]["favorite"], json!(favorite));
                            assert_eq!(request["args"]["limit"], json!(limit));
                            assert_eq!(request["args"]["offset"], json!(offset));
                            assert_eq!(request["args"]["summary"], json!(summary));
                            let expected = serde_json::to_value(filters).unwrap();
                            for (key, value) in expected.as_object().unwrap() {
                                if key == "sort_field" && value.is_null() {
                                    assert_eq!(request["args"][key], "sort_name");
                                } else if key == "sort_direction"
                                    && value.is_null()
                                    && filters.sort_field.is_none()
                                {
                                    assert_eq!(request["args"][key], "asc");
                                } else {
                                    assert_eq!(&request["args"][key], value);
                                }
                            }
                        }
                        CatalogQuery::Search { .. } => {
                            assert_eq!(request["args"]["search_query"], "needle");
                            assert_eq!(request["args"]["media_types"], json!(["album"]));
                            assert_eq!(request["args"]["limit"], 25);
                            assert_eq!(request["args"]["library_only"], false);
                        }
                        _ => {
                            assert_eq!(request["args"]["item_id"], "007");
                            assert_eq!(
                                request["args"]["provider_instance_id_or_domain"],
                                "source-a"
                            );
                            if matches!(request_query, CatalogQuery::ProviderItem { .. }) {
                                assert_eq!(request["args"]["media_type"], "album");
                                assert_eq!(request["args"]["allow_update_metadata"], false);
                            }
                        }
                    }
                    if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        signal.notify_one();
                        barrier.wait();
                    }
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let mut first = catalog.observe_catalog(read_query.clone(), false);
            let initial = first.next().await.unwrap();
            assert_eq!(initial.value, Some(Arc::new(previous_value.clone())));
            assert_eq!(initial.phase, CatalogLoadPhase::Refreshing);
            if matches!(
                read_query,
                CatalogQuery::Search { .. }
                    | CatalogQuery::AlbumTracks { .. }
                    | CatalogQuery::LibraryAlbums { .. }
            ) {
                assert_eq!(initial.knowledge, CatalogReadKnowledge::ObservedResponse);
                assert_eq!(initial.freshness, CatalogFreshness::Stale);
            } else {
                assert_eq!(initial.knowledge, CatalogReadKnowledge::Partial);
                assert_eq!(initial.freshness, CatalogFreshness::Unknown);
                assert!(initial.observed_at.is_none());
            }
            entered.notified().await;
            let mut second = catalog.observe_catalog(read_query.clone(), false);
            assert_eq!(second.next().await.unwrap().value, Some(Arc::new(previous_value)));
            release.wait();
            for observation in [&mut first, &mut second] {
                let completed = completed_snapshot(observation).await;
                assert_eq!(completed.phase, CatalogLoadPhase::Ready);
                assert_eq!(completed.freshness, CatalogFreshness::Fresh);
                assert_eq!(completed.value, Some(Arc::new(refreshed_value.clone())));
            }
            assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);
            let mut forced = catalog.observe_catalog(read_query.clone(), true);
            assert_eq!(
                forced.next().await.unwrap().value,
                Some(Arc::new(refreshed_value.clone()))
            );
            let failure = forced.next().await.unwrap();
            assert_eq!(failure.phase, CatalogLoadPhase::Failed);
            assert_eq!(failure.value, Some(Arc::new(refreshed_value.clone())));
            assert_eq!(failure.knowledge, CatalogReadKnowledge::ObservedResponse);
            assert!(failure.error.is_some());
            drop(first);
            drop(second);
            drop(forced);
            drop(catalog);
            cache.shutdown().await.unwrap();
            worker.join().unwrap();
            assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 2);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    assert_eq!(
                        read_album_read(snapshot, &read_query, &UserId("user".into()))?
                            .unwrap()
                            .value,
                        refreshed_value
                    );
                    assert!(
                        read_album_read(snapshot, &read_query, &UserId("other".into()))?.is_none()
                    );
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_confirmed_mutation_checks_server_identity_before_cache_commit() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let (client, worker) = server(vec![("info", json!({"server_id":"different-server"}))]);
        let mut catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let CacheAccess::Active(context) = &mut Arc::get_mut(&mut catalog.inner).unwrap().access
        else {
            panic!("expected an active cache")
        };
        Arc::get_mut(context).unwrap().expected_server_id = Some("server".into());
        let artist = serde_json::from_value(
            json!({"provider":"library","item_id":"007","name":"Wrong server","favorite":true}),
        )
        .unwrap();
        assert!(catalog
            .commit_confirmed_media(crate::domain::media::MediaItem::Artist(artist))
            .await
            .unwrap_err()
            .contains("server identity changed"));
        cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                assert!(read_partial_artist(snapshot, &query(), &UserId("user".into()))?.is_none());
                Ok(())
            })
            .await
            .unwrap();
        drop(catalog);
        cache.shutdown().await.unwrap();
        worker.join().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_last_consumer_drop_cancels_started_network_and_prevents_late_commit() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let signal = entered.clone();
        let barrier = release.clone();
        let (client, worker) = server_with_request_handler(
            vec![(
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"Late"}),
            )],
            move |_| {
                signal.notify_one();
                barrier.wait();
            },
        );
        let client = Arc::new(client);
        let catalog = CachedCatalog::new(
            client.clone(),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut observation = catalog.observe_catalog(query(), false);
        observation.next().await.unwrap();
        entered.notified().await;
        assert_eq!(client.pending_read_count(), 1);
        drop(observation);
        tokio::time::timeout(Duration::from_secs(2), async {
            while client.pending_read_count() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        release.wait();
        // The canceled client may close its socket before the fixture publishes its response.
        let _ = worker.join();
        cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                assert_eq!(
                    read_artist_read(snapshot, &query(), &UserId("user".into()))?
                        .unwrap()
                        .value,
                    value("Old")
                );
                Ok(())
            })
            .await
            .unwrap();
        drop(catalog);
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn independent_catalog_refreshes_do_not_share_network_or_result_publication() {
        use crate::domain::catalog::{
            AlbumLibraryFilters, AlbumLibrarySort, ArtistLibraryFilters, ArtistLibrarySort,
        };
        let artist = CatalogQuery::LibraryArtists {
            favorite: None,
            limit: 2,
            offset: 0,
            summary: true,
            filters: ArtistLibraryFilters {
                sort_field: Some(ArtistLibrarySort::Random),
                ..Default::default()
            },
        };
        let album_queries =
            [AlbumLibrarySort::Random, AlbumLibrarySort::RandomPlayCount].map(|sort| {
                CatalogQuery::LibraryAlbums {
                    favorite: None,
                    limit: 2,
                    offset: 0,
                    summary: true,
                    filters: AlbumLibraryFilters {
                        sort_field: Some(sort),
                        ..Default::default()
                    },
                }
            });
        for (command, query) in std::iter::once(("music/artists/library_items", artist))
            .chain(
                album_queries
                    .into_iter()
                    .map(|query| ("music/albums/library_items", query)),
            )
            .chain(std::iter::once((
                "music/tracks/similar_tracks",
                CatalogQuery::SimilarTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("base".into()),
                    limit: 2,
                    allow_lookup: true,
                },
            )))
            .chain(
                [("builtin", true), ("library", true), ("source-a", true)].map(
                    |(provider, allow_dynamic_tracks)| {
                        (
                            "music/playlists/playlist_tracks",
                            CatalogQuery::PlaylistTracks {
                                provider: provider.into(),
                                item_id: MediaId("playlist".into()),
                                allow_dynamic_tracks,
                            },
                        )
                    },
                ),
            )
            .chain(std::iter::once((
                "music/recommendations/items",
                CatalogQuery::RecommendationItems {
                    provider: "recommendations".into(),
                    item_id: MediaId("random_artists".into()),
                },
            )))
        {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let signal = entered.clone();
            let barrier = release.clone();
            let mut requests = 0;
            let media_type = if command == "music/artists/library_items" {
                "artist"
            } else if command == "music/albums/library_items" {
                "album"
            } else {
                "track"
            };
            let (client, worker) = server_with_request_handler(
                vec![
                    (
                        command,
                        json!([{"media_type":media_type,"provider":"library","item_id":"a","name":"Sample A"}]),
                    ),
                    (
                        command,
                        json!([{"media_type":media_type,"provider":"library","item_id":"b","name":"Sample B"}]),
                    ),
                ],
                move |request| {
                    if command == "music/tracks/similar_tracks" {
                        assert_eq!(
                            request["args"],
                            json!({"item_id":"base","provider_instance_id_or_domain":"source-a","limit":2,"allow_lookup":true})
                        );
                    }
                    requests += 1;
                    if requests == 1 {
                        signal.notify_one();
                        barrier.wait();
                    }
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(300),
            )
            .await
            .unwrap();
            let mut first = catalog.observe_catalog(query.clone(), false);
            assert_eq!(
                first.next().await.unwrap().phase,
                CatalogLoadPhase::Refreshing
            );
            entered.notified().await;
            let mut second = catalog.observe_catalog(query, false);
            assert_eq!(
                second.next().await.unwrap().phase,
                CatalogLoadPhase::Refreshing
            );
            assert!(catalog.inner.refreshes.lock().unwrap().is_empty());
            release.wait();
            let first_result = completed_snapshot(&mut first).await;
            let second_result = completed_snapshot(&mut second).await;
            assert_eq!(first_result.phase, CatalogLoadPhase::Ready);
            assert_eq!(second_result.phase, CatalogLoadPhase::Ready);
            assert_ne!(first_result.value, second_result.value);
            drop(first);
            drop(second);
            drop(catalog);
            cache.shutdown().await.unwrap();
            worker.join().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_provider_item_uses_ma_transport_and_retains_non_library_observation_after_reopen(
    ) {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let query = CatalogQuery::ProviderItem {
            provider: "source-a".into(),
            item_id: MediaId("007".into()),
            media_type: crate::domain::media::MediaType::Artist,
        };
        let previous = CatalogValue::Item(crate::domain::media::MediaItem::Artist(
            serde_json::from_value(
                json!({"provider":"source-a","item_id":"007","name":"Cached provider"}),
            )
            .unwrap(),
        ));
        let stored_query = query.clone();
        let stored_value = previous.clone();
        cache
            .database()
            .await
            .unwrap()
            .write_transaction(move |transaction| {
                write_artist_read(
                    transaction,
                    &stored_query,
                    &UserId("user".into()),
                    &stored_value,
                    100,
                )
            })
            .await
            .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let signal = entered.clone();
        let barrier = release.clone();
        let (client, worker) = server_with_request_handler(
            vec![(
                "music/item",
                json!({"media_type":"artist","provider":"source-a","item_id":"007","name":"Refreshed provider"}),
            )],
            move |request| {
                assert_eq!(request["args"]["allow_update_metadata"], false);
                assert_eq!(
                    request["args"]["provider_instance_id_or_domain"],
                    "source-a"
                );
                signal.notify_one();
                barrier.wait();
            },
        );
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut observation = catalog.observe_catalog(query.clone(), false);
        assert_eq!(observation.next().await.unwrap().value, Some(Arc::new(previous)));
        entered.notified().await;
        release.wait();
        let completed = completed_snapshot(&mut observation).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        let confirmed = Arc::unwrap_or_clone(completed.value.unwrap());
        drop(observation);
        drop(catalog);
        cache.shutdown().await.unwrap();
        worker.join().unwrap();
        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".into()).await.unwrap();
        reopened
            .database()
            .await
            .unwrap()
            .read_snapshot(move |snapshot| {
                assert_eq!(
                    read_artist_read(snapshot, &query, &UserId("user".into()))?
                        .unwrap()
                        .value,
                    confirmed
                );
                let memberships: i64 = snapshot.query_row(
                    "SELECT COUNT(*) FROM artist_library_entries",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(memberships, 0);
                Ok(())
            })
            .await
            .unwrap();
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn track_lists_share_refresh_retain_failure_and_reject_preconfirmation_response_after_reopen(
    ) {
        use crate::domain::media::{FieldKnowledge, MediaItem};
        for (command, query) in [
            (
                "music/tracks/track_versions",
                CatalogQuery::TrackVersions {
                    provider: "source-a".into(),
                    item_id: MediaId("base".into()),
                },
            ),
            (
                "music/tracks/similar_tracks",
                CatalogQuery::SimilarTracks {
                    provider: "source-a".into(),
                    item_id: MediaId("base".into()),
                    limit: 0,
                    allow_lookup: false,
                },
            ),
            (
                "music/recently_played_items",
                CatalogQuery::RecentlyPlayed { limit: 0 },
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let is_recently_played = matches!(&query, CatalogQuery::RecentlyPlayed { .. });
            let expected_arguments = match &query {
                CatalogQuery::RecentlyPlayed { limit } => {
                    json!({"limit":limit,"media_types":["track"]})
                }
                CatalogQuery::SimilarTracks {
                    limit,
                    allow_lookup,
                    ..
                } => {
                    json!({"item_id":"base","provider_instance_id_or_domain":"source-a","limit":limit,"allow_lookup":allow_lookup})
                }
                _ => json!({"item_id":"base","provider_instance_id_or_domain":"source-a"}),
            };
            let old = CatalogValue::Tracks(serde_json::from_value(json!([{"provider":"source-a","item_id":"version","name":"Old","duration":240,"album":null,"image":null,
                "version":"Original version","sort_name":"Original sort","external_ids":[["isrc","retained-id"]],"is_playable":false}])).unwrap());
            let seed_query = query.clone();
            let seed_value = old.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_cached_read(
                        transaction,
                        &seed_query,
                        &UserId("user".into()),
                        &seed_value,
                        chrono::Utc::now().timestamp(),
                    )
                })
                .await
                .unwrap();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let signal = entered.clone();
            let barrier = release.clone();
            let requests_seen = std::sync::atomic::AtomicUsize::new(0);
            let updated_response = if is_recently_played {
                json!([
                    {"media_type":"track","provider":"source-a","item_id":"version","name":"Updated","image":null,"available":true,
                        "version":"","sort_name":"updated","uri":"source-a://track/version","external_ids":[],"is_playable":true,"year":null},
                    {"media_type":"track","provider":"source-b","item_id":"version","name":"Other","image":null,"available":false,
                        "version":"","sort_name":"other","uri":"source-b://track/version","external_ids":[],"is_playable":true,"year":null}
                ])
            } else {
                json!([
                    {"provider":"source-a","item_id":"version","name":"Updated","duration":240,"album":null,"image":null},
                    {"provider":"source-b","item_id":"version","name":"Other","album":null,"image":null}
                ])
            };
            let (client, worker) = server_with_request_handler(
                vec![
                    (command, updated_response),
                    (command, json!({"test_http_status":503})),
                    (command, json!([])),
                    (
                        command,
                        json!([{"media_type":"track","provider":"source-a","item_id":"version","name":"Before confirmation"}]),
                    ),
                ],
                move |request| {
                    assert_eq!(request["args"], expected_arguments);
                    if matches!(
                        requests_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                        0 | 3
                    ) {
                        signal.notify_one();
                        barrier.wait();
                    }
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
            let mut first = catalog.observe_catalog(query.clone(), false);
            let mut second = catalog.observe_catalog(query.clone(), false);
            assert_eq!(first.next().await.unwrap().value, Some(Arc::new(old.clone())));
            assert_eq!(second.next().await.unwrap().value, Some(Arc::new(old)));
            entered.notified().await;
            release.wait();
            let completed = completed_snapshot(&mut first).await;
            assert_eq!(completed.phase, CatalogLoadPhase::Ready);
            assert_eq!(
                completed.source,
                Some(crate::domain::catalog::CatalogDataSource::Server)
            );
            assert_eq!(completed_snapshot(&mut second).await.value, completed.value);
            let Some(CatalogValue::Tracks(tracks)) = completed.value.as_deref() else {
                panic!("versions missing")
            };
            assert_eq!(tracks.len(), 2);
            assert_eq!(tracks[1].media.provider.as_str(), "source-b");
            let mut failed = catalog.observe_catalog(query.clone(), true);
            let retained = completed_snapshot(&mut failed).await;
            assert_eq!(retained.phase, CatalogLoadPhase::Failed);
            assert_eq!(retained.value, completed.value);
            let mut empty = catalog.observe_catalog(query.clone(), true);
            let replaced = completed_snapshot(&mut empty).await;
            assert_eq!(replaced.phase, CatalogLoadPhase::Ready);
            assert_eq!(replaced.value, Some(Arc::new(CatalogValue::Tracks(Vec::new()))));
            let mut stale_first = catalog.observe_catalog(query.clone(), true);
            let mut stale_second = catalog.observe_catalog(query.clone(), true);
            assert_eq!(stale_first.next().await.unwrap().value, replaced.value);
            assert_eq!(stale_second.next().await.unwrap().value, replaced.value);
            entered.notified().await;
            let confirmed = serde_json::from_value(json!({"provider":"library","item_id":"base","name":"Confirmed base","favorite":null,
            "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"base","in_library":false}]})).unwrap();
            catalog
                .commit_confirmed_media(MediaItem::Track(confirmed))
                .await
                .unwrap();
            release.wait();
            for updates in [&mut stale_first, &mut stale_second] {
                let retired = completed_snapshot(updates).await;
                assert_eq!(retired.phase, CatalogLoadPhase::Failed);
                assert_eq!(retired.value, None);
                assert_eq!(retired.knowledge, CatalogReadKnowledge::Unknown);
                assert_eq!(retired.observed_at, None);
            }
            drop(first);
            drop(second);
            drop(failed);
            drop(empty);
            drop(stale_first);
            drop(stale_second);
            drop(catalog);
            cache.shutdown().await.unwrap();
            assert_eq!(worker.join().unwrap().len(), 4);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    let user = UserId("user".into());
                    let observed = read_cached_read(snapshot, &query, &user)?;
                    if is_recently_played {
                        assert_eq!(observed.unwrap().value, CatalogValue::Tracks(Vec::new()));
                    } else {
                        assert!(observed.is_none());
                    }
                    assert!(read_partial_cached(snapshot, &query, &user)?.is_none());
                    let Some(CatalogValue::Track(base)) = read_partial_cached(
                        snapshot,
                        &CatalogQuery::Track {
                            provider: "library".into(),
                            item_id: MediaId("base".into()),
                        },
                        &user,
                    )?
                    else {
                        panic!("base missing")
                    };
                    assert_eq!(base.media.name, "Confirmed base");
                    assert_eq!(base.media.favorite, FieldKnowledge::Known(None));
                    let Some(CatalogValue::Track(version)) = read_partial_cached(
                        snapshot,
                        &CatalogQuery::Track {
                            provider: "source-a".into(),
                            item_id: MediaId("version".into()),
                        },
                        &user,
                    )?
                    else {
                        panic!("version missing")
                    };
                    assert_eq!(version.media.name, "Updated");
                    assert_eq!(version.duration, 240);
                    assert_eq!(version.media.version, "Original version");
                    assert_eq!(version.media.sort_name.as_deref(), Some("Original sort"));
                    assert_eq!(
                        version.media.external_ids,
                        vec![vec!["isrc".to_owned(), "retained-id".to_owned()]]
                    );
                    assert!(!version.media.is_playable);
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn track_search_shares_fresh_refresh_retains_failure_and_reopens_server_empty() {
        use crate::domain::media::MediaType;
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let query = CatalogQuery::Search {
            query: "needle".into(),
            media_type: MediaType::Track,
            limit: 12,
        };
        let old = CatalogValue::Search(
            serde_json::from_value(
                json!({"tracks":[{"provider":"source-a","item_id":"007","name":"Old"}]}),
            )
            .unwrap(),
        );
        let seed_query = query.clone();
        let seed_value = old.clone();
        cache
            .database()
            .await
            .unwrap()
            .write_transaction(move |transaction| {
                write_cached_read(
                    transaction,
                    &seed_query,
                    &UserId("user".into()),
                    &seed_value,
                    chrono::Utc::now().timestamp(),
                )
            })
            .await
            .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let signal = entered.clone();
        let barrier = release.clone();
        let requests_seen = std::sync::atomic::AtomicUsize::new(0);
        let (client, worker) = server_with_request_handler(
            vec![
                (
                    "music/search",
                    json!({"tracks":[
                        {"provider":"source-a","item_id":"007","name":"Updated","album":{"provider":"source-a","item_id":"a","name":"A"},"disc_number":1,"track_number":4},
                        {"provider":"source-b","item_id":"007","name":"Other"},
                        {"provider":"source-a","item_id":"007","name":"Updated","album":{"provider":"source-a","item_id":"b","name":"B"},"disc_number":2,"track_number":8}
                    ]}),
                ),
                ("music/search", json!({"test_http_status":503})),
                ("music/search", json!({"tracks":[]})),
            ],
            move |request| {
                assert_eq!(
                    request["args"],
                    json!({"search_query":"needle","media_types":["track"],"limit":12,"library_only":false})
                );
                if requests_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    signal.notify_one();
                    barrier.wait();
                }
            },
        );
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        let mut first = catalog.observe_catalog(query.clone(), false);
        let mut second = catalog.observe_catalog(query.clone(), false);
        assert_eq!(first.next().await.unwrap().value, Some(Arc::new(old.clone())));
        assert_eq!(second.next().await.unwrap().value, Some(Arc::new(old)));
        entered.notified().await;
        release.wait();
        let completed = completed_snapshot(&mut first).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        assert_eq!(
            completed.source,
            Some(crate::domain::catalog::CatalogDataSource::Server)
        );
        assert_eq!(completed_snapshot(&mut second).await.value, completed.value);
        let Some(CatalogValue::Search(results)) = completed.value.as_deref() else {
            panic!("track search missing")
        };
        assert_eq!(results.tracks.len(), 3);
        assert_eq!(results.tracks[1].media.provider.as_str(), "source-b");
        let mut failed = catalog.observe_catalog(query.clone(), true);
        let retained = completed_snapshot(&mut failed).await;
        assert_eq!(retained.phase, CatalogLoadPhase::Failed);
        assert_eq!(retained.value, completed.value);
        let mut empty = catalog.observe_catalog(query.clone(), true);
        let replaced = completed_snapshot(&mut empty).await;
        assert_eq!(replaced.phase, CatalogLoadPhase::Ready);
        assert_eq!(
            replaced.value,
            Some(Arc::new(CatalogValue::Search(Default::default())))
        );
        drop(first);
        drop(second);
        drop(failed);
        drop(empty);
        drop(catalog);
        cache.shutdown().await.unwrap();
        assert_eq!(worker.join().unwrap().len(), 3);
        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".into()).await.unwrap();
        reopened
            .database()
            .await
            .unwrap()
            .read_snapshot(move |snapshot| {
                let observed = read_cached_read(snapshot, &query, &UserId("user".into()))?.unwrap();
                assert_eq!(observed.value, CatalogValue::Search(Default::default()));
                assert!(read_cached_read(snapshot, &query, &UserId("other".into()))?.is_none());
                assert!(read_partial_cached(snapshot, &query, &UserId("other".into()))?.is_none());
                assert_eq!(
                    snapshot.query_row(
                        "SELECT COUNT(*) FROM tracks WHERE item_id='007'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )?,
                    2
                );
                Ok(())
            })
            .await
            .unwrap();
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recommendation_cache_preserves_online_authority_commit_failure_and_reopened_empty() {
        for (query, command, old_response, new_response, unsupported_response) in [
            (
                CatalogQuery::RecommendationFolders,
                "music/recommendations",
                json!([{"provider":"recommendations","item_id":"row","name":"Old"}]),
                json!([{"provider":"recommendations","item_id":"row","name":"Updated"}]),
                json!([{"provider":"recommendations","item_id":"row","name":"Online only","items":[
                    {"media_type":"track","provider":"source","item_id":"intruder","name":"Must roll back"},
                    {"media_type":"radio","provider":"source","item_id":"radio","name":"Online Radio"}
                ]}]),
            ),
            (
                CatalogQuery::RecommendationItems {
                    provider: "recommendations".into(),
                    item_id: MediaId("row".into()),
                },
                "music/recommendations/items",
                json!([{"media_type":"track","provider":"source","item_id":"old","name":"Old"}]),
                json!([
                    {"media_type":"artist","provider":"source","item_id":"artist","name":"Artist"},
                    {"media_type":"album","provider":"source","item_id":"album","name":"Album"},
                    {"media_type":"playlist","provider":"source","item_id":"playlist","name":"Playlist"},
                    {"media_type":"track","provider":"source","item_id":"track","name":"Track","position":1},
                    {"media_type":"track","provider":"source","item_id":"track","name":"Track","position":4}
                ]),
                json!([
                    {"media_type":"track","provider":"source","item_id":"intruder","name":"Must roll back"},
                    {"media_type":"radio","provider":"source","item_id":"radio","name":"Online Radio"}
                ]),
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let decode = |response: serde_json::Value| match &query {
                CatalogQuery::RecommendationFolders => {
                    CatalogValue::RecommendationFolders(serde_json::from_value(response).unwrap())
                }
                _ => CatalogValue::RecommendationItems(serde_json::from_value(response).unwrap()),
            };
            let old = decode(old_response);
            let updated = decode(new_response.clone());
            let unsupported = decode(unsupported_response.clone());
            let empty = decode(json!([]));
            let seed_query = query.clone();
            let seed = old.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_cached_read(
                        transaction,
                        &seed_query,
                        &UserId("user".into()),
                        &seed,
                        chrono::Utc::now().timestamp(),
                    )
                })
                .await
                .unwrap();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let signal = entered.clone();
            let barrier = release.clone();
            let mut requests_seen = 0;
            let (client, worker) = server_with_request_handler(
                vec![
                    (command, new_response),
                    (command, json!({"test_http_status":503})),
                    (command, unsupported_response),
                    (command, json!([])),
                ],
                move |request| {
                    assert_eq!(
                        request["args"],
                        if command == "music/recommendations" {
                            json!({})
                        } else {
                            json!({"provider":"recommendations","item_id":"row"})
                        }
                    );
                    requests_seen += 1;
                    if requests_seen == 1 {
                        signal.notify_one();
                        barrier.wait();
                    }
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
            let mut first = catalog.observe_catalog(query.clone(), false);
            let local = first.next().await.unwrap();
            assert_eq!(local.value, Some(Arc::new(old)));
            assert_eq!(local.freshness, CatalogFreshness::Fresh);
            entered.notified().await;
            release.wait();
            let ready = completed_snapshot(&mut first).await;
            assert_eq!(ready.phase, CatalogLoadPhase::Ready);
            assert_eq!(ready.value, Some(Arc::new(updated.clone())));
            assert_eq!(
                ready.source,
                Some(crate::domain::catalog::CatalogDataSource::Server)
            );
            let mut failed = catalog.observe_catalog(query.clone(), true);
            let retained = completed_snapshot(&mut failed).await;
            assert_eq!(retained.phase, CatalogLoadPhase::Failed);
            assert_eq!(retained.value, Some(Arc::new(updated.clone())));
            let mut online_only = catalog.observe_catalog(query.clone(), true);
            let uncommitted = completed_snapshot(&mut online_only).await;
            assert_eq!(uncommitted.phase, CatalogLoadPhase::Ready);
            assert_eq!(uncommitted.value, Some(Arc::new(unsupported)));
            assert!(uncommitted.cache_error.is_some());
            let retained_query = query.clone();
            cache
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    assert_eq!(
                        read_cached_read(snapshot, &retained_query, &UserId("user".into()))?
                            .unwrap()
                            .value,
                        updated
                    );
                    assert_eq!(
                        snapshot.query_row(
                            "SELECT COUNT(*) FROM tracks WHERE item_id='intruder'",
                            [],
                            |row| row.get::<_, i64>(0)
                        )?,
                        0
                    );
                    Ok(())
                })
                .await
                .unwrap();
            let mut replacement = catalog.observe_catalog(query.clone(), true);
            let replaced = completed_snapshot(&mut replacement).await;
            assert_eq!(replaced.phase, CatalogLoadPhase::Ready);
            assert_eq!(replaced.value, Some(Arc::new(empty.clone())));
            drop(first);
            drop(failed);
            drop(online_only);
            drop(replacement);
            drop(catalog);
            cache.shutdown().await.unwrap();
            assert_eq!(worker.join().unwrap().len(), 4);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    assert_eq!(
                        read_cached_read(snapshot, &query, &UserId("user".into()))?
                            .unwrap()
                            .value,
                        empty
                    );
                    assert!(read_cached_read(snapshot, &query, &UserId("other".into()))?.is_none());
                    assert!(
                        read_partial_cached(snapshot, &query, &UserId("other".into()))?.is_none()
                    );
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_search_uses_ma_and_reopens_canonical_occurrences_without_membership() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let search_query = CatalogQuery::Search {
            query: "needle".into(),
            media_type: crate::domain::media::MediaType::Artist,
            limit: 25,
        };
        let (client, worker) = server_with_request_handler(
            vec![(
                "music/search",
                json!({"artists":[
                    {"provider":"source-a","item_id":"007","name":"Source A"},
                    {"provider":"source-b","item_id":"007","name":"Source B"},
                    {"provider":"source-a","item_id":"007","name":"Source A"}
                ]}),
            )],
            |request| {
                assert_eq!(request["args"]["search_query"], "needle");
                assert_eq!(request["args"]["media_types"], json!(["artist"]));
                assert_eq!(request["args"]["limit"], 25);
                assert_eq!(request["args"]["library_only"], false);
            },
        );
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut observation = catalog.observe_catalog(search_query.clone(), false);
        let initial = observation.next().await.unwrap();
        assert!(initial.value.is_none());
        assert_eq!(initial.knowledge, CatalogReadKnowledge::Unknown);
        let completed = completed_snapshot(&mut observation).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        assert_eq!(completed.knowledge, CatalogReadKnowledge::ObservedResponse);
        assert_eq!(completed.freshness, CatalogFreshness::Fresh);
        let observed_at = completed.observed_at.unwrap();
        let Some(CatalogValue::Search(results)) = completed.value.as_deref().cloned() else {
            panic!("artist search response expected");
        };
        assert_eq!(results.artists.len(), 3);
        let updated: crate::domain::media::Artist = serde_json::from_value(
            json!({"provider":"source-a","item_id":"007","name":"Updated A"}),
        )
        .unwrap();
        cache
            .database()
            .await
            .unwrap()
            .write_transaction(move |transaction| {
                write_artist_read(
                    transaction,
                    &CatalogQuery::Artist {
                        provider: "source-a".into(),
                        item_id: MediaId("007".into()),
                    },
                    &UserId("user".into()),
                    &CatalogValue::Artist(updated),
                    observed_at + 1,
                )
            })
            .await
            .unwrap();
        drop(observation);
        drop(catalog);
        cache.shutdown().await.unwrap();
        worker.join().unwrap();

        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".into()).await.unwrap();
        reopened
            .database()
            .await
            .unwrap()
            .read_snapshot(move |snapshot| {
                let observation =
                    read_artist_read(snapshot, &search_query, &UserId("user".into()))?.unwrap();
                assert_eq!(observation.observed_at, observed_at);
                let CatalogValue::Search(results) = observation.value else {
                    panic!("stored artist search expected");
                };
                let identities = results
                    .artists
                    .iter()
                    .map(|artist| {
                        (
                            artist.media.provider.as_str(),
                            artist.media.item_id.as_str(),
                            artist.media.name.as_str(),
                        )
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    identities,
                    vec![
                        ("source-a", "007", "Updated A"),
                        ("source-b", "007", "Source B"),
                        ("source-a", "007", "Updated A"),
                    ]
                );
                assert!(
                    read_artist_read(snapshot, &search_query, &UserId("other".into()))?.is_none()
                );
                let other_limit = CatalogQuery::Search {
                    query: "needle".into(),
                    media_type: crate::domain::media::MediaType::Artist,
                    limit: 24,
                };
                assert!(
                    read_artist_read(snapshot, &other_limit, &UserId("user".into()))?.is_none()
                );
                let library =
                    read_artist_read(snapshot, &query(), &UserId("user".into()))?.unwrap();
                assert_eq!(library.value, value("Old"));
                let artist_count: i64 =
                    snapshot.query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))?;
                assert_eq!(artist_count, 3);
                let memberships: i64 = snapshot.query_row(
                    "SELECT COUNT(*) FROM artist_library_entries",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(memberships, 0);
                Ok(())
            })
            .await
            .unwrap();
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_favorite_confirmation_prevents_older_refresh_commit_and_survives_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let signal = entered.clone();
        let barrier = release.clone();
        let (client, worker) = server_with_request_handler(
            vec![(
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"Before mutation","favorite":true}),
            )],
            move |_| {
                signal.notify_one();
                barrier.wait();
            },
        );
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".to_owned()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut observation = catalog.observe_catalog(query(), false);
        assert_eq!(observation.next().await.unwrap().value, Some(Arc::new(value("Old"))));
        entered.notified().await;
        let confirmed = serde_json::from_value(
            json!({"provider":"library","item_id":"007","name":"Confirmed","favorite":null}),
        )
        .unwrap();
        catalog
            .commit_confirmed_media(crate::domain::media::MediaItem::Artist(confirmed))
            .await
            .unwrap();
        release.wait();
        let retired = observation.next().await.unwrap();
        assert_eq!(retired.phase, CatalogLoadPhase::Failed);
        assert_eq!(retired.knowledge, CatalogReadKnowledge::Partial);
        assert_eq!(retired.observed_at, None);
        let Some(CatalogValue::Artist(artist)) = retired.value.as_deref().cloned() else {
            panic!("confirmed readback missing")
        };
        assert_eq!(artist.media.name, "Confirmed");
        assert_eq!(
            artist.media.favorite,
            crate::domain::media::FieldKnowledge::Known(None)
        );
        drop(observation);
        drop(catalog);
        cache.shutdown().await.unwrap();
        worker.join().unwrap();
        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".to_owned()).await.unwrap();
        reopened
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                let user = UserId("user".to_owned());
                assert!(read_artist_read(snapshot, &query(), &user)?.is_none());
                let Some(CatalogValue::Artist(artist)) =
                    read_partial_artist(snapshot, &query(), &user)?
                else {
                    panic!("confirmed artist missing")
                };
                assert_eq!(artist.media.name, "Confirmed");
                assert_eq!(
                    artist.media.favorite,
                    crate::domain::media::FieldKnowledge::Known(None)
                );
                Ok(())
            })
            .await
            .unwrap();
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn album_favorite_confirmation_prevents_older_shared_refresh_and_survives_reopen() {
        use super::super::album_reads::{read_album_read, read_partial_album, write_album_read};
        use crate::domain::media::{Album, FieldKnowledge, MediaItem};
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let album_query = CatalogQuery::Album {
            provider: "library".into(),
            item_id: MediaId("007".into()),
        };
        let old: Album = serde_json::from_value(json!({
            "provider":"library","item_id":"007","name":"Old album","favorite":true,
            "metadata":{"description":"Retained"}
        }))
        .unwrap();
        let seed_query = album_query.clone();
        let seed_value = CatalogValue::Album(old.clone());
        cache
            .database()
            .await
            .unwrap()
            .write_transaction(move |transaction| {
                write_album_read(
                    transaction,
                    &seed_query,
                    &UserId("user".into()),
                    &seed_value,
                    100,
                )
            })
            .await
            .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let signal = entered.clone();
        let barrier = release.clone();
        let (client, worker) = server_with_request_handler(
            vec![(
                "music/albums/get",
                json!({
                    "provider":"library","item_id":"007","name":"Before mutation","favorite":true
                }),
            )],
            move |_| {
                signal.notify_one();
                barrier.wait();
            },
        );
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut first = catalog.observe_catalog(album_query.clone(), false);
        let mut second = catalog.observe_catalog(album_query.clone(), false);
        assert_eq!(
            first.next().await.unwrap().value,
            Some(Arc::new(CatalogValue::Album(old.clone())))
        );
        assert_eq!(
            second.next().await.unwrap().value,
            Some(Arc::new(CatalogValue::Album(old)))
        );
        entered.notified().await;
        let confirmed: Album = serde_json::from_value(json!({
            "provider":"library","item_id":"007","name":"Confirmed album","favorite":null
        }))
        .unwrap();
        catalog
            .commit_confirmed_media(MediaItem::Album(confirmed))
            .await
            .unwrap();
        release.wait();
        for updates in [&mut first, &mut second] {
            let retired = completed_snapshot(updates).await;
            assert_eq!(retired.phase, CatalogLoadPhase::Failed);
            assert_eq!(retired.knowledge, CatalogReadKnowledge::Partial);
            assert_eq!(retired.observed_at, None);
            let Some(CatalogValue::Album(album)) = retired.value.as_deref().cloned() else {
                panic!("confirmed album readback missing")
            };
            assert_eq!(album.media.name, "Confirmed album");
            assert_eq!(album.media.favorite, FieldKnowledge::Known(None));
            assert_eq!(
                album.media.metadata.unwrap().description.as_deref(),
                Some("Retained")
            );
        }
        drop(first);
        drop(second);
        drop(catalog);
        cache.shutdown().await.unwrap();
        worker.join().unwrap();
        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".into()).await.unwrap();
        reopened
            .database()
            .await
            .unwrap()
            .read_snapshot(move |snapshot| {
                let user = UserId("user".into());
                assert!(read_album_read(snapshot, &album_query, &user)?.is_none());
                let Some(CatalogValue::Album(album)) =
                    read_partial_album(snapshot, &album_query, &user)?
                else {
                    panic!("confirmed album missing after reopen")
                };
                assert_eq!(album.media.name, "Confirmed album");
                assert_eq!(album.media.favorite, FieldKnowledge::Known(None));
                assert_eq!(
                    album.media.metadata.unwrap().description.as_deref(),
                    Some("Retained")
                );
                Ok(())
            })
            .await
            .unwrap();
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn confirmed_mutations_keep_acceptance_order_across_sessions_and_sql_failure() {
        use crate::domain::media::{MediaItem, Playlist};
        for (should_fail_older_commit, should_cancel_older_commit) in
            [(false, false), (true, false), (false, true)]
        {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().to_owned();
            let owner = Arc::new(SessionCacheOwner::new(move || Ok(path.clone())));
            let identity_requested = Arc::new(tokio::sync::Notify::new());
            let signal = identity_requested.clone();
            let expected_info_requests = if should_cancel_older_commit { 3 } else { 4 };
            let (client, server_worker) = server_with_request_handler(
                vec![("info", json!({"server_id":"server"})); expected_info_requests],
                move |_| signal.notify_one(),
            );
            let client = Arc::new(client);
            let older = Arc::new(owner.session(client.clone(), Some(UserId("user".into())), 0));
            let newer = Arc::new(owner.session(client, Some(UserId("user".into())), 0));
            let context = older
                .inner
                .cache_context()
                .await
                .map_err(|_| "cache initialization failed")
                .unwrap();
            newer
                .inner
                .cache_context()
                .await
                .map_err(|_| "cache initialization failed")
                .unwrap();
            identity_requested.notified().await;
            assert!(Arc::ptr_eq(
                &older.inner.confirmed_writes,
                &newer.inner.confirmed_writes
            ));
            let original: Playlist = serde_json::from_value(json!({
                "provider":"library","item_id":"007","name":"Before confirmation",
                "access":{"owner":"user","sharing":"private"}
            }))
            .unwrap();
            let query = CatalogQuery::Playlist {
                provider: "library".into(),
                item_id: MediaId("007".into()),
            };
            let seed_query = query.clone();
            let seed_value = CatalogValue::Playlist(original.clone());
            context.database.write_transaction(move |transaction| {
                write_cached_read(transaction, &seed_query, &UserId("user".into()), &seed_value, 100)?;
                if should_fail_older_commit {
                    transaction.execute_batch("CREATE TRIGGER fail_confirmation BEFORE INSERT ON playlists WHEN NEW.provider='library' AND NEW.item_id='007' BEGIN SELECT RAISE(ABORT,'synthetic confirmation failure'); END;")?;
                }
                Ok(())
            }).await.unwrap();
            let held_order = owner.confirmed_writes.lock().await;
            let mut confirmed = original.clone();
            confirmed.media.name = "Older favorite confirmation".into();
            let older_catalog = older.clone();
            let mut older_write = Box::pin(async move {
                older_catalog
                    .commit_confirmed_media(MediaItem::Playlist(confirmed))
                    .await
            });
            assert!(futures_util::poll!(older_write.as_mut()).is_pending());
            assert_eq!(
                older
                    .inner
                    .confirmed_revision
                    .load(std::sync::atomic::Ordering::Acquire),
                1
            );
            let newer_catalog = newer.clone();
            let mut removal = Box::pin(async move {
                newer_catalog
                    .commit_confirmed_playlist_removal(original)
                    .await
            });
            assert!(futures_util::poll!(removal.as_mut()).is_pending());
            assert_eq!(
                newer
                    .inner
                    .confirmed_revision
                    .load(std::sync::atomic::Ordering::Acquire),
                1
            );
            let independent: MediaItem = serde_json::from_value(json!({
                "media_type":"track","provider":"library","item_id":"independent","name":"Independent track"
            })).unwrap();
            let independent_catalog = older.clone();
            let mut independent_write = Box::pin(async move {
                independent_catalog
                    .commit_confirmed_media(independent)
                    .await
            });
            assert!(futures_util::poll!(independent_write.as_mut()).is_pending());
            let removal_worker = tokio::spawn(removal);
            let independent_worker = tokio::spawn(independent_write);
            let older_worker = tokio::spawn(older_write);
            assert!(tokio::time::timeout(
                Duration::from_millis(100),
                identity_requested.notified()
            )
            .await
            .is_err());
            let read_query = query.clone();
            assert!(tokio::time::timeout(
                Duration::from_secs(5),
                context.database.read_snapshot(move |snapshot| {
                    Ok(read_cached_read(snapshot, &read_query, &UserId("user".into()))?.is_some())
                })
            )
            .await
            .unwrap()
            .unwrap());
            let older_worker = if should_cancel_older_commit {
                older_worker.abort();
                assert!(older_worker.await.unwrap_err().is_cancelled());
                None
            } else {
                Some(older_worker)
            };
            drop(held_order);
            if let Some(older_worker) = older_worker {
                let older_result = older_worker.await.unwrap();
                assert_eq!(older_result.is_err(), should_fail_older_commit);
                if let Err(error) = older_result {
                    assert!(error.contains("synthetic confirmation failure"));
                }
            }
            removal_worker.await.unwrap().unwrap();
            independent_worker.await.unwrap().unwrap();
            drop(context);
            drop(older);
            drop(newer);
            owner.shutdown(1).await.unwrap();
            assert_eq!(server_worker.join().unwrap().len(), expected_info_requests);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    let user = UserId("user".into());
                    assert!(read_cached_read(snapshot, &query, &user)?.is_none());
                    assert!(read_partial_cached(snapshot, &query, &user)?.is_none());
                    let independent_query = CatalogQuery::Track {
                        provider: "library".into(),
                        item_id: MediaId("independent".into()),
                    };
                    let Some(CatalogValue::Track(track)) =
                        read_partial_cached(snapshot, &independent_query, &user)?
                    else {
                        panic!("independent confirmation missing")
                    };
                    assert_eq!(track.media.name, "Independent track");
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn playlist_removal_retires_older_shared_refresh_even_on_sql_failure_and_survives_reopen()
    {
        use crate::domain::media::{MediaItem, MediaType, Playlist};
        for (is_provider_item, should_fail_commit) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let query = if is_provider_item {
                CatalogQuery::ProviderItem {
                    media_type: MediaType::Playlist,
                    provider: "source-a".into(),
                    item_id: MediaId("remote".into()),
                }
            } else {
                CatalogQuery::Playlist {
                    provider: "library".into(),
                    item_id: MediaId("007".into()),
                }
            };
            let original: Playlist = serde_json::from_value(json!({"provider":"library","item_id":"007","name":"Old playlist","favorite":true,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote"}]})).unwrap();
            let value = if is_provider_item {
                CatalogValue::Item(MediaItem::Playlist(original.clone()))
            } else {
                CatalogValue::Playlist(original.clone())
            };
            let seed_query = query.clone();
            let seed_value = value.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_cached_read(
                        transaction,
                        &seed_query,
                        &UserId("user".into()),
                        &seed_value,
                        100,
                    )
                })
                .await
                .unwrap();
            if should_fail_commit {
                cache.database().await.unwrap().write_transaction(|transaction| {
                    transaction.execute_batch("CREATE TRIGGER fail_removal BEFORE DELETE ON playlists WHEN OLD.provider='library' AND OLD.item_id='007' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;")?;
                    Ok(())
                }).await.unwrap();
            }
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let signal = entered.clone();
            let barrier = release.clone();
            let (client, worker) = server_with_request_handler(
                vec![(
                    if is_provider_item {
                        "music/item"
                    } else {
                        "music/playlists/get"
                    },
                    json!({"media_type":"playlist","provider":"library","item_id":"007","name":"Late response","favorite":true}),
                )],
                move |_| {
                    signal.notify_one();
                    barrier.wait();
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let mut first = catalog.observe_catalog(query.clone(), false);
            let mut second = catalog.observe_catalog(query.clone(), false);
            assert_eq!(first.next().await.unwrap().value, Some(Arc::new(value.clone())));
            assert_eq!(second.next().await.unwrap().value, Some(Arc::new(value.clone())));
            entered.notified().await;
            assert_eq!(
                catalog
                    .commit_confirmed_playlist_removal(original)
                    .await
                    .is_err(),
                should_fail_commit
            );
            release.wait();
            for updates in [&mut first, &mut second] {
                let retired = completed_snapshot(updates).await;
                assert_eq!(retired.phase, CatalogLoadPhase::Failed);
                if should_fail_commit {
                    assert_eq!(retired.value, Some(Arc::new(value.clone())));
                } else {
                    assert!(retired.value.is_none());
                }
            }
            drop(first);
            drop(second);
            drop(catalog);
            cache.shutdown().await.unwrap();
            assert_eq!(worker.join().unwrap().len(), 1);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    assert_eq!(
                        read_cached_read(snapshot, &query, &UserId("user".into()))?.is_some(),
                        should_fail_commit
                    );
                    let stored = super::super::playlists::read_playlist(
                        snapshot,
                        &crate::domain::identifiers::ProviderInstanceId("library".into()),
                        &MediaId("007".into()),
                        &UserId("user".into()),
                    )?;
                    assert_eq!(stored.is_some(), should_fail_commit);
                    if let Some(stored) = stored {
                        assert_eq!(stored.media.name, "Old playlist");
                    }
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn track_and_playlist_favorite_confirmation_prevents_older_shared_refresh_and_survives_reopen(
    ) {
        use crate::domain::media::{FieldKnowledge, MediaItem, MediaType, Track};
        for (is_playlist, is_provider_item, should_fail_commit) in [
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, true, false),
            (false, false, true),
            (false, true, true),
            (true, false, true),
            (true, true, true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let read_query = if is_provider_item {
                CatalogQuery::ProviderItem {
                    media_type: if is_playlist {
                        MediaType::Playlist
                    } else {
                        MediaType::Track
                    },
                    provider: "source-a".into(),
                    item_id: MediaId("remote".into()),
                }
            } else if is_playlist {
                CatalogQuery::Playlist {
                    provider: "library".into(),
                    item_id: MediaId("007".into()),
                }
            } else {
                CatalogQuery::Track {
                    provider: "library".into(),
                    item_id: MediaId("007".into()),
                }
            };
            let old: Track = serde_json::from_value(json!({
                "provider":"library","item_id":"007","name":"Old track","favorite":true,"duration":321,
                "metadata":{"description":"Retained"},"album":{"provider":"library","item_id":"album-a","name":"A"},
                "disc_number":1,"track_number":4,
                "provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote","in_library":false}]
            })).unwrap();
            let old = if is_playlist {
                MediaItem::Playlist(serde_json::from_value(json!({"provider":"library","item_id":"007","name":"Old playlist","favorite":true,"metadata":{"description":"Retained"},"provider_mappings":[{"provider_domain":"source","provider_instance":"source-a","item_id":"remote","in_library":false}]})).unwrap())
            } else {
                MediaItem::Track(old)
            };
            let old_value = if is_provider_item {
                CatalogValue::Item(old)
            } else {
                match old {
                    MediaItem::Track(item) => CatalogValue::Track(item),
                    MediaItem::Playlist(item) => CatalogValue::Playlist(item),
                    _ => unreachable!(),
                }
            };
            let seed_value = old_value.clone();
            let seed_query = read_query.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_cached_read(
                        transaction,
                        &seed_query,
                        &UserId("user".into()),
                        &seed_value,
                        100,
                    )
                })
                .await
                .unwrap();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let signal = entered.clone();
            let barrier = release.clone();
            let command = if is_provider_item {
                "music/item"
            } else if is_playlist {
                "music/playlists/get"
            } else {
                "music/tracks/get"
            };
            let (client, worker) = server_with_request_handler(
                vec![(
                    command,
                    json!({"media_type":if is_playlist { "playlist" } else { "track" },"provider":"library","item_id":"007","name":"Before mutation","favorite":true}),
                )],
                move |_| {
                    signal.notify_one();
                    barrier.wait();
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let mut first = catalog.observe_catalog(read_query.clone(), false);
            let mut second = catalog.observe_catalog(read_query.clone(), false);
            assert_eq!(first.next().await.unwrap().value, Some(Arc::new(old_value.clone())));
            assert_eq!(second.next().await.unwrap().value, Some(Arc::new(old_value.clone())));
            entered.notified().await;
            {
                let invalid = serde_json::from_value(json!({"media_type":if is_playlist { "playlist" } else { "track" },"provider":"library","item_id":"007","name":"Invalid","favorite":null,"external_ids":[["malformed"]]})).unwrap();
                assert!(catalog.commit_confirmed_media(invalid).await.is_err());
                let verify_query = read_query.clone();
                cache
                    .database()
                    .await
                    .unwrap()
                    .read_snapshot(move |snapshot| {
                        assert_eq!(
                            read_cached_read(snapshot, &verify_query, &UserId("user".into()))?
                                .unwrap()
                                .value,
                            old_value
                        );
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
            let confirmed = serde_json::from_value(json!({"media_type":if is_playlist { "playlist" } else { "track" },"provider":"library","item_id":"007","name":"Confirmed media","favorite":null})).unwrap();
            if !should_fail_commit {
                catalog.commit_confirmed_media(confirmed).await.unwrap();
            }
            release.wait();
            for updates in [&mut first, &mut second] {
                let retired = completed_snapshot(updates).await;
                assert_eq!(retired.phase, CatalogLoadPhase::Failed);
                if !should_fail_commit {
                    assert_eq!(retired.knowledge, CatalogReadKnowledge::Partial);
                    assert_eq!(retired.observed_at, None);
                }
                let item = match Arc::unwrap_or_clone(retired.value.unwrap()) {
                    CatalogValue::Track(item) => MediaItem::Track(item),
                    CatalogValue::Playlist(item) => MediaItem::Playlist(item),
                    CatalogValue::Item(item) => item,
                    _ => panic!("confirmed media readback missing"),
                };
                let expected_name = if should_fail_commit {
                    if is_playlist {
                        "Old playlist"
                    } else {
                        "Old track"
                    }
                } else {
                    "Confirmed media"
                };
                assert_eq!(item.fields().name, expected_name);
                assert_eq!(
                    item.fields().favorite,
                    FieldKnowledge::Known(if should_fail_commit { Some(true) } else { None })
                );
                assert_eq!(
                    item.fields()
                        .metadata
                        .as_ref()
                        .unwrap()
                        .description
                        .as_deref(),
                    Some("Retained")
                );
                if let MediaItem::Track(track) = item {
                    assert_eq!(track.duration, 321);
                    assert_eq!(track.album.unwrap().media.item_id.as_str(), "album-a");
                    assert_eq!((track.disc_number, track.track_number), (1, 4));
                }
            }
            drop(first);
            drop(second);
            drop(catalog);
            cache.shutdown().await.unwrap();
            assert_eq!(worker.join().unwrap().len(), 1);
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    let user = UserId("user".into());
                    assert_eq!(
                        read_cached_read(snapshot, &read_query, &user)?.is_some(),
                        should_fail_commit
                    );
                    let item = match read_partial_cached(snapshot, &read_query, &user)?.unwrap() {
                        CatalogValue::Track(item) => MediaItem::Track(item),
                        CatalogValue::Playlist(item) => MediaItem::Playlist(item),
                        CatalogValue::Item(item) => item,
                        _ => panic!("confirmed media missing after reopen"),
                    };
                    let expected_name = if should_fail_commit {
                        if is_playlist {
                            "Old playlist"
                        } else {
                            "Old track"
                        }
                    } else {
                        "Confirmed media"
                    };
                    assert_eq!(item.fields().name, expected_name);
                    assert_eq!(
                        item.fields().favorite,
                        FieldKnowledge::Known(if should_fail_commit { Some(true) } else { None })
                    );
                    if let MediaItem::Track(track) = item {
                        assert_eq!(track.duration, 321);
                        assert_eq!(track.album.unwrap().media.item_id.as_str(), "album-a");
                        assert_eq!(track.track_number, 4);
                    }
                    Ok(())
                })
                .await
                .unwrap();
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn simultaneous_catalog_starts_share_initial_refresh_and_promote_priority() {
        use crate::application::repositories::RequestPriority;

        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let catalog = CachedCatalog::new(
            Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None)),
            cache.clone(),
            UserId("user".to_owned()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        for read_query in std::iter::once(query()).chain(
            ["builtin", "library", "source-a"].map(|provider| CatalogQuery::PlaylistTracks {
                provider: provider.into(),
                item_id: MediaId("playlist".into()),
                allow_dynamic_tracks: false,
            }),
        ) {
            let first = with_request_priority(RequestPriority::Background, async {
                catalog.inner.refresh(read_query.clone(), false)
            })
            .await;
            assert_eq!(first.changes.borrow().phase, CatalogLoadPhase::ReadingLocal);
            let second = catalog.inner.refresh(read_query.clone(), false);
            assert!(Arc::ptr_eq(&first, &second));
            assert_eq!(first.group.effective_priority(), RequestPriority::Foreground);
            let forced = catalog.inner.refresh(read_query, true);
            assert!(!Arc::ptr_eq(&first, &forced));
            drop(second);
            assert_eq!(first.changes.borrow().phase, CatalogLoadPhase::ReadingLocal);
            drop((first, forced));
        }
        let dynamic_query = CatalogQuery::PlaylistTracks {
            provider: "library".into(),
            item_id: MediaId("playlist".into()),
            allow_dynamic_tracks: true,
        };
        let first = catalog.inner.refresh(dynamic_query.clone(), false);
        let second = catalog.inner.refresh(dynamic_query, false);
        assert!(!Arc::ptr_eq(&first, &second));
        drop((first, second));
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_publishes_local_before_delayed_shared_remote_commit() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker_entered = entered.clone();
        let worker_release = release.clone();
        let (client, worker) = server_with_request_handler(
            vec![(
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"Fresh"}),
            )],
            move |_| {
                worker_entered.notify_one();
                worker_release.wait();
            },
        );
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".to_owned()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut first = catalog.observe_catalog(query(), false);
        let first_local = with_request_priority(
            crate::application::repositories::RequestPriority::Background,
            first.next(),
        )
        .await
        .unwrap();
        assert_eq!(first_local.value, Some(Arc::new(value("Old"))));
        assert_eq!(first_local.phase, CatalogLoadPhase::Refreshing);
        let mut second = catalog.observe_catalog(query(), false);
        assert_eq!(
            with_request_priority(
                crate::application::repositories::RequestPriority::Foreground,
                second.next()
            )
            .await
            .unwrap()
            .value,
            Some(Arc::new(value("Old")))
        );
        entered.notified().await;
        let refreshes = catalog.inner.refreshes.lock().unwrap();
        assert_eq!(refreshes.len(), 1);
        assert_eq!(
            refreshes
                .values()
                .next()
                .unwrap()
                .upgrade()
                .unwrap()
                .group
                .effective_priority(),
            crate::application::repositories::RequestPriority::Foreground
        );
        drop(refreshes);
        drop(first);
        release.wait();
        let completed = completed_snapshot(&mut second).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        assert_eq!(completed.value, Some(Arc::new(value("Fresh"))));
        assert!(completed.observed_at.unwrap() > 100);
        assert!(second.next().await.is_none());
        drop(second);
        let persisted = cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                read_artist_read(snapshot, &query(), &UserId("user".to_owned()))
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.value, value("Fresh"));
        assert_eq!(worker.join().unwrap().len(), 1);
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_refresh_error_retains_local_and_force_refresh_reports_failure() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let (client, worker) = server(vec![("music/artists/get", json!({"test_http_status":503}))]);
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".to_owned()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut updates = catalog.observe_catalog(query(), true);
        assert_eq!(updates.next().await.unwrap().value, Some(Arc::new(value("Old"))));
        let failed = updates.next().await.unwrap();
        assert_eq!(failed.phase, CatalogLoadPhase::Failed);
        assert_eq!(failed.value, Some(Arc::new(value("Old"))));
        assert_eq!(failed.observed_at, Some(100));
        assert!(failed.error.is_some());
        drop(updates);
        worker.join().unwrap();
        let observed = cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                read_artist_read(snapshot, &query(), &UserId("user".to_owned()))
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.observed_at, 100);
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_drops_last_consumer_without_retaining_refresh_or_starting_network()
    {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let catalog = CachedCatalog::new(
            Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None)),
            cache.clone(),
            UserId("user".to_owned()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut updates = catalog.observe_catalog(query(), false);
        updates.next().await.unwrap();
        let weak = catalog
            .inner
            .refreshes
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        let abort = weak
            .upgrade()
            .unwrap()
            .worker
            .lock()
            .unwrap()
            .clone()
            .unwrap();
        drop(updates);
        assert_eq!(weak.strong_count(), 0);
        tokio::task::yield_now().await;
        assert!(abort.is_finished());
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_old_catalog_cannot_read_or_write_new_server_database() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let catalog = CachedCatalog::new(
            Arc::new(MusicAssistantClient::new("http://127.0.0.1:1", None)),
            cache.clone(),
            UserId("user".to_owned()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        cache.activate("other-server".to_owned()).await.unwrap();
        let mut updates = catalog.observe_catalog(query(), false);
        let state = updates.next().await.unwrap();
        assert_eq!(state.phase, CatalogLoadPhase::Invalidated);
        assert!(state.value.is_none());
        assert!(state.error.is_some());
        drop(updates);
        let count: i64 = cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                snapshot
                    .query_row("SELECT COUNT(*) FROM artist_read_states", [], |row| {
                        row.get(0)
                    })
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(count, 0);
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_session_owner_reuses_server_identity_and_partitions_users() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let owner = Arc::new(SessionCacheOwner::new(move || Ok(root.clone())));
        let (client, worker) = server(vec![
            ("info", json!({"server_id":"server"})),
            (
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"First"}),
            ),
        ]);
        let first = owner.session(Arc::new(client), Some(UserId("user-a".to_owned())), 1);
        assert!(!directory.path().join("Cache").exists());
        assert_eq!(
            first.load_catalog(query(), false).await.unwrap(),
            value("First")
        );
        assert_eq!(worker.join().unwrap().len(), 2);
        let (client, worker) = server(vec![
            ("info", json!({"server_id":"server"})),
            (
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"First"}),
            ),
        ]);
        let second = owner.session(Arc::new(client), Some(UserId("user-a".to_owned())), 2);
        assert_eq!(
            second.load_catalog(query(), false).await.unwrap(),
            value("First")
        );
        assert_eq!(worker.join().unwrap().len(), 2);
        assert!(first.load_catalog(query(), false).await.is_err());
        let (client, worker) = server(vec![
            ("info", json!({"server_id":"server"})),
            (
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"User B"}),
            ),
        ]);
        let third = owner.session(Arc::new(client), Some(UserId("user-b".to_owned())), 3);
        assert_eq!(
            third.load_catalog(query(), false).await.unwrap(),
            value("User B")
        );
        assert_eq!(worker.join().unwrap().len(), 2);
        let cached = owner.cache.lock().await.as_ref().unwrap().clone();
        let count: i64 = cached
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| {
                snapshot
                    .query_row("SELECT COUNT(*) FROM artist_read_states", [], |row| {
                        row.get(0)
                    })
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(count, 2);
        owner.shutdown(4).await.unwrap();
        assert!(third.load_catalog(query(), false).await.is_err());
        let (client, worker) = server(vec![
            ("info", json!({"server_id":"other-server"})),
            (
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"Other Server"}),
            ),
        ]);
        let fourth = owner.session(Arc::new(client), Some(UserId("user-a".to_owned())), 5);
        assert_eq!(
            fourth.load_catalog(query(), false).await.unwrap(),
            value("Other Server")
        );
        assert_eq!(worker.join().unwrap().len(), 2);
        owner.shutdown(6).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sidebar_full_playlist_refresh_is_shared_with_details_and_retained_offline() {
        use crate::application::repositories::{with_request_priority, RequestPriority};
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let query = CatalogQuery::PlaylistTracks {
            item_id: crate::domain::identifiers::MediaId("sidebar".into()),
            provider: "library".into(),
            allow_dynamic_tracks: false,
        };
        let response = json!((0..601).map(|position| json!({
            "media_type":"track", "item_id":format!("track-{}", position % 3),
            "provider":"library", "name":format!("Track {}", position % 3),
            "duration":180, "position":position
        })).collect::<Vec<_>>());
        let entered = Arc::new(tokio::sync::Notify::new());
        let arrived = entered.clone();
        let (release, gate) = std::sync::mpsc::channel();
        let (client, worker) = crate::data::music_assistant::client::tests::server_with_request_handler(
            vec![("music/playlists/playlist_tracks", response)], move |_| {
                arrived.notify_one();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            },
        );
        let catalog = CachedCatalog::new(Arc::new(client), cache.clone(), UserId("user".into()), Duration::ZERO).await.unwrap();
        let sidebar = with_request_priority(RequestPriority::Background, async {
            catalog.inner.refresh(query.clone(), false)
        }).await;
        tokio::time::timeout(Duration::from_secs(3), entered.notified()).await.unwrap();
        let details = with_request_priority(RequestPriority::Foreground, async {
            catalog.inner.refresh(query.clone(), false)
        }).await;
        let is_shared = Arc::ptr_eq(&sidebar, &details);
        release.send(()).unwrap();
        assert!(is_shared, "opening details duplicated the sidebar refresh");
        let mut changes = details.changes.subscribe();
        tokio::time::timeout(Duration::from_secs(5), async {
            while changes.borrow_and_update().phase != CatalogLoadPhase::Ready {
                changes.changed().await.unwrap();
            }
        }).await.unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["args"]["limit"], 0);
        let offline = CachedCatalog::new(
            Arc::new(crate::data::music_assistant::client::MusicAssistantClient::new("http://127.0.0.1:1", None)),
            cache.clone(), UserId("user".into()), Duration::ZERO,
        ).await.unwrap();
        let Ok((_, cached)) = offline.inner.read_local(&query).await else { panic!("expected offline cache read") };
        assert_eq!(cached.knowledge, CatalogReadKnowledge::ObservedResponse);
        let Some(CatalogValue::Tracks(tracks)) = cached.value.as_deref().cloned() else { panic!("expected cached tracks") };
        assert_eq!(tracks.len(), 601);
        assert_eq!(tracks.iter().map(|track| u64::from(track.duration)).sum::<u64>(), 601 * 180);
        assert_eq!(tracks[0].media.item_id, tracks[3].media.item_id);
        assert_eq!(tracks[600].media.position, Some(600));
        drop(offline);
        drop(sidebar);
        drop(details);
        drop(catalog);
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn completed_dynamic_radio_read_waits_past_cached_sample_and_rejects_refresh_failures() {
        use futures_util::FutureExt;
        for response in [
            json!([{"media_type":"track","item_id":"new","provider":"source","name":"New","duration":180}]),
            json!([]),
            json!({"test_http_status":503}),
            json!([{"media_type":"album","item_id":"wrong","provider":"source","name":"Wrong"}]),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let query = CatalogQuery::PlaylistTracks {
                item_id: crate::domain::identifiers::MediaId("source://track/seed".into()),
                provider: "radio_playlist".into(),
                allow_dynamic_tracks: true,
            };
            let (client, worker) = server(vec![(
                "music/playlists/playlist_tracks",
                json!([{"media_type":"track","item_id":"old","provider":"source","name":"Old","duration":180}]),
            )]);
            let initial = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let CatalogValue::Tracks(cached) =
                initial.load_catalog(query.clone(), true).await.unwrap()
            else {
                panic!("tracks expected")
            };
            assert_eq!(cached[0].media.item_id.as_str(), "old");
            worker.join().unwrap();
            drop(initial);
            let entered = Arc::new(tokio::sync::Notify::new());
            let arrived = entered.clone();
            let (release, gate) = std::sync::mpsc::channel();
            let (client, worker) =
                crate::data::music_assistant::client::tests::server_with_request_handler(
                    vec![("music/playlists/playlist_tracks", response.clone())],
                    move |request| {
                        if request["command"] == "music/playlists/playlist_tracks" {
                            arrived.notify_one();
                            gate.recv().unwrap();
                        }
                    },
                );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let Ok((_, local)) = catalog.inner.read_local(&query).await else {
                panic!("expected a readable radio cache")
            };
            assert_eq!(
                local.source,
                Some(crate::domain::catalog::CatalogDataSource::Cache)
            );
            assert_eq!(local.knowledge, CatalogReadKnowledge::ObservedResponse);
            assert_eq!(local.value, Some(Arc::new(CatalogValue::Tracks(cached))));
            let mut completed = Box::pin(catalog.load_catalog(query, true));
            tokio::time::timeout(Duration::from_secs(3), async {
                tokio::select! {
                    result = &mut completed => panic!("radio completed before server response: {result:?}"),
                    _ = entered.notified() => {},
                }
            }).await.unwrap();
            assert!(completed.as_mut().now_or_never().is_none());
            release.send(()).unwrap();
            let result = completed.await;
            let requests = worker.join().unwrap();
            let request = requests
                .iter()
                .find(|request| request["command"] == "music/playlists/playlist_tracks")
                .unwrap();
            assert_eq!(request["args"]["force_refresh"], true);
            assert_eq!(request["args"]["allow_dynamic_tracks"], true);
            if response.as_array().is_some_and(|items| items.is_empty()) {
                assert!(matches!(result, Ok(CatalogValue::Tracks(tracks)) if tracks.is_empty()));
            } else if response[0]["item_id"] == "new" {
                assert!(
                    matches!(result, Ok(CatalogValue::Tracks(tracks)) if tracks.len() == 1 && tracks[0].media.item_id.as_str() == "new")
                );
            } else {
                assert!(result.is_err());
            }
            drop(catalog);
            cache.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cold_session_creates_database_and_persists_server_response() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let owner = Arc::new(SessionCacheOwner::new(move || Ok(root.clone())));
        let (client, worker) = server(vec![
            (
                "auth/me",
                json!({"user_id":"user","username":"synthetic","role":"user","enabled":true}),
            ),
            ("info", json!({"server_id":"server","schema_version":42})),
            (
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"Persisted"}),
            ),
        ]);
        let catalog = owner.session(Arc::new(client), None, 1);
        let loaded =
            tokio::time::timeout(Duration::from_secs(2), catalog.load_catalog(query(), false))
                .await
                .expect("cold cache initialization stalled")
                .unwrap();
        assert_eq!(loaded, value("Persisted"));
        assert!(directory
            .path()
            .join("Cache/metadata.sqlite3")
            .is_file());
        assert_eq!(worker.join().unwrap().len(), 3);
        owner.shutdown(2).await.unwrap();
        let reopened = ServerCache::initialize(directory.path().to_owned())
            .await
            .unwrap();
        reopened.activate("server".into()).await.unwrap();
        let observed = reopened
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| read_cached_read(snapshot, &query(), &UserId("user".into())))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.value, value("Persisted"));
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn parallel_server_precedes_blocked_cache_without_late_cache_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        let root = directory.path().to_owned();
        let owner = Arc::new(SessionCacheOwner::new(move || Ok(root.clone())));
        let mut guard = owner.cache.lock().await;
        *guard = Some(cache.clone());
        let (client, worker) = server(vec![
            (
                "music/artists/get",
                json!({"provider":"library","item_id":"007","name":"Server"}),
            ),
            ("info", json!({"server_id":"server"})),
        ]);
        let catalog = owner.session(Arc::new(client), Some(UserId("user".into())), 1);
        let mut updates = catalog.observe_catalog(query(), false);
        let first = tokio::time::timeout(Duration::from_secs(2), updates.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            first.source,
            Some(crate::domain::catalog::CatalogDataSource::Server)
        );
        assert_eq!(first.value, Some(Arc::new(value("Server"))));
        assert_eq!(first.phase, CatalogLoadPhase::Refreshing);
        drop(guard);
        while let Some(snapshot) = updates.next().await {
            assert_eq!(snapshot.value, Some(Arc::new(value("Server"))));
            assert_eq!(
                snapshot.source,
                Some(crate::domain::catalog::CatalogDataSource::Server)
            );
            if snapshot.phase == CatalogLoadPhase::Ready {
                break;
            }
        }
        let observed = cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| read_cached_read(snapshot, &query(), &UserId("user".into())))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.value, value("Server"));
        assert_eq!(worker.join().unwrap().len(), 2);
        owner.shutdown(2).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn parallel_server_survives_cache_write_failure_without_partial_commit() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        cache.database().await.unwrap().write_transaction(|transaction| {
            transaction.execute_batch("CREATE TRIGGER reject_observation BEFORE UPDATE ON artist_read_states BEGIN SELECT RAISE(ABORT, 'synthetic write failure'); END;")?;
            Ok(())
        }).await.unwrap();
        let (client, worker) = server(vec![(
            "music/artists/get",
            json!({"provider":"library","item_id":"007","name":"Server"}),
        )]);
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let mut updates = catalog.observe_catalog(query(), false);
        let completed = completed_snapshot(&mut updates).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        assert_eq!(completed.value, Some(Arc::new(value("Server"))));
        assert_eq!(
            completed.source,
            Some(crate::domain::catalog::CatalogDataSource::Server)
        );
        assert!(completed.error.is_none());
        assert!(completed.cache_error.is_some());
        let persisted = cache
            .database()
            .await
            .unwrap()
            .read_snapshot(|snapshot| read_cached_read(snapshot, &query(), &UserId("user".into())))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.value, value("Old"));
        assert_eq!(persisted.observed_at, 100);
        worker.join().unwrap();
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn parallel_server_partial_fields_retain_late_cache_details_but_explicit_null_wins() {
        for has_explicit_null in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let cached = CatalogValue::Artist(
                serde_json::from_value(json!({
                    "provider":"library","item_id":"007","name":"Old",
                    "metadata":{"description":"Retained biography"}
                }))
                .unwrap(),
            );
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_artist_read(transaction, &query(), &UserId("user".into()), &cached, 100)
                })
                .await
                .unwrap();
            let root = directory.path().to_owned();
            let owner = Arc::new(SessionCacheOwner::new(move || Ok(root.clone())));
            let mut guard = owner.cache.lock().await;
            *guard = Some(cache.clone());
            let mut response = json!({"provider":"library","item_id":"007","name":"Server"});
            if has_explicit_null {
                response["metadata"] = json!({"description":null});
            }
            let (client, worker) = server(vec![
                ("music/artists/get", response),
                ("info", json!({"server_id":"server"})),
            ]);
            let catalog = owner.session(Arc::new(client), Some(UserId("user".into())), 1);
            let mut updates = catalog.observe_catalog(query(), false);
            let first = tokio::time::timeout(Duration::from_secs(2), updates.next())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                first.source,
                Some(crate::domain::catalog::CatalogDataSource::Server)
            );
            drop(guard);
            let completed = completed_snapshot(&mut updates).await;
            assert_eq!(
                completed.source,
                Some(crate::domain::catalog::CatalogDataSource::Server)
            );
            let CatalogValue::Artist(artist) = Arc::unwrap_or_clone(completed.value.unwrap()) else {
                panic!("expected artist")
            };
            assert_eq!(artist.media.name, "Server");
            assert_eq!(
                artist
                    .media
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.description.as_deref()),
                if has_explicit_null {
                    None
                } else {
                    Some("Retained biography")
                }
            );
            assert_eq!(worker.join().unwrap().len(), 2);
            owner.shutdown(2).await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn parallel_server_survives_cache_initialization_failure() {
        let owner = Arc::new(SessionCacheOwner::new(|| {
            Err("synthetic directory failure".into())
        }));
        let (client, worker) = server(vec![(
            "music/artists/get",
            json!({"provider":"library","item_id":"007","name":"Server"}),
        )]);
        let catalog = owner.session(Arc::new(client), Some(UserId("user".into())), 1);
        let mut updates = catalog.observe_catalog(query(), false);
        let completed = completed_snapshot(&mut updates).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        assert_eq!(completed.value, Some(Arc::new(value("Server"))));
        assert!(completed
            .cache_error
            .unwrap()
            .contains("synthetic directory failure"));
        assert!(completed.error.is_none());
        worker.join().unwrap();
        owner.shutdown(2).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_library_partial_cache_first_survives_failure_without_claiming_empty_coverage() {
        use crate::domain::catalog::ArtistLibraryFilters;
        for response in [json!({"test_http_status":503}), json!([])] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(|transaction| {
                    let all = CatalogQuery::LibraryArtists {
                        favorite: None,
                        limit: 500,
                        offset: 0,
                        summary: false,
                        filters: ArtistLibraryFilters::default(),
                    };
                    let artist = serde_json::from_value(
                        json!({"provider":"library","item_id":"needle","name":"Known Needle"}),
                    )
                    .unwrap();
                    write_artist_read(
                        transaction,
                        &all,
                        &UserId("user".into()),
                        &CatalogValue::Artists(vec![artist]),
                        100,
                    )
                })
                .await
                .unwrap();
            let succeeds = response.is_array();
            let entered = Arc::new(tokio::sync::Notify::new());
            let signal = entered.clone();
            let release = Arc::new(std::sync::Barrier::new(2));
            let gate = release.clone();
            let (client, worker) = server_with_request_handler(
                vec![("music/artists/library_items", response)],
                move |_| {
                    signal.notify_one();
                    gate.wait();
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(300),
            )
            .await
            .unwrap();
            let query = CatalogQuery::LibraryArtists {
                favorite: None,
                limit: 1,
                offset: 0,
                summary: true,
                filters: ArtistLibraryFilters {
                    search: Some("Needle".into()),
                    ..Default::default()
                },
            };
            let mut updates = catalog.observe_catalog(query, false);
            let local = updates.next().await.unwrap();
            assert_eq!(local.phase, CatalogLoadPhase::Refreshing);
            assert_eq!(local.knowledge, CatalogReadKnowledge::Partial);
            assert_eq!(local.freshness, CatalogFreshness::Unknown);
            assert_eq!(local.observed_at, None);
            let Some(CatalogValue::Artists(items)) = local.value.as_deref() else {
                panic!("expected cached artists")
            };
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].media.item_id.as_str(), "needle");
            entered.notified().await;
            release.wait();
            let refreshed = completed_snapshot(&mut updates).await;
            if succeeds {
                assert_eq!(refreshed.phase, CatalogLoadPhase::Ready);
                assert_eq!(refreshed.knowledge, CatalogReadKnowledge::ObservedResponse);
                assert_eq!(refreshed.value, Some(Arc::new(CatalogValue::Artists(Vec::new()))));
                assert_eq!(refreshed.freshness, CatalogFreshness::Fresh);
                assert!(refreshed.observed_at.is_some());
            } else {
                assert_eq!(refreshed.phase, CatalogLoadPhase::Failed);
                assert_eq!(refreshed.value, local.value);
                assert_eq!(refreshed.knowledge, CatalogReadKnowledge::Partial);
                assert_eq!(refreshed.freshness, CatalogFreshness::Unknown);
                assert!(refreshed.error.is_some());
            }
            drop(updates);
            assert_eq!(worker.join().unwrap().len(), 1);
            cache.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn album_library_partial_cache_first_retains_failure_and_accepts_server_empty_page() {
        use super::super::album_reads::{read_album_read, write_album_read};
        use crate::domain::catalog::{AlbumLibraryFilters, CatalogDataSource};
        for response in [json!({"test_http_status":503}), json!([])] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            cache.database().await.unwrap().write_transaction(|transaction| {
                let page=CatalogQuery::LibraryAlbums {favorite:None,limit:500,offset:0,summary:false,filters:Default::default()};
                let album=serde_json::from_value(json!({"provider":"library","item_id":"needle","name":"Known Needle","sort_name":"Known Needle",
                    "provider_mappings":[{"provider_domain":"source","provider_instance":"member","item_id":"needle","in_library":true}]})).unwrap();
                write_album_read(transaction,&page,&UserId("user".into()),&CatalogValue::Albums(vec![album]),100)
            }).await.unwrap();
            let succeeds = response.is_array();
            let entered = Arc::new(tokio::sync::Notify::new());
            let signal = entered.clone();
            let release = Arc::new(std::sync::Barrier::new(2));
            let gate = release.clone();
            let (client, worker) = server_with_request_handler(
                vec![("music/albums/library_items", response)],
                move |_| {
                    signal.notify_one();
                    gate.wait();
                },
            );
            let catalog = CachedCatalog::new(
                Arc::new(client),
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(300),
            )
            .await
            .unwrap();
            let query = CatalogQuery::LibraryAlbums {
                favorite: None,
                limit: 1,
                offset: 0,
                summary: true,
                filters: AlbumLibraryFilters {
                    search: Some("Needle".into()),
                    ..Default::default()
                },
            };
            let mut updates = catalog.observe_catalog(query.clone(), false);
            let local = updates.next().await.unwrap();
            assert_eq!(local.phase, CatalogLoadPhase::Refreshing);
            assert_eq!(local.source, Some(CatalogDataSource::Cache));
            assert_eq!(local.knowledge, CatalogReadKnowledge::Partial);
            assert_eq!(local.freshness, CatalogFreshness::Unknown);
            assert_eq!(local.observed_at, None);
            let Some(CatalogValue::Albums(items)) = local.value.as_deref() else {
                panic!("expected partial albums")
            };
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].media.item_id.as_str(), "needle");
            entered.notified().await;
            release.wait();
            let refreshed = completed_snapshot(&mut updates).await;
            if succeeds {
                assert_eq!(refreshed.phase, CatalogLoadPhase::Ready);
                assert_eq!(refreshed.source, Some(CatalogDataSource::Server));
                assert_eq!(refreshed.knowledge, CatalogReadKnowledge::ObservedResponse);
                assert_eq!(refreshed.freshness, CatalogFreshness::Fresh);
                assert_eq!(refreshed.value, Some(Arc::new(CatalogValue::Albums(vec![]))));
            } else {
                assert_eq!(refreshed.phase, CatalogLoadPhase::Failed);
                assert_eq!(refreshed.value, local.value);
                assert_eq!(refreshed.knowledge, CatalogReadKnowledge::Partial);
                assert_eq!(refreshed.freshness, CatalogFreshness::Unknown);
                assert!(refreshed.error.is_some());
            }
            let observed = cache
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    read_album_read(snapshot, &query, &UserId("user".into()))
                })
                .await
                .unwrap();
            assert_eq!(observed.is_some(), succeeds);
            drop(updates);
            drop(catalog);
            assert_eq!(worker.join().unwrap().len(), 1);
            cache.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_partial_header_survives_refresh_failure_without_freshness_claim() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(directory.path()).await;
        cache
            .database()
            .await
            .unwrap()
            .write_transaction(|transaction| {
                transaction.execute("DELETE FROM artist_read_states", [])?;
                Ok(())
            })
            .await
            .unwrap();
        let (client, worker) = server(vec![("music/artists/get", json!({"test_http_status":503}))]);
        let catalog = CachedCatalog::new(
            Arc::new(client),
            cache.clone(),
            UserId("user".into()),
            Duration::from_secs(300),
        )
        .await
        .unwrap();
        let mut updates = catalog.observe_catalog(query(), false);
        let local = updates.next().await.unwrap();
        assert_eq!(local.value, Some(Arc::new(value("Old"))));
        assert_eq!(local.knowledge, CatalogReadKnowledge::Partial);
        assert_eq!(local.observed_at, None);
        let failed = updates.next().await.unwrap();
        assert_eq!(failed.phase, CatalogLoadPhase::Failed);
        assert_eq!(failed.value, local.value);
        assert_eq!(failed.freshness, CatalogFreshness::Unknown);
        drop(updates);
        worker.join().unwrap();
        cache.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_reopen_precedes_identity_reply_and_distinguishes_offline_from_server_change(
    ) {
        for changes_server in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().to_owned();
            let owner = Arc::new(SessionCacheOwner::new(move || Ok(root.clone())));
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let worker_entered = entered.clone();
            let worker_release = release.clone();
            let mut requests = 0;
            let response = if changes_server {
                json!({"server_id":"other-server"})
            } else {
                json!({"test_http_status":503})
            };
            let (client, worker) = server_with_request_handler(
                vec![
                    ("info", json!({"server_id":"server"})),
                    (
                        "music/artists/get",
                        json!({"provider":"library","item_id":"007","name":"Old"}),
                    ),
                    (
                        "music/artists/get",
                        json!({"provider":"library","item_id":"007","name":"Old"}),
                    ),
                    ("info", response),
                ],
                move |_| {
                    requests += 1;
                    if requests == 3 {
                        worker_entered.notify_one();
                        worker_release.wait();
                    }
                },
            );
            let client = Arc::new(client);
            let first = owner.session(client.clone(), Some(UserId("user".into())), 1);
            assert_eq!(
                first.load_catalog(query(), false).await.unwrap(),
                value("Old")
            );
            owner.shutdown(2).await.unwrap();
            let second = owner.session(client, Some(UserId("user".into())), 3);
            let mut updates = second.observe_catalog(query(), false);
            let local = updates.next().await.unwrap();
            assert_eq!(local.value, Some(Arc::new(value("Old"))));
            assert_eq!(local.phase, CatalogLoadPhase::Refreshing);
            assert_eq!(local.freshness, CatalogFreshness::Stale);
            entered.notified().await;
            release.wait();
            let terminal = completed_snapshot(&mut updates).await;
            if changes_server {
                assert_eq!(terminal.phase, CatalogLoadPhase::Invalidated);
                assert!(terminal.value.is_none());
                let cache = owner.cache.lock().await.as_ref().unwrap().clone();
                assert_eq!(
                    cache.observe().borrow().server_id.as_deref(),
                    Some("other-server")
                );
            } else {
                assert_eq!(terminal.phase, CatalogLoadPhase::Ready);
                assert_eq!(terminal.value, local.value);
                assert!(terminal.cache_error.is_some());
                assert_eq!(
                    terminal.source,
                    Some(crate::domain::catalog::CatalogDataSource::Server)
                );
            }
            assert!(updates.next().await.is_none());
            drop(updates);
            owner.shutdown(4).await.unwrap();
            assert_eq!(worker.join().unwrap().len(), 4);
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn artist_cache_first_favorite_publisher_observes_cached_page_before_failed_remote_refresh(
    ) {
        use crate::application::repositories::InitialUserDataUpdate;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let owner = Arc::new(SessionCacheOwner::new(move || Ok(root.clone())));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker_entered = entered.clone();
        let worker_release = release.clone();
        let mut count = 0;
        let (client, worker) = server_with_request_handler(
            vec![
                ("music/tracks/library_items", json!([])),
                ("music/albums/library_items", json!([])),
                ("info", json!({"server_id":"server"})),
                (
                    "music/artists/library_items",
                    json!([{"provider":"library","item_id":"007","name":"Cached","favorite":true}]),
                ),
                ("music/playlists/library_items", json!([])),
                ("music/tracks/library_items", json!([])),
                ("music/albums/library_items", json!([])),
                ("info", json!({"server_id":"server"})),
                (
                    "music/artists/library_items",
                    json!({"test_http_status":503}),
                ),
                ("music/playlists/library_items", json!([])),
            ],
            move |request| {
                if request["command"] == "music/artists/library_items" {
                    count += 1;
                    if count == 2 {
                        worker_entered.notify_one();
                        worker_release.wait();
                    }
                }
            },
        );
        let client = Arc::new(client);
        let first = owner.session(client.clone(), Some(UserId("user".into())), 1);
        let mut discard = |_: InitialUserDataUpdate| Ok::<(), String>(());
        let favorites = client
            .load_favorite_pages(&first, false, &mut discard)
            .await
            .unwrap();
        assert_eq!(favorites.artists[0].media.name, "Cached");
        owner.shutdown(2).await.unwrap();
        let second = Arc::new(owner.session(client.clone(), Some(UserId("user".into())), 3));
        let visible = Arc::new(tokio::sync::Notify::new());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let published = observed.clone();
        let publisher_visible = visible.clone();
        let loading = tokio::spawn(async move {
            let mut publish = move |update| {
                if let InitialUserDataUpdate::FavoritePage(favorites) = update {
                    if !favorites.artists.is_empty() {
                        publisher_visible.notify_one();
                    }
                    published.lock().unwrap().push(favorites);
                }
                Ok::<(), String>(())
            };
            client
                .load_favorite_pages(second.as_ref(), false, &mut publish)
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), visible.notified())
            .await
            .unwrap();
        assert_eq!(
            observed.lock().unwrap().last().unwrap().artists[0]
                .media
                .name,
            "Cached"
        );
        entered.notified().await;
        assert!(!loading.is_finished());
        release.wait();
        assert!(loading.await.unwrap().is_err());
        assert_eq!(
            observed.lock().unwrap().last().unwrap().artists[0]
                .media
                .name,
            "Cached"
        );
        owner.shutdown(4).await.unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 10);
        let artist_request = requests
            .iter()
            .find(|request| request["command"] == "music/artists/library_items")
            .unwrap();
        assert_eq!(artist_request["args"]["summary"], false);
        assert_eq!(artist_request["args"]["sort_field"], "sort_name");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn private_playlist_metadata_publishes_cache_before_server_and_retains_error_or_accepts_empty(
    ) {
        use crate::application::repositories::InitialUserDataUpdate;
        for should_fail in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let query = CatalogQuery::LibraryPlaylists {
                favorite: None,
                limit: 500,
                offset: 0,
                summary: true,
            };
            let cached = CatalogValue::Playlists(serde_json::from_value(json!([
                {"provider":"library","item_id":"owned","name":"Cached","access":{"owner":"user","sharing":"private"}},
                {"provider":"library","item_id":"other","name":"Other","access":{"owner":"other","sharing":"private"}},
                {"provider":"library","item_id":"public","name":"Public","access":{"owner":"user","sharing":"everyone"}}
            ])).unwrap());
            let write_query = query.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_cached_read(
                        transaction,
                        &write_query,
                        &UserId("user".into()),
                        &cached,
                        chrono::Utc::now().timestamp(),
                    )
                })
                .await
                .unwrap();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let worker_entered = entered.clone();
            let worker_release = release.clone();
            let (client, worker) = server_with_request_handler(
                vec![(
                    "music/playlists/library_items",
                    if should_fail {
                        json!({"test_http_status":503})
                    } else {
                        json!([])
                    },
                )],
                move |request| {
                    if request["command"] == "music/playlists/library_items" {
                        worker_entered.notify_one();
                        worker_release.wait();
                    }
                },
            );
            let client = Arc::new(client);
            let catalog = CachedCatalog::new(
                client.clone(),
                cache.clone(),
                UserId("user".into()),
                Duration::from_secs(3600),
            )
            .await
            .unwrap();
            let visible = Arc::new(tokio::sync::Notify::new());
            let observed = Arc::new(Mutex::new(Vec::new()));
            let published = observed.clone();
            let publisher_visible = visible.clone();
            let loading = tokio::spawn(async move {
                let mut publisher = move |update| {
                    if let InitialUserDataUpdate::PlaylistPage(playlists) = update {
                        if !playlists.is_empty() {
                            publisher_visible.notify_one();
                        }
                        published.lock().unwrap().push(playlists);
                    }
                    Ok::<(), String>(())
                };
                client
                    .load_private_playlist_metadata(&catalog, "user", false, &mut publisher)
                    .await
            });
            tokio::time::timeout(Duration::from_secs(2), visible.notified())
                .await
                .unwrap();
            let initial = observed.lock().unwrap()[0].clone();
            assert_eq!(initial.len(), 1);
            assert_eq!(initial[0].playlist.media.name, "Cached");
            assert!(initial[0].is_loading_tracks);
            assert!(initial[0].items.is_empty());
            tokio::time::timeout(Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            assert!(!loading.is_finished());
            release.wait();
            let result = tokio::time::timeout(Duration::from_secs(10), loading)
                .await
                .unwrap()
                .unwrap();
            if should_fail {
                assert!(result.is_err());
                assert_eq!(observed.lock().unwrap().len(), 1);
                assert_eq!(observed.lock().unwrap().last().unwrap(), &initial);
            } else {
                assert!(result.unwrap().is_empty());
                assert_eq!(observed.lock().unwrap().len(), 2);
                assert!(observed.lock().unwrap().last().unwrap().is_empty());
            }
            let requests = worker.join().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["args"]["summary"], true);
            assert!(requests[0]["args"]["favorite"].is_null());
            cache.shutdown().await.unwrap();
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            let observation = reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    read_cached_read(snapshot, &query, &UserId("user".into()))
                })
                .await
                .unwrap()
                .unwrap();
            let CatalogValue::Playlists(items) = observation.value else {
                panic!("playlist page missing")
            };
            assert_eq!(items.len(), if should_fail { 3 } else { 0 });
            reopened.shutdown().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn library_favorite_publisher_preserves_cache_on_error_and_uses_server_pagination_after_reopen(
    ) {
        use crate::application::repositories::InitialUserDataUpdate;
        use crate::domain::media::MediaType;
        for (media_type, should_fail) in [
            (MediaType::Track, true),
            (MediaType::Track, false),
            (MediaType::Album, true),
            (MediaType::Album, false),
            (MediaType::Playlist, true),
            (MediaType::Playlist, false),
        ] {
            let command = match media_type {
                MediaType::Track => "music/tracks/library_items",
                MediaType::Album => "music/albums/library_items",
                MediaType::Playlist => "music/playlists/library_items",
                _ => unreachable!(),
            };
            let names = move |favorites: &crate::domain::library::Favorites| -> Vec<String> {
                match media_type {
                    MediaType::Track => favorites
                        .tracks
                        .iter()
                        .map(|item| item.media.name.clone())
                        .collect(),
                    MediaType::Playlist => favorites
                        .playlists
                        .iter()
                        .map(|item| item.media.name.clone())
                        .collect(),
                    MediaType::Album => favorites
                        .albums
                        .iter()
                        .map(|item| item.media.name.clone())
                        .collect(),
                    _ => unreachable!(),
                }
            };
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let query = match media_type {
                MediaType::Track => CatalogQuery::LibraryTracks {
                    favorite: Some(true),
                    limit: 500,
                    offset: 0,
                    summary: false,
                },
                MediaType::Playlist => CatalogQuery::LibraryPlaylists {
                    favorite: Some(true),
                    limit: 500,
                    offset: 0,
                    summary: false,
                },
                MediaType::Album => CatalogQuery::LibraryAlbums {
                    favorite: Some(true),
                    limit: 500,
                    offset: 0,
                    summary: false,
                    filters: Default::default(),
                },
                _ => unreachable!(),
            };
            let response = json!([
                {"provider":"library","item_id":"cached-a","name":"Cached A","favorite":true},
                {"provider":"library","item_id":"cached-b","name":"Cached B","favorite":true}
            ]);
            let cached = match media_type {
                MediaType::Track => CatalogValue::Tracks(serde_json::from_value(response).unwrap()),
                MediaType::Playlist => {
                    CatalogValue::Playlists(serde_json::from_value(response).unwrap())
                }
                MediaType::Album => CatalogValue::Albums(serde_json::from_value(response).unwrap()),
                _ => unreachable!(),
            };
            let write_query = query.clone();
            let write_value = cached.clone();
            cache
                .database()
                .await
                .unwrap()
                .write_transaction(move |transaction| {
                    write_cached_read(
                        transaction,
                        &write_query,
                        &UserId("user".into()),
                        &write_value,
                        100,
                    )
                })
                .await
                .unwrap();
            let server_page = if should_fail {
                json!({"test_http_status":503})
            } else {
                json!((0..500).map(|index| json!({"provider":"library","item_id":format!("server-{index}"),"name":format!("Server {index:03}"),"favorite":true})).collect::<Vec<_>>())
            };
            let mut responses = [
                "music/tracks/library_items",
                "music/albums/library_items",
                "music/artists/library_items",
                "music/playlists/library_items",
            ]
            .into_iter()
            .filter(|candidate| *candidate != command)
            .map(|candidate| (candidate, json!([])))
            .collect::<Vec<_>>();
            responses.push((command, server_page));
            if !should_fail {
                responses.push((
                    command,
                    json!([{"provider":"library","item_id":"tail","name":"Tail","favorite":true}]),
                ));
            }
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(std::sync::Barrier::new(2));
            let worker_entered = entered.clone();
            let worker_release = release.clone();
            let mut has_blocked = false;
            let (client, worker) = server_with_request_handler(responses, move |request| {
                if !has_blocked && request["command"] == command {
                    has_blocked = true;
                    worker_entered.notify_one();
                    worker_release.wait();
                }
            });
            let client = Arc::new(client);
            let catalog = CachedCatalog::new(
                client.clone(),
                cache.clone(),
                UserId("user".into()),
                Duration::ZERO,
            )
            .await
            .unwrap();
            let visible = Arc::new(tokio::sync::Notify::new());
            let observed = Arc::new(Mutex::new(Vec::new()));
            let published = observed.clone();
            let publisher_visible = visible.clone();
            let loading = tokio::spawn(async move {
                let mut publish = move |update| {
                    if let InitialUserDataUpdate::FavoritePage(favorites) = update {
                        if !names(&favorites).is_empty() {
                            publisher_visible.notify_one();
                        }
                        published.lock().unwrap().push(favorites);
                    }
                    Ok::<(), String>(())
                };
                client
                    .load_favorite_pages(&catalog, false, &mut publish)
                    .await
            });
            tokio::time::timeout(Duration::from_secs(2), visible.notified())
                .await
                .unwrap();
            assert_eq!(
                names(observed.lock().unwrap().last().unwrap())[0],
                "Cached A"
            );
            tokio::time::timeout(Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            assert!(!loading.is_finished());
            release.wait();
            let result = tokio::time::timeout(Duration::from_secs(10), loading)
                .await
                .unwrap()
                .unwrap();
            if should_fail {
                assert!(result.is_err());
                assert_eq!(names(observed.lock().unwrap().last().unwrap()).len(), 2);
            } else {
                let favorites = result.unwrap();
                assert_eq!(names(&favorites).len(), 501);
                assert_eq!(names(&favorites)[0], "Server 000");
                assert_eq!(names(&favorites).last().unwrap(), "Tail");
            }
            let mut publications = observed
                .lock()
                .unwrap()
                .iter()
                .map(&names)
                .filter(|items| !items.is_empty())
                .collect::<Vec<_>>();
            // Independent categories may repeat the unchanged target projection.
            publications.dedup();
            assert_eq!(publications.len(), if should_fail { 1 } else { 3 });
            let requests = worker.join().unwrap();
            let page_requests = requests
                .iter()
                .filter(|request| request["command"] == command)
                .collect::<Vec<_>>();
            assert_eq!(page_requests.len(), if should_fail { 1 } else { 2 });
            for (index, request) in page_requests.iter().enumerate() {
                assert_eq!(request["args"]["favorite"], true);
                assert_eq!(request["args"]["summary"], false);
                assert_eq!(request["args"]["limit"], 500);
                assert_eq!(request["args"]["offset"], index * 500);
                assert_eq!(request["args"]["sort_field"], "sort_name");
                assert_eq!(request["args"]["sort_direction"], "asc");
            }
            cache.shutdown().await.unwrap();
            let reopened = ServerCache::initialize(directory.path().to_owned())
                .await
                .unwrap();
            reopened.activate("server".into()).await.unwrap();
            let observed = reopened
                .database()
                .await
                .unwrap()
                .read_snapshot(move |snapshot| {
                    read_cached_read(snapshot, &query, &UserId("user".into()))
                })
                .await
                .unwrap()
                .unwrap();
            let retained_names = match observed.value {
                CatalogValue::Tracks(items) => items
                    .into_iter()
                    .map(|item| item.media.name)
                    .collect::<Vec<_>>(),
                CatalogValue::Albums(items) => items
                    .into_iter()
                    .map(|item| item.media.name)
                    .collect::<Vec<_>>(),
                CatalogValue::Playlists(items) => items
                    .into_iter()
                    .map(|item| item.media.name)
                    .collect::<Vec<_>>(),
                _ => panic!("expected a library page"),
            };
            assert_eq!(retained_names.len(), if should_fail { 2 } else { 500 });
            assert_eq!(
                retained_names[0],
                if should_fail {
                    "Cached A"
                } else {
                    "Server 000"
                }
            );
            reopened.shutdown().await.unwrap();
        }
    }
}
