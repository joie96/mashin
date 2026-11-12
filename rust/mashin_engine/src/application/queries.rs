//! Typed state reads shared by native entry points.

use crate::application::errors::ApplicationError;
use crate::application::session::SessionTasks;
use crate::application::Application;
use crate::domain::media::{
    Album, Artist, MediaItem, MediaItemImage, MediaType, MusicAssistantSearchResults, Playlist,
    Track,
};
use crate::domain::providers::{
    project_album_availability, project_fields_availability, project_item_availability,
    project_track_availability, ProviderInstance, ProviderManifest,
};
use crate::domain::recommendations::{RecommendationSnapshot, RecommendationView};
use crate::domain::settings::UpdateChannel;
use crate::domain::settings::{session_settings, SessionSettings};
use crate::playback::models::{MediaSessionState, PlayerState, Queue};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
pub(crate) struct SessionQueryContext {
    pub(crate) tasks: Arc<SessionTasks>,
    pub(crate) is_authenticated: bool,
    pub(crate) generation: u64,
}

impl Application {
    // Queries and snapshots.

    pub(crate) async fn ensure_session_repositories(
        &self,
        expected_generation: u64,
    ) -> Result<(), ApplicationError> {
        let tasks = {
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            if coordinator.session_generation != expected_generation {
                return Err(ApplicationError::command_failed(
                    "session changed before online service initialization",
                ));
            }
            if coordinator.repositories.is_some() {
                return Ok(());
            }
            coordinator.session_tasks.clone()
        };
        let _load = tasks.lock_repositories_load().await;
        let (factory, client, server, user) = {
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            if coordinator.session_generation != expected_generation {
                return Err(ApplicationError::command_failed(
                    "session changed before online service initialization",
                ));
            }
            if coordinator.repositories.is_some() {
                return Ok(());
            }
            let connection = coordinator.connection.as_ref().ok_or_else(|| {
                ApplicationError::command_failed("a restored session is required")
            })?;
            (
                coordinator.environment.repositories.clone(),
                connection.client.clone(),
                connection.server_url.clone(),
                coordinator
                    .session_configuration
                    .as_ref()
                    .and_then(|session| session.user_id.clone()),
            )
        };
        let repositories = factory(client, server, user, expected_generation)
            .await
            .map_err(ApplicationError::command_failed)?;
        let accepted = {
            let mut coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            if coordinator.session_generation == expected_generation {
                coordinator.user_data = Some(crate::application::library::LibraryState::new(
                    repositories.library.clone(),
                ));
                coordinator.repositories = Some(repositories.clone());
                true
            } else {
                false
            }
        };
        if !accepted {
            return Err(ApplicationError::command_failed(
                "session changed while initializing online services",
            ));
        }
        Ok(())
    }

