use crate::components::blocks_renderer::BlocksRenderer;
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
}

/// Renders a bullet or ordered list with its items.
///
/// Each item is prefixed with a bullet (`•`), checkbox (`☒`/`☐`),
/// or number. Nested blocks within items are rendered recursively.
#[component]
pub fn ListBlock(props: &ListBlockProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
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
                            Text(content: format!("{} ", marker), color: theme::YELLOW)
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
}
