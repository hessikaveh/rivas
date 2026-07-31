use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, ImageDecoder, ImageEncoder};

const MAX_GIF_FRAMES: usize = 60;

use crate::assets::asset_cache::AssetCache;
pub use crate::assets::asset_cache::ImageData;

static IMAGE_CACHE: std::sync::LazyLock<AssetCache> = std::sync::LazyLock::new(AssetCache::new);

/// Resolves an image URL to an absolute file path.
///
/// Searches in order: the URL itself (if absolute), `base_dir`, then the current
/// working directory. Returns an error if the file is not found.
pub fn resolve_path(url: &str, base_dir: Option<&Path>) -> Result<PathBuf> {
    let path = PathBuf::from(url);

    if path.is_absolute() {
        if path.exists() {
            return Ok(path);
        }

        anyhow::bail!("Image not found: {}", path.display());
    }

    if let Some(base) = base_dir {
        let resolved = base.join(&path);
        if resolved.exists() {
            return Ok(resolved);
        }
    }

    let cwd = std::env::current_dir()?;
    let resolved = cwd.join(&path);
    if resolved.exists() {
        return Ok(resolved);
    }

    anyhow::bail!("Image not found: {url}")
}

fn is_svg_ext(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".svg") || lower.ends_with(".svgz")
}

/// Loads an image from a URL (local path or HTTP), returning cached or freshly-decoded data.
///
/// For local files, the cache key includes the file modification time for invalidation.
/// Supports PNG, JPEG, GIF (animated, up to 60 frames), and SVG (rasterized via `resvg`).
/// `max_width` scales the image to fit within the given pixel width.
pub fn load_image(url: &str, base_dir: Option<&Path>, max_width: u32) -> Result<ImageData> {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    if let Some(base) = base_dir {
        base.hash(&mut hasher);
    }
    max_width.hash(&mut hasher);

    if !url.starts_with("http://") && !url.starts_with("https://") {
        if let Ok(path) = resolve_path(url, base_dir) {
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(mtime) = meta.modified() {
                    mtime.hash(&mut hasher);
                }
            }
        }
    }
    let cache_key = hasher.finish();

    if let Some(cached) = IMAGE_CACHE.get(cache_key) {
        return Ok(cached);
    }

    let image_data = if url.starts_with("http://") || url.starts_with("https://") {
        load_remote_image(url, max_width)?
    } else {
        let path = resolve_path(url, base_dir)?;
        load_local_image(&path, max_width)?
    };

    IMAGE_CACHE.insert(cache_key, image_data.clone());
    Ok(image_data)
}

fn load_local_image(path: &Path, max_width: u32) -> Result<ImageData> {
    let path_str = path.to_string_lossy();

    if is_svg_ext(&path_str) {
        let svg = std::fs::read_to_string(path)
            .context(format!("Failed to read SVG: {}", path.display()))?;
        return svg_to_png(&svg, max_width);
    }

    if path_str.ends_with(".gif") {
        return load_gif_frames(path, max_width);
    }

    load_static_png(path, max_width)
}

fn load_remote_image(url: &str, max_width: u32) -> Result<ImageData> {
    let response = ureq::get(url)
        .call()
        .context(format!("Failed to download {url}"))?;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let bytes = response
        .into_body()
        .read_to_vec()
        .context(format!("Failed to read response from {url}"))?;

    if content_type.contains("image/svg+xml") || is_svg_ext(url) {
        let svg = String::from_utf8(bytes).context("SVG from remote is not valid UTF-8")?;
        return svg_to_png(&svg, max_width);
    }

    if content_type.contains("image/gif") || url.to_lowercase().ends_with(".gif") {
        return load_gif_frames_from_bytes(&bytes, max_width);
    }

    load_static_from_bytes(&bytes, max_width)
}

fn svg_to_png(svg: &str, max_width: u32) -> Result<ImageData> {
    let (png_data, w, h) = crate::assets::svg::rasterize_svg_to_png(svg, max_width)?;
    Ok(ImageData::Png(png_data, w, h))
}

fn encode_static(img: image::RgbaImage, max_width: u32) -> Result<ImageData> {
    let img = if img.width() > max_width {
        let ratio = max_width as f32 / img.width() as f32;
        let new_h = (img.height() as f32 * ratio) as u32;
        image::imageops::resize(
            &img,
            max_width,
            new_h,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };
    let (w, h) = (img.width(), img.height());

    let mut png_buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_buf);
    encoder.write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)?;

    Ok(ImageData::Png(png_buf, w, h))
}

fn load_static_png(path: &Path, max_width: u32) -> Result<ImageData> {
    let img = image::open(path)?.to_rgba8();
    encode_static(img, max_width)
}

fn load_static_from_bytes(bytes: &[u8], max_width: u32) -> Result<ImageData> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    encode_static(img, max_width)
}

fn load_gif_frames(path: &Path, max_width: u32) -> Result<ImageData> {
    let file = std::fs::File::open(path)?;
    let decoder = GifDecoder::new(BufReader::new(file))?;
    process_gif_frames(decoder, max_width)
}

fn load_gif_frames_from_bytes(bytes: &[u8], max_width: u32) -> Result<ImageData> {
    let decoder = GifDecoder::new(BufReader::new(Cursor::new(bytes)))?;
    process_gif_frames(decoder, max_width)
}

fn process_gif_frames<R: BufRead + Seek>(
    decoder: GifDecoder<R>,
    max_width: u32,
) -> Result<ImageData> {
    let (orig_w, orig_h) = decoder.dimensions();
    let raw_frames = decoder.into_frames().collect_frames()?;

    let total = raw_frames.len();

    let selected: Vec<usize> = if total > MAX_GIF_FRAMES {
        (0..MAX_GIF_FRAMES)
            .map(|i| (i * total) / MAX_GIF_FRAMES)
            .collect()
    } else {
        (0..total).collect()
    };

    if selected.is_empty() {
        bail!("No frames in GIF");
    }

    let ((out_w, out_h), scale) = if orig_w > max_width {
        let s = max_width as f32 / orig_w as f32;
        ((max_width, (orig_h as f32 * s) as u32), s)
    } else {
        ((orig_w, orig_h), 1.0)
    };

    let mut frames = Vec::with_capacity(selected.len());
    for idx in &selected {
        let frame = &raw_frames[*idx];
        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay_ms = if denom > 0 { numer / denom } else { 0 };
        let mut rgba = frame.clone().into_buffer();

        if scale != 1.0 {
            rgba =
                image::imageops::resize(&rgba, out_w, out_h, image::imageops::FilterType::Lanczos3);
        }

        let mut png_buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_buf);
        encoder.write_image(rgba.as_raw(), out_w, out_h, image::ExtendedColorType::Rgba8)?;

        frames.push((png_buf, delay_ms));
    }

    if frames.len() <= 1 {
        let (data, _) = frames.into_iter().next().unwrap_or_default();
        return Ok(ImageData::Png(data, out_w, out_h));
    }

    Ok(ImageData::Gif {
        frames,
        width: out_w,
        height: out_h,
    })
}
