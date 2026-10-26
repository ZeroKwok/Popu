//! Self-contained visual content. No files or network resources are resolved.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;

use crate::protocol::{ImageSource, ProtocolError};

fn invalid(message: &str) -> ProtocolError { ProtocolError::Invalid(message.into()) }

pub fn validate_height(height: Option<u32>) -> Result<(), ProtocolError> {
    if height.is_some_and(|height| !(64..=1200).contains(&height)) {
        return Err(invalid("visual height must be within 64-1200"));
    }
    Ok(())
}

pub fn validate_image(source: &ImageSource) -> Result<(), ProtocolError> {
    if source.alt.trim().is_empty() || source.alt.chars().count() > 1000 {
        return Err(invalid("image alt must contain 1-1000 characters"));
    }
    if source.src.len() > 256 * 1024 { return Err(invalid("image source exceeds 256 KiB")); }
    let (header, encoded) = source.src.split_once(',').ok_or_else(|| invalid("image must be a base64 data URI"))?;
    let mime = header.strip_prefix("data:").and_then(|value| value.strip_suffix(";base64"))
        .ok_or_else(|| invalid("image must be a base64 data URI"))?;
    let bytes = STANDARD.decode(encoded).map_err(|_| invalid("invalid image base64"))?;
    if bytes.is_empty() { return Err(invalid("image must not be empty")); }
    if mime == "image/svg+xml" { return validate_svg(&bytes); }
    let expected = match mime {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/webp" => image::ImageFormat::WebP,
        _ => return Err(invalid("supported image types are SVG, PNG, JPEG and WebP")),
    };
    if image::guess_format(&bytes).ok() != Some(expected) { return Err(invalid("image data does not match its media type")); }
    let reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), expected);
    let (width, height) = reader.into_dimensions().map_err(|_| invalid("invalid image dimensions"))?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(invalid("image dimensions must be within 1-4096 pixels"));
    }
    Ok(())
}

fn validate_svg(bytes: &[u8]) -> Result<(), ProtocolError> {
    let xml = std::str::from_utf8(bytes).map_err(|_| invalid("SVG must be UTF-8"))?;
    // Common SVG exporters include a DOCTYPE. Parse it without resolving any
    // external document, but reject custom entities rather than expanding them.
    if xml.contains("<!ENTITY") { return Err(invalid("SVG entity declarations are not supported")); }
    let options = roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: 10_000, ..Default::default() };
    let document = roxmltree::Document::parse_with_options(xml, options).map_err(|_| invalid("invalid SVG document"))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some("http://www.w3.org/2000/svg") {
        return Err(invalid("image must have an SVG root and namespace"));
    }
    for node in document.descendants().filter(|node| node.is_element()) {
        let name = node.tag_name().name().to_ascii_lowercase();
        if matches!(name.as_str(), "script" | "foreignobject" | "iframe" | "object" | "embed" | "animate" | "animatetransform" | "set") {
            return Err(invalid("SVG must contain only static, self-contained graphics"));
        }
        if name == "style" { validate_svg_css(node.text().unwrap_or_default())?; }
        for attribute in node.attributes() {
            let name = attribute.name().to_ascii_lowercase();
            let value = attribute.value().trim().to_ascii_lowercase();
            if name.starts_with("on") || (name == "href" && !value.starts_with('#'))
                || value.contains("javascript:") {
                return Err(invalid("SVG scripts and external resources are not supported"));
            }
            validate_svg_css(&value)?;
        }
    }
    Ok(())
}

