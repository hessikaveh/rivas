use crate::components::blocks_renderer::BlocksRenderer;
use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::document::model::Block;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Properties for the [`QuoteBlock`] component.
#[derive(Default, Props)]
pub struct QuoteBlockProps {
    /// Child blocks inside the blockquote.
    pub children: Vec<Block>,
    /// Optional file path for link resolution in nested content.
    pub file_path: Option<PathBuf>,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a blockquote with a `▎` left-border accent and dark background.
#[component]
pub fn QuoteBlock(props: &QuoteBlockProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let file_path = props.file_path.clone().unwrap_or_default();
    let source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let highlighted = hooks.use_cached_highlight(source, MARKDOWN, theme::FG);
    if let Some(mut raw) = props.raw.clone() {
        raw.highlight = Some(highlighted);
        return element! {
            View(flex_direction: FlexDirection::Row, padding_left: 2, padding_right: 1, margin_bottom: 1, background_color: theme::DARK_BG) {
                View() {
                    Text(content: " ▎ ".to_string(), color: theme::TEAL)
                }
                View(flex_grow: 1.0) {
                    RawBuffer(raw: raw, color: theme::FG)
                }
            }
        };
    }
    element! {
        View(flex_direction: FlexDirection::Row, padding_left: 2, padding_right: 1, margin_bottom: 1, background_color: theme::DARK_BG) {
            View() {
                Text(content: " ▎ ".to_string(), color: theme::TEAL)
            }
            BlocksRenderer(
                blocks: props.children.clone(),
                file_path: file_path,
                viewport: props.viewport.clone(),
            )
        }
    }
}
