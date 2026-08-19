use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::inline_renderer::render_inlines;
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::document::model::Inline;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Properties for the [`Paragraph`] component.
#[derive(Default, Props)]
pub struct ParagraphProps {
    /// Inline content to render as paragraph text.
    pub content: Vec<Inline>,
    /// Optional file path for link resolution.
    pub file_path: Option<PathBuf>,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a paragraph of inline Markdown content.
#[component]
pub fn Paragraph(props: &ParagraphProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let highlighted = hooks.use_cached_highlight(source, MARKDOWN, theme::FG);

    let element = if let Some(mut raw) = props.raw.clone() {
        raw.highlight = Some(highlighted);
        element! {
            View(margin_bottom: 1, background_color: theme::DARK_BG, padding_left: 2, padding_right: 2) {
                RawBuffer(raw: raw, color: theme::FG)
            }
        }
        .into_any()
    } else {
        let styled_elements = render_inlines(
            &props.content,
            theme::FG,
            false,
            props.file_path.as_ref(),
            props.viewport.as_ref().and_then(|v| v.height),
            props.viewport.as_ref().and_then(|v| v.width),
        );

        element! {
            View(margin_bottom: 1, flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap) {
                #(styled_elements)
            }
        }
        .into_any()
    };

    element
}
