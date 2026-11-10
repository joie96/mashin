//! Static SVG normalization on the image worker, never in a browser or WebView.
//! Resource resolvers deny file/network/data images; text, filters and active content
//! are unsupported. Only validated PNG bytes enter the shared image file/index owner.

use super::database::CacheError;
use resvg::{tiny_skia, usvg};

// XML validation precedes SVG expansion and allocation; no DTD/entity processing is allowed.
pub(super) fn normalize(bytes: &[u8]) -> Result<Vec<u8>, CacheError> {
    let invalid = || CacheError::InvalidImageObservation("invalid or unsupported static SVG");
    let source = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let document = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
        },
    )
    .map_err(|_| invalid())?;
    let root = document.root_element();
    if root.tag_name().name() != "svg"
        || root.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
    {
        return Err(invalid());
    }
    for node in document.descendants() {
        if node.is_pi() || node.ancestors().take(130).count() > 128 {
            return Err(invalid());
        }
        if !node.is_element() {
            continue;
        }
        if node.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
            || matches!(
                node.tag_name().name(),
                "script"
                    | "foreignObject"
                    | "image"
                    | "text"
                    | "textPath"
                    | "tspan"
                    | "filter"
                    | "animate"
                    | "animateMotion"
                    | "animateTransform"
                    | "set"
                    | "a"
            )
        {
            return Err(invalid());
        }
        for attribute in node.attributes() {
            if attribute.name().to_ascii_lowercase().starts_with("on")
                || attribute.name() == "base"
                || (attribute.name() == "href" && !attribute.value().starts_with('#'))
            {
                return Err(invalid());
            }
        }
    }
    let options = usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(source, &options).map_err(|_| invalid())?;
    let size = tree.size();
    if size.width() > 4096.0 || size.height() > 4096.0 {
        return Err(invalid());
    }
    let size = size.to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(invalid)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_svg_renders_real_pixels_with_transparency_and_local_references() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><defs><path id="tile" d="M0 0H2V2H0Z"/></defs><use href="#tile" fill="#ff0000"/></svg>"##;
        let png = normalize(svg).unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (4, 2));
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(3, 0).0, [0, 0, 0, 0]);
        assert_eq!(normalize(svg).unwrap(), png);
    }

    #[test]
    fn active_content_external_images_entities_and_unsupported_features_are_rejected() {
        for content in [
            "<script>alert(1)</script>",
            "<foreignObject/>",
            "<image href='https://example.invalid/image.png'/>",
            "<image href='file:///private.png'/>",
            "<image href='data:image/png;base64,aGVsbG8='/>",
            "<use href='external.svg#tile'/>",
            "<rect onclick='run()'/>",
            "<animate/>",
            "<text>unsupported font access</text>",
            "<filter/>",
        ] {
            let svg = format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'>{content}</svg>"
            );
            assert!(normalize(svg.as_bytes()).is_err(), "accepted {content}");
        }
        assert!(normalize(br#"<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///private">]><svg xmlns="http://www.w3.org/2000/svg">&x;</svg>"#).is_err());
        assert!(normalize(br#"<?xml-stylesheet href="https://example.invalid/style.css"?><svg xmlns="http://www.w3.org/2000/svg"/>"#).is_err());
        assert!(normalize(b"<svg/>").is_err());
        assert!(normalize(b"not XML").is_err());
    }

    #[test]
    fn excessive_raster_dimensions_and_xml_depth_are_rejected_before_rendering() {
        for dimension in ["0", "4097", "1000000"] {
            let svg =
                format!("<svg xmlns='http://www.w3.org/2000/svg' width='{dimension}' height='2'/>");
            assert!(normalize(svg.as_bytes()).is_err());
        }
        let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'>{}<path d='M0 0H2V2Z'/>{}</svg>", "<g>".repeat(130), "</g>".repeat(130));
        assert!(normalize(svg.as_bytes()).is_err());
    }
}
