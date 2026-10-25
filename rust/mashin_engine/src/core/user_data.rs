//! Session-bound MA snapshots and volatile favorite workers. Network work never holds the core lock.

use super::*;
use crate::music_assistant::event_hub::{
    EventHubLeaseReason, MusicAssistantEvent, MusicAssistantEventHub,
};
use futures_util::{stream, StreamExt};
use std::sync::Arc;

pub(super) struct UserDataEvents {
    hub: Arc<Mutex<MusicAssistantEventHub>>,
    worker: tokio::task::JoinHandle<()>,
}

impl Drop for UserDataEvents {
    fn drop(&mut self) {
        self.worker.abort();
        if let Ok(mut hub) = self.hub.lock() {
            hub.release(EventHubLeaseReason::UserData);
        }
    }
}

// Initial loading and refresh preserve optimistic wishes received during network queries.
pub(crate) fn read_user_data() -> Result<UserDataSnapshot, CoreError> {
    CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?
        .user_data_snapshot()
        .map_err(CoreError::command_failed)
}

pub(crate) fn read_or_load_user_data() -> Result<UserDataSnapshot, CoreError> {
    let _load_guard = CORE_NETWORK_RUNTIME.block_on(CORE_USER_DATA_LOAD.lock());
    {
        let coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        if let Some(data) = coordinator
            .user_data
            .as_ref()
            .filter(|data| data.has_loaded_library_data)
        {
            return Ok(data.snapshot.clone());
        }
    }
    let (client, generation) = session_client()?;
    CORE_NETWORK_RUNTIME
        .block_on(async {
            let user = client.get_current_user().await?;
            let favorites = client.load_favorites().await?;
            publish_initial_user_data(generation, |data| {
                let mut snapshot = data.snapshot.clone();
                snapshot.favorites = favorites;
                snapshot.is_loading_playlists = true;
                data.replace_snapshot(snapshot);
            })?;
            let playlists = client
                .load_private_playlist_metadata(user.user_id.as_str())
                .await?;
            publish_initial_user_data(generation, |data| {
                data.snapshot.playlists = playlists.clone();
                data.snapshot.is_loading_playlists = false;
            })?;
            let mut pending = stream::iter(playlists.into_iter().map(|mut playlist| {
                let client = client.clone();
                async move {
                    let items = client
                        .get_playlist_tracks(
                            &playlist.playlist.media.item_id,
                            "library",
                            true,
                            false,
                        )
                        .await;
                    match items {
                        Ok(items) => playlist.items = items,
                        Err(error) => {
                            LOGGER.warn(format!("load private playlist tracks failed: {error}"))
                        }
                    }
                    playlist.is_loading_tracks = false;
                    playlist
                }
            }))
            .buffer_unordered(3);
            while let Some(playlist) = pending.next().await {
                let playlist_id = playlist.playlist.media.item_id.as_str().to_owned();
                publish_initial_user_data(generation, |data| {
                    data.snapshot.replace_playlist(&playlist_id, Some(playlist));
                })?;
            }
            publish_initial_user_data(generation, |data| data.has_loaded_library_data = true)
        })
        .map_err(CoreError::command_failed)?;
    start_user_data_events()?;
    read_user_data()
}

pub(crate) fn refresh_user_data() -> Result<UserDataSnapshot, CoreError> {
    CORE_NETWORK_RUNTIME.block_on(refresh_user_data_for_session(None, UserDataRefresh::All))
}

enum UserDataRefresh {
    All,
    Favorites,
    Playlists,
    Playlist(String),
    PlaylistCover(String),
}

