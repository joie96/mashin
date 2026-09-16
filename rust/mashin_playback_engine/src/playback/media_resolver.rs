//! Resolves playback references into engine-neutral playable media.

use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::media::{MediaItem, MediaItemFields, Track};

const ARTIST_TRACK_LIMIT: usize = 25;

// Music Assistant media resolution for local playback queues.
pub(crate) async fn resolve_music_assistant_media_items(
    client: &MusicAssistantClient,
    media_items: Vec<MediaItem>,
) -> Result<Vec<Track>, String> {
    let mut tracks = Vec::new();
    for media_item in media_items {
        match media_item {
            MediaItem::Track(track) => tracks.push(track),
            MediaItem::Playlist(playlist) => {
                let Some(provider) = resolve_provider(&playlist.media) else {
                    continue;
                };
                tracks.extend(
                    client
                        .get_playlist_tracks(&playlist.media.item_id, &provider, false, false)
                        .await?,
                );
            }
            MediaItem::Album(album) => {
                let Some(provider) = resolve_provider(&album.media) else {
                    continue;
                };
                tracks.extend(
                    client
                        .get_album_tracks(&album.media.item_id, &provider)
                        .await?,
                );
            }
            MediaItem::Artist(artist) => {
                let Some(provider) = resolve_provider(&artist.media) else {
                    continue;
                };
                let top_tracks = client
                    .get_artist_top_tracks(&artist.media.item_id, &provider)
                    .await?;
                if top_tracks.is_empty() {
                    tracks.extend(
                        client
                            .get_artist_tracks(&artist.media.item_id, &provider)
                            .await?
                            .into_iter()
                            .take(ARTIST_TRACK_LIMIT),
                    );
                } else {
                    tracks.extend(top_tracks.into_iter().take(ARTIST_TRACK_LIMIT));
                }
            }
            _ => {}
        }
    }
    Ok(tracks)
}

fn resolve_provider(fields: &MediaItemFields) -> Option<String> {
    if !fields.provider.as_str().trim().is_empty() {
        return Some(fields.provider.as_str().to_string());
    }
    fields
        .provider_mappings
        .iter()
        .map(|mapping| mapping.provider_instance.as_str())
        .find(|provider| !provider.trim().is_empty())
        .or_else(|| {
            fields
                .provider_mappings
                .iter()
                .map(|mapping| mapping.provider_domain.as_str())
                .find(|provider| !provider.trim().is_empty())
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::resolve_provider;
    use crate::music_assistant::models::identifiers::{MediaId, ProviderId, ProviderInstanceId};
    use crate::music_assistant::models::media::{MediaItemFields, ProviderMapping};

    fn fields(provider: &str, mappings: Vec<ProviderMapping>) -> MediaItemFields {
        MediaItemFields {
            item_id: MediaId("item-42".to_string()),
            provider: ProviderInstanceId(provider.to_string()),
            name: "Item".to_string(),
            version: String::new(),
            sort_name: None,
            uri: None,
            external_ids: Vec::new(),
            is_playable: true,
            provider_mappings: mappings,
            metadata: None,
            favorite: false,
            position: None,
            date_added: None,
        }
    }

    fn provider_mapping() -> ProviderMapping {
        ProviderMapping {
            item_id: MediaId("mapped-item".to_string()),
            provider_domain: ProviderId("domain".to_string()),
            provider_instance: ProviderInstanceId("instance".to_string()),
            available: true,
            in_library: None,
            is_unique: None,
            audio_format: None,
            url: None,
            details: None,
        }
    }

    #[test]
    fn resolves_the_direct_provider_before_a_provider_mapping() {
        assert_eq!(
            resolve_provider(&fields("library", vec![provider_mapping()])).as_deref(),
            Some("library")
        );
    }

    #[test]
    fn resolves_a_provider_mapping_when_the_direct_provider_is_empty() {
        assert_eq!(
            resolve_provider(&fields("", vec![provider_mapping()])).as_deref(),
            Some("instance")
        );
    }
}
