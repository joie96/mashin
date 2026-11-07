//! Session startup publishes metadata before optional hydration and never owns confirmed storage.

use super::recommendations::Recommendations;
use super::repositories::CatalogClient;
use super::repositories::{InitialUserDataUpdate, LibraryClient, UserDataPublisher};
use crate::application::repositories::{with_request_priority, RequestPriority};
use crate::domain::catalog::{CatalogQuery, CatalogValue};
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
        load_initial_user_data(library, user_data_publisher),
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

// The phase order is independent of UI subscriptions and optional track/image downloads.
pub(crate) async fn load_initial_user_data(
    library: &dyn LibraryClient,
    mut publisher: impl UserDataPublisher,
) -> Result<(), String> {
    let user_id = library.get_current_user_id().await?;
    publisher
        .publish(InitialUserDataUpdate::UserIdentity(user_id.clone()))
        .await?;
    let playlists = library.load_private_playlists(user_id.as_str()).await?;
    publisher
        .publish(InitialUserDataUpdate::Playlists(playlists))
        .await?;
    let favorites = library.load_favorite_pages(&mut publisher).await?;
    publisher
        .publish(InitialUserDataUpdate::Favorites(favorites))
        .await
}

// Background hydration delivers server results into the visible playlist projection.
pub(crate) async fn load_initial_playlist_tracks(
    catalog: &dyn CatalogClient,
    playlists: Vec<PrivatePlaylist>,
    should_load: impl Fn(&str) -> bool + Sync,
    mut publisher: impl UserDataPublisher,
) -> Result<(), String> {
    let should_load = &should_load;
    let mut pending = stream::iter(playlists.into_iter().map(|playlist| async move {
        if !should_load(playlist.playlist.media.item_id.as_str()) {
            return None;
        }
        let item_id = playlist.playlist.media.item_id;
        let query = CatalogQuery::PlaylistTracks {
            item_id: item_id.clone(),
            provider: "library".into(),
            allow_dynamic_tracks: false,
        };
        let result = with_request_priority(RequestPriority::Background, async {
            match catalog.load_catalog(query, false).await? {
                CatalogValue::Tracks(items) => Ok(items),
                _ => Err("playlist query did not return tracks".to_owned()),
            }
        })
        .await;
        Some((item_id, result))
    }))
    .buffer_unordered(3);
    let mut load_error = None;
    while let Some(result) = pending.next().await {
        let Some((item_id, result)) = result else {
            continue;
        };
        match result {
            Ok(items) => {
                publisher
                    .publish(InitialUserDataUpdate::PlaylistTracks { item_id, items })
                    .await?
            }
            Err(error) => load_error = Some(error),
        }
    }
    load_error.map_or(Ok(()), Err)
}
