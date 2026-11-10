//! Image identity within the selected server's independent image index.
//! Source keys retain observed identities, not URLs constructed for transport or byte hashes.
//! These values perform no I/O; generation checks and publication belong to the image owner.

use crate::domain::identifiers::ProviderInstanceId;
use crate::domain::media::MediaItemImage;

use super::database::CacheError;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::data) enum ImageSourceKey {
    ProxyId(String),
    ModelReference {
        provider: ProviderInstanceId,
        path: String,
    },
    InlineReference {
        provider: ProviderInstanceId,
        owner_uri: String,
        image_role: String,
        position: u32,
    },
    ProviderIconReference {
        provider: ProviderInstanceId,
        role: String,
    },
}

impl ImageSourceKey {
    pub(super) fn from_provider_icon(
        image: &MediaItemImage,
        role: &str,
    ) -> Result<Self, CacheError> {
        if image.proxy_id.as_deref().is_some_and(|id| !id.is_empty()) {
            return Self::from_image(image);
        }
        validate_provider_icon_reference(&image.provider, role)?;
        Ok(Self::ProviderIconReference {
            provider: image.provider.clone(),
            role: role.to_owned(),
        })
    }

    // Inline bytes are associated with their observed model slot, never a generated source ID.
    pub(super) fn from_inline(
        image: &MediaItemImage,
        owner_uri: &str,
        position: u32,
    ) -> Result<Self, CacheError> {
        if image.proxy_id.as_deref().is_some_and(|id| !id.is_empty()) {
            return Self::from_image(image);
        }
        validate_inline_reference(&image.provider, owner_uri, &image.image_type)?;
        Ok(Self::InlineReference {
            provider: image.provider.clone(),
            owner_uri: owner_uri.to_owned(),
            image_role: image.image_type.clone(),
            position,
        })
    }

