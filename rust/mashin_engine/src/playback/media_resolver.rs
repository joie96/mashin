//! Resolves playback references into engine-neutral playable media.

use crate::music_assistant::client::MusicAssistantClient;
use crate::music_assistant::models::identifiers::MediaId;
use crate::music_assistant::models::media::{MediaItem, MediaItemFields, Track};
use crate::music_assistant::user_data::LOCAL_PLAYLIST_PROVIDER;
use rand::seq::SliceRandom;
use std::collections::HashMap;

const ARTIST_TRACK_LIMIT: usize = 25;
const RADIO_ARTIST_SEED_LIMIT: usize = 20;
const RADIO_ARTIST_START_LIMIT: usize = 10;

pub(crate) struct RadioTrackSelection {
    pub(crate) started_tracks: Vec<Track>,
    pub(crate) seed_tracks: Vec<Track>,
}

// Radio starts from explicit tracks or one random track per non-track media type.
pub(crate) async fn resolve_radio_tracks(
    client: &MusicAssistantClient,
    media_items: Vec<MediaItem>,
    local_playlist_tracks: &HashMap<MediaId, Vec<Track>>,
) -> Result<RadioTrackSelection, String> {
    let mut started_tracks = Vec::new();
    let mut seed_tracks = Vec::new();
    let mut album_tracks = Vec::new();
    let mut playlist_tracks = Vec::new();
    let mut artist_start_groups = Vec::new();
    for media_item in media_items {
        match media_item {
            MediaItem::Track(track) => {
                started_tracks.push(track.clone());
                seed_tracks.push(track);
            }
            MediaItem::Playlist(playlist)
                if playlist
                    .media
                    .provider
                    .as_str()
                    .eq_ignore_ascii_case(LOCAL_PLAYLIST_PROVIDER) =>
            {
                let tracks = local_playlist_tracks
                    .get(&playlist.media.item_id)
                    .ok_or_else(|| {
                        format!(
                            "local playlist '{}' is not loaded",
                            playlist.media.item_id.as_str()
                        )
                    })?;
                playlist_tracks.extend(tracks.iter().cloned());
                seed_tracks.extend(tracks.iter().cloned());
            }
            MediaItem::Album(_) | MediaItem::Playlist(_) | MediaItem::Artist(_) => {
                let is_album = matches!(media_item, MediaItem::Album(_));
                let is_playlist = matches!(media_item, MediaItem::Playlist(_));
                let mut tracks =
                    resolve_music_assistant_media_items(client, vec![media_item]).await?;
                if is_album {
                    album_tracks.extend(tracks.iter().cloned());
                } else if is_playlist {
                    playlist_tracks.extend(tracks.iter().cloned());
                } else {
                    tracks.truncate(RADIO_ARTIST_SEED_LIMIT);
                    artist_start_groups.push(tracks.clone());
                }
                seed_tracks.extend(tracks);
            }
            _ => {}
        }
    }
    started_tracks.extend(select_radio_start_tracks(
        &album_tracks,
        &artist_start_groups,
        &playlist_tracks,
    ));
    if started_tracks.is_empty() {
        return Err("radio requires at least one playable seed track".to_string());
    }
    Ok(RadioTrackSelection {
        started_tracks,
        seed_tracks,
    })
}

fn select_radio_start_tracks(
    album_tracks: &[Track],
    artist_start_groups: &[Vec<Track>],
    playlist_tracks: &[Track],
) -> Vec<Track> {
    let mut random = rand::thread_rng();
    let mut started_tracks = Vec::new();
    if let Some(track) = album_tracks.choose(&mut random) {
        started_tracks.push(track.clone());
    }
    let playable_artists = artist_start_groups
        .iter()
        .filter(|tracks| !tracks.is_empty())
        .collect::<Vec<_>>();
    if let Some(track) = playable_artists
        .choose(&mut random)
        .and_then(|tracks| tracks[..tracks.len().min(RADIO_ARTIST_START_LIMIT)].choose(&mut random))
    {
        started_tracks.push(track.clone());
    }
    if let Some(track) = playlist_tracks.choose(&mut random) {
        started_tracks.push(track.clone());
    }
    started_tracks
}

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
    use super::{resolve_provider, resolve_radio_tracks, select_radio_start_tracks};
    use crate::music_assistant::client::MusicAssistantClient;
    use crate::music_assistant::models::identifiers::{MediaId, ProviderId, ProviderInstanceId};
    use crate::music_assistant::models::media::{
        MediaItem, MediaItemFields, ProviderMapping, Track,
    };
    use std::collections::HashMap;

    fn track(index: usize) -> Track {
        serde_json::from_value(serde_json::json!({
            "media_type": "track",
            "item_id": format!("track-{index}"),
            "provider": "library",
            "name": format!("Track {index}"),
        }))
        .expect("deserialize track")
    }

    #[tokio::test]
    async fn local_playlist_radio_starts_one_track_and_uses_every_track_as_a_seed() {
        let client = MusicAssistantClient::new("http://localhost", None);
        let playlist = serde_json::from_value(serde_json::json!({
            "media_type": "playlist",
            "item_id": "local-1",
            "provider": "mashin",
            "name": "Local playlist",
        }))
        .expect("deserialize local playlist");
        let tracks = vec![track(1), track(2), track(3)];
        let selection = resolve_radio_tracks(
            &client,
            vec![MediaItem::Playlist(playlist)],
            &HashMap::from([(MediaId("local-1".to_string()), tracks.clone())]),
        )
        .await
        .expect("resolve local playlist");

        assert_eq!(selection.seed_tracks, tracks);
        assert_eq!(selection.started_tracks.len(), 1);
        assert!(selection.seed_tracks.contains(&selection.started_tracks[0]));
    }

    #[tokio::test]
    async fn track_radio_starts_all_supplied_tracks_in_order() {
        let client = MusicAssistantClient::new("http://localhost", None);
        let tracks = vec![track(1), track(2)];
        let selection = resolve_radio_tracks(
            &client,
            tracks.iter().cloned().map(MediaItem::Track).collect(),
            &HashMap::new(),
        )
        .await
        .expect("resolve supplied tracks");

        assert_eq!(selection.started_tracks, tracks);
        assert_eq!(selection.seed_tracks, tracks);
    }

    #[test]
    fn radio_starts_one_album_and_playlist_track_and_an_artist_top_ten_track() {
        let artist_tracks = (0..20).map(track).collect::<Vec<_>>();
        let artist_groups = vec![artist_tracks];
        for _ in 0..20 {
            let selected = select_radio_start_tracks(
                &[track(20), track(21)],
                &artist_groups,
                &[track(22), track(23)],
            );
            assert_eq!(selected.len(), 3);
            assert!(["track-20", "track-21"].contains(&selected[0].media.item_id.as_str()));
            assert!(
                (0..10).any(|index| selected[1].media.item_id.as_str() == format!("track-{index}"))
            );
            assert!(["track-22", "track-23"].contains(&selected[2].media.item_id.as_str()));
        }
    }

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
