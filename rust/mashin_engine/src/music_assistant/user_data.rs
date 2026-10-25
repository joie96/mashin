//! Session-owned MA favorites, private playlists, and volatile favorite synchronization.

use super::client::MusicAssistantClient;
use super::models::identifiers::MediaId;
use super::models::media::{Album, Artist, MediaItem, Playlist, PlaylistSharing, Track};
use super::playlist_collage::build_playlist_collage;
use futures_util::{stream, StreamExt, TryStreamExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const PAGE_SIZE: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Favorites {
    #[serde(default)]
    pub(crate) tracks: Vec<Track>,
    #[serde(default)]
    pub(crate) albums: Vec<Album>,
    #[serde(default)]
    pub(crate) playlists: Vec<Playlist>,
    #[serde(default)]
    pub(crate) artists: Vec<Artist>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PrivatePlaylist {
    #[serde(flatten)]
    pub(crate) playlist: Playlist,
    #[serde(default)]
    pub(crate) items: Vec<Track>,
    #[serde(default)]
    pub(crate) is_loading_tracks: bool,
}

impl PrivatePlaylist {
    // Metadata-only events must not regenerate randomized artwork or trigger write loops.
    fn has_same_tracks(&self, other: &Self) -> bool {
        self.items.len() == other.items.len()
            && self.items.iter().zip(&other.items).all(|(left, right)| {
                match (&left.media.uri, &right.media.uri) {
                    (Some(left), Some(right)) => left == right,
                    _ => {
                        left.media.provider == right.media.provider
                            && left.media.item_id == right.media.item_id
                    }
                }
            })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct UserDataSnapshot {
    #[serde(default)]
    pub(crate) favorites: Favorites,
    #[serde(default)]
    pub(crate) playlists: Vec<PrivatePlaylist>,
    #[serde(default)]
    pub(crate) is_loading_playlists: bool,
}

impl UserDataSnapshot {
    // A targeted refresh must preserve unrelated playlists and their loaded contents.
    pub(crate) fn replace_playlist(
        &mut self,
        playlist_id: &str,
        playlist: Option<PrivatePlaylist>,
    ) {
        let position = self
            .playlists
            .iter()
            .position(|entry| entry.playlist.media.item_id.as_str() == playlist_id);
        match (position, playlist) {
            (Some(position), Some(playlist)) => self.playlists[position] = playlist,
            (Some(position), None) => {
                self.playlists.remove(position);
            }
            (None, Some(playlist)) => self.playlists.push(playlist),
            (None, None) => {}
        }
    }
}

impl MusicAssistantClient {
    // Snapshot assembly loads typed MA pages without holding application locks.
    pub(crate) async fn load_library_user_data(
        &self,
        user_id: &str,
    ) -> Result<UserDataSnapshot, String> {
        let favorites = self.load_favorites().await?;
        let playlists = self.load_private_playlists(user_id).await?;
        Ok(UserDataSnapshot {
            favorites,
            playlists,
            is_loading_playlists: false,
        })
    }

    pub(crate) async fn load_favorites(&self) -> Result<Favorites, String> {
        let (tracks, albums, artists, playlists) = tokio::try_join!(
            self.favorite_pages("tracks"),
            self.favorite_pages("albums"),
            self.favorite_pages("artists"),
            self.favorite_pages("playlists"),
        )?;
        Ok(Favorites {
            tracks,
            albums,
            artists,
            playlists,
        })
    }

    pub(crate) async fn load_private_playlists(
        &self,
        user_id: &str,
    ) -> Result<Vec<PrivatePlaylist>, String> {
        let playlists = self.load_private_playlist_metadata(user_id).await?;
        stream::iter(playlists.into_iter().map(|mut playlist| async move {
            playlist.items = self
                .get_playlist_tracks(&playlist.playlist.media.item_id, "library", true, false)
                .await?;
            playlist.is_loading_tracks = false;
            Ok(playlist)
        }))
        .buffered(3)
        .try_collect()
        .await
    }

    pub(crate) async fn load_private_playlist_metadata(
        &self,
        user_id: &str,
    ) -> Result<Vec<PrivatePlaylist>, String> {
        let mut playlists = Vec::new();
        let library_playlists: Vec<Playlist> = self.library_pages("playlists", None).await?;
        for owned in library_playlists {
            if !owned.access.as_ref().is_some_and(|access| {
                access.owner.as_deref() == Some(user_id)
                    && access.sharing == PlaylistSharing::Private
            }) {
                continue;
            }
            playlists.push(PrivatePlaylist {
                playlist: owned,
                items: Vec::new(),
                is_loading_tracks: true,
            });
        }
        Ok(playlists)
    }

    pub(crate) async fn load_private_playlist(
        &self,
        playlist_id: &str,
        user_id: &str,
    ) -> Result<Option<PrivatePlaylist>, String> {
        let playlist = self
            .get_playlist(&MediaId(playlist_id.to_owned()), "library")
            .await?;
        if !playlist.access.as_ref().is_some_and(|access| {
            access.owner.as_deref() == Some(user_id) && access.sharing == PlaylistSharing::Private
        }) {
            return Ok(None);
        }
        let items = self
            .get_playlist_tracks(&playlist.media.item_id, "library", true, false)
            .await?;
        Ok(Some(PrivatePlaylist {
            playlist,
            items,
            is_loading_tracks: false,
        }))
    }

    pub(crate) async fn refresh_playlist_collages(
        &self,
        previous: &UserDataSnapshot,
        snapshot: &mut UserDataSnapshot,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        for current in &mut snapshot.playlists {
            let Some(old) = previous
                .playlists
                .iter()
                .find(|old| old.playlist.media.item_id == current.playlist.media.item_id)
            else {
                continue;
            };
            if old.has_same_tracks(current) || !current.playlist.is_editable {
                continue;
            }
            if let Err(error) = self.refresh_private_playlist_cover(current).await {
                errors.push(format!(
                    "playlist {} artwork update failed: {error}",
                    current.playlist.media.item_id.as_str()
                ));
            }
        }
        errors
    }

    pub(crate) async fn refresh_private_playlist_cover(
        &self,
        playlist: &mut PrivatePlaylist,
    ) -> Result<(), String> {
        if !playlist.playlist.is_editable {
            return Err("playlist is not editable".into());
        }
        let image = build_playlist_collage(&playlist.items, self.base_url()).await;
        if image.is_none() && !playlist.items.is_empty() {
            return Err("playlist has no usable artwork".into());
        }
        playlist.playlist = self
            .set_private_playlist_image(&playlist.playlist, image)
            .await?;
        Ok(())
    }

    // Full-list queries retain stable ordering across pages.
    async fn favorite_pages<Item: DeserializeOwned>(
        &self,
        media_type: &str,
    ) -> Result<Vec<Item>, String> {
        self.library_pages(media_type, Some(true)).await
    }

    async fn library_pages<Item: DeserializeOwned>(
        &self,
        media_type: &str,
        favorite: Option<bool>,
    ) -> Result<Vec<Item>, String> {
        let mut items = Vec::new();
        loop {
            let page = self
                .get_library_page(media_type, favorite, PAGE_SIZE, items.len())
                .await?;
            let is_last_page = page.len() < PAGE_SIZE;
            items.extend(page);
            if is_last_page {
                return Ok(items);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LibraryUserData {
    pub(crate) snapshot: UserDataSnapshot,
    pub(crate) has_loaded_library_data: bool,
    pub(crate) sync: FavoriteSync,
    pub(crate) revision: u64,
    pending_items: HashMap<String, MediaItem>,
}

impl LibraryUserData {
    // Optimistic commands share one worker for provider and library identities.
    pub(crate) fn submit(
        &mut self,
        item: MediaItem,
        is_favorite: bool,
    ) -> (String, MediaItem, bool) {
        self.revision = self.revision.wrapping_add(1);
        let (identity, canonical) = if let Some((identity, pending)) = self
            .pending_items
            .iter()
            .find(|(_, candidate)| same_item(candidate, &item))
        {
            (identity.clone(), pending.clone())
        } else {
            let canonical = self
                .snapshot
                .favorites
                .items()
                .into_iter()
                .find(|candidate| same_item(candidate, &item))
                .unwrap_or(item);
            (identity(&canonical), canonical)
        };
        self.snapshot
            .favorites
            .set_item(canonical.clone(), is_favorite);
        self.pending_items
            .insert(identity.clone(), canonical.clone());
        let should_start = self.sync.submit(identity.clone(), is_favorite);
        (identity, canonical, should_start)
    }

    pub(crate) fn finish(
        &mut self,
        identity: &str,
        written_state: bool,
        confirmed: Option<MediaItem>,
    ) -> bool {
        self.revision = self.revision.wrapping_add(1);
        let pending = self.pending_items.get(identity).cloned();
        if let Some(item) = &pending {
            self.snapshot.favorites.set_item(item.clone(), false);
        }
        let is_finished = self.sync.finish(identity, written_state);
        if let Some(item) = confirmed.or(pending) {
            let is_favorite = if is_finished {
                item.fields().favorite == Some(true)
            } else {
                self.sync.desired_state(identity).unwrap_or(false)
            };
            self.snapshot.favorites.set_item(item.clone(), is_favorite);
            if !is_finished {
                self.pending_items.insert(identity.to_owned(), item);
            }
        }
        if is_finished {
            self.pending_items.remove(identity);
        }
        is_finished
    }

    pub(crate) fn replace_snapshot(&mut self, mut snapshot: UserDataSnapshot) {
        for (identity, item) in &self.pending_items {
            if let Some(is_favorite) = self.sync.desired_state(identity) {
                snapshot.favorites.set_item(item.clone(), is_favorite);
            }
        }
        self.snapshot = snapshot;
    }
}

impl Favorites {
    // Snapshot projection and identity matching never perform network requests.
    pub(crate) fn items(&self) -> Vec<MediaItem> {
        self.tracks
            .iter()
            .cloned()
            .map(MediaItem::Track)
            .chain(self.albums.iter().cloned().map(MediaItem::Album))
            .chain(self.artists.iter().cloned().map(MediaItem::Artist))
            .chain(self.playlists.iter().cloned().map(MediaItem::Playlist))
            .collect()
    }

    pub(crate) fn set_item(&mut self, item: MediaItem, is_favorite: bool) {
        macro_rules! update {
            ($items:expr, $variant:ident, $value:expr) => {{
                $items.retain(|existing| !same_item(&MediaItem::$variant(existing.clone()), &item));
                if is_favorite {
                    let mut value = $value.clone();
                    value.media.favorite = Some(true);
                    $items.push(value);
                }
            }};
        }
        match &item {
            MediaItem::Track(value) => update!(self.tracks, Track, value),
            MediaItem::Album(value) => update!(self.albums, Album, value),
            MediaItem::Artist(value) => update!(self.artists, Artist, value),
            MediaItem::Playlist(value) => update!(self.playlists, Playlist, value),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FavoriteSync {
    pending: HashMap<String, bool>,
}

impl FavoriteSync {
    // Commands and worker coordination.
    pub(crate) fn submit(&mut self, identity: String, is_favorite: bool) -> bool {
        self.pending.insert(identity, is_favorite).is_none()
    }

    pub(crate) fn desired_state(&self, identity: &str) -> Option<bool> {
        self.pending.get(identity).copied()
    }

    pub(crate) fn finish(&mut self, identity: &str, written_state: bool) -> bool {
        if self.desired_state(identity) != Some(written_state) {
            return false;
        }
        self.pending.remove(identity);
        true
    }
}

// Provider and library aliases identify the same item within one media type.
pub(crate) fn same_item(left: &MediaItem, right: &MediaItem) -> bool {
    if std::mem::discriminant(left) != std::mem::discriminant(right) {
        return false;
    }
    let left = left.fields();
    let right = right.fields();
    (left.provider == right.provider && left.item_id == right.item_id)
        || left.provider_mappings.iter().any(|mapping| {
            (mapping.provider_instance == right.provider && mapping.item_id == right.item_id)
                || right.provider_mappings.iter().any(|other| {
                    mapping.provider_instance == other.provider_instance
                        && mapping.item_id == other.item_id
                })
        })
        || right.provider_mappings.iter().any(|mapping| {
            mapping.provider_instance == left.provider && mapping.item_id == left.item_id
        })
}

fn identity(item: &MediaItem) -> String {
    format!(
        "{:?}:{}:{}",
        std::mem::discriminant(item),
        item.fields().provider.as_str(),
        item.fields().item_id.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::super::client::tests::server;
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn favorite_loading_reads_every_page_with_full_items() {
        let first = (0..PAGE_SIZE)
            .map(|index| {
                json!({
                    "item_id":index.to_string(), "provider":"library", "name":"Track"
                })
            })
            .collect::<Vec<_>>();
        let (client, worker) = server(vec![
            ("music/tracks/library_items", json!(first)),
            (
                "music/tracks/library_items",
                json!([{
                    "item_id":"last", "provider":"library", "name":"Last"
                }]),
            ),
        ]);
        let tracks: Vec<Track> = client.favorite_pages("tracks").await.unwrap();
        assert_eq!(tracks.len(), PAGE_SIZE + 1);
        let requests = worker.join().unwrap();
        assert_eq!(requests[0]["args"]["favorite"], true);
        assert_eq!(requests[0]["args"]["summary"], false);
        assert_eq!(requests[1]["args"]["offset"], PAGE_SIZE);
    }

    #[tokio::test]
    async fn private_playlist_metadata_is_available_before_tracks() {
        let (client, worker) = server(vec![(
            "music/playlists/library_items",
            json!([{"item_id":"42", "provider":"library", "name":"Playlist",
                "access":{"owner":"user-one", "sharing":"private"}}]),
        )]);
        let playlists = client
            .load_private_playlist_metadata("user-one")
            .await
            .unwrap();
        assert_eq!(playlists.len(), 1);
        assert!(playlists[0].is_loading_tracks);
        assert!(playlists[0].items.is_empty());
        assert_eq!(
            serde_json::to_value(&playlists[0]).unwrap()["is_loading_tracks"],
            true
        );
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn private_playlist_loading_accepts_unrated_playlists() {
        let (client, worker) = server(vec![
            (
                "music/playlists/library_items",
                json!([
                    {"item_id":"41", "provider":"library", "name":"Other", "favorite":null},
                    {"item_id":"42", "provider":"library", "name":"Playlist",
                     "favorite":null, "access":{"owner":"user-one", "sharing":"private"}}
                ]),
            ),
            ("music/playlists/playlist_tracks", json!([])),
        ]);
        let playlists = client.load_private_playlists("user-one").await.unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].playlist.media.item_id.as_str(), "42");
        worker.join().unwrap();
    }

    fn item(provider: &str, item_id: &str, favorite: bool) -> MediaItem {
        serde_json::from_value(serde_json::json!({
            "media_type":"track", "item_id":item_id, "provider":provider, "name":"Track", "favorite":favorite,
            "provider_mappings":[{"provider_domain":"test", "provider_instance":"test-one", "item_id":"source"}]
        })).unwrap()
    }

    #[tokio::test]
    async fn targeted_playlist_refresh_loads_only_its_metadata_and_tracks() {
        let (client, worker) = server(vec![
            (
                "music/playlists/get",
                json!({"item_id":"42", "provider":"library", "name":"Playlist", "access":{"owner":"user-one", "sharing":"private"}}),
            ),
            (
                "music/playlists/playlist_tracks",
                json!([{"item_id":"new", "provider":"test", "name":"New"}]),
            ),
        ]);
        let playlist = client
            .load_private_playlist("42", "user-one")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(playlist.items.len(), 1);
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["args"]["force_refresh"], true);
        assert_eq!(requests[1]["args"]["item_id"], "42");
    }

    #[test]
    fn targeted_playlist_replacement_preserves_other_contents_and_removes_lost_access() {
        let playlist = |id: &str, name: &str| PrivatePlaylist {
            playlist: serde_json::from_value(
                json!({"item_id":id, "provider":"library", "name":name}),
            )
            .unwrap(),
            items: Vec::new(),
            is_loading_tracks: false,
        };
        let mut snapshot = UserDataSnapshot {
            playlists: vec![playlist("41", "Other"), playlist("42", "Old")],
            ..Default::default()
        };
        snapshot.replace_playlist("42", Some(playlist("42", "New")));
        assert_eq!(snapshot.playlists.len(), 2);
        assert_eq!(snapshot.playlists[0].playlist.media.name, "Other");
        assert_eq!(snapshot.playlists[1].playlist.media.name, "New");
        snapshot.replace_playlist("42", None);
        assert_eq!(snapshot.playlists.len(), 1);
    }

    fn artwork_snapshot(track_ids: &[&str]) -> UserDataSnapshot {
        use base64::Engine;
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode_image(&image::DynamicImage::ImageRgb8(
                image::RgbImage::from_pixel(2, 2, image::Rgb([20, 110, 220])),
            ))
            .unwrap();
        let source = format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        UserDataSnapshot {
            playlists: vec![PrivatePlaylist {
                playlist: serde_json::from_value(json!({
                    "item_id":"42", "provider":"library", "name":"My playlist", "is_editable":true,
                    "access":{"owner":"user-one", "sharing":"private"},
                    "provider_mappings":[{"item_id":"source-playlist", "provider_domain":"builtin", "provider_instance":"builtin-one"}],
                    "metadata":{"description":"Keep this", "images":[
                        {"type":"thumb", "path":"old.jpg", "provider":"builtin"},
                        {"type":"fanart", "path":"fanart.jpg", "provider":"builtin"}
                    ]}
                })).unwrap(),
                items: track_ids.iter().map(|id| serde_json::from_value(json!({
                    "item_id":id, "provider":"library", "name":"Track", "uri":format!("library://track/{id}"),
                    "image":{"type":"thumb", "path":source, "provider":"builtin"}
                })).unwrap()).collect(),
                is_loading_tracks: false,
            }],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn adding_or_removing_tracks_uploads_artwork_and_metadata_events_do_not_loop() {
        for (before, after) in [(vec!["1"], vec!["1", "2"]), (vec!["1", "2"], vec!["2"])] {
            let previous = artwork_snapshot(&before);
            let mut snapshot = artwork_snapshot(&after);
            let image =
                build_playlist_collage(&snapshot.playlists[0].items, "https://music.example")
                    .await
                    .unwrap();
            let mut confirmed = json!(snapshot.playlists[0].playlist);
            confirmed["metadata"]["images"][0]["path"] = json!(image);
            let mut stored = json!(snapshot.playlists[0].playlist);
            stored["metadata"]["last_refresh"] = json!(12345);
            stored["supported_mediatypes"] = json!(["track", "radio"]);
            let (client, worker) = server(vec![
                ("music/playlists/get", stored),
                ("music/playlists/update", confirmed),
            ]);
            assert!(client
                .refresh_playlist_collages(&previous, &mut snapshot)
                .await
                .is_empty());
            assert_eq!(
                snapshot.playlists[0]
                    .playlist
                    .media
                    .metadata
                    .as_ref()
                    .unwrap()
                    .images[0]
                    .path,
                image
            );
            let requests = worker.join().unwrap();
            let args = &requests[1]["args"];
            assert_eq!(args["item_id"], "42");
            assert_eq!(args["overwrite"], true);
            assert_eq!(args["update"]["name"], "My playlist");
            assert_eq!(args["update"]["access"]["owner"], "user-one");
            assert_eq!(args["update"]["metadata"]["description"], "Keep this");
            assert_eq!(args["update"]["metadata"]["last_refresh"], 12345);
            assert_eq!(
                args["update"]["supported_mediatypes"],
                json!(["track", "radio"])
            );
            let images = &args["update"]["metadata"]["images"];
            assert_eq!(images[0]["type"], "thumb");
            assert_eq!(images[0]["provider"], "builtin");
            assert_eq!(images[0]["remotely_accessible"], false);
            assert!(images[0]["path"]
                .as_str()
                .unwrap()
                .starts_with("data:image/jpeg;base64,"));
            assert_eq!(images[1]["path"], "fanart.jpg");
            let mut metadata_event = snapshot.clone();
            metadata_event.playlists[0].items[0].media.name = "Updated metadata".into();
            assert!(client
                .refresh_playlist_collages(&snapshot, &mut metadata_event)
                .await
                .is_empty());
        }
    }

    #[tokio::test]
    async fn explicit_cover_refresh_replaces_existing_artwork_without_track_changes() {
        let mut snapshot = artwork_snapshot(&["1", "2"]);
        let original = snapshot.playlists[0].clone();
        let image = build_playlist_collage(&original.items, "https://music.example")
            .await
            .unwrap();
        let stored = json!(original.playlist);
        let mut confirmed = stored.clone();
        confirmed["metadata"]["images"][0]["path"] = json!(image);
        let (client, worker) = server(vec![
            ("music/playlists/get", stored),
            ("music/playlists/update", confirmed),
        ]);
        client
            .refresh_private_playlist_cover(&mut snapshot.playlists[0])
            .await
            .unwrap();
        assert!(original.has_same_tracks(&snapshot.playlists[0]));
        let requests = worker.join().unwrap();
        assert_eq!(
            requests[1]["args"]["update"]["metadata"]["images"][0]["path"],
            image
        );
        assert_eq!(
            snapshot.playlists[0]
                .playlist
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images[0]
                .path,
            image
        );
    }

    #[tokio::test]
    async fn empty_playlist_clears_thumbnail_but_keeps_other_images() {
        let previous = artwork_snapshot(&["1"]);
        let mut snapshot = artwork_snapshot(&[]);
        let stored = json!(snapshot.playlists[0].playlist);
        let (client, worker) = server(vec![
            ("music/playlists/get", stored.clone()),
            ("music/playlists/update", stored),
        ]);
        assert!(client
            .refresh_playlist_collages(&previous, &mut snapshot)
            .await
            .is_empty());
        let requests = worker.join().unwrap();
        let images = requests[1]["args"]["update"]["metadata"]["images"]
            .as_array()
            .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0]["type"], "fanart");
    }

    #[tokio::test]
    async fn artwork_failure_keeps_confirmed_tracks_and_initial_loading_does_not_write() {
        let previous = artwork_snapshot(&["1"]);
        let mut snapshot = artwork_snapshot(&["1", "2"]);
        let (client, worker) = server(vec![
            ("music/playlists/get", json!(snapshot.playlists[0].playlist)),
            ("music/playlists/update", json!({"test_http_status":500})),
        ]);
        assert_eq!(
            client
                .refresh_playlist_collages(&previous, &mut snapshot)
                .await
                .len(),
            1
        );
        worker.join().unwrap();
        assert_eq!(snapshot.playlists[0].items.len(), 2);
        assert!(client
            .refresh_playlist_collages(&UserDataSnapshot::default(), &mut snapshot)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn unchanged_artwork_is_not_written_again() {
        let snapshot = artwork_snapshot(&["1"]);
        let (client, worker) = server(vec![]);
        client
            .set_private_playlist_image(&snapshot.playlists[0].playlist, Some("old.jpg".into()))
            .await
            .unwrap();
        assert!(worker.join().unwrap().is_empty());
    }

    #[test]
    fn optimistic_state_survives_refresh_and_an_older_write_completion() {
        let mut cache = LibraryUserData::default();
        let (identity, _, should_start) = cache.submit(item("test-one", "source", false), true);
        assert!(should_start);
        assert_eq!(cache.snapshot.favorites.tracks.len(), 1);
        let (other_identity, _, should_start) = cache.submit(item("library", "42", true), false);
        assert_eq!(identity, other_identity);
        assert!(!should_start);
        cache.replace_snapshot(UserDataSnapshot {
            favorites: Favorites {
                tracks: vec![match item("library", "42", true) {
                    MediaItem::Track(track) => track,
                    _ => unreachable!(),
                }],
                ..Favorites::default()
            },
            playlists: Vec::new(),
            is_loading_playlists: false,
        });
        assert!(cache.snapshot.favorites.tracks.is_empty());
        assert!(!cache.finish(&identity, true, Some(item("library", "42", true))));
        assert!(cache.snapshot.favorites.tracks.is_empty());
        assert!(cache.finish(&identity, false, Some(item("library", "42", false))));
        assert!(cache.snapshot.favorites.tracks.is_empty());
    }

    #[test]
    fn canonical_library_confirmation_keeps_the_existing_worker_identity() {
        let mut cache = LibraryUserData::default();
        let (identity, _, _) = cache.submit(item("test-one", "source", false), true);
        cache.submit(item("test-one", "source", false), false);
        assert!(!cache.finish(&identity, true, Some(item("library", "42", true))));
        let (next_identity, _, should_start) = cache.submit(item("library", "42", true), true);
        assert_eq!(next_identity, identity);
        assert!(!should_start);
        assert!(cache.finish(&identity, true, Some(item("library", "42", true))));
        assert_eq!(cache.snapshot.favorites.tracks.len(), 1);
    }

    #[test]
    fn confirmed_library_alias_replaces_provider_entry_without_duplicates() {
        let mut cache = LibraryUserData::default();
        let (identity, _, _) = cache.submit(item("test-one", "source", false), true);
        assert!(cache.finish(&identity, true, Some(item("library", "42", true))));
        assert_eq!(cache.snapshot.favorites.tracks.len(), 1);
        assert_eq!(
            cache.snapshot.favorites.tracks[0].media.provider.as_str(),
            "library"
        );
        let (_, canonical, should_start) = cache.submit(item("test-one", "source", true), false);
        assert!(should_start);
        assert_eq!(canonical.fields().provider.as_str(), "library");
        assert!(cache.snapshot.favorites.tracks.is_empty());
    }

    #[test]
    fn separate_items_start_independent_workers_but_repeated_clicks_coalesce() {
        let mut sync = FavoriteSync::default();
        assert!(sync.submit("first".into(), true));
        assert!(sync.submit("second".into(), true));
        assert!(!sync.submit("first".into(), false));
        assert!(!sync.finish("first", true));
        assert_eq!(sync.desired_state("first"), Some(false));
        assert!(sync.finish("second", true));
        assert_eq!(sync.desired_state("second"), None);
        assert!(sync.finish("first", false));
        assert!(sync.submit("first".into(), true));
    }

    #[test]
    fn clicking_back_to_the_inflight_state_does_not_require_another_write() {
        let mut sync = FavoriteSync::default();
        assert!(sync.submit("item".into(), true));
        assert!(!sync.submit("item".into(), false));
        assert!(!sync.submit("item".into(), true));
        assert!(sync.finish("item", true));
    }
}
