use super::identifiers::{MediaId, MediaUri, ProviderId, ProviderInstanceId};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FieldKnowledge<Value> {
    #[default]
    Unknown,
    Known(Value),
}

impl<Value> FieldKnowledge<Value> {
    // Missing summary fields cannot erase authoritative detail knowledge.
    pub(crate) fn merge(&mut self, incoming: Self) {
        if matches!(incoming, Self::Known(_)) {
            *self = incoming;
        }
    }

    // Field absence is independent of a known nullable server value.
    pub(crate) fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub(crate) fn known_value(&self) -> Option<&Value> {
        match self {
            Self::Unknown => None,
            Self::Known(value) => Some(value),
        }
    }
}

impl<'de, Value: Deserialize<'de>> Deserialize<'de> for FieldKnowledge<Value> {
    fn deserialize<Decoder: Deserializer<'de>>(decoder: Decoder) -> Result<Self, Decoder::Error> {
        Value::deserialize(decoder).map(Self::Known)
    }
}

impl<Value: Serialize> Serialize for FieldKnowledge<Value> {
    fn serialize<Encoder: serde::Serializer>(
        &self,
        encoder: Encoder,
    ) -> Result<Encoder::Ok, Encoder::Error> {
        match self {
            Self::Known(value) => value.serialize(encoder),
            Self::Unknown => encoder.serialize_none(),
        }
    }
}

