use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::assets::asset_cache::{AssetCache, ImageData};
use crate::assets::svg::rasterize_svg_to_png;
use anyhow::Result;
use selkie::{RenderConfig, parse, render_with_config};

static MERMAID_CACHE: std::sync::LazyLock<AssetCache> = std::sync::LazyLock::new(AssetCache::new);

/// Renders a Mermaid diagram source string to a PNG image.
///
/// Parses the Mermaid syntax, renders to SVG, applies theme-aware style
/// overrides, then rasterizes to PNG. Results are cached by source + max_width
/// + active theme. Returns `(png_bytes, width, height)`.
pub fn render_mermaid_to_png(source: &str, max_width: u32) -> Result<(Vec<u8>, u32, u32)> {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    max_width.hash(&mut hasher);
    crate::theme::index().hash(&mut hasher);
    let cache_key = hasher.finish();

    if let Some(ImageData::Png(data, w, h)) = MERMAID_CACHE.get(cache_key) {
        return Ok((data, w, h));
    }

    let mut render_config = RenderConfig::default();
    render_config.theme.font_family = "DejaVu Sans".to_string();
    let diagram = parse(source)?;
    let mut svg = render_with_config(&diagram, &render_config)?;

    // Text/line colors follow the active app palette so diagrams stay legible
    // on both dark and light themes.
    let (text_color, line_color) = if crate::theme::is_dark() {
        ("#A15EED", "#CCCCCC")
    } else {
        ("#5B3E90", "#586e75")
    };
    let style_override = format!(
        r#"<defs><style>
    text, tspan, .label {{ fill: {text_color} !important; }}
    .edgeLabel {{ color: {text_color} !important; }}
    line, path {{ stroke: {line_color} !important; }}
    </style></defs>"#
    );

    if let Some(svg_start) = svg.find("<svg")
        && let Some(pos) = svg[svg_start..].find('>')
    {
        svg.insert_str(svg_start + pos + 1, &style_override);
    }

    let result = rasterize_svg_to_png(&svg, max_width)?;
    MERMAID_CACHE.insert(
        cache_key,
        ImageData::Png(result.0.clone(), result.1, result.2),
    );
    Ok(result)
}
