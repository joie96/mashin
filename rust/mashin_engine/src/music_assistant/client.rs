use super::models::identifiers::{AccessToken, MediaId, PlayerId, QueueId};
use super::models::media::{
    Album, Artist, MediaItem, MediaItemFields, MediaType, MusicAssistantSearchResults, Playlist, Track,
};
use super::models::player::MusicAssistantPlayer;
use super::models::queue::{MusicAssistantQueue, QueueItem, RepeatMode};
use super::models::recommendation::MusicAssistantRecommendationFolder;
use super::models::session::{MusicAssistantUser, ProviderInstance, ProviderManifest, ServerInfo};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};

struct ProviderAvailabilitySnapshot {
    endpoint: String,
    access_token: Option<AccessToken>,
    expires_at: Instant,
    providers: Option<Vec<ProviderInstance>>,
}

static PROVIDER_AVAILABILITY: once_cell::sync::Lazy<tokio::sync::Mutex<Vec<ProviderAvailabilitySnapshot>>> =
    once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(Vec::new()));

const RADIO_MAXIMUM_TRACK_DURATION_SECONDS: u32 = 20 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpCommandError {
    Http { command: String, status: u16, details: String },
    Transport { command: String, is_timeout: bool, is_connect: bool, details: String },
    InvalidResponse { command: String, details: String },
}

impl HttpCommandError {
    pub(crate) fn is_track_staging_failure(&self) -> bool {
        matches!(self, Self::Http { command, status: 500, .. } if command == "player_queues/play_media")
    }
}

impl std::fmt::Display for HttpCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http { command, status, details } => write!(formatter, "Music Assistant command '{command}' returned HTTP {status}: {details}"),
            Self::Transport { command, details, .. } => write!(formatter, "Music Assistant command '{command}' transport failed: {details}"),
            Self::InvalidResponse { command, details } => write!(formatter, "Music Assistant command '{command}' returned invalid JSON: {details}"),
        }
    }
}

impl From<HttpCommandError> for String {
    fn from(error: HttpCommandError) -> Self {
        error.to_string()
    }
}

pub(crate) struct MusicAssistantClient {
    endpoint: String,
    access_token: Option<AccessToken>,
    http_client: reqwest::Client,
}

