use crate::components::highlight::{MARKDOWN, UseHighlight};
use crate::components::inline_renderer::render_inlines;
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::components::scroll::Viewport;
use crate::document::model::{Alignment, TableCell, inlines_to_text};
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

/// Properties for the [`TableBlock`] component.
#[derive(Default, Props)]
pub struct TableBlockProps {
    /// Column header cells.
    pub headers: Vec<TableCell>,
    /// Per-column alignment (left, center, right, none).
    pub alignments: Vec<Alignment>,
    /// Table body rows, each containing cells.
    pub rows: Vec<Vec<TableCell>>,
    /// File path for link resolution in cell content.
    pub file_path: PathBuf,
    /// Optional viewport dimensions for responsive rendering.
    pub viewport: Option<Viewport>,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a Markdown table with alignment support and styled borders.
///
/// Calculates column widths from content, renders headers with bold styling,
/// and applies alignment to each cell.
#[component]
pub fn TableBlock(props: &TableBlockProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
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

    let ncols = props.headers.len();
    if ncols == 0 {
        return element! {
            View(margin_bottom: 1) {
                Text(content: "Empty table".to_string(), color: theme::comment())
            }
        }
        .into_any();
    }

    let max_table_width = props
        .viewport
        .as_ref()
        .and_then(|v| v.width)
        .unwrap_or(100)
        .saturating_sub(6)
        .max(20);
    // Column widths use terminal display width (CJK/emoji count as 2 cells),
    // not char counts, otherwise borders misalign on wide glyphs. Each column
    // is sized dynamically to fit its widest cell (plus padding and one cell
    // of render slack) so content never wraps mid-word.
    let cell_text = |cell: &TableCell| inlines_to_text(&cell.content);
    let mut col_natural: Vec<u32> = props
        .headers
        .iter()
        .map(|cell| UnicodeWidthStr::width(cell_text(cell).as_str()) as u32)
        .collect();
    let mut col_word: Vec<u32> = props
        .headers
        .iter()
        .map(|cell| {
            cell_text(cell)
                .split_whitespace()
                .map(|w| UnicodeWidthStr::width(w) as u32)
                .max()
                .unwrap_or(0)
        })
        .collect();
    for row in &props.rows {
        for (i, cell) in row.iter().enumerate().take(ncols) {
            let text = cell_text(cell);
            col_natural[i] = col_natural[i].max(UnicodeWidthStr::width(text.as_str()) as u32);
            col_word[i] = col_word[i].max(
                text.split_whitespace()
                    .map(|w| UnicodeWidthStr::width(w) as u32)
                    .max()
                    .unwrap_or(0),
            );
        }
    }

    // Fit-to-content width; the extra cell avoids off-by-one wraps from
    // padding/border measurement inside the layout engine.
    const CELL_PAD: u32 = 2;
    let mut col_widths: Vec<u32> = col_natural.iter().map(|w| w + CELL_PAD + 1).collect();

    // When the natural sizes overflow the viewport, clamp columns — but never
    // below their longest word (+padding), so words stay unbroken.
    let max_col_width = (max_table_width / ncols as u32).max(4);
    for i in 0..ncols {
        let word_floor = (col_word[i] + CELL_PAD).min(max_col_width);
        col_widths[i] = col_widths[i].clamp(word_floor.max(4), max_col_width);
    }
    let table_width = col_widths.iter().sum::<u32>() + 2; // + outer border

    element! {
        View(
            flex_direction: FlexDirection::Column,
            margin_bottom: 1,
            border_style: BorderStyle::Single,
            border_color: theme::border(),
            background_color: theme::bg(),
            width: table_width,
        ) {
            // Header Row
            View(flex_direction: FlexDirection::Row, border_style: BorderStyle::Single, border_edges: Edges::Bottom, border_color: theme::border()) {
                #(props.headers.iter().enumerate().map(|(i, cell)| {
                    let alignment = props.alignments.get(i).cloned().unwrap_or(Alignment::None);
                    let justify = match alignment {
                        Alignment::Left | Alignment::None => JustifyContent::Start,
                        Alignment::Center => JustifyContent::Center,
                        Alignment::Right => JustifyContent::End,
                    };
                    element! {
                        View(width: col_widths[i], justify_content: justify, padding_left: 1, padding_right: 1) {
                            View(flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap) {
                                #(render_inlines(&cell.content, theme::cyan(), true, Some(&props.file_path), props.viewport.as_ref().and_then(|v| v.height), props.viewport.as_ref().and_then(|v| v.width)))
                            }
                        }
                    }.into_any()
                }))
            }
            // Data Rows
            #(props.rows.iter().enumerate().map(|(row_idx, row)| {
                element! {
                    View(
                        flex_direction: FlexDirection::Row,
                        background_color: if row_idx % 2 == 1 { Some(theme::dark_bg()) } else { None }
                    ) {
                        #((0..ncols).map(|col_idx| {
                            let cell = row.get(col_idx).cloned().unwrap_or(TableCell { content: Vec::new() });
                            let alignment = props.alignments.get(col_idx).cloned().unwrap_or(Alignment::None);
                            let justify = match alignment {
                                Alignment::Left | Alignment::None => JustifyContent::Start,
                                Alignment::Center => JustifyContent::Center,
                                Alignment::Right => JustifyContent::End,
                            };
                            element! {
                                View(width: col_widths[col_idx], justify_content: justify, padding_left: 1, padding_right: 1) {
                                    View(flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap) {
                                        #(render_inlines(&cell.content, theme::fg(), false, Some(&props.file_path), props.viewport.as_ref().and_then(|v| v.height), props.viewport.as_ref().and_then(|v| v.width)))
                                    }
                                }
                            }.into_any()
                        }))
                    }
                }.into_any()
            }))
        }
    }.into_any()
}