    // Native synchronous commands run on blocking workers, never while holding application state.
    pub(crate) fn ensure_session_repositories_sync(
        &self,
        expected_generation: u64,
    ) -> Result<(), ApplicationError> {
        {
            let coordinator = self
                .coordinator
                .lock()
                .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
            if coordinator.session_generation != expected_generation {
                return Err(ApplicationError::command_failed(
                    "session changed before repository lookup",
                ));
            }
            if coordinator.repositories.is_some() {
                return Ok(());
            }
        }
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    self.runtime
                        .block_on(self.ensure_session_repositories(expected_generation))
                })
                .join()
        })
        .map_err(|_| ApplicationError::internal("online initialization worker panicked"))?
    }

    pub(crate) fn catalog_repository(
        &self,
        expected_generation: u64,
    ) -> Result<Arc<dyn crate::application::repositories::CatalogClient>, ApplicationError> {
        self.ensure_session_repositories_sync(expected_generation)?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        if coordinator.session_generation != expected_generation {
            return Err(ApplicationError::command_failed(
                "session changed before catalog request",
            ));
        }
        coordinator
            .update_initial_user_data(expected_generation, |_| {})
            .map_err(ApplicationError::command_failed)?;
        coordinator
            .repositories
            .as_ref()
            .map(|repositories| repositories.catalog.clone())
            .ok_or_else(|| ApplicationError::internal("session repositories are not initialized"))
    }

    pub(crate) fn provider_repository(
        &self,
        expected_generation: u64,
    ) -> Result<Arc<dyn crate::application::repositories::ProviderClient>, ApplicationError> {
        self.ensure_session_repositories_sync(expected_generation)?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        if coordinator.session_generation != expected_generation {
            return Err(ApplicationError::command_failed(
                "session changed before provider request",
            ));
        }
        coordinator
            .update_initial_user_data(expected_generation, |_| {})
            .map_err(ApplicationError::command_failed)?;
        coordinator
            .repositories
            .as_ref()
            .map(|repositories| repositories.providers.clone())
            .ok_or_else(|| ApplicationError::internal("session repositories are not initialized"))
    }

    pub(crate) fn library_repository(
        &self,
    ) -> Result<
        (
            Arc<dyn crate::application::repositories::LibraryClient>,
            u64,
        ),
        ApplicationError,
    > {
        let generation = self.query_context()?.generation;
        self.ensure_session_repositories_sync(generation)?;
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        let generation = coordinator.session_generation;
        coordinator
            .update_initial_user_data(generation, |_| {})
            .map_err(ApplicationError::command_failed)?;
        let data = coordinator.user_data.as_ref().ok_or_else(|| {
            ApplicationError::internal("library state missing after initialization")
        })?;
        Ok((data.repository.clone(), generation))
    }

    pub(crate) fn query_context(&self) -> Result<SessionQueryContext, ApplicationError> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        Ok(SessionQueryContext {
            tasks: coordinator.session_tasks.clone(),
            is_authenticated: coordinator.connection.is_some(),
            generation: coordinator.session_generation,
        })
    }

    // Initial reads use the same complete contracts as their corresponding events.
    pub(crate) fn read_queue(&self) -> Result<Queue, ApplicationError> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        Ok(coordinator.playback.queue())
    }

    pub(crate) fn read_media_session_state(&self) -> Result<MediaSessionState, ApplicationError> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        Ok(coordinator.media_session_state())
    }

    pub(crate) fn read_session_settings(&self) -> Result<SessionSettings, ApplicationError> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        Ok(session_settings(coordinator.session_configuration.as_ref()))
    }

    pub(crate) fn read_player_state(&self) -> Result<PlayerState, ApplicationError> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ApplicationError::internal("core coordinator lock poisoned"))?;
        Ok(coordinator.playback.player_state())
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ApplicationQuery {
    GetSessionSettings {},
    GetCacheUsage {},
    GetUpdateChannel {
        default_channel: UpdateChannel,
    },
    RefreshQueue {},
    RefreshPlayerState {},
    StartSessionDataLoad {},
    LoadUserData {},
    GetUserData {},
    MediaSessionState {},
    CancelQuery {},
    Recommendations {
        view: RecommendationView,
        #[serde(default)]
        refresh: bool,
    },
    ProviderManifests {},
    ProviderIconData {
        provider: String,
    },
    MediaArtwork {
        item: MediaItem,
    },
    ReadImage {
        request: crate::domain::artwork::ImageRequest,
        #[serde(default)]
        refresh: bool,
    },
    ObserveImage {
        request: crate::domain::artwork::ImageRequest,
        observation_id: u64,
        #[serde(default)]
        is_background: bool,
    },
    RecentlyPlayedTracks {
        #[serde(default = "default_limit")]
        limit: u32,
    },
    SearchMedia {
        query: String,
        media_type: MediaType,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    Artist {
        item_id: String,
        provider: String,
    },
    Album {
        item_id: String,
        provider: String,
    },
    AlbumTracks {
        item_id: String,
        provider: String,
    },
    Playlist {
        item_id: String,
        provider: String,
    },
    PlaylistTracks {
        item_id: String,
        provider: String,
        #[serde(default)]
        force_refresh: bool,
        #[serde(default)]
        allow_dynamic_tracks: bool,
    },
    ArtistAlbums {
        item_id: String,
        provider: String,
        #[serde(default)]
        is_background: bool,
    },
    ArtistTopTracks {
        item_id: String,
        provider: String,
    },
    SimilarArtists {
        item_id: String,
        provider: String,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    SimilarTracks {
        item_id: String,
        provider: String,
        #[serde(default = "default_limit")]
        limit: u32,
        #[serde(default)]
        allow_lookup: bool,
    },
    TrackVersions {
        item_id: String,
        provider: String,
    },
}

impl ApplicationQuery {
    // Optional enrichment shares catalog identity while retaining its scheduling priority.
    pub(crate) fn request_priority(&self) -> super::repositories::RequestPriority {
        use super::repositories::{current_priority, RequestPriority};
        if matches!(self, Self::ArtistAlbums { is_background: true, .. } | Self::ObserveImage { is_background: true, .. }) {
            RequestPriority::Background
        } else if matches!(self, Self::ReadImage { .. } | Self::ObserveImage { .. } | Self::ProviderIconData { .. }) {
            RequestPriority::Icon
        } else {
            current_priority()
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum QueryResult {
    Unit,
    CacheUsage(super::repositories::CacheUsage),
    SessionSettings(crate::domain::settings::SessionSettings),
    UpdateChannel(UpdateChannel),
    Queue(crate::playback::models::Queue),
    PlayerState(crate::playback::models::PlayerState),
    MediaSessionState(crate::playback::models::MediaSessionState),
    UserData(crate::domain::library::UserDataSnapshot),
    Recommendations(RecommendationSnapshot),
    ProviderManifests(Vec<ProviderManifest>),
    ProviderIconData(Option<String>),
    MediaArtwork(Option<MediaItemImage>),
    ImageRead(ImageReadSnapshot),
    MediaItems(Vec<MediaItem>),
    SearchMedia(MusicAssistantSearchResults),
    Artist(Artist),
    Album(Album),
    Tracks(Vec<Track>),
    Playlist(Playlist),
    Albums(Vec<Album>),
    Artists(Vec<Artist>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageReadSnapshot {
    #[serde(skip)]
    pub(crate) did_publish_change: bool,
    #[serde(serialize_with = "serialize_image_bytes")]
    pub(crate) bytes: Option<Vec<u8>>,
    pub(crate) cache_error: Option<String>,
    pub(crate) has_refresh: bool,
}

fn serialize_image_bytes<S: serde::Serializer>(bytes: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error> {
    use base64::Engine;
    bytes.as_ref().map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)).serialize(serializer)
}

impl ImageReadSnapshot {
    /// One subscriber owns local delivery and the refresh until completion or cancellation.
    pub(crate) async fn observe(
        loader: &dyn super::repositories::ImageLoader,
        request: &crate::domain::artwork::ImageRequest,
        on_local: impl FnOnce(Self),
    ) -> Result<Self, String> {
        let started = std::time::Instant::now();
        let logger = crate::logging::Logger::new("Image.Read");
        let mut load = loader.read_image(request).await?;
        logger.trace(format_args!("stage=local elapsed_ms={} bytes={} has_refresh={} cache_error={}", started.elapsed().as_millis(), load.current.bytes.as_ref().map_or(0, |bytes| bytes.len()), load.refresh.is_some(), load.current.cache_error.is_some()));
        if load.refresh.is_some() { on_local(Self {
            did_publish_change: false,
            bytes: load.current.bytes.clone(),
            cache_error: load.current.cache_error.clone(),
            has_refresh: true,
        }); }
        let (update, did_publish_change) = match load.refresh.take() {
            Some(refresh) => {
                let update = refresh.await?;
                let did_publish_change = update.cache_error.is_none()
                    && update.bytes.is_some()
                    && update.bytes != load.current.bytes;
                (update, did_publish_change)
            }
            None => (load.current, false),
        };
        logger.trace(format_args!("stage=completed elapsed_ms={} bytes={} changed={} cache_error={}", started.elapsed().as_millis(), update.bytes.as_ref().map_or(0, |bytes| bytes.len()), did_publish_change, update.cache_error.is_some()));
        Ok(Self {
            did_publish_change,
            bytes: update.bytes,
            cache_error: update.cache_error,
            has_refresh: false,
        })
    }

    // The local query drops its unpolled refresh; the scoped follow-up owns network cancellation.
    pub(crate) async fn load(
        loader: &dyn super::repositories::ImageLoader,
        request: &crate::domain::artwork::ImageRequest,
        refresh: bool,
    ) -> Result<Self, String> {
        let mut load = loader.read_image(request).await?;
        let has_refresh = load.refresh.is_some() && !refresh;
        let mut did_publish_change = false;
        let update = if refresh {
            match load.refresh.take() {
                Some(refresh) => {
                    let update = refresh.await?;
                    did_publish_change = update.cache_error.is_none()
                        && update.bytes.is_some()
                        && update.bytes != load.current.bytes;
                    update
                }
                None => load.current,
            }
        } else {
            load.current
        };
        Ok(Self {
            did_publish_change,
            bytes: update.bytes,
            cache_error: update.cache_error,
            has_refresh,
        })
    }
}

impl QueryResult {
    // Only detached query projections receive effective provider flags.
    pub(crate) fn project_availability(&mut self, providers: Option<&[ProviderInstance]>) {
        match self {
            Self::MediaItems(items) => {
                for item in items {
                    project_item_availability(item, providers);
                }
            }
            Self::Artist(item) => project_fields_availability(&mut item.media, providers),
            Self::Album(item) => project_album_availability(item, providers),
            Self::Playlist(item) => project_fields_availability(&mut item.media, providers),
            Self::Tracks(items) => {
                for item in items {
                    project_track_availability(item, providers);
                }
            }
            Self::Albums(items) => {
                for item in items {
                    project_album_availability(item, providers);
                }
            }
            Self::Artists(items) => {
                for item in items {
                    project_fields_availability(&mut item.media, providers);
                }
            }
            Self::SearchMedia(results) => {
                for item in &mut results.tracks {
                    project_track_availability(item, providers);
                }
                for item in &mut results.albums {
                    project_album_availability(item, providers);
                }
                for item in &mut results.artists {
                    project_fields_availability(&mut item.media, providers);
                }
                for item in &mut results.playlists {
                    project_fields_availability(&mut item.media, providers);
                }
                for item in &mut results.radio {
                    project_item_availability(item, providers);
                }
            }
            Self::Recommendations(snapshot) => {
                for folder in &mut snapshot.folders {
                    for item in &mut folder.items {
                        project_item_availability(item, providers);
                    }
                    for item in &mut folder.playlist_tracks {
                        project_track_availability(item, providers);
                    }
                }
            }
            Self::UserData(snapshot) => {
                super::snapshots::project_library_availability(snapshot, providers)
            }
            _ => {}
        }
    }

    pub(crate) fn has_media(&self) -> bool {
        matches!(
            self,
            Self::MediaItems(_)
                | Self::Artist(_)
                | Self::Album(_)
                | Self::Playlist(_)
                | Self::Tracks(_)
                | Self::Albums(_)
                | Self::Artists(_)
                | Self::SearchMedia(_)
                | Self::Recommendations(_)
                | Self::UserData(_)
        )
    }
}

pub(crate) fn project_catalog_availability(
    value: &mut crate::domain::catalog::CatalogValue,
    providers: Option<&[ProviderInstance]>,
) {
    use crate::domain::catalog::CatalogValue;
    match value {
        CatalogValue::ProviderInstances(_) | CatalogValue::ProviderManifests(_) => {}
        CatalogValue::Artist(item) => project_fields_availability(&mut item.media, providers),
        CatalogValue::Album(item) => project_album_availability(item, providers),
        CatalogValue::Playlist(item) => project_fields_availability(&mut item.media, providers),
        CatalogValue::Playlists(items) => items
            .iter_mut()
            .for_each(|item| project_fields_availability(&mut item.media, providers)),
        CatalogValue::Track(item) => project_track_availability(item, providers),
        CatalogValue::Item(item) => project_item_availability(item, providers),
        CatalogValue::Tracks(items) => items
            .iter_mut()
            .for_each(|item| project_track_availability(item, providers)),
        CatalogValue::Albums(items) => items
            .iter_mut()
            .for_each(|item| project_album_availability(item, providers)),
        CatalogValue::Artists(items) => items
            .iter_mut()
            .for_each(|item| project_fields_availability(&mut item.media, providers)),
        CatalogValue::Search(results) => {
            let mut projection = QueryResult::SearchMedia(std::mem::take(results));
            projection.project_availability(providers);
            if let QueryResult::SearchMedia(projected) = projection {
                *results = projected;
            }
        }
        CatalogValue::RecommendationItems(items) => items
            .iter_mut()
            .for_each(|item| project_item_availability(item, providers)),
        CatalogValue::RecommendationFolders(folders) => {
            for folder in folders {
                for item in &mut folder.items {
                    project_item_availability(item, providers);
                }
                for track in &mut folder.playlist_tracks {
                    project_track_availability(track, providers);
                }
            }
        }
    }
}

fn default_limit() -> u32 {
    50
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::repositories::{ImageLoad, ImageLoader, ImageUpdate};
    use futures_util::future::BoxFuture;

    #[test]
    fn artist_album_request_preserves_explicit_background_priority() {
        for is_background in [false, true] {
            let request: ApplicationQuery = serde_json::from_value(serde_json::json!({
                "kind": "artistAlbums", "itemId": "artist", "provider": "source",
                "isBackground": is_background
            })).unwrap();
            assert_eq!(request.request_priority(), if is_background {
                crate::application::repositories::RequestPriority::Background
            } else {
                crate::application::repositories::RequestPriority::Foreground
            });
            assert!(matches!(request, ApplicationQuery::ArtistAlbums { is_background: actual, .. } if actual == is_background));
        }
        let request: ApplicationQuery = serde_json::from_value(serde_json::json!({
            "kind": "artistAlbums", "itemId": "artist", "provider": "source"
        })).unwrap();
        assert!(matches!(request, ApplicationQuery::ArtistAlbums { is_background: false, .. }));
    }

    #[test]
    fn image_requests_use_the_interactive_artwork_lane() {
        use crate::application::repositories::RequestPriority;
        for request in [
            serde_json::json!({"kind":"readImage","request":{"image":{"type":"thumb","provider":"builtin","path":"","remotely_accessible":false},"size":80,"position":0}}),
            serde_json::json!({"kind":"observeImage","observationId":7,"request":{"image":{"type":"thumb","provider":"builtin","path":"","remotely_accessible":false},"size":80,"position":0}}),
            serde_json::json!({"kind":"providerIconData","provider":"source"}),
        ] {
            let request: ApplicationQuery = serde_json::from_value(request).unwrap();
            assert_eq!(request.request_priority(), RequestPriority::Icon);
        }
        let background: ApplicationQuery = serde_json::from_value(serde_json::json!({"kind":"observeImage","observationId":7,"isBackground":true,"request":{"image":{"type":"thumb","provider":"builtin","path":"","remotely_accessible":false},"size":80,"position":0}})).unwrap();
        assert_eq!(background.request_priority(), RequestPriority::Background);
    }

    struct ImageReadFixture {
        bytes: Option<Vec<u8>>,
        cache_error: Option<String>,
    }

    #[tokio::test(flavor = "current_thread")]
    async fn image_observation_delivers_local_before_refresh_and_loads_once() {
        struct ObservedLoader {
            reads: std::sync::atomic::AtomicUsize,
            is_fresh: std::sync::atomic::AtomicBool,
            release: std::sync::Arc<tokio::sync::Notify>,
        }
        impl ImageLoader for ObservedLoader {
            fn read_image<'request>(&'request self, _: &'request crate::domain::artwork::ImageRequest) -> BoxFuture<'request, Result<ImageLoad, String>> {
                self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if self.is_fresh.load(std::sync::atomic::Ordering::SeqCst) {
                    return Box::pin(async { Ok(ImageLoad { current: ImageUpdate { bytes: Some(vec![1]), cache_error: None }, refresh: None }) });
                }
                let released = self.release.clone();
                Box::pin(async move { Ok(ImageLoad {
                    current: ImageUpdate { bytes: Some(vec![1]), cache_error: None },
                    refresh: Some(Box::pin(async move {
                        released.notified().await;
                        Ok(ImageUpdate { bytes: Some(vec![2]), cache_error: None })
                    })),
                }) })
            }
        }
        let loader = ObservedLoader { reads: std::sync::atomic::AtomicUsize::new(0), is_fresh: std::sync::atomic::AtomicBool::new(false), release: std::sync::Arc::new(tokio::sync::Notify::new()) };
        let request = serde_json::from_value(serde_json::json!({"image":{"type":"thumb","provider":"builtin","proxy_id":"a".repeat(64)},"position":0})).unwrap();
        let local = std::cell::RefCell::new(None);
        let mut pending = Box::pin(ImageReadSnapshot::observe(&loader, &request, |value| *local.borrow_mut() = Some(value)));
        assert!(futures_util::poll!(&mut pending).is_pending());
        assert!(local.borrow().as_ref().unwrap().has_refresh);
        assert_eq!(local.borrow().as_ref().unwrap().bytes.as_deref(), Some([1].as_slice()));
        loader.release.notify_one();
        let completed = pending.await.unwrap();
        assert_eq!(completed.bytes.as_deref(), Some([2].as_slice()));
        assert!(completed.did_publish_change);
        assert_eq!(loader.reads.load(std::sync::atomic::Ordering::SeqCst), 1);
        loader.is_fresh.store(true, std::sync::atomic::Ordering::SeqCst);
        let fresh = ImageReadSnapshot::observe(&loader, &request, |_| panic!("fresh bytes must only be sent with terminal completion")).await.unwrap();
        assert_eq!(fresh.bytes.as_deref(), Some([1].as_slice()));
        assert!(!fresh.has_refresh);
        assert_eq!(loader.reads.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    impl ImageLoader for ImageReadFixture {
        fn read_image<'request>(
            &'request self,
            _: &'request crate::domain::artwork::ImageRequest,
        ) -> BoxFuture<'request, Result<ImageLoad, String>> {
            let bytes = self.bytes.clone();
            let cache_error = self.cache_error.clone();
            Box::pin(async move {
                Ok(ImageLoad {
                    current: ImageUpdate {
                        bytes: Some(vec![1]),
                        cache_error: None,
                    },
                    refresh: Some(Box::pin(
                        async move { Ok(ImageUpdate { bytes, cache_error }) },
                    )),
                })
            })
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn image_change_signal_requires_changed_bytes_and_successful_publication() {
        let request = serde_json::from_value(serde_json::json!({"image":{"type":"thumb","provider":"builtin","proxy_id":"a".repeat(64)},"position":0})).unwrap();
        for (bytes, cache_error, expected) in [
            (Some(vec![2]), None, true),
            (Some(vec![1]), None, false),
            (None, None, false),
            (
                Some(vec![2]),
                Some("synthetic commit failure".to_owned()),
                false,
            ),
        ] {
            let fixture = ImageReadFixture { bytes, cache_error };
            let local = ImageReadSnapshot::load(&fixture, &request, false)
                .await
                .unwrap();
            assert!(!local.did_publish_change);
            let refreshed = ImageReadSnapshot::load(&fixture, &request, true)
                .await
                .unwrap();
            assert_eq!(refreshed.did_publish_change, expected);
            assert!(serde_json::to_value(refreshed)
                .unwrap()
                .get("didPublishChange")
                .is_none());
        }
    }
}