impl MusicAssistantClient {
    // Construction.
    pub(crate) fn new(base_url: &str, access_token: Option<AccessToken>) -> Self {
        Self {
            endpoint: format!("{}/api", base_url.trim_end_matches('/')),
            access_token,
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("create Music Assistant HTTP client"),
        }
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

    pub(crate) async fn update_user(&self, update: Value) -> Result<MusicAssistantUser, String> {
        self.execute_command("auth/user/update", update).await
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

    // Complete incoming user-data media before insertion, never during playback or store reads.
    pub(crate) async fn complete_user_data_media(&self, items: Vec<MediaItem>) -> Result<Vec<MediaItem>, String> {
        let mut completed = Vec::with_capacity(items.len());
        for item in items {
            completed.push(self.complete_user_data_item(item).await?);
        }
        Ok(completed)
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
        self.execute_command(
            "music/playlists/playlist_tracks",
            json!({
                "item_id": item_id,
                "provider_instance_id_or_domain": provider,
                "force_refresh": force_refresh,
                "allow_dynamic_tracks": allow_dynamic_tracks,
            }),
        )
        .await
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
    ) -> Result<Vec<MusicAssistantRecommendationFolder>, String> {
        self.execute_command("music/recommendations", json!({}))
            .await
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
    pub(crate) async fn add_playlist_tracks(
        &self,
        playlist_id: &str,
        uris: Vec<String>,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "music/playlists/add_playlist_tracks",
            json!({ "db_playlist_id": playlist_id, "uris": uris }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn remove_playlist_tracks(
        &self,
        playlist_id: &str,
        positions_to_remove: Vec<u32>,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "music/playlists/remove_playlist_tracks",
            json!({
                "db_playlist_id": playlist_id,
                "positions_to_remove": positions_to_remove,
            }),
        )
        .await
        .map(|_| ())
    }

    // Remote player and queue queries.
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

    pub(crate) async fn set_queue_autoplay(
        &self,
        queue_id: &QueueId,
        autoplay_enabled: bool,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "player_queues/autoplay",
            json!({ "queue_id": queue_id, "autoplay_enabled": autoplay_enabled }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn queue_clear(&self, queue_id: &QueueId) -> Result<(), String> {
        self.execute_command::<Value>(
            "player_queues/clear",
            json!({ "queue_id": queue_id, "skip_stop": true }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn queue_play_media(
        &self,
        queue_id: QueueId,
        media: Vec<MediaItem>,
        option: &str,
        radio_mode: bool,
    ) -> Result<(), HttpCommandError> {
        let media = media
            .into_iter()
            .map(queue_media_payload)
            .collect::<Vec<_>>();
        self.execute_http_command::<Value>(
            "player_queues/play_media",
            json!({
                "queue_id": queue_id,
                "media": media,
                "option": option,
                "radio_mode": radio_mode,
                "sort_by": "original",
            }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn queue_play_index(
        &self,
        queue_id: QueueId,
        index: usize,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "player_queues/play_index",
            json!({ "queue_id": queue_id, "index": index }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn set_queue_shuffle(
        &self,
        queue_id: &str,
        shuffle_enabled: bool,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "player_queues/shuffle",
            json!({ "queue_id": queue_id, "shuffle_enabled": shuffle_enabled }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn set_queue_repeat(
        &self,
        queue_id: &str,
        repeat_mode: RepeatMode,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "player_queues/repeat",
            json!({ "queue_id": queue_id, "repeat_mode": repeat_mode }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn player_seek(
        &self,
        player_id: PlayerId,
        position: i64,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "players/cmd/seek",
            json!({ "player_id": player_id, "position": position.max(0) }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn player_stop(&self, player_id: &PlayerId) -> Result<(), String> {
        self.execute_command::<Value>("players/cmd/stop", json!({ "player_id": player_id }))
            .await
            .map(|_| ())
    }

    // HTTP command transport.
    fn complete_user_data_item(&self, mut item: MediaItem) -> futures_util::future::BoxFuture<'_, Result<MediaItem, String>> {
        Box::pin(async move {
            if matches!(&item, MediaItem::Playlist(playlist) if playlist.media.provider.as_str() == super::user_data::LOCAL_PLAYLIST_PROVIDER) {
                return Ok(item);
            }
            if item.fields().provider_mappings.is_empty() {
                let fields = item.fields();
                item = match &item {
                    MediaItem::Track(_) => MediaItem::Track(self.get_track(&fields.item_id, fields.provider.as_str()).await?),
                    MediaItem::Album(_) => MediaItem::Album(self.get_album(&fields.item_id, fields.provider.as_str()).await?),
                    MediaItem::Artist(_) => MediaItem::Artist(self.get_artist(&fields.item_id, fields.provider.as_str()).await?),
                    MediaItem::Playlist(_) => MediaItem::Playlist(self.get_playlist(&fields.item_id, fields.provider.as_str()).await?),
                    _ => return Err("unsupported user data media type".to_string()),
                };
                if item.fields().provider_mappings.is_empty() {
                    return Err(format!("complete provider mappings are required for {}:{}", item.fields().provider.as_str(), item.fields().item_id.as_str()));
                }
            }
            match &mut item {
                MediaItem::Track(track) => {
                    for artist in &mut track.artists {
                        let MediaItem::Artist(completed) = self.complete_user_data_item(MediaItem::Artist(artist.clone())).await? else {
                            return Err("expected a completed artist".to_string());
                        };
                        *artist = completed;
                    }
                    if let Some(album) = track.album.take() {
                        let MediaItem::Album(completed) = self.complete_user_data_item(MediaItem::Album(album)).await? else {
                            return Err("expected a completed album".to_string());
                        };
                        track.album = Some(completed);
                    }
                }
                MediaItem::Album(album) => {
                    for artist in &mut album.artists {
                        let MediaItem::Artist(completed) = self.complete_user_data_item(MediaItem::Artist(artist.clone())).await? else {
                            return Err("expected a completed artist".to_string());
                        };
                        *artist = completed;
                    }
                }
                _ => {}
            }
            Ok(item)
        })
    }

    async fn provider_availability(&self) -> Option<Vec<ProviderInstance>> {
        let mut snapshots = PROVIDER_AVAILABILITY.lock().await;
        snapshots.retain(|snapshot| snapshot.expires_at > Instant::now());
        if let Some(snapshot) = snapshots.iter().find(|snapshot| {
            snapshot.endpoint == self.endpoint && snapshot.access_token == self.access_token
        }) {
            return snapshot.providers.clone();
        }
        let providers = tokio::time::timeout(
            Duration::from_secs(5),
            self.execute_http_command::<Vec<ProviderInstance>>("providers", json!({})),
        )
        .await
        .ok()
        .and_then(Result::ok);
        snapshots.push(ProviderAvailabilitySnapshot {
            endpoint: self.endpoint.clone(),
            access_token: self.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(if providers.is_some() { 30 } else { 5 }),
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
            let providers = self.provider_availability().await;
            annotate_media_availability(&mut response, providers.as_deref());
        }
        serde_json::from_value(response).map_err(|error| {
            format!("Music Assistant command '{command}' returned invalid data: {error}")
        })
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

// Transport always sends modeled media objects, including nested media type tags.
fn queue_media_payload(media_item: MediaItem) -> Value {
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

// Radio helpers use Music Assistant's dynamic radio playlists.
pub(crate) async fn get_radio_tracks(
    client: &MusicAssistantClient,
    radio_seed: &MediaItem,
    started_tracks: Vec<Track>,
) -> Result<Vec<Track>, String> {
    let seed_uri = radio_seed_uri(radio_seed)
        .ok_or_else(|| "radio seed has no Music Assistant URI".to_string())?;
    let radio_tracks = client
        .get_playlist_tracks(&MediaId(seed_uri), "radio_playlist", true, true)
        .await?;
    Ok(filter_radio_tracks(radio_tracks, &started_tracks))
}

fn filter_radio_tracks(radio_tracks: Vec<Track>, started_tracks: &[Track]) -> Vec<Track> {
    let started_track_keys = started_tracks.iter().map(track_key).collect::<HashSet<_>>();
    let mut track_keys = HashSet::new();
    radio_tracks
        .into_iter()
        .filter(|track| track.media.is_playable && track.media.is_available())
        .filter(|track| !started_track_keys.contains(&track_key(track)))
        .filter(|track| track.duration <= RADIO_MAXIMUM_TRACK_DURATION_SECONDS)
        .filter(|track| track_keys.insert(track_key(track)))
        .collect()
}

fn item_reference_arguments(item_id: &MediaId, provider: &str) -> Value {
    json!({
        "item_id": item_id,
        "provider_instance_id_or_domain": provider,
    })
}

fn radio_seed_uri(media_item: &MediaItem) -> Option<String> {
    let fields = media_item.fields();
    if let Some(uri) = fields
        .uri
        .as_ref()
        .map(|uri| uri.as_str())
        .filter(|uri| !uri.trim().is_empty())
    {
        return Some(uri.to_string());
    }
    let provider = fields.provider.as_str().trim();
    let item_id = fields.item_id.as_str().trim();
    if provider.is_empty() || item_id.is_empty() {
        return None;
    }
    let media_type = match media_item {
        MediaItem::Artist(_) => "artist",
        MediaItem::Album(_) => "album",
        MediaItem::Track(_) => "track",
        MediaItem::Playlist(_) => "playlist",
        _ => return None,
    };
    Some(format!("{provider}://{media_type}/{item_id}"))
}

fn track_key(track: &Track) -> String {
    track
        .media
        .uri
        .as_ref()
        .map(|uri| uri.as_str())
        .filter(|uri| !uri.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{}:{}",
                track.media.provider.as_str(),
                track.media.item_id.as_str()
            )
        })
}

fn merge_item_reference_arguments(item_id: &MediaId, provider: &str, arguments: Value) -> Value {
    let mut merged = item_reference_arguments(item_id, provider);
    if let (Some(merged), Some(arguments)) = (merged.as_object_mut(), arguments.as_object()) {
        merged.extend(arguments.clone());
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::{
        filter_radio_tracks, item_reference_arguments, merge_item_reference_arguments,
        queue_media_payload, radio_seed_uri,
    };
    use crate::music_assistant::models::identifiers::MediaId;
    use crate::music_assistant::models::media::{MediaItem, Track};
    use crate::music_assistant::user_data::{UserDataController, UserDataSnapshot};
    use crate::music_assistant::user_data_store::StoredUserData;
    use serde_json::{json, Value};

    #[tokio::test]
    async fn complete_ingestion_keeps_mapped_media_without_network_requests() {
        let client = super::MusicAssistantClient::new("http://127.0.0.1:0", None);
        let item: MediaItem = serde_json::from_value(json!({
            "media_type": "track", "item_id": "track", "provider": "ytmusic--test", "name": "Track",
            "provider_mappings": [{ "item_id": "video", "provider_domain": "ytmusic", "provider_instance": "ytmusic--test" }],
            "metadata": { "lyrics": "Full lyrics" }
        })).unwrap();
        let completed = client.complete_user_data_media(vec![item.clone()]).await.unwrap();
        assert_eq!(completed, vec![item]);
    }

    #[tokio::test]
    async fn incomplete_ingestion_fails_without_returning_partial_media() {
        let client = super::MusicAssistantClient::new("http://127.0.0.1:0", None);
        let item: MediaItem = serde_json::from_value(json!({
            "media_type": "track", "item_id": "track", "provider": "ytmusic--test", "name": "Track"
        })).unwrap();
        assert!(client.complete_user_data_media(vec![item]).await.is_err());
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
    fn radio_uses_the_original_media_uri_as_playlist_item_id() {
        for media_type in ["track", "album", "artist", "playlist"] {
            let media_item: MediaItem = serde_json::from_value(json!({
                "media_type": media_type,
                "item_id": "item-1",
                "provider": "library",
                "name": "Seed",
                "uri": format!("library://{media_type}/item-1"),
            }))
            .expect("deserialize media item");
            assert_eq!(
                radio_seed_uri(&media_item).as_deref(),
                Some(format!("library://{media_type}/item-1").as_str())
            );
        }
    }

    #[test]
    fn radio_keeps_seed_tracks_that_were_not_started() {
        let track = |item_id: &str, duration: u32| -> Track {
            serde_json::from_value(json!({
                "media_type": "track",
                "item_id": item_id,
                "provider": "library",
                "name": item_id,
                "duration": duration,
            }))
            .expect("deserialize radio track")
        };
        let started = track("started", 180);
        let result = filter_radio_tracks(
            vec![
                started.clone(),
                track("other-seed", 180),
                track("other-seed", 180),
                track("too-long", 1201),
            ],
            &[started],
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].media.item_id.as_str(), "other-seed");
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
                    })).unwrap();
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
    fn persisted_favorites_and_playlist_items_keep_complete_payloads() {
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
                        "item_id": "local", "provider": "mashin", "name": "Playlist",
                        "items": [track, track],
                    }],
                })).unwrap();
                let preferences = serde_json::from_value(json!({
                    "mashin.user_data_version": 5,
                    "mashin.user_data": StoredUserData::from_snapshot(&snapshot),
                })).unwrap();
                let restored = UserDataController::from_preferences(preferences).unwrap().snapshot();
                let favorite_track = restored.favorites.tracks[0].clone();
                assert_eq!(favorite_track.media.provider_mappings.len(), 1);
                let expected_track: Track = serde_json::from_value(track).unwrap();
                let track_payload = queue_media_payload(MediaItem::Track(favorite_track));
                assert_eq!(track_payload["provider_mappings"], json!(expected_track.media.provider_mappings));
                let expected_album: super::Album = serde_json::from_value(album).unwrap();
                let album_payload = queue_media_payload(MediaItem::Album(restored.favorites.albums[0].clone()));
                assert_eq!(album_payload["provider_mappings"], json!(expected_album.media.provider_mappings));
                let playlist_media: Vec<_> = restored.playlists[0].items.iter()
                    .cloned().map(MediaItem::Track).map(queue_media_payload).collect();
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
        assert_eq!(payload["provider_mappings"][0]["provider_instance"], "ytmusic--test");

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
