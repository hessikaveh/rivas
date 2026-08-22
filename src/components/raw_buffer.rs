use iocraft::prelude::*;

use crate::theme;

/// Raw buffer + cursor info used to render a block's source in Normal mode.
#[derive(Clone, Default)]
pub struct RawState {
    /// The raw source text of the block (`span.0..span.1`).
    pub text: String,
    /// Zero-indexed line within the block the cursor is on.
    pub cursor_line: Option<usize>,
    /// Byte offset of the cursor within that line.
    pub cursor_col: usize,
    /// Whether a pending operator should show the `_` cursor style.
    pub operator: bool,
    /// Optional per-line syntax-highlighted spans (byte-aligned with `text`).
    pub highlight: Option<Vec<Vec<(String, Color)>>>,
}

/// Props for the [`RawBuffer`] component.
#[derive(Default, Props)]
pub struct RawBufferProps {
    /// Raw buffer + cursor state. When `None`, renders nothing.
    pub raw: Option<RawState>,
    /// Foreground color for the raw text (used when highlighting is absent).
    pub color: Option<Color>,
}

/// Builds the [`MixedTextContent`] segments for the cursor line, inserting the
/// Normal-mode cursor as an inverted cell at `cursor_col`.
///
/// Splitting the highlighted spans around the cursor keeps the token coloring
/// on both sides; the cell is a style-only change, so the line's wrapped width
/// is unaffected by where the cursor sits.
fn build_line_contents(
    spans: &[(String, Color)],
    cursor_col: usize,
    operator: bool,
    fg: Color,
) -> Vec<MixedTextContent> {
    let mut parts: Vec<MixedTextContent> = Vec::new();

    let total: usize = spans.iter().map(|(t, _)| t.len()).sum();
    let mut acc = 0usize;
    let mut inserted = false;

    for (text, color) in spans {
        if !inserted && acc + text.len() <= cursor_col {
            // Entirely before the cursor.
            parts.push(MixedTextContent::new(text.clone()).color(*color));
        } else if !inserted {
            // The cursor falls inside this span.
            let off = cursor_col.saturating_sub(acc).min(text.len());
            let before = &text[..off];
            let rest = &text[off..];
            if !before.is_empty() {
                parts.push(MixedTextContent::new(before.to_string()).color(*color));
            }
            let cell_char = if operator {
                "_".to_string()
            } else if let Some(c) = rest.chars().next() {
                c.to_string()
            } else {
                " ".to_string()
            };
            parts.push(MixedTextContent::new(cell_char).color(*color).invert());
            if let Some(c) = rest.chars().next() {
                let after = &rest[c.len_utf8()..];
                if !after.is_empty() {
                    parts.push(MixedTextContent::new(after.to_string()).color(*color));
                }
            }
            inserted = true;
        } else {
            // Entirely after the cursor.
            parts.push(MixedTextContent::new(text.clone()).color(*color));
        }
        acc += text.len();
    }

    // Cursor sits past the end of the line (or the line is empty): append the
    // trailing block cursor cell so the position is always visible.
    if !inserted && cursor_col >= total {
        parts.push(MixedTextContent::new(" ".to_string()).color(fg).invert());
    }

    parts
}

/// Renders raw buffer lines with the Normal-mode cursor as an inverted cell.
///
/// Each logical line is a single [`MixedText`], so wrapping is measured over the
/// full line and the cursor (a style-only change) never disturbs the wrap. Only
/// the line containing the cursor is split; syntect-highlighted spans are used
/// verbatim when the raw state carries them.
#[component]
pub fn RawBuffer(props: &RawBufferProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let fg = props.color.unwrap_or(theme::fg());

    let lines = props.raw.clone().map(|raw| {
        let highlight = raw.highlight.clone();
        raw.text
            .split('\n')
            .enumerate()
            .map(|(idx, line)| {
                let is_cursor_line = Some(idx) == raw.cursor_line;
                let spans: Vec<(String, Color)> = highlight
                    .as_ref()
                    .and_then(|h| h.get(idx).cloned())
                    .unwrap_or_else(|| vec![(line.to_string(), fg)]);

                let contents = if is_cursor_line {
                    build_line_contents(&spans, raw.cursor_col, raw.operator, fg)
                } else {
                    spans
                        .into_iter()
                        .map(|(text, color)| MixedTextContent::new(text).color(color))
                        .collect()
                };

                element! {
                    MixedText(contents: contents, wrap: TextWrap::Wrap)
                }
                .into_any()
            })
            .collect::<Vec<_>>()
    });

    element! {
        View(flex_direction: FlexDirection::Column) {
            #(lines.into_iter().flatten())
        }
    }
}
