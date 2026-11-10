//! Session-scoped data capabilities; display consumers observe, commands may await completion.
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::{MediaId, UserId};
use crate::domain::library::{Favorites, PrivatePlaylist, UserDataSnapshot};
use crate::domain::media::{MediaItem, Playlist, Track};
use crate::domain::providers::{ProviderInstance, ProviderManifest};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RequestPriority {
    Background,
    Startup,
    Icon,
    #[default]
    Foreground,
}
tokio::task_local! { static REQUEST_PRIORITY: RequestPriority; }
// Request-local scheduling context never retains completed responses.
pub(crate) async fn with_request_priority<T>(
    priority: RequestPriority,
    operation: impl std::future::Future<Output = T>,
) -> T {
    REQUEST_PRIORITY.scope(priority, operation).await
}
pub(crate) fn current_priority() -> RequestPriority {
    REQUEST_PRIORITY
        .try_with(|priority| *priority)
        .unwrap_or_default()
}

pub(crate) trait CatalogClient: Send + Sync {
    fn clear_database_cache(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(std::future::ready(Err(
            "database cache maintenance is unavailable".into(),
        )))
    }

    /// Persists only a server-confirmed media readback after an online mutation.
    fn commit_confirmed_media(&self, _item: MediaItem) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// Retires a playlist only after its server library removal was confirmed.
    fn commit_confirmed_playlist_removal(
        &self,
        _playlist: Playlist,
    ) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// Reads confirmed contents without publishing a partial metadata/content cache update.
    fn read_confirmed_playlist_contents(
        &self,
        playlist: Playlist,
    ) -> BoxFuture<'_, Result<Vec<Track>, String>> {
        Box::pin(async move {
            let CatalogValue::Tracks(tracks) = self
                .load_catalog(
                    CatalogQuery::PlaylistTracks {
                        item_id: playlist.media.item_id,
                        provider: playlist.media.provider.as_str().to_owned(),
                        allow_dynamic_tracks: false,
                    },
                    true,
                )
                .await?
            else {
                return Err("playlist tracks unavailable".into());
            };
            Ok(tracks)
        })
    }

    /// Commits metadata and contents together after the caller publishes server confirmation.
    fn commit_confirmed_playlist_contents(
        &self,
        _playlist: Playlist,
        _tracks: Vec<Track>,
    ) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(std::future::ready(Ok(())))
    }

    // Startup reservations and online catalog queries.
    fn begin_startup(&self) -> Box<dyn Send>;
    fn load_catalog(
        &self,
        query: CatalogQuery,
        force_refresh: bool,
    ) -> BoxFuture<'_, Result<CatalogValue, String>>;
    fn observe_catalog(
        &self,
        query: CatalogQuery,
        force_refresh: bool,
    ) -> futures_util::stream::BoxStream<'_, crate::domain::catalog::CatalogLoadSnapshot> {
        use crate::domain::catalog::{
            CatalogFreshness, CatalogLoadPhase, CatalogLoadSnapshot, CatalogReadKnowledge,
        };
        use futures_util::StreamExt;
        Box::pin(
            futures_util::stream::once(self.load_catalog(query, force_refresh)).map(|result| {
                match result {
                    Ok(value) => CatalogLoadSnapshot {
                        value: Some(value),
                        source: Some(crate::domain::catalog::CatalogDataSource::Server),
                        cache_error: None,
                        phase: CatalogLoadPhase::Ready,
                        knowledge: CatalogReadKnowledge::ObservedResponse,
                        freshness: CatalogFreshness::Fresh,
                        observed_at: None,
                        error: None,
                    },
                    Err(error) => CatalogLoadSnapshot {
                        value: None,
                        source: None,
                        cache_error: None,
                        phase: CatalogLoadPhase::Failed,
                        knowledge: CatalogReadKnowledge::Unknown,
                        freshness: CatalogFreshness::Unknown,
                        observed_at: None,
                        error: Some(error),
                    },
                }
            }),
        )
    }
}
pub(crate) trait ProviderClient: Send + Sync {
    // Provider metadata and icon queries.
    fn read_manifests(&self) -> BoxFuture<'_, Result<ProviderRead<ProviderManifest>, String>>;
    fn read_instances(&self) -> BoxFuture<'_, Result<ProviderRead<ProviderInstance>, String>>;
    fn get_icon_data<'request>(
        &'request self,
        provider: &'request str,
    ) -> BoxFuture<'request, Result<Option<String>, String>>;
}

pub(crate) struct ProviderUpdate<T> {
    pub(crate) items: Option<Vec<T>>,
    pub(crate) cache_error: Option<String>,
}

pub(crate) struct ProviderRead<T> {
    pub(crate) current: ProviderUpdate<T>,
    pub(crate) refresh: Option<BoxFuture<'static, Result<ProviderUpdate<T>, ProviderRefreshError>>>,
}

#[derive(Debug)]
pub(crate) enum ProviderRefreshError {
    Failed(String),
    Invalidated(String),
}

impl<T> ProviderRead<T> {
    // Commands may require completion; read projections can consume current and retain refresh.
    pub(crate) async fn resolve(self) -> Result<ProviderUpdate<T>, String> {
        match self.refresh {
            Some(refresh) => match refresh.await {
                Ok(update) => Ok(update),
                Err(ProviderRefreshError::Failed(_)) if self.current.items.is_some() => {
                    Ok(self.current)
                }
                Err(
                    ProviderRefreshError::Failed(error) | ProviderRefreshError::Invalidated(error),
                ) => Err(error),
            },
            None => Ok(self.current),
        }
    }
}
pub(crate) struct ImageUpdate {
    pub(crate) bytes: Option<Vec<u8>>,
    pub(crate) cache_error: Option<String>,
}

