use crate::assets::math::{
    MathMode, MathRender, bracket_glyphs, math_mode, render_math_unicode_ast,
};
use crate::components::highlight::{LATEX, UseHighlight};
use crate::components::kitty_graphic::{KittyGraphic, UseKittyGraphic};
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::debug;
use crate::output::graphics_manager::GfxSource;
use crate::theme;
use iocraft::prelude::*;

/// Properties for the [`MathBlock`] component.
#[derive(Default, Props)]
pub struct MathBlockProps {
    /// LaTeX math source code.
    pub content: String,
    /// `true` for display math (centered, larger), `false` for inline math.
    pub display: bool,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a LaTeX math block using either Unicode text or Kitty graphics,
/// depending on the current math mode setting.
#[component]
pub fn MathBlock(props: &MathBlockProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let source = props.raw.as_ref().map(|r| r.text.as_str()).unwrap_or("");
    let highlighted = hooks.use_cached_highlight(source, LATEX, theme::cyan());

    if let Some(mut raw) = props.raw.clone() {
        raw.highlight = Some(highlighted);
        element! {
            View(margin_bottom: 1, background_color: theme::dark_bg()) {
                RawBuffer(raw: raw, color: theme::cyan())
            }
        }
        .into_any()
    } else if math_mode() == MathMode::Image {
        element! {
            View(margin_bottom: 1) {
                KittyMath(content: props.content.clone(), display: props.display.clone(), viewport: props.viewport.clone())
            }
        }
        .into_any()
    } else {
        element! {
            UnicodeMath(content: props.content.clone(), display: props.display.clone())
        }
        .into_any()
    }
}

/// Image-free math renderer: converts LaTeX to Unicode glyphs and shows them
/// as plain terminal text. Matrices are laid out structurally via
/// Properties for the [`UnicodeMath`] component.
#[derive(Default, Props)]
pub struct UnicodeMathProps {
    /// LaTeX math source code to render as Unicode text.
    pub content: String,
    /// `true` for display math, `false` for inline math.
    pub display: bool,
}

/// Renders a LaTeX math expression as Unicode text in the terminal.
///
/// Converts LaTeX to Unicode using `unicodeit`, with brace-aware rewrites
/// for fractions, roots, and matrix layouts. Uses [`MatrixMath`] for
/// matrix environments to keep borders aligned.
#[component]
pub fn UnicodeMath(props: &UnicodeMathProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut cached = hooks.use_ref(|| (String::new(), MathRender::Text(String::new())));
    let render = {
        let guard = cached.read();
        if guard.0 == props.content {
            guard.1.clone()
        } else {
            drop(guard);
            let r = render_math_unicode_ast(&props.content);
            cached.set((props.content.clone(), r.clone()));
            r
        }
    };
    match render {
        MathRender::Text(text) => {
            if props.display {
                element! {
                    View(margin_bottom: 1, margin_left: 2) {
                        Text(content: text, color: theme::cyan())
                    }
                }
                .into_any()
            } else {
                element! { Text(content: text, color: theme::cyan()) }.into_any()
            }
        }
        MathRender::Matrix {
            rows,
            col_widths,
            kind,
        } => {
            let (l_open, l_mid, l_close, r_open, r_mid, r_close) = bracket_glyphs(kind);
            if props.display {
                element! {
                    View(margin_bottom: 1, margin_left: 2) {
                        MatrixMath(
                            rows: rows,
                            col_widths: col_widths,
                            l_open: l_open,
                            l_mid: l_mid,
                            l_close: l_close,
                            r_open: r_open,
                            r_mid: r_mid,
                            r_close: r_close,
                        )
                    }
                }
                .into_any()
            } else {
                element! {
                    MatrixMath(
                        rows: rows,
                        col_widths: col_widths,
                        l_open: l_open,
                        l_mid: l_mid,
                        l_close: l_close,
                        r_open: r_open,
                        r_mid: r_mid,
                        r_close: r_close,
                    )
                }
                .into_any()
            }
        }
    }
}

/// Structural matrix renderer. Lays rows out as flex rows so the bracket
/// glyphs (which are discrete cells) and column-padded cells always align,
/// instead of relying on space-padding inside a single string.
#[derive(Default, Props)]
pub struct MatrixMathProps {
    pub rows: Vec<Vec<String>>,
    pub col_widths: Vec<usize>,
    pub l_open: String,
    pub l_mid: String,
    pub l_close: String,
    pub r_open: String,
    pub r_mid: String,
    pub r_close: String,
}

#[component]
pub fn MatrixMath(props: &MatrixMathProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let nrows = props.rows.len();

    let mut row_elems: Vec<AnyElement<'static>> = Vec::new();
    for (ri, row) in props.rows.iter().enumerate() {
        let left = if ri == 0 {
            props.l_open.clone()
        } else if ri == nrows - 1 {
            props.l_close.clone()
        } else {
            props.l_mid.clone()
        };
        let right = if ri == 0 {
            props.r_open.clone()
        } else if ri == nrows - 1 {
            props.r_close.clone()
        } else {
            props.r_mid.clone()
        };

        let mut cell_elems: Vec<AnyElement<'static>> = Vec::new();
        cell_elems.push(element! { Text(content: left, color: theme::cyan()) }.into_any());
        cell_elems
            .push(element! { Text(content: " ".to_string(), color: theme::cyan()) }.into_any());
        for (ci, cell) in row.iter().enumerate() {
            let w = props.col_widths.get(ci).copied().unwrap_or(0);
            let pad = w.saturating_sub(unicode_width::UnicodeWidthStr::width(cell.as_str()));
            let content = format!("{}{}", cell, " ".repeat(pad));
            cell_elems.push(element! { Text(content: content, color: theme::cyan()) }.into_any());
        }
        cell_elems
            .push(element! { Text(content: " ".to_string(), color: theme::cyan()) }.into_any());
        cell_elems.push(element! { Text(content: right, color: theme::cyan()) }.into_any());

        row_elems.push(
            element! {
                View(flex_direction: FlexDirection::Row) {
                    #(cell_elems.into_iter())
                }
            }
            .into_any(),
        );
    }

    element! {
        View(flex_direction: FlexDirection::Column) {
            #(row_elems.into_iter())
        }
    }
    .into_any()
}

/// Properties for the [`KittyMath`] component.
#[derive(Default, Props)]
pub struct KittyMathProps {
    /// LaTeX math source code to render as an image.
    pub content: String,
    /// `true` for display math, `false` for inline math.
    pub display: bool,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
}

/// Renders a LaTeX math expression as a Kitty terminal graphic.
///
/// Uses the `mathjax` renderer to produce a PNG, then places it in the terminal.
#[component]
pub fn KittyMath(props: &KittyMathProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let vw = props.viewport.as_ref().and_then(|v| v.width).unwrap_or(100);
    let vh = props
        .viewport
        .as_ref()
        .and_then(|v| v.height)
        .unwrap_or(100);
    // Unique per-occurrence key so identical formulas don't share a terminal
    // graphic id (which would let one occurrence's detach/place clobber others).
    let cache_key = format!("math:{}:{}:{}", vw, props.display, props.content);
    let content = props.content.clone();
    let display = props.display;
    let gfx: KittyGraphic = hooks.use_kitty_graphic(
        cache_key,
        vw,
        vh,
        props.viewport.as_ref().and_then(|v| v.scroll_offset),
        if display { 2 } else { 1 },
        move |max_w, mc, mr| GfxSource::Math {
            content,
            display,
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
        let m_cols = cols.max(8);
        let m_rows = rows.max(3);
        let label = if props.display {
            "Math (display)"
        } else {
            "Math (inline)"
        };
        element! {
            View(
                width: m_cols,
                height: m_rows,
                border_style: BorderStyle::Single,
                border_color: theme::dbg_math(),
                background_color: theme::dbg_bg(),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            ) {
                Text(content: format!("{} {}x{}", label, m_cols, m_rows), color: theme::dbg_math(), weight: Weight::Bold)
            }
        }
        .into_any()
    } else {
        element! {View(width: cols.max(1), height: gfx.declared_rows.max(1))}.into_any()
    }
}
