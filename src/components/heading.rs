use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::inline_renderer::render_inlines;
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::document::model::Inline;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Properties for the [`Heading`] component.
#[derive(Default, Props)]
pub struct HeadingProps {
    /// Heading level (1–6), controls the `#` prefix count and color.
    pub level: u8,
    /// Inline content to render as the heading text.
    pub content: Vec<Inline>,
    /// Path to the file being rendered (for link resolution).
    pub file_path: PathBuf,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders an ATX heading (`#` through `######`) with level-based coloring.
#[component]
pub fn Heading(props: &HeadingProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let prefix = "#".repeat(props.level as usize);
    let color = match props.level {
        1 => theme::cyan(),
        2 => theme::green(),
        3 => theme::yellow(),
        4 => theme::orange(),
        5 => theme::magenta(),
        // h6 stays dimmer than body text but still distinct from plain prose.
        _ => theme::teal(),
    };

    let source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let highlighted = hooks.use_cached_highlight(source, MARKDOWN, color);

    let element = if let Some(mut raw) = props.raw.clone() {
        raw.highlight = Some(highlighted);
        element! {
            View(margin_bottom: 1, background_color: theme::dark_bg()) {
                RawBuffer(raw: raw, color: color)
            }
        }
        .into_any()
    } else {
        let styled_elements = render_inlines(
            &props.content,
            color,
            true,
            Some(&props.file_path),
            props.viewport.as_ref().and_then(|v| v.height),
            props.viewport.as_ref().and_then(|v| v.width),
        );

        element! {
            View(margin_bottom: 1, flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap) {
                Text(content: format!("{} ", prefix), color: color)
                #(styled_elements)
            }
        }
        .into_any()
    };

    element
}
