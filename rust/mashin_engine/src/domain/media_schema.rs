//! Shared canonical fields and defaults; emitted as domain values and boundary wire models.
macro_rules! provider_schema {
    ($emit:ident) => {
        $emit! {
            #[derive(Eq)]
            ProviderManifest, ProviderManifestInput, "ProviderManifestInput", ProviderManifestKnowledge {
                required {domain:String}
                partial {
                    name:Option<String>=None,
                    description:Option<String>=None,
                    icon:Option<String>=None,
                    icon_svg:Option<String>=None,
                    icon_svg_dark:Option<String>=None,
                    icon_svg_monochrome:Option<String>=None,
                    #[serde(deserialize_with = "deserialize_known_vec")]
                    icon_images:Vec<String>=Vec::new(),
                    #[serde(rename="type")]
                    provider_type:Option<String>=None,
                    builtin:Option<bool>=None,
                }
            }
        }
    };
}
pub(crate) use provider_schema;

macro_rules! media_schema {
    ($emit:ident) => {
        $emit! {
            AudioFormat, AudioFormatInput, "AudioFormatInput", AudioFormatKnowledge {
                required {}
                partial {
                    content_type: ContentType = ContentType::Unknown,
                    codec_type: ContentType = ContentType::Unknown,
                    sample_rate: u32 = 44_100,
                    bit_depth: u8 = 16,
                    channels: u8 = 2,
                    output_format_str: String = String::new(),
                    bit_rate: Option<u32> = None,
                }
            }
        }
        $emit! {
            ProviderMapping, ProviderMappingInput, "ProviderMappingInput", ProviderMappingKnowledge {
                required {
                    item_id: MediaId,
                    provider_domain: ProviderId,
                    provider_instance: ProviderInstanceId,
                }
                partial {
                    available: bool = true,
                    in_library: Option<bool> = None,
                    is_unique: Option<bool> = None,
                    audio_format: Option<AudioFormat> = None,
                    url: Option<String> = None,
                    details: Option<String> = None,
                }
            }
        }
        $emit! {
            #[derive(Default)]
            MediaItemMetadata, MediaItemMetadataInput, "MediaItemMetadataInput", MetadataKnowledge {
                required {}
                partial {
                    description: Option<String> = None,
                    description_language: Option<String> = None,
                    #[serde(deserialize_with = "deserialize_known_vec")]
                    images: Vec<MediaItemImage> = Vec::new(),
                    #[serde(deserialize_with = "deserialize_known_vec")]
                    genres: Vec<String> = Vec::new(),
                    label: Option<String> = None,
                    popularity: Option<u32> = None,
                    cache_checksum: Option<String> = None,
                    explicit: Option<bool> = None,
                    lyrics: Option<String> = None,
                    transcript: Option<String> = None,
                }
            }
        }
        $emit! {
            MediaItemFields, MediaItemFieldsInput, "MediaItemFieldsInput", MediaFieldsKnowledge {
                required {
                    item_id: MediaId,
                    provider: ProviderInstanceId,
                    name: String,
                    #[serde(default, skip_serializing_if = "FieldKnowledge::is_unknown")]
                    favorite: FieldKnowledge<Option<bool>>,
                }
                partial {
                    version: String = String::new(),
                    sort_name: Option<String> = None,
                    uri: Option<MediaUri> = None,
                    #[serde(deserialize_with = "deserialize_known_vec")]
                    external_ids: Vec<Vec<String>> = Vec::new(),
                    is_playable: bool = true,
                    available: Option<bool> = None,
                    #[serde(deserialize_with = "deserialize_known_vec")]
                    provider_mappings: Vec<ProviderMapping> = Vec::new(),
                    metadata: Option<MediaItemMetadata> = None,
                    position: Option<u32> = None,
                    date_added: Option<String> = None,
                }
            }
        }
        $emit! {
            Artist, ArtistInput, "ArtistInput", ArtistKnowledge {
                required { #[serde(flatten)] media: MediaItemFields }
                partial { artist_type: Option<String> = None }
            } mapping_image(media, image)
        }
        $emit! {
            Album, AlbumInput, "AlbumInput", AlbumKnowledge {
                required { #[serde(flatten)] media: MediaItemFields }
                partial {
                    #[serde(deserialize_with = "deserialize_known_album_year")]
                    year: Option<i32> = None,
                    #[serde(deserialize_with = "deserialize_known_vec")]
                    artists: Vec<Artist> = Vec::new(),
                    album_type: AlbumType = AlbumType::Unknown,
                }
            } mapping_image(media, image)
        }
        $emit! {
            Track, TrackInput, "TrackInput", TrackKnowledge {
                required { #[serde(flatten)] media: MediaItemFields }
                partial {
                    duration: u32 = 0,
                    #[serde(deserialize_with = "deserialize_known_vec")]
                    artists: Vec<Artist> = Vec::new(),
                    album: Option<Album> = None,
                    image: Option<MediaItemImage> = None,
                    disc_number: u16 = 0,
                    track_number: u16 = 0,
                }
            }
        }
        $emit! {
            Playlist, PlaylistInput, "PlaylistInput", PlaylistKnowledge {
                required { #[serde(flatten)] media: MediaItemFields }
                partial {
                    owner: String = String::new(),
                    is_editable: bool = false,
                    is_dynamic: bool = false,
                    access: Option<PlaylistAccess> = None,
                }
            } mapping_image(media, image)
        }
    };
}
pub(crate) use media_schema;