async fn refresh_user_data_for_session(
    expected_generation: Option<u64>,
    refresh: UserDataRefresh,
) -> Result<UserDataSnapshot, CoreError> {
    let _load_guard = CORE_USER_DATA_LOAD.lock().await;
    let (client, generation) = session_client()?;
    if expected_generation.is_some_and(|expected| expected != generation) {
        return Err(CoreError::command_failed(
            "user data session changed during playlist update",
        ));
    }
    let (revision, mut snapshot) = {
        let coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        let data = coordinator
            .user_data
            .as_ref()
            .ok_or_else(|| CoreError::command_failed("user data is not loaded"))?;
        (data.revision, data.snapshot.clone())
    };
    let previous_snapshot = snapshot.clone();
    let result = async {
        match &refresh {
            UserDataRefresh::All => {
                let user = client.get_current_user().await?;
                snapshot = client.load_library_user_data(user.user_id.as_str()).await?;
            }
            UserDataRefresh::Favorites => snapshot.favorites = client.load_favorites().await?,
            UserDataRefresh::Playlists => {
                let user = client.get_current_user().await?;
                snapshot.playlists = client.load_private_playlists(user.user_id.as_str()).await?;
            }
            UserDataRefresh::Playlist(playlist_id)
            | UserDataRefresh::PlaylistCover(playlist_id) => {
                let user = client.get_current_user().await?;
                let playlist = client
                    .load_private_playlist(playlist_id, user.user_id.as_str())
                    .await?;
                snapshot.replace_playlist(playlist_id, playlist);
            }
        }
        Ok::<_, String>(())
    }
    .await;
    result.map_err(CoreError::command_failed)?;
    let mut snapshot = {
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        if coordinator.user_data_session_generation != generation {
            return Err(CoreError::command_failed(
                "session changed while refreshing user data",
            ));
        }
        let data = coordinator
            .user_data
            .as_mut()
            .ok_or_else(|| CoreError::command_failed("user data is not loaded"))?;
        if data.revision != revision
            || !matches!(refresh, UserDataRefresh::All | UserDataRefresh::Favorites)
        {
            snapshot.favorites = data.snapshot.favorites.clone();
        }
        data.replace_snapshot(snapshot);
        data.snapshot.clone()
    };
    publish_background_event(MediaSessionEvent::UserDataChanged);

    // Confirmed tracks are visible before artwork downloads; the refresh lock still orders writes.
    let playlists_before_artwork = snapshot.playlists.clone();
    if let UserDataRefresh::PlaylistCover(playlist_id) = &refresh {
        let playlist = snapshot
            .playlists
            .iter_mut()
            .find(|entry| entry.playlist.media.item_id.as_str() == playlist_id)
            .ok_or_else(|| CoreError::command_failed("private playlist is not available"))?;
        client
            .refresh_private_playlist_cover(playlist)
            .await
            .map_err(CoreError::command_failed)?;
    } else {
        for error in client
            .refresh_playlist_collages(&previous_snapshot, &mut snapshot)
            .await
        {
            LOGGER.warn(error);
        }
    }
    let has_updated_artwork = snapshot.playlists != playlists_before_artwork;
    let snapshot = {
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        if coordinator.user_data_session_generation != generation {
            return Err(CoreError::command_failed(
                "session changed while updating playlist artwork",
            ));
        }
        let data = coordinator
            .user_data
            .as_mut()
            .ok_or_else(|| CoreError::command_failed("user data is not loaded"))?;
        if has_updated_artwork {
            snapshot.favorites = data.snapshot.favorites.clone();
            data.replace_snapshot(snapshot);
        }
        data.snapshot.clone()
    };
    if has_updated_artwork {
        publish_background_event(MediaSessionEvent::UserDataChanged);
    }
    Ok(snapshot)
}

// Favorite commands only touch memory; each identity has one independent background worker.
pub(crate) fn set_user_data_favorites(
    items: Vec<MediaItem>,
    is_favorite: bool,
) -> Result<UserDataSnapshot, CoreError> {
    let (client, generation, workers, snapshot) = {
        let mut coordinator = CORE_COORDINATOR
            .lock()
            .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
        let client = coordinator
            .connection
            .as_ref()
            .ok_or_else(|| CoreError::command_failed("a restored session is required"))?
            .client
            .clone();
        let generation = coordinator.user_data_session_generation;
        let data = coordinator
            .user_data
            .as_mut()
            .ok_or_else(|| CoreError::command_failed("user data is not loaded"))?;
        if items.iter().any(|item| {
            !matches!(
                item,
                MediaItem::Track(_)
                    | MediaItem::Album(_)
                    | MediaItem::Artist(_)
                    | MediaItem::Playlist(_)
            )
        }) {
            return Err(CoreError::command_failed("unsupported favorite media type"));
        }
        let workers = items
            .into_iter()
            .filter_map(|item| {
                let (identity, item, should_start) = data.submit(item, is_favorite);
                should_start.then_some((identity, item))
            })
            .collect::<Vec<_>>();
        (client, generation, workers, data.snapshot.clone())
    };
    for (identity, item) in workers {
        CORE_NETWORK_RUNTIME.spawn(run_favorite_worker(
            client.clone(),
            generation,
            identity,
            item,
        ));
    }
    publish_background_event(MediaSessionEvent::UserDataChanged);
    Ok(snapshot)
}

