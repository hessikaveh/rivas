use crate::assets::math::{MathMode, math_mode, scripts_to_unicode};
use crate::components::image::KittyImage;
use crate::components::math_block::{KittyMath, UnicodeMath};
use crate::components::scroll::Viewport;
use crate::document::model::{Inline, inlines_to_text};
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Text styling inherited while rendering inline content.
#[derive(Clone, Copy)]
struct TextStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
}

impl TextStyle {
    fn plain() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

/// Combines each non-whitespace character with U+0336 (COMBINING LONG STROKE
/// OVERLAY) so the text renders struck-through in the terminal. Terminals
/// support strike-through only rarely, so like sub/superscripts we fall back
/// to a Unicode glyph overlay instead of an ANSI attribute.
fn strike_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        out.push(c);
        if !c.is_whitespace() {
            out.push('\u{0336}');
        }
    }
    out
}

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
        TextStyle {
            bold,
            ..TextStyle::plain()
        },
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
    style: TextStyle,
    file_path: Option<&PathBuf>,
    viewport_height: Option<u32>,
    viewport_width: Option<u32>,
    out: &mut Vec<AnyElement<'static>>,
) {
    for inline in inlines {
        let styled_text = |content: String| {
            let content = if style.strikethrough {
                strike_text(&content)
            } else {
                content
            };
            element! {
                Text(
                    content: content,
                    color: color,
                    weight: if style.bold { Weight::Bold } else { Weight::Normal },
                    italic: style.italic,
                    decoration: if style.underline { TextDecoration::Underline } else { TextDecoration::None }
                )
            }
        };
        match inline {
            Inline::Text(t) => {
                out.push(styled_text(t.clone()).into_any());
            }
            Inline::Bold(ch) => {
                render_inlines_recursive(
                    ch,
                    color,
                    TextStyle {
                        bold: true,
                        ..style
                    },
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
                    TextStyle {
                        italic: true,
                        ..style
                    },
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
                    TextStyle {
                        strikethrough: true,
                        ..style
                    },
                    file_path,
                    viewport_height,
                    viewport_width,
                    out,
                );
            }
            Inline::Underline(ch) => {
                render_inlines_recursive(
                    ch,
                    color,
                    TextStyle {
                        underline: true,
                        ..style
                    },
                    file_path,
                    viewport_height,
                    viewport_width,
                    out,
                );
            }
            Inline::Subscript(ch) => {
                out.push(styled_text(scripts_to_unicode(&inlines_to_text(ch), false)).into_any());
            }
            Inline::Superscript(ch) => {
                out.push(styled_text(scripts_to_unicode(&inlines_to_text(ch), true)).into_any());
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
                    style,
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
                // A zero-width `Text("\n")` never forces a flex wrap (it measures
                // width 0), so hard breaks collapse onto the current line. Span
                // the full container width instead so the next inline wraps.
                out.push(element! { View(width: 100pct) {} }.into_any());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strike_text_overlays_every_non_whitespace_char() {
        assert_eq!(strike_text("abc"), "a\u{0336}b\u{0336}c\u{0336}");
        assert_eq!(strike_text("a b"), "a\u{0336} b\u{0336}");
        assert_eq!(strike_text(" "), " ");
    }

    #[test]
    fn strike_text_preserves_display_width() {
        let plain = "% a b c";
        let struck = strike_text(plain);
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(struck.as_str()),
            unicode_width::UnicodeWidthStr::width(plain),
            "combining overlay must stay width zero so layout is unaffected"
        );
    }
}
