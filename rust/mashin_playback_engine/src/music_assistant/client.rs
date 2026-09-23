use super::models::identifiers::{AccessToken, MediaId, PlayerId, QueueId};
use super::models::media::{
    Album, Artist, MediaItem, MusicAssistantSearchResults, Playlist, Track,
};
use super::models::player::MusicAssistantPlayer;
use super::models::queue::{MusicAssistantQueue, QueueItem, RepeatMode};
use super::models::recommendation::MusicAssistantRecommendationFolder;
use super::models::session::{MusicAssistantUser, ProviderManifest, ServerInfo};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::HashSet;

const RADIO_MAXIMUM_TRACK_DURATION_SECONDS: u32 = 20 * 60;

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
            http_client: reqwest::Client::new(),
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

    pub(crate) async fn search(
        &self,
        search_query: &str,
        limit: u32,
    ) -> Result<MusicAssistantSearchResults, String> {
        self.execute_command(
            "music/search",
            json!({
                "search_query": search_query,
                "media_types": ["artist", "album", "track", "playlist"],
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
    ) -> Result<(), String> {
        self.execute_command::<Value>(
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
    async fn execute_command<Response>(
        &self,
        command: &str,
        arguments: Value,
    ) -> Result<Response, String>
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
            .map_err(|error| format!("Music Assistant command failed: {error}"))?;
        let status = response.status();
        let response_body = response.text().await.map_err(|error| {
            format!("Music Assistant command '{command}' response failed: {error}")
        })?;
        if !status.is_success() {
            let response_excerpt = response_body.chars().take(512).collect::<String>();
            return Err(format!(
                "Music Assistant command '{command}' returned HTTP {status}: {response_excerpt}",
            ));
        }
        serde_json::from_str(&response_body).map_err(|error| {
            let response_excerpt = response_body.chars().take(512).collect::<String>();
            format!(
                "Music Assistant command '{command}' returned invalid JSON: {error}; response: {response_excerpt}"
            )
        })
    }
}

// Radio helpers use Music Assistant's dynamic radio playlists.
pub(crate) async fn get_radio_tracks(
    client: &MusicAssistantClient,
    media_items: Vec<MediaItem>,
    seed_tracks: Vec<Track>,
) -> Result<Vec<Track>, String> {
    let seed_track_keys = seed_tracks.iter().map(track_key).collect::<HashSet<_>>();
    let mut radio_tracks = Vec::new();
    for media_item in media_items {
        let Some(seed_uri) = radio_seed_uri(&media_item) else {
            continue;
        };
        radio_tracks.extend(
            client
                .get_playlist_tracks(&MediaId(seed_uri), "radio_playlist", true, true)
                .await?,
        );
    }
    let mut track_keys = HashSet::new();
    Ok(radio_tracks
        .into_iter()
        .filter(|track| !seed_track_keys.contains(&track_key(track)))
        .filter(|track| track.duration <= RADIO_MAXIMUM_TRACK_DURATION_SECONDS)
        .filter(|track| track_keys.insert(track_key(track)))
        .collect())
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
    use super::{item_reference_arguments, merge_item_reference_arguments};
    use crate::music_assistant::models::identifiers::MediaId;

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
