use super::models::identifiers::{AccessToken, MediaId, PlayerId, QueueId, QueueItemId, UserId};
use super::models::media::{
    Album, Artist, MediaItem, MediaItemFields, MediaItemImage, MediaType,
    MusicAssistantSearchResults, Playlist, PlaylistSharing, Track,
};
use super::models::player::MusicAssistantPlayer;
use super::models::queue::{MusicAssistantQueue, QueueItem, QueueOption, RepeatMode};
use super::models::recommendation::{
    MusicAssistantRecommendationFolder, RecommendationSnapshot, RecommendationView,
};
use super::models::session::{MusicAssistantUser, ProviderInstance, ProviderManifest, ServerInfo};
use crate::logging::Logger;
use rand::seq::SliceRandom;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

type CachedPlaylistTracks = Arc<tokio::sync::Mutex<Option<Vec<Track>>>>;
static LOGGER: Logger = Logger::new(module_path!());

#[derive(Default)]
struct RecommendationCache {
    folders: Vec<MusicAssistantRecommendationFolder>,
    completed_views: HashSet<RecommendationView>,
}

struct ArtworkCacheEntry {
    expires_at: Instant,
    image: Option<MediaItemImage>,
}

#[derive(Deserialize)]
struct PlaylistTask {
    id: String,
    status: String,
    #[serde(default)]
    last_error: Option<String>,
}

pub(crate) enum PlayerCommand {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Seek(f64),
    Volume(u8),
    Mute(bool),
}

pub(crate) enum QueueCommand {
    Clear,
    PlayMedia {
        media: Vec<MediaItem>,
        option: QueueOption,
        start_item: Option<usize>,
        shuffle: bool,
    },
    PlayIndex(usize),
    PlayItem(QueueItemId),
    MoveItem {
        item_id: QueueItemId,
        shift: i64,
    },
    DeleteItem(QueueItemId),
    MoveItemToEnd(QueueItemId),
    Shuffle(bool),
    Repeat(RepeatMode),
    Autoplay(bool),
}

struct ProviderAvailabilitySnapshot {
    endpoint: String,
    access_token: Option<AccessToken>,
    expires_at: Instant,
    providers: Option<Vec<ProviderInstance>>,
}

static PROVIDER_AVAILABILITY: once_cell::sync::Lazy<
    tokio::sync::Mutex<Vec<ProviderAvailabilitySnapshot>>,
> = once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(Vec::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpCommandError {
    Http {
        command: String,
        status: u16,
        details: String,
    },
    Transport {
        command: String,
        is_timeout: bool,
        is_connect: bool,
        details: String,
    },
    InvalidResponse {
        command: String,
        details: String,
    },
}

impl HttpCommandError {
    pub(crate) fn is_track_staging_failure(&self) -> bool {
        matches!(self, Self::Http { command, status: 500, .. } if command == "player_queues/play_media")
    }
}

impl std::fmt::Display for HttpCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http {
                command,
                status,
                details,
            } => write!(
                formatter,
                "Music Assistant command '{command}' returned HTTP {status}: {details}"
            ),
            Self::Transport {
                command, details, ..
            } => write!(
                formatter,
                "Music Assistant command '{command}' transport failed: {details}"
            ),
            Self::InvalidResponse { command, details } => write!(
                formatter,
                "Music Assistant command '{command}' returned invalid JSON: {details}"
            ),
        }
    }
}

impl From<HttpCommandError> for String {
    fn from(error: HttpCommandError) -> Self {
        error.to_string()
    }
}

pub(crate) struct MusicAssistantClient {
    base_url: String,
    endpoint: String,
    access_token: Option<AccessToken>,
    http_client: reqwest::Client,
    current_user_id: tokio::sync::OnceCell<UserId>,
    artwork_cache: tokio::sync::Mutex<HashMap<(String, String, String), ArtworkCacheEntry>>,
    recommendations: tokio::sync::Mutex<RecommendationCache>,
    recommendations_load: tokio::sync::OnceCell<Result<(), String>>,
    recommendation_playlist_tracks:
        tokio::sync::Mutex<HashMap<(String, String), CachedPlaylistTracks>>,
}

impl MusicAssistantClient {
    // Construction.
    pub(crate) fn new(base_url: &str, access_token: Option<AccessToken>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            endpoint: format!("{}/api", base_url.trim_end_matches('/')),
            access_token,
            current_user_id: tokio::sync::OnceCell::new(),
            artwork_cache: tokio::sync::Mutex::new(HashMap::new()),
            recommendations: tokio::sync::Mutex::new(RecommendationCache::default()),
            recommendations_load: tokio::sync::OnceCell::new(),
            recommendation_playlist_tracks: tokio::sync::Mutex::new(HashMap::new()),
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("create Music Assistant HTTP client"),
        }
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    // Session, server, and provider queries.
    pub(crate) async fn login(&self, username: &str, password: &str) -> Result<Value, String> {
        self.execute_command(
            "auth/login",
            json!({
                "username": username,
                "password": password,
                "provider_id": "builtin",
            }),
        )
        .await
    }

    pub(crate) async fn logout(&self) -> Result<(), String> {
        self.execute_command::<Value>("auth/logout", json!({}))
            .await
            .map(|_| ())
    }

    pub(crate) async fn get_server_info(&self) -> Result<ServerInfo, String> {
        self.execute_command("info", json!({})).await
    }

    pub(crate) async fn has_valid_session(&self) -> bool {
        self.get_server_info().await.is_ok()
    }

    pub(crate) async fn get_current_user(&self) -> Result<MusicAssistantUser, String> {
        self.execute_command("auth/me", json!({})).await
    }

    pub(crate) fn initialize_current_user_id(&self, user_id: UserId) -> Result<(), String> {
        if user_id.as_str().trim().is_empty() {
            return Err("Music Assistant user ID is empty".into());
        }
        if self.current_user_id.get() == Some(&user_id) {
            return Ok(());
        }
        self.current_user_id
            .set(user_id)
            .map_err(|_| "Music Assistant session user ID cannot change".into())
    }

    pub(crate) async fn get_current_user_id(&self) -> Result<UserId, String> {
        self.current_user_id
            .get_or_try_init(|| async {
                let user = self.get_current_user().await?;
                if user.user_id.as_str().trim().is_empty() {
                    return Err("Music Assistant user ID is empty".to_owned());
                }
                Ok(user.user_id)
            })
            .await
            .cloned()
    }

    pub(crate) async fn get_provider_manifests(&self) -> Result<Vec<ProviderManifest>, String> {
        self.execute_command("providers/manifests", json!({})).await
    }

    pub(crate) async fn get_provider_icon_data(
        &self,
        provider: &str,
    ) -> Result<Option<String>, String> {
        let icon_data: Value = self
            .execute_command(
                "providers/icon",
                json!({ "provider": provider, "variant": "default" }),
            )
            .await?;
        Ok(icon_data
            .as_str()
            .map(str::trim)
            .filter(|icon_data| !icon_data.is_empty())
            .map(str::to_owned))
    }