    // Reference fallback identifies inline/model content; it does not authorize a URL fetch.
    pub(super) fn from_image(image: &MediaItemImage) -> Result<Self, CacheError> {
        if let Some(proxy_id) = image.proxy_id.as_deref().filter(|value| !value.is_empty()) {
            if proxy_id.len() != 64 || !proxy_id.bytes().all(|value| value.is_ascii_hexdigit()) {
                return Err(CacheError::InvalidImageObservation("invalid proxy ID"));
            }
            return Ok(Self::ProxyId(proxy_id.to_owned()));
        }
        if image.provider.as_str().trim().is_empty() || image.path.trim().is_empty() {
            return Err(CacheError::InvalidImageObservation(
                "missing image reference",
            ));
        }
        Ok(Self::ModelReference {
            provider: image.provider.clone(),
            path: image.path.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::data) enum ImageSize {
    Original,
    Pixels80,
    Pixels160,
    Pixels256,
    Pixels512,
    Pixels1024,
}

impl ImageSize {
    // Proxy request parameters
    pub(super) fn from_pixels(pixels: u16) -> Result<Self, CacheError> {
        match pixels {
            0 => Ok(Self::Original),
            80 => Ok(Self::Pixels80),
            160 => Ok(Self::Pixels160),
            256 => Ok(Self::Pixels256),
            512 => Ok(Self::Pixels512),
            1024 => Ok(Self::Pixels1024),
            _ => Err(CacheError::InvalidImageObservation(
                "unsupported proxy image size",
            )),
        }
    }

    pub(in crate::data) fn pixels(self) -> u16 {
        match self {
            Self::Original => 0,
            Self::Pixels80 => 80,
            Self::Pixels160 => 160,
            Self::Pixels256 => 256,
            Self::Pixels512 => 512,
            Self::Pixels1024 => 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::data) enum ImageOutputFormat {
    Automatic,
    JpegPreserveTransparency,
    JpegFlattenTransparency,
    Png,
    Svg,
}

impl ImageOutputFormat {
    // Requested format is not the MIME type of the returned bytes.
    pub(in crate::data) fn proxy_parameter(self) -> Option<&'static str> {
        match self {
            Self::Automatic => None,
            Self::JpegPreserveTransparency => Some("jpg"),
            Self::JpegFlattenTransparency => Some("jpeg"),
            Self::Png => Some("png"),
            Self::Svg => Some("svg"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::data) struct ImageVariantParameters {
    pub(in crate::data) size: ImageSize,
    pub(in crate::data) output_format: ImageOutputFormat,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::data) struct ImageVariantKey {
    pub(in crate::data) source: ImageSourceKey,
    pub(in crate::data) parameters: ImageVariantParameters,
}

pub(super) fn validate_provider_icon_reference(
    provider: &ProviderInstanceId,
    role: &str,
) -> Result<(), CacheError> {
    if provider.as_str().is_empty()
        || !provider
            .as_str()
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-'))
        || !matches!(
            role,
            "icon" | "icon_svg" | "icon_svg_dark" | "icon_svg_monochrome" | "default"
        )
    {
        return Err(CacheError::InvalidImageObservation(
            "invalid provider icon reference",
        ));
    }
    Ok(())
}

pub(super) fn validate_inline_reference(
    provider: &ProviderInstanceId,
    owner_uri: &str,
    image_role: &str,
) -> Result<(), CacheError> {
    let invalid = || CacheError::InvalidImageObservation("invalid inline model reference");
    let uri = reqwest::Url::parse(owner_uri).map_err(|_| invalid())?;
    if provider.as_str().trim().is_empty()
        || uri.host_str().is_none()
        || matches!(uri.scheme(), "http" | "https" | "data" | "file")
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
        || image_role.is_empty()
        || !image_role
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-'))
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;
    use crate::data::cache::image_files::ImagePayload;

    fn image(provider: &str, path: &str, proxy_id: Option<String>) -> MediaItemImage {
        MediaItemImage {
            image_type: "thumb".to_owned(),
            provider: ProviderInstanceId(provider.to_owned()),
            path: path.to_owned(),
            remotely_accessible: false,
            proxy_id,
            cached_inline: None,
        }
    }

    #[test]
    fn inline_slots_preserve_model_identity_and_observed_proxy_ids_take_priority() {
        let original = image("builtin", "data:image/png;base64,AAAA", None);
        let source = ImageSourceKey::from_inline(&original, "library://album/7", 0).unwrap();
        let mut changed = original.clone();
        changed.path = "data:image/png;base64,BBBB".into();
        assert_eq!(
            source,
            ImageSourceKey::from_inline(&changed, "library://album/7", 0).unwrap()
        );
        assert_ne!(
            source,
            ImageSourceKey::from_inline(&changed, "library://album/7", 1).unwrap()
        );
        assert_ne!(
            source,
            ImageSourceKey::from_inline(&changed, "library://album/8", 0).unwrap()
        );
        changed.image_type = "fanart".into();
        assert_ne!(
            source,
            ImageSourceKey::from_inline(&changed, "library://album/7", 0).unwrap()
        );
        for uri in [
            "",
            "data:image/png;base64,AAAA",
            "https://server/cover?token=secret",
            "library://user:secret@album/7",
            "library://album/7?token=secret",
        ] {
            assert!(ImageSourceKey::from_inline(&original, uri, 0).is_err());
        }
        changed.proxy_id = Some("a".repeat(64));
        assert_eq!(
            ImageSourceKey::from_inline(&changed, "", 0).unwrap(),
            ImageSourceKey::ProxyId("a".repeat(64))
        );
        changed.proxy_id = Some("invalid".into());
        assert!(ImageSourceKey::from_inline(&changed, "library://album/7", 0).is_err());
    }

    #[test]
    fn proxy_source_uses_observed_id_not_reference_or_variant() {
        let proxy_id = "Ab".repeat(32);
        let original = image("provider_a", "server-local/old.png", Some(proxy_id.clone()));
        let mut changed_reference =
            image("provider_b", "server-local/new.jpg", Some(proxy_id.clone()));
        changed_reference.image_type = "fanart".to_owned();
        changed_reference.remotely_accessible = true;
        let source = ImageSourceKey::from_image(&original).unwrap();
        assert_eq!(source, ImageSourceKey::ProxyId(proxy_id));
        assert_eq!(
            source,
            ImageSourceKey::from_image(&changed_reference).unwrap()
        );
        assert_ne!(
            source,
            ImageSourceKey::from_image(&image(
                "provider_a",
                "server-local/old.png",
                Some("c".repeat(64))
            ))
            .unwrap()
        );
        let small = ImageVariantKey {
            source: source.clone(),
            parameters: ImageVariantParameters {
                size: ImageSize::Pixels80,
                output_format: ImageOutputFormat::Png,
            },
        };
        let large = ImageVariantKey {
            source,
            parameters: ImageVariantParameters {
                size: ImageSize::Pixels512,
                ..small.parameters
            },
        };
        assert_eq!(small.source, large.source);
        assert_ne!(small, large);
    }

    #[test]
    fn reference_fallback_preserves_provider_and_exact_path_without_inventing_ids() {
        let reference = image("provider_a", "007/cover.png?revision=2", None);
        let source = ImageSourceKey::from_image(&reference).unwrap();
        assert_eq!(
            source,
            ImageSourceKey::ModelReference {
                provider: ProviderInstanceId("provider_a".to_owned()),
                path: "007/cover.png?revision=2".to_owned(),
            }
        );
        assert_ne!(
            source,
            ImageSourceKey::from_image(&image("provider_b", &reference.path, None)).unwrap()
        );
        assert_ne!(
            source,
            ImageSourceKey::from_image(&image("provider_a", "7/cover.png?revision=2", None))
                .unwrap()
        );
        let mut empty_proxy = reference.clone();
        empty_proxy.proxy_id = Some(String::new());
        assert_eq!(source, ImageSourceKey::from_image(&empty_proxy).unwrap());
        let inline = image("builtin", "data:image/png;base64,reference", None);
        assert_eq!(
            ImageSourceKey::from_image(&inline).unwrap(),
            ImageSourceKey::ModelReference {
                provider: ProviderInstanceId("builtin".to_owned()),
                path: inline.path,
            }
        );
    }

    #[test]
    fn invalid_proxy_ids_do_not_silently_fall_back_to_another_source() {
        for proxy_id in [
            "short".to_owned(),
            "g".repeat(64),
            " ".repeat(64),
            format!("{}/extra", "a".repeat(64)),
        ] {
            assert!(ImageSourceKey::from_image(&image(
                "builtin",
                "valid-reference",
                Some(proxy_id)
            ))
            .is_err());
        }
        for reference in [image("", "cover.png", None), image("builtin", " ", None)] {
            assert!(ImageSourceKey::from_image(&reference).is_err());
        }
        assert!(ImageSourceKey::from_image(&image("", "", Some("a".repeat(64)))).is_ok());
    }

    #[test]
    fn variants_preserve_server_sizes_format_and_transparency_semantics() {
        for pixels in [0, 80, 160, 256, 512, 1024] {
            assert_eq!(ImageSize::from_pixels(pixels).unwrap().pixels(), pixels);
        }
        for pixels in [1, 79, 300, 2048, u16::MAX] {
            assert!(ImageSize::from_pixels(pixels).is_err());
        }
        let source = ImageSourceKey::ProxyId("a".repeat(64));
        let variants = [
            (ImageOutputFormat::Automatic, None),
            (ImageOutputFormat::JpegPreserveTransparency, Some("jpg")),
            (ImageOutputFormat::JpegFlattenTransparency, Some("jpeg")),
            (ImageOutputFormat::Png, Some("png")),
            (ImageOutputFormat::Svg, Some("svg")),
        ];
        for (index, (output_format, parameter)) in variants.iter().enumerate() {
            assert_eq!(output_format.proxy_parameter(), *parameter);
            let key = ImageVariantKey {
                source: source.clone(),
                parameters: ImageVariantParameters {
                    size: ImageSize::Pixels256,
                    output_format: *output_format,
                },
            };
            for (other_format, _) in &variants[index + 1..] {
                assert_ne!(
                    key,
                    ImageVariantKey {
                        source: source.clone(),
                        parameters: ImageVariantParameters {
                            output_format: *other_format,
                            ..key.parameters
                        }
                    }
                );
            }
        }
    }

    #[test]
    fn new_validated_bytes_keep_the_same_proxy_source_and_variant() {
        let reference = image("builtin", "playlist-cover", Some("a".repeat(64)));
        let key = ImageVariantKey {
            source: ImageSourceKey::from_image(&reference).unwrap(),
            parameters: ImageVariantParameters {
                size: ImageSize::Original,
                output_format: ImageOutputFormat::JpegPreserveTransparency,
            },
        };
        let mut payloads = Vec::new();
        for color in [0, 255] {
            let bitmap = image::RgbImage::from_pixel(2, 2, image::Rgb([color, 0, 0]));
            let mut encoded = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(bitmap)
                .write_to(&mut encoded, image::ImageFormat::Png)
                .unwrap();
            let data_uri = format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(encoded.into_inner())
            );
            payloads.push(ImagePayload::decode_inline(&data_uri).unwrap());
            assert_eq!(key.source, ImageSourceKey::from_image(&reference).unwrap());
        }
        assert_ne!(payloads[0].bytes, payloads[1].bytes);
        assert_eq!(payloads[0].mime_type, "image/png");
        assert_eq!(payloads[1].mime_type, "image/png");
        assert_eq!(key.parameters.output_format.proxy_parameter(), Some("jpg"));
    }
}
