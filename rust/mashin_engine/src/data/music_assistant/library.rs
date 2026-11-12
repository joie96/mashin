//! Session-owned MA favorites, private playlists, and volatile favorite synchronization.

use super::client::MusicAssistantClient;

use crate::application::repositories::{CatalogClient, InitialUserDataUpdate, UserDataPublisher};
use crate::domain::catalog::{
    CatalogLoadPhase, CatalogLoadSnapshot, CatalogQuery, CatalogReadKnowledge, CatalogValue,
};
use crate::domain::identifiers::MediaId;
use crate::domain::library::{Favorites, PrivatePlaylist, UserDataSnapshot};
use crate::domain::media::{MediaItem, Playlist, PlaylistSharing};
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
    // Favorite commands.
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
        if created.media.provider.as_str() != "library" || created.media.item_id.as_str().is_empty()
        {
            return Err("playlist creation returned an invalid library identity".into());
        }
        let confirmed: Playlist = self
            .execute_command(
                "music/playlists/set_access",
                json!({
                    "item_id": created.media.item_id, "sharing": "private", "owner": user_id,
                    "shared_users": [], "collaborative": false,
                }),
            )
            .await
            .map_err(|error| {
                format!("playlist created, but private access was not confirmed: {error}")
            })?;
        if confirmed.media.provider.as_str() != "library"
            || confirmed.media.item_id != created.media.item_id
            || !confirmed.access.as_ref().is_some_and(|access| {
                access.owner.as_deref() == Some(user_id)
                    && access.sharing == PlaylistSharing::Private
            })
        {
            return Err("playlist created, but private ownership was not confirmed".into());
        }
        let mut merged = MediaItem::Playlist(created);
        merged.merge_known(MediaItem::Playlist(confirmed));
        let MediaItem::Playlist(confirmed) = merged else {
            unreachable!()
        };
        Ok(confirmed)
    }

    pub(crate) async fn rename_private_playlist(
        &self,
        item_id: &str,
        name: &str,
    ) -> Result<Playlist, String> {
        let (original, mut update) = self.read_owned_private_playlist(item_id).await?;
        update["name"] = json!(name);
        update["sort_name"] = json!(name);
        let confirmed: Playlist = self
            .execute_command(
                "music/playlists/update",
                json!({
                    "item_id": original.media.item_id, "update": update, "overwrite": true,
                }),
            )
            .await?;
        if confirmed.media.provider.as_str() != "library"
            || confirmed.media.item_id != original.media.item_id
        {
            return Err("playlist rename returned a different identity".into());
        }
        let mut merged = MediaItem::Playlist(original);
        merged.merge_known(MediaItem::Playlist(confirmed));
        let MediaItem::Playlist(confirmed) = merged else {
            unreachable!()
        };
        Ok(confirmed)
    }

    // Returns the validated original only after the server confirms its library removal.
    pub(crate) async fn remove_private_playlist(&self, item_id: &str) -> Result<Playlist, String> {
        let (playlist, _) = self.read_owned_private_playlist(item_id).await?;
        self.execute_command::<()>(
            "music/library/remove_item",
            json!({"media_type": "playlist", "library_item_id": playlist.media.item_id}),
        )
        .await?;
        Ok(playlist)
    }

    pub(crate) async fn get_private_playlist(&self, playlist_id: &str) -> Result<Playlist, String> {
        self.read_owned_private_playlist(playlist_id)
            .await
            .map(|(playlist, _)| playlist)
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
        if playlist.media.provider.as_str() != "library"
            || playlist.media.item_id.as_str().is_empty()
        {
            return Err("playlist image update requires a library identity".into());
        }
        let (original, mut update) = self
            .read_owned_private_playlist(playlist.media.item_id.as_str())
            .await?;
        let metadata = update
            .get_mut("metadata")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "MA returned playlist metadata without an object".to_string())?;
        let mut images = match metadata.remove("images") {
            None => return Err("MA returned playlist images without known state".into()),
            Some(Value::Null) => Vec::new(),
            Some(Value::Array(images)) => images,
            _ => return Err("MA returned invalid playlist images".into()),
        };
        let current = original.media.metadata.as_ref().and_then(|metadata| {
            metadata
                .images
                .iter()
                .find(|image| image.image_type == "thumb")
        });
        if current.map(|image| image.path.as_str()) == image.as_deref() {
            return Ok(original);
        }
        images.retain(|image| image["type"] != "thumb");
        if let Some(path) = image {
            images.insert(
                0,
                json!(crate::domain::media::MediaItemImage {
                    image_type: "thumb".into(),
                    path,
                    provider: crate::domain::identifiers::ProviderInstanceId("builtin".into()),
                    remotely_accessible: false,
                    proxy_id: None,
                    cached_inline: None
                }),
            );
        }
        metadata.insert("images".into(), json!(images));
        let confirmed: Playlist = self
            .execute_command(
                "music/playlists/update",
                json!({"item_id":original.media.item_id, "update":update, "overwrite":true}),
            )
            .await?;
        if confirmed.media.provider.as_str() != "library"
            || confirmed.media.item_id != original.media.item_id
        {
            return Err("playlist image update returned a different identity".into());
        }
        let mut merged = MediaItem::Playlist(original);
        merged.merge_known(MediaItem::Playlist(confirmed));
        let MediaItem::Playlist(confirmed) = merged else {
            unreachable!()
        };
        Ok(confirmed)
    }

    // Snapshot assembly loads typed MA pages without holding application locks.
    pub(crate) async fn load_library_user_data(
        &self,
        catalog: &dyn crate::application::repositories::CatalogClient,
        user_id: &str,
    ) -> Result<UserDataSnapshot, String> {
        let favorites = self.load_favorites(catalog).await?;
        let mut publisher = |_: InitialUserDataUpdate| Ok::<(), String>(());
        let playlists = self
            .load_private_playlist_metadata(catalog, user_id, true, &mut publisher)
            .await?;
        Ok(UserDataSnapshot {
            revision: 0,
            favorites,
            playlists,
            is_loading_favorites: false,
            is_loading_playlists: false,
            has_loaded_favorites: true,
            favorite_overrides: Vec::new(),
        })
    }

    pub(crate) async fn load_favorites(
        &self,
        catalog: &dyn crate::application::repositories::CatalogClient,
    ) -> Result<Favorites, String> {
        let mut publish = |_: InitialUserDataUpdate| Ok::<(), String>(());
        self.load_favorite_pages(catalog, true, &mut publish).await
    }

    pub(crate) async fn load_favorite_pages(
        &self,
        catalog: &dyn crate::application::repositories::CatalogClient,
        force_refresh: bool,
        publish: &mut (impl UserDataPublisher + ?Sized),
    ) -> Result<Favorites, String> {
        use futures_util::StreamExt;

        let favorites = tokio::sync::Mutex::new(Favorites::default());
        let publisher = tokio::sync::Mutex::new(publish);
        macro_rules! observe_pages {
            ($field:ident, $query:ident, $value:ident, $kind:literal $(, $filter:ident: $filter_value:expr)?) => {{
                async {
                    let mut pages = LibraryPageReader::new(
                        catalog,
                        |offset| CatalogQuery::$query {
                                $($filter: $filter_value,)?
                                favorite: Some(true), limit: PAGE_SIZE as u32,
                                offset, summary: false,
                        },
                        |value| match value { CatalogValue::$value(items) => Some(items), _ => None },
                        force_refresh,
                    ).observe();
                    while let Some(rows) = pages.next().await {
                        let rows = rows.map_err(|error| format!(concat!($kind, " favorite pages: {}"), error))?;
                        let mut favorites = favorites.lock().await;
                        favorites.$field = rows;
                        publisher.lock().await
                            .publish(InitialUserDataUpdate::FavoritePage(favorites.clone()))
                            .await?;
                    }
                    Ok::<(), String>(())
                }
            }};
        }
        let (tracks, albums, artists, playlists) = tokio::join!(
            observe_pages!(tracks, LibraryTracks, Tracks, "track"),
            observe_pages!(albums, LibraryAlbums, Albums, "album", filters: Default::default()),
            observe_pages!(artists, LibraryArtists, Artists, "artist", filters: Default::default()),
            observe_pages!(playlists, LibraryPlaylists, Playlists, "playlist"),
        );
        tracks?;
        albums?;
        artists?;
        playlists?;
        Ok(favorites.into_inner())
    }

    pub(crate) async fn load_private_playlist_metadata(
        &self,
        catalog: &dyn crate::application::repositories::CatalogClient,
        user_id: &str,
        force_refresh: bool,
        publisher: &mut dyn UserDataPublisher,
    ) -> Result<Vec<PrivatePlaylist>, String> {
        use futures_util::StreamExt;
        let mut pages = LibraryPageReader::new(
            catalog,
            |offset| CatalogQuery::LibraryPlaylists {
                favorite: None,
                limit: PAGE_SIZE as u32,
                offset,
                summary: true,
            },
            |value| match value {
                CatalogValue::Playlists(items) => Some(items),
                _ => None,
            },
            force_refresh,
        )
        .observe();
        let mut published = None;
        while let Some(rows) = pages.next().await {
            let playlists = rows?
                .into_iter()
                .filter(|owned| {
                    owned.access.as_ref().is_some_and(|access| {
                        access.owner.as_deref() == Some(user_id)
                            && access.sharing == PlaylistSharing::Private
                    })
                })
                .map(|playlist| PrivatePlaylist {
                    playlist,
                    items: Vec::new(),
                    is_loading_tracks: true,
                })
                .collect::<Vec<_>>();
            if published.as_ref() != Some(&playlists) {
                publisher
                    .publish(InitialUserDataUpdate::PlaylistPage(playlists.clone()))
                    .await?;
                published = Some(playlists);
            }
        }
        Ok(published.unwrap_or_default())
    }

    pub(crate) async fn load_private_playlist(
        &self,
        playlist_id: &str,
        user_id: &str,
    ) -> Result<Option<PrivatePlaylist>, String> {
        let playlist: Playlist = crate::data::music_assistant::catalog::load_playlist(
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

    // Complete wire data stays request-local; ownership validation and cloning run off the executor.
    async fn read_owned_private_playlist(
        &self,
        item_id: &str,
    ) -> Result<(Playlist, Value), String> {
        let user_id = self.get_current_user_id().await?;
        let response: Value = crate::data::music_assistant::catalog::load_playlist(
            self,
            &MediaId(item_id.into()),
            "library",
            false,
        )
        .await?;
        let expected_id = item_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let playlist: Playlist =
                serde_json::from_value(response.clone()).map_err(|error| error.to_string())?;
            if playlist.media.provider.as_str() != "library"
                || playlist.media.item_id.as_str() != expected_id
                || !playlist.access.as_ref().is_some_and(|access| {
                    access.owner.as_deref() == Some(user_id.as_str())
                        && access.sharing == PlaylistSharing::Private
                })
            {
                return Err("playlist is not a private playlist owned by the current user".into());
            }
            Ok((playlist, response))
        })
        .await
        .map_err(|error| error.to_string())?
    }
}

