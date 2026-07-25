use crate::components::blocks_renderer::BlocksRenderer;
use crate::components::scroll::Viewport;
use crate::document::model::Block;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

#[derive(Default, Props)]
pub struct QuoteBlockProps {
    pub children: Vec<Block>,
    pub file_path: Option<PathBuf>,
    pub viewport: Option<Viewport>,
}

#[component]
pub fn QuoteBlock(props: &QuoteBlockProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let file_path = props.file_path.clone().unwrap_or_default();
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
