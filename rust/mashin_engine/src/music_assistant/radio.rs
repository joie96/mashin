//! Stateless radio selection and recommendation loading; queue writes use the caller's media command.
//! Seeds start before the radio request; this module never owns queue or autoplay state.

use super::client::MusicAssistantClient;
use super::models::identifiers::MediaId;
use super::models::media::{MediaItem, Track};
use super::models::queue::QueueOption;
use super::user_data::{LocalPlaylist, LOCAL_PLAYLIST_PROVIDER};
use rand::seq::IndexedRandom;
use std::collections::HashSet;

struct RadioSelection {
    seed: MediaItem,
    started_tracks: Vec<Track>,
}

// Playback orchestration leaves authorization and queue mutations with the active backend.
pub(crate) async fn start_radio<Command, Completion>(
    client: &MusicAssistantClient,
    media: Vec<MediaItem>,
    local_playlists: &[LocalPlaylist],
    mut play_media: Command,
) -> Result<(), String>
where
    Command: FnMut(Vec<MediaItem>, QueueOption) -> Completion,
    Completion: std::future::Future<Output = Result<(), String>>,
{
    let selection = resolve_selection(client, media, local_playlists).await?;
    let seed_uri = media_uri(&selection.seed)?;
    play_media(
        selection
            .started_tracks
            .iter()
            .cloned()
            .map(MediaItem::Track)
            .collect(),
        QueueOption::Play,
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
    local_playlists: &[LocalPlaylist],
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
        MediaItem::Playlist(playlist)
            if playlist
                .media
                .provider
                .as_str()
                .eq_ignore_ascii_case(LOCAL_PLAYLIST_PROVIDER) =>
        {
            local_playlists
                .iter()
                .find(|stored| stored.playlist.media.item_id == playlist.media.item_id)
                .ok_or("local radio playlist is no longer available")?
                .items
                .clone()
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
    let seed = if matches!(&seed, MediaItem::Playlist(playlist) if playlist.media.provider.as_str().eq_ignore_ascii_case(LOCAL_PLAYLIST_PROVIDER))
    {
        MediaItem::Track(started.clone())
    } else {
        seed
    };
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
        .map(|track| track_key(track))
        .collect::<HashSet<_>>();
    tracks
        .into_iter()
        .filter(|track| track.media.is_playable && track.media.is_available())
        .filter(|track| track.duration <= 20 * 60)
        .filter(|track| excluded.insert(track_key(track)))
        .collect()
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

#[cfg(test)]
mod tests {
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
            &[],
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
            assert!(start_radio(&client, media, &[], |_, _| async {
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
    async fn local_playlist_uses_its_selected_track_as_the_server_radio_seed() {
        let playlist: LocalPlaylist = serde_json::from_value(json!({
            "media_type":"playlist", "item_id":"local", "provider":LOCAL_PLAYLIST_PROVIDER,
            "name":"Local", "items":[track("seed")],
        }))
        .unwrap();
        let client = MusicAssistantClient::new("http://127.0.0.1:1", None);
        let selection = resolve_selection(
            &client,
            vec![MediaItem::Playlist(playlist.playlist.clone())],
            &[playlist],
        )
        .await
        .unwrap();
        assert_eq!(media_uri(&selection.seed).unwrap(), "test://track/seed");
        assert_eq!(selection.started_tracks.len(), 1);
    }

    #[tokio::test]
    async fn radio_http_contract_starts_seed_before_fetching_and_appending_recommendations() {
        use super::super::commands::QueueCommand;
        use super::super::models::identifiers::QueueId;

        for (start_status, radio_status, recommendations) in [
            (200, 200, vec![track("seed"), track("next"), track("next")]),
            (200, 200, vec![]),
            (200, 500, vec![]),
            (503, 200, vec![]),
        ] {
            let mut requests = vec![(
                "player_queues/play_media",
                media_arguments("seed", "play"),
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
                    if !recommendations.is_empty() {
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
                &[],
                |media, option| {
                    client.execute_queue_command(
                        &queue_id,
                        QueueCommand::PlayMedia {
                            media,
                            option,
                            start_item: None,
                            shuffle: false,
                        },
                    )
                },
            )
            .await;
            assert_eq!(result.is_ok(), start_status == 200 && radio_status == 200);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn container_radio_starts_a_track_and_retains_the_container_seed() {
        use super::super::commands::QueueCommand;
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
                    media_arguments("seed", "play"),
                    json!(null),
                    200,
                ),
                (
                    "music/playlists/playlist_tracks",
                    json!({
                        "item_id":format!("test://{kind}/container"), "provider_instance_id_or_domain":"radio_playlist",
                        "force_refresh":true, "allow_dynamic_tracks":true,
                    }),
                    json!([]),
                    200,
                ),
            ];
            let (url, server) = http_fixture(requests);
            let client = MusicAssistantClient::new(&url, None);
            let queue_id = QueueId("room".into());
            let media = serde_json::from_value(json!({"media_type":kind, "item_id":"container", "provider":"test", "name":"Container"})).unwrap();
            start_radio(&client, vec![media], &[], |media, option| {
                client.execute_queue_command(
                    &queue_id,
                    QueueCommand::PlayMedia {
                        media,
                        option,
                        start_item: None,
                        shuffle: false,
                    },
                )
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