async fn run_favorite_worker(
    client: Arc<MusicAssistantClient>,
    generation: u64,
    identity: String,
    mut item: MediaItem,
) {
    loop {
        let desired = {
            let Ok(coordinator) = CORE_COORDINATOR.lock() else {
                return;
            };
            if coordinator.user_data_session_generation != generation {
                return;
            }
            coordinator
                .user_data
                .as_ref()
                .and_then(|data| data.sync.desired_state(&identity))
        };
        let Some(is_favorite) = desired else {
            return;
        };
        let result = client.set_favorite(&item, is_favorite).await;
        let confirmed = client.resolve_library_item(&item).await;
        let error = result.err().or_else(|| confirmed.as_ref().err().cloned());
        let confirmed = confirmed.ok().flatten();
        if let Some(canonical) = &confirmed {
            item = canonical.clone();
        }
        let is_finished = {
            let Ok(mut coordinator) = CORE_COORDINATOR.lock() else {
                return;
            };
            if coordinator.user_data_session_generation != generation {
                return;
            }
            let Some(data) = coordinator.user_data.as_mut() else {
                return;
            };
            data.finish(&identity, is_favorite, confirmed)
        };
        publish_background_event(MediaSessionEvent::UserDataChanged);
        if let Some(error) = error {
            crate::ffi::errors::record_and_publish_error(CoreError::command_failed(format!(
                "favorite synchronization failed: {error}"
            )));
        }
        if is_finished {
            return;
        }
    }
}

// Private playlist commands use the server's ownership checks and refresh confirmed state.
pub(crate) fn create_private_playlist(name: String) -> Result<UserDataSnapshot, CoreError> {
    if name.trim().is_empty() {
        return Err(CoreError::command_failed("playlist name is required"));
    }
    let (client, generation) = session_client()?;
    CORE_NETWORK_RUNTIME
        .block_on(async {
            let user = client.get_current_user().await?;
            client
                .create_private_playlist(name.trim(), user.user_id.as_str())
                .await
        })
        .map_err(CoreError::command_failed)?;
    refresh_for_session(generation)
}

pub(crate) fn rename_private_playlist(
    playlist_id: &str,
    name: String,
) -> Result<UserDataSnapshot, CoreError> {
    if name.trim().is_empty() {
        return Err(CoreError::command_failed("playlist name is required"));
    }
    let (client, generation) = session_client()?;
    CORE_NETWORK_RUNTIME
        .block_on(async {
            let playlist = client.get_private_playlist(playlist_id).await?;
            client.rename_private_playlist(&playlist, name.trim()).await
        })
        .map_err(CoreError::command_failed)?;
    refresh_for_session(generation)
}

pub(crate) fn remove_private_playlist(playlist_id: &str) -> Result<UserDataSnapshot, CoreError> {
    let (client, generation) = session_client()?;
    CORE_NETWORK_RUNTIME
        .block_on(async {
            client.get_private_playlist(playlist_id).await?;
            client.delete_private_playlist(playlist_id).await
        })
        .map_err(CoreError::command_failed)?;
    refresh_for_session(generation)
}

pub(crate) fn refresh_private_playlist_cover(
    playlist_id: &str,
) -> Result<UserDataSnapshot, CoreError> {
    CORE_NETWORK_RUNTIME.block_on(refresh_user_data_for_session(
        None,
        UserDataRefresh::PlaylistCover(playlist_id.to_owned()),
    ))
}

