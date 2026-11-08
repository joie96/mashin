//! Blocking bitmap validation and file publication for the independent image cache.
//! Callers own generation checks and quiescence; no decoding or I/O belongs on an executor.

use base64::Engine;
use image::ImageFormat;

use super::database::CacheError;

pub(super) struct ImagePayload {
    pub(super) bytes: Vec<u8>,
    pub(super) mime_type: &'static str,
}

impl ImagePayload {
    // Inline normalization validates actual content, not only the declared media type.
    pub(super) fn decode_inline(source: &str) -> Result<Self, CacheError> {
        let (header, encoded) = source.split_once(',').ok_or(CacheError::InvalidImageObservation("invalid data URI"))?;
        let declared = header.strip_prefix("data:").and_then(|value| value.strip_suffix(";base64"))
            .ok_or(CacheError::InvalidImageObservation("image requires a base64 data URI"))?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)
            .map_err(|_| CacheError::InvalidImageObservation("invalid image base64"))?;
        let format = image::guess_format(&bytes)
            .map_err(|_| CacheError::InvalidImageObservation("unsupported image content"))?;
        let mime_type = match format {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Gif => "image/gif",
            _ => return Err(CacheError::InvalidImageObservation("unsupported bitmap format")),
        };
        if declared != mime_type {
            return Err(CacheError::InvalidImageObservation("image MIME type contradicts content"));
        }
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().map_err(|_| CacheError::InvalidImageObservation("invalid or unsafe bitmap"))?;
        Ok(Self { bytes, mime_type })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_images_validate_bitmap_content_and_reject_active_or_mislabeled_data() {
        for (format, mime_type) in [(ImageFormat::Png, "image/png"), (ImageFormat::Jpeg, "image/jpeg")] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 2)).write_to(&mut bytes, format).unwrap();
            let bytes = bytes.into_inner();
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let payload = ImagePayload::decode_inline(&format!("data:{mime_type};base64,{encoded}")).unwrap();
            assert_eq!(payload.bytes, bytes);
            assert_eq!(payload.mime_type, mime_type);
            assert!(ImagePayload::decode_inline(&format!("data:image/gif;base64,{encoded}")).is_err());
            assert!(ImagePayload::decode_inline(&format!("data:{mime_type};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes[..bytes.len() / 2]))).is_err());
        }
        for source in [
            "not-a-data-uri", "data:image/png;base64,", "data:image/png;base64,%%%",
            "data:image/png,raw", "data:text/html;base64,PHNjcmlwdD4=",
            "data:image/svg+xml;base64,PHN2Zz48c2NyaXB0Lz48L3N2Zz4=",
        ] {
            assert!(ImagePayload::decode_inline(source).is_err());
        }
    }
}