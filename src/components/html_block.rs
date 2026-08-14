use crate::components::inline_renderer::render_inlines;
use crate::components::scroll::Viewport;
use crate::document::parser::parse_html_fragment;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

/// Properties for the [`HtmlBlock`] component.
#[derive(Default, Props)]
pub struct HtmlBlockProps {
    /// Raw HTML content to display as text.
    pub content: String,
    /// Base path of the source document, for resolving relative image URLs.
    pub file_path: Option<PathBuf>,
    /// Optional viewport dimensions for responsive image rendering.
    pub viewport: Option<Viewport>,
}

/// Renders an HTML block by parsing its content with the supported inline
/// HTML tags and showing the resulting text, stripping everything else.
#[component]
pub fn HtmlBlock(props: &HtmlBlockProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let inlines = parse_html_fragment(&props.content);
    let styled_elements = render_inlines(
        &inlines,
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