fn validate_svg_css(value: &str) -> Result<(), ProtocolError> {
    if value.to_ascii_lowercase().contains("@import") { return Err(invalid("SVG styles must be self-contained")); }
    static URLS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r#"(?i)url\s*\(\s*['\"]?([^'\")\s]+)"#).unwrap());
    if URLS.captures_iter(value).any(|capture| !capture[1].starts_with('#')) {
        return Err(invalid("SVG styles may reference only local fragments"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Request;
    use serde_json::json;

    fn svg_source(body: &str) -> ImageSource {
        ImageSource { src: format!("data:image/svg+xml;base64,{}", STANDARD.encode(format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">{body}</svg>"##))), alt: "Example".into() }
    }

    #[test]
    fn svg_supports_static_styles_and_local_references_but_not_active_or_external_content() {
        validate_image(&svg_source(r##"<style>rect { fill: #ddd; }</style><defs><linearGradient id="g"/></defs><rect width="20" height="20" fill="url(#g)"/>"##)).unwrap();
        for body in [r#"<script>alert(1)</script>"#, r#"<rect onclick="alert(1)"/>"#,
            r#"<image href="https://example.com/image.png"/>"#, r#"<foreignObject/>"#,
            r#"<style>@import 'https://example.com/style.css';</style>"#,
            r#"<rect style="fill:url('https://example.com/image.svg')"/>"#] {
            assert!(validate_image(&svg_source(body)).is_err(), "{body}");
        }
        validate_svg(br#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"><svg xmlns="http://www.w3.org/2000/svg"/>"#).unwrap();
        assert!(validate_svg(br#"<!DOCTYPE svg [<!ENTITY text "expanded">]><svg xmlns="http://www.w3.org/2000/svg">&text;</svg>"#).is_err());
    }

    #[test]
    fn image_sources_require_valid_supported_data_and_bounded_dimensions() {
        for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg, image::ImageFormat::WebP] {
            let mut buffer = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 2)).write_to(&mut buffer, format).unwrap();
            let mime = match format { image::ImageFormat::Png => "png", image::ImageFormat::Jpeg => "jpeg", _ => "webp" };
            let source = ImageSource { src: format!("data:image/{mime};base64,{}", STANDARD.encode(buffer.into_inner())), alt: "Image".into() };
            validate_image(&source).unwrap();
        }
        let mut oversized = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(4097, 1)).write_to(&mut oversized, image::ImageFormat::Png).unwrap();
        assert!(validate_image(&ImageSource { src: format!("data:image/png;base64,{}", STANDARD.encode(oversized.into_inner())), alt: "Too wide".into() }).is_err());
        for src in ["file:///tmp/image.png", "https://example.com/image.png", "data:image/png;base64,invalid!", "data:image/png;base64,YWJj"] {
            assert!(validate_image(&ImageSource { src: src.into(), alt: "Image".into() }).is_err());
        }
        let mut source = svg_source("<rect/>");
        source.alt = " ".into();
        assert!(validate_image(&source).is_err());
        source.alt = "Image".into(); source.src = "a".repeat(256 * 1024 + 1);
        assert!(validate_image(&source).is_err());
    }

    #[test]
    fn chart_specs_are_generic_inline_and_bounded() {
        for mark in ["line", "bar", "rect", "point", "area"] {
            validate_chart(&json!({"mark":mark,"data":{"values":[{"x":1,"y":2}]}})).unwrap();
        }
        validate_chart(&json!({"layer":[{"mark":"rect"},{"mark":"text"}]})).unwrap();
        for spec in [json!({}), json!("bar"), json!({"mark":"line","data":{"url":"https://example.com"}}),
            json!({"layer":[{"mark":"point","encoding":{"href":{"value":"https://example.com"}}}]}),
            json!({"mark":"text","description":"a".repeat(128*1024)})] {
            assert!(validate_chart(&spec).is_err());
        }
        let mut nested = json!({"mark":"line"});
        for _ in 0..35 { nested = json!({"layer":[nested]}); }
        assert!(validate_chart(&nested).is_err());
    }

    #[test]
    fn visual_validation_is_enforced_through_the_request_protocol() {
        let image = svg_source("<rect/>");
        let mut value = json!({"version":"1.0","mode":"notify","content":{"title":"Visual","icon":image,"blocks":[{"type":"chart","engine":"vega_lite","height":220,"spec":{"mark":"bar","data":{"values":[{"x":1}]}}}]}});
        Request::parse(&value.to_string()).unwrap();
        value["content"]["blocks"][0]["height"] = json!(0);
        assert!(Request::parse(&value.to_string()).is_err());
        value["content"]["blocks"][0]["height"] = json!(220);
        value["content"]["blocks"][0]["engine"] = json!("unknown");
        assert!(Request::parse(&value.to_string()).is_err());
    }

    #[test]
    fn visual_examples_parse_and_survive_content_updates() {
        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../protocol/examples");
        let mut count = 0;
        for entry in std::fs::read_dir(examples).unwrap() {
            let path = entry.unwrap().path();
            if !path.file_name().unwrap().to_string_lossy().contains("-visual-") { continue; }
            let request = Request::parse(&std::fs::read_to_string(&path).unwrap()).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let content = request.content.clone();
            let mut registry = crate::core::RequestRegistry::default();
            let (created, _) = registry.create(request, None);
            registry.update(created.request_id, 1, content.clone()).unwrap();
            let snapshot = registry.snapshot(created.request_id).unwrap();
            assert_eq!(snapshot.revision, 2);
            assert_eq!(serde_json::to_value(snapshot.content).unwrap(), serde_json::to_value(content).unwrap());
            count += 1;
        }
        assert_eq!(count, 7);
    }
}

pub fn validate_chart(spec: &Value) -> Result<(), ProtocolError> {
    let object = spec.as_object().ok_or_else(|| invalid("chart spec must be an object"))?;
    if !["mark", "layer", "hconcat", "vconcat", "concat", "facet", "repeat"].iter().any(|key| object.contains_key(*key)) {
        return Err(invalid("chart spec must define a mark or composition"));
    }
    if serde_json::to_vec(spec)?.len() > 128 * 1024 { return Err(invalid("chart spec exceeds 128 KiB")); }
    let mut budget = 20_000;
    validate_chart_node(spec, 0, &mut budget)
}

fn validate_chart_node(value: &Value, depth: usize, budget: &mut usize) -> Result<(), ProtocolError> {
    if depth > 32 || *budget == 0 { return Err(invalid("chart spec exceeds nesting or node limits")); }
    *budget -= 1;
    match value {
        Value::Object(object) => for (key, value) in object {
            if matches!(key.as_str(), "url" | "href") { return Err(invalid("charts must use inline data without URLs or links")); }
            validate_chart_node(value, depth + 1, budget)?;
        },
        Value::Array(values) => for value in values { validate_chart_node(value, depth + 1, budget)?; },
        _ => {}
    }
    Ok(())
}
