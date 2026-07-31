use crate::components::inline_renderer::render_inlines;
use crate::components::scroll::Viewport;
use crate::document::model::{Alignment, TableCell, inlines_to_text};
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;

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
}

/// Renders a Markdown table with alignment support and styled borders.
///
/// Calculates column widths from content, renders headers with bold styling,
/// and applies alignment to each cell.
#[component]
pub fn TableBlock(props: &TableBlockProps, _hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let ncols = props.headers.len();
    if ncols == 0 {
        return element! {
            View(margin_bottom: 1) {
                Text(content: "Empty table".to_string(), color: theme::COMMENT)
            }
        }
        .into_any();
    }

    let max_table_width = props
        .viewport
        .as_ref()
        .and_then(|v| v.width)
        .unwrap_or(100)
        .saturating_sub(4)
        .max(20);
    let mut col_widths: Vec<u32> = props
        .headers
        .iter()
        .map(|cell| inlines_to_text(&cell.content).chars().count() as u32 + 2)
        .collect();

    for row in &props.rows {
        for (i, cell) in row.iter().enumerate().take(ncols) {
            col_widths[i] =
                col_widths[i].max(inlines_to_text(&cell.content).chars().count() as u32 + 2);
        }
    }

    let max_col_width = (max_table_width / ncols as u32).max(4);
    let min_col_width = max_col_width.min(6);
    for width in &mut col_widths {
        *width = (*width).clamp(min_col_width, max_col_width);
    }
    let table_width = col_widths.iter().sum::<u32>();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            margin_bottom: 1,
            border_style: BorderStyle::Single,
            border_color: theme::BORDER,
            background_color: theme::BG,
            width: table_width,
        ) {
            // Header Row
            View(flex_direction: FlexDirection::Row, border_style: BorderStyle::Single, border_edges: Edges::Bottom, border_color: theme::BORDER) {
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
                                #(render_inlines(&cell.content, theme::CYAN, true, Some(&props.file_path), props.viewport.as_ref().and_then(|v| v.height), props.viewport.as_ref().and_then(|v| v.width)))
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
                        background_color: if row_idx % 2 == 1 { Some(theme::DARK_BG) } else { None }
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
                                        #(render_inlines(&cell.content, theme::FG, false, Some(&props.file_path), props.viewport.as_ref().and_then(|v| v.height), props.viewport.as_ref().and_then(|v| v.width)))
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
