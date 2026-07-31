use crate::theme;
use iocraft::prelude::*;

/// Properties for the [`HtmlBlock`] component.
#[derive(Default, Props)]
pub struct HtmlBlockProps {
    /// Raw HTML content to display as a preview.
    pub content: String,
}

/// Renders a raw HTML block as a truncated preview in a bordered box.
///
/// Shows the first 50 characters of the first line as a preview,
/// since full HTML rendering is not supported.
#[component]
pub fn HtmlBlock(props: &HtmlBlockProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let preview = props
        .content
        .lines()
        .next()
        .unwrap_or("<html>")
        .chars()
        .take(50)
        .collect::<String>();

    element! {
        View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2, margin_bottom: 1, border_style: BorderStyle::Single) {
            View() {
                Text(content: "HTML Block".to_string(), color: theme::RED)
            }
            View {
                Text(content: preview, color: theme::COMMENT)
            }
        }
    }
}
