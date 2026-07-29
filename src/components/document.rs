use std::path::PathBuf;

use iocraft::prelude::*;

use crate::components::blocks_renderer::BlocksRenderer;
use crate::components::cursor_info::CursorInfo;
use crate::components::editor::{Buffer, EditorState, Mode, handle_key};
use crate::components::scroll::{
    ScrollDelta, ScrollIntent, Viewport, intent_from_key, scroll_delta,
};
use crate::debug;
use crate::document::cache::ParseCache;
use crate::document::parser::parse_markdown;
use crate::theme;

#[derive(Default, Props)]
pub struct DocumentProps {
    pub content: String,
    pub file_path: PathBuf,
    pub viewport_height: Option<u32>,
    pub viewport_width: Option<u32>,
    pub keyboard_navigation: Option<bool>,
    pub follow_ref: Option<Ref<usize>>,
    pub cursor_offset: Option<Ref<usize>>,
    pub debug: bool,
    pub debug_annotations: bool,
    pub on_change: Handler<String>,
    pub on_quit: Handler<()>,
}

#[component]
pub fn Document(props: &DocumentProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let content_prop = props.content.clone();

    let editor_state = hooks.use_ref(|| {
        Some(EditorState::new(
            props.file_path.to_string_lossy().to_string(),
            &content_prop,
        ))
    });

    // Use the editor_state for rendering status and passing to blocks_renderer
    let state_guard = editor_state.read();
    let current_mode = state_guard
        .as_ref()
        .map(|s| s.mode.clone())
        .unwrap_or(Mode::Normal);
    let current_msg = state_guard
        .as_ref()
        .map(|s| s.message.clone())
        .unwrap_or_default();
    let current_cmd = state_guard
        .as_ref()
        .map(|s| s.cmd_buf.clone())
        .unwrap_or_default();
    let cursor_row_col = state_guard.as_ref().and_then(|s| Some((s.row, s.col)));
    let cursor_line_preview = state_guard.as_ref().and_then(|s| {
        let (row, col) = (s.row, s.col);
        let line = s.buf.line(row);
        let byte = s.buf.byte_offset(row, col);
        let before = &line[..byte.min(line.len())];
        let (cursor_ch, after) = if col < s.buf.char_count(row) {
            let c = line[byte..].chars().next()?;
            let char_len = c.len_utf8();
            let after_byte = (byte + char_len).min(line.len());
            (c.to_string(), line[after_byte..].to_string())
        } else {
            (" ".to_string(), String::new())
        };
        Some((before.to_string(), cursor_ch, after))
    });
    drop(state_guard);

    // To trigger re-renders on edit
    let tick = hooks.use_state(|| 0u64);

    // Keep editor state in sync with content prop if it changes externally
    hooks.use_effect(
        {
            let mut editor_state = editor_state.clone();
            let content = content_prop.clone();
            move || {
                if let Some(s) = editor_state.write().as_mut() {
                    if s.buf.to_text() != content {
                        s.buf = Buffer::new(&content);
                    }
                }
            }
        },
        content_prop.clone(),
    );

    // Use parse cache to memoize markdown parsing
    let cache = hooks.use_ref(|| ParseCache::new());
    let current_content = editor_state
        .read()
        .as_ref()
        .map(|s| s.buf.to_text())
        .unwrap_or_default();
    let doc = if let Some(cached_doc) = cache.read().get(&current_content) {
        cached_doc
    } else {
        let parsed = parse_markdown(&current_content);
        cache.read().insert(&current_content, parsed.clone());
        parsed
    };

    let vh = props.viewport_height;
    let vw = props.viewport_width;
    let _keyboard_navigation = props.keyboard_navigation.unwrap_or(true);
    let scroll_handle = hooks.use_ref_default::<ScrollViewHandle>();
    let mut pending_g = hooks.use_state(|| false);
    // `stick_to_bottom` is set when the user presses `G`/`End`. Because the
    // document's *measured* content height can still grow after that press
    // (async Kitty image/graphic loads increase `ScrollView`'s measured
    // height), a single `scroll_to_bottom()` can land short of the true end.
    // We re-pin to the bottom on every frame until the viewport is actually at
    // the measured bottom, then clear the intent.
    let mut stick_to_bottom = hooks.use_state(|| false);
    let _follow_ref = props.follow_ref;
    let on_change = props.on_change.clone();
    let on_quit = props.on_quit.clone();

    // Re-pin to the bottom while `stick_to_bottom` is set. Runs every render so
    // that once the (async, image-loaded) measured content height grows, the
    // viewport catches up to the true end instead of stopping short. When the
    // viewport is already at the measured bottom the intent is cleared.
    hooks.use_effect(
        {
            let mut scroll_handle = scroll_handle.clone();
            let mut stick_to_bottom = stick_to_bottom.clone();
            move || {
                if !stick_to_bottom.get() {
                    return;
                }
                // Use the measured content height. With virtualization disabled,
                // `ScrollView` always measures the full document, so this is the
                // true total and `scroll_to_bottom` lands exactly at the end even
                // as async graphic loads grow the content.
                let content_h = scroll_handle.read().content_height() as i32;
                let vph = scroll_handle.read().viewport_height() as i32;
                let off = scroll_handle.read().scroll_offset();
                let target = (content_h - vph).max(0);
                let at_bottom = off >= target;
                debug::log_event(&debug::DebugEvent::StickBottom {
                    ts: debug::elapsed_ms(),
                    active: stick_to_bottom.get(),
                    content_h,
                    off,
                    target,
                    repin: !at_bottom,
                });
                if at_bottom {
                    stick_to_bottom.set(false);
                } else {
                    scroll_handle.write().scroll_to_bottom();
                }
            }
        },
        (scroll_handle.read().scroll_offset(),),
    );

    hooks.use_terminal_events({
        let mut scroll_handle = scroll_handle;
        let _content = current_content.clone();
        let cursor_offset = props.cursor_offset.clone();
        let mut editor_state = editor_state.clone();
        let mut tick = tick.clone();
        let on_change = on_change.clone();
        let on_quit = on_quit.clone();
        move |event| {
            let TerminalEvent::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) = event
            else {
                return;
            };

            if kind == KeyEventKind::Release {
                return;
            }

            let ctrl = modifiers.contains(KeyModifiers::CONTROL);

            // Handle editing
            let mut quit = false;
            let mut changed = false;
            let mut rerender = false;
            if let Some(s) = editor_state.write().as_mut() {
                let before = s.buf.to_text();
                s.view_height = vh.unwrap_or(20) as usize;
                s.view_width = vw.unwrap_or(80) as usize;

                if handle_key(s, code, ctrl) {
                    quit = true;
                }
                let after = s.buf.to_text();
                if before != after {
                    changed = true;
                    on_change(after);
                }
                if s.needs_rerender {
                    rerender = true;
                    s.needs_rerender = false;
                }

                // Update global cursor offset
                if let Some(mut off_ref) = cursor_offset.clone() {
                    off_ref.set(s.absolute_byte_offset());
                }
            }

            if quit {
                on_quit(());
                return;
            }

            if changed || rerender {
                tick.set(tick.get().wrapping_add(1));
            }

            // Scroll and Navigation logic (Normal and Visual modes only)
            let current_mode = editor_state
                .read()
                .as_ref()
                .map(|s| s.mode.clone())
                .unwrap_or(Mode::Normal);
            let is_editing_mode = matches!(
                current_mode,
                Mode::Insert | Mode::Command | Mode::Search { .. }
            );
            let (intent, new_pending_g) =
                intent_from_key(code, ctrl, pending_g.get(), is_editing_mode);
            pending_g.set(new_pending_g);

            let old_scroll = scroll_handle.read().scroll_offset();

            if let ScrollIntent::None = intent { /* no-op */
            } else {
                let viewport_height = scroll_handle.read().viewport_height() as i32;
                let delta = scroll_delta(intent, viewport_height);
                match delta {
                    ScrollDelta::Absolute(offset) => {
                        scroll_handle.write().scroll_to(offset);
                    }
                    ScrollDelta::Relative(d) => {
                        scroll_handle.write().scroll_by(d);
                    }
                    ScrollDelta::ToEnd => {
                        scroll_handle.write().scroll_to_bottom();
                        stick_to_bottom.set(true);
                    }
                    ScrollDelta::None => {}
                }
            }

            let new_scroll = scroll_handle.read().scroll_offset();
            if old_scroll != new_scroll {
                debug::log_event(&debug::DebugEvent::Scroll {
                    ts: debug::elapsed_ms(),
                    old: old_scroll,
                    new: new_scroll,
                });
            }
        }
    });

    let file_path = props.file_path.clone();

    element! {
    View(width: vw.unwrap_or(100), height: vh.unwrap_or(100), flex_direction: FlexDirection::Column, background_color: theme::BG) {
        View(flex_grow: 1.0, border_style: BorderStyle::Single, border_color: theme::BORDER){
                ScrollView(
                    handle: Some(scroll_handle),
                    keyboard_scroll: Some(false),
                    scrollbar_thumb_color: Some(theme::FG),
                    scrollbar_track_color: Some(theme::DARK_BG),
                ) {
                    View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2, padding_top: 1, padding_bottom: 1) {
                    BlocksRenderer(
                        blocks: doc.blocks,
                        content: current_content,
                        file_path: file_path,
                        viewport: Some(Viewport::new(vh, vw, Some(scroll_handle.read().scroll_offset()))),
                        cursor_offset: props.cursor_offset.clone(),
                        editor_state: Some(editor_state.clone()),
                        scroll_handle: Some(scroll_handle.clone()),
                        debug: props.debug,
                        debug_annotations: props.debug_annotations,
                    )
                    }
                }
            }
            View(width: 100pct, height: 1, background_color: theme::STATUS_BG, flex_direction: FlexDirection::Row) {
                View(background_color: current_mode.color(), padding_left: 1, padding_right: 1) {
                    Text(content: format!(" {} ", current_mode.label()), color: theme::DARK_BG, weight: Weight::Bold)
                }
                View(flex_grow: 1.0, padding_left: 1) {
                    #(if current_mode == Mode::Command {
                        Some(element! {
                            Text(content: format!(":{}", current_cmd), color: theme::FG)
                        })
                    } else if let Mode::Search { .. } = current_mode {
                        Some(element! {
                            Text(content: current_cmd.clone(), color: theme::FG)
                        })
                    } else {
                        Some(element! {
                            Text(content: current_msg.clone(), color: theme::FG)
                        })
                    }.into_iter())
                }
                #(if let Some((row, col)) = cursor_row_col {
                    if !matches!(current_mode, Mode::Command | Mode::Search { .. }) {
                        let (before_win, cursor_ch, after_win) = cursor_line_preview
                            .unwrap_or_default();
                        let total = vw.unwrap_or(80).saturating_sub(theme::TOTAL_VIEWPORT_OFFSET + 12) as usize;
                        Some(element! {
                            View(padding_right: 1) {
                                CursorInfo(
                                    row: row,
                                    col: col,
                                    before: before_win,
                                    cursor_char: cursor_ch,
                                    after: after_win,
                                    prefix_color: Some(theme::COMMENT),
                                    budget: Some(total),
                                )
                            }
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }.into_iter())
            }
        }
    }
}
