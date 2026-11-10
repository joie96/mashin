//! Stateless radio selection and recommendation loading; queue writes use the caller's media command.
//! Seeds start before the radio request; this module never owns queue or autoplay state.
//! Seed lists use completed catalog reads; dynamic recommendations always require a new sample.

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
    let mut candidates = load_completed_tracks(
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
            load_completed_tracks(
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
            let top = load_completed_tracks(
                catalog,
                CatalogQuery::ArtistTopTracks {
                    item_id: artist.media.item_id.clone(),
                    provider: artist.media.provider.as_str().into(),
                },
                false,
            )
            .await?;
            if top.is_empty() {
                load_completed_tracks(
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
            load_completed_tracks(
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

// Command preparation must not mistake partial or still-refreshing seed data for completion.
async fn load_completed_tracks(
    catalog: &dyn CatalogClient,
    query: CatalogQuery,
    force_refresh: bool,
) -> Result<Vec<Track>, String> {
    match catalog.load_catalog(query, force_refresh).await? {
        CatalogValue::Tracks(items) => Ok(items),
        _ => Err("radio catalog request did not return tracks".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{future::BoxFuture, FutureExt};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    // Controlled catalog completions keep queue effects distinguishable from read starts.
    struct RadioCatalog {
        reads: Mutex<Vec<(CatalogQuery, bool)>>,
        responses: Mutex<VecDeque<tokio::sync::oneshot::Receiver<Result<CatalogValue, String>>>>,
    }

    impl CatalogClient for RadioCatalog {
        fn begin_startup(&self) -> Box<dyn Send> {
            Box::new(())
        }

        fn load_catalog(
            &self,
            query: CatalogQuery,
            force_refresh: bool,
        ) -> BoxFuture<'_, Result<CatalogValue, String>> {
            self.reads.lock().unwrap().push((query, force_refresh));
            let response = self.responses.lock().unwrap().pop_front().unwrap();
            Box::pin(async move { response.await.map_err(|error| error.to_string())? })
        }
    }

    fn track(id: &str) -> Track {
        let MediaItem::Track(track) = serde_json::from_value(serde_json::json!({
            "media_type":"track", "item_id":id, "provider":"source", "name":id, "duration":180,
        }))
        .unwrap() else {
            unreachable!()
        };
        track
    }

    // Radio consumes completed command inputs, never a cached dynamic sample before refresh.
    #[test]
    fn dynamic_radio_waits_for_forced_completion_and_never_appends_on_failure() {
        for outcome in 0..3 {
            crate::application::tests::runtime().block_on(async {
                let (send, receive) = tokio::sync::oneshot::channel();
                let catalog = RadioCatalog { reads: Mutex::new(Vec::new()), responses: Mutex::new(VecDeque::from([receive])) };
                let commands = Mutex::new(Vec::new());
                let mut operation = Box::pin(start_radio(&catalog, None, vec![MediaItem::Track(track("seed"))], |media, option| {
                    commands.lock().unwrap().push((media, option));
                    futures_util::future::ready(Ok(()))
                }));
                assert!(operation.as_mut().now_or_never().is_none());
                assert_eq!(commands.lock().unwrap().len(), 1);
                assert_eq!(commands.lock().unwrap()[0].1, QueueOption::Replace);
                let reads = catalog.reads.lock().unwrap().clone();
                assert!(matches!(reads.as_slice(), [(CatalogQuery::PlaylistTracks { item_id, provider, allow_dynamic_tracks: true }, true)]
                    if item_id.as_str() == "source://track/seed" && provider == "radio_playlist"));
                send.send(match outcome {
                    0 => Ok(CatalogValue::Tracks(vec![track("seed"), track("next"), track("next") ])),
                    1 => Ok(CatalogValue::Tracks(Vec::new())),
                    _ => Err("radio refresh failed".into()),
                }).unwrap();
                let result = operation.await;
                assert_eq!(result.is_err(), outcome == 2);
                let commands = commands.lock().unwrap();
                assert_eq!(commands.len(), if outcome == 0 { 2 } else { 1 });
                if outcome == 0 {
                    assert_eq!(commands[1].1, QueueOption::Add);
                    assert_eq!(commands[1].0.len(), 1);
                    assert_eq!(commands[1].0[0].fields().item_id.as_str(), "next");
                }
            });
        }
    }

    #[test]
    fn static_radio_seed_lists_complete_before_queue_replacement() {
        for kind in ["album", "playlist"] {
            for outcome in 0..3 {
                crate::application::tests::runtime().block_on(async {
                    let (send_seed, receive_seed) = tokio::sync::oneshot::channel();
                    let (send_dynamic, receive_dynamic) = tokio::sync::oneshot::channel();
                    let catalog = RadioCatalog { reads: Mutex::new(Vec::new()), responses: Mutex::new(VecDeque::from([receive_seed, receive_dynamic])) };
                    let commands = Mutex::new(Vec::new());
                    let seed = serde_json::from_value(serde_json::json!({"media_type":kind,"item_id":"selected","provider":"source","name":"Seed"})).unwrap();
                    let mut operation = Box::pin(start_radio(&catalog, None, vec![seed], |media, option| {
                        commands.lock().unwrap().push((media, option));
                        futures_util::future::ready(Ok(()))
                    }));
                    assert!(operation.as_mut().now_or_never().is_none());
                    assert!(commands.lock().unwrap().is_empty());
                    let expected = if kind == "album" {
                        CatalogQuery::AlbumTracks { item_id: MediaId("selected".into()), provider: "source".into() }
                    } else {
                        CatalogQuery::PlaylistTracks { item_id: MediaId("selected".into()), provider: "source".into(), allow_dynamic_tracks: false }
                    };
                    assert_eq!(*catalog.reads.lock().unwrap(), vec![(expected, false)]);
                    send_seed.send(match outcome {
                        0 => Ok(CatalogValue::Tracks(vec![track("seed")])),
                        1 => Ok(CatalogValue::Tracks(Vec::new())),
                        _ => Err("seed refresh failed".into()),
                    }).unwrap();
                    if outcome == 0 {
                        assert!(operation.as_mut().now_or_never().is_none());
                        assert_eq!(commands.lock().unwrap().len(), 1);
                        assert_eq!(commands.lock().unwrap()[0].1, QueueOption::Replace);
                        assert!(matches!(catalog.reads.lock().unwrap().last(), Some((CatalogQuery::PlaylistTracks { provider, allow_dynamic_tracks: true, .. }, true)) if provider == "radio_playlist"));
                        send_dynamic.send(Ok(CatalogValue::Tracks(Vec::new()))).unwrap();
                        operation.await.unwrap();
                    } else {
                        assert!(operation.await.is_err());
                        assert!(commands.lock().unwrap().is_empty());
                        assert_eq!(catalog.reads.lock().unwrap().len(), 1);
                    }
                });
            }
        }
    }

    #[test]
    fn artist_radio_fallback_waits_for_completed_top_tracks_and_stops_on_error() {
        for top_fails in [false, true] {
            crate::application::tests::runtime().block_on(async {
                let (send_top, receive_top) = tokio::sync::oneshot::channel();
                let (send_fallback, receive_fallback) = tokio::sync::oneshot::channel();
                let (send_dynamic, receive_dynamic) = tokio::sync::oneshot::channel();
                let catalog = RadioCatalog { reads: Mutex::new(Vec::new()), responses: Mutex::new(VecDeque::from([receive_top, receive_fallback, receive_dynamic])) };
                let commands = Mutex::new(Vec::new());
                let seed = serde_json::from_value(serde_json::json!({"media_type":"artist","item_id":"selected","provider":"source","name":"Artist"})).unwrap();
                let mut operation = Box::pin(start_radio(&catalog, None, vec![seed], |media, option| {
                    commands.lock().unwrap().push((media, option));
                    futures_util::future::ready(Ok(()))
                }));
                assert!(operation.as_mut().now_or_never().is_none());
                assert_eq!(*catalog.reads.lock().unwrap(), vec![(CatalogQuery::ArtistTopTracks { item_id: MediaId("selected".into()), provider: "source".into() }, false)]);
                assert!(commands.lock().unwrap().is_empty());
                send_top.send(if top_fails { Err("top tracks failed".into()) } else { Ok(CatalogValue::Tracks(Vec::new())) }).unwrap();
                if top_fails {
                    assert!(operation.await.is_err());
                    assert!(commands.lock().unwrap().is_empty());
                    assert_eq!(catalog.reads.lock().unwrap().len(), 1);
                    return;
                }
                assert!(operation.as_mut().now_or_never().is_none());
                assert!(commands.lock().unwrap().is_empty());
                assert!(matches!(catalog.reads.lock().unwrap().last(), Some((CatalogQuery::ArtistTracks { item_id, provider }, false)) if item_id.as_str() == "selected" && provider == "source"));
                send_fallback.send(Ok(CatalogValue::Tracks(vec![track("seed")]))).unwrap();
                assert!(operation.as_mut().now_or_never().is_none());
                assert_eq!(commands.lock().unwrap().len(), 1);
                send_dynamic.send(Ok(CatalogValue::Tracks(vec![track("next")]))).unwrap();
                operation.await.unwrap();
                let commands = commands.lock().unwrap();
                assert_eq!(commands.len(), 2);
                assert_eq!(commands[1].1, QueueOption::Add);
            });
        }
    }
}
