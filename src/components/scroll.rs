use crate::document::model::{Block, inlines_to_text};
use crate::output::graphics_manager::IMAGE_HEIGHT_CACHE;
use crate::theme;
use iocraft::prelude::KeyCode;

/// Describes a user-initiated scroll action from a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollIntent {
    /// Scroll to the very top of the document.
    ToTop,
    /// Scroll to the very bottom of the document.
    ToBottom,
    /// Scroll down by one full viewport height.
    PageDown,
    /// Scroll up by one full viewport height.
    PageUp,
    /// Scroll down by half a viewport height.
    HalfPageDown,
    /// Scroll up by half a viewport height.
    HalfPageUp,
    /// No scroll action is requested.
    None,
}

/// Maps a key event to a [`ScrollIntent`]. Only processes keys when
/// the current mode is non-editing (Normal or Visual).
///
/// Also manages the `pending_g` state for the G/gg sequence: a bare
/// `g` sets `pending_g` to true, and the next `g` (without Ctrl) fires
/// `ToTop`. Any other key clears `pending_g`.
pub fn intent_from_key(
    code: KeyCode,
    ctrl: bool,
    pending_g: bool,
    is_editing_mode: bool,
) -> (ScrollIntent, bool) {
    if is_editing_mode {
        return (ScrollIntent::None, false);
    }

    match code {
        KeyCode::Char('g') if !ctrl && pending_g => (ScrollIntent::ToTop, false),
        KeyCode::Char('g') if !ctrl => (ScrollIntent::None, true),
        KeyCode::Char('G') if !ctrl => (ScrollIntent::ToBottom, false),
        KeyCode::End => (ScrollIntent::ToBottom, false),
        KeyCode::Char('d') if ctrl => (ScrollIntent::HalfPageDown, false),
        KeyCode::Char('u') if ctrl => (ScrollIntent::HalfPageUp, false),
        KeyCode::Char('f') if ctrl => (ScrollIntent::PageDown, false),
        KeyCode::PageDown => (ScrollIntent::PageDown, false),
        KeyCode::Char('b') if ctrl => (ScrollIntent::PageUp, false),
        KeyCode::PageUp => (ScrollIntent::PageUp, false),
        KeyCode::Home => (ScrollIntent::ToTop, false),
        _ => (ScrollIntent::None, false),
    }
}

/// Returns the scroll delta (in lines) for a given intent and viewport
/// size. Used by the caller to invoke `ScrollViewHandle::scroll_by` or
/// `ScrollViewHandle::scroll_to`.
pub fn scroll_delta(intent: ScrollIntent, viewport_h: i32) -> ScrollDelta {
    let page = viewport_h.max(1);
    let half_page = (page / 2).max(1);
    match intent {
        ScrollIntent::ToTop => ScrollDelta::Absolute(0),
        ScrollIntent::ToBottom => ScrollDelta::ToEnd,
        ScrollIntent::PageDown => ScrollDelta::Relative(page),
        ScrollIntent::PageUp => ScrollDelta::Relative(-page),
        ScrollIntent::HalfPageDown => ScrollDelta::Relative(half_page),
        ScrollIntent::HalfPageUp => ScrollDelta::Relative(-half_page),
        ScrollIntent::None => ScrollDelta::None,
    }
}

/// The result of resolving a [`ScrollIntent`] to an actual scroll operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDelta {
    /// No scrolling needed.
    None,
    /// Scroll to offset 0 (top).
    Absolute(i32),
    /// Scroll to a relative delta from current position.
    Relative(i32),
    /// Scroll to the bottom of the content (max offset).
    ToEnd,
}

/// Tracks the scroll-invariant baseline for a positioned element.
///
/// The invariant is: `baseline = rect.top + scroll_offset`, which stays
/// constant across scrolls because when the scroll offset changes the
/// `rect.top` (measured from the previous frame) lags by one frame.
///
/// The correct `y` position for any frame is therefore `baseline -
/// scroll_offset`, even when `use_component_rect()` reports a stale
/// rect on the frame where a scroll just happened.
/// Tracks the scroll position for a component to enable scroll-into-view behavior.
///
/// Maintains a baseline from the component's layout rect and scroll offset,
/// allowing stable position tracking across layout reflows.
pub struct ScrollPosition {
    baseline: i32,
    baseline_scroll: i32,
    last_rect_y: i32,
}

