//! Artwork selection never mutates a media record or replaces unrelated metadata.

use super::media::{MediaItem, MediaItemImage};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImageRequest {
    pub(crate) image: MediaItemImage,
    pub(crate) owner_uri: Option<super::identifiers::MediaUri>,
    pub(crate) position: u32,
    #[serde(default = "default_image_size")]
    pub(crate) size: u16,
    pub(crate) provider_icon_role: Option<String>,
}

fn default_image_size() -> u16 {
    512
}

pub(crate) fn is_inline_image(image: &MediaItemImage) -> bool {
    is_inline_source(&image.path)
}

pub(crate) fn is_inline_source(source: &str) -> bool {
    source.starts_with("data:") || source.trim_start().starts_with('<')
}

pub(crate) fn select_artwork(item: &MediaItem) -> Option<MediaItemImage> {
    item.fields()
        .metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .images
                .iter()
                .find(|image| usable_artwork(image))
                .cloned()
        })
        .or_else(|| match item {
            MediaItem::Track(track) => track
                .image
                .as_ref()
                .filter(|image| usable_artwork(image))
                .cloned(),
            _ => None,
        })
}

fn usable_artwork(image: &MediaItemImage) -> bool {
    image.cached_inline.is_some()
        || !image.path.trim().is_empty()
        || image
            .proxy_id
            .as_ref()
            .is_some_and(|proxy| !proxy.trim().is_empty())
}
