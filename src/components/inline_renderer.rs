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

/// Folds soft-break spaces into the *trailing* text of the previous inline so
/// a source newline never becomes (or prefixes) its own layout element.
///
/// Why trailing, not leading: iocraft wraps whole flex items to the next row
/// when they don't fit, keeping any leading space intact — which showed up as
/// a visible indent before the first word of wrapped lines. A *trailing* space
/// at the end of a wrapped line is invisible, so appending backwards is always
/// safe. Soft breaks that follow non-text elements (code chips, images, math)
/// survive here and are consumed by the renderer as a small margin instead.
fn merge_softbreaks(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            Inline::SoftBreak => {
                let folded = match out.last_mut() {
                    Some(Inline::Text(t)) => {
                        if !t.ends_with(' ') {
                            t.push(' ');
                        }
                        true
                    }
                    Some(other) => append_space_to_last_text(other),
                    None => false,
                };
                if !folded {
                    out.push(Inline::SoftBreak);
                }
            }
            Inline::Bold(c) => out.push(Inline::Bold(merge_softbreaks(c))),
            Inline::Italic(c) => out.push(Inline::Italic(merge_softbreaks(c))),
            Inline::Strikethrough(c) => out.push(Inline::Strikethrough(merge_softbreaks(c))),
            Inline::Underline(c) => out.push(Inline::Underline(merge_softbreaks(c))),
            Inline::Subscript(c) => out.push(Inline::Subscript(merge_softbreaks(c))),
            Inline::Superscript(c) => out.push(Inline::Superscript(merge_softbreaks(c))),
            other => out.push(other),
        }
    }
    out
}

/// Appends a separating space to the deepest trailing text of `inline`.
/// Returns `false` when there is no text descendant to append to.
fn append_space_to_last_text(inline: &mut Inline) -> bool {
    match inline {
        Inline::Text(t) => {
            if !t.ends_with(' ') {
                t.push(' ');
            }
            true
        }
        Inline::Bold(c)
        | Inline::Italic(c)
        | Inline::Strikethrough(c)
        | Inline::Underline(c)
        | Inline::Subscript(c)
        | Inline::Superscript(c) => c.last_mut().map_or(false, append_space_to_last_text),
        Inline::Link { text, .. } => text.last_mut().map_or(false, append_space_to_last_text),
        _ => false,
    }
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
    let merged = merge_softbreaks(inlines.to_vec());
    let mut elements = Vec::new();
    let mut pending_space = false;
    render_inlines_recursive(
        &merged,
        base_color,
        TextStyle {
            bold,
            ..TextStyle::plain()
        },
        file_path,
        viewport_height,
        viewport_width,
        &mut elements,
        &mut pending_space,
    );
    elements
}

fn styled_span(color: Color, style: TextStyle, content: String) -> AnyElement<'static> {
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
    .into_any()
}

/// Whether `inline` is (or ends with) whitespace, so a following chip needs no
/// extra leading pad of its own.
fn prev_ends_with_ws(inline: &Inline) -> bool {
    match inline {
        Inline::Text(t) => t.ends_with(' '),
        Inline::SoftBreak | Inline::HardBreak => true,
        Inline::Bold(c)
        | Inline::Italic(c)
        | Inline::Strikethrough(c)
        | Inline::Underline(c)
        | Inline::Subscript(c)
        | Inline::Superscript(c) => c.last().map_or(true, prev_ends_with_ws),
        Inline::Link { text, .. } => text.last().map_or(false, prev_ends_with_ws),
        _ => false,
    }
}

/// Punctuation that should hug a preceding inline code chip (no trailing pad).
fn next_starts_with_punct(next: &[Inline]) -> bool {
    matches!(next.first(), Some(Inline::Text(t)) if t.starts_with([',', '.', ';', ':', '!', '?', ')', ']', '}']))
}

