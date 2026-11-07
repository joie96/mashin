//! MA wire values and neutral playback values are translated only at this boundary.

use super::dto::player::MusicAssistantPlayer;
use super::dto::queue::MusicAssistantQueue;
use crate::domain::media::MediaItem;
use crate::playback::server::{ServerPlayer, ServerPlayerMedia, ServerQueue};
use serde_json::{json, Value};

// Playback state mapping.
impl From<MusicAssistantPlayer> for ServerPlayer {
    fn from(player: MusicAssistantPlayer) -> Self {
        Self {
            player_id: player.player_id,
            playback_state: player.playback_state,
            elapsed_time: player.elapsed_time,
            volume_level: player.volume_level,
            volume_muted: player.volume_muted,
            active_source: player.active_source,
            current_media: player.current_media.map(|media| ServerPlayerMedia {
                queue_item_id: media.queue_item_id,
                duration: media.duration,
            }),
        }
    }
}

impl From<MusicAssistantQueue> for ServerQueue {
    fn from(queue: MusicAssistantQueue) -> Self {
        Self {
            queue_id: queue.queue_id,
            current_index: queue.current_index,
            shuffle_enabled: queue.shuffle_enabled,
            repeat_mode: queue.repeat_mode,
            autoplay_enabled: queue.autoplay_enabled,
            current_item: queue.current_item,
        }
    }
}

// Queue request mapping.
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
