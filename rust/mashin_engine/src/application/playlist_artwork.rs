//! Randomized JPEG collage for MA playlist artwork, loaded outside the core coordinator lock.

use crate::application::repositories::{LibraryClient, NetworkImages};
use crate::application::session::SessionTasks;
use crate::domain::library::PrivatePlaylist;
use crate::domain::media::{MediaItemImage, Track};
use base64::Engine;
use image::{codecs::jpeg::JpegEncoder, imageops, DynamicImage, Rgba, RgbaImage};
use rand::seq::SliceRandom;
use std::sync::Arc;

pub(crate) struct PlaylistArtwork {
    library: Arc<dyn LibraryClient>,
    images: Arc<dyn NetworkImages>,
    base_url: String,
    tasks: Arc<SessionTasks>,
    runtime: tokio::runtime::Handle,
}

impl PlaylistArtwork {
    // Configuration and cover commands; confirmed values are committed by the caller.
    pub(crate) fn new(
        library: Arc<dyn LibraryClient>,
        images: Arc<dyn NetworkImages>,
        base_url: String,
        tasks: Arc<SessionTasks>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            library,
            images,
            base_url,
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
        let paths = image_paths(tracks, &self.base_url, &mut rand::rng());
        let mut images = Vec::new();
        for path in paths {
            if let Some(bytes) = self.images.load_bytes(&path).await {
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

fn image_source(image: &MediaItemImage, base_url: &str) -> Option<String> {
    if image.path.starts_with("data:") {
        return Some(image.path.clone());
    }
    if let Some(proxy_id) = image.proxy_id.as_deref().filter(|id| !id.is_empty()) {
        if let Ok(mut url) = reqwest::Url::parse(base_url) {
            if let Ok(mut segments) = url.path_segments_mut() {
                segments.pop_if_empty().push("imageproxy").push(proxy_id);
            }
            return Some(url.into());
        }
    }
    (!image.path.is_empty()).then(|| image.path.clone())
}

fn image_paths(tracks: &[Track], base_url: &str, random: &mut impl rand::Rng) -> Vec<String> {
    let mut candidates = tracks.iter().collect::<Vec<_>>();
    candidates.shuffle(random);
    candidates
        .into_iter()
        .filter_map(|track| {
            track
                .album
                .as_ref()
                .and_then(|album| album.media.metadata.as_ref())
                .and_then(|metadata| metadata.images.first())
                .and_then(|image| image_source(image, base_url))
                .or_else(|| {
                    track
                        .media
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.images.first())
                        .and_then(|image| image_source(image, base_url))
                })
                .or_else(|| {
                    track
                        .image
                        .as_ref()
                        .and_then(|image| image_source(image, base_url))
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
