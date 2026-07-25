use crate::assets::math::{MathMode, math_mode};
use crate::components::image::KittyImage;
use crate::components::math_block::{KittyMath, UnicodeMath};
use crate::components::scroll::Viewport;
use crate::document::model::Inline;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Renders a list of inlines into a Vec of AnyElement for display
pub fn render_inlines(
    inlines: &[Inline],
    base_color: Color,
    bold: bool,
    file_path: Option<&PathBuf>,
    viewport_height: Option<u32>,
    viewport_width: Option<u32>,
) -> Vec<AnyElement<'static>> {
    let mut elements = Vec::new();
    render_inlines_recursive(
        inlines,
        base_color,
        bold,
        false,
        file_path,
        viewport_height,
        viewport_width,
        &mut elements,
    );
    elements
}

fn render_inlines_recursive(
    inlines: &[Inline],
    color: Color,
    bold: bool,
    italic: bool,
    file_path: Option<&PathBuf>,
    viewport_height: Option<u32>,
    viewport_width: Option<u32>,
    out: &mut Vec<AnyElement<'static>>,
) {
    for inline in inlines {
        match inline {
            Inline::Text(t) => {
                out.push(
                    element! {
                        Text(
                            content: t.clone(),
                            color: color,
                            weight: if bold { Weight::Bold } else { Weight::Normal }
                        )
                    }
                    .into_any(),
                );
            }
            Inline::Bold(ch) => {
                render_inlines_recursive(
                    ch,
                    color,
                    true,
                    italic,
                    file_path,
                    viewport_height,
                    viewport_width,
                    out,
                );
            }
            Inline::Italic(ch) => {
                render_inlines_recursive(
                    ch,
                    color,
                    bold,
                    true,
                    file_path,
                    viewport_height,
                    viewport_width,
                    out,
                );
            }
            Inline::Strikethrough(ch) => {
                render_inlines_recursive(
                    ch,
                    color,
                    bold,
                    italic,
                    file_path,
                    viewport_height,
                    viewport_width,
                    out,
                );
            }
            Inline::Code(c) => {
                out.push(
                    element! { Text(content: format!(" {} ", c), color: theme::GREEN) }.into_any(),
                );
            }
            Inline::Link { text, url, .. } => {
                render_inlines_recursive(
                    text,
                    theme::BLUE,
                    bold,
                    italic,
                    file_path,
                    viewport_height,
                    viewport_width,
                    out,
                );
                out.push(
                    element! { Text(content: format!(" ({})", url), color: theme::COMMENT) }
                        .into_any(),
                );
            }
            Inline::SoftBreak => {
                out.push(element! { Text(content: " ".to_string(), color: color) }.into_any());
            }
            Inline::HardBreak => {
                out.push(element! { Text(content: "\n".to_string(), color: color) }.into_any());
            }
            Inline::Math(m) => {
                if math_mode() == MathMode::Image {
                    let vp = viewport_height.map(|h| Viewport {
                        height: Some(h),
                        width: viewport_width,
                        scroll_offset: None,
                    });
                    out.push(
                        element! {
                            KittyMath(content: m.clone(), display: false, viewport: vp)
                        }
                        .into_any(),
                    );
                } else {
                    out.push(
                        element! {
                            UnicodeMath(content: m.clone(), display: false)
                        }
                        .into_any(),
                    );
                }
            }
            Inline::Image { alt: _, url } => {
                // For inline images, we use KittyImage directly without block margins
                let vp = viewport_height.map(|h| Viewport {
                    height: Some(h),
                    width: viewport_width,
                    scroll_offset: None,
                });
                out.push(element! {
                    KittyImage(url: url.clone(), file_path: file_path.cloned().unwrap_or_default(), viewport: vp)
                }.into_any());
            }
        }
    }
}
