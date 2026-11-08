//! Online capabilities; completed media responses belong to their requesting page.
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
    fn get_manifests(&self) -> BoxFuture<'_, Result<Vec<ProviderManifest>, String>>;
    fn get_instances(&self) -> BoxFuture<'_, Result<Vec<ProviderInstance>, String>>;
    fn get_icon_data<'request>(
        &'request self,
        provider: &'request str,
    ) -> BoxFuture<'request, Result<Option<String>, String>>;
}
pub(crate) trait ArtworkClient: Send + Sync {
    // Resolve artwork from item metadata or online provider mappings.
    fn get_media_artwork<'request>(
        &'request self,
        item: &'request MediaItem,
    ) -> BoxFuture<'request, Option<crate::domain::media::MediaItemImage>>;
}
pub(crate) trait NetworkImages: Send + Sync {
    // Network bytes belong to the caller, not a persistent image store.
    fn load_bytes<'request>(
        &'request self,
        source: &'request str,
    ) -> BoxFuture<'request, Option<Vec<u8>>>;
}
#[derive(Clone)]
pub(crate) struct SessionServices {
    pub(crate) images: Arc<dyn NetworkImages>,
    pub(crate) artwork: Arc<dyn ArtworkClient>,
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