struct LibraryPage<Item> {
    current_items: Option<Vec<Item>>,
    confirmed_items: Option<Vec<Item>>,
    is_ready: bool,
    has_server_response: bool,
    generation: u64,
    error: Option<String>,
}

// Request-local pages retain subscriptions, not a second confirmed library.
struct LibraryPageReader<'catalog, Item> {
    catalog: &'catalog dyn CatalogClient,
    query: Box<dyn Fn(u32) -> CatalogQuery + Send + Sync + 'catalog>,
    extract: fn(CatalogValue) -> Option<Vec<Item>>,
    force_refresh: bool,
    pending: futures_util::stream::SelectAll<
        futures_util::stream::BoxStream<'catalog, (u32, u64, Option<CatalogLoadSnapshot>)>,
    >,
    pages: std::collections::BTreeMap<u32, LibraryPage<Item>>,
    published: Option<Vec<Item>>,
    is_finished: bool,
}

impl<'catalog, Item: Clone + PartialEq + Send + 'catalog> LibraryPageReader<'catalog, Item> {
    // Configuration and subscription ownership.
    fn new(
        catalog: &'catalog dyn CatalogClient,
        query: impl Fn(u32) -> CatalogQuery + Send + Sync + 'catalog,
        extract: fn(CatalogValue) -> Option<Vec<Item>>,
        force_refresh: bool,
    ) -> Self {
        let mut reader = Self {
            catalog,
            query: Box::new(query),
            extract,
            force_refresh,
            pending: futures_util::stream::SelectAll::new(),
            pages: Default::default(),
            published: None,
            is_finished: false,
        };
        reader.start_page(0);
        reader
    }

    fn observe(self) -> futures_util::stream::BoxStream<'catalog, Result<Vec<Item>, String>> {
        use futures_util::StreamExt;
        Box::pin(futures_util::stream::unfold(
            self,
            |mut reader| async move {
                if reader.is_finished {
                    return None;
                }
                loop {
                    let Some((offset, generation, snapshot)) = reader.pending.next().await else {
                        reader.is_finished = true;
                        return Some((Err("library pages ended before completion".into()), reader));
                    };
                    if reader.pages[&offset].generation != generation {
                        continue;
                    }
                    if let Some(snapshot) = snapshot {
                        if snapshot.phase == CatalogLoadPhase::Invalidated {
                            reader.is_finished = true;
                            return Some((
                                Err(snapshot
                                    .error
                                    .unwrap_or_else(|| "library pages invalidated".into())),
                                reader,
                            ));
                        }
                        let page = reader.pages.get_mut(&offset).unwrap();
                        let observed_items =
                            if snapshot.knowledge == CatalogReadKnowledge::ObservedResponse {
                                snapshot.value.map(std::sync::Arc::unwrap_or_clone).and_then(reader.extract)
                            } else {
                                None
                            };
                        let has_observed_response = observed_items.is_some();
                        if let Some(items) = observed_items {
                            if snapshot.source
                                == Some(crate::domain::catalog::CatalogDataSource::Cache)
                                || page.current_items.is_none()
                            {
                                page.current_items = Some(items.clone());
                            }
                            if snapshot.phase == CatalogLoadPhase::Ready {
                                page.confirmed_items = Some(items);
                            }
                        }
                        page.is_ready = snapshot.phase == CatalogLoadPhase::Ready;
                        page.has_server_response = page.is_ready
                            && snapshot.source
                                == Some(crate::domain::catalog::CatalogDataSource::Server);
                        if snapshot.phase == CatalogLoadPhase::Failed {
                            page.error = Some(
                                snapshot
                                    .error
                                    .unwrap_or_else(|| "library page failed".into()),
                            );
                        } else if page.is_ready && !has_observed_response {
                            page.error =
                                Some("library page completed without an observed response".into());
                        }
                        let should_start_next = page.error.is_none()
                            && page
                                .confirmed_items
                                .as_ref()
                                .or(page.current_items.as_ref())
                                .is_some_and(|items| items.len() == PAGE_SIZE);
                        if should_start_next {
                            reader.start_page(offset + PAGE_SIZE as u32);
                        }
                    } else {
                        let page = reader.pages.get_mut(&offset).unwrap();
                        if !page.is_ready && page.error.is_none() {
                            page.error = Some("library page ended before completion".into());
                        }
                    }
                    reader.refresh_cached_boundary();
                    match reader.projection() {
                        Err(error) => {
                            reader.is_finished = true;
                            return Some((Err(error), reader));
                        }
                        Ok((items, is_complete)) => {
                            reader.is_finished = is_complete;
                            if let Some(items) = items {
                                if reader.published.as_ref() != Some(&items) {
                                    reader.published = Some(items.clone());
                                    return Some((Ok(items), reader));
                                }
                            }
                            if is_complete {
                                return None;
                            }
                        }
                    }
                }
            },
        ))
    }

    // Page coordination and ordered projections.
    fn start_page(&mut self, offset: u32) {
        if self.pages.contains_key(&offset) {
            return;
        }
        self.pages.insert(
            offset,
            LibraryPage {
                current_items: None,
                confirmed_items: None,
                is_ready: false,
                has_server_response: false,
                generation: 0,
                error: None,
            },
        );
        self.subscribe(offset, self.force_refresh);
    }

    fn subscribe(&mut self, offset: u32, force_refresh: bool) {
        use futures_util::StreamExt;
        let generation = self.pages[&offset].generation;
        let catalog = self.catalog;
        let observations = catalog
            .observe_catalog((self.query)(offset), force_refresh)
            .map(move |snapshot| (offset, generation, Some(snapshot)))
            .chain(futures_util::stream::once(std::future::ready((
                offset, generation, None,
            ))));
        self.pending.push(Box::pin(observations));
    }

    fn refresh_cached_boundary(&mut self) {
        if !self.pages[&0].has_server_response {
            return;
        }
        let mut offset = 0;
        while let Some(page) = self.pages.get_mut(&offset) {
            if page.error.is_some() {
                return;
            }
            if !page.has_server_response {
                if page.is_ready {
                    page.is_ready = false;
                    page.confirmed_items = None;
                    page.generation += 1;
                    self.subscribe(offset, true);
                }
                return;
            }
            if page
                .confirmed_items
                .as_ref()
                .is_none_or(|items| items.len() < PAGE_SIZE)
            {
                return;
            }
            offset += PAGE_SIZE as u32;
        }
    }

    fn projection(&self) -> Result<(Option<Vec<Item>>, bool), String> {
        let head_is_ready = self.pages[&0].is_ready;
        let requires_server_pages = self.pages[&0].has_server_response;
        let mut rows = Vec::new();
        let mut has_observed_page = false;
        let mut all_pages_ready = true;
        let mut offset = 0;
        while let Some(page) = self.pages.get(&offset) {
            if head_is_ready || offset == 0 {
                if let Some(error) = &page.error {
                    return Err(error.clone());
                }
            }
            if requires_server_pages && !page.has_server_response {
                break;
            }
            let items = if head_is_ready {
                page.confirmed_items
                    .as_ref()
                    .or(page.current_items.as_ref())
            } else {
                page.current_items.as_ref()
            };
            let Some(items) = items else {
                break;
            };
            has_observed_page = true;
            all_pages_ready &= page.is_ready;
            rows.extend_from_slice(items);
            if items.len() < PAGE_SIZE {
                return Ok((Some(rows), all_pages_ready));
            }
            offset += PAGE_SIZE as u32;
        }
        Ok((has_observed_page.then_some(rows), false))
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
        catalog: &'request dyn crate::application::repositories::CatalogClient,
        user_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<UserDataSnapshot, String>> {
        Box::pin(MusicAssistantClient::load_library_user_data(
            self, catalog, user_id,
        ))
    }
    fn load_favorites<'request>(
        &'request self,
        catalog: &'request dyn crate::application::repositories::CatalogClient,
    ) -> futures_util::future::BoxFuture<'request, Result<Favorites, String>> {
        Box::pin(MusicAssistantClient::load_favorites(self, catalog))
    }
    fn load_favorite_pages<'request>(
        &'request self,
        catalog: &'request dyn crate::application::repositories::CatalogClient,
        force_refresh: bool,
        publisher: &'request mut dyn UserDataPublisher,
    ) -> futures_util::future::BoxFuture<'request, Result<Favorites, String>> {
        Box::pin(MusicAssistantClient::load_favorite_pages(
            self,
            catalog,
            force_refresh,
            publisher,
        ))
    }
    fn load_private_playlists<'request>(
        &'request self,
        catalog: &'request dyn crate::application::repositories::CatalogClient,
        user_id: &'request str,
        force_refresh: bool,
        publisher: &'request mut dyn UserDataPublisher,
    ) -> futures_util::future::BoxFuture<'request, Result<Vec<PrivatePlaylist>, String>> {
        Box::pin(MusicAssistantClient::load_private_playlist_metadata(
            self,
            catalog,
            user_id,
            force_refresh,
            publisher,
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
        item_id: &'request str,
        name: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Playlist, String>> {
        Box::pin(MusicAssistantClient::rename_private_playlist(
            self, item_id, name,
        ))
    }
    fn remove_private_playlist<'request>(
        &'request self,
        item_id: &'request str,
    ) -> futures_util::future::BoxFuture<'request, Result<Playlist, String>> {
        Box::pin(MusicAssistantClient::remove_private_playlist(self, item_id))
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
    mod paginated_sections {
        use super::*;
        use crate::application::repositories::CatalogClient;
        use crate::domain::catalog::{
            CatalogDataSource, CatalogFreshness, CatalogLoadPhase, CatalogLoadSnapshot,
            CatalogQuery, CatalogReadKnowledge, CatalogValue,
        };
        use futures_util::{future::BoxFuture, stream, stream::BoxStream, StreamExt};

        struct PagedCatalog {
            release_head: tokio::sync::watch::Sender<bool>,
            release_tail: tokio::sync::watch::Sender<bool>,
            terminal: CatalogLoadPhase,
            offsets: std::sync::Mutex<Vec<u32>>,
            head_is_fresh: bool,
            tail_is_fresh: bool,
            should_keep_head: bool,
            should_observe_tracks: bool,
        }

        impl PagedCatalog {
            fn new(terminal: CatalogLoadPhase) -> Self {
                Self {
                    release_head: tokio::sync::watch::channel(false).0,
                    release_tail: tokio::sync::watch::channel(false).0,
                    terminal,
                    offsets: Default::default(),
                    head_is_fresh: false,
                    tail_is_fresh: false,
                    should_keep_head: false,
                    should_observe_tracks: false,
                }
            }
        }

        impl CatalogClient for PagedCatalog {
            fn begin_startup(&self) -> Box<dyn Send> {
                Box::new(())
            }

            fn load_catalog(
                &self,
                _query: CatalogQuery,
                _force: bool,
            ) -> BoxFuture<'_, Result<CatalogValue, String>> {
                Box::pin(async { Err("pagination must observe cached pages".into()) })
            }

            fn observe_catalog(
                &self,
                query: CatalogQuery,
                force: bool,
            ) -> BoxStream<'_, CatalogLoadSnapshot> {
                let (offset, limit, is_tracks) = match query {
                    CatalogQuery::LibraryTracks {
                        offset,
                        limit,
                        favorite,
                        summary,
                    } if self.should_observe_tracks => {
                        assert_eq!(favorite, Some(true));
                        assert!(!summary);
                        (offset, limit, true)
                    }
                    CatalogQuery::LibraryPlaylists { offset, limit, .. } => (offset, limit, false),
                    CatalogQuery::LibraryAlbums { .. }
                    | CatalogQuery::LibraryArtists { .. }
                    | CatalogQuery::LibraryTracks { .. } => {
                        let value = if matches!(query, CatalogQuery::LibraryAlbums { .. }) {
                            CatalogValue::Albums(Vec::new())
                        } else if matches!(query, CatalogQuery::LibraryTracks { .. }) {
                            CatalogValue::Tracks(Vec::new())
                        } else {
                            CatalogValue::Artists(Vec::new())
                        };
                        return Box::pin(stream::iter([CatalogLoadSnapshot {
                            publication_revision: 0,
                            value: Some(std::sync::Arc::new(value)),
                            source: Some(CatalogDataSource::Server),
                            phase: CatalogLoadPhase::Ready,
                            knowledge: CatalogReadKnowledge::ObservedResponse,
                            freshness: CatalogFreshness::Fresh,
                            observed_at: Some(1),
                            error: None,
                            cache_error: None,
                        }]));
                    }
                    _ => panic!("unexpected page query"),
                };
                assert_eq!(limit, PAGE_SIZE as u32);
                self.offsets.lock().unwrap().push(offset);
                let count = if offset == 0 { PAGE_SIZE } else { 1 };
                let items: Vec<_> = (0..count)
                    .map(|index| {
                        json!({
                            "provider":"library", "item_id":format!("{}", offset as usize + index),
                            "name":"Cached", "access":{"owner":"user", "sharing":"private"}
                        })
                    })
                    .collect();
                let snapshot = CatalogLoadSnapshot {
                    publication_revision: 0,
                    value: Some(std::sync::Arc::new(if is_tracks {
                        CatalogValue::Tracks(serde_json::from_value(json!(items)).unwrap())
                    } else {
                        CatalogValue::Playlists(serde_json::from_value(json!(items)).unwrap())
                    })),
                    source: Some(CatalogDataSource::Cache),
                    phase: CatalogLoadPhase::Refreshing,
                    knowledge: CatalogReadKnowledge::ObservedResponse,
                    freshness: CatalogFreshness::Stale,
                    observed_at: Some(1),
                    error: None,
                    cache_error: None,
                };
                let mut completed = snapshot.clone();
                completed.phase = self.terminal;
                if self.terminal == CatalogLoadPhase::Ready {
                    if offset != 0 || !self.should_keep_head {
                        completed.value = Some(std::sync::Arc::new(if is_tracks {
                            CatalogValue::Tracks(Vec::new())
                        } else {
                            CatalogValue::Playlists(Vec::new())
                        }));
                    }
                    completed.source = Some(CatalogDataSource::Server);
                } else {
                    completed.error = Some("controlled pagination failure".into());
                }
                let mut release = if offset == 0 {
                    self.release_head.subscribe()
                } else {
                    self.release_tail.subscribe()
                };
                if (offset == 0 && self.head_is_fresh)
                    || (offset != 0 && self.tail_is_fresh && !force)
                {
                    let mut snapshot = snapshot;
                    snapshot.phase = CatalogLoadPhase::Ready;
                    return Box::pin(stream::iter([snapshot]));
                }
                Box::pin(stream::iter([snapshot]).chain(stream::once(async move {
                    while !*release.borrow_and_update() {
                        release.changed().await.unwrap();
                    }
                    completed
                })))
            }
        }

        #[tokio::test]
        async fn paginated_private_playlists_publish_cached_tail_before_head_refresh() {
            for terminal in [
                CatalogLoadPhase::Ready,
                CatalogLoadPhase::Failed,
                CatalogLoadPhase::Invalidated,
            ] {
                let (client, worker) = server(Vec::new());
                let catalog = PagedCatalog::new(terminal);
                let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
                let mut publisher = move |update| {
                    sender
                        .send(update)
                        .map_err(|_| "publisher closed".to_owned())
                };
                let loading =
                    client.load_private_playlist_metadata(&catalog, "user", false, &mut publisher);
                tokio::pin!(loading);
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    loop {
                        tokio::select! {
                            result = &mut loading => panic!("head refresh completed before release: {result:?}"),
                            update = received.recv() => {
                                if let InitialUserDataUpdate::PlaylistPage(items) = update.unwrap() {
                                    if items.len() == PAGE_SIZE + 1 { break; }
                                }
                            }
                        }
                    }
                }).await.unwrap();
                assert_eq!(*catalog.offsets.lock().unwrap(), vec![0, PAGE_SIZE as u32]);
                catalog.release_head.send(true).unwrap();
                let result = tokio::time::timeout(std::time::Duration::from_secs(2), &mut loading)
                    .await
                    .unwrap();
                assert_eq!(result.is_ok(), terminal == CatalogLoadPhase::Ready);
                if let Ok(items) = result {
                    assert!(items.is_empty());
                }
                assert_eq!(catalog.release_tail.receiver_count(), 0);
                while let Ok(update) = received.try_recv() {
                    if let InitialUserDataUpdate::PlaylistPage(items) = update {
                        assert_eq!(
                            items.len(),
                            if terminal == CatalogLoadPhase::Ready {
                                0
                            } else {
                                PAGE_SIZE + 1
                            }
                        );
                    }
                }
                assert!(worker.join().unwrap().is_empty());
            }
        }

        #[tokio::test]
        async fn paginated_private_playlists_keep_early_remote_tail_separate_from_cached_head() {
            let catalog = PagedCatalog::new(CatalogLoadPhase::Ready);
            let mut pages = LibraryPageReader::new(
                &catalog,
                |offset| CatalogQuery::LibraryPlaylists {
                    favorite: None,
                    limit: PAGE_SIZE as u32,
                    offset,
                    summary: true,
                },
                |value| match value {
                    CatalogValue::Playlists(items) => Some(items),
                    _ => None,
                },
                false,
            )
            .observe();
            assert_eq!(pages.next().await.unwrap().unwrap().len(), PAGE_SIZE);
            assert_eq!(pages.next().await.unwrap().unwrap().len(), PAGE_SIZE + 1);
            catalog.release_tail.send(true).unwrap();
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), pages.next())
                    .await
                    .is_err()
            );
            catalog.release_head.send(true).unwrap();
            assert!(pages.next().await.unwrap().unwrap().is_empty());
            assert!(pages.next().await.is_none());
        }

        #[tokio::test]
        async fn paginated_private_playlists_publish_cached_tail_after_fresh_cached_head() {
            let mut catalog = PagedCatalog::new(CatalogLoadPhase::Ready);
            catalog.head_is_fresh = true;
            let mut pages = LibraryPageReader::new(
                &catalog,
                |offset| CatalogQuery::LibraryPlaylists {
                    favorite: None,
                    limit: PAGE_SIZE as u32,
                    offset,
                    summary: true,
                },
                |value| match value {
                    CatalogValue::Playlists(items) => Some(items),
                    _ => None,
                },
                false,
            )
            .observe();
            assert_eq!(pages.next().await.unwrap().unwrap().len(), PAGE_SIZE);
            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(2), pages.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap()
                    .len(),
                PAGE_SIZE + 1
            );
            catalog.release_tail.send(true).unwrap();
            assert_eq!(pages.next().await.unwrap().unwrap().len(), PAGE_SIZE);
            assert!(pages.next().await.is_none());
        }

        #[tokio::test]
        async fn paginated_private_playlists_reconfirm_fresh_tail_after_server_head() {
            let mut catalog = PagedCatalog::new(CatalogLoadPhase::Ready);
            catalog.tail_is_fresh = true;
            catalog.should_keep_head = true;
            let mut pages = LibraryPageReader::new(
                &catalog,
                |offset| CatalogQuery::LibraryPlaylists {
                    favorite: None,
                    limit: PAGE_SIZE as u32,
                    offset,
                    summary: true,
                },
                |value| match value {
                    CatalogValue::Playlists(items) => Some(items),
                    _ => None,
                },
                false,
            )
            .observe();
            assert_eq!(pages.next().await.unwrap().unwrap().len(), PAGE_SIZE);
            assert_eq!(pages.next().await.unwrap().unwrap().len(), PAGE_SIZE + 1);
            catalog.release_head.send(true).unwrap();
            assert_eq!(pages.next().await.unwrap().unwrap().len(), PAGE_SIZE);
            assert_eq!(
                *catalog.offsets.lock().unwrap(),
                vec![0, PAGE_SIZE as u32, PAGE_SIZE as u32]
            );
            catalog.release_tail.send(true).unwrap();
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(2), pages.next())
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        #[tokio::test]
        async fn paginated_favorites_publish_cached_tail_before_head_refresh() {
            for terminal in [
                CatalogLoadPhase::Ready,
                CatalogLoadPhase::Failed,
                CatalogLoadPhase::Invalidated,
            ] {
                for should_observe_tracks in [true, false] {
                    let (client, worker) = server(Vec::new());
                    let mut catalog = PagedCatalog::new(terminal);
                    catalog.should_observe_tracks = should_observe_tracks;
                    let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
                    let mut publisher = move |update| {
                        sender
                            .send(update)
                            .map_err(|_| "publisher closed".to_owned())
                    };
                    let loading = client.load_favorite_pages(&catalog, false, &mut publisher);
                    tokio::pin!(loading);
                    tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    tokio::select! {
                        result = &mut loading => panic!("favorite refresh completed before release: {result:?}"),
                        update = received.recv() => {
                            if let InitialUserDataUpdate::FavoritePage(items) = update.unwrap() {
                                if items.playlists.len() == PAGE_SIZE + 1 &&
                                    (!should_observe_tracks || items.tracks.len() == PAGE_SIZE + 1) { break; }
                            }
                        }
                    }
                }
            }).await.unwrap();
                    catalog.release_head.send(true).unwrap();
                    let result =
                        tokio::time::timeout(std::time::Duration::from_secs(2), &mut loading)
                            .await
                            .unwrap();
                    assert_eq!(result.is_ok(), terminal == CatalogLoadPhase::Ready);
                    if let Ok(items) = result {
                        assert!(items.playlists.is_empty());
                        assert!(items.tracks.is_empty());
                    }
                    assert_eq!(catalog.release_tail.receiver_count(), 0);
                    assert_eq!(worker.join().unwrap().len(), 0);
                }
            }
        }
    }

    mod initial_sections {
        use super::*;
        use crate::application::repositories::CatalogClient;
        use crate::domain::catalog::{
            CatalogDataSource, CatalogFreshness, CatalogLoadPhase, CatalogLoadSnapshot,
            CatalogQuery, CatalogReadKnowledge, CatalogValue,
        };
        use futures_util::{future::BoxFuture, stream, stream::BoxStream, StreamExt};

        struct SectionCatalog<'client> {
            client: &'client MusicAssistantClient,
            should_block_private_playlists: bool,
            should_block_favorites: bool,
            terminal: CatalogLoadPhase,
            release: tokio::sync::Notify,
        }

        impl CatalogClient for SectionCatalog<'_> {
            fn begin_startup(&self) -> Box<dyn Send> {
                Box::new(())
            }

            fn load_catalog(
                &self,
                _query: CatalogQuery,
                _force_refresh: bool,
            ) -> BoxFuture<'_, Result<CatalogValue, String>> {
                Box::pin(async { Err("section loading must observe cached pages".into()) })
            }

            fn observe_catalog(
                &self,
                query: CatalogQuery,
                _force_refresh: bool,
            ) -> BoxStream<'_, CatalogLoadSnapshot> {
                let (value, should_block) = match query {
                    CatalogQuery::LibraryTracks { .. } => {
                        return self.client.observe_catalog(query, _force_refresh)
                    }
                    CatalogQuery::LibraryPlaylists { favorite: None, .. } => (
                        CatalogValue::Playlists(
                            serde_json::from_value(json!([{
                                "provider":"library", "item_id":"private", "name":"Cached private",
                                "access":{"owner":"user", "sharing":"private"}
                            }]))
                            .unwrap(),
                        ),
                        self.should_block_private_playlists,
                    ),
                    CatalogQuery::LibraryAlbums { .. } => (
                        CatalogValue::Albums(
                            serde_json::from_value(json!([{
                                "provider":"library", "item_id":"album", "name":"Cached album"
                            }]))
                            .unwrap(),
                        ),
                        self.should_block_favorites,
                    ),
                    CatalogQuery::LibraryArtists { .. } => (
                        CatalogValue::Artists(
                            serde_json::from_value(json!([{
                                "provider":"library", "item_id":"artist", "name":"Cached artist"
                            }]))
                            .unwrap(),
                        ),
                        self.should_block_favorites,
                    ),
                    CatalogQuery::LibraryPlaylists { .. } => (
                        CatalogValue::Playlists(
                            serde_json::from_value(json!([{
                                "provider":"library", "item_id":"favorite", "name":"Cached favorite"
                            }]))
                            .unwrap(),
                        ),
                        self.should_block_favorites,
                    ),
                    _ => panic!("unexpected section query"),
                };
                let snapshot = CatalogLoadSnapshot {
                    publication_revision: 0,
                    value: Some(std::sync::Arc::new(value.clone())),
                    source: Some(CatalogDataSource::Cache),
                    phase: if should_block {
                        CatalogLoadPhase::Refreshing
                    } else {
                        CatalogLoadPhase::Ready
                    },
                    knowledge: CatalogReadKnowledge::ObservedResponse,
                    freshness: CatalogFreshness::Stale,
                    observed_at: Some(1),
                    error: None,
                    cache_error: None,
                };
                if !should_block {
                    return Box::pin(stream::iter([snapshot]));
                }
                let mut completed = snapshot.clone();
                completed.phase = self.terminal;
                if self.terminal == CatalogLoadPhase::Ready {
                    completed.source = Some(CatalogDataSource::Server);
                    completed.value = Some(std::sync::Arc::new(match value {
                        CatalogValue::Albums(_) => CatalogValue::Albums(Vec::new()),
                        CatalogValue::Artists(_) => CatalogValue::Artists(Vec::new()),
                        CatalogValue::Playlists(_) => CatalogValue::Playlists(Vec::new()),
                        _ => unreachable!(),
                    }));
                } else {
                    completed.error = Some("controlled section failure".into());
                }
                Box::pin(stream::iter([snapshot]).chain(stream::once(async move {
                    self.release.notified().await;
                    completed
                })))
            }
        }

        #[tokio::test]
        async fn favorite_sections_publish_cached_categories_when_track_request_fails() {
            let (client, worker) = server(vec![(
                "music/tracks/library_items",
                json!({"test_http_status":500}),
            )]);
            let catalog = SectionCatalog {
                client: &client,
                should_block_private_playlists: false,
                should_block_favorites: false,
                terminal: CatalogLoadPhase::Ready,
                release: Default::default(),
            };
            let mut pages = Vec::new();
            let mut publisher = |update| {
                if let InitialUserDataUpdate::FavoritePage(items) = update {
                    pages.push(items);
                }
                Ok::<(), String>(())
            };
            assert!(client
                .load_favorite_pages(&catalog, false, &mut publisher)
                .await
                .is_err());
            let last = pages.last().unwrap();
            assert_eq!(last.albums.len(), 1);
            assert_eq!(last.artists.len(), 1);
            assert_eq!(last.playlists.len(), 1);
            assert_eq!(worker.join().unwrap().len(), 1);
        }

        #[tokio::test]
        async fn favorite_sections_publish_all_cached_categories_before_refresh_completion() {
            for terminal in [
                CatalogLoadPhase::Ready,
                CatalogLoadPhase::Failed,
                CatalogLoadPhase::Invalidated,
            ] {
                let (client, worker) = server(vec![("music/tracks/library_items", json!([]))]);
                let catalog = SectionCatalog {
                    client: &client,
                    should_block_private_playlists: false,
                    should_block_favorites: true,
                    terminal,
                    release: Default::default(),
                };
                let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
                let mut publisher = move |update| {
                    sender
                        .send(update)
                        .map_err(|_| "publisher closed".to_owned())
                };
                let loading = client.load_favorite_pages(&catalog, false, &mut publisher);
                tokio::pin!(loading);
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    loop {
                        tokio::select! {
                            result = &mut loading => panic!("favorite refresh completed before release: {result:?}"),
                            update = received.recv() => {
                                if let InitialUserDataUpdate::FavoritePage(items) = update.unwrap() {
                                    if items.albums.len() == 1 && items.artists.len() == 1 && items.playlists.len() == 1 {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }).await.unwrap();
                catalog.release.notify_waiters();
                let result = tokio::time::timeout(std::time::Duration::from_secs(2), &mut loading)
                    .await
                    .unwrap();
                assert_eq!(result.is_ok(), terminal == CatalogLoadPhase::Ready);
                if let Ok(items) = result {
                    assert!(items.albums.is_empty());
                    assert!(items.artists.is_empty());
                    assert!(items.playlists.is_empty());
                }
                assert_eq!(worker.join().unwrap().len(), 1);
            }
        }

        #[tokio::test]
        async fn initial_sections_publish_favorites_before_private_playlist_refresh_completes() {
            for terminal in [
                CatalogLoadPhase::Ready,
                CatalogLoadPhase::Failed,
                CatalogLoadPhase::Invalidated,
            ] {
                let (client, worker) = server(vec![("music/tracks/library_items", json!([]))]);
                client
                    .initialize_current_user_id(UserId("user".into()))
                    .unwrap();
                let catalog = SectionCatalog {
                    client: &client,
                    should_block_private_playlists: true,
                    should_block_favorites: false,
                    terminal,
                    release: Default::default(),
                };
                let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
                let publisher = move |update| {
                    sender
                        .send(update)
                        .map_err(|_| "publisher closed".to_owned())
                };
                let loading = crate::application::startup::load_initial_user_data(
                    &client, &catalog, publisher,
                );
                tokio::pin!(loading);
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    let mut has_private_page = false;
                    loop {
                        tokio::select! {
                            result = &mut loading => panic!("private refresh completed before release: {result:?}"),
                            update = received.recv() => match update.unwrap() {
                                InitialUserDataUpdate::PlaylistPage(items) => {
                                    assert_eq!(items.len(), 1);
                                    has_private_page = true;
                                }
                                InitialUserDataUpdate::Favorites(items) => {
                                    assert_eq!(items.albums.len(), 1);
                                    assert_eq!(items.artists.len(), 1);
                                    assert_eq!(items.playlists.len(), 1);
                                    assert!(has_private_page);
                                    break;
                                }
                                InitialUserDataUpdate::Playlists(_) => panic!("private refresh is still blocked"),
                                _ => {}
                            }
                        }
                    }
                }).await.unwrap();
                catalog.release.notify_waiters();
                let result = tokio::time::timeout(std::time::Duration::from_secs(2), &mut loading)
                    .await
                    .unwrap();
                assert_eq!(result.is_ok(), terminal == CatalogLoadPhase::Ready);
                assert_eq!(worker.join().unwrap().len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn private_playlist_removal_requires_null_confirmation_and_preserves_original_identity() {
        for response in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!(false),
            serde_json::json!({"test_http_status":403}),
            serde_json::json!({"test_http_status":500}),
        ] {
            let should_succeed = response.is_null();
            let original = serde_json::json!({"provider":"library","item_id":"42","name":"Original",
                "access":{"owner":"user","sharing":"private"},
                "provider_mappings":[{"provider_domain":"builtin","provider_instance":"builtin","item_id":"original.m3u"}]});
            let (client, worker) = server(vec![
                ("music/playlists/get", original),
                ("music/library/remove_item", response),
            ]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            let result = client.remove_private_playlist("42").await;
            assert_eq!(result.is_ok(), should_succeed);
            if let Ok(removed) = result {
                assert_eq!(removed.media.item_id.as_str(), "42");
                assert_eq!(removed.media.name, "Original");
                assert_eq!(
                    removed.media.provider_mappings[0].item_id.as_str(),
                    "original.m3u"
                );
            }
            let requests = worker.join().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0]["args"]["allow_update_metadata"], false);
            assert_eq!(
                requests[1]["args"],
                serde_json::json!({"media_type":"playlist","library_item_id":"42"})
            );
        }
    }

    #[tokio::test]
    async fn private_playlist_removal_rejects_unowned_or_mismatched_sources_before_command() {
        for changed in [
            serde_json::json!({"access":null}),
            serde_json::json!({"access":{"owner":"other","sharing":"private"}}),
            serde_json::json!({"access":{"owner":"user","sharing":"everyone"}}),
            serde_json::json!({"provider":"spotify"}),
            serde_json::json!({"item_id":"other"}),
        ] {
            let mut original = serde_json::json!({"provider":"library","item_id":"42","name":"Original","access":{"owner":"user","sharing":"private"}});
            for (field, value) in changed.as_object().unwrap() {
                original[field] = value.clone();
            }
            let (client, worker) = server(vec![("music/playlists/get", original)]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            assert!(client.remove_private_playlist("42").await.is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn private_playlist_creation_preserves_details_and_returns_confirmed_server_metadata() {
        let created = serde_json::json!({"provider":"library","item_id":"42","name":"Created",
            "metadata":{"description":"Created details"}});
        let confirmed = serde_json::json!({"provider":"library","item_id":"42","name":"Private server name",
            "access":{"owner":"user","sharing":"private"}});
        let (client, worker) = server(vec![
            ("music/playlists/create_playlist", created),
            ("music/playlists/set_access", confirmed),
        ]);
        let playlist = client
            .create_private_playlist("Requested", "user")
            .await
            .unwrap();
        assert_eq!(playlist.media.name, "Private server name");
        assert_eq!(
            playlist.media.metadata.unwrap().description.as_deref(),
            Some("Created details")
        );
        assert_eq!(playlist.access.unwrap().owner.as_deref(), Some("user"));
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["args"], serde_json::json!({"name":"Requested"}));
        assert_eq!(
            requests[1]["args"],
            serde_json::json!({"item_id":"42","sharing":"private",
            "owner":"user","shared_users":[],"collaborative":false})
        );
    }

    #[tokio::test]
    async fn private_playlist_creation_rejects_invalid_created_identity_before_access() {
        for response in [
            serde_json::json!({"provider":"spotify","item_id":"42","name":"Invalid"}),
            serde_json::json!({"provider":"library","item_id":"","name":"Invalid"}),
            serde_json::json!({"provider":"library","item_id":"42","name":7}),
            serde_json::json!({"test_http_status":500}),
        ] {
            let (client, worker) = server(vec![("music/playlists/create_playlist", response)]);
            assert!(client
                .create_private_playlist("Requested", "user")
                .await
                .is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn private_playlist_creation_requires_confirmed_identity_and_private_owner() {
        for changed in [
            serde_json::json!({"provider":"spotify"}),
            serde_json::json!({"item_id":"other"}),
            serde_json::json!({"access":null}),
            serde_json::json!({"access":{"owner":"other","sharing":"private"}}),
            serde_json::json!({"access":{"owner":"user","sharing":"everyone"}}),
            serde_json::json!({"name":7}),
            serde_json::json!({"test_http_status":403}),
        ] {
            let created = serde_json::json!({"provider":"library","item_id":"42","name":"Created"});
            let mut confirmed = serde_json::json!({"provider":"library","item_id":"42","name":"Private",
                "access":{"owner":"user","sharing":"private"}});
            for (field, value) in changed.as_object().unwrap() {
                confirmed[field] = value.clone();
            }
            let (client, worker) = server(vec![
                ("music/playlists/create_playlist", created),
                ("music/playlists/set_access", confirmed),
            ]);
            let error = client
                .create_private_playlist("Requested", "user")
                .await
                .unwrap_err();
            assert!(error.contains("playlist created, but"));
            assert_eq!(worker.join().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn private_playlist_image_uses_fresh_owned_source_and_preserves_wire_fields() {
        for requested in [Some("https://example.com/requested.jpg".to_owned()), None] {
            let original = json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Fresh name",
                "supported_mediatypes":["track","radio"],"translation_key":"keep-key","translation_params":{"owner":"Display owner"},
                "metadata":{"description":"Retained description","future_metadata":{"value":7},"images":[
                    {"type":"thumb","provider":"builtin","path":"https://example.com/old.jpg"},
                    {"type":"banner","provider":"builtin","path":"https://example.com/banner.jpg","future_image":9}]},
                "access":{"owner":"user","sharing":"private"}});
            let response = json!({"provider":"library","item_id":"42","name":"Server name",
                "metadata":{"images":[{"type":"thumb","provider":"builtin","path":"https://example.com/confirmed.jpg"}]}});
            let stale: Playlist = serde_json::from_value(json!({"provider":"library","item_id":"42","name":"Stale name",
                "metadata":{"images":requested.as_ref().map(|path| vec![json!({"type":"thumb","provider":"builtin","path":path})]).unwrap_or_default()}})).unwrap();
            let (client, worker) = server(vec![
                ("music/playlists/get", original.clone()),
                ("music/playlists/update", response),
            ]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            let confirmed = client
                .set_private_playlist_image(&stale, requested.clone())
                .await
                .unwrap();
            assert_eq!(confirmed.media.name, "Server name");
            let metadata = confirmed.media.metadata.unwrap();
            assert_eq!(
                metadata.description.as_deref(),
                Some("Retained description")
            );
            assert_eq!(metadata.images[0].path, "https://example.com/confirmed.jpg");
            let requests = worker.join().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0]["args"]["allow_update_metadata"], false);
            assert_eq!(requests[1]["args"]["overwrite"], true);
            let update = &requests[1]["args"]["update"];
            assert_eq!(update["name"], original["name"]);
            assert_eq!(
                update["supported_mediatypes"],
                original["supported_mediatypes"]
            );
            assert_eq!(update["translation_key"], original["translation_key"]);
            assert_eq!(update["translation_params"], original["translation_params"]);
            assert_eq!(
                update["metadata"]["future_metadata"],
                original["metadata"]["future_metadata"]
            );
            let images = update["metadata"]["images"].as_array().unwrap();
            assert_eq!(
                images
                    .iter()
                    .find(|image| image["type"] == "banner")
                    .unwrap(),
                &original["metadata"]["images"][1]
            );
            assert_eq!(
                images
                    .iter()
                    .find(|image| image["type"] == "thumb")
                    .map(|image| image["path"].as_str().unwrap()),
                requested.as_deref()
            );
        }
    }

    #[tokio::test]
    async fn private_playlist_image_noop_returns_fresh_owned_metadata() {
        for (path, has_null_images) in [
            (Some("https://example.com/same.jpg"), false),
            (None, false),
            (None, true),
        ] {
            let source = json!({"provider":"library","item_id":"42","name":"Fresh name",
                "metadata":{"description":"Fresh description","images":if has_null_images { serde_json::Value::Null } else { json!(path.map(|path| vec![json!({"type":"thumb","provider":"builtin","path":path})]).unwrap_or_default()) }},
                "access":{"owner":"user","sharing":"private"}});
            let stale: Playlist = serde_json::from_value(
                json!({"provider":"library","item_id":"42","name":"Stale name"}),
            )
            .unwrap();
            let (client, worker) = server(vec![("music/playlists/get", source)]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            let confirmed = client
                .set_private_playlist_image(&stale, path.map(str::to_owned))
                .await
                .unwrap();
            assert_eq!(confirmed.media.name, "Fresh name");
            assert_eq!(
                confirmed.media.metadata.unwrap().description.as_deref(),
                Some("Fresh description")
            );
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn private_playlist_image_rejects_invalid_source_and_confirmation() {
        let source = json!({"provider":"library","item_id":"42","name":"Original",
            "metadata":{"images":[]},"access":{"owner":"user","sharing":"private"}});
        let playlist: Playlist = serde_json::from_value(source.clone()).unwrap();
        for (provider, item_id) in [("source", "42"), ("library", "")] {
            let mut invalid = playlist.clone();
            invalid.media.provider =
                crate::domain::identifiers::ProviderInstanceId(provider.into());
            invalid.media.item_id = crate::domain::identifiers::MediaId(item_id.into());
            let (client, worker) = server(Vec::new());
            assert!(client
                .set_private_playlist_image(&invalid, None)
                .await
                .is_err());
            assert!(worker.join().unwrap().is_empty());
        }
        for invalid in [
            json!({"provider":"library","item_id":"42","name":"Invalid","access":{"owner":"other","sharing":"private"}}),
            json!({"provider":"library","item_id":"42","name":"Invalid","access":{"owner":"user","sharing":"everyone"}}),
            json!({"provider":"source","item_id":"42","name":"Invalid","access":{"owner":"user","sharing":"private"}}),
            json!({"provider":"library","item_id":"other","name":"Invalid","access":{"owner":"user","sharing":"private"}}),
            json!({"provider":"library","item_id":"42","name":"Invalid","metadata":null,"access":{"owner":"user","sharing":"private"}}),
            json!({"provider":"library","item_id":"42","name":"Invalid","metadata":{},"access":{"owner":"user","sharing":"private"}}),
            json!({"test_http_status":503}),
        ] {
            let (client, worker) = server(vec![("music/playlists/get", invalid)]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            assert!(client
                .set_private_playlist_image(&playlist, None)
                .await
                .is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
        for invalid in [
            json!({"provider":"source","item_id":"42","name":"Invalid"}),
            json!({"provider":"library","item_id":"other","name":"Invalid"}),
            serde_json::Value::Null,
            json!({"test_http_status":403}),
            json!({"test_http_status":500}),
        ] {
            let (client, worker) = server(vec![
                ("music/playlists/get", source.clone()),
                ("music/playlists/update", invalid),
            ]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            assert!(client
                .set_private_playlist_image(&playlist, Some("requested".into()))
                .await
                .is_err());
            assert_eq!(worker.join().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn private_playlist_rename_returns_server_data_and_preserves_unmodeled_wire_fields() {
        let original = serde_json::json!({"media_type":"playlist","provider":"library","item_id":"42","name":"Original",
            "supported_mediatypes":["track","radio"],"translation_key":"keep-key","translation_params":{"owner":"Display owner"},
            "metadata":{"description":"Original description","future_metadata":{"value":7}},
            "access":{"owner":"user","sharing":"private"}});
        let response = serde_json::json!({"provider":"library","item_id":"42","name":"Server name","sort_name":"Server sort",
            "metadata":{"description":"Server description"},"access":{"owner":"user","sharing":"private"}});
        let (client, worker) = super::super::client::tests::server(vec![
            ("music/playlists/get", original.clone()),
            ("music/playlists/update", response),
        ]);
        client
            .initialize_current_user_id(crate::domain::identifiers::UserId("user".into()))
            .unwrap();
        let confirmed = client
            .rename_private_playlist("42", "Requested name")
            .await
            .unwrap();
        assert_eq!(confirmed.media.name, "Server name");
        assert_eq!(confirmed.media.sort_name.as_deref(), Some("Server sort"));
        assert_eq!(
            confirmed.media.metadata.unwrap().description.as_deref(),
            Some("Server description")
        );
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["args"]["allow_update_metadata"], false);
        let update = &requests[1]["args"]["update"];
        assert_eq!(update["name"], "Requested name");
        assert_eq!(
            update["supported_mediatypes"],
            original["supported_mediatypes"]
        );
        assert_eq!(update["translation_key"], original["translation_key"]);
        assert_eq!(update["translation_params"], original["translation_params"]);
        assert_eq!(
            update["metadata"]["future_metadata"],
            original["metadata"]["future_metadata"]
        );
    }

    use super::super::client::tests::server;
    use super::*;
    use crate::domain::identifiers::UserId;

    use serde_json::json;

    #[tokio::test]
    async fn private_playlist_rename_rejects_unowned_or_mismatched_sources_before_update() {
        for changed in [
            json!({"access":{"owner":"other","sharing":"private"}}),
            json!({"access":{"owner":"user","sharing":"everyone"}}),
            json!({"access":null}),
            json!({"provider":"spotify"}),
            json!({"item_id":"other"}),
            json!({"name":7}),
            json!({"test_http_status":403}),
        ] {
            let mut original = json!({"provider":"library","item_id":"42","name":"Original",
                "access":{"owner":"user","sharing":"private"}});
            for (field, value) in changed.as_object().unwrap() {
                original[field] = value.clone();
            }
            let (client, worker) = server(vec![("music/playlists/get", original)]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            assert!(client
                .rename_private_playlist("42", "Requested")
                .await
                .is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn private_playlist_rename_rejects_failed_or_mismatched_confirmations() {
        for response in [
            json!({"provider":"library","item_id":"other","name":"Wrong"}),
            json!({"provider":"spotify","item_id":"42","name":"Wrong"}),
            json!({"provider":"library","item_id":"42","name":7}),
            json!({"test_http_status":403}),
            json!({"test_http_status":500}),
        ] {
            let original = json!({"provider":"library","item_id":"42","name":"Original",
                "access":{"owner":"user","sharing":"private"}});
            let (client, worker) = server(vec![
                ("music/playlists/get", original),
                ("music/playlists/update", response),
            ]);
            client
                .initialize_current_user_id(UserId("user".into()))
                .unwrap();
            assert!(client
                .rename_private_playlist("42", "Requested")
                .await
                .is_err());
            let requests = worker.join().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1]["args"]["item_id"], "42");
            assert_eq!(requests[1]["args"]["overwrite"], true);
        }
    }

    #[tokio::test]
    async fn private_playlist_metadata_is_available_before_tracks() {
        let (client, worker) = server(vec![(
            "music/playlists/library_items",
            json!([{"item_id":"42", "provider":"library", "name":"Playlist",
                "access":{"owner":"user-one", "sharing":"private"}}]),
        )]);
        let mut publisher = |_: InitialUserDataUpdate| Ok::<(), String>(());
        let playlists = client
            .load_private_playlist_metadata(&client, "user-one", false, &mut publisher)
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
        let mut publisher = |_: InitialUserDataUpdate| Ok::<(), String>(());
        let playlists = client
            .load_private_playlist_metadata(&client, "user-one", false, &mut publisher)
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
