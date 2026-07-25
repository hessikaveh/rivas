use crate::components::inline_renderer::render_inlines;
use crate::components::scroll::Viewport;
use crate::document::model::Inline;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

#[derive(Default, Props)]
pub struct ParagraphProps {
    pub content: Vec<Inline>,
    pub file_path: Option<PathBuf>,
    pub viewport: Option<Viewport>,
}

#[component]
pub fn Paragraph(props: &ParagraphProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
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
}
