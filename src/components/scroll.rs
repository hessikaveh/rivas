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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn delta_absolute_top() {
        assert_eq!(scroll_delta(ScrollIntent::ToTop, 24), ScrollDelta::Absolute(0));
    }

    #[test]
    fn delta_to_end() {
        assert_eq!(scroll_delta(ScrollIntent::ToBottom, 24), ScrollDelta::ToEnd);
    }

    #[test]
    fn delta_page_down() {
        assert_eq!(scroll_delta(ScrollIntent::PageDown, 24), ScrollDelta::Relative(24));
    }

    #[test]
    fn delta_half_page_down() {
        assert_eq!(scroll_delta(ScrollIntent::HalfPageDown, 24), ScrollDelta::Relative(12));
    }

    #[test]
    fn delta_page_up() {
        assert_eq!(scroll_delta(ScrollIntent::PageUp, 24), ScrollDelta::Relative(-24));
    }
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
pub struct ScrollPosition {
    baseline: i32,
    baseline_scroll: i32,
    last_rect_y: i32,
}

impl ScrollPosition {
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

    /// Returns the current baseline value.
    pub fn baseline(&self) -> i32 {
        self.baseline
    }

    /// Returns the scroll offset captured at the time the baseline was recorded.
    pub fn captured_scroll_offset(&self) -> i32 {
        self.baseline_scroll
    }
}

/// Computes the visible block range for virtual scrolling.
///
/// Given cumulative block heights, the current scroll offset, viewport
/// height, and buffer size, returns `(first_visible, last_visible)`.
/// Off-screen blocks are replaced with spacer Views of the same estimated
/// height so that ScrollView's measured ` content_height` stays accurate.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_position_starts_at_zero() {
        let sp = ScrollPosition::new();
        assert_eq!(sp.y(0), 0);
    }

    #[test]
    fn scroll_position_baseline_is_set_on_rect_move() {
        let mut sp = ScrollPosition::new();
        sp.update(10, 5);
        assert_eq!(sp.baseline(), 15);
        assert_eq!(sp.y(5), 10);
        assert_eq!(sp.captured_scroll_offset(), 5);
    }

    #[test]
    fn scroll_position_y_correct_when_scroll_changes_rect_stale() {
        let mut sp = ScrollPosition::new();
        sp.update(10, 5);
        // rect hasn't moved, but scroll changed — y should still be correct
        assert_eq!(sp.y(15), 0); // baseline(15) - scroll_offset(15) = 0
    }

    #[test]
    fn scroll_position_y_correct_on_new_rect() {
        let mut sp = ScrollPosition::new();
        sp.update(10, 5);
        sp.update(20, 15);
        assert_eq!(sp.baseline(), 35);
        assert_eq!(sp.y(15), 20);
    }

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
}