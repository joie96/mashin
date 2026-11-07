//! Stateless radio selection and recommendation loading; queue writes use the caller's media command.
//! Seeds start before the radio request; this module never owns queue or autoplay state.

use crate::application::repositories::CatalogClient;
use crate::domain::catalog::{CatalogQuery, CatalogValue};
use crate::domain::identifiers::MediaId;
use crate::domain::media::{MediaItem, Track};
use crate::domain::providers::{
    project_item_availability, project_track_availability, ProviderInstance,
};
use crate::domain::radio::{
    choose_seed, filter_recommendations, is_playable, media_uri, RadioSelection,
};
use crate::playback::models::QueueOption;
use rand::seq::IndexedRandom;

// Playback orchestration leaves authorization and queue mutations with the active backend.
pub(crate) async fn start_radio<Command, Completion>(
    catalog: &dyn CatalogClient,
    providers: Option<&[ProviderInstance]>,
    media: Vec<MediaItem>,
    mut play_media: Command,
) -> Result<(), String>
where
    Command: FnMut(Vec<MediaItem>, QueueOption) -> Completion,
    Completion: std::future::Future<Output = Result<(), String>>,
{
    let selection = resolve_selection(catalog, providers, media).await?;
    let seed_uri = media_uri(&selection.seed)?;
    play_media(
        selection
            .started_tracks
            .iter()
            .cloned()
            .map(MediaItem::Track)
            .collect(),
        QueueOption::Replace,
    )
    .await?;
    let mut candidates = load_tracks(
        catalog,
        CatalogQuery::PlaylistTracks {
            item_id: MediaId(seed_uri),
            provider: "radio_playlist".into(),
            allow_dynamic_tracks: true,
        },
        true,
    )
    .await?;
    for track in &mut candidates {
        project_track_availability(track, providers);
    }
    let tracks = filter_recommendations(candidates, &selection.started_tracks);
    if !tracks.is_empty() {
        play_media(
            tracks.into_iter().map(MediaItem::Track).collect(),
            QueueOption::Add,
        )
        .await?;
    }
    Ok(())
}

// Seed resolution preserves the pre-Mashin radio selection rules.
async fn resolve_selection(
    catalog: &dyn CatalogClient,
    providers: Option<&[ProviderInstance]>,
    mut media: Vec<MediaItem>,
) -> Result<RadioSelection, String> {
    for item in &mut media {
        project_item_availability(item, providers);
    }
    let seed = choose_seed(&media, &mut rand::rng())?;
    if matches!(seed, MediaItem::Track(_)) {
        return Ok(RadioSelection {
            seed,
            started_tracks: media
                .into_iter()
                .filter(|item| is_playable(item))
                .filter_map(|item| {
                    if let MediaItem::Track(track) = item {
                        Some(track)
                    } else {
                        None
                    }
                })
                .collect(),
        });
    }
    let mut tracks = match &seed {
        MediaItem::Album(album) => {
            load_tracks(
                catalog,
                CatalogQuery::AlbumTracks {
                    item_id: album.media.item_id.clone(),
                    provider: album.media.provider.as_str().into(),
                },
                false,
            )
            .await?
        }
        MediaItem::Artist(artist) => {
            let top = load_tracks(
                catalog,
                CatalogQuery::ArtistTopTracks {
                    item_id: artist.media.item_id.clone(),
                    provider: artist.media.provider.as_str().into(),
                },
                false,
            )
            .await?;
            if top.is_empty() {
                load_tracks(
                    catalog,
                    CatalogQuery::ArtistTracks {
                        item_id: artist.media.item_id.clone(),
                        provider: artist.media.provider.as_str().into(),
                    },
                    false,
                )
                .await?
                .into_iter()
                .take(25)
                .collect()
            } else {
                top.into_iter().take(25).collect()
            }
        }
        MediaItem::Playlist(playlist) => {
            load_tracks(
                catalog,
                CatalogQuery::PlaylistTracks {
                    item_id: playlist.media.item_id.clone(),
                    provider: playlist.media.provider.as_str().into(),
                    allow_dynamic_tracks: false,
                },
                false,
            )
            .await?
        }
        _ => unreachable!(),
    };
    for track in &mut tracks {
        project_track_availability(track, providers);
    }
    tracks.retain(|track| track.media.is_playable && track.media.is_available());
    if matches!(seed, MediaItem::Artist(_)) {
        tracks.truncate(10);
    }
    let started = tracks
        .choose(&mut rand::rng())
        .cloned()
        .ok_or("radio requires at least one playable seed track")?;
    Ok(RadioSelection {
        seed,
        started_tracks: vec![started],
    })
}

async fn load_tracks(
    catalog: &dyn CatalogClient,
    query: CatalogQuery,
    force_refresh: bool,
) -> Result<Vec<Track>, String> {
    match catalog.load_catalog(query, force_refresh).await? {
        CatalogValue::Tracks(items) => Ok(items),
        _ => Err("radio catalog request did not return tracks".into()),
    }
}
