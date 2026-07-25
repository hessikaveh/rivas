use crate::components::code_block::CodeBlock;
use crate::components::cursor_info::CursorInfo;
use crate::components::editor::{EditorState, Mode};
use crate::components::heading::Heading;
use crate::components::html_block::HtmlBlock;
use crate::components::image::Image;
use crate::components::list_block::ListBlock;
use crate::components::math_block::MathBlock;
use crate::components::mermaid_block::MermaidBlock;
use crate::components::paragraph::Paragraph;
use crate::components::quote_block::QuoteBlock;
use crate::components::scroll::{
    Viewport, build_cumulative_heights, compute_scroll_into_view_target, estimate_block_height,
    find_cursor_block, visible_range_with_cursor,
};
use crate::components::table_block::TableBlock;
use crate::components::thematic_break::ThematicBreak;
use crate::debug;
use crate::document::model::Block;
use crate::output::graphics_manager::IMAGE_HEIGHT_CACHE;
use crate::theme;
use iocraft::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default, Props)]
struct ScrollIntoViewContainerProps {
    pub scroll_handle: Option<Ref<ScrollViewHandle>>,
    pub cursor_moved: bool,
    pub child: Option<Arc<dyn Fn() -> AnyElement<'static> + Send + Sync + 'static>>,
    /// Row of the cursor within this block (0-indexed). Used for precise
    /// scroll-into-view when the block is taller than the viewport.
    pub cursor_row: Option<i32>,
    /// Extra rows below the block content to keep visible (e.g. the status
    /// box showing "Ln X, Col Y: ...").
    pub bottom_offset: Option<i32>,
}

#[component]
fn ScrollIntoViewContainer(
    props: &ScrollIntoViewContainerProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let rect = hooks.use_component_rect();
    let mut pending = hooks.use_state(|| false);

    if props.cursor_moved {
        pending.set(true);
    }

    // Baseline invariant: `use_component_rect()` is measured from the *previous*
    // frame and is therefore one scroll-step stale. `rect.top + scroll_off`
    // recovers the block's true content-row position, but while a scroll is in
    // flight `rect.top` still reflects the old layout, so the sum is wrong for a
    // single frame. We capture the invariant only once `rect.top` actually
    // settles (changes), so a mid-flight stale rect never feeds a bogus target
    // and the auto-scroll converges in one step instead of oscillating.
    let mut baseline = hooks.use_state(|| None::<(i32, i32)>);
    let mut last_rect_top = hooks.use_state(|| i32::MIN);
    if let Some(r) = rect {
        if r.top != last_rect_top.get() {
            if let Some(scroll_ref) = &props.scroll_handle {
                let scroll_off = scroll_ref.read().scroll_offset();
                let top = r.top + scroll_off;
                let bottom = r.bottom + scroll_off;
                baseline.set(Some((top, bottom)));
                last_rect_top.set(r.top);
            }
        }
    }

    hooks.use_effect(
        {
            let mut pending = pending.clone();
            let scroll_handle = props.scroll_handle.clone();
            let baseline = baseline.clone();
            let cursor_row = props.cursor_row;
            let bottom_offset = props.bottom_offset.unwrap_or(0);
            move || {
                // Only consume the pending request once we actually have a
                // baseline (i.e. `use_component_rect` has reported at least one
                // real frame for this element). A freshly mounted cursor block
                // gets its first rect a frame after `cursor_moved`, so retrying
                // here is what makes `j`/`k` scroll the view to follow the
                // cursor instead of silently doing nothing.
                if pending.get() && baseline.get().is_some() {
                    if let (Some(scroll_ref), Some((block_top_content, block_bottom_content))) =
                        (&scroll_handle, baseline.get())
                    {
                        let mut scroll_ref = scroll_ref.clone();
                        let viewport_h = scroll_ref.read().viewport_height() as i32;
                        let content_h = scroll_ref.read().content_height() as i32;

                        if viewport_h > 0 {
                            let scroll_off = scroll_ref.read().scroll_offset();
                            let target = compute_scroll_into_view_target(
                                block_top_content,
                                block_bottom_content,
                                viewport_h,
                                content_h,
                                scroll_off,
                                cursor_row,
                                bottom_offset,
                            );
                            if let Some(target) = target {
                                scroll_ref.write().scroll_to(target);
                                debug::log_event(&debug::DebugEvent::CursorScroll {
                                    ts: debug::elapsed_ms(),
                                    block_top: block_top_content,
                                    block_bottom: block_bottom_content,
                                    scroll_off,
                                    target,
                                    viewport_h,
                                });
                            }
                        }
                        pending.set(false);
                    }
                }
            }
        },
        (pending.get(), baseline.get()),
    );

    element! {
        View() {
            #(props.child.as_ref().map(|f| f()).into_iter())
        }
    }
}