impl ScrollPosition {
    /// Creates a new scroll position tracker with uninitialized baseline.
    pub fn new() -> Self {
        Self {
            baseline: 0,
            baseline_scroll: 0,
            last_rect_y: i32::MIN,
        }
    }

    /// Update the baseline from the latest component rect and scroll
    /// offset. Must be called each frame (or when either value changes).
    pub fn update(&mut self, rect_y: i32, scroll_offset: i32) {
        if rect_y != self.last_rect_y {
            self.baseline = rect_y + scroll_offset;
            self.baseline_scroll = scroll_offset;
            self.last_rect_y = rect_y;
        }
    }

    /// Returns the correct `y` position for the current scroll offset.
    ///
    /// If the rect has moved since the last call (i.e. a new layout
    /// frame), the baseline is recomputed from `rect_y + scroll_offset`.
    /// If the rect is stale (mid-scroll frame), the previously captured
    /// baseline is used with the current scroll offset, giving the
    /// correct position without oscillation.
    pub fn y(&self, scroll_offset: i32) -> i32 {
        self.baseline - scroll_offset
    }

    /// Returns the scroll offset captured at the time the baseline was recorded.
    pub fn captured_scroll_offset(&self) -> i32 {
        self.baseline_scroll
    }
}

/// Bundles viewport dimensions and the current scroll offset for
/// consumption by scrollable components. Replaces the pattern of
/// passing `viewport_height`, `viewport_width`, and `scroll_offset`
/// Viewport dimensions and scroll state, passed through the component tree.
#[derive(Default, Clone, Debug)]
pub struct Viewport {
    /// Height of the visible area in lines. Defaults to 24 if not set.
    pub height: Option<u32>,
    /// Width of the visible area in columns. Defaults to 80 if not set.
    pub width: Option<u32>,
    /// Current scroll offset in lines (for calculating relative positions).
    pub scroll_offset: Option<i32>,
}

impl Viewport {
    /// Creates a new viewport with the given dimensions and scroll offset.
    pub fn new(height: Option<u32>, width: Option<u32>, scroll_offset: Option<i32>) -> Self {
        Self {
            height,
            width,
            scroll_offset,
        }
    }

    /// Returns the viewport height in lines, defaulting to 24.
    pub fn height(&self) -> u32 {
        self.height.unwrap_or(24)
    }

    /// Returns the viewport width in columns, defaulting to 80.
    pub fn width(&self) -> u32 {
        self.width.unwrap_or(80)
    }
}
/// Estimate the height of a block in terminal rows.
pub fn estimate_block_height(block: &Block, content: &str, vw: Option<u32>) -> u32 {
    // Content width matches what the rendered Paragraph actually wraps at
    // (viewport minus border + padding), so virtual-scroll spacers don't
    // underestimate real heights in narrow windows.
    let wrap_width = vw
        .unwrap_or(80)
        .saturating_sub(theme::CONTENT_H_INSET)
        .max(1) as usize;
    match block {
        Block::Heading { .. } => 2,
        Block::Paragraph { content, .. } => {
            let text = inlines_to_text(content);
            let chars = text.chars().count();
            ((chars as f32 / wrap_width as f32).ceil() as u32).max(1)
        }
        Block::Code { code, .. } => code.lines().count() as u32 + 2,
        Block::Math { display, .. } => {
            let cache_key = format!("math:{}:{}:{}", vw.unwrap_or(100), display, content);
            IMAGE_HEIGHT_CACHE
                .get(&cache_key)
                .map(|(_, h)| h)
                .unwrap_or(if *display { 2 } else { 1 })
        }
        Block::Mermaid { source, .. } => {
            let cache_key = format!("mermaid:{}:{}", vw.unwrap_or(100), source);
            IMAGE_HEIGHT_CACHE
                .get(&cache_key)
                .map(|(_, h)| h)
                .unwrap_or(10)
        }
        Block::Table { rows, .. } => (rows.len() + 1) as u32,
        Block::List { items, .. } => items.len() as u32,
        Block::Quote { children, .. } => children
            .iter()
            .map(|b| estimate_block_height(b, content, vw))
            .sum::<u32>()
            .max(1),
        Block::ThematicBreak { .. } => 1,
        Block::Image { url, .. } => {
            let cache_key = format!("{}:{}", vw.unwrap_or(100), url);
            IMAGE_HEIGHT_CACHE
                .get(&cache_key)
                .map(|(_, h)| h)
                .unwrap_or(5)
        }
        Block::Html { content, .. } => content.lines().count() as u32,
    }
}

