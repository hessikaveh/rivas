use crate::components::inline_renderer::render_inlines;
use crate::components::scroll::Viewport;
use crate::document::model::Inline;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

#[derive(Default, Props)]
pub struct HeadingProps {
    pub level: u8,
    pub content: Vec<Inline>,
    pub file_path: PathBuf,
    pub viewport: Option<Viewport>,
}

#[component]
pub fn Heading(props: &HeadingProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let prefix = "#".repeat(props.level as usize);
    let color = match props.level {
        1 => theme::CYAN,
        2 => theme::GREEN,
        3 => theme::YELLOW,
        _ => theme::FG,
    };

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
}
