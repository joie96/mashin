//! Session-owned MA favorites, private playlists, and volatile favorite synchronization.

use super::client::MusicAssistantClient;

use crate::application::repositories::{InitialUserDataUpdate, UserDataPublisher};
use crate::domain::identifiers::MediaId;
use crate::domain::library::{Favorites, PrivatePlaylist, UserDataSnapshot};
use crate::domain::media::{MediaItem, Playlist, PlaylistSharing};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

const PAGE_SIZE: usize = 500;

#[derive(Deserialize)]
struct PlaylistTask {
    id: String,
    status: String,
    #[serde(default)]
    last_error: Option<String>,
}

impl MusicAssistantClient {
    // Music library queries.
    pub(crate) async fn get_library_page<Item: DeserializeOwned>(
        &self,
        media_type: &str,
        favorite: Option<bool>,
        limit: usize,
        offset: usize,
        summary: bool,
    ) -> Result<Vec<Item>, String> {
        self.execute_command(
            &format!("music/{media_type}/library_items"),
            json!({
                "favorite": favorite, "limit": limit, "offset": offset,
                "summary": summary,
                "sort_field": "sort_name", "sort_direction": "asc",
            }),
        )
        .await
    }

    pub(crate) async fn set_favorite(
        &self,
        item: &MediaItem,
        is_favorite: bool,
    ) -> Result<(), String> {
        self.execute_command::<Value>(
            "music/favorites/set_item",
            json!({"item": item, "favorite": if is_favorite { Some(true) } else { None }}),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn resolve_library_item(
        &self,
        item: &MediaItem,
    ) -> Result<Option<MediaItem>, String> {
        let media_type =
            serde_json::to_value(item).map_err(|error| error.to_string())?["media_type"].clone();
        if item.fields().provider.as_str() == "library" {
            return self
                .execute_command(
                    "music/item",
                    json!({
                        "media_type": media_type, "item_id": item.fields().item_id,
                        "provider_instance_id_or_domain": "library", "allow_update_metadata": false,
                    }),
                )
                .await
                .map(Some);
        }
        self.execute_command(
            "music/get_library_item",
            json!({
                "media_type": media_type,
                "item_id": item.fields().item_id,
                "provider_instance_id_or_domain": item.fields().provider,
            }),
        )
        .await
    }

    // Playlist commands.
    pub(crate) async fn create_private_playlist(
        &self,
        name: &str,
        user_id: &str,
    ) -> Result<Playlist, String> {
        let created: Playlist = self
            .execute_command("music/playlists/create_playlist", json!({"name": name}))
            .await?;
        self.execute_command(
            "music/playlists/set_access",
            json!({
                "item_id": created.media.item_id, "sharing": "private", "owner": user_id,
                "shared_users": [], "collaborative": false,
            }),
        )
        .await
    }

    pub(crate) async fn rename_private_playlist(
        &self,
        playlist: &Playlist,
        name: &str,
    ) -> Result<(), String> {
        let mut update = serde_json::to_value(MediaItem::Playlist(playlist.clone()))
            .map_err(|error| error.to_string())?;
        update["name"] = json!(name);
        update["sort_name"] = json!(name);
        self.execute_command::<Value>(
            "music/playlists/update",
            json!({
                "item_id": playlist.media.item_id, "update": update, "overwrite": true,
            }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn delete_private_playlist(&self, playlist_id: &str) -> Result<(), String> {
        self.execute_command::<Value>(
            "music/library/remove_item",
            json!({"media_type": "playlist", "library_item_id": playlist_id}),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn get_private_playlist(&self, playlist_id: &str) -> Result<Playlist, String> {
        let user_id = self.get_current_user_id().await?;
        let owned = crate::data::music_assistant::catalog::load_playlist(
            self,
            &MediaId(playlist_id.into()),
            "library",
            false,
        )
        .await?;
        if !owned.access.as_ref().is_some_and(|access| {
            access.owner.as_deref() == Some(user_id.as_str())
                && access.sharing == PlaylistSharing::Private
        }) {
            return Err("playlist is not a private playlist owned by the current user".into());
        }
        Ok(owned)
    }

    pub(crate) async fn add_playlist_tracks(
        &self,
        playlist_id: &str,
        uris: Vec<String>,
    ) -> Result<(), String> {
        let task = self
            .execute_command::<PlaylistTask>(
                "music/playlists/add_playlist_tracks",
                json!({ "db_playlist_id": playlist_id, "uris": uris }),
            )
            .await?;
        self.wait_for_playlist_task(task).await
    }

    pub(crate) async fn remove_playlist_tracks(
        &self,
        playlist_id: &str,
        positions_to_remove: Vec<u32>,
    ) -> Result<(), String> {
        let task = self
            .execute_command::<PlaylistTask>(
                "music/playlists/remove_playlist_tracks",
                json!({
                    "db_playlist_id": playlist_id,
                    "positions_to_remove": positions_to_remove,
                }),
            )
            .await?;
        self.wait_for_playlist_task(task).await
    }

    // Accepted playlist operations must reach a terminal task state.
    async fn wait_for_playlist_task(&self, mut task: PlaylistTask) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                match task.status.as_str() {
                    "success" => return Ok(()),
                    "failed" | "cancelled" => {
                        return Err(task
                            .last_error
                            .unwrap_or_else(|| "playlist operation failed".into()));
                    }
                    _ => {}
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
                task = self
                    .execute_command("tasks/get", json!({"task_id": task.id}))
                    .await?;
            }
        })
        .await
        .map_err(|_| "playlist update confirmation timed out".to_string())?
    }

    // The update envelope preserves server fields that are outside our read model.
    pub(crate) async fn set_private_playlist_image(
        &self,
        playlist: &Playlist,
        image: Option<String>,
    ) -> Result<Playlist, String> {
        let current = playlist.media.metadata.as_ref().and_then(|metadata| {
            metadata
                .images
                .iter()
                .find(|image| image.image_type == "thumb")
        });
        if current.map(|image| image.path.as_str()) == image.as_deref() {
            return Ok(playlist.clone());
        }
        let mut update:Value = self.execute_command("music/playlists/get", json!({"item_id":playlist.media.item_id, "provider_instance_id_or_domain":"library", "allow_update_metadata":false})).await?;
        let metadata = update
            .get_mut("metadata")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "MA returned playlist metadata without an object".to_string())?;
        let mut images = match metadata.remove("images") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(images)) => images,
            _ => return Err("MA returned invalid playlist images".into()),
        };
        images.retain(|image| image["type"] != "thumb");
        if let Some(path) = image {
            images.insert(
                0,
                json!(crate::domain::media::MediaItemImage {
                    image_type: "thumb".into(),
                    path,
                    provider: crate::domain::identifiers::ProviderInstanceId("builtin".into()),
                    remotely_accessible: false,
                    proxy_id: None
                }),
            );
        }
        metadata.insert("images".into(), json!(images));
        self.execute_command(
            "music/playlists/update",
            json!({"item_id":playlist.media.item_id, "update":update, "overwrite":true}),
        )
        .await
    }

    // Snapshot assembly loads typed MA pages without holding application locks.
    pub(crate) async fn load_library_user_data(
        &self,
        user_id: &str,
    ) -> Result<UserDataSnapshot, String> {
        let favorites = self.load_favorites().await?;
        let playlists = self.load_private_playlist_metadata(user_id).await?;
        Ok(UserDataSnapshot {
            favorites,
            playlists,
            is_loading_favorites: false,
            is_loading_playlists: false,
            has_loaded_favorites: true,
            favorite_overrides: Vec::new(),
        })
    }

    pub(crate) async fn load_favorites(&self) -> Result<Favorites, String> {
        let tracks = self.favorite_pages("tracks").await?;
        let albums = self.favorite_pages("albums").await?;
        let artists = self.favorite_pages("artists").await?;
        let playlists = self.favorite_pages("playlists").await?;
        Ok(Favorites {
            tracks,
            albums,
            artists,
            playlists,
        })
    }

    pub(crate) async fn load_favorite_pages(
        &self,
        publish: &mut (impl UserDataPublisher + ?Sized),
    ) -> Result<Favorites, String> {
        let mut favorites = Favorites::default();
        macro_rules! load_pages {
            ($field:ident) => {
                loop {
                    let page = self
                        .get_library_page(
                            stringify!($field),
                            Some(true),
                            PAGE_SIZE,
                            favorites.$field.len(),
                            false,
                        )
                        .await?;
                    let is_last_page = page.len() < PAGE_SIZE;
                    favorites.$field.extend(page);
                    publish
                        .publish(InitialUserDataUpdate::FavoritePage(favorites.clone()))
                        .await?;
                    if is_last_page {
                        break;
                    }
                }
            };
        }
        load_pages!(tracks);
        load_pages!(albums);
        load_pages!(artists);
        load_pages!(playlists);
        Ok(favorites)
    }

    pub(crate) async fn load_private_playlist_metadata(
        &self,
        user_id: &str,
    ) -> Result<Vec<PrivatePlaylist>, String> {
        let mut playlists = Vec::new();
        let library_playlists: Vec<Playlist> = self.library_pages("playlists", None, true).await?;
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
        let playlist = crate::data::music_assistant::catalog::load_playlist(
            self,
            &MediaId(playlist_id.to_owned()),
            "library",
            true,
        )
        .await?;
        if !playlist.access.as_ref().is_some_and(|access| {
            access.owner.as_deref() == Some(user_id) && access.sharing == PlaylistSharing::Private
        }) {
            return Ok(None);
        }
        Ok(Some(PrivatePlaylist {
            playlist,
            items: Vec::new(),
            is_loading_tracks: true,
        }))
    }

    // Full-list queries retain stable ordering across pages.
    async fn favorite_pages<Item: DeserializeOwned>(
        &self,
        media_type: &str,
    ) -> Result<Vec<Item>, String> {
        self.library_pages(media_type, Some(true), false).await
    }

    async fn library_pages<Item: DeserializeOwned>(
        &self,
        media_type: &str,
        favorite: Option<bool>,
        summary: bool,
    ) -> Result<Vec<Item>, String> {
        let mut items = Vec::new();
        loop {
            let page = self
                .get_library_page(media_type, favorite, PAGE_SIZE, items.len(), summary)
                .await?;
            let is_last_page = page.len() < PAGE_SIZE;
            items.extend(page);
            if is_last_page {
                return Ok(items);
            }
        }
    }
}

impl crate::application::repositories::LibraryClient for MusicAssistantClient {
    fn get_current_user_id(
        &self,
    ) -> futures_util::future::BoxFuture<'_, Result<crate::domain::identifiers::UserId, String>>
    {
        Box::pin(MusicAssistantClient::get_current_user_id(self))
    }
    fn load_library_user_data<'request>(
        &'request self,
        user_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<UserDataSnapshot, String>> {
        Box::pin(MusicAssistantClient::load_library_user_data(self, user_id))
    }
    fn load_favorites(&self) -> futures_util::future::BoxFuture<'_, Result<Favorites, String>> {
        Box::pin(MusicAssistantClient::load_favorites(self))
    }
    fn load_favorite_pages<'request>(
        &'request self,
        publisher: &'request mut dyn UserDataPublisher,
    ) -> futures_util::future::BoxFuture<'request, Result<Favorites, String>> {
        Box::pin(MusicAssistantClient::load_favorite_pages(self, publisher))
    }
    fn load_private_playlists<'request>(
        &'request self,
        user_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Vec<PrivatePlaylist>, String>> {
        Box::pin(MusicAssistantClient::load_private_playlist_metadata(
            self, user_id,
        ))
    }
    fn load_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
        user_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Option<PrivatePlaylist>, String>> {
        Box::pin(MusicAssistantClient::load_private_playlist(
            self, item_id, user_id,
        ))
    }
    fn get_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Playlist, String>> {
        Box::pin(MusicAssistantClient::get_private_playlist(self, item_id))
    }
    fn resolve_library_item<'request>(
        &'request self,
        item: &'request MediaItem,
    ) -> futures_util::future::BoxFuture<'request, Result<Option<MediaItem>, String>> {
        Box::pin(MusicAssistantClient::resolve_library_item(self, item))
    }
    fn set_favorite<'request>(
        &'request self,
        item: &'request MediaItem,
        is_favorite: bool,
    ) -> futures_util::future::BoxFuture<'request, Result<(), String>> {
        Box::pin(MusicAssistantClient::set_favorite(self, item, is_favorite))
    }
    fn create_private_playlist<'request>(
        &'request self,
        name: &'request str,
        user_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Playlist, String>> {
        Box::pin(MusicAssistantClient::create_private_playlist(
            self, name, user_id,
        ))
    }
    fn rename_private_playlist<'request>(
        &'request self,
        playlist: &'request Playlist,
        name: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<(), String>> {
        Box::pin(MusicAssistantClient::rename_private_playlist(
            self, playlist, name,
        ))
    }
    fn delete_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<(), String>> {
        Box::pin(MusicAssistantClient::delete_private_playlist(self, item_id))
    }
    fn add_playlist_tracks<'request>(
        &'request self,
        item_id: &'request str,
        uris: Vec<String>,
    ) -> futures_util::future::BoxFuture<'request, Result<(), String>> {
        Box::pin(MusicAssistantClient::add_playlist_tracks(
            self, item_id, uris,
        ))
    }
    fn remove_playlist_tracks<'request>(
        &'request self,
        item_id: &'request str,
        positions: Vec<u32>,
    ) -> futures_util::future::BoxFuture<'request, Result<(), String>> {
        Box::pin(MusicAssistantClient::remove_playlist_tracks(
            self, item_id, positions,
        ))
    }
    fn set_private_playlist_image<'request>(
        &'request self,
        playlist: &'request Playlist,
        image: Option<String>,
    ) -> futures_util::future::BoxFuture<'request, Result<Playlist, String>> {
        Box::pin(MusicAssistantClient::set_private_playlist_image(
            self, playlist, image,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::client::tests::server;
    use super::*;

    use serde_json::json;

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
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]["args"].get("provider").is_none());
        assert_eq!(requests[0]["args"]["summary"], true);
        assert!(requests[0]["args"]["favorite"].is_null());
    }

    #[tokio::test]
    async fn private_playlist_metadata_filters_ownership_after_paginating_slim_rows() {
        let other_playlists = (0..PAGE_SIZE)
            .map(|index| {
                json!({
                    "item_id":index.to_string(), "provider":"library", "name":"Shared",
                    "access":{"owner":"other", "sharing":"private"}
                })
            })
            .collect::<Vec<_>>();
        let (client, worker) = server(vec![
            ("music/playlists/library_items", json!(other_playlists)),
            (
                "music/playlists/library_items",
                json!([
                    {"item_id":"owned", "provider":"library", "name":"Owned", "is_editable":true,
                     "access":{"owner":"user-one", "sharing":"private"},
                     "metadata":{"images":[{"type":"thumb", "path":"cover.jpg", "provider":"builtin"}]}},
                    {"item_id":"public", "provider":"library", "name":"Public",
                     "access":{"owner":"user-one", "sharing":"everyone"}},
                    {"item_id":"legacy", "provider":"library", "name":"Legacy"}
                ]),
            ),
        ]);
        let playlists = client
            .load_private_playlist_metadata("user-one")
            .await
            .unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].playlist.media.item_id.as_str(), "owned");
        assert!(playlists[0].playlist.is_editable);
        assert_eq!(
            playlists[0]
                .playlist
                .media
                .metadata
                .as_ref()
                .unwrap()
                .images[0]
                .path,
            "cover.jpg"
        );
        let requests = worker.join().unwrap();
        assert_eq!(requests[1]["args"]["offset"], PAGE_SIZE);
        for request in requests {
            assert!(request["args"].get("provider").is_none());
            assert_eq!(request["args"]["summary"], true);
        }
    }

    #[tokio::test]
    async fn targeted_playlist_metadata_leaves_tracks_to_the_catalog_repository() {
        let (client, worker) = server(vec![(
            "music/playlists/get",
            json!({"item_id":"42", "provider":"library", "name":"Playlist", "access":{"owner":"user-one", "sharing":"private"}}),
        )]);
        let playlist = client
            .load_private_playlist("42", "user-one")
            .await
            .unwrap()
            .unwrap();
        assert!(playlist.items.is_empty());
        assert!(playlist.is_loading_tracks);
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["args"]["item_id"], "42");
    }
}
