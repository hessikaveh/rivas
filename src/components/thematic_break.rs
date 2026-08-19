use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::theme;
use iocraft::prelude::*;

/// Properties for the [`ThematicBreak`] component (currently empty).
#[derive(Default, Props)]
pub struct ThematicBreakProps {
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a horizontal rule (`---`, `***`, or `___`) as a line of dashes.
#[component]
pub fn ThematicBreak(
    props: &ThematicBreakProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let highlighted = hooks.use_cached_highlight(source, MARKDOWN, theme::DARK_GREY);

    if let Some(mut raw) = props.raw.clone() {
        raw.highlight = Some(highlighted);
        return element! {
            View(margin_bottom: 1, background_color: theme::DARK_BG, padding_left: 2, padding_right: 2) {
                RawBuffer(raw: raw, color: theme::DARK_GREY)
            }
        }
        .into_any();
    }
    element! {
        View(margin_bottom: 1) {
            Text(content: "───────────────────────────────".to_string(), color: theme::DARK_GREY)
        }
    }
    .into_any()
}
