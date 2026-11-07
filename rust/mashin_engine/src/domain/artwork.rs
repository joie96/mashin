//! Artwork selection never mutates a media record or replaces unrelated metadata.

use super::media::{MediaItem, MediaItemImage};

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
    !image.path.trim().is_empty()
        || image
            .proxy_id
            .as_ref()
            .is_some_and(|proxy| !proxy.trim().is_empty())
}
