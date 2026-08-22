use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::theme;
use iocraft::prelude::*;

/// Properties for the [`ThematicBreak`] component.
#[derive(Default, Props)]
pub struct ThematicBreakProps {
    /// Optional viewport dimensions so the rule can span the content width.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a horizontal rule (`---`, `***`, or `___`) as a full-width line of
/// box-drawing dashes spanning the available content columns.
#[component]
pub fn ThematicBreak(
    props: &ThematicBreakProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let highlighted = hooks.use_cached_highlight(source, MARKDOWN, theme::dark_grey());

    if let Some(mut raw) = props.raw.clone() {
        raw.highlight = Some(highlighted);
        return element! {
            View(margin_bottom: 1, background_color: theme::dark_bg()) {
                RawBuffer(raw: raw, color: theme::dark_grey())
            }
        }
        .into_any();
    }
    // Span the content area (viewport minus chrome) instead of a fixed length
    // so the rule reads as a true horizontal break at any terminal size.
    let width = props
        .viewport
        .as_ref()
        .and_then(|v| v.width)
        .unwrap_or(80)
        .saturating_sub(theme::CONTENT_H_INSET + theme::VIEWPORT_SCROLLBAR_WIDTH)
        .max(8);
    element! {
        View(margin_bottom: 1) {
            Text(content: "─".repeat(width as usize), color: theme::dark_grey())
        }
    }
    .into_any()
}