pub(crate) struct ImageLoad {
    pub(crate) current: ImageUpdate,
    pub(crate) refresh: Option<BoxFuture<'static, Result<ImageUpdate, String>>>,
}

pub(crate) trait ImageLoader: Send + Sync {
    fn clear_image_cache(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(std::future::ready(Err(
            "image cache maintenance is unavailable".into(),
        )))
    }

    fn read_image<'request>(
        &'request self,
        request: &'request crate::domain::artwork::ImageRequest,
    ) -> BoxFuture<'request, Result<ImageLoad, String>>;
}

#[derive(Clone)]
pub(crate) struct SessionServices {
    pub(crate) images: Arc<dyn ImageLoader>,
    pub(crate) recommendations: Arc<crate::application::recommendations::Recommendations>,
    pub(crate) library: Arc<dyn LibraryClient>,
    pub(crate) providers: Arc<dyn ProviderClient>,
    pub(crate) catalog: Arc<dyn CatalogClient>,
}
pub(crate) enum InitialUserDataUpdate {
    UserIdentity(UserId),
    Favorites(Favorites),
    FavoritePage(Favorites),
    PlaylistPage(Vec<PrivatePlaylist>),
    Playlists(Vec<PrivatePlaylist>),
    PlaylistTracks { item_id: MediaId, items: Vec<Track> },
}
pub(crate) trait UserDataPublisher: Send {
    fn publish(&mut self, update: InitialUserDataUpdate) -> BoxFuture<'_, Result<(), String>>;
}
impl<Publish: FnMut(InitialUserDataUpdate) -> Result<(), String> + Send> UserDataPublisher
    for Publish
{
    // Publication may reject data belonging to a retired session.
    fn publish(&mut self, update: InitialUserDataUpdate) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(std::future::ready(self(update)))
    }
}
// Concurrent section loaders publish through one ordered snapshot writer.
pub(crate) struct SharedUserDataPublisher<'publisher, Publisher: UserDataPublisher + ?Sized> {
    publisher: &'publisher tokio::sync::Mutex<&'publisher mut Publisher>,
}

impl<'publisher, Publisher: UserDataPublisher + ?Sized>
    SharedUserDataPublisher<'publisher, Publisher>
{
    pub(crate) fn new(
        publisher: &'publisher tokio::sync::Mutex<&'publisher mut Publisher>,
    ) -> Self {
        Self { publisher }
    }
}

impl<Publisher: UserDataPublisher + ?Sized> UserDataPublisher
    for SharedUserDataPublisher<'_, Publisher>
{
    fn publish(&mut self, update: InitialUserDataUpdate) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move { self.publisher.lock().await.publish(update).await })
    }
}

pub(crate) trait LibraryClient: Send + Sync {
    // Server-confirmed library reads and ownership checks.
    fn get_current_user_id(&self) -> BoxFuture<'_, Result<UserId, String>>;
    fn load_library_user_data<'request>(
        &'request self,
        catalog: &'request dyn CatalogClient,
        user_id: &'request str,
    ) -> BoxFuture<'request, Result<UserDataSnapshot, String>>;
    fn load_favorites<'request>(
        &'request self,
        catalog: &'request dyn CatalogClient,
    ) -> BoxFuture<'request, Result<Favorites, String>>;
    fn load_favorite_pages<'request>(
        &'request self,
        catalog: &'request dyn CatalogClient,
        force_refresh: bool,
        publisher: &'request mut dyn UserDataPublisher,
    ) -> BoxFuture<'request, Result<Favorites, String>>;
    fn load_private_playlists<'request>(
        &'request self,
        catalog: &'request dyn CatalogClient,
        user_id: &'request str,
        force_refresh: bool,
        publisher: &'request mut dyn UserDataPublisher,
    ) -> BoxFuture<'request, Result<Vec<PrivatePlaylist>, String>>;
    fn load_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
        user_id: &'request str,
    ) -> BoxFuture<'request, Result<Option<PrivatePlaylist>, String>>;
    fn get_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
    ) -> BoxFuture<'request, Result<Playlist, String>>;
    fn resolve_library_item<'request>(
        &'request self,
        item: &'request MediaItem,
    ) -> BoxFuture<'request, Result<Option<MediaItem>, String>>;
    // Mutations complete online before callers replace their visible projection.
    fn set_favorite<'request>(
        &'request self,
        item: &'request MediaItem,
        is_favorite: bool,
    ) -> BoxFuture<'request, Result<(), String>>;
    fn create_private_playlist<'request>(
        &'request self,
        name: &'request str,
        user_id: &'request str,
    ) -> BoxFuture<'request, Result<Playlist, String>>;
    fn rename_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
        name: &'request str,
    ) -> BoxFuture<'request, Result<Playlist, String>>;
    fn remove_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
    ) -> BoxFuture<'request, Result<Playlist, String>>;
    fn add_playlist_tracks<'request>(
        &'request self,
        item_id: &'request str,
        uris: Vec<String>,
    ) -> BoxFuture<'request, Result<(), String>>;
    fn remove_playlist_tracks<'request>(
        &'request self,
        item_id: &'request str,
        positions: Vec<u32>,
    ) -> BoxFuture<'request, Result<(), String>>;
    fn set_private_playlist_image<'request>(
        &'request self,
        playlist: &'request Playlist,
        image: Option<String>,
    ) -> BoxFuture<'request, Result<Playlist, String>>;
}
