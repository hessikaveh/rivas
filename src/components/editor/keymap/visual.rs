use iocraft::prelude::*;

use super::super::{EditorState, Mode};

/// Handles key presses in Visual mode.
///
/// Motions extend the visual selection. `d`/`x` deletes the selection,
/// `y` yanks it, `c` changes it (deletes and enters Insert mode).
/// `Esc` or `v` returns to Normal mode.
///
/// Always returns `false` (visual mode never triggers a quit).
pub fn handle_visual(s: &mut EditorState, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc | KeyCode::Char('v') => {
            s.mode = Mode::Normal;
        }
        KeyCode::Char('d') | KeyCode::Char('x') => {
            let d = s.visual_start;
            s.execute_operator('d', d, super::super::mode::MotionType::Inclusive, '"');
            s.mode = Mode::Normal;
        }
        KeyCode::Char('y') => {
            let d = s.visual_start;
            s.execute_operator('y', d, super::super::mode::MotionType::Inclusive, '"');
            s.mode = Mode::Normal;
        }
        KeyCode::Char('c') => {
            let d = s.visual_start;
            s.execute_operator('c', d, super::super::mode::MotionType::Inclusive, '"');
        }
        key => {
            if let Some(dest) = motion_from_key(s, key) {
                s.row = dest.0;
                s.col = dest.1;
                s.col_want = s.col;
                s.needs_rerender = true;
            }
        }
    }
    false
}

pub fn motion_from_key(s: &EditorState, key: KeyCode) -> Option<(usize, usize)> {
    match key {
        KeyCode::Char(c) => s.apply_motion(c, None),
        KeyCode::Left => s.apply_motion('h', None),
        KeyCode::Right => s.apply_motion('l', None),
        KeyCode::Up => s.apply_motion('k', None),
        KeyCode::Down => s.apply_motion('j', None),
        _ => None,
    }
}
