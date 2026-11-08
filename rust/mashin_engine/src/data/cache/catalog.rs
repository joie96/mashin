//! Parallel migrated-media reads over the existing MA read-group scheduler.
//! Only active refreshes live in RAM. Last-consumer drop aborts network work;
//! accepted SQLite writes retain worker durability and leases reject old generations.
//! Confirmed mutations reserve owner-wide FIFO order before asynchronous cache prerequisites.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

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
    group: Arc<ReadGroup>,
    changes: watch::Sender<CatalogLoadSnapshot>,
    updates: tokio::sync::broadcast::Sender<CatalogLoadSnapshot>,
    worker: Mutex<Option<tokio::task::AbortHandle>>,
}

enum RefreshError {
    Failed(String),
    Invalidated(String),
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
struct ServerIdentity {
    server_id: String,
}
impl Drop for Refresh {
    fn drop(&mut self) {
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
    fn publish(&self, snapshot: CatalogLoadSnapshot) {
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

impl CatalogRuntime {
    // Reserve confirmation order before identity/storage waits; readers never acquire this guard.
    async fn begin_confirmed_write(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.confirmed_revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.confirmed_writes.lock().await
    }

    // Retired sessions and leases cannot start or publish a new observation.
    fn require_active(&self) -> Result<(), RefreshError> {
        match &self.access {
            CacheAccess::Active(context) => context
                .database
                .require_active()
                .map_err(RefreshError::from_cache),
            CacheAccess::Session(session) => {
                if session
                    .owner
                    .generation
                    .load(std::sync::atomic::Ordering::Acquire)
                    != session.generation
                {
                    Err(RefreshError::Invalidated(
                        "catalog belongs to a retired session".into(),
                    ))
                } else {
                    Ok(())
                }
            }
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
                refresh.changes.borrow().phase == CatalogLoadPhase::Refreshing
            })
        });
        let expected_revision = self
            .confirmed_revision
            .load(std::sync::atomic::Ordering::Acquire);
        let key = (query.clone(), force_refresh, expected_revision);
        let requires_independent_refresh = match &query {
            CatalogQuery::LibraryArtists { filters, .. } => filters.requires_new_random_sample(),
            CatalogQuery::LibraryAlbums { filters, .. } => filters.requires_new_random_sample(),
            CatalogQuery::PlaylistTracks { .. } | CatalogQuery::RecommendationItems { .. } => true,
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
            return refresh;
        }
        let group = self.client.create_read_group();
        let (changes, _) = watch::channel(CatalogLoadSnapshot {
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
                    while local_result.is_none() || remote_result.is_none() {
                        tokio::select! {
                            result = &mut local, if local_result.is_none() => {
                                if runtime.require_active().is_err() { break; }
                                if let Some(publication) = publication.upgrade() {
                                    let mut state = publication.changes.borrow().clone();
                                    state.phase = CatalogLoadPhase::Refreshing;
                                    match &result {
                                        Ok((_, cached)) if state.source != Some(crate::domain::catalog::CatalogDataSource::Server) => {
                                            state.value = cached.value.clone();
                                            state.source = cached.source;
                                            state.knowledge = cached.knowledge;
                                            state.freshness = cached.freshness;
                                            state.observed_at = cached.observed_at;
                                        }
                                        Ok((_, cached)) => {
                                            if let Some(server_value) = state.value.as_ref() {
                                                state.value = Some(merge_server_value(cached.value.clone(), server_value.clone()));
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
                                if let (Ok(value), Some(publication)) = (&result, publication.upgrade()) {
                                    if runtime.confirmed_revision.load(std::sync::atomic::Ordering::Acquire) == expected_revision {
                                        let mut state = publication.changes.borrow().clone();
                                        state.phase = CatalogLoadPhase::Refreshing;
                                        state.value = Some(merge_server_value(state.value.take(), value.clone()));
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
                            publication.publish(state);
                        }
                        return;
                    }
                    let local_result = local_result.unwrap();
                    let Some(publication) = publication.upgrade() else { return; };
                    let mut state = publication.changes.borrow().clone();
                    let value = match remote_result.unwrap() {
                        Ok(value) => value,
                        Err(error) => {
                            state.phase = CatalogLoadPhase::Failed;
                            state.error = Some(error);
                            publication.publish(state);
                            return;
                        }
                    };
                    let persistence = match local_result {
                        Ok((context, _)) => {
                            let verify = runtime.verify_server_identity(&context, false);
                            match tokio::time::timeout(Duration::from_secs(5), verify).await {
                                Ok(Ok(())) => {
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
                                Ok(Err(error)) => Err(error),
                                Err(_) => Err(RefreshError::Failed("cache identity verification exceeded its deadline".into())),
                            }
                        }
                        Err(error) => Err(error),
                    };
                    if runtime.confirmed_revision.load(std::sync::atomic::Ordering::Acquire) != expected_revision {
                        state.phase = CatalogLoadPhase::Failed;
                        state.error = Some("catalog refresh superseded by confirmed mutation".into());
                        if let Ok(context) = runtime.cache_context().await {
                            let user_id = context.user_id.clone();
                            let latest_query = query.clone();
                            state.value = context.database.read_snapshot(move |snapshot| read_partial_cached(snapshot, &latest_query, &user_id)).await.ok().flatten();
                        } else { state.value = None; }
                        state.knowledge = if state.value.is_some() { CatalogReadKnowledge::Partial } else { CatalogReadKnowledge::Unknown };
                        state.freshness = CatalogFreshness::Unknown;
                        state.observed_at = None;
                    } else {
                        state.phase = CatalogLoadPhase::Ready;
                        match persistence {
                            Ok(()) => {}
                            Err(RefreshError::Failed(error)) => {
                                LOGGER.debug(format_args!("Cache persistence failed: {error}"));
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
        let (observed, partial) = context
            .database
            .read_snapshot(move |snapshot| {
                let observed = read_cached_read(snapshot, &read_query, &user_id)?;
                let partial = if observed.is_none() {
                    read_partial_cached(snapshot, &read_query, &user_id)?
                } else {
                    None
                };
                Ok((observed, partial))
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
        let value = observed
            .as_ref()
            .map(|value| value.value.clone())
            .or(partial);
        let snapshot = CatalogLoadSnapshot {
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
            value,
            phase: CatalogLoadPhase::Refreshing,
            observed_at: observed.map(|value| value.observed_at),
            error: None,
            cache_error: None,
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

    // Legacy consumers await completion; observing consumers receive local data before refresh.
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
            self.inner.client.observe_catalog(query, force_refresh)
        }
    }
}

pub(crate) struct CatalogCacheOwner {
    directory: Arc<dyn Fn() -> Result<std::path::PathBuf, String> + Send + Sync>,
    generation: std::sync::atomic::AtomicU64,
    cache: tokio::sync::Mutex<Option<ServerCache>>,
    confirmed_writes: Arc<tokio::sync::Mutex<()>>,
}

struct SessionCache {
    owner: Arc<CatalogCacheOwner>,
    client: Arc<MusicAssistantClient>,
    user_id: Option<UserId>,
    generation: u64,
    resolved: tokio::sync::OnceCell<Arc<CacheContext>>,
}

impl CatalogCacheOwner {
    // A single app-session factory owns the cache across user and address changes.
    pub(crate) fn new(
        directory: impl Fn() -> Result<std::path::PathBuf, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            directory: Arc::new(directory),
            generation: std::sync::atomic::AtomicU64::new(0),
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
                    resolved: tokio::sync::OnceCell::new(),
                })),
                fresh_for: Duration::from_secs(300),
                confirmed_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                confirmed_writes: self.confirmed_writes.clone(),
                refreshes: Mutex::new(HashMap::new()),
            }),
        }
    }
    pub(crate) async fn shutdown(&self, generation: u64) -> Result<(), String> {
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
}

impl SessionCache {
    // Identity lookup and filesystem initialization happen only for migrated media reads.
    async fn resolve(&self) -> Result<Arc<CacheContext>, String> {
        if self
            .owner
            .generation
            .load(std::sync::atomic::Ordering::Acquire)
            != self.generation
        {
            return Err("catalog belongs to a retired session".to_owned());
        }
        let catalog = self
            .resolved
            .get_or_try_init(|| async {
                LOGGER.debug(format_args!("Cache initialization: resolving user"));
                let user_id = match self.user_id.clone() {
                    Some(user_id) => user_id,
                    None => {
                        crate::application::repositories::LibraryClient::get_current_user_id(
                            self.client.as_ref(),
                        )
                        .await?
                    }
                };
                let mut owner = self.owner.cache.lock().await;
                if self
                    .owner
                    .generation
                    .load(std::sync::atomic::Ordering::Acquire)
                    != self.generation
                {
                    return Err("session changed before cache initialization".to_owned());
                }
                if owner.is_none() {
                    LOGGER.debug(format_args!("Cache initialization: opening storage"));
                    *owner = Some(
                        ServerCache::initialize((self.owner.directory)()?)
                            .await
                            .map_err(|error| error.to_string())?,
                    );
                }
                let cache = owner.as_ref().unwrap().clone();
                drop(owner);
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
                if self
                    .owner
                    .generation
                    .load(std::sync::atomic::Ordering::Acquire)
                    != self.generation
                {
                    return Err("session changed before database activation".into());
                }
                LOGGER.debug(format_args!("Cache initialization: activating database"));
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
                if self
                    .owner
                    .generation
                    .load(std::sync::atomic::Ordering::Acquire)
                    != self.generation
                {
                    return Err("session changed during cache initialization".to_owned());
                }
                LOGGER.debug(format_args!("Cache initialization: database ready"));
                Ok(Arc::new(CacheContext {
                    cache,
                    database,
                    user_id,
                    expected_server_id: Some(server_id),
                    is_identity_verified: std::sync::atomic::AtomicBool::new(is_verified),
                }))
            })
            .await?;
        Ok(catalog.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::music_assistant::client::tests::{server, server_with_request_handler};
    use crate::domain::identifiers::MediaId;
    use serde_json::json;

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
            assert_eq!(initial.value, Some(old_value.clone()));
            assert_eq!(initial.knowledge, CatalogReadKnowledge::ObservedResponse);
            assert_eq!(initial.freshness, CatalogFreshness::Fresh);
            entered.notified().await;
            let mut second = catalog.observe_catalog(read_query.clone(), false);
            assert_eq!(second.next().await.unwrap().value, Some(old_value));
            release.wait();
            for updates in [&mut first, &mut second] {
                let completed = completed_snapshot(updates).await;
                assert_eq!(completed.phase, CatalogLoadPhase::Ready);
                assert_eq!(
                    completed.source,
                    Some(crate::domain::catalog::CatalogDataSource::Server)
                );
                assert_eq!(completed.value, Some(new_value.clone()));
                assert!(completed.cache_error.is_none());
            }
            assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);
            let mut forced = catalog.observe_catalog(read_query.clone(), true);
            assert_eq!(forced.next().await.unwrap().value, Some(new_value.clone()));
            let failed = completed_snapshot(&mut forced).await;
            assert_eq!(failed.phase, CatalogLoadPhase::Failed);
            assert_eq!(failed.value, Some(new_value.clone()));
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
            assert_eq!(initial.value, Some(previous_value.clone()));
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
            assert_eq!(second.next().await.unwrap().value, Some(previous_value));
            release.wait();
            for observation in [&mut first, &mut second] {
                let completed = completed_snapshot(observation).await;
                assert_eq!(completed.phase, CatalogLoadPhase::Ready);
                assert_eq!(completed.freshness, CatalogFreshness::Fresh);
                assert_eq!(completed.value, Some(refreshed_value.clone()));
            }
            assert_eq!(request_count.load(std::sync::atomic::Ordering::SeqCst), 1);
            let mut forced = catalog.observe_catalog(read_query.clone(), true);
            assert_eq!(
                forced.next().await.unwrap().value,
                Some(refreshed_value.clone())
            );
            let failure = forced.next().await.unwrap();
            assert_eq!(failure.phase, CatalogLoadPhase::Failed);
            assert_eq!(failure.value, Some(refreshed_value.clone()));
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
                [("builtin", false), ("library", true), ("source-a", false)].map(
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
        assert_eq!(observation.next().await.unwrap().value, Some(previous));
        entered.notified().await;
        release.wait();
        let completed = completed_snapshot(&mut observation).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        let confirmed = completed.value.unwrap();
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
            assert_eq!(first.next().await.unwrap().value, Some(old.clone()));
            assert_eq!(second.next().await.unwrap().value, Some(old));
            entered.notified().await;
            release.wait();
            let completed = completed_snapshot(&mut first).await;
            assert_eq!(completed.phase, CatalogLoadPhase::Ready);
            assert_eq!(
                completed.source,
                Some(crate::domain::catalog::CatalogDataSource::Server)
            );
            assert_eq!(completed_snapshot(&mut second).await.value, completed.value);
            let Some(CatalogValue::Tracks(tracks)) = &completed.value else {
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
            assert_eq!(replaced.value, Some(CatalogValue::Tracks(Vec::new())));
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
        assert_eq!(first.next().await.unwrap().value, Some(old.clone()));
        assert_eq!(second.next().await.unwrap().value, Some(old));
        entered.notified().await;
        release.wait();
        let completed = completed_snapshot(&mut first).await;
        assert_eq!(completed.phase, CatalogLoadPhase::Ready);
        assert_eq!(
            completed.source,
            Some(crate::domain::catalog::CatalogDataSource::Server)
        );
        assert_eq!(completed_snapshot(&mut second).await.value, completed.value);
        let Some(CatalogValue::Search(results)) = &completed.value else {
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
            Some(CatalogValue::Search(Default::default()))
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
            assert_eq!(local.value, Some(old));
            assert_eq!(local.freshness, CatalogFreshness::Fresh);
            entered.notified().await;
            release.wait();
            let ready = completed_snapshot(&mut first).await;
            assert_eq!(ready.phase, CatalogLoadPhase::Ready);
            assert_eq!(ready.value, Some(updated.clone()));
            assert_eq!(
                ready.source,
                Some(crate::domain::catalog::CatalogDataSource::Server)
            );
            let mut failed = catalog.observe_catalog(query.clone(), true);
            let retained = completed_snapshot(&mut failed).await;
            assert_eq!(retained.phase, CatalogLoadPhase::Failed);
            assert_eq!(retained.value, Some(updated.clone()));
            let mut online_only = catalog.observe_catalog(query.clone(), true);
            let uncommitted = completed_snapshot(&mut online_only).await;
            assert_eq!(uncommitted.phase, CatalogLoadPhase::Ready);
            assert_eq!(uncommitted.value, Some(unsupported));
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
            assert_eq!(replaced.value, Some(empty.clone()));
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
        let Some(CatalogValue::Search(results)) = completed.value else {
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
        assert_eq!(observation.next().await.unwrap().value, Some(value("Old")));
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
        let Some(CatalogValue::Artist(artist)) = retired.value else {
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
            Some(CatalogValue::Album(old.clone()))
        );
        assert_eq!(
            second.next().await.unwrap().value,
            Some(CatalogValue::Album(old))
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
            let Some(CatalogValue::Album(album)) = retired.value else {
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
            let owner = Arc::new(CatalogCacheOwner::new(move || Ok(path.clone())));
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
            assert_eq!(first.next().await.unwrap().value, Some(value.clone()));
            assert_eq!(second.next().await.unwrap().value, Some(value.clone()));
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
                    assert_eq!(retired.value, Some(value.clone()));
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
            assert_eq!(first.next().await.unwrap().value, Some(old_value.clone()));
            assert_eq!(second.next().await.unwrap().value, Some(old_value.clone()));
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
                let item = match retired.value.unwrap() {
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
        assert_eq!(first_local.value, Some(value("Old")));
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
            Some(value("Old"))
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
        assert_eq!(completed.value, Some(value("Fresh")));
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
        assert_eq!(updates.next().await.unwrap().value, Some(value("Old")));
        let failed = updates.next().await.unwrap();
        assert_eq!(failed.phase, CatalogLoadPhase::Failed);
        assert_eq!(failed.value, Some(value("Old")));
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
        let owner = Arc::new(CatalogCacheOwner::new(move || Ok(root.clone())));
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
    async fn cold_session_creates_database_and_persists_server_response() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let owner = Arc::new(CatalogCacheOwner::new(move || Ok(root.clone())));
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
            .join("Cache/mashin-media-cache.sqlite3")
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
        let owner = Arc::new(CatalogCacheOwner::new(move || Ok(root.clone())));
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
        assert_eq!(first.value, Some(value("Server")));
        assert_eq!(first.phase, CatalogLoadPhase::Refreshing);
        drop(guard);
        while let Some(snapshot) = updates.next().await {
            assert_eq!(snapshot.value, Some(value("Server")));
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
        assert_eq!(completed.value, Some(value("Server")));
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
            let owner = Arc::new(CatalogCacheOwner::new(move || Ok(root.clone())));
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
            let CatalogValue::Artist(artist) = completed.value.unwrap() else {
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
        let owner = Arc::new(CatalogCacheOwner::new(|| {
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
        assert_eq!(completed.value, Some(value("Server")));
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
            let Some(CatalogValue::Artists(items)) = &local.value else {
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
                assert_eq!(refreshed.value, Some(CatalogValue::Artists(Vec::new())));
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
            let Some(CatalogValue::Albums(items)) = &local.value else {
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
                assert_eq!(refreshed.value, Some(CatalogValue::Albums(vec![])));
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
        assert_eq!(local.value, Some(value("Old")));
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
            let owner = Arc::new(CatalogCacheOwner::new(move || Ok(root.clone())));
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
            assert_eq!(local.value, Some(value("Old")));
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
        let owner = Arc::new(CatalogCacheOwner::new(move || Ok(root.clone())));
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
        assert_eq!(requests.len(), 9);
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
    async fn library_album_and_playlist_publisher_preserves_cache_on_error_and_uses_server_pagination_after_reopen(
    ) {
        use crate::application::repositories::InitialUserDataUpdate;
        for (is_playlist, should_fail) in
            [(false, true), (false, false), (true, true), (true, false)]
        {
            let command = if is_playlist {
                "music/playlists/library_items"
            } else {
                "music/albums/library_items"
            };
            let names = |favorites: &crate::domain::library::Favorites| -> Vec<String> {
                if is_playlist {
                    favorites
                        .playlists
                        .iter()
                        .map(|item| item.media.name.clone())
                        .collect()
                } else {
                    favorites
                        .albums
                        .iter()
                        .map(|item| item.media.name.clone())
                        .collect()
                }
            };
            let directory = tempfile::tempdir().unwrap();
            let cache = cache(directory.path()).await;
            let query = if is_playlist {
                CatalogQuery::LibraryPlaylists {
                    favorite: Some(true),
                    limit: 500,
                    offset: 0,
                    summary: false,
                }
            } else {
                CatalogQuery::LibraryAlbums {
                    favorite: Some(true),
                    limit: 500,
                    offset: 0,
                    summary: false,
                    filters: Default::default(),
                }
            };
            let response = json!([
                {"provider":"library","item_id":"cached-a","name":"Cached A","favorite":true},
                {"provider":"library","item_id":"cached-b","name":"Cached B","favorite":true}
            ]);
            let cached = if is_playlist {
                CatalogValue::Playlists(serde_json::from_value(response).unwrap())
            } else {
                CatalogValue::Albums(serde_json::from_value(response).unwrap())
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
            let mut responses = vec![("music/tracks/library_items", json!([]))];
            if is_playlist {
                responses.extend([
                    ("music/albums/library_items", json!([])),
                    ("music/artists/library_items", json!([])),
                ]);
            }
            responses.push((command, server_page));
            if !should_fail {
                responses.push((
                    command,
                    json!([{"provider":"library","item_id":"tail","name":"Tail","favorite":true}]),
                ));
                if !is_playlist {
                    responses.extend([
                        ("music/artists/library_items", json!([])),
                        ("music/playlists/library_items", json!([])),
                    ]);
                }
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
                        if (is_playlist && !favorites.playlists.is_empty())
                            || (!is_playlist && !favorites.albums.is_empty())
                        {
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
            let publications = observed
                .lock()
                .unwrap()
                .iter()
                .filter(|favorites| !names(favorites).is_empty())
                .count();
            // Other completed media pages remain distinct publisher updates.
            assert_eq!(
                publications,
                if should_fail {
                    1
                } else if is_playlist {
                    3
                } else {
                    5
                }
            );
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
