use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::theme;

fn tail_to_width(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in s.chars().rev() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > max {
            out.insert(0, '…');
            break;
        }
        out.insert(0, ch);
        width += w;
    }
    out
}

fn head_to_width(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > max {
            out.push('…');
            break;
        }
        out.push(ch);
        width += w;
    }
    out
}

/// Properties for the [`CursorInfo`] component.
#[derive(Default, Props)]
pub struct CursorInfoProps {
    /// Cursor row (0-indexed line number).
    pub row: usize,
    /// Cursor column (0-indexed character index).
    pub col: usize,
    /// Text before the cursor character (for the status bar preview).
    pub before: String,
    /// The character under the cursor (displayed with highlight).
    pub cursor_char: String,
    /// Text after the cursor character (for the status bar preview).
    pub after: String,
    /// Show the "↳" arrow prefix (used in the under-block indicator).
    pub show_arrow: Option<bool>,
    /// Background color for the cursor character highlight.
    /// When set, the cursor char is rendered inside a View with this background.
    /// When None, the cursor char is rendered as plain text.
    pub cursor_bg: Option<Color>,
    /// Width budget for the text preview (before + after). If not set, uses
    /// the full available width minus the prefix.
    pub budget: Option<usize>,
    /// Text color for the "Ln X, Col Y:" prefix.
    pub prefix_color: Option<Color>,
}

/// Renders the status bar cursor position indicator with a text preview.
///
/// Displays `"Ln X, Col Y: "` followed by a truncated preview of the current
/// line with the cursor character highlighted. Truncation respects Unicode
/// display width to avoid overflow.
#[component]
pub fn CursorInfo(props: &CursorInfoProps) -> impl Into<AnyElement<'static>> {
    let arrow = if props.show_arrow.unwrap_or(false) {
        "↳ "
    } else {
        ""
    };
    let prefix = format!("{}Ln {}, Col {}: ", arrow, props.row + 1, props.col);
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());

    let total = props.budget.unwrap_or(60);
    let budget = total.saturating_sub(prefix_width).saturating_sub(1).max(8);
    let before_keep = budget / 2;
    let after_keep = budget - before_keep;
    let before_win = tail_to_width(&props.before, before_keep);
    let after_win = head_to_width(&props.after, after_keep);

    let prefix_color = props.prefix_color.unwrap_or(theme::yellow());

    element! {
        View(flex_direction: FlexDirection::Row) {
            Text(content: prefix, color: prefix_color, weight: Weight::Bold)
            Text(content: before_win, color: theme::fg())
            #(if let Some(bg) = props.cursor_bg {
                Some(element! {
                    View(background_color: bg) {
                        Text(content: props.cursor_char.clone(), color: theme::dark_bg())
                    }
                })
            } else {
                Some(element! {
                    View() {
                        Text(content: props.cursor_char.clone(), color: theme::fg())
                    }
                })
            }.into_iter())
            Text(content: after_win, color: theme::fg())
        }
    }
}