/// Build cumulative block heights and byte start offsets for virtual scrolling.
/// Returns `(cumulative_heights, start_offsets)` where `cumulative_heights[i]`
/// is the total height of blocks `0..i`, and `start_offsets[i]` is the byte
/// offset of block `i` in the source content.
pub fn build_cumulative_heights(
    blocks: &[Block],
    content: &str,
    vw: Option<u32>,
) -> (Vec<u32>, Vec<usize>) {
    let mut cumulative = Vec::with_capacity(blocks.len() + 1);
    let mut starts = Vec::with_capacity(blocks.len());
    let mut total = 0u32;
    cumulative.push(0);
    for block in blocks {
        starts.push(block.span().0);
        total += estimate_block_height(block, content, vw);
        cumulative.push(total);
    }
    (cumulative, starts)
}

/// Find the block index containing a given cursor byte offset using binary
/// search on the precomputed start-offset array.
pub fn find_cursor_block(
    starts: &[usize],
    cursor_offset: Option<usize>,
    block_count: usize,
) -> usize {
    cursor_offset
        .map(|off| match starts.binary_search(&off) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        })
        .unwrap_or(0)
        .min(block_count.saturating_sub(1))
}

/// Given cumulative block heights, the current scroll offset, viewport
/// height, and buffer size, returns `(first_visible, last_visible)`.
/// Off-screen blocks are replaced with spacer Views of the same estimated
/// height so that ScrollView's measured `content_height` stays accurate.
pub fn compute_visible_range(
    scroll_offset: u32,
    viewport_h: u32,
    buffer: u32,
    heights: &[u32],
    block_count: usize,
) -> (usize, usize) {
    if block_count == 0 {
        return (0, 0);
    }
    let first_visible = heights
        .partition_point(|&h| h <= scroll_offset)
        .saturating_sub(1);
    let last_visible = heights
        .partition_point(|&h| h <= scroll_offset + viewport_h + buffer)
        .min(block_count);
    (first_visible, last_visible)
}

/// Computes the optimal scroll target to bring a block into view.
///
/// Returns `Some(target)` if the block needs scrolling, or `None` if it
/// is already fully visible. When `cursor_row` is provided and the block
/// is taller than the viewport, the target is computed from the cursor's
/// row rather than the block edges, giving precise follow-scrolling.
pub fn compute_scroll_into_view_target(
    block_top: i32,
    block_bottom: i32,
    viewport_h: i32,
    content_h: i32,
    scroll_off: i32,
    cursor_row: Option<i32>,
    bottom_offset: i32,
) -> Option<i32> {
    let top_margin = 1;
    let effective_bottom = block_bottom + bottom_offset;
    let max_offset = (content_h - viewport_h).max(0);

    let mut target = scroll_off;

    if let Some(row) = cursor_row {
        let block_h = block_bottom - block_top;
        if block_h > viewport_h {
            let cursor_content_pos = block_top + row;
            if cursor_content_pos < target + top_margin {
                target = (cursor_content_pos - top_margin).max(0);
            } else if cursor_content_pos >= target + viewport_h - 1 {
                target = (cursor_content_pos - viewport_h + 2).max(0);
            }
        } else {
            if block_top < target + top_margin {
                target = (block_top - top_margin).max(0);
            } else if effective_bottom > target + viewport_h {
                let bottom_target = (effective_bottom - viewport_h).max(0);
                if bottom_target < max_offset || target >= max_offset {
                    target = bottom_target.min(max_offset);
                }
            }
        }
    } else {
        if block_top < target + top_margin {
            target = (block_top - top_margin).max(0);
        } else if effective_bottom > target + viewport_h {
            let bottom_target = (effective_bottom - viewport_h).max(0);
            if bottom_target < max_offset || target >= max_offset {
                target = bottom_target.min(max_offset);
            }
        }
    }

    if target != scroll_off {
        Some(target)
    } else {
        None
    }
}

