//! Stateless radio selection and recommendation loading; queue writes use the caller's media command.
//! Seeds start before the radio request; this module never owns queue or autoplay state.

use super::client::MusicAssistantClient;
use super::models::identifiers::MediaId;
use super::models::media::{MediaItem, Track};
use super::models::queue::QueueOption;
use super::user_data::same_item;
use rand::seq::IndexedRandom;

struct RadioSelection {
    seed: MediaItem,
    started_tracks: Vec<Track>,
}

// Playback orchestration leaves authorization and queue mutations with the active backend.
pub(crate) async fn start_radio<Command, Completion>(
    client: &MusicAssistantClient,
    media: Vec<MediaItem>,
    mut play_media: Command,
) -> Result<(), String>
where
    Command: FnMut(Vec<MediaItem>, QueueOption) -> Completion,
    Completion: std::future::Future<Output = Result<(), String>>,
{
    let selection = resolve_selection(client, media).await?;
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
    let candidates = client
        .get_playlist_tracks(&MediaId(seed_uri), "radio_playlist", true, true)
        .await?;
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
    client: &MusicAssistantClient,
    media: Vec<MediaItem>,
) -> Result<RadioSelection, String> {
    let seed = media
        .iter()
        .filter(|item| is_playable(item))
        .collect::<Vec<_>>()
        .choose(&mut rand::rng())
        .map(|item| (*item).clone())
        .ok_or("radio requires at least one playable media item")?;
    if !matches!(
        seed,
        MediaItem::Track(_) | MediaItem::Album(_) | MediaItem::Artist(_) | MediaItem::Playlist(_)
    ) || media
        .iter()
        .any(|item| std::mem::discriminant(item) != std::mem::discriminant(&seed))
    {
        return Err("radio requires media items of the same supported type".into());
    }
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
            client
                .get_album_tracks(&album.media.item_id, album.media.provider.as_str())
                .await?
        }
        MediaItem::Artist(artist) => {
            let top = client
                .get_artist_top_tracks(&artist.media.item_id, artist.media.provider.as_str())
                .await?;
            if top.is_empty() {
                client
                    .get_artist_tracks(&artist.media.item_id, artist.media.provider.as_str())
                    .await?
                    .into_iter()
                    .take(25)
                    .collect()
            } else {
                top.into_iter().take(25).collect()
            }
        }
        MediaItem::Playlist(playlist) => {
            client
                .get_playlist_tracks(
                    &playlist.media.item_id,
                    playlist.media.provider.as_str(),
                    false,
                    false,
                )
                .await?
        }
        _ => unreachable!(),
    };
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

// Identity and filtering helpers exclude the explicit start tracks from generated recommendations.
fn is_playable(item: &MediaItem) -> bool {
    item.fields().is_playable && item.fields().is_available()
}

fn media_uri(item: &MediaItem) -> Result<String, String> {
    let fields = item.fields();
    if let Some(uri) = fields
        .uri
        .as_ref()
        .map(|uri| uri.as_str())
        .filter(|uri| !uri.trim().is_empty())
    {
        return Ok(uri.to_owned());
    }
    let kind = match item {
        MediaItem::Track(_) => "track",
        MediaItem::Album(_) => "album",
        MediaItem::Artist(_) => "artist",
        MediaItem::Playlist(_) => "playlist",
        _ => return Err("unsupported radio seed type".into()),
    };
    if fields.provider.as_str().trim().is_empty() || fields.item_id.as_str().trim().is_empty() {
        return Err("radio seed has no Music Assistant URI".into());
    }
    Ok(format!(
        "{}://{kind}/{}",
        fields.provider.as_str(),
        fields.item_id.as_str()
    ))
}

fn filter_recommendations(tracks: Vec<Track>, started: &[Track]) -> Vec<Track> {
    let mut excluded = started
        .iter()
        .cloned()
        .map(MediaItem::Track)
        .collect::<Vec<_>>();
    let mut recommendations = Vec::new();
    for track in tracks {
        if !track.media.is_playable || !track.media.is_available() || track.duration > 20 * 60 {
            continue;
        }
        let candidate = MediaItem::Track(track.clone());
        if excluded.iter().any(|item| {
            same_item(item, &candidate)
                || item.fields().uri.as_ref().is_some_and(|uri| {
                    !uri.as_str().trim().is_empty() && candidate.fields().uri.as_ref() == Some(uri)
                })
        }) {
            continue;
        }
        excluded.push(candidate);
        recommendations.push(track);
    }
    recommendations
}

