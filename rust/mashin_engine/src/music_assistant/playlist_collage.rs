//! Randomized JPEG collage for MA playlist artwork, loaded outside the core coordinator lock.

use super::models::media::{MediaItemImage, Track};
use base64::Engine;
use image::{codecs::jpeg::JpegEncoder, imageops, DynamicImage, Rgba, RgbaImage};
use rand::seq::SliceRandom;
use std::time::Duration;

const COLLAGE_SIZE: u32 = 500;
const TILE_COUNT: usize = 4;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

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

async fn load_image(path: &str, client: &reqwest::Client) -> Option<DynamicImage> {
    let bytes = if let Some((header, encoded)) = path.split_once(',') {
        if !header.starts_with("data:image/")
            || !header.ends_with(";base64")
            || encoded.len() > MAX_IMAGE_BYTES * 2
        {
            return None;
        }
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?
    } else {
        let url = reqwest::Url::parse(path).ok()?;
        if url.scheme() != "https" && url.scheme() != "http" {
            return None;
        }
        let response = client.get(url).send().await.ok()?.error_for_status().ok()?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_IMAGE_BYTES as u64)
        {
            return None;
        }
        response.bytes().await.ok()?.to_vec()
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    image::load_from_memory(&bytes).ok()
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

/// Returns no image if the playlist has no usable cover; a failed individual image is skipped.
pub(crate) async fn build_playlist_collage(tracks: &[Track], base_url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let mut images = Vec::new();
    let paths = image_paths(tracks, base_url, &mut rand::rng());
    for path in paths {
        if let Some(image) = load_image(&path, &client).await {
            images.push(image);
        }
    }
    if images.is_empty() {
        return None;
    }
    encode_collage(&images).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn random_selection_uses_four_distinct_items_from_the_entire_playlist() {
        let tracks = (0..100).map(|index| serde_json::from_value(serde_json::json!({
            "item_id": index.to_string(), "provider":"library", "name":"Track",
            "image":{"type":"thumb", "path":format!("https://example.org/{index}.jpg"), "provider":"builtin"}
        })).unwrap()).collect::<Vec<Track>>();
        let mut random = StdRng::seed_from_u64(42);
        let paths = image_paths(&tracks, "https://music.example", &mut random);
        assert_eq!(paths.len(), 4);
        assert_eq!(
            paths.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
        assert!(paths
            .iter()
            .any(|path| !["0.jpg", "1.jpg", "2.jpg", "3.jpg"]
                .iter()
                .any(|suffix| path.ends_with(suffix))));
    }

    #[test]
    fn collage_is_a_500_pixel_jpeg_data_uri_with_repeated_tiles() {
        let red = DynamicImage::ImageRgba8(RgbaImage::from_pixel(20, 20, Rgba([255, 0, 0, 255])));
        let green = DynamicImage::ImageRgba8(RgbaImage::from_pixel(20, 20, Rgba([0, 255, 0, 255])));
        let data_uri = encode_collage(&[red, green]).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_uri.strip_prefix("data:image/jpeg;base64,").unwrap())
            .unwrap();
        assert_eq!(&bytes[..2], &[0xff, 0xd8]);
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (500, 500));
        assert!(decoded.get_pixel(100, 100)[0] > 200);
        assert!(decoded.get_pixel(350, 100)[1] > 200);
        assert!(decoded.get_pixel(100, 350)[0] > 200);
    }

    #[test]
    fn album_image_precedes_track_image_and_proxy_ids_resolve_against_session() {
        let track: Track = serde_json::from_value(serde_json::json!({
            "item_id": "track", "provider": "library", "name": "Track",
            "image": { "type": "thumb", "path": "https://example.org/track.jpg", "provider": "library" },
            "album": { "item_id": "album", "provider": "library", "name": "Album",
                "metadata": { "images": [{ "type": "thumb", "path": "relative.jpg",
                    "provider": "library", "proxy_id": "album-1" }] } }
        })).unwrap();
        assert_eq!(
            image_paths(&[track], "https://music.example/", &mut rand::rng()),
            ["https://music.example/imageproxy/album-1"]
        );
    }

    #[test]
    fn missing_album_art_falls_back_to_track_image() {
        let track: Track = serde_json::from_value(serde_json::json!({
            "item_id": "t", "provider": "library", "name": "Track",
            "image": { "type": "thumb", "path": "https://music.example/track.jpg", "provider": "library" },
            "album": { "item_id": "a", "provider": "library", "name": "Album",
                "metadata": { "images": [{ "type": "thumb", "path": "", "provider": "library" }] } }
        })).unwrap();
        assert_eq!(
            image_paths(&[track], "https://music.example", &mut rand::rng()),
            ["https://music.example/track.jpg"]
        );
    }

    #[tokio::test]
    async fn data_uri_album_art_generates_a_jpeg_without_network_access() {
        let source = encode_collage(&[DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            20,
            20,
            Rgba([20, 110, 220, 255]),
        ))])
        .unwrap();
        let track: Track = serde_json::from_value(serde_json::json!({
            "item_id": "t", "provider": "library", "name": "Track",
            "album": { "item_id": "a", "provider": "library", "name": "Album",
                "metadata": { "images": [{ "type": "thumb", "path": source,
                    "provider": "library" }] } }
        }))
        .unwrap();
        let result = build_playlist_collage(&[track], "https://music.example")
            .await
            .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result.strip_prefix("data:image/jpeg;base64,").unwrap())
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (500, 500));
        assert!(build_playlist_collage(&[], "https://music.example")
            .await
            .is_none());
    }
}
