//! Maps wire fields and provider-specific normalization to canonical domain values.
//! The shared schema preserves field knowledge without a second independently maintained model.

use crate::domain::identifiers::{MediaId, MediaUri, ProviderId, ProviderInstanceId};
use crate::domain::media::*;
use crate::domain::media_schema::{media_schema, provider_schema};
use crate::domain::providers::{ProviderManifest, ProviderManifestKnowledge};
use serde::{Deserialize, Deserializer, Serialize};

macro_rules! wire_media_model {
    ($(#[$attribute:meta])* $model:ident, $input:ident, $input_name:literal, $knowledge:ident {
        required { $($(#[$required_attribute:meta])* $required:ident: $required_type:ty),* $(,)? }
        partial { $($(#[$field_attribute:meta])* $field:ident: $field_type:ty = $default:expr),* $(,)? }
    } $(mapping_image($media:ident, $image:ident))?) => {
        #[derive(Serialize, Deserialize)]
        struct $input {
            $($(#[$required_attribute])* $required: $required_type,)*
            $(#[serde(default, skip_serializing_if = "FieldKnowledge::is_unknown")]
              $(#[$field_attribute])* $field: FieldKnowledge<$field_type>,)*
            $(#[serde(default, skip_serializing)] $image: Option<MediaItemImage>,)?
        }

        impl From<$input> for $model {
            fn from(input: $input) -> Self {
                let knowledge = $knowledge { $($field: !input.$field.is_unknown(),)* };
                let result = Self {
                    $($required: input.$required,)*
                    $($field: match input.$field { FieldKnowledge::Known(value) => value, FieldKnowledge::Unknown => $default },)*
                    knowledge,
                };
                $(let mut result = result; include_mapping_image(&mut result.$media, input.$image);)?
                result
            }
        }

        impl From<$model> for $input {
            fn from(value: $model) -> Self {
                Self {
                    $($required: value.$required,)*
                    $($field: if value.knowledge.$field || value.$field != { let default: $field_type = $default; default } {
                        FieldKnowledge::Known(value.$field)
                    } else { FieldKnowledge::Unknown },)*
                    $($image: None,)?
                }
            }
        }

        impl Serialize for $model {
            fn serialize<Encoder:serde::Serializer>(&self, encoder:Encoder) -> Result<Encoder::Ok, Encoder::Error> {
                $input::from(self.clone()).serialize(encoder)
            }
        }

        impl<'de> Deserialize<'de> for $model {
            fn deserialize<Decoder:Deserializer<'de>>(decoder:Decoder) -> Result<Self, Decoder::Error> {
                $input::deserialize(decoder).map(Self::from)
            }
        }
    };
}

media_schema!(wire_media_model);
provider_schema!(wire_media_model);
fn deserialize_known_vec<'de, Decoder, Item>(
    decoder: Decoder,
) -> Result<FieldKnowledge<Vec<Item>>, Decoder::Error>
where
    Decoder: Deserializer<'de>,
    Item: Deserialize<'de>,
{
    deserialize_nullable_vec(decoder).map(FieldKnowledge::Known)
}

// Providers may return decimal strings; domain and persisted album years remain integers.
fn deserialize_album_year<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AlbumYearInput {
        Number(i32),
        Text(String),
    }

    match Option::<AlbumYearInput>::deserialize(deserializer)? {
        None => Ok(None),
        Some(AlbumYearInput::Number(year)) => Ok(Some(year)),
        Some(AlbumYearInput::Text(year)) => year
            .trim()
            .parse::<i32>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn deserialize_known_album_year<'de, Decoder: Deserializer<'de>>(
    decoder: Decoder,
) -> Result<FieldKnowledge<Option<i32>>, Decoder::Error> {
    deserialize_album_year(decoder).map(FieldKnowledge::Known)
}

#[derive(Deserialize)]
struct PodcastInput {
    #[serde(flatten)]
    media: MediaItemFields,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    total_episodes: Option<u32>,
    #[serde(default)]
    image: Option<MediaItemImage>,
}

// Music Assistant mappings put the preview image beside the media fields.
fn include_mapping_image(media: &mut MediaItemFields, image: Option<MediaItemImage>) {
    if let Some(image) = image {
        let metadata = media
            .metadata
            .get_or_insert_with(MediaItemMetadata::default);
        if metadata.images.is_empty() {
            metadata.images.push(image);
        }
    }
}

impl<'de> Deserialize<'de> for Podcast {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = PodcastInput::deserialize(deserializer)?;
        let mut media = input.media;
        include_mapping_image(&mut media, input.image);
        Ok(Self {
            media,
            publisher: input.publisher,
            total_episodes: input.total_episodes,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn summary_roundtrip_preserves_details_and_explicit_empty_values_clear_them() {
        let mut detail: MediaItem = serde_json::from_value(serde_json::json!({
                "media_type":"track", "provider":"library", "item_id":"42", "name":"Detail", "favorite":false,
                "duration":240, "metadata":{"description":"Loaded", "genres":["Rock"]},
                "album":{"provider":"library","item_id":"album","name":"Album","year":2024},
                "artists":[{"provider":"library","item_id":"artist","name":"Artist","metadata":{"description":"Biography"}}]
            })).unwrap();
        let summary: MediaItem = serde_json::from_value(serde_json::json!({
            "media_type":"track", "provider":"library", "item_id":"42", "name":"Summary",
            "metadata":{"popularity":10},
            "album":{"provider":"library","item_id":"album","name":"Album summary"},
            "artists":[{"provider":"library","item_id":"artist","name":"Artist summary"}]
        }))
        .unwrap();
        let summary = serde_json::from_value(serde_json::to_value(summary).unwrap()).unwrap();
        detail.merge_known(summary);
        assert_eq!(
            detail.fields().favorite,
            super::FieldKnowledge::Known(Some(false))
        );
        assert_eq!(
            detail
                .fields()
                .metadata
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("Loaded")
        );
        assert_eq!(detail.duration_seconds(), Some(240));
        if let MediaItem::Track(track) = &detail {
            assert_eq!(track.album.as_ref().unwrap().year, Some(2024));
            assert_eq!(
                track.artists[0]
                    .media
                    .metadata
                    .as_ref()
                    .unwrap()
                    .description
                    .as_deref(),
                Some("Biography")
            );
        } else {
            panic!("expected track");
        }
        let cleared = serde_json::from_value(serde_json::json!({
                "media_type":"track", "provider":"library", "item_id":"42", "name":"Cleared", "favorite":null,
                "duration":0, "metadata":{"description":null,"genres":[]}
            })).unwrap();
        detail.merge_known(cleared);
        assert_eq!(detail.fields().favorite, super::FieldKnowledge::Known(None));
        let metadata = detail.fields().metadata.as_ref().unwrap();
        assert!(metadata.description.is_none());
        assert!(metadata.genres.is_empty());
        assert_eq!(metadata.popularity, Some(10));
        assert_eq!(detail.duration_seconds(), Some(0));
    }
    #[test]
    fn favorite_field_roundtrip_distinguishes_missing_unrated_disliked_and_liked() {
        use super::{FieldKnowledge, Track};
        for rating in [None, Some(None), Some(Some(false)), Some(Some(true))] {
            let mut input =
                serde_json::json!({"item_id":"42", "provider":"library", "name":"Track"});
            if let Some(value) = rating {
                input["favorite"] = serde_json::json!(value);
            }
            let track: Track = serde_json::from_value(input).unwrap();
            assert_eq!(
                track.media.favorite,
                match rating {
                    None => FieldKnowledge::Unknown,
                    Some(value) => FieldKnowledge::Known(value),
                }
            );
            let serialized = serde_json::to_value(&track).unwrap();
            assert_eq!(
                serialized.get("favorite"),
                rating.map(|value| serde_json::json!(value)).as_ref()
            );
            let restored: Track = serde_json::from_value(serialized).unwrap();
            assert_eq!(restored.media.favorite, track.media.favorite);
        }
    }

    #[test]
    fn missing_summary_rating_preserves_detail_but_authoritative_null_replaces_it() {
        use super::FieldKnowledge;
        let mut rating = FieldKnowledge::Known(Some(true));
        rating.merge(FieldKnowledge::Unknown);
        assert_eq!(rating, FieldKnowledge::Known(Some(true)));
        rating.merge(FieldKnowledge::Known(None));
        assert_eq!(rating, FieldKnowledge::Known(None));
    }

    use super::{Album, MediaItem, MediaItemMetadata, MusicAssistantSearchResults};
    use serde_json::json;

    #[test]
    fn album_years_accept_numbers_strings_null_and_missing_values() {
        for (input, expected) in [
            (json!(2020), Some(2020)),
            (json!("2026"), Some(2026)),
            (json!(" 2023 "), Some(2023)),
            (json!(null), None),
        ] {
            let album: Album = serde_json::from_value(json!({
                "item_id": "album", "provider": "ytmusic--test", "name": "Album",
                "year": input
            }))
            .unwrap();
            assert_eq!(album.year, expected);
            assert_eq!(
                serde_json::to_value(album).unwrap()["year"],
                json!(expected)
            );
        }
        let album: Album = serde_json::from_value(json!({
            "item_id": "album", "provider": "ytmusic--test", "name": "Album"
        }))
        .unwrap();
        assert_eq!(album.year, None);
        for invalid in [
            json!("invalid"),
            json!(2020.5),
            json!(true),
            json!("2147483648"),
        ] {
            assert!(serde_json::from_value::<Album>(json!({
                "item_id": "album", "provider": "ytmusic--test", "name": "Album",
                "year": invalid
            }))
            .is_err());
        }
    }

    #[test]
    fn youtube_search_normalizes_album_years_for_favorite_commands() {
        let results: MusicAssistantSearchResults = serde_json::from_value(json!({
            "albums": [{
                "media_type": "album", "item_id": "MPREb_epgTIaIJXwO",
                "provider": "ytmusic--test", "name": "TAKE ON ME [THE LAST OF US]",
                "uri": "ytmusic--test://album/MPREb_epgTIaIJXwO", "year": "2026",
                "album_type": "single", "metadata": { "images": [], "genres": null }
            }],
            "tracks": [{
                "item_id": "track", "provider": "ytmusic--test", "name": "Track",
                "album": {
                    "item_id": "nested", "provider": "ytmusic--test", "name": "Album",
                    "year": "2023"
                }
            }]
        }))
        .unwrap();
        assert_eq!(results.albums[0].year, Some(2026));
        assert_eq!(results.tracks[0].album.as_ref().unwrap().year, Some(2023));
        let command = serde_json::to_value(MediaItem::Album(results.albums[0].clone())).unwrap();
        assert_eq!(command["year"], json!(2026));
        assert!(matches!(
            serde_json::from_value::<MediaItem>(command).unwrap(),
            MediaItem::Album(_)
        ));
    }

    #[test]
    fn deserializes_null_metadata_collections_as_empty() {
        let metadata: MediaItemMetadata = serde_json::from_str(
            r#"{
                "images": null,
                "genres": null
            }"#,
        )
        .expect("deserialize nullable media metadata collections");

        assert!(metadata.images.is_empty());
        assert!(metadata.genres.is_empty());
    }

    #[test]
    fn normalizes_mapping_images_into_complete_media_models() {
        let playlist: MediaItem = serde_json::from_value(json!({
            "media_type": "playlist",
            "item_id": "artist-radio",
            "provider": "listenbrainz_recommendations--test",
            "name": "Radio: Artist",
            "image": {
                "type": "thumb",
                "path": "https://example.test/artist.jpg",
                "provider": "url",
                "remotely_accessible": true,
                "proxy_id": "playlist-proxy-id"
            }
        }))
        .expect("playlist mapping should deserialize");
        let serialized = serde_json::to_value(playlist).expect("playlist should serialize");
        assert_eq!(
            serialized["metadata"]["images"][0]["proxy_id"],
            "playlist-proxy-id"
        );

        let track: MediaItem = serde_json::from_value(json!({
            "media_type": "track",
            "item_id": "track-1",
            "provider": "library",
            "name": "Track one",
            "artists": [{
                "item_id": "artist-1",
                "provider": "library",
                "name": "Artist one",
                "media_type": "artist",
                "image": {
                    "type": "thumb",
                    "path": "artist.jpg",
                    "provider": "library",
                    "proxy_id": "artist-proxy-id"
                }
            }],
            "album": {
                "item_id": "album-1",
                "provider": "library",
                "name": "Album one",
                "media_type": "album",
                "image": {
                    "type": "thumb",
                    "path": "cover.jpg",
                    "provider": "library",
                    "proxy_id": "cover-proxy-id"
                }
            },
            "image": {
                "type": "thumb",
                "path": "track.jpg",
                "provider": "library",
                "proxy_id": "track-proxy-id"
            }
        }))
        .expect("track with album mapping should deserialize");

        let serialized = serde_json::to_value(track).expect("track should serialize");
        assert_eq!(
            serialized["album"]["metadata"]["images"][0]["proxy_id"],
            "cover-proxy-id"
        );

        assert_eq!(
            serialized["artists"][0]["metadata"]["images"][0]["proxy_id"],
            "artist-proxy-id"
        );
        assert_eq!(serialized["image"]["proxy_id"], "track-proxy-id");

        let episode: MediaItem = serde_json::from_value(json!({
            "media_type": "podcast_episode",
            "item_id": "episode-1",
            "provider": "library",
            "name": "Episode one",
            "podcast": {
                "item_id": "podcast-1",
                "provider": "library",
                "name": "Podcast one",
                "media_type": "podcast",
                "image": {
                    "type": "thumb",
                    "path": "podcast.jpg",
                    "provider": "library",
                    "proxy_id": "podcast-proxy-id"
                }
            }
        }))
        .expect("podcast episode with mapping should deserialize");

        let serialized = serde_json::to_value(episode).expect("episode should serialize");
        assert_eq!(
            serialized["podcast"]["metadata"]["images"][0]["proxy_id"],
            "podcast-proxy-id"
        );
    }
}
