//! Randomized JPEG collage for MA playlist artwork, loaded outside the core coordinator lock.

use crate::application::repositories::{ImageLoader, LibraryClient};
use crate::application::session::SessionTasks;
use crate::domain::artwork::{is_inline_image, ImageRequest};
use crate::domain::identifiers::MediaUri;
use crate::domain::library::PrivatePlaylist;
use crate::domain::media::{MediaItemImage, Track};
use base64::Engine;
use image::{codecs::jpeg::JpegEncoder, imageops, DynamicImage, Rgba, RgbaImage};
use rand::seq::SliceRandom;
use std::sync::Arc;

pub(crate) struct PlaylistArtwork {
    library: Arc<dyn LibraryClient>,
    images: Arc<dyn ImageLoader>,
    tasks: Arc<SessionTasks>,
    runtime: tokio::runtime::Handle,
}

impl PlaylistArtwork {
    // Configuration and cover commands; confirmed values are committed by the caller.
    pub(crate) fn new(
        library: Arc<dyn LibraryClient>,
        images: Arc<dyn ImageLoader>,
        tasks: Arc<SessionTasks>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            library,
            images,
            tasks,
            runtime,
        }
    }

    pub(crate) async fn refresh_playlist_collages(
        &self,
        previous: &[PrivatePlaylist],
        playlists: &mut [PrivatePlaylist],
    ) -> Vec<String> {
        let mut errors = Vec::new();
        for current in playlists {
            let Some(old) = previous
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
        let image = self.build_playlist_collage(&playlist.items).await;
        if image.is_none() && !playlist.items.is_empty() {
            return Err("playlist has no usable artwork".into());
        }
        playlist.playlist = self
            .library
            .set_private_playlist_image(&playlist.playlist, image)
            .await?;
        Ok(())
    }

    // CPU work stays registered with the session even when its awaiting subscriber is canceled.
    async fn build_playlist_collage(&self, tracks: &[Track]) -> Option<String> {
        let requests = image_requests(tracks, &mut rand::rng());
        let mut images = Vec::new();
        for request in requests {
            let Ok(mut read) = self.images.read_image(&request).await else {
                continue;
            };
            let bytes = if let Some(bytes) = read.current.bytes {
                if let Some(refresh) = read.refresh.take() {
                    let _ = self.tasks.spawn(&self.runtime, async move {
                        let _ = refresh.await;
                    });
                }
                Some(bytes)
            } else if let Some(refresh) = read.refresh.take() {
                refresh.await.ok().and_then(|update| update.bytes)
            } else {
                None
            };
            if let Some(bytes) = bytes {
                images.push(bytes);
            }
        }
        if images.is_empty() {
            return None;
        }
        self.tasks
            .run_blocking(&self.runtime, move || {
                let images: Vec<_> = images
                    .into_iter()
                    .filter_map(|bytes| {
                        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                            .with_guessed_format()
                            .ok()?;
                        let mut limits = image::Limits::default();
                        limits.max_image_width = Some(4096);
                        limits.max_image_height = Some(4096);
                        limits.max_alloc = Some(64 * 1024 * 1024);
                        reader.limits(limits);
                        reader.decode().ok()
                    })
                    .collect();
                if images.is_empty() {
                    None
                } else {
                    encode_collage(&images).ok()
                }
            })
            .await
            .ok()
            .flatten()
    }
}

const COLLAGE_SIZE: u32 = 500;
const TILE_COUNT: usize = 4;

fn image_request(image: &MediaItemImage, owner_uri: &Option<MediaUri>) -> Option<ImageRequest> {
    if !is_inline_image(image) && !image.proxy_id.as_deref().is_some_and(|id| !id.is_empty()) {
        return None;
    }
    Some(ImageRequest {
        image: image.clone(),
        owner_uri: owner_uri.clone(),
        position: 0,
        size: 512,
        provider_icon_role: None,
    })
}

fn image_requests(tracks: &[Track], random: &mut impl rand::Rng) -> Vec<ImageRequest> {
    let mut candidates = tracks.iter().collect::<Vec<_>>();
    candidates.shuffle(random);
    candidates
        .into_iter()
        .filter_map(|track| {
            track
                .album
                .as_ref()
                .and_then(|album| {
                    album
                        .media
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.images.first())
                        .and_then(|image| image_request(image, &album.media.uri))
                })
                .or_else(|| {
                    track
                        .media
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.images.first())
                        .and_then(|image| image_request(image, &track.media.uri))
                })
                .or_else(|| {
                    track
                        .image
                        .as_ref()
                        .and_then(|image| image_request(image, &track.media.uri))
                })
        })
        .take(TILE_COUNT)
        .collect()
}

