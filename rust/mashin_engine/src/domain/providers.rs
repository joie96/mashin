//! Provider facts and effective media availability; no transport or storage ownership.

use super::identifiers::ProviderInstanceId;
use super::media::{partial_media_model, Album, MediaItem, MediaItemFields, Track};
use super::media_schema::provider_schema;
use serde::{Deserialize, Serialize};

provider_schema!(partial_media_model);

impl ProviderManifest {
    pub(crate) fn merge_known(&mut self, incoming: Self) {
        self.merge_partial(incoming);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProviderInstance {
    pub(crate) instance_id: ProviderInstanceId,
    pub(crate) available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaAvailability {
    Available,
    Unavailable,
    Unknown,
}

// Missing live knowledge never overrides a definite metadata restriction.
pub(crate) fn media_availability(
    media: &MediaItemFields,
    providers: Option<&[ProviderInstance]>,
) -> MediaAvailability {
    if !media.is_available() {
        return MediaAvailability::Unavailable;
    }
    if media.provider_mappings.is_empty() {
        return MediaAvailability::Available;
    }
    let Some(providers) = providers else {
        return MediaAvailability::Unknown;
    };
    if media.provider_mappings.iter().any(|mapping| {
        mapping.available
            && providers.iter().any(|provider| {
                provider.instance_id == mapping.provider_instance && provider.available
            })
    }) {
        MediaAvailability::Available
    } else {
        MediaAvailability::Unavailable
    }
}

// Derived flags belong to read projections, never to the confirmed metadata record.
pub(crate) fn project_fields_availability(
    media: &mut MediaItemFields,
    providers: Option<&[ProviderInstance]>,
) {
    media.available = Some(media_availability(media, providers) != MediaAvailability::Unavailable);
}

pub(crate) fn project_album_availability(
    album: &mut Album,
    providers: Option<&[ProviderInstance]>,
) {
    project_fields_availability(&mut album.media, providers);
    for artist in &mut album.artists {
        project_fields_availability(&mut artist.media, providers);
    }
}

pub(crate) fn project_track_availability(
    track: &mut Track,
    providers: Option<&[ProviderInstance]>,
) {
    project_fields_availability(&mut track.media, providers);
    for artist in &mut track.artists {
        project_fields_availability(&mut artist.media, providers);
    }
    if let Some(album) = &mut track.album {
        project_album_availability(album, providers);
    }
}

pub(crate) fn project_item_availability(
    item: &mut MediaItem,
    providers: Option<&[ProviderInstance]>,
) {
    match item {
        MediaItem::Track(track) => project_track_availability(track, providers),
        MediaItem::Album(album) => project_album_availability(album, providers),
        MediaItem::PodcastEpisode(episode) => {
            project_fields_availability(&mut episode.media, providers);
            project_fields_availability(&mut episode.podcast.media, providers);
        }
        item => project_fields_availability(item.fields_mut(), providers),
    }
}

#[cfg(test)]
mod tests {
    use super::{media_availability, MediaAvailability};

    use serde_json::json;

    #[test]
    fn unknown_live_status_preserves_definite_metadata_restrictions() {
        for extra in [
            json!({"available":false}),
            json!({"provider_mappings":[{
                "item_id":"one","provider_domain":"test","provider_instance":"one","available":false
            }]}),
        ] {
            let mut input = json!({"item_id":"track","provider":"library","name":"Track"});
            input
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let media = serde_json::from_value(input).unwrap();
            assert_eq!(
                media_availability(&media, None),
                MediaAvailability::Unavailable
            );
        }
    }
}
