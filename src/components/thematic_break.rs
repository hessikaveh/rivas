use crate::theme;
use iocraft::prelude::*;

/// Properties for the [`ThematicBreak`] component (currently empty).
#[derive(Default, Props)]
pub struct ThematicBreakProps {}

/// Renders a horizontal rule (`---`, `***`, or `___`) as a line of dashes.
#[component]
pub fn ThematicBreak(_props: &ThematicBreakProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    element! {
        View(margin_bottom: 1) {
            Text(content: "───────────────────────────────".to_string(), color: theme::DARK_GREY)
        }
    }
}