#[derive(Default, Props)]
pub struct BlocksRendererProps {
    pub blocks: Vec<Block>,
    pub content: String,
    pub file_path: PathBuf,
    pub viewport: Option<Viewport>,
    pub cursor_offset: Option<Ref<usize>>,
    pub editor_state: Option<Ref<Option<EditorState>>>,
    pub scroll_handle: Option<Ref<ScrollViewHandle>>,
    pub debug: bool,
    pub debug_annotations: bool,
}

#[component]
pub fn BlocksRenderer(
    props: &BlocksRendererProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let cursor_offset_val = props.cursor_offset.as_ref().map(|r| r.get());
    let last_offset = hooks.use_state(|| cursor_offset_val.unwrap_or(0));
    let cursor_moved = cursor_offset_val.map_or(false, |off| off != last_offset.get());

    hooks.use_effect(
        {
            let mut last_offset = last_offset.clone();
            move || {
                if let Some(off) = cursor_offset_val {
                    last_offset.set(off);
                }
            }
        },
        cursor_offset_val,
    );
    let file_path = props.file_path.clone();
    let vh = props.viewport.as_ref().and_then(|v| v.height);
    let vw = props.viewport.as_ref().and_then(|v| v.width);
    let cursor_offset = props.cursor_offset.as_ref().map(|r| r.get());

    let (vis_start, vis_end, mode, is_editing_mode, cursor_row_col) =
        if let Some(state_ref) = &props.editor_state {
            let s_opt = state_ref.read();
            if let Some(s) = s_opt.as_ref() {
                let start = s.absolute_byte_offset_at(s.visual_start.0, s.visual_start.1);
                let end = s.absolute_byte_offset();
                let editing = matches!(s.mode, Mode::Insert | Mode::Command | Mode::Search { .. });
                (
                    Some(start.min(end)),
                    Some(start.max(end)),
                    s.mode.clone(),
                    editing,
                    Some((s.row, s.col)),
                )
            } else {
                (None, None, Mode::Normal, false, None)
            }
        } else {
            (None, None, Mode::Normal, false, None)
        };

    let block_counts = props.blocks.len();

    // Cache cumulative block heights and start offsets — only recompute when blocks change
    let cum_key = format!(
        "{}:{}:{}",
        block_counts,
        vw.unwrap_or(0),
        IMAGE_HEIGHT_CACHE.generation()
    );
    let mut cum_data = hooks.use_ref(|| (Vec::<u32>::new(), Vec::<usize>::new()));
    let mut cum_key_ref = hooks.use_ref(String::new);
    if *cum_key_ref.read() != cum_key {
        cum_data.set(build_cumulative_heights(&props.blocks, &props.content, vw));
        cum_key_ref.set(cum_key);
    }

    // Binary search to find visible range using cached cumulative heights
    let scroll_offset = props
        .viewport
        .as_ref()
        .and_then(|v| v.scroll_offset)
        .unwrap_or(0)
        .max(0) as u32;
    let viewport_h = props.viewport.as_ref().map(|v| v.height()).unwrap_or(24);
    let buffer = viewport_h * 2;
    let (heights, starts) = {
        let d = cum_data.read();
        (d.0.clone(), d.1.clone())
    };

    let cursor_block_idx = find_cursor_block(&starts, cursor_offset, block_counts);

    let (first_visible, last_visible) = visible_range_with_cursor(
        scroll_offset,
        viewport_h as u32,
        buffer,
        &heights,
        block_counts,
        cursor_block_idx,
    );

    // Log render tick for debug
    if props.debug {
        if let Some(state_ref) = &props.editor_state {
            let s_opt = state_ref.read();
            if let Some(s) = s_opt.as_ref() {
                debug::log_event(&debug::DebugEvent::RenderTick {
                    ts: debug::elapsed_ms(),
                    cursor: debug::CursorPos {
                        byte: s.absolute_byte_offset(),
                        row: s.row,
                        col: s.col,
                    },
                    scroll: props
                        .viewport
                        .as_ref()
                        .and_then(|v| v.scroll_offset)
                        .unwrap_or(0),
                    content_height: props
                        .scroll_handle
                        .as_ref()
                        .map(|h| h.read().content_height() as i32)
                        .unwrap_or(0),
                    viewport: debug::ViewportInfo {
                        w: vw.unwrap_or(80),
                        h: vh.unwrap_or(24),
                    },
                    blocks: block_counts,
                    mode: format!("{:?}", s.mode),
                });
            }
        }
    }

    element! {
            View(flex_direction: FlexDirection::Column) {
                #(props.blocks.iter().enumerate().map(|(i, block)| {
                    // Virtual scrolling: skip off-screen blocks
                    if i < first_visible || i >= last_visible {
                        let h = heights[i + 1] - heights[i];
                        return element! { View(height: h) {} }.into_any();
                    }

                    let span = block.span();
                    let next_span_start = props.blocks.get(i + 1).map(|b| b.span().0).unwrap_or(props.content.len());

                    // is_cursor_here: cursor is on this block or in the gap before the next block
                    let is_cursor_here = cursor_offset.map_or(false, |off| {
                        if i + 1 == block_counts {
                            off >= span.0 && off <= next_span_start
                        } else {
                            off >= span.0 && off < next_span_start
                        }
                    });
                    // Only show raw text editing view when cursor is on the block AND
                    // the editor is in an editing mode (Insert/Command/Search).
                    // In Normal mode, blocks stay as their rendered markdown form (view-only).
                    let is_active = is_editing_mode && is_cursor_here;
                    let is_selected = mode == Mode::Visual && vis_start.map_or(false, |start| {
                        vis_end.map_or(false, |end| {
                            span.0 <= end && span.1 >= start
                        })
                    });

                    if is_active || is_selected {
                        let off = cursor_offset.unwrap_or(0);
                        let text_end = if is_active && off > span.1 {
                            if i + 1 == block_counts {
                                off.min(next_span_start)
                            } else {
                                off.min(next_span_start - 1)
                            }
                        } else {
                            span.1
                        };
                        let text = &props.content[span.0..text_end];
                        let rel_off = (off - span.0).min(text.len());

                        let lines: Vec<&str> = text.split('\n').collect();
                        let mut current_byte_acc = 0;
                        let mut cursor_line_idx = None;
                        let mut cursor_rel_off = 0;

                        for (idx, line) in lines.iter().enumerate() {
                            let line_len = line.len();
                            if rel_off >= current_byte_acc && rel_off <= current_byte_acc + line_len {
                                cursor_line_idx = Some(idx);
                                cursor_rel_off = rel_off - current_byte_acc;
                            }
                            current_byte_acc += line_len + 1;
                        }

                        let cursor_bg = match mode {
                            Mode::Normal => theme::FG,
                            Mode::Insert => theme::GREEN,
                            Mode::Visual => theme::MAGENTA,
                            Mode::Command | Mode::Search { .. } => theme::YELLOW,
                        };

                        let (cursor_fg, cursor_bg_final, cursor_char) = if let Some(state_ref) = &props.editor_state {
                            let s_opt = state_ref.read();
                            if let Some(s) = s_opt.as_ref() {
                                if s.mode == Mode::Insert {
                                    (cursor_bg, theme::DARK_BG, "┃")
                                } else if s.operator.is_some() {
                                    (cursor_bg, theme::DARK_BG, "_")
                                } else {
                                    (theme::DARK_BG, cursor_bg, " ")
                                }
                            } else {
                                (theme::DARK_BG, cursor_bg, " ")
                            }
                        } else {
                            (theme::DARK_BG, cursor_bg, " ")
                        };

                        // Convert to owned strings for the factory closure
                        let lines_owned: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
                        let span_siv = span;
                        let vw_siv = vw;
                        let vis_start_siv = vis_start;
                        let vis_end_siv = vis_end;
                        let mode_siv = mode.clone();
                        let cursor_line_idx_siv = cursor_line_idx;
                        let cursor_rel_off_siv = cursor_rel_off;
                        let cursor_char_siv = cursor_char.to_string();
                        let cursor_fg_siv = cursor_fg;
                        let cursor_bg_final_siv = cursor_bg_final.clone();
                        let editor_state_siv = props.editor_state.clone();
                        let scroll_handle_siv = props.scroll_handle.clone();

                        let factory: Arc<dyn Fn() -> AnyElement<'static> + Send + Sync + 'static> = Arc::new(move || {
                            element! {
                                View(
                                    background_color: theme::DARK_BG,
                                    padding_left: 2,
                                    padding_right: 2,
                                    flex_direction: FlexDirection::Column,
                                    overflow: Overflow::Hidden,
                                ) {
                                    #(lines_owned.iter().enumerate().map(|(idx, line)| {
                                        let line_start_off = span_siv.0 + lines_owned[..idx].iter().map(|l| l.len() + 1).sum::<usize>();
                                        let wrap_width = (vw_siv.unwrap_or(80) as i32 - theme::TOTAL_VIEWPORT_OFFSET as i32).max(1) as usize;
                                        let mut segments = Vec::new();
                                        let mut remaining: &str = line;
                                        while !remaining.is_empty() {
                                            let mut split_at = remaining.char_indices().nth(wrap_width).map(|(i, _)| i).unwrap_or(remaining.len());

                                            if split_at < remaining.len() {
                                                if let Some(last_space) = remaining[..split_at].rfind(' ') {
                                                    if last_space > 0 {
                                                        split_at = last_space + 1;
                                                    }
                                                }
                                            }
                                            segments.push(&remaining[..split_at]);
                                            remaining = &remaining[split_at..];
                                        }
                                        if segments.is_empty() {
                                            segments.push("");
                                        }

                                        element! {
                                            View(flex_direction: FlexDirection::Column) {
                                                #(segments.iter().enumerate().map(|(seg_idx, segment)| {
                                                    if mode_siv == Mode::Visual {
                                                        if let (Some(start), Some(end)) = (vis_start_siv, vis_end_siv) {
                                                            let seg_start_off = line_start_off + segments[..seg_idx].iter().map(|s| s.len()).sum::<usize>();
                                                            let mut line_parts: Vec<(bool, String)> = Vec::new();
                                                            let mut current_pos = seg_start_off;
                                                            let seg_chars: Vec<char> = segment.chars().collect();
                                                            for c in seg_chars {
                                                                let char_len = c.len_utf8();
                                                                let is_selected = current_pos >= start && current_pos <= end;
                                                                if let Some(last) = line_parts.last_mut() {
                                                                    if last.0 == is_selected {
                                                                        last.1.push(c);
                                                                        current_pos += char_len;
                                                                        continue;
                                                                    }
                                                                }
                                                                line_parts.push((is_selected, c.to_string()));
                                                                current_pos += char_len;
                                                            }
                                                            element! {
                                                                View(flex_direction: FlexDirection::Row) {
                                                                    #(line_parts.iter().map(|(selected, text)| element! {
                                                                        Text(content: text.clone(), color: if *selected { theme::MAGENTA } else { theme::FG }, wrap: TextWrap::Wrap)
                                                                    }))
                                                                }
                                                            }.into_any()
                                                        } else {
                                                            element! { Text(content: segment.to_string(), color: theme::FG, wrap: TextWrap::Wrap) }.into_any()
                                                        }
                                                    } else if Some(idx) == cursor_line_idx_siv {
                                                        let mut seg_idx_cursor = 0;
                                                        let mut seg_rel_off = cursor_rel_off_siv;
                                                        for seg in &segments {
                                                            if seg_rel_off <= seg.len() { break; }
                                                            seg_rel_off -= seg.len();
                                                            seg_idx_cursor += 1;
                                                        }
                                                        if seg_idx == seg_idx_cursor {
                                                            let (before, after_with_char) = segment.split_at(seg_rel_off.min(segment.len()));

                                                            let before_str = before.to_string();
                                                            let cursor_char_str = cursor_char_siv.clone();
                                                            let cursor_bg_final_clone = cursor_bg_final_siv.clone();
                                                            let cursor_fg_clone = cursor_fg_siv;
                                                            let editor_state_clone = editor_state_siv.clone();

                                                            let inner_factory: Arc<dyn Fn() -> AnyElement<'static> + Send + Sync + 'static> = if let Some(c) = after_with_char.chars().next() {
                                                                let char_len = c.len_utf8();
                                                                let after_str = after_with_char[char_len..].to_string();
                                                                let c_str = c.to_string();

                                                                Arc::new(move || {
                                                                    if let Some(state_ref) = &editor_state_clone {
                                                                        let s_opt = state_ref.read();
                                                                        if let Some(s) = s_opt.as_ref() {
                                                                            if s.mode == Mode::Insert {
                                                                                element! {
                                                                                    View(flex_direction: FlexDirection::Row) {
                                                                                        Text(content: before_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                        View(background_color: cursor_bg_final_clone, width: 1) {
                                                                                            Text(content: cursor_char_str.clone(), color: cursor_fg_clone, wrap: TextWrap::Wrap)
                                                                                        }
                                                                                        Text(content: format!("{}{}", c_str, after_str), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                    }
                                                                                }.into_any()
                                                                            } else if s.operator.is_some() {
                                                                                element! {
                                                                                    View(flex_direction: FlexDirection::Row) {
                                                                                        Text(content: before_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                        View(background_color: cursor_bg_final_clone, width: 1) {
                                                                                            Text(content: c_str.clone(), color: cursor_fg_clone, wrap: TextWrap::Wrap)
                                                                                        }
                                                                                        Text(content: after_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                    }
                                                                                }.into_any()
                                                                            } else {
                                                                                element! {
                                                                                    View(flex_direction: FlexDirection::Row) {
                                                                                        Text(content: before_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                        View(background_color: cursor_bg_final_clone, width: 1) {
                                                                                            Text(content: c_str.clone(), color: cursor_fg_clone, wrap: TextWrap::Wrap)
                                                                                        }
                                                                                        Text(content: after_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                    }
                                                                                }.into_any()
                                                                            }
                                                                        } else {
                                                                            element! {
                                                                                View(flex_direction: FlexDirection::Row) {
                                                                                    Text(content: before_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                    View(background_color: cursor_bg_final_clone, width: 1) {
                                                                                        Text(content: c_str.clone(), color: cursor_fg_clone, wrap: TextWrap::Wrap)
                                                                                    }
                                                                                    Text(content: after_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                }
                                                                            }.into_any()
                                                                        }
                                                                    } else {
                                                                        element! {
                                                                            View(flex_direction: FlexDirection::Row) {
                                                                                Text(content: before_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                                View(background_color: cursor_bg_final_clone, width: 1) {
                                                                                    Text(content: c_str.clone(), color: cursor_fg_clone, wrap: TextWrap::Wrap)
                                                                                }
                                                                                Text(content: after_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                            }
                                                                        }.into_any()
                                                                    }
                                                                })
                                                            } else {
                                                                Arc::new(move || {
                                                                    element! {
                                                                        View(flex_direction: FlexDirection::Row) {
                                                                            Text(content: before_str.clone(), color: theme::FG, wrap: TextWrap::Wrap)
                                                                            View(background_color: cursor_bg_final_clone, width: 1) {
                                                                                Text(content: cursor_char_str.clone(), color: cursor_fg_clone, wrap: TextWrap::Wrap)
                                                                            }
                                                                            Text(content: "", color: theme::FG, wrap: TextWrap::Wrap)
                                                                        }
                                                                    }.into_any()
                                                                })
                                                            };

                                                            element! {
                                                                ScrollIntoViewContainer(
                                                                    scroll_handle: scroll_handle_siv.clone(),
                                                                    cursor_moved,
                                                                    child: Some(inner_factory),
                                                                    cursor_row: cursor_line_idx_siv.map(|r| r as i32),
                                                                    bottom_offset: Some(0),
                                                                )
                                                            }.into_any()
                                                        } else {
                                                            element! { Text(content: segment.to_string(), color: theme::FG, wrap: TextWrap::Wrap) }.into_any()
                                                        }
                                                    } else {
                                                        element! { Text(content: segment.to_string(), color: theme::FG, wrap: TextWrap::Wrap) }.into_any()
                                                    }
                                                }))
                                            }
                                        }.into_any()
                                    }))
                                }
                            }.into_any()
                        });

                        element! {
                            ScrollIntoViewContainer(
                                scroll_handle: scroll_handle_siv,
                                cursor_moved,
                                child: Some(factory),
                                cursor_row: cursor_line_idx.map(|r| r as i32),
                                bottom_offset: Some(0),
                            )
                        }.into_any()
                    } else {
                        // Render block as formatted markdown.
                        // If cursor is on this block (Normal mode), wrap with a left-border
                        // accent so the user can see where the cursor is before pressing `i`.
                        let rendered = match block {
                            Block::Heading { level, content, .. } => element!{
    Heading(level: *level, content: content.clone(), file_path: file_path.clone(), viewport: props.viewport.clone())}.into_any(),
                            Block::Paragraph { content, .. } => element!{Paragraph(content: content.clone(), file_path: file_path.clone(), viewport: props.viewport.clone())}.into_any(),
                            Block::Code { language, code, .. } => element!{CodeBlock(language: language.clone(), code: code.clone())}.into_any(),
                            Block::Mermaid { source, .. } => element!{MermaidBlock(source: source.clone(), viewport: props.viewport.clone())}.into_any(),
                            Block::Math { content, display, .. } => element!{MathBlock(content: content.clone(), display: *display, viewport: props.viewport.clone())}.into_any(),
                            Block::Quote { children, .. } => element!{QuoteBlock(children: children.clone(), file_path: Some(file_path.clone()), viewport: props.viewport.clone())}.into_any(),
                            Block::List { ordered, start, items, .. } => element!{ListBlock(ordered: *ordered, start: *start, items: items.clone(), file_path: file_path.clone(), viewport: props.viewport.clone())}.into_any(),
                            Block::Table { headers, alignments, rows, .. } => element!{TableBlock(headers: headers.clone(), alignments: alignments.clone(), rows: rows.clone(), file_path: file_path.clone(), viewport: props.viewport.clone())}.into_any(),
                            Block::ThematicBreak{..} => element!{ThematicBreak()}.into_any(),
                            Block::Image { alt, url, title, .. } => element!{Image(url: url.clone(), file_path: file_path.clone(), title: title.clone(), alt: Some(alt.clone()), viewport: props.viewport.clone())}.into_any(),
                            Block::Html { content, .. } => element!{HtmlBlock(content: content.clone())}.into_any(),
                        };

                        // Wrap with debug border/label if debug annotations are enabled
                        let rendered = if props.debug_annotations {
                            let (label, color) = match block {
                                Block::Heading { .. } => ("H".to_string(), theme::DBG_HEADING),
                                Block::Paragraph { .. } => ("P".to_string(), theme::DBG_PARAGRAPH),
                                Block::Code { .. } => ("Code".to_string(), theme::DBG_CODE),
                                Block::Image { .. } => ("Img".to_string(), theme::DBG_IMAGE),
                                Block::Math { .. } => ("Math".to_string(), theme::DBG_MATH),
                                Block::Mermaid { .. } => ("Mermaid".to_string(), theme::DBG_MERMAID),
                                Block::Quote { .. } => (">".to_string(), theme::DBG_QUOTE),
                                Block::Table { .. } => ("Table".to_string(), theme::DBG_TABLE),
                                Block::List { .. } => ("List".to_string(), theme::DBG_LIST),
                                Block::ThematicBreak { .. } => ("---".to_string(), theme::DBG_BREAK),
                                Block::Html { .. } => ("HTML".to_string(), theme::DBG_HTML),
                            };
                            let est_h = estimate_block_height(block, &props.content, vw);
                            debug::log_event(&debug::DebugEvent::BlockLayout {
                                ts: debug::elapsed_ms(),
                                idx: i,
                                block_type: label.clone(),
                                span_start: span.0,
                                span_end: span.1,
                                est_height: est_h,
                            });
                            element! {
                                View(flex_direction: FlexDirection::Column) {
                                    View(flex_direction: FlexDirection::Row, background_color: color, padding_left: 1) {
                                        Text(content: format!("[{} {}..{} h={}]", label, span.0, span.1, est_h), color: theme::DARK_BG, weight: Weight::Bold)
                                    }
                                    View(border_style: BorderStyle::Single, border_color: color, background_color: theme::DBG_BG) {
                                        #(Some(rendered).into_iter())
                                    }
                                }
                            }.into_any()
                        } else {
                            rendered
                        };

                        if is_cursor_here && !is_editing_mode {
                            // Show a left-border accent indicator on the active block
                            // so the user knows where the cursor is in Normal mode.
                            let off = cursor_offset.unwrap_or(0);
                            let text = &props.content[span.0..span.1];
                            let rel_off = off.saturating_sub(span.0).min(text.len());

                            let lines: Vec<&str> = text.split('\n').collect();
                            let mut current_byte_acc = 0;
                            let mut cursor_line_idx = None;
                            let mut cursor_rel_off = 0;

                            for (idx, line) in lines.iter().enumerate() {
                                let line_len = line.len();
                                if rel_off >= current_byte_acc && rel_off <= current_byte_acc + line_len {
                                    cursor_line_idx = Some(idx);
                                    cursor_rel_off = rel_off - current_byte_acc;
                                }
                                current_byte_acc += line_len + 1;
                            }

                            let mut cursor_line_text = "";
                            let mut cursor_char_idx = 0;
                            if let Some(idx) = cursor_line_idx {
                                if idx < lines.len() {
                                    cursor_line_text = lines[idx];
                                    cursor_char_idx = cursor_rel_off;
                                }
                            }

                            let before = &cursor_line_text[..cursor_char_idx.min(cursor_line_text.len())];
                            let char_at_cursor = cursor_line_text.char_indices()
                                .find(|&(idx, _)| idx == cursor_char_idx)
                                .map(|(_, c)| c);
                            let cursor_char = char_at_cursor.map(|c| c.to_string()).unwrap_or_else(|| " ".to_string());
                            let after = if let Some(c) = char_at_cursor {
                                let char_len = c.len_utf8();
                                &cursor_line_text[(cursor_char_idx + char_len).min(cursor_line_text.len())..]
                            } else {
                                ""
                            };

                            let block_clone = block.clone();
                            let file_path_clone = file_path.clone();
                            let viewport_clone = props.viewport.clone();
                            let before_str = before.to_string();
                            let cursor_char_str = cursor_char.to_string();
                            let after_str = after.to_string();
                            let cursor_row_col_clone = cursor_row_col.clone();
                            let info_row = cursor_row_col_clone.map(|(r, _)| r).unwrap_or(0);
                            let info_col = cursor_row_col_clone.map(|(_, c)| c).unwrap_or(0);
                            let total = viewport_clone.as_ref().map(|v| v.width()).unwrap_or(80)
                                .saturating_sub(theme::TOTAL_VIEWPORT_OFFSET + 12) as usize;

                            let factory: Arc<dyn Fn() -> AnyElement<'static> + Send + Sync + 'static> = Arc::new(move || {
                                let rendered = match &block_clone {
                                    Block::Heading { level, content, .. } => element!{
    Heading(level: *level, content: content.clone(), file_path: file_path_clone.clone(), viewport: viewport_clone.clone())}.into_any(),
                                    Block::Paragraph { content, .. } => element!{Paragraph(content: content.clone(), file_path: file_path_clone.clone(), viewport: viewport_clone.clone())}.into_any(),
                                    Block::Code { language, code, .. } => element!{CodeBlock(language: language.clone(), code: code.clone())}.into_any(),
                                    Block::Mermaid { source, .. } => element!{MermaidBlock(source: source.clone(), viewport: viewport_clone.clone())}.into_any(),
                                    Block::Math { content, display, .. } => element!{MathBlock(content: content.clone(), display: *display, viewport: viewport_clone.clone())}.into_any(),
                                    Block::Quote { children, .. } => element!{QuoteBlock(children: children.clone(), file_path: Some(file_path_clone.clone()), viewport: viewport_clone.clone())}.into_any(),
                                    Block::List { ordered, start, items, .. } => element!{ListBlock(ordered: *ordered, start: *start, items: items.clone(), file_path: file_path_clone.clone(), viewport: viewport_clone.clone())}.into_any(),
                                    Block::Table { headers, alignments, rows, .. } => element!{TableBlock(headers: headers.clone(), alignments: alignments.clone(), rows: rows.clone(), file_path: file_path_clone.clone(), viewport: viewport_clone.clone())}.into_any(),
                                    Block::ThematicBreak{..} => element!{ThematicBreak()}.into_any(),
                                    Block::Image { alt, url, title, .. } => element!{Image(url: url.clone(), file_path: file_path_clone.clone(), title: title.clone(), alt: Some(alt.clone()), viewport: viewport_clone.clone())}.into_any(),
                                    Block::Html { content, .. } => element!{HtmlBlock(content: content.clone())}.into_any(),
                                };

                                element! {
                                    View(flex_direction: FlexDirection::Column) {
                                        View(flex_direction: FlexDirection::Row) {
                                            View(width: 2, background_color: theme::BLUE) {}
                                            View(flex_grow: 1.0, background_color: theme::STATUS_BG) {
                                                #(Some(rendered).into_iter())
                                            }
                                        }
                                        View(
                                            padding_left: 4,
                                            padding_right: 2,
                                            margin_bottom: 1,
                                            background_color: theme::DARK_BG,
                                        ) {
                                            CursorInfo(
                                                row: info_row,
                                                col: info_col,
                                                before: before_str.clone(),
                                                cursor_char: cursor_char_str.clone(),
                                                after: after_str.clone(),
                                                show_arrow: Some(true),
                                                cursor_bg: Some(theme::BLUE),
                                                budget: Some(total),
                                            )
                                        }
                                    }
                                }.into_any()
                            });

                            element! {
                                ScrollIntoViewContainer(
                                    scroll_handle: props.scroll_handle.clone(),
                                    cursor_moved,
                                    child: Some(factory),
                                    cursor_row: cursor_line_idx.map(|r| r as i32),
                                    bottom_offset: Some(2),
                                )
                            }.into_any()
                        } else {
                            rendered
                        }
                    }
                }))
            }
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::*;

    #[test]
    fn estimate_heading_height() {
        let block = Block::Heading {
            level: 1,
            content: vec![Inline::Text("Hello".to_string())],
            span: (0, 10),
        };
        assert_eq!(estimate_block_height(&block, "", Some(80)), 2);
    }

    #[test]
    fn estimate_paragraph_height_wraps() {
        // 160 chars at wrap_width=80 → 2 rows
        let long_text = "a".repeat(160);
        let block = Block::Paragraph {
            content: vec![Inline::Text(long_text)],
            span: (0, 160),
        };
        assert_eq!(estimate_block_height(&block, "", Some(80)), 2);
    }

    #[test]
    fn estimate_paragraph_short_fits_one_row() {
        let block = Block::Paragraph {
            content: vec![Inline::Text("short".to_string())],
            span: (0, 5),
        };
        assert_eq!(estimate_block_height(&block, "", Some(80)), 1);
    }

    #[test]
    fn estimate_code_block_height() {
        let block = Block::Code {
            language: Some("rust".to_string()),
            code: "fn main() {\n    println!(\"hi\");\n}".to_string(),
            span: (0, 30),
        };
        // 3 lines of code + 2 (language label + padding)
        assert_eq!(estimate_block_height(&block, "", Some(80)), 5);
    }

    #[test]
    fn estimate_table_height() {
        let block = Block::Table {
            headers: vec![TableCell {
                content: vec![Inline::Text("A".to_string())],
            }],
            alignments: vec![Alignment::Left],
            rows: vec![
                vec![TableCell {
                    content: vec![Inline::Text("1".to_string())],
                }],
                vec![TableCell {
                    content: vec![Inline::Text("2".to_string())],
                }],
            ],
            span: (0, 20),
        };
        // header + 2 rows = 3
        assert_eq!(estimate_block_height(&block, "", Some(80)), 3);
    }

    #[test]
    fn estimate_list_height() {
        let block = Block::List {
            ordered: false,
            start: None,
            items: vec![
                ListItem {
                    checked: None,
                    content: vec![Block::Paragraph {
                        content: vec![],
                        span: (0, 5),
                    }],
                },
                ListItem {
                    checked: None,
                    content: vec![Block::Paragraph {
                        content: vec![],
                        span: (6, 10),
                    }],
                },
            ],
            span: (0, 10),
        };
        assert_eq!(estimate_block_height(&block, "", Some(80)), 2);
    }

    #[test]
    fn estimate_thematic_break() {
        let block = Block::ThematicBreak { span: (0, 3) };
        assert_eq!(estimate_block_height(&block, "", Some(80)), 1);
    }
}
