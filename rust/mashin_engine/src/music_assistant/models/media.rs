use super::identifiers::{MediaId, MediaUri, ProviderId, ProviderInstanceId};
use serde::{Deserialize, Deserializer, Serialize};

pub(crate) fn deserialize_nullable_vec<'de, D, Item>(deserializer: D) -> Result<Vec<Item>, D::Error>
where
    D: serde::Deserializer<'de>,
    Item: Deserialize<'de>,
{
    Option::<Vec<Item>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MediaType {
    Artist,
    Album,
    Track,
    Playlist,
    Radio,
    Audiobook,
    Podcast,
    PodcastEpisode,
    Folder,
    Collection,
    Announcement,
    FlowStream,
    PluginSource,
    AudioSource,
    SoundEffect,
    Genre,
    #[serde(other)]
    Unknown,
}

impl Default for MediaType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AlbumType {
    Album,
    Single,
    Live,
    Soundtrack,
    Compilation,
    Ep,
    #[serde(other)]
    Unknown,
}

impl Default for AlbumType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContentType {
    Pcm,
    Opus,
    Flac,
    Mp3,
    Aac,
    Wav,
    Ogg,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AudioFormat {
    #[serde(default)]
    pub(crate) content_type: ContentType,
    #[serde(default)]
    pub(crate) codec_type: ContentType,
    #[serde(default = "default_sample_rate")]
    pub(crate) sample_rate: u32,
    #[serde(default = "default_bit_depth")]
    pub(crate) bit_depth: u8,
    #[serde(default = "default_channels")]
    pub(crate) channels: u8,
    #[serde(default)]
    pub(crate) output_format_str: String,
    #[serde(default)]
    pub(crate) bit_rate: Option<u32>,
}

fn default_sample_rate() -> u32 {
    44_100
}
fn default_bit_depth() -> u8 {
    16
}
fn default_channels() -> u8 {
    2
}

impl Default for ContentType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProviderMapping {
    pub(crate) item_id: MediaId,
    pub(crate) provider_domain: ProviderId,
    pub(crate) provider_instance: ProviderInstanceId,
    #[serde(default = "default_true")]
    pub(crate) available: bool,
    #[serde(default)]
    pub(crate) in_library: Option<bool>,
    #[serde(default)]
    pub(crate) is_unique: Option<bool>,
    #[serde(default)]
    pub(crate) audio_format: Option<AudioFormat>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) details: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MediaItemImage {
    #[serde(rename = "type")]
    pub(crate) image_type: String,
    pub(crate) path: String,
    pub(crate) provider: ProviderInstanceId,
    #[serde(default)]
    pub(crate) remotely_accessible: bool,
    #[serde(default)]
    pub(crate) proxy_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub(crate) struct MediaItemMetadata {
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) description_language: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) images: Vec<MediaItemImage>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) genres: Vec<String>,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) popularity: Option<u32>,
    #[serde(default)]
    pub(crate) cache_checksum: Option<String>,
    #[serde(default)]
    pub(crate) explicit: Option<bool>,
    #[serde(default)]
    pub(crate) lyrics: Option<String>,
    #[serde(default)]
    pub(crate) transcript: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MediaItemFields {
    pub(crate) item_id: MediaId,
    pub(crate) provider: ProviderInstanceId,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) sort_name: Option<String>,
    #[serde(default)]
    pub(crate) uri: Option<MediaUri>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) external_ids: Vec<Vec<String>>,
    #[serde(default = "default_true")]
    pub(crate) is_playable: bool,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) provider_mappings: Vec<ProviderMapping>,
    #[serde(default)]
    pub(crate) metadata: Option<MediaItemMetadata>,
    #[serde(default)]
    pub(crate) favorite: bool,
    #[serde(default)]
    pub(crate) position: Option<u32>,
    #[serde(default)]
    pub(crate) date_added: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Artist {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) artist_type: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Album {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) year: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) artists: Vec<Artist>,
    #[serde(default)]
    pub(crate) album_type: AlbumType,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Track {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) duration: u32,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) artists: Vec<Artist>,
    #[serde(default)]
    pub(crate) album: Option<Album>,
    #[serde(default)]
    pub(crate) image: Option<MediaItemImage>,
    #[serde(default)]
    pub(crate) disc_number: u16,
    #[serde(default)]
    pub(crate) track_number: u16,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Playlist {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) owner: String,
    #[serde(default)]
    pub(crate) is_editable: bool,
    #[serde(default)]
    pub(crate) is_dynamic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub(crate) struct MusicAssistantSearchResults {
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) artists: Vec<Artist>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) albums: Vec<Album>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) tracks: Vec<Track>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) playlists: Vec<Playlist>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    pub(crate) radio: Vec<MediaItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Radio {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) duration: Option<u32>,
    #[serde(default)]
    pub(crate) is_dynamic: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Audiobook {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) duration: u32,
    #[serde(default)]
    pub(crate) resume_position_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Podcast {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) publisher: Option<String>,
    #[serde(default)]
    pub(crate) total_episodes: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PodcastEpisode {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    pub(crate) podcast: Podcast,
    #[serde(default)]
    pub(crate) duration: u32,
    #[serde(default)]
    pub(crate) resume_position_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct GenericMediaItem {
    #[serde(flatten)]
    pub(crate) media: MediaItemFields,
    #[serde(default)]
    pub(crate) duration: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BrowseFolder {
    pub(crate) item_id: MediaId,
    pub(crate) provider: ProviderInstanceId,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) image: Option<MediaItemImage>,
}

#[derive(Deserialize)]
struct ArtistInput {
    #[serde(flatten)]
    media: MediaItemFields,
    #[serde(default)]
    artist_type: Option<String>,
    #[serde(default)]
    image: Option<MediaItemImage>,
}

#[derive(Deserialize)]
struct AlbumInput {
    #[serde(flatten)]
    media: MediaItemFields,
    #[serde(default)]
    year: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_nullable_vec")]
    artists: Vec<Artist>,
    #[serde(default)]
    album_type: AlbumType,
    #[serde(default)]
    image: Option<MediaItemImage>,
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

impl<'de> Deserialize<'de> for Artist {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = ArtistInput::deserialize(deserializer)?;
        let mut media = input.media;
        include_mapping_image(&mut media, input.image);
        Ok(Self {
            media,
            artist_type: input.artist_type,
        })
    }
}

impl<'de> Deserialize<'de> for Album {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = AlbumInput::deserialize(deserializer)?;
        let mut media = input.media;
        include_mapping_image(&mut media, input.image);
        Ok(Self {
            media,
            year: input.year,
            artists: input.artists,
            album_type: input.album_type,
        })
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "media_type", rename_all = "snake_case")]
pub(crate) enum MediaItem {
    Artist(Artist),
    Album(Album),
    Track(Track),
    Playlist(Playlist),
    Radio(Radio),
    Audiobook(Audiobook),
    Podcast(Podcast),
    PodcastEpisode(PodcastEpisode),
    Folder(GenericMediaItem),
    Collection(GenericMediaItem),
    Announcement(GenericMediaItem),
    FlowStream(GenericMediaItem),
    PluginSource(GenericMediaItem),
    AudioSource(GenericMediaItem),
    SoundEffect(GenericMediaItem),
    Genre(GenericMediaItem),
}