    // Music library queries.
    pub(crate) async fn get_artist(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Artist, String> {
        self.execute_command(
            "music/artists/get",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_album(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Album, String> {
        self.execute_command(
            "music/albums/get",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_track(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Track, String> {
        self.execute_command(
            "music/tracks/get",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_playlist(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Playlist, String> {
        self.execute_command(
            "music/playlists/get",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_library_artists(
        &self,
        arguments: Value,
    ) -> Result<Vec<Artist>, String> {
        self.execute_command("music/artists/library_items", arguments)
            .await
    }

    pub(crate) async fn get_library_albums(&self, arguments: Value) -> Result<Vec<Album>, String> {
        self.execute_command("music/albums/library_items", arguments)
            .await
    }

    pub(crate) async fn get_library_tracks(&self, arguments: Value) -> Result<Vec<Track>, String> {
        self.execute_command("music/tracks/library_items", arguments)
            .await
    }

    pub(crate) async fn get_library_playlists(
        &self,
        arguments: Value,
    ) -> Result<Vec<Playlist>, String> {
        self.execute_command("music/playlists/library_items", arguments)
            .await
    }

    pub(crate) async fn get_artist_albums(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Vec<Album>, String> {
        self.execute_command(
            "music/artists/artist_albums",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_track_versions(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Vec<Track>, String> {
        self.execute_command(
            "music/tracks/track_versions",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_similar_tracks(
        &self,
        item_id: &MediaId,
        provider: &str,
        arguments: Value,
    ) -> Result<Vec<Track>, String> {
        self.execute_command(
            "music/tracks/similar_tracks",
            merge_item_reference_arguments(item_id, provider, arguments),
        )
        .await
    }

    pub(crate) async fn get_similar_artists(
        &self,
        item_id: &MediaId,
        provider: &str,
        arguments: Value,
    ) -> Result<Vec<Artist>, String> {
        self.execute_command(
            "music/artists/similar_artists",
            merge_item_reference_arguments(item_id, provider, arguments),
        )
        .await
    }

    pub(crate) async fn get_playlist_tracks(
        &self,
        item_id: &MediaId,
        provider: &str,
        force_refresh: bool,
        allow_dynamic_tracks: bool,
    ) -> Result<Vec<Track>, String> {
        let cached_tracks = if force_refresh || allow_dynamic_tracks {
            None
        } else {
            self.recommendation_playlist_tracks
                .lock()
                .await
                .get(&(provider.to_owned(), item_id.as_str().to_owned()))
                .cloned()
        };
        let mut snapshot = match cached_tracks.as_ref() {
            Some(tracks) => Some(tracks.lock().await),
            None => None,
        };
        if let Some(Some(tracks)) = snapshot.as_deref() {
            return Ok(tracks.clone());
        }
        let tracks: Vec<Track> = self
            .execute_command(
                "music/playlists/playlist_tracks",
                json!({
                    "item_id": item_id,
                    "provider_instance_id_or_domain": provider,
                    "force_refresh": force_refresh,
                    "allow_dynamic_tracks": allow_dynamic_tracks,
                }),
            )
            .await?;
        if let Some(snapshot) = snapshot.as_deref_mut() {
            *snapshot = Some(tracks.clone());
        }
        Ok(tracks)
    }

    pub(crate) async fn get_album_tracks(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Vec<Track>, String> {
        self.execute_command(
            "music/albums/album_tracks",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_artist_top_tracks(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Vec<Track>, String> {
        self.execute_command(
            "music/artists/top_tracks",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_artist_tracks(
        &self,
        item_id: &MediaId,
        provider: &str,
    ) -> Result<Vec<Track>, String> {
        self.execute_command(
            "music/artists/artist_tracks",
            item_reference_arguments(item_id, provider),
        )
        .await
    }

    pub(crate) async fn get_recently_played_tracks(
        &self,
        limit: u32,
    ) -> Result<Vec<Track>, String> {
        self.execute_command(
            "music/recently_played_items",
            json!({ "limit": limit, "media_types": ["track"] }),
        )
        .await
    }

    pub(crate) async fn get_recommendations(
        &self,
        view: RecommendationView,
    ) -> Result<RecommendationSnapshot, String> {
        let cache = self.recommendations.lock().await;
        Ok(RecommendationSnapshot {
            folders: cache
                .folders
                .iter()
                .filter(|folder| view.includes(folder.item_id.as_str()))
                .cloned()
                .collect(),
            is_loading: !cache.completed_views.contains(&view),
        })
    }

    // The session owns this one-shot load; readers never perform network work.
    pub(crate) async fn load_recommendations(
        &self,
        mut on_update: impl FnMut(),
    ) -> Result<(), String> {
        self.recommendations_load
            .get_or_init(|| async {
                let started = Instant::now();
                LOGGER.debug("Initial recommendations load started");
                let result = self.load_recommendation_folders(&mut on_update).await;
                LOGGER.info(format_args!(
                    "Initial recommendations load finished: elapsed_ms={} success={}",
                    started.elapsed().as_millis(),
                    result.is_ok()
                ));
                if result.is_err() {
                    let mut cache = self.recommendations.lock().await;
                    cache
                        .completed_views
                        .extend([RecommendationView::Home, RecommendationView::Explore]);
                }
                on_update();
                result
            })
            .await
            .clone()
    }

    async fn load_recommendation_folders(
        &self,
        on_update: &mut impl FnMut(),
    ) -> Result<(), String> {
        let started = Instant::now();
        LOGGER.trace("Initial recommendation folders load started");
        let result = self
            .execute_command::<Vec<MusicAssistantRecommendationFolder>>(
                "music/recommendations",
                json!({}),
            )
            .await;
        LOGGER.trace(format_args!(
            "Initial recommendation folders load finished: elapsed_ms={} success={}",
            started.elapsed().as_millis(),
            result.is_ok()
        ));
        let mut folders = result?;
        for folder in &mut folders {
            folder.is_loading = folder.items.is_empty()
                || !recommendation_playlists(std::slice::from_ref(folder)).is_empty();
        }
        self.recommendations.lock().await.folders = folders.clone();
        on_update();
        for view in [RecommendationView::Home, RecommendationView::Explore] {
            for (index, folder) in folders
                .iter_mut()
                .enumerate()
                .filter(|(_, folder)| view.includes(folder.item_id.as_str()))
            {
                if folder.items.is_empty() {
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
                        Ok(items) => folder.items = items,
                        Err(error) => {
                            LOGGER.warn(format!("load recommendation folder failed: {error}"))
                        }
                    }
                }
                for playlist in recommendation_playlists(std::slice::from_ref(folder)) {
                    self.recommendation_playlist_tracks
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
                            LOGGER.warn(format!("load recommendation playlist failed: {error}"))
                        }
                    }
                }
                folder.is_loading = false;
                self.recommendations.lock().await.folders[index] = folder.clone();
                on_update();
            }
            self.recommendations
                .lock()
                .await
                .completed_views
                .insert(view);
            on_update();
        }
        Ok(())
    }

    pub(crate) async fn search_media(
        &self,
        search_query: &str,
        media_types: &[MediaType],
        limit: u32,
    ) -> Result<MusicAssistantSearchResults, String> {
        self.execute_command(
            "music/search",
            json!({
                "search_query": search_query,
                "media_types": media_types,
                "limit": limit,
                "library_only": false,
            }),
        )
        .await
    }

    // Playlist mutations.
    // Favorites and canonical library identities.
    pub(crate) async fn get_library_page<Item: DeserializeOwned>(
        &self,
        media_type: &str,
        favorite: Option<bool>,
        limit: usize,
        offset: usize,
        summary: bool,
    ) -> Result<Vec<Item>, String> {
        self.execute_command(
            &format!("music/{media_type}/library_items"),
            json!({
                "favorite": favorite, "limit": limit, "offset": offset,
                "summary": summary,
                "sort_field": "sort_name", "sort_direction": "asc",
            }),
        )
        .await
    }

    pub(crate) async fn set_favorite(
        &self,
        item: &MediaItem,
        is_favorite: bool,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "music/favorites/set_item",
            json!({"item": item, "favorite": if is_favorite { Some(true) } else { None }}),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn resolve_library_item(
        &self,
        item: &MediaItem,
    ) -> Result<Option<MediaItem>, String> {
        let media_type =
            serde_json::to_value(item).map_err(|error| error.to_string())?["media_type"].clone();
        if item.fields().provider.as_str() == "library" {
            return self
                .execute_command(
                    "music/item",
                    json!({
                        "media_type": media_type, "item_id": item.fields().item_id,
                        "provider_instance_id_or_domain": "library", "allow_update_metadata": false,
                    }),
                )
                .await
                .map(Some);
        }
        self.execute_command(
            "music/get_library_item",
            json!({
                "media_type": media_type,
                "item_id": item.fields().item_id,
                "provider_instance_id_or_domain": item.fields().provider,
            }),
        )
        .await
    }

    // Playlist commands.
    pub(crate) async fn create_private_playlist(
        &self,
        name: &str,
        user_id: &str,
    ) -> Result<Playlist, String> {
        let created: Playlist = self
            .execute_command("music/playlists/create_playlist", json!({"name": name}))
            .await?;
        self.execute_command(
            "music/playlists/set_access",
            json!({
                "item_id": created.media.item_id, "sharing": "private", "owner": user_id,
                "shared_users": [], "collaborative": false,
            }),
        )
        .await
    }

    pub(crate) async fn rename_private_playlist(
        &self,
        playlist: &Playlist,
        name: &str,
    ) -> Result<(), String> {
        let mut update = serde_json::to_value(MediaItem::Playlist(playlist.clone()))
            .map_err(|error| error.to_string())?;
        update["name"] = json!(name);
        update["sort_name"] = json!(name);
        self.execute_command::<Value>(
            "music/playlists/update",
            json!({
                "item_id": playlist.media.item_id, "update": update, "overwrite": true,
            }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn set_private_playlist_image(
        &self,
        playlist: &Playlist,
        image: Option<String>,
    ) -> Result<Playlist, String> {
        let current = playlist.media.metadata.as_ref().and_then(|metadata| {
            metadata
                .images
                .iter()
                .find(|image| image.image_type == "thumb")
        });
        if current.map(|image| image.path.as_str()) == image.as_deref() {
            return Ok(playlist.clone());
        }
        let mut update: Value = self.execute_command(
            "music/playlists/get",
            json!({"item_id":playlist.media.item_id, "provider_instance_id_or_domain":"library", "allow_update_metadata":false}),
        ).await?;
        let metadata = update
            .get_mut("metadata")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "MA returned playlist metadata without an object".to_string())?;
        let mut images = match metadata.remove("images") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(images)) => images,
            _ => return Err("MA returned invalid playlist images".into()),
        };
        images.retain(|image| image["type"] != "thumb");
        if let Some(path) = image {
            images.insert(
                0,
                json!(super::models::media::MediaItemImage {
                    image_type: "thumb".into(),
                    path,
                    provider: super::models::identifiers::ProviderInstanceId("builtin".into()),
                    remotely_accessible: false,
                    proxy_id: None,
                }),
            );
        }
        metadata.insert("images".into(), json!(images));
        self.execute_command(
            "music/playlists/update",
            json!({
                "item_id": playlist.media.item_id,
                "update": update, "overwrite": true,
            }),
        )
        .await
    }

    pub(crate) async fn delete_private_playlist(&self, playlist_id: &str) -> Result<(), String> {
        self.execute_command::<Value>(
            "music/library/remove_item",
            json!({"media_type": "playlist", "library_item_id": playlist_id}),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn get_private_playlist(&self, playlist_id: &str) -> Result<Playlist, String> {
        let user_id = self.get_current_user_id().await?;
        let owned: Playlist = self.execute_command("music/playlists/get", json!({
            "item_id": playlist_id, "provider_instance_id_or_domain": "library", "allow_update_metadata": false,
        })).await?;
        if !owned.access.as_ref().is_some_and(|access| {
            access.owner.as_deref() == Some(user_id.as_str())
                && access.sharing == PlaylistSharing::Private
        }) {
            return Err("playlist is not a private playlist owned by the current user".into());
        }
        Ok(owned)
    }

    pub(crate) async fn add_playlist_tracks(
        &self,
        playlist_id: &str,
        uris: Vec<String>,
    ) -> Result<(), String> {
        let task = self
            .execute_command::<PlaylistTask>(
                "music/playlists/add_playlist_tracks",
                json!({ "db_playlist_id": playlist_id, "uris": uris }),
            )
            .await?;
        self.wait_for_playlist_task(task).await
    }

    pub(crate) async fn remove_playlist_tracks(
        &self,
        playlist_id: &str,
        positions_to_remove: Vec<u32>,
    ) -> Result<(), String> {
        let task = self
            .execute_command::<PlaylistTask>(
                "music/playlists/remove_playlist_tracks",
                json!({
                    "db_playlist_id": playlist_id,
                    "positions_to_remove": positions_to_remove,
                }),
            )
            .await?;
        self.wait_for_playlist_task(task).await
    }

    // Playback and queue commands.
    pub(crate) async fn execute_player_command(
        &self,
        player_id: &PlayerId,
        command: PlayerCommand,
    ) -> Result<(), String> {
        let (name, arguments) = match command {
            PlayerCommand::Play => ("play", json!({"player_id":player_id})),
            PlayerCommand::Pause => ("pause", json!({"player_id":player_id})),
            PlayerCommand::Stop => ("stop", json!({"player_id":player_id})),
            PlayerCommand::Next => ("next", json!({"player_id":player_id})),
            PlayerCommand::Previous => ("previous", json!({"player_id":player_id})),
            PlayerCommand::Seek(position) => {
                if !position.is_finite() || position < 0.0 {
                    return Err("seek position must be finite and nonnegative".into());
                }
                (
                    "seek",
                    json!({"player_id":player_id,"position":position.round() as i64}),
                )
            }
            PlayerCommand::Volume(volume) => (
                "volume_set",
                json!({"player_id":player_id,"volume_level":volume.min(100)}),
            ),
            PlayerCommand::Mute(muted) => {
                ("volume_mute", json!({"player_id":player_id,"muted":muted}))
            }
        };
        self.execute_command::<Value>(&format!("players/cmd/{name}"), arguments)
            .await
            .map(|_| ())
    }

    pub(crate) async fn execute_queue_command(
        &self,
        queue_id: &QueueId,
        command: QueueCommand,
    ) -> Result<(), String> {
        let (name, arguments) = queue_command_request(queue_id, command)?;
        self.execute_command::<Value>(&format!("player_queues/{name}"), arguments)
            .await
            .map(|_| ())
    }

    // Remote player and queue queries.
    pub(crate) async fn get_player(
        &self,
        player_id: &PlayerId,
    ) -> Result<MusicAssistantPlayer, String> {
        self.execute_command("players/get", json!({"player_id": player_id}))
            .await
    }

    pub(crate) async fn get_active_queue(
        &self,
        player_id: &PlayerId,
    ) -> Result<Option<MusicAssistantQueue>, String> {
        self.execute_command(
            "player_queues/get_active_queue",
            json!({"player_id": player_id}),
        )
        .await
    }

    pub(crate) async fn get_players(
        &self,
        return_unavailable: bool,
        return_protocol_players: bool,
    ) -> Result<Vec<MusicAssistantPlayer>, String> {
        self.execute_command(
            "players/all",
            json!({
                "return_unavailable": return_unavailable,
                "return_protocol_players": return_protocol_players,
            }),
        )
        .await
    }

    pub(crate) async fn get_player_queues(&self) -> Result<Vec<MusicAssistantQueue>, String> {
        self.execute_command("player_queues/all", json!({})).await
    }

    pub(crate) async fn get_queue_items(
        &self,
        queue_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<QueueItem>, String> {
        self.execute_command(
            "player_queues/items",
            json!({ "queue_id": queue_id, "limit": limit, "offset": offset }),
        )
        .await
    }

    // Accepted playlist operations must reach a terminal task state.
    async fn wait_for_playlist_task(&self, mut task: PlaylistTask) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                match task.status.as_str() {
                    "success" => return Ok(()),
                    "failed" | "cancelled" => {
                        return Err(task
                            .last_error
                            .unwrap_or_else(|| "playlist operation failed".into()));
                    }
                    _ => {}
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
                task = self
                    .execute_command("tasks/get", json!({"task_id": task.id}))
                    .await?;
            }
        })
        .await
        .map_err(|_| "playlist update confirmation timed out".to_string())?
    }

    // HTTP command transport.
    async fn provider_availability(&self) -> Option<Vec<ProviderInstance>> {
        let mut snapshots = PROVIDER_AVAILABILITY.lock().await;
        snapshots.retain(|snapshot| snapshot.expires_at > Instant::now());
        if let Some(snapshot) = snapshots.iter().find(|snapshot| {
            snapshot.endpoint == self.endpoint && snapshot.access_token == self.access_token
        }) {
            return snapshot.providers.clone();
        }
        drop(snapshots);
        let providers = tokio::time::timeout(
            Duration::from_secs(5),
            self.execute_http_command::<Vec<ProviderInstance>>("providers", json!({})),
        )
        .await
        .ok()
        .and_then(Result::ok);
        let mut snapshots = PROVIDER_AVAILABILITY.lock().await;
        snapshots.retain(|snapshot| {
            snapshot.endpoint != self.endpoint || snapshot.access_token != self.access_token
        });
        snapshots.push(ProviderAvailabilitySnapshot {
            endpoint: self.endpoint.clone(),
            access_token: self.access_token.clone(),
            expires_at: Instant::now()
                + Duration::from_secs(if providers.is_some() { 30 } else { 5 }),
            providers: providers.clone(),
        });
        providers
    }

    async fn execute_command<Response>(
        &self,
        command: &str,
        arguments: Value,
    ) -> Result<Response, String>
    where
        Response: DeserializeOwned,
    {
        let mut response: Value = self
            .execute_http_command(command, arguments)
            .await
            .map_err(String::from)?;
        if command.starts_with("music/") {
            self.fill_missing_artwork(&mut response).await;
            let providers = self.provider_availability().await;
            annotate_media_availability(&mut response, providers.as_deref());
        }
        serde_json::from_value(response).map_err(|error| {
            format!("Music Assistant command '{command}' returned invalid data: {error}")
        })
    }

    // Artwork enrichment stays at the response boundary and preserves item identity and metadata.
    async fn fill_missing_artwork(&self, response: &mut Value) {
        let mut pending = vec![response];
        while let Some(value) = pending.pop() {
            let media_type = value
                .get("media_type")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(media_type) = media_type {
                if let Ok(fields) = serde_json::from_value::<MediaItemFields>(value.clone()) {
                    let preview = value
                        .get("image")
                        .cloned()
                        .and_then(|image| serde_json::from_value::<MediaItemImage>(image).ok());
                    let has_image = fields
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.images.iter().any(usable_artwork))
                        || preview.as_ref().is_some_and(usable_artwork);
                    if !has_image {
                        if let Some(image) = self.find_provider_artwork(&media_type, &fields).await
                        {
                            let metadata = value
                                .as_object_mut()
                                .unwrap()
                                .entry("metadata")
                                .or_insert_with(|| json!({}));
                            if !metadata.is_object() {
                                *metadata = json!({});
                            }
                            metadata["images"] = json!([image]);
                        }
                    }
                }
            }
            match value {
                Value::Array(items) => pending.extend(items.iter_mut()),
                Value::Object(fields) => pending.extend(fields.values_mut()),
                _ => {}
            }
        }
    }

    // Raw item requests cannot re-enter enrichment; misses and successful lookups share a session cache.
    async fn find_provider_artwork(
        &self,
        media_type: &str,
        fields: &MediaItemFields,
    ) -> Option<MediaItemImage> {
        for mapping in &fields.provider_mappings {
            let provider = mapping.provider_instance.as_str();
            if provider.is_empty()
                || provider == "library"
                || mapping.item_id.as_str().is_empty()
                || (provider == fields.provider.as_str() && mapping.item_id == fields.item_id)
            {
                continue;
            }
            let key = (
                media_type.to_owned(),
                provider.to_owned(),
                mapping.item_id.as_str().to_owned(),
            );
            {
                let cache = self.artwork_cache.lock().await;
                if let Some(entry) = cache
                    .get(&key)
                    .filter(|entry| entry.expires_at > Instant::now())
                {
                    if entry.image.is_some() {
                        return entry.image.clone();
                    }
                    continue;
                }
            }
            let result = self
                .execute_http_command::<MediaItem>(
                    "music/item",
                    json!({
                        "media_type": media_type,
                        "item_id": mapping.item_id,
                        "provider_instance_id_or_domain": provider,
                        "allow_update_metadata": false,
                    }),
                )
                .await;
            let image = result.ok().and_then(|item| {
                item.fields()
                    .metadata
                    .as_ref()
                    .and_then(|metadata| {
                        metadata
                            .images
                            .iter()
                            .find(|image| usable_artwork(image))
                            .cloned()
                    })
                    .or_else(|| match item {
                        MediaItem::Track(track) => track.image.filter(usable_artwork),
                        _ => None,
                    })
            });
            let mut cache = self.artwork_cache.lock().await;
            cache.retain(|_, entry| entry.expires_at > Instant::now());
            cache.insert(
                key,
                ArtworkCacheEntry {
                    expires_at: Instant::now() + Duration::from_secs(300),
                    image: image.clone(),
                },
            );
            if image.is_some() {
                return image;
            }
        }
        None
    }

    async fn execute_http_command<Response>(
        &self,
        command: &str,
        arguments: Value,
    ) -> Result<Response, HttpCommandError>
    where
        Response: DeserializeOwned,
    {
        let mut request = self.http_client.post(&self.endpoint).json(&json!({
            "command": command,
            "args": arguments,
        }));
        if let Some(access_token) = self
            .access_token
            .as_ref()
            .filter(|token| !token.as_str().trim().is_empty())
        {
            request = request.bearer_auth(access_token.as_str());
        }
        let response = request
            .send()
            .await
            .map_err(|error| HttpCommandError::Transport {
                command: command.to_string(),
                is_timeout: error.is_timeout(),
                is_connect: error.is_connect(),
                details: error.to_string(),
            })?;
        let status = response.status();
        let response_body = response
            .text()
            .await
            .map_err(|error| HttpCommandError::Transport {
                command: command.to_string(),
                is_timeout: error.is_timeout(),
                is_connect: error.is_connect(),
                details: error.to_string(),
            })?;
        if !status.is_success() {
            let response_excerpt = response_body.chars().take(512).collect::<String>();
            return Err(HttpCommandError::Http {
                command: command.to_string(),
                status: status.as_u16(),
                details: response_excerpt,
            });
        }
        serde_json::from_str(&response_body).map_err(|error| {
            let response_excerpt = response_body.chars().take(512).collect::<String>();
            HttpCommandError::InvalidResponse {
                command: command.to_string(),
                details: format!("{error}; response: {response_excerpt}"),
            }
        })
    }
}

fn usable_artwork(image: &MediaItemImage) -> bool {
    !image.path.trim().is_empty()
        || image
            .proxy_id
            .as_ref()
            .is_some_and(|proxy| !proxy.trim().is_empty())
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

// Transport always sends modeled media objects, including nested media type tags.
pub(crate) fn queue_media_payload(media_item: MediaItem) -> Value {
    let mut payload = json!(&media_item);
    match media_item {
        MediaItem::Track(track) => {
            tag_artists(&mut payload["artists"]);
            if track.album.is_some() {
                tag_media_type(&mut payload["album"], "album");
                tag_artists(&mut payload["album"]["artists"]);
            }
        }
        MediaItem::Album(_) => tag_artists(&mut payload["artists"]),
        MediaItem::PodcastEpisode(_) => tag_media_type(&mut payload["podcast"], "podcast"),
        _ => {}
    }
    payload
}

// Serialization boundary: preserve provider metadata while adding the effective availability flag.
fn annotate_media_availability(value: &mut Value, providers: Option<&[ProviderInstance]>) {
    match value {
        Value::Array(items) => {
            for item in items {
                annotate_media_availability(item, providers);
            }
        }
        Value::Object(fields) => {
            if fields.contains_key("item_id") && fields.contains_key("provider") {
                if let Ok(media) =
                    serde_json::from_value::<MediaItemFields>(Value::Object(fields.clone()))
                {
                    let available = media.is_available()
                        && (media.provider_mappings.is_empty()
                            || media.provider_mappings.iter().any(|mapping| {
                                mapping.available
                                    && providers
                                        .map(|providers| {
                                            providers.iter().any(|provider| {
                                                provider.instance_id == mapping.provider_instance
                                                    && provider.available
                                            })
                                        })
                                        .unwrap_or(true)
                            }));
                    fields.insert("available".to_string(), json!(available));
                }
            }
            for field in fields.values_mut() {
                annotate_media_availability(field, providers);
            }
        }
        _ => {}
    }
}

fn tag_artists(artists: &mut Value) {
    if let Some(artists) = artists.as_array_mut() {
        for artist in artists {
            tag_media_type(artist, "artist");
        }
    }
}

fn tag_media_type(media_item: &mut Value, media_type: &str) {
    if let Some(fields) = media_item.as_object_mut() {
        fields.insert("media_type".to_string(), json!(media_type));
    }
}

fn item_reference_arguments(item_id: &MediaId, provider: &str) -> Value {
    json!({
        "item_id": item_id,
        "provider_instance_id_or_domain": provider,
    })
}

fn merge_item_reference_arguments(item_id: &MediaId, provider: &str, arguments: Value) -> Value {
    let mut merged = item_reference_arguments(item_id, provider);
    if let (Some(merged), Some(arguments)) = (merged.as_object_mut(), arguments.as_object()) {
        merged.extend(arguments.clone());
    }
    merged
}

fn queue_command_request(
    queue_id: &QueueId,
    command: QueueCommand,
) -> Result<(&'static str, Value), String> {
    let (name, arguments) = match command {
        QueueCommand::Clear => ("clear", json!({"queue_id": queue_id})),
        QueueCommand::PlayMedia {
            mut media,
            option,
            start_item,
            shuffle,
        } => {
            let start = start_item
                .map(|index| {
                    media
                        .get(index)
                        .cloned()
                        .ok_or_else(|| "start item is outside the media request".to_string())
                })
                .transpose()?
                .map(queue_media_payload);
            if shuffle {
                media.shuffle(&mut rand::rng());
            }
            let media = media
                .into_iter()
                .map(queue_media_payload)
                .collect::<Vec<_>>();
            (
                "play_media",
                json!({"queue_id":queue_id,"media":media,"option":option,"start_item":start,"sort_by":"original"}),
            )
        }
        QueueCommand::PlayIndex(index) => {
            ("play_index", json!({"queue_id":queue_id,"index":index}))
        }
        QueueCommand::PlayItem(item_id) => {
            ("play_index", json!({"queue_id":queue_id,"index":item_id}))
        }
        QueueCommand::MoveItem { item_id, shift } => (
            "move_item",
            json!({"queue_id":queue_id,"queue_item_id":item_id,"pos_shift":shift}),
        ),
        QueueCommand::DeleteItem(item_id) => (
            "delete_item",
            json!({"queue_id":queue_id,"item_id_or_index":item_id}),
        ),
        QueueCommand::MoveItemToEnd(item_id) => (
            "move_item_end",
            json!({"queue_id":queue_id,"queue_item_id":item_id}),
        ),
        QueueCommand::Shuffle(enabled) => (
            "shuffle",
            json!({"queue_id":queue_id,"shuffle_enabled":enabled}),
        ),
        QueueCommand::Repeat(mode) => ("repeat", json!({"queue_id":queue_id,"repeat_mode":mode})),
        QueueCommand::Autoplay(enabled) => (
            "autoplay",
            json!({"queue_id":queue_id,"autoplay_enabled":enabled}),
        ),
    };
    Ok((name, arguments))
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn queue_contract_uses_ma_item_ids_and_separate_clear_command() {
        let queue = QueueId("room".into());
        for (command, expected_name, expected) in [
            (QueueCommand::Clear, "clear", json!({"queue_id":"room"})),
            (
                QueueCommand::MoveItemToEnd(QueueItemId("stable-item".into())),
                "move_item_end",
                json!({"queue_id":"room", "queue_item_id":"stable-item"}),
            ),
            (
                QueueCommand::MoveItem {
                    item_id: QueueItemId("stable-item".into()),
                    shift: 0,
                },
                "move_item",
                json!({"queue_id":"room", "queue_item_id":"stable-item", "pos_shift":0}),
            ),
            (
                QueueCommand::Autoplay(true),
                "autoplay",
                json!({"queue_id":"room", "autoplay_enabled":true}),
            ),
            (
                QueueCommand::Autoplay(false),
                "autoplay",
                json!({"queue_id":"room", "autoplay_enabled":false}),
            ),
            (
                QueueCommand::PlayItem(QueueItemId("stable-item".into())),
                "play_index",
                json!({"queue_id":"room", "index":"stable-item"}),
            ),
            (
                QueueCommand::DeleteItem(QueueItemId("stable-item".into())),
                "delete_item",
                json!({"queue_id":"room", "item_id_or_index":"stable-item"}),
            ),
            (
                QueueCommand::MoveItem {
                    item_id: QueueItemId("stable-item".into()),
                    shift: -2,
                },
                "move_item",
                json!({"queue_id":"room", "queue_item_id":"stable-item", "pos_shift":-2}),
            ),
        ] {
            let (name, arguments) = queue_command_request(&queue, command).unwrap();
            assert_eq!(name, expected_name);
            assert_eq!(arguments, expected);
        }
    }

    #[test]
    fn media_items_are_serialized_in_original_order_without_shuffle() {
        let media: Vec<MediaItem> = serde_json::from_value(json!([
            {"media_type":"album", "item_id":"album", "provider":"library", "name":"Album"},
            {"media_type":"track", "item_id":"track", "provider":"library", "name":"Track"}
        ]))
        .unwrap();
        let (name, arguments) = queue_command_request(
            &QueueId("room".into()),
            QueueCommand::PlayMedia {
                media,
                option: QueueOption::Play,
                start_item: Some(1),
                shuffle: false,
            },
        )
        .unwrap();
        assert_eq!(name, "play_media");
        assert_eq!(arguments["media"][0]["media_type"], "album");
        assert_eq!(arguments["media"][1]["item_id"], "track");
        assert_eq!(arguments["start_item"], arguments["media"][1]);
        assert!(arguments.get("shuffle").is_none());
        assert!(arguments.get("radio_mode").is_none());
        assert!(arguments.get("startDownload").is_none());
    }

    #[test]
    fn shuffle_reorders_items_locally_and_preserves_the_explicit_start_item() {
        let media: Vec<MediaItem> = serde_json::from_value(json!([
            {"media_type":"track", "item_id":"first", "provider":"library", "name":"First"},
            {"media_type":"track", "item_id":"second", "provider":"library", "name":"Second"},
            {"media_type":"track", "item_id":"third", "provider":"library", "name":"Third"}
        ]))
        .unwrap();
        let original: Vec<Value> = media.iter().cloned().map(queue_media_payload).collect();
        let mut observed_reordering = false;
        for _ in 0..32 {
            let (name, arguments) = queue_command_request(
                &QueueId("room".into()),
                QueueCommand::PlayMedia {
                    media: media.clone(),
                    option: QueueOption::Play,
                    start_item: Some(1),
                    shuffle: true,
                },
            )
            .unwrap();
            assert_eq!(name, "play_media");
            let shuffled = arguments["media"].as_array().unwrap();
            assert_eq!(shuffled.len(), original.len());
            for item in &original {
                assert_eq!(shuffled.iter().filter(|entry| *entry == item).count(), 1);
            }
            assert_eq!(arguments["start_item"], original[1]);
            assert!(arguments.get("shuffle").is_none());
            assert_eq!(arguments["sort_by"], "original");
            observed_reordering |= shuffled != &original;
        }
        assert!(observed_reordering);
    }

    #[tokio::test]
    async fn rejects_invalid_seek_and_start_without_sending_a_request() {
        let client = MusicAssistantClient::new("http://127.0.0.1:1", None);
        assert!(client
            .execute_player_command(&PlayerId("player".into()), PlayerCommand::Seek(f64::NAN))
            .await
            .unwrap_err()
            .contains("finite"));
        assert!(client
            .execute_queue_command(
                &QueueId("queue".into()),
                QueueCommand::PlayMedia {
                    media: Vec::new(),
                    option: QueueOption::Play,
                    start_item: Some(1),
                    shuffle: false,
                }
            )
            .await
            .unwrap_err()
            .contains("outside"));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use super::{item_reference_arguments, merge_item_reference_arguments, queue_media_payload};
    use crate::music_assistant::models::identifiers::MediaId;
    use crate::music_assistant::models::media::{MediaItem, Track};
    use crate::music_assistant::user_data::UserDataSnapshot;
    use serde_json::{json, Value};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    pub(crate) fn server(
        responses: Vec<(&'static str, Value)>,
    ) -> (MusicAssistantClient, std::thread::JoinHandle<Vec<Value>>) {
        server_with_request_handler(responses, |_| {})
    }

    pub(crate) fn server_with_request_handler(
        responses: Vec<(&'static str, Value)>,
        mut handle_request: impl FnMut(&Value) + Send + 'static,
    ) -> (MusicAssistantClient, std::thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let mut responses = responses.into_iter().peekable();
            while responses.peek().is_some() {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (body_start, length) = loop {
                    let mut buffer = [0; 4096];
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&buffer[..read]);
                    if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while bytes.len() < body_start + length {
                    let mut buffer = [0; 4096];
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&buffer[..read]);
                }
                let request: Value =
                    serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
                let response = if request["command"] == "providers" {
                    json!([])
                } else {
                    let (expected, response) = responses.next().unwrap();
                    assert_eq!(request["command"], expected);
                    handle_request(&request);
                    requests.push(request);
                    response
                };
                let status = response
                    .get("test_http_status")
                    .and_then(Value::as_u64)
                    .unwrap_or(200);
                let body = response.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            requests
        });
        (
            MusicAssistantClient::new(&format!("http://{address}"), None),
            worker,
        )
    }

    #[tokio::test]
    async fn current_user_id_reuses_login_identity_without_auth_me() {
        let (client, worker) = server(vec![("test/finish", json!(null))]);
        client
            .initialize_current_user_id(UserId("user-1".into()))
            .unwrap();
        assert_eq!(
            client.get_current_user_id().await.unwrap().as_str(),
            "user-1"
        );
        assert!(client
            .initialize_current_user_id(UserId("other".into()))
            .is_err());
        client
            .execute_http_command::<Value>("test/finish", json!({}))
            .await
            .unwrap();
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn current_user_id_shares_legacy_lookup_and_retries_failures() {
        let (client, worker) = server(vec![
            ("auth/me", json!({"test_http_status":500})),
            (
                "auth/me",
                json!({"user_id":"user-1", "username":"test", "role":"user"}),
            ),
        ]);
        assert!(client.get_current_user_id().await.is_err());
        let (first, second) =
            tokio::join!(client.get_current_user_id(), client.get_current_user_id());
        assert_eq!(first.unwrap().as_str(), "user-1");
        assert_eq!(second.unwrap().as_str(), "user-1");
        assert_eq!(
            client.get_current_user_id().await.unwrap().as_str(),
            "user-1"
        );
        assert_eq!(worker.join().unwrap().len(), 2);
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
        first.load_recommendations(|| {}).await.unwrap();
        second.load_recommendations(|| {}).await.unwrap();
        assert!(first
            .get_recommendations(RecommendationView::Home)
            .await
            .unwrap()
            .folders
            .is_empty());
        assert_eq!(
            second
                .get_recommendations(RecommendationView::Home)
                .await
                .unwrap()
                .folders
                .len(),
            1
        );
        assert!(first
            .get_recommendations(RecommendationView::Home)
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
        client.load_recommendations(|| {}).await.unwrap();
        let snapshot = client
            .get_recommendations(RecommendationView::Explore)
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
            .get_recommendations(RecommendationView::Explore)
            .await
            .unwrap();
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn recommendations_share_inflight_and_completed_requests() {
        let (client, worker) = server(vec![("music/recommendations", json!([]))]);
        assert!(
            client
                .get_recommendations(RecommendationView::Home)
                .await
                .unwrap()
                .is_loading
        );
        let (first, second) = tokio::join!(
            client.load_recommendations(|| {}),
            client.load_recommendations(|| {})
        );
        first.unwrap();
        second.unwrap();
        let (home, explore) = tokio::join!(
            client.get_recommendations(RecommendationView::Home),
            client.get_recommendations(RecommendationView::Explore)
        );
        assert!(home.unwrap().folders.is_empty());
        assert!(explore.unwrap().folders.is_empty());
        assert!(client
            .get_recommendations(RecommendationView::Home)
            .await
            .unwrap()
            .folders
            .is_empty());
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recommendations_do_not_retry_failed_startup_requests_on_read() {
        let (client, worker) = server(vec![(
            "music/recommendations",
            json!({"test_http_status":500}),
        )]);
        assert!(client.load_recommendations(|| {}).await.is_err());
        assert!(client.load_recommendations(|| {}).await.is_err());
        let snapshot = client
            .get_recommendations(RecommendationView::Home)
            .await
            .unwrap();
        assert!(snapshot.folders.is_empty());
        assert!(!snapshot.is_loading);
        assert_eq!(worker.join().unwrap().len(), 1);
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
        client.load_recommendations(|| {}).await.unwrap();
        client
            .get_recommendations(RecommendationView::Home)
            .await
            .unwrap();
        client
            .execute_http_command::<Value>("test/finish", json!({}))
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
            .load_recommendations(|| {
                updates.push(client.recommendations.try_lock().unwrap().folders.clone());
            })
            .await
            .unwrap();
        assert!(updates[0][0].is_loading);
        assert!(updates[0][1].is_loading);
        assert!(updates[1][0].is_loading);
        assert!(!updates[1][1].is_loading);
        client
            .load_recommendations(|| panic!("startup was repeated"))
            .await
            .unwrap();
        for view in [
            RecommendationView::Home,
            RecommendationView::Explore,
            RecommendationView::Home,
            RecommendationView::Explore,
        ] {
            let snapshot = client.get_recommendations(view).await.unwrap();
            assert!(!snapshot.is_loading);
            assert_eq!(snapshot.folders.len(), 1);
            assert!(view.includes(snapshot.folders[0].item_id.as_str()));
        }
        client
            .execute_http_command::<Value>("test/finish", json!({}))
            .await
            .unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[1]["args"]["item_id"], "user_recommendations");
        assert_eq!(requests[2]["args"]["item_id"], "user_exploration_weekly");
    }

    #[tokio::test]
    async fn recommendations_keep_failed_rows_terminal_until_the_session_ends() {
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
        ]);
        client.load_recommendations(|| {}).await.unwrap();
        assert!(client
            .get_recommendations(RecommendationView::Home)
            .await
            .unwrap()
            .folders[0]
            .items
            .is_empty());
        assert_eq!(
            client
                .get_recommendations(RecommendationView::Home)
                .await
                .unwrap()
                .folders[0]
                .items
                .len(),
            0
        );
        assert_eq!(worker.join().unwrap().len(), 2);
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
            ("music/recommendations/items", json!([track])),
        ]);
        client.load_recommendations(|| {}).await.unwrap();
        let folders = client
            .get_recommendations(RecommendationView::Home)
            .await
            .unwrap()
            .folders;
        assert_eq!(folders[0].items.len(), 1);
        assert!(folders[1].items.is_empty());
        assert_eq!(folders[2].items.len(), 1);
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests[1]["args"],
            json!({"provider":"listenbrainz", "item_id":"user_top_tracks_monthly"})
        );
        assert_eq!(
            requests[2]["args"],
            json!({"provider":"listenbrainz", "item_id":"user_top_artists_monthly"})
        );
    }

    #[tokio::test]
    async fn missing_artwork_tries_mappings_and_reuses_cached_results() {
        let source = json!({"media_type":"artist", "item_id":"library-artist", "provider":"library", "name":"Original",
        "metadata":{"description":"Original description", "images":[]},
        "provider_mappings":[
            {"item_id":"empty", "provider_instance":"first", "provider_domain":"first"},
            {"item_id":"found", "provider_instance":"second", "provider_domain":"second"},
            {"item_id":"unused", "provider_instance":"third", "provider_domain":"third"}
        ]});
        let image =
            json!({"type":"thumb", "path":"https://example.com/artist.jpg", "provider":"second"});
        let (client, worker) = server(vec![
            ("music/artists/get", source.clone()),
            (
                "music/item",
                json!({"media_type":"artist", "item_id":"empty", "provider":"first", "name":"Empty"}),
            ),
            (
                "music/item",
                json!({"media_type":"artist", "item_id":"found", "provider":"second", "name":"Other", "metadata":{"images":[image.clone()]}}),
            ),
            ("music/artists/get", source),
        ]);
        for _ in 0..2 {
            let artist = client
                .get_artist(&MediaId("library-artist".into()), "library")
                .await
                .unwrap();
            assert_eq!(artist.media.item_id.as_str(), "library-artist");
            assert_eq!(artist.media.provider.as_str(), "library");
            assert_eq!(artist.media.name, "Original");
            let metadata = artist.media.metadata.unwrap();
            assert_eq!(
                metadata.description.as_deref(),
                Some("Original description")
            );
            assert_eq!(
                serde_json::to_value(&metadata.images[0]).unwrap()["path"],
                image["path"]
            );
        }
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(
            requests[1]["args"]["provider_instance_id_or_domain"],
            "first"
        );
        assert_eq!(
            requests[2]["args"]["provider_instance_id_or_domain"],
            "second"
        );
    }

    #[tokio::test]
    async fn missing_artwork_keeps_items_when_providers_fail_and_caches_misses() {
        let source = json!({"media_type":"artist", "item_id":"original", "provider":"library", "name":"Original",
        "metadata":{"description":"Keep this", "images":[]},
        "provider_mappings":[
            {"item_id":"failed", "provider_instance":"first", "provider_domain":"first"},
            {"item_id":"empty", "provider_instance":"second", "provider_domain":"second"}
        ]});
        let (client, worker) = server(vec![
            ("music/artists/get", source.clone()),
            ("music/item", json!({"test_http_status":500})),
            (
                "music/item",
                json!({"media_type":"artist", "item_id":"empty", "provider":"second", "name":"Empty"}),
            ),
            ("music/artists/get", source),
        ]);
        for _ in 0..2 {
            let artist = client
                .get_artist(&MediaId("original".into()), "library")
                .await
                .unwrap();
            let metadata = artist.media.metadata.unwrap();
            assert_eq!(metadata.description.as_deref(), Some("Keep this"));
            assert!(metadata.images.is_empty());
        }
        assert_eq!(worker.join().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn missing_artwork_enriches_nested_search_items_without_replacing_metadata() {
        let source = json!({"artists":[{"media_type":"artist", "item_id":"original", "provider":"library", "name":"Original",
            "metadata":{"description":"Keep this", "genres":["Rock"]},
            "provider_mappings":[{"item_id":"source", "provider_instance":"other", "provider_domain":"other"}]}]});
        let (client, worker) = server(vec![
            ("music/search", source),
            (
                "music/item",
                json!({"media_type":"artist", "item_id":"source", "provider":"other", "name":"Other", "metadata":{"description":"Do not copy", "images":[{"type":"thumb", "path":"", "provider":"other", "proxy_id":"artist-proxy"}]}}),
            ),
        ]);
        let result: Value = client
            .execute_command("music/search", json!({}))
            .await
            .unwrap();
        let artist = &result["artists"][0];
        assert_eq!(artist["provider"], "library");
        assert_eq!(artist["item_id"], "original");
        assert_eq!(artist["name"], "Original");
        assert_eq!(artist["metadata"]["description"], "Keep this");
        assert_eq!(artist["metadata"]["genres"], json!(["Rock"]));
        assert_eq!(artist["metadata"]["images"][0]["proxy_id"], "artist-proxy");
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn existing_artwork_never_fetches_provider_items() {
        let (client, worker) = server(vec![(
            "music/artists/get",
            json!({"media_type":"artist", "item_id":"original", "provider":"library", "name":"Original",
            "metadata":{"images":[{"type":"thumb", "path":"https://example.com/original.jpg", "provider":"library"}]},
            "provider_mappings":[{"item_id":"other", "provider_instance":"other", "provider_domain":"other"}]}),
        )]);
        let artist = client
            .get_artist(&MediaId("original".into()), "library")
            .await
            .unwrap();
        assert_eq!(
            artist.media.metadata.unwrap().images[0].path,
            "https://example.com/original.jpg"
        );
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn removing_a_favorite_sends_null_not_dislike() {
        let (client, worker) = server(vec![("music/favorites/set_item", Value::Null)]);
        let item = serde_json::from_value(
            json!({"media_type":"track", "item_id":"1", "provider":"library", "name":"Track"}),
        )
        .unwrap();
        client.set_favorite(&item, false).await.unwrap();
        let requests = worker.join().unwrap();
        assert!(requests[0]["args"]["favorite"].is_null());
    }

    #[tokio::test]
    async fn playlist_tasks_are_confirmed_and_failures_are_returned() {
        for status in ["success", "failed"] {
            let (client, worker) = server(vec![
                (
                    "music/playlists/add_playlist_tracks",
                    json!({"id":"task", "status":"pending"}),
                ),
                (
                    "tasks/get",
                    json!({"id":"task", "status":status, "last_error":"test failure"}),
                ),
            ]);
            let result = client
                .add_playlist_tracks("42", vec!["test://track/a".into()])
                .await;
            assert_eq!(result.is_ok(), status == "success");
            worker.join().unwrap();
        }
    }

    #[test]
    fn playlist_favorite_preserves_all_three_server_states() {
        for favorite in [Value::Null, json!(true), json!(false)] {
            let playlist: Playlist = serde_json::from_value(json!({
                "item_id":"42", "provider":"library", "name":"Playlist", "favorite":favorite
            }))
            .unwrap();
            assert_eq!(
                serde_json::to_value(playlist).unwrap()["favorite"],
                favorite
            );
        }
        let playlist: Playlist = serde_json::from_value(json!({
            "item_id":"42", "provider":"library", "name":"Playlist"
        }))
        .unwrap();
        assert!(serde_json::to_value(playlist).unwrap()["favorite"].is_null());
    }

    #[tokio::test]
    async fn private_playlist_creation_uses_default_builtin_and_sets_private_ownership() {
        let playlist = json!({"item_id":"42", "provider":"library", "name":"Playlist"});
        let (client, worker) = server(vec![
            ("music/playlists/create_playlist", playlist.clone()),
            ("music/playlists/set_access", playlist),
        ]);
        client
            .create_private_playlist("Playlist", "user-one")
            .await
            .unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(requests[0]["args"], json!({"name":"Playlist"}));
        assert_eq!(requests[1]["args"]["owner"], "user-one");
        assert_eq!(requests[1]["args"]["sharing"], "private");
    }

    #[test]
    fn availability_combines_mappings_with_live_provider_status() {
        let mut track = json!({ "item_id": "track", "provider": "library", "name": "Track", "provider_mappings": [
            { "item_id": "one", "provider_domain": "deezer", "provider_instance": "deezer-one", "available": false },
            { "item_id": "two", "provider_domain": "deezer", "provider_instance": "deezer-two", "available": true }
        ] });
        let mut providers: Vec<super::ProviderInstance> = serde_json::from_value(json!([
            { "instance_id": "deezer-one", "available": true }, { "instance_id": "deezer-two", "available": false }
        ])).unwrap();
        super::annotate_media_availability(&mut track, Some(&providers));
        assert_eq!(track["available"], false);
        track.as_object_mut().unwrap().remove("available");
        providers[1].available = true;
        super::annotate_media_availability(&mut track, Some(&providers));
        assert_eq!(track["available"], true);
        assert_eq!(track["provider_mappings"][0]["available"], false);
    }

    #[test]
    fn unknown_provider_status_does_not_block_metadata_available_tracks() {
        let mut tracks = json!([
            { "item_id": "unknown", "provider": "library", "name": "Unknown" },
            { "item_id": "slim", "provider": "deezer", "name": "Unavailable", "available": false },
            { "item_id": "mapped", "provider": "deezer", "name": "Unavailable", "provider_mappings": [
                { "item_id": "mapped", "provider_domain": "deezer", "provider_instance": "deezer-one", "available": false }
            ] }
        ]);
        super::annotate_media_availability(&mut tracks, None);
        assert_eq!(tracks[0]["available"], true);
        assert_eq!(tracks[1]["available"], false);
        assert_eq!(tracks[2]["available"], false);
    }

    #[test]
    fn only_play_media_http_500_is_a_track_staging_failure() {
        for (command, status, expected) in [
            ("player_queues/play_media", 500, true),
            ("player_queues/play_media", 401, false),
            ("player_queues/play_media", 503, false),
            ("player_queues/clear", 500, false),
        ] {
            let error = super::HttpCommandError::Http {
                command: command.to_string(),
                status,
                details: "Internal server error".to_string(),
            };
            assert_eq!(error.is_track_staging_failure(), expected);
        }
        assert!(!super::HttpCommandError::Transport {
            command: "player_queues/play_media".to_string(),
            is_timeout: true,
            is_connect: false,
            details: "timeout".to_string(),
        }
        .is_track_staging_failure());
    }

    #[test]
    fn queue_media_remain_objects_even_without_mappings() {
        for provider in ["ytmusic--test", "deezer--test", "jellyfin--test", "library"] {
            for media_type in ["track", "album", "artist", "playlist", "podcast_episode"] {
                for uri in [None, Some(""), Some("custom://track/original")] {
                    let item: MediaItem = serde_json::from_value(json!({
                        "media_type": media_type,
                        "item_id": "item/with:identifier",
                        "provider": provider,
                        "name": "Cached media",
                        "uri": uri,
                        "podcast": {
                            "item_id": "podcast", "provider": provider, "name": "Podcast"
                        },
                    }))
                    .unwrap();
                    let payload = queue_media_payload(item);
                    assert!(payload.is_object());
                    assert_eq!(payload["provider"], provider);
                    assert_eq!(payload["media_type"], media_type);
                    assert_eq!(payload["uri"], json!(uri));
                }
            }
        }
    }

    #[test]
    fn snapshot_favorites_and_playlist_items_keep_complete_queue_payloads() {
        for (domain, provider) in [
            ("ytmusic", "ytmusic--test"),
            ("deezer", "deezer--test"),
            ("jellyfin", "jellyfin--test"),
            ("deezer", "library"),
        ] {
            for has_uri in [true, false] {
                let track_uri = format!("{provider}://track/track-1");
                let album_uri = format!("{provider}://album/album-1");
                let track = json!({
                    "item_id": "track-1", "provider": provider, "name": "Track",
                    "uri": has_uri.then_some(&track_uri),
                    "provider_mappings": [{
                        "item_id": "track-1", "provider_domain": domain, "provider_instance": "source--test"
                    }],
                });
                let album = json!({
                    "item_id": "album-1", "provider": provider, "name": "Album",
                    "uri": has_uri.then_some(&album_uri),
                    "provider_mappings": [{
                        "item_id": "album-1", "provider_domain": domain, "provider_instance": "source--test"
                    }],
                });
                let snapshot: UserDataSnapshot = serde_json::from_value(json!({
                    "favorites": { "tracks": [track], "albums": [album] },
                    "playlists": [{
                        "item_id": "42", "provider": "library", "name": "Playlist",
                        "items": [track, track],
                    }],
                }))
                .unwrap();
                let restored: UserDataSnapshot =
                    serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap();
                let favorite_track = restored.favorites.tracks[0].clone();
                assert_eq!(favorite_track.media.provider_mappings.len(), 1);
                let expected_track: Track = serde_json::from_value(track).unwrap();
                let track_payload = queue_media_payload(MediaItem::Track(favorite_track));
                assert_eq!(
                    track_payload["provider_mappings"],
                    json!(expected_track.media.provider_mappings)
                );
                let expected_album: super::Album = serde_json::from_value(album).unwrap();
                let album_payload =
                    queue_media_payload(MediaItem::Album(restored.favorites.albums[0].clone()));
                assert_eq!(
                    album_payload["provider_mappings"],
                    json!(expected_album.media.provider_mappings)
                );
                let playlist_media: Vec<_> = restored.playlists[0]
                    .items
                    .iter()
                    .cloned()
                    .map(MediaItem::Track)
                    .map(queue_media_payload)
                    .collect();
                assert_eq!(playlist_media.len(), 2);
                assert_eq!(playlist_media[0], playlist_media[1]);
                assert!(playlist_media.iter().all(Value::is_object));
            }
        }
    }

    #[test]
    fn serializes_complete_queue_media_with_nested_types() {
        let track: MediaItem = serde_json::from_value(json!({
            "media_type": "track",
            "item_id": "track-1",
            "provider": "ytmusic",
            "provider_mappings": [{
                "item_id": "track-1", "provider_domain": "ytmusic", "provider_instance": "ytmusic--test"
            }],
            "name": "Take on Me",
            "duration": 232,
            "artists": [{
                "media_type": "artist",
                "item_id": "artist-1",
                "provider": "ytmusic",
                "name": "a-ha"
            }],
            "album": {
                "media_type": "album",
                "item_id": "album-1",
                "provider": "ytmusic",
                "name": "Hunting High and Low"
            }
        }))
        .expect("deserialize track with nested media");

        let payload = queue_media_payload(track);
        assert_eq!(payload["media_type"], "track");
        assert_eq!(payload["duration"], 232);
        assert_eq!(payload["album"]["media_type"], "album");
        assert_eq!(payload["album"]["item_id"], "album-1");
        assert_eq!(payload["artists"][0]["media_type"], "artist");
        assert_eq!(payload["artists"][0]["name"], "a-ha");
        assert_eq!(
            payload["provider_mappings"][0]["provider_instance"],
            "ytmusic--test"
        );

        let album: MediaItem = serde_json::from_value(json!({
            "media_type": "album",
            "item_id": "album-1",
            "provider": "ytmusic",
            "provider_mappings": [{
                "item_id": "album-1", "provider_domain": "ytmusic", "provider_instance": "ytmusic--test"
            }],
            "name": "Hunting High and Low",
            "artists": [{
                "media_type": "artist",
                "item_id": "artist-1",
                "provider": "ytmusic",
                "name": "a-ha"
            }]
        }))
        .expect("deserialize album with artist");
        let payload = queue_media_payload(album);
        assert_eq!(payload["media_type"], "album");
        assert_eq!(payload["artists"][0]["media_type"], "artist");

        let episode: MediaItem = serde_json::from_value(json!({
            "media_type": "podcast_episode",
            "item_id": "episode-1",
            "provider": "library",
            "provider_mappings": [{
                "item_id": "episode-1", "provider_domain": "test", "provider_instance": "test--instance"
            }],
            "name": "Episode one",
            "podcast": {
                "media_type": "podcast",
                "item_id": "podcast-1",
                "provider": "library",
                "name": "Podcast one"
            }
        }))
        .expect("deserialize episode with podcast");
        let payload = queue_media_payload(episode);
        assert_eq!(payload["media_type"], "podcast_episode");
        assert_eq!(payload["podcast"]["media_type"], "podcast");
        assert_eq!(payload["podcast"]["name"], "Podcast one");
    }

    #[test]
    fn builds_library_track_query_arguments() {
        let item_id = MediaId("album-42".to_string());
        let provider = "library";

        assert_eq!(
            item_reference_arguments(&item_id, provider),
            serde_json::json!({
                "item_id": "album-42",
                "provider_instance_id_or_domain": "library",
            })
        );
        assert_eq!(
            merge_item_reference_arguments(&item_id, provider, serde_json::json!({ "limit": 25 }),),
            serde_json::json!({
                "item_id": "album-42",
                "provider_instance_id_or_domain": "library",
                "limit": 25,
            })
        );
    }
}