/// Like [`compute_visible_range`] but enforces a minimum block count
/// threshold (500) below which all blocks are rendered without
/// virtualization. Also adjusts the range to ensure the cursor block
/// is always visible.
pub fn visible_range_with_cursor(
    scroll_offset: u32,
    viewport_h: u32,
    buffer: u32,
    heights: &[u32],
    block_count: usize,
    cursor_block_idx: usize,
) -> (usize, usize) {
    if block_count == 0 {
        return (0, 0);
    }
    const VIRTUALIZE_THRESHOLD: usize = 500;
    let (first_visible, last_visible) = if block_count > VIRTUALIZE_THRESHOLD {
        compute_visible_range(scroll_offset, viewport_h, buffer, heights, block_count)
    } else {
        (0, block_count)
    };
    let first_visible = first_visible.min(cursor_block_idx);
    let last_visible = last_visible.max(cursor_block_idx + 1);
    (first_visible, last_visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ScrollPosition tests ──────────────────────────────────

    #[test]
    fn scroll_position_starts_at_zero() {
        let sp = ScrollPosition::new();
        assert_eq!(sp.y(0), 0);
    }

    #[test]
    fn scroll_position_y_correct_when_scroll_changes_rect_stale() {
        let mut sp = ScrollPosition::new();
        sp.update(10, 5);
        assert_eq!(sp.y(15), 0);
    }

    #[test]
    fn scroll_position_y_correct_on_new_rect() {
        let mut sp = ScrollPosition::new();
        sp.update(10, 5);
        sp.update(20, 15);
        assert_eq!(sp.y(15), 20);
    }

    // ── Intent tests ──────────────────────────────────────────

    #[test]
    fn intent_gg_scrolls_to_top() {
        let (intent, pending) = intent_from_key(KeyCode::Char('g'), false, true, false);
        assert_eq!(intent, ScrollIntent::ToTop);
        assert!(!pending);
    }

    #[test]
    fn intent_g_sets_pending() {
        let (intent, pending) = intent_from_key(KeyCode::Char('g'), false, false, false);
        assert_eq!(intent, ScrollIntent::None);
        assert!(pending);
    }

    #[test]
    fn intent_G_scrolls_to_bottom() {
        let (intent, pending) = intent_from_key(KeyCode::Char('G'), false, false, false);
        assert_eq!(intent, ScrollIntent::ToBottom);
        assert!(!pending);
    }

    #[test]
    fn intent_ctrl_d_scrolls_half_page() {
        let (intent, pending) = intent_from_key(KeyCode::Char('d'), true, false, false);
        assert_eq!(intent, ScrollIntent::HalfPageDown);
        assert!(!pending);
    }

    #[test]
    fn intent_ctrl_u_scrolls_half_page_up() {
        let (intent, pending) = intent_from_key(KeyCode::Char('u'), true, false, false);
        assert_eq!(intent, ScrollIntent::HalfPageUp);
        assert!(!pending);
    }

    #[test]
    fn intent_ins_mode_returns_none() {
        let (intent, pending) = intent_from_key(KeyCode::Char('G'), false, false, true);
        assert_eq!(intent, ScrollIntent::None);
        assert!(!pending);
    }

    #[test]
    fn intent_other_key_clears_pending() {
        let (_, pending) = intent_from_key(KeyCode::Char('j'), false, true, false);
        assert!(!pending);
    }

    // ── Delta tests ────────────────────────────────────────────

    #[test]
    fn delta_absolute_top() {
        assert_eq!(
            scroll_delta(ScrollIntent::ToTop, 24),
            ScrollDelta::Absolute(0)
        );
    }

    #[test]
    fn delta_to_end() {
        assert_eq!(scroll_delta(ScrollIntent::ToBottom, 24), ScrollDelta::ToEnd);
    }

    #[test]
    fn delta_page_down() {
        assert_eq!(
            scroll_delta(ScrollIntent::PageDown, 24),
            ScrollDelta::Relative(24)
        );
    }

    #[test]
    fn delta_half_page_down() {
        assert_eq!(
            scroll_delta(ScrollIntent::HalfPageDown, 24),
            ScrollDelta::Relative(12)
        );
    }

    #[test]
    fn delta_page_up() {
        assert_eq!(
            scroll_delta(ScrollIntent::PageUp, 24),
            ScrollDelta::Relative(-24)
        );
    }

    // ── compute_visible_range tests ────────────────────────────

    #[test]
    fn compute_visible_range_returns_all_when_few_blocks() {
        let heights = vec![0, 5, 10];
        let (first, last) = compute_visible_range(0, 24, 48, &heights, 2);
        assert_eq!(first, 0);
        assert_eq!(last, 2);
    }

    #[test]
    fn compute_visible_range_skips_offscreen() {
        let heights: Vec<u32> = (0..=100).collect();
        let (first, last) = compute_visible_range(20, 10, 20, &heights, 100);
        assert!(first > 0);
        assert!(last < 100);
    }

    #[test]
    fn compute_visible_range_clamps_to_block_count() {
        let heights = vec![0, 5];
        let (first, last) = compute_visible_range(0, 24, 48, &heights, 1);
        assert_eq!(first, 0);
        assert_eq!(last, 1);
    }

    #[test]
    fn visible_range_with_cursor_shows_all_below_threshold() {
        let heights: Vec<u32> = (0..=10).collect();
        let (first, last) = visible_range_with_cursor(5, 10, 20, &heights, 10, 3);
        assert_eq!(first, 0);
        assert_eq!(last, 10);
    }

    #[test]
    fn visible_range_with_cursor_ensures_cursor_visible() {
        let mut heights = Vec::with_capacity(501);
        for i in 0..=501 {
            heights.push(i);
        }
        // Scroll past the cursor block so it would be off-screen
        let (first, last) = visible_range_with_cursor(400, 10, 20, &heights, 500, 100);
        assert!(first <= 100);
        assert!(last > 100);
    }

    #[test]
    fn visible_range_with_cursor_empty_returns_zero() {
        let heights = vec![0];
        let (first, last) = visible_range_with_cursor(0, 10, 20, &heights, 0, 0);
        assert_eq!(first, 0);
        assert_eq!(last, 0);
    }

    // ── compute_scroll_into_view_target tests ─────────────────

    #[test]
    fn scroll_into_view_block_already_visible() {
        let result = compute_scroll_into_view_target(5, 15, 24, 100, 0, None, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn scroll_into_view_block_above_viewport() {
        let result = compute_scroll_into_view_target(0, 5, 10, 100, 20, None, 0);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn scroll_into_view_block_below_viewport() {
        let result = compute_scroll_into_view_target(80, 90, 24, 200, 0, None, 0);
        assert_eq!(result, Some(66));
    }

    #[test]
    fn scroll_into_view_cursor_row_tall_block() {
        // block_top=50, block_bottom=150 (100h block), viewport_h=24,
        // cursor at row 75 (content pos = 125), scrolled to 0
        let result = compute_scroll_into_view_target(50, 150, 24, 200, 0, Some(75), 0);
        // cursor_content_pos (125) >= scroll_off + viewport_h - 1 (23)
        assert_eq!(result, Some(103));
    }
}
