use crate::components::kitty_graphic::{KittyGraphic, UseKittyGraphic};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::debug;
use crate::output::capabilities;
use crate::output::graphics_manager::GfxSource;
use crate::theme;
use iocraft::prelude::*;

/// Properties for the [`MermaidBlock`] component.
#[derive(Default, Props)]
pub struct MermaidBlockProps {
    /// Mermaid diagram source code.
    pub source: String,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Viewport columns at/above which the raw source editor sits beside the diagram
/// instead of underneath, when the cursor is on the block in Normal mode.
const SIDE_BY_SIDE_MIN_WIDTH: u32 = 120;
/// Columns reserved for the source editor when rendered side by side.
const SIDE_BY_SIDE_SOURCE: u32 = 40;
/// Minimum diagram width (columns) so the graphic never collapses.
const MIN_DIAGRAM_WIDTH: u32 = 20;

/// Renders a Mermaid diagram, using Kitty graphics if available, or text fallback.
#[component]
pub fn MermaidBlock(props: &MermaidBlockProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let raw = props.raw.clone();

    // Fallback: without kitty there is no diagram to preserve (the text fallback
    // already prints the source), so show the source editor alone.
    if raw.is_some() && !capabilities::has_kitty() {
        return element! {
            View(flex_direction: FlexDirection::Column, margin_bottom: 1, background_color: theme::dark_bg()) {
                RawBuffer(raw: raw.unwrap(), color: theme::fg())
            }
        }
        .into_any();
    }

    // Normal-mode cursor on the block + kitty: keep the diagram visible and put
    // the source editor beside it when there is room, otherwise stack it below.
    if let Some(raw) = raw {
        let vw = props.viewport.as_ref().and_then(|v| v.width).unwrap_or(100);
        let beside = vw >= SIDE_BY_SIDE_MIN_WIDTH;
        let diagram_viewport = if beside {
            // Reserve horizontal room for the source box so the diagram rasterizes
            // narrower and the two fit side by side.
            props.viewport.as_ref().cloned().map(|mut v| {
                v.width = Some(
                    vw.saturating_sub(SIDE_BY_SIDE_SOURCE)
                        .max(MIN_DIAGRAM_WIDTH),
                );
                v
            })
        } else {
            props.viewport.clone()
        };

        return element! {
            View(flex_direction: if beside { FlexDirection::Row } else { FlexDirection::Column }, margin_bottom: 1) {
                View() {
                    KittyMermaid(source: props.source.clone(), viewport: diagram_viewport)
                }
                View(margin_bottom: 1, background_color: theme::dark_bg()) {
                    RawBuffer(raw: raw, color: theme::fg())
                }
            }
        }
        .into_any();
    }

    element! {
       View(flex_direction: FlexDirection::Column, margin_bottom: 1) {
           #(if capabilities::has_kitty() {
                Some(element! {
                    KittyMermaid(source: props.source.clone(), viewport: props.viewport.clone())
                }.into_any())
           } else {
               Some(element! {
                   View(flex_direction: FlexDirection::Column, margin_bottom: 1) {
                       Text(content: "[Mermaid diagram]".to_string(), color: theme::comment())
                       #(props.source.lines().map(|line| element! {
                           Text(content: line.to_string(), color: theme::fg())
                       }).collect::<Vec<_>>())
                   }
               }.into_any())
           })
       }
    }
    .into_any()
}

/// Properties for the [`KittyMermaid`] component.
#[derive(Default, Props)]
pub struct KittyMermaidProps {
    /// Mermaid diagram source code to render via Kitty graphics.
    pub source: String,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
}

/// Renders a Mermaid diagram as a Kitty terminal graphic.
///
/// Parses the Mermaid source, renders to SVG, rasterizes to PNG,
/// and places it in the terminal. Handles scroll-into-view and layout.
#[component]
pub fn KittyMermaid(props: &KittyMermaidProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let vw = props.viewport.as_ref().and_then(|v| v.width).unwrap_or(100);
    let vh = props
        .viewport
        .as_ref()
        .and_then(|v| v.height)
        .unwrap_or(100);
    // Unique per-occurrence key so identical diagrams don't share a terminal
    // graphic id (which would let one occurrence's detach/place clobber others).
    let cache_key = format!("mermaid:{}:{}", vw, props.source);
    let source = props.source.clone();
    let gfx: KittyGraphic = hooks.use_kitty_graphic(
        cache_key,
        vw,
        vh,
        props.viewport.as_ref().and_then(|v| v.scroll_offset),
        10,
        move |max_w, mc, mr| GfxSource::Mermaid {
            source,
            max_w,
            max_cols: mc,
            max_rows: mr,
        },
    );
    let cols = gfx.cols;
    let rows = gfx.rows;

    if let Some(err) = gfx.error {
        return element! {
            View() {
                Text(content: err, color: theme::red())
            }
        }
        .into_any();
    }

    if debug::are_annotations_enabled() {
        let m_cols = cols.max(10);
        let m_rows = rows.max(5);
        element! {
            View(
                width: m_cols,
                height: m_rows,
                border_style: BorderStyle::Single,
                border_color: theme::dbg_mermaid(),
                background_color: theme::dbg_bg(),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            ) {
                Text(content: format!("Mermaid {}x{}", m_cols, m_rows), color: theme::dbg_mermaid(), weight: Weight::Bold)
            }
        }
        .into_any()
    } else {
        element! {View(width: cols.max(1), height: gfx.declared_rows.max(1))}.into_any()
    }
}
