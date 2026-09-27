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
    pub(crate) autoplay_seed_tracks: Vec<Track>,
    pub(crate) radio_seed: MediaItem,
}

// Radio starts from explicit tracks or a track in one randomly chosen container.
pub(crate) async fn resolve_radio_tracks(
    client: &MusicAssistantClient,
    media_items: Vec<MediaItem>,
    local_playlist_tracks: &HashMap<MediaId, Vec<Track>>,
) -> Result<RadioTrackSelection, String> {
    let radio_seed = select_radio_seed(&media_items)?;
    if matches!(radio_seed, MediaItem::Track(_)) {
        let started_tracks = media_items
            .into_iter()
            .filter_map(|item| match item {
                MediaItem::Track(track) => Some(track),
                _ => None,
            })
            .collect::<Vec<_>>();
        let MediaItem::Track(seed_track) = &radio_seed else {
            unreachable!()
        };
        return Ok(RadioTrackSelection {
            started_tracks,
            autoplay_seed_tracks: vec![seed_track.clone()],
            radio_seed,
        });
    }

    let mut source_tracks = match &radio_seed {
        MediaItem::Playlist(playlist)
            if playlist
                .media
                .provider
                .as_str()
                .eq_ignore_ascii_case(LOCAL_PLAYLIST_PROVIDER) =>
        {
            local_playlist_tracks
                .get(&playlist.media.item_id)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "local playlist '{}' is not loaded",
                        playlist.media.item_id.as_str()
                    )
                })?
        }
        _ => resolve_music_assistant_media_items(client, vec![radio_seed.clone()]).await?,
    };
    if matches!(radio_seed, MediaItem::Artist(_)) {
        source_tracks.truncate(RADIO_ARTIST_SEED_LIMIT);
    }
    let started_track = select_radio_start_track(&radio_seed, &source_tracks)?;
    let radio_seed = if matches!(&radio_seed, MediaItem::Playlist(playlist) if playlist.media.provider.as_str().eq_ignore_ascii_case(LOCAL_PLAYLIST_PROVIDER))
    {
        MediaItem::Track(started_track.clone())
    } else {
        radio_seed
    };
    Ok(RadioTrackSelection {
        started_tracks: vec![started_track],
        autoplay_seed_tracks: source_tracks,
        radio_seed,
    })
}

fn select_radio_seed(media_items: &[MediaItem]) -> Result<MediaItem, String> {
    let radio_seed = media_items
        .choose(&mut rand::thread_rng())
        .cloned()
        .ok_or_else(|| "radio requires at least one media item".to_string())?;
    if !matches!(
        radio_seed,
        MediaItem::Track(_) | MediaItem::Album(_) | MediaItem::Artist(_) | MediaItem::Playlist(_)
    ) || media_items
        .iter()
        .any(|item| std::mem::discriminant(item) != std::mem::discriminant(&radio_seed))
    {
        return Err("radio requires media items of the same supported type".to_string());
    }
    Ok(radio_seed)
}

fn select_radio_start_track(
    radio_seed: &MediaItem,
    seed_tracks: &[Track],
) -> Result<Track, String> {
    let start_candidates = if matches!(radio_seed, MediaItem::Artist(_)) {
        &seed_tracks[..seed_tracks.len().min(RADIO_ARTIST_START_LIMIT)]
    } else {
        seed_tracks
    };
    start_candidates
        .choose(&mut rand::thread_rng())
        .cloned()
        .ok_or_else(|| "radio requires at least one playable seed track".to_string())
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
    use super::{
        resolve_provider, resolve_radio_tracks, select_radio_seed, select_radio_start_track,
    };
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
    async fn local_playlist_radio_starts_one_track_and_keeps_all_tracks_for_autoplay() {
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

        assert_eq!(selection.autoplay_seed_tracks, tracks);
        assert_eq!(selection.started_tracks.len(), 1);
        assert!(selection
            .autoplay_seed_tracks
            .contains(&selection.started_tracks[0]));
        assert_eq!(
            selection.radio_seed,
            MediaItem::Track(selection.started_tracks[0].clone())
        );
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
        assert_eq!(selection.autoplay_seed_tracks.len(), 1);
        assert_eq!(
            selection.radio_seed,
            MediaItem::Track(selection.autoplay_seed_tracks[0].clone())
        );
        assert!(tracks.contains(&selection.autoplay_seed_tracks[0]));
    }

    #[test]
    fn radio_chooses_one_supplied_container_and_one_of_its_tracks() {
        for media_type in ["album", "playlist", "artist"] {
            let items = (0..2)
                .map(|index| {
                    serde_json::from_value(serde_json::json!({
                        "media_type": media_type,
                        "item_id": format!("item-{index}"),
                        "provider": "library",
                        "name": format!("Item {index}"),
                    }))
                    .expect("deserialize radio seed")
                })
                .collect::<Vec<MediaItem>>();
            let tracks = (0..20).map(track).collect::<Vec<_>>();
            for _ in 0..20 {
                let selected = select_radio_seed(&items).expect("select radio seed");
                assert!(items.contains(&selected));
                let started =
                    select_radio_start_track(&selected, &tracks).expect("select start track");
                let limit = if media_type == "artist" { 10 } else { 20 };
                assert!(tracks[..limit].contains(&started));
            }
        }
    }

    #[test]
    fn radio_rejects_mixed_media_types() {
        let album = serde_json::from_value(serde_json::json!({
            "media_type": "album", "item_id": "album-1", "provider": "library", "name": "Album",
        }))
        .expect("deserialize album");
        assert!(select_radio_seed(&[MediaItem::Track(track(1)), album]).is_err());
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