fn encode_collage(images: &[DynamicImage]) -> Result<String, String> {
    let mut collage = RgbaImage::from_pixel(COLLAGE_SIZE, COLLAGE_SIZE, Rgba([0, 0, 0, 255]));
    for tile_index in 0..TILE_COUNT {
        let image = &images[tile_index % images.len()];
        let tile = image
            .resize_to_fill(
                COLLAGE_SIZE / 2,
                COLLAGE_SIZE / 2,
                imageops::FilterType::Lanczos3,
            )
            .to_rgba8();
        imageops::replace(
            &mut collage,
            &tile,
            ((tile_index % 2) as u32 * COLLAGE_SIZE / 2).into(),
            ((tile_index / 2) as u32 * COLLAGE_SIZE / 2).into(),
        );
    }
    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, 85)
        .encode_image(&DynamicImage::ImageRgb8(
            DynamicImage::ImageRgba8(collage).to_rgb8(),
        ))
        .map_err(|error| format!("encode playlist collage failed: {error}"))?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(encoded)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::repositories::{ImageLoad, ImageUpdate};
    use futures_util::future::BoxFuture;
    use std::sync::Mutex;

    struct RecordingImages {
        requests: Mutex<Vec<ImageRequest>>,
        bytes: Vec<u8>,
        has_cached_image: bool,
        started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    }

    impl ImageLoader for RecordingImages {
        fn read_image<'request>(
            &'request self,
            request: &'request ImageRequest,
        ) -> BoxFuture<'request, Result<ImageLoad, String>> {
            self.requests.lock().unwrap().push(request.clone());
            let bytes = self.bytes.clone();
            let has_cached_image = self.has_cached_image;
            let started = self.started.lock().unwrap().take();
            Box::pin(async move {
                Ok(ImageLoad {
                    current: ImageUpdate {
                        bytes: has_cached_image.then(|| bytes.clone()),
                        cache_error: None,
                    },
                    refresh: Some(Box::pin(async move {
                        if let Some(started) = started {
                            let _ = started.send(());
                        }
                        if has_cached_image {
                            futures_util::future::pending::<()>().await;
                        }
                        Ok(ImageUpdate {
                            bytes: Some(bytes),
                            cache_error: None,
                        })
                    })),
                })
            })
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn collage_uses_model_references_and_local_bytes_without_waiting_for_refresh() {
        for has_cached_image in [true, false] {
            let mut encoded = std::io::Cursor::new(Vec::new());
            DynamicImage::new_rgb8(2, 2)
                .write_to(&mut encoded, image::ImageFormat::Png)
                .unwrap();
            let (started, start) = tokio::sync::oneshot::channel();
            let images = Arc::new(RecordingImages {
                requests: Mutex::new(Vec::new()),
                bytes: encoded.into_inner(),
                has_cached_image,
                started: Mutex::new(Some(started)),
            });
            let tasks = Arc::new(SessionTasks::default());
            let client = Arc::new(
                crate::data::music_assistant::client::MusicAssistantClient::new(
                    "http://127.0.0.1:1",
                    None,
                ),
            );
            let artwork = PlaylistArtwork::new(
                client,
                images.clone(),
                tasks.clone(),
                tokio::runtime::Handle::current(),
            );
            let track: Track = serde_json::from_value(serde_json::json!({"item_id":"7", "provider":"library", "media_type":"track", "name":"track", "uri":"library://track/7", "metadata":{"images":[{"type":"thumb", "provider":"builtin", "path":"server-local/path", "proxy_id":"a".repeat(64)}]}})).unwrap();
            let collage = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                artwork.build_playlist_collage(&[track]),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(collage.starts_with("data:image/jpeg;base64,"));
            start.await.unwrap();
            let requests = images.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(
                requests[0].owner_uri.as_ref().unwrap().as_str(),
                "library://track/7"
            );
            assert_eq!(requests[0].position, 0);
            assert_eq!(requests[0].image.path, "server-local/path");
            drop(requests);
            tasks.join().await.unwrap();
        }
    }
}
