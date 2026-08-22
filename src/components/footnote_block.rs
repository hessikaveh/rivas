use crate::components::blocks_renderer::BlocksRenderer;
use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::document::model::Block;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Properties for the [`FootnoteBlock`] component.
#[derive(Default, Props)]
pub struct FootnoteBlockProps {
    /// The footnote's label, e.g. `"1"` for `[^1]`.
    pub label: String,
    /// Child blocks making up the footnote body.
    pub children: Vec<Block>,
    /// File path for link resolution in nested content.
    pub file_path: Option<PathBuf>,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a footnote definition (`[^label]: ...`) with a dim marker and its
/// body indented underneath.
#[component]
pub fn FootnoteBlock(
    props: &FootnoteBlockProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let file_path = props.file_path.clone().unwrap_or_default();
    let source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let highlighted = hooks.use_cached_highlight(source, MARKDOWN, theme::fg());

    if let Some(mut raw) = props.raw.clone() {
        raw.highlight = Some(highlighted);
        return element! {
            View(margin_bottom: 1, background_color: theme::dark_bg()) {
                RawBuffer(raw: raw, color: theme::fg())
            }
        }
        .into_any();
    }

    // Marker column sized to the label ("[^" + label + "] " + one cell gap)
    // so longer names like `note` never overflow and wrap their closing bracket.
    let marker_width = (props.label.chars().count() + 5).max(6) as u32;

    element! {
        View(flex_direction: FlexDirection::Row, margin_bottom: 1) {
            View(width: marker_width) {
                Text(content: format!("[^{}] ", props.label), color: theme::comment())
            }
            View(flex_grow: 1.0) {
                BlocksRenderer(
                    blocks: props.children.clone(),
                    file_path: file_path,
                    viewport: props.viewport.clone(),
                )
            }
        }
    }
    .into_any()
}