#[cfg(test)]
mod tests {
    use futures_util::TryFutureExt;
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn explicit_tracks_resolve_without_network_and_keep_their_order() {
        let media = vec![
            MediaItem::Track(track("first")),
            MediaItem::Track(track("second")),
        ];
        let selected = resolve_selection(
            &MusicAssistantClient::new("http://127.0.0.1:1", None),
            media,
        )
        .await
        .unwrap();
        assert_eq!(selected.started_tracks[0].media.item_id.as_str(), "first");
        assert_eq!(selected.started_tracks[1].media.item_id.as_str(), "second");
        assert!(matches!(selected.seed, MediaItem::Track(_)));
    }

    #[tokio::test]
    async fn invalid_radio_selections_never_start_playback() {
        let client = MusicAssistantClient::new("http://127.0.0.1:1", None);
        let album: MediaItem = serde_json::from_value(
            json!({"media_type":"album", "item_id":"album", "provider":"test", "name":"Album"}),
        )
        .unwrap();
        for media in [vec![], vec![MediaItem::Track(track("seed")), album]] {
            assert!(start_radio(&client, media, |_, _| async {
                panic!("invalid selection must not start playback")
            })
            .await
            .is_err());
        }
    }

    #[test]
    fn radio_uri_and_recommendation_filter_preserve_the_previous_contract() {
        let seed = track("seed");
        assert_eq!(
            media_uri(&MediaItem::Track(seed.clone())).unwrap(),
            "test://track/seed"
        );
        let mut unavailable = track("unavailable");
        unavailable.media.available = Some(false);
        let mut long = track("long");
        long.duration = 1201;
        let filtered = filter_recommendations(
            vec![
                seed.clone(),
                track("next"),
                track("next"),
                unavailable,
                long,
            ],
            &[seed],
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].media.item_id.as_str(), "next");
    }

    #[tokio::test]
    async fn private_playlist_resolves_tracks_through_the_normal_ma_endpoint() {
        let playlist = serde_json::from_value(json!({
            "media_type":"playlist", "item_id":"42", "provider":"library", "name":"Private",
        }))
        .unwrap();
        let (client, worker) = super::super::client::tests::server(vec![(
            "music/playlists/playlist_tracks",
            json!([track("seed")]),
        )]);
        let selection = resolve_selection(&client, vec![MediaItem::Playlist(playlist)])
            .await
            .unwrap();
        assert_eq!(media_uri(&selection.seed).unwrap(), "library://playlist/42");
        assert_eq!(selection.started_tracks.len(), 1);
        worker.join().unwrap();
    }

    #[test]
    fn radio_filter_excludes_seeds_with_uri_and_provider_aliases() {
        let seed: Track = serde_json::from_value(json!({
            "item_id":"42", "provider":"library", "name":"Seed", "duration":200,
            "provider_mappings":[{
                "item_id":"source", "provider_instance":"test-one", "provider_domain":"test"
            }]
        }))
        .unwrap();
        let library_result: Track = serde_json::from_value(json!({
            "item_id":"42", "provider":"library", "uri":"library://track/42",
            "name":"Seed", "duration":200
        }))
        .unwrap();
        let provider_result: Track = serde_json::from_value(json!({
            "item_id":"source", "provider":"test-one", "name":"Seed", "duration":200
        }))
        .unwrap();
        let filtered = filter_recommendations(
            vec![library_result, provider_result, track("next")],
            &[seed],
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].media.item_id.as_str(), "next");
    }

    #[test]
    fn radio_filter_excludes_all_seeds_and_deduplicates_provider_aliases() {
        let library_track: Track = serde_json::from_value(json!({
            "item_id":"42", "provider":"library", "name":"Mapped", "duration":200,
            "provider_mappings":[{
                "item_id":"source", "provider_instance":"test", "provider_domain":"test"
            }]
        }))
        .unwrap();
        let source = track("source");
        let other_seed = track("other-seed");
        let filtered = filter_recommendations(
            vec![library_track.clone(), other_seed.clone(), track("next")],
            &[source.clone(), other_seed],
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].media.item_id.as_str(), "next");
        for candidates in [
            vec![source.clone(), library_track.clone()],
            vec![library_track, source],
        ] {
            assert_eq!(filter_recommendations(candidates, &[]).len(), 1);
        }
    }

    #[tokio::test]
    async fn radio_http_contract_replaces_queue_before_fetching_and_appending_recommendations() {
        use super::super::client::QueueCommand;
        use super::super::models::identifiers::QueueId;

        let mut seed_with_uri = track("seed");
        seed_with_uri.media.uri = Some(super::super::models::identifiers::MediaUri(
            "test://track/seed".into(),
        ));
        for (start_status, radio_status, recommendations) in [
            (
                200,
                200,
                vec![seed_with_uri.clone(), track("next"), track("next")],
            ),
            (200, 200, vec![seed_with_uri]),
            (200, 200, vec![]),
            (200, 500, vec![]),
            (503, 200, vec![]),
        ] {
            let mut requests = vec![(
                "player_queues/play_media",
                media_arguments("seed", "replace"),
                json!(null),
                start_status,
            )];
            if start_status == 200 {
                requests.push(("music/playlists/playlist_tracks", json!({
                    "item_id":"test://track/seed", "provider_instance_id_or_domain":"radio_playlist",
                    "force_refresh":true, "allow_dynamic_tracks":true,
                }), json!(recommendations), radio_status));
                if radio_status == 200 {
                    requests.push(("providers", json!({}), json!([]), 200));
                    if recommendations
                        .iter()
                        .any(|track| track.media.item_id.as_str() == "next")
                    {
                        requests.push((
                            "player_queues/play_media",
                            media_arguments("next", "add"),
                            json!(null),
                            200,
                        ));
                    }
                }
            }
            let (url, server) = http_fixture(requests);
            let client = MusicAssistantClient::new(&url, None);
            let queue_id = QueueId("room".into());
            let result = start_radio(
                &client,
                vec![MediaItem::Track(track("seed"))],
                |media, option| {
                    client.execute_queue_command(
                        &queue_id,
                        QueueCommand::PlayMedia {
                            media,
                            option,
                            start_item: None,
                            shuffle: false,
                        },
                    ).map_err(String::from)
                },
            )
            .await;
            assert_eq!(result.is_ok(), start_status == 200 && radio_status == 200);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn container_radio_replaces_queue_and_retains_the_container_seed() {
        use super::super::client::QueueCommand;
        use super::super::models::identifiers::QueueId;

        for (kind, command) in [
            ("album", "music/albums/album_tracks"),
            ("artist", "music/artists/top_tracks"),
            ("playlist", "music/playlists/playlist_tracks"),
        ] {
            let mut arguments =
                json!({"item_id":"container", "provider_instance_id_or_domain":"test"});
            if kind == "playlist" {
                arguments["force_refresh"] = json!(false);
                arguments["allow_dynamic_tracks"] = json!(false);
            }
            let requests = vec![
                (command, arguments, json!([track("seed")]), 200),
                ("providers", json!({}), json!([]), 200),
                (
                    "player_queues/play_media",
                    media_arguments("seed", "replace"),
                    json!(null),
                    200,
                ),
                (
                    "music/playlists/playlist_tracks",
                    json!({
                        "item_id":format!("test://{kind}/container"), "provider_instance_id_or_domain":"radio_playlist",
                        "force_refresh":true, "allow_dynamic_tracks":true,
                    }),
                    json!([track("seed"), track("next")]),
                    200,
                ),
                (
                    "player_queues/play_media",
                    media_arguments("next", "add"),
                    json!(null),
                    200,
                ),
            ];
            let (url, server) = http_fixture(requests);
            let client = MusicAssistantClient::new(&url, None);
            let queue_id = QueueId("room".into());
            let media = serde_json::from_value(json!({"media_type":kind, "item_id":"container", "provider":"test", "name":"Container"})).unwrap();
            start_radio(&client, vec![media], |media, option| {
                client.execute_queue_command(
                    &queue_id,
                    QueueCommand::PlayMedia {
                        media,
                        option,
                        start_item: None,
                        shuffle: false,
                    },
                ).map_err(String::from)
            })
            .await
            .unwrap();
            server.await.unwrap();
        }
    }

    // HTTP fixtures exercise production request encoding and require each previous request to finish.
    fn http_fixture(
        requests: Vec<(&'static str, serde_json::Value, serde_json::Value, u16)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let worker = tokio::spawn(async move {
            for (command, arguments, response, status) in requests {
                let (socket, _) =
                    tokio::time::timeout(std::time::Duration::from_secs(3), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let mut reader = BufReader::new(socket);
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    assert!(reader.read_line(&mut header).await.unwrap() > 0);
                    if header == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).await.unwrap();
                let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(request, json!({"command":command, "args":arguments}));
                let body = response.to_string();
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .unwrap();
            }
        });
        (url, worker)
    }

    fn media_arguments(identifier: &str, option: &str) -> serde_json::Value {
        json!({"queue_id":"room", "media":[super::super::client::queue_media_payload(MediaItem::Track(track(identifier)))],
            "option":option, "start_item":null, "sort_by":"original"})
    }

    fn track(identifier: &str) -> Track {
        serde_json::from_value(json!({"media_type":"track", "item_id":identifier, "provider":"test", "name":identifier, "duration":200, "available":true})).unwrap()
    }
}
