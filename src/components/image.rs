use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::kitty_graphic::{KittyGraphic, UseKittyGraphic};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::debug;
use crate::output::capabilities;
use crate::output::graphics_manager::GfxSource;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Properties for the [`Image`] component.
#[derive(Default, Props)]
pub struct ImageProps {
    /// URL or file path of the image.
    pub url: String,
    /// Base directory for resolving relative image paths.
    pub file_path: PathBuf,
    /// Optional title displayed above the image.
    pub title: Option<String>,
    /// Alt text used as fallback label when the image cannot be displayed.
    pub alt: Option<String>,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders an inline image, using Kitty graphics protocol if available,
/// or a text fallback (`[Image: alt]`) otherwise.
#[component]
pub fn Image(props: &ImageProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let label = props.alt.as_deref().unwrap_or(&props.url);
    let raw = props.raw.clone();
    // Raw source lines get markdown highlighting like every other block.
    let raw_source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let raw_highlighted = hooks.use_cached_highlight(raw_source, MARKDOWN, theme::fg());
    let with_highlight = |mut r: RawState| {
        r.highlight = Some(raw_highlighted.clone());
        r
    };

    // With the cursor on the block in Normal mode we keep the rendered graphic
    // visible and show the raw source line underneath. Without kitty graphics
    // there is no picture to preserve (only the `[Image: …]` label), so fall
    // back to the source editor alone, matching the other text blocks.
    if raw.is_some() && !capabilities::has_kitty() {
        return element! {
            View(margin_bottom: 1, background_color: theme::dark_bg()) {
                RawBuffer(raw: raw.map(with_highlight), color: theme::fg())
            }
        }
        .into_any();
    }

    element! {
        View(flex_direction: FlexDirection::Column, margin_bottom: 1) {
            #(props.title.clone().map(|title| element! {
                View() {
                    Text(content: title, color: theme::comment())
                }
            }))
            #(if capabilities::has_kitty() {
                Some(element! {
                    KittyImage(url: props.url.clone(), file_path: props.file_path.clone(), viewport: props.viewport.clone())
                }.into_any())
            } else {
                Some(element! {
                    Text(content: format!("[Image: {}]", label), color: theme::comment())
                }.into_any())
            })
            #(raw.map(with_highlight).map(|r| element! {
                View(margin_bottom: 1, background_color: theme::dark_bg()) {
                    RawBuffer(raw: r, color: theme::fg())
                }
            }))
        }
    }
    .into_any()
}

/// Properties for the [`KittyImage`] component.
#[derive(Default, Props)]
pub struct KittyImageProps {
    /// URL or file path of the image to display via Kitty graphics protocol.
    pub url: String,
    /// Base directory for resolving relative image paths.
    pub file_path: PathBuf,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
}

/// Displays an image using the Kitty terminal graphics protocol.
///
/// Loads the image, calculates display dimensions, and places it in the terminal.
/// Handles scroll-into-view behavior and dynamic height estimation for layout.
#[component]
pub fn KittyImage(props: &KittyImageProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let vw = props.viewport.as_ref().and_then(|v| v.width).unwrap_or(100);
    let vh = props
        .viewport
        .as_ref()
        .and_then(|v| v.height)
        .unwrap_or(100);
    // Unique per-occurrence key so identical images don't share a terminal
    // graphic id (which would let one occurrence's detach/place clobber others).
    let cache_key = format!("{}:{}", vw, props.url);
    let url = props.url.clone();
    let base_dir = props.file_path.parent().map(|p| p.to_path_buf());
    let gfx: KittyGraphic = hooks.use_kitty_graphic(
        cache_key,
        vw,
        vh,
        props.viewport.as_ref().and_then(|v| v.scroll_offset),
        5,
        move |max_w, mc, mr| GfxSource::Image {
            url,
            base_dir: base_dir.clone(),
            max_w,
            max_cols: mc,
            max_rows: mr,
        },
    );
    let cols = gfx.cols;
    let rows = gfx.rows;

    if let Some(err) = gfx.error {
        return element! {
            View() {
                Text(content: err, color: theme::red())
            }
        }
        .into_any();
    }

    if debug::are_annotations_enabled() {
        let img_cols = cols.max(1);
        let img_rows = rows.max(1);
        let url_display: String = props.url.chars().take(24).collect();
        element! {
            View(
                width: img_cols,
                height: img_rows,
                border_style: BorderStyle::Single,
                border_color: theme::dbg_image(),
                background_color: theme::dbg_bg(),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            ) {
                Text(content: format!("IMG {}x{}", img_cols, img_rows), color: theme::dbg_image(), weight: Weight::Bold)
                Text(content: url_display, color: theme::comment())
            }
        }
        .into_any()
    } else {
        element! {View(width: cols.max(1), height: gfx.declared_rows.max(1))}.into_any()
    }
}