impl MediaItem {
    pub(crate) fn fields(&self) -> &MediaItemFields {
        match self {
            Self::Artist(item) => &item.media,
            Self::Album(item) => &item.media,
            Self::Track(item) => &item.media,
            Self::Playlist(item) => &item.media,
            Self::Radio(item) => &item.media,
            Self::Audiobook(item) => &item.media,
            Self::Podcast(item) => &item.media,
            Self::PodcastEpisode(item) => &item.media,
            Self::Folder(item)
            | Self::Collection(item)
            | Self::Announcement(item)
            | Self::FlowStream(item)
            | Self::PluginSource(item)
            | Self::AudioSource(item)
            | Self::SoundEffect(item)
            | Self::Genre(item) => &item.media,
        }
    }
    pub(crate) fn duration_seconds(&self) -> Option<u32> {
        match self {
            Self::Track(item) => Some(item.duration),
            Self::Radio(item) => item.duration,
            Self::Audiobook(item) => Some(item.duration),
            Self::PodcastEpisode(item) => Some(item.duration),
            Self::Folder(item)
            | Self::Collection(item)
            | Self::Announcement(item)
            | Self::FlowStream(item)
            | Self::PluginSource(item)
            | Self::AudioSource(item)
            | Self::SoundEffect(item)
            | Self::Genre(item) => item.duration,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaItem, MediaItemMetadata};
    use serde_json::json;

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