pub(crate) fn add_private_playlist_tracks(
    playlist_id: &str,
    tracks: Vec<Track>,
) -> Result<UserDataSnapshot, CoreError> {
    let (client, generation) = session_client()?;
    CORE_NETWORK_RUNTIME
        .block_on(async {
            client.get_private_playlist(playlist_id).await?;
            let uris = tracks
                .iter()
                .map(|track| {
                    track
                        .media
                        .uri
                        .as_ref()
                        .map(|uri| uri.as_str().to_owned())
                        .ok_or_else(|| "playlist track URI is required".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            client.add_playlist_tracks(playlist_id, uris).await
        })
        .map_err(CoreError::command_failed)?;
    CORE_NETWORK_RUNTIME.block_on(refresh_user_data_for_session(
        Some(generation),
        UserDataRefresh::Playlist(playlist_id.to_owned()),
    ))
}

pub(crate) fn remove_private_playlist_tracks(
    playlist_id: &str,
    uris: Vec<String>,
) -> Result<UserDataSnapshot, CoreError> {
    let (client, generation) = session_client()?;
    CORE_NETWORK_RUNTIME
        .block_on(async {
            client.get_private_playlist(playlist_id).await?;
            let tracks = client
                .get_playlist_tracks(&MediaId(playlist_id.into()), "library", true, false)
                .await?;
            let positions = tracks
                .iter()
                .filter(|track| {
                    track
                        .media
                        .uri
                        .as_ref()
                        .is_some_and(|uri| uris.iter().any(|target| target == uri.as_str()))
                })
                .map(|track| {
                    track
                        .media
                        .position
                        .ok_or_else(|| "MA did not return a playlist position".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            client.remove_playlist_tracks(playlist_id, positions).await
        })
        .map_err(CoreError::command_failed)?;
    CORE_NETWORK_RUNTIME.block_on(refresh_user_data_for_session(
        Some(generation),
        UserDataRefresh::Playlist(playlist_id.to_owned()),
    ))
}

// Initial snapshots retain favorite workers and reject completions from replaced sessions.
fn publish_initial_user_data(
    generation: u64,
    update: impl FnOnce(&mut LibraryUserData),
) -> Result<(), String> {
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| "core coordinator lock poisoned".to_string())?;
    coordinator.update_initial_user_data(generation, update)?;
    drop(coordinator);
    publish_background_event(MediaSessionEvent::UserDataChanged);
    Ok(())
}

// Session guards protect all network completions from logout and account replacement.
fn session_client() -> Result<(Arc<MusicAssistantClient>, u64), CoreError> {
    let coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let client = coordinator
        .connection
        .as_ref()
        .ok_or_else(|| CoreError::command_failed("a restored session is required"))?
        .client
        .clone();
    Ok((client, coordinator.user_data_session_generation))
}

fn refresh_for_session(generation: u64) -> Result<UserDataSnapshot, CoreError> {
    CORE_NETWORK_RUNTIME.block_on(refresh_user_data_for_session(
        Some(generation),
        UserDataRefresh::Playlists,
    ))
}

// The UserData lease stays active independently of playback and ends with its session.
fn start_user_data_events() -> Result<(), CoreError> {
    let mut coordinator = CORE_COORDINATOR
        .lock()
        .map_err(|_| CoreError::internal("core coordinator lock poisoned"))?;
    let connection = coordinator
        .connection
        .as_ref()
        .ok_or_else(|| CoreError::command_failed("session ended while starting user data events"))?
        .clone();
    let generation = coordinator.user_data_session_generation;
    let hub = connection.event_hub.clone();
    let _runtime_guard = CORE_NETWORK_RUNTIME.enter();
    let mut receiver = {
        let mut hub = hub
            .lock()
            .map_err(|_| CoreError::internal("event hub lock poisoned"))?;
        let receiver = hub.subscribe();
        hub.acquire(EventHubLeaseReason::UserData);
        receiver
    };
    let worker = CORE_NETWORK_RUNTIME.spawn(async move {
        loop {
            let refresh = match receiver.recv().await {
                Ok(MusicAssistantEvent::Connected)
                | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => UserDataRefresh::All,
                Ok(MusicAssistantEvent::PlaylistsChanged) => UserDataRefresh::Playlists,
                Ok(MusicAssistantEvent::PlaylistChanged(playlist_id)) => {
                    UserDataRefresh::Playlist(playlist_id.0)
                }
                Ok(MusicAssistantEvent::FavoritesChanged) => UserDataRefresh::Favorites,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                _ => continue,
            };
            if let Err(error) = refresh_user_data_for_session(Some(generation), refresh).await {
                crate::ffi::errors::record_and_publish_error(error);
            }
        }
    });
    coordinator.user_data_events = Some(UserDataEvents { hub, worker });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn user_data_refresh_waits_for_an_inflight_load() {
        let guard = CORE_USER_DATA_LOAD.lock().await;
        let mut refresh = Box::pin(refresh_user_data_for_session(None, UserDataRefresh::All));
        assert!(timeout(Duration::from_millis(20), refresh.as_mut())
            .await
            .is_err());
        drop(guard);
        assert!(timeout(Duration::from_secs(1), refresh)
            .await
            .unwrap()
            .is_err());
    }
}