fn render_inlines_recursive(
    inlines: &[Inline],
    color: Color,
    style: TextStyle,
    file_path: Option<&PathBuf>,
    viewport_height: Option<u32>,
    viewport_width: Option<u32>,
    out: &mut Vec<AnyElement<'static>>,
    pending_space: &mut bool,
) {
    // Per-position count of leading chars already absorbed by the previous
    // inline (e.g. a footnote marker consuming the comma that follows it), so
    // punctuation never becomes its own flex item that can wrap to a line start.
    let mut consumed = vec![0usize; inlines.len()];
    for (idx, inline) in inlines.iter().enumerate() {
        match inline {
            Inline::Text(t) => {
                let t = &t[consumed[idx]..];
                // Fold a pending soft-break space into this run so wrapping
                // happens between real words, never before a leading space.
                let content = if *pending_space {
                    format!(" {t}")
                } else {
                    t.to_string()
                };
                *pending_space = false;
                out.push(styled_span(color, style, content));
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
                    pending_space,
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
                    pending_space,
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
                    pending_space,
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
                    pending_space,
                );
            }
            Inline::Subscript(ch) => {
                let content = if *pending_space {
                    format!(" {}", scripts_to_unicode(&inlines_to_text(ch), false))
                } else {
                    scripts_to_unicode(&inlines_to_text(ch), false)
                };
                *pending_space = false;
                out.push(styled_span(color, style, content));
            }
            Inline::Superscript(ch) => {
                let content = if *pending_space {
                    format!(" {}", scripts_to_unicode(&inlines_to_text(ch), true))
                } else {
                    scripts_to_unicode(&inlines_to_text(ch), true)
                };
                *pending_space = false;
                out.push(styled_span(color, style, content));
            }
            Inline::Code(c) => {
                // Context-aware padding: the chip carries its own padding
                // spaces, but drop the trailing one when punctuation follows
                // (`code`, not "code ,") and the leading one when the chip
                // opens the run or follows whitespace.
                let lead = idx > 0 && !prev_ends_with_ws(&inlines[idx - 1]) && !*pending_space;
                let trail = !next_starts_with_punct(&inlines[idx + 1..]);
                let content = format!(
                    "{}{}{}",
                    if lead { " " } else { "" },
                    c,
                    if trail { " " } else { "" }
                );
                let margin = if *pending_space { 1 } else { 0 };
                *pending_space = false;
                out.push(
                    element! {
                        View(background_color: theme::status_bg(), margin_left: margin) {
                            Text(content: content, color: theme::green())
                        }
                    }
                    .into_any(),
                );
            }
            Inline::Link { text, url } => {
                // Glow-style: link text styled distinctly (blue + underline);
                // no `(url)` suffix. Links with no visible label fall back to
                // the URL itself so `[...](target)` never disappears entirely.
                if inlines_to_text(text).trim().is_empty() {
                    let content = if style.strikethrough {
                        strike_text(url)
                    } else if *pending_space {
                        format!(" {url}")
                    } else {
                        url.clone()
                    };
                    *pending_space = false;
                    out.push(
                        element! {
                            Text(
                                content: content,
                                color: theme::blue(),
                                weight: if style.bold { Weight::Bold } else { Weight::Normal },
                                italic: style.italic,
                                decoration: TextDecoration::Underline,
                            )
                        }
                        .into_any(),
                    );
                } else {
                    render_inlines_recursive(
                        text,
                        theme::blue(),
                        TextStyle {
                            underline: true,
                            ..style
                        },
                        file_path,
                        viewport_height,
                        viewport_width,
                        out,
                        pending_space,
                    );
                }
            }
            Inline::SoftBreak => {
                // Defer the space to the next inline so it never becomes a
                // standalone flex item that can wrap to a line start.
                *pending_space = true;
            }
            Inline::HardBreak => {
                // A zero-width `Text("\n")` never forces a flex wrap (it measures
                // width 0), so hard breaks collapse onto the current line. Span
                // the full container width instead so the next inline wraps.
                *pending_space = false;
                out.push(element! { View(width: 100pct) {} }.into_any());
            }
            Inline::FootnoteRef(label) => {
                // Numeric labels render as Unicode superscripts (e.g. `text¹`);
                // anything else keeps the readable `[^label]` form in dim color.
                let content = if !label.is_empty() && label.chars().all(|c| c.is_ascii_digit()) {
                    scripts_to_unicode(label, true)
                } else {
                    format!("[^{}]", label)
                };
                // Absorb any punctuation that immediately follows so it stays
                // attached to the marker instead of wrapping to a line start
                // (e.g. "one[^note]," — the comma must never sit alone).
                let mut extra = String::new();
                if let Some(Inline::Text(t)) = inlines.get(idx + 1) {
                    for ch in t[consumed[idx + 1]..].chars() {
                        if matches!(ch, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
                            extra.push(ch);
                            consumed[idx + 1] += ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                }
                let margin = if *pending_space { 1 } else { 0 };
                *pending_space = false;
                out.push(
                    element! { Text(content: format!("{}{}{}", " ".repeat(margin as usize), content, extra), color: theme::comment()) }
                        .into_any(),
                );
            }
            Inline::Math(m) => {
                let margin = if *pending_space { 1 } else { 0 };
                *pending_space = false;
                if math_mode() == MathMode::Image {
                    let vp = viewport_height.map(|h| Viewport {
                        height: Some(h),
                        width: viewport_width,
                        scroll_offset: None,
                    });
                    out.push(
                        element! {
                            View(margin_left: margin) {
                                KittyMath(content: m.clone(), display: false, viewport: vp)
                            }
                        }
                        .into_any(),
                    );
                } else {
                    out.push(
                        element! {
                            View(margin_left: margin) {
                                UnicodeMath(content: m.clone(), display: false)
                            }
                        }
                        .into_any(),
                    );
                }
            }
            Inline::Image { alt: _, url } => {
                // For inline images, we use KittyImage directly without block margins
                let margin = *pending_space;
                *pending_space = false;
                let vp = viewport_height.map(|h| Viewport {
                    height: Some(h),
                    width: viewport_width,
                    scroll_offset: None,
                });
                out.push(element! {
                    View(margin_left: if margin { 1 } else { 0 }) {
                        KittyImage(url: url.clone(), file_path: file_path.cloned().unwrap_or_default(), viewport: vp)
                    }
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
