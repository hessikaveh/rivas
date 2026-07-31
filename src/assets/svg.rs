use std::sync::OnceLock;

use anyhow::Result;
use resvg::usvg;

static SVG_OPTS: OnceLock<usvg::Options<'static>> = OnceLock::new();

fn svg_opts() -> &'static usvg::Options<'static> {
    SVG_OPTS.get_or_init(|| {
        let mut opt = usvg::Options::default();
        opt.fontdb_mut()
            .load_font_data(include_bytes!("../assets/fonts/DejaVuSans.ttf").to_vec());
        opt.font_family = "DejaVu Sans".to_string();
        opt
    })
}

/// Rasterizes an SVG string to a PNG byte buffer.
///
/// Scales the SVG to fit within `max_width` pixels (preserving aspect ratio),
/// capping at the original size. Returns `(png_bytes, width, height)`.
pub fn rasterize_svg_to_png(svg: &str, max_width: u32) -> Result<(Vec<u8>, u32, u32)> {
    let opt = svg_opts();
    let tree = usvg::Tree::from_str(svg, opt)?;
    let size = tree.size();
    let scale = (max_width as f32 / size.width()).min(1.0);
    let w = (size.width() * scale).ceil() as u32;
    let h = (size.height() * scale).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))
        .ok_or_else(|| anyhow::anyhow!("pixmap allocation failed ({}x{})", w, h))?;

    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png()?;
    Ok((png, w, h))
}
