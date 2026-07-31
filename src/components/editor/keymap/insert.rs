use iocraft::prelude::*;

use super::super::{EditorState, Mode};

/// Handles key presses in Insert mode.
///
/// Typed characters are inserted into the buffer at the cursor position.
/// Supports Enter (line split), Backspace (delete backward / join lines),
/// Delete (delete forward), and arrow keys for cursor movement.
/// Esc or Ctrl-C exits to Normal mode (cursor moves left one position).
///
/// Always returns `false` (insert mode never triggers a quit).
pub fn handle_insert(s: &mut EditorState, code: KeyCode, ctrl: bool) -> bool {
    match code {
        KeyCode::Esc => {
            s.mode = Mode::Normal;
            s.col = s.col.saturating_sub(1);
            s.clamp();
            s.col_want = s.col;
        }
        KeyCode::Char('c') if ctrl => {
            s.mode = Mode::Normal;
            s.col = s.col.saturating_sub(1);
            s.clamp();
            s.col_want = s.col;
        }
        KeyCode::Char(c) if !ctrl => {
            s.buf.insert_char(s.row, s.col, c);
            s.col += 1;
            s.modified = true;
        }
        KeyCode::Enter => {
            s.buf.split_line(s.row, s.col);
            s.row += 1;
            s.col = 0;
            s.modified = true;
        }
        KeyCode::Backspace => {
            if s.col > 0 {
                s.col -= 1;
                s.buf.delete_char(s.row, s.col);
                s.modified = true;
            } else if s.row > 0 {
                let prev = s.buf.char_count(s.row - 1);
                s.buf.join_lines(s.row - 1);
                s.row -= 1;
                s.col = prev;
                s.modified = true;
            }
        }
        KeyCode::Delete => {
            if s.col < s.buf.char_count(s.row) {
                s.buf.delete_char(s.row, s.col);
                s.modified = true;
            } else if s.row + 1 < s.buf.line_count() {
                s.buf.join_lines(s.row);
                s.modified = true;
            }
        }
        KeyCode::Left => {
            if s.col > 0 {
                s.col -= 1;
            }
        }
        KeyCode::Right => {
            let l = s.buf.char_count(s.row);
            if s.col < l {
                s.col += 1;
            }
        }
        KeyCode::Up => {
            if s.row > 0 {
                s.row -= 1;
                s.clamp();
            }
        }
        KeyCode::Down => {
            if s.row + 1 < s.buf.line_count() {
                s.row += 1;
                s.clamp();
            }
        }
        KeyCode::Home => {
            s.col = 0;
        }
        KeyCode::End => {
            s.col = s.buf.char_count(s.row);
        }
        _ => {}
    }
    false
}