pub(crate) fn deserialize_nullable_vec<'de, D, Item>(deserializer: D) -> Result<Vec<Item>, D::Error>
where
    D: serde::Deserializer<'de>,
    Item: Deserialize<'de>,
{
    Option::<Vec<Item>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

// One schema keeps typed values, field presence, serialization and partial merges in agreement.
macro_rules! partial_media_model {
    ($(#[$attribute:meta])* $model:ident, $input:ident, $input_name:literal, $knowledge:ident {
        required { $($(#[$required_attribute:meta])* $required:ident: $required_type:ty),* $(,)? }
        partial { $($(#[$field_attribute:meta])* $field:ident: $field_type:ty = $default:expr),* $(,)? }
    } $(mapping_image($media:ident, $image:ident))?) => {
        $(#[$attribute])*
        #[derive(Debug, Clone, PartialEq)]
        pub(crate) struct $model {
            $(pub(crate) $required: $required_type,)*
            $(pub(crate) $field: $field_type,)*
            pub(crate) knowledge: $knowledge,
        }

        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub(crate) struct $knowledge { $(pub(crate) $field: bool,)* }

        impl $model {
            fn merge_partial(&mut self, incoming: Self) {
                $(if incoming.knowledge.$field || incoming.$field != { let default: $field_type = $default; default } {
                    self.$field = incoming.$field;
                    self.knowledge.$field = true;
                })*
            }
        }
    };
}

use super::media_schema::media_schema;
pub(crate) use partial_media_model;
media_schema!(partial_media_model);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

impl Default for ContentType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CachedInlineImage {
    pub(crate) owner_uri: Option<MediaUri>,
    pub(crate) position: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MediaItemImage {
    #[serde(rename = "type")]
    pub(crate) image_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) path: String,
    pub(crate) provider: ProviderInstanceId,
    #[serde(default)]
    pub(crate) remotely_accessible: bool,
    #[serde(default)]
    pub(crate) proxy_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) cached_inline: Option<CachedInlineImage>,
}

impl MediaItemFields {
    // Availability remains optimistic when neither mappings nor a server flag are known.
    pub(crate) fn is_available(&self) -> bool {
        self.available != Some(false)
            && (self.provider_mappings.is_empty()
                || self
                    .provider_mappings
                    .iter()
                    .any(|mapping| mapping.available))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PlaylistAccess {
    pub(crate) owner: Option<String>,
    pub(crate) sharing: PlaylistSharing,
    #[serde(default)]
    pub(crate) shared_users: Vec<String>,
    #[serde(default)]
    pub(crate) collaborative: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlaylistSharing {
    Private,
    All,
    Selected,
    #[serde(other)]
    Unknown,
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
    // Typed media identity for catalog queries.
    pub(crate) fn media_type(&self) -> MediaType {
        match self {
            Self::Artist(_) => MediaType::Artist,
            Self::Album(_) => MediaType::Album,
            Self::Track(_) => MediaType::Track,
            Self::Playlist(_) => MediaType::Playlist,
            Self::Radio(_) => MediaType::Radio,
            Self::Audiobook(_) => MediaType::Audiobook,
            Self::Podcast(_) => MediaType::Podcast,
            Self::PodcastEpisode(_) => MediaType::PodcastEpisode,
            Self::Folder(_) => MediaType::Folder,
            Self::Collection(_) => MediaType::Collection,
            Self::Announcement(_) => MediaType::Announcement,
            Self::FlowStream(_) => MediaType::FlowStream,
            Self::PluginSource(_) => MediaType::PluginSource,
            Self::AudioSource(_) => MediaType::AudioSource,
            Self::SoundEffect(_) => MediaType::SoundEffect,
            Self::Genre(_) => MediaType::Genre,
        }
    }

    // Merge loaded fields only; an external summary cannot undo a library canonical identity.
    pub(crate) fn merge_known(&mut self, mut incoming: Self) {
        match (&*self, &mut incoming) {
            (Self::Track(current), Self::Track(incoming)) => {
                Self::merge_artists(&current.artists, &mut incoming.artists);
                if let (Some(previous), Some(album)) = (&current.album, &mut incoming.album) {
                    if previous.media.provider == album.media.provider
                        && previous.media.item_id == album.media.item_id
                    {
                        let mut merged = Self::Album(previous.clone());
                        merged.merge_known(Self::Album(album.clone()));
                        if let Self::Album(merged) = merged {
                            *album = merged;
                        }
                    }
                }
            }
            (Self::Album(current), Self::Album(incoming)) => {
                Self::merge_artists(&current.artists, &mut incoming.artists)
            }
            _ => {}
        }
        let mut fields = incoming.fields().clone();
        match (&mut *self, &incoming) {
            (Self::Track(current), Self::Track(incoming)) => {
                current.merge_partial(incoming.clone())
            }
            (Self::Artist(current), Self::Artist(incoming)) => {
                current.merge_partial(incoming.clone())
            }
            (Self::Album(current), Self::Album(incoming)) => {
                current.merge_partial(incoming.clone())
            }
            (Self::Playlist(current), Self::Playlist(incoming)) => {
                current.merge_partial(incoming.clone())
            }
            _ => {}
        }
        let current = self.fields_mut();
        for mapping in &mut fields.provider_mappings {
            if let Some(previous) = current.provider_mappings.iter().find(|previous| {
                previous.item_id == mapping.item_id
                    && previous.provider_instance == mapping.provider_instance
                    && previous.provider_domain == mapping.provider_domain
            }) {
                let mut merged = previous.clone();
                if let (Some(previous_format), Some(incoming_format)) =
                    (&previous.audio_format, &mut mapping.audio_format)
                {
                    let mut format = previous_format.clone();
                    format.merge_partial(incoming_format.clone());
                    *incoming_format = format;
                }
                merged.merge_partial(mapping.clone());
                *mapping = merged;
            }
        }
        if let (Some(previous), Some(incoming)) = (&current.metadata, &mut fields.metadata) {
            let mut merged = previous.clone();
            merged.merge_partial(incoming.clone());
            *incoming = merged;
        }
        if fields.provider.as_str() == "library" || current.provider.as_str() != "library" {
            current.item_id = fields.item_id.clone();
            current.provider = fields.provider.clone();
        }
        current.name = fields.name.clone();
        current.favorite.merge(fields.favorite);
        current.merge_partial(fields);
    }

    // Shared media fields and playback facts.
    pub(crate) fn fields_mut(&mut self) -> &mut MediaItemFields {
        match self {
            Self::Artist(item) => &mut item.media,
            Self::Album(item) => &mut item.media,
            Self::Track(item) => &mut item.media,
            Self::Playlist(item) => &mut item.media,
            Self::Radio(item) => &mut item.media,
            Self::Audiobook(item) => &mut item.media,
            Self::Podcast(item) => &mut item.media,
            Self::PodcastEpisode(item) => &mut item.media,
            Self::Folder(item)
            | Self::Collection(item)
            | Self::Announcement(item)
            | Self::FlowStream(item)
            | Self::PluginSource(item)
            | Self::AudioSource(item)
            | Self::SoundEffect(item)
            | Self::Genre(item) => &mut item.media,
        }
    }

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

    // Embedded summaries use the same field-knowledge merge as top-level records.
    fn merge_artists(current: &[Artist], incoming: &mut [Artist]) {
        for artist in incoming {
            if let Some(previous) = current.iter().find(|previous| {
                previous.media.provider == artist.media.provider
                    && previous.media.item_id == artist.media.item_id
            }) {
                let mut merged = Self::Artist(previous.clone());
                merged.merge_known(Self::Artist(artist.clone()));
                if let Self::Artist(merged) = merged {
                    *artist = merged;
                }
            }
        }
    }
}
