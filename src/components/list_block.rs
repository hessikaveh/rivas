use crate::components::blocks_renderer::BlocksRenderer;
use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::document::model::ListItem;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Properties for the [`ListBlock`] component.
#[derive(Default, Props)]
pub struct ListBlockProps {
    /// `true` for ordered lists (numbered), `false` for bullet lists.
    pub ordered: bool,
    /// Starting number for ordered lists (defaults to 1).
    pub start: Option<u64>,
    /// The list items to render.
    pub items: Vec<ListItem>,
    /// File path for link resolution in nested content.
    pub file_path: PathBuf,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a bullet or ordered list with its items.
///
/// Each item is prefixed with a bullet (`•`), checkbox (`☒`/`☐`),
/// or number. Nested blocks within items are rendered recursively.
#[component]
pub fn ListBlock(props: &ListBlockProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
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

    let mut num = props.start.unwrap_or(1);

    element! {
        View(flex_direction: FlexDirection::Column) {
            #(props.items.iter().map(|item| {
                let marker = if let Some(checked) = item.checked {
                    if checked { "☒" } else { "☐" }.to_string()
                } else if props.ordered {
                    let m = format!("{}.", num);
                    num += 1;
                    m
                } else {
                    "•".to_string()
                };

                element! {
                    View(flex_direction: FlexDirection::Row) {
                        View(width: 4) {
                            Text(content: format!("{} ", marker), color: theme::yellow())
                        }
                        View(flex_grow: 1.0) {
                        BlocksRenderer(
                                blocks: item.content.clone(),
                                file_path: props.file_path.clone(),
                                viewport: props.viewport.clone(),
                            )
                        }
                    }
                }
                .into_any()
            }))
        }
    }
    .into_any()
}
