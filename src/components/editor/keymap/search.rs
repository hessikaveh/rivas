use iocraft::prelude::*;

use super::super::{EditorState, Mode};

pub fn handle_search(s: &mut EditorState, code: KeyCode, forward: bool) -> bool {
    match code {
        KeyCode::Esc => {
            s.mode = Mode::Normal;
            s.cmd_buf.clear();
        }
        KeyCode::Enter => {
            s.last_search = s.cmd_buf.clone();
            s.search_forward = forward;
            s.cmd_buf.clear();
            s.mode = Mode::Normal;
            s.needs_rerender = true;
            do_search(s, forward);
        }
        KeyCode::Backspace => {
            s.cmd_buf.pop();
        }
        KeyCode::Char(c) => {
            s.cmd_buf.push(c);
        }
        _ => {}
    }
    false
}

pub(crate) fn do_search(s: &mut EditorState, forward: bool) {
    if s.last_search.is_empty() {
        return;
    }
    let res = if forward {
        s.buf.search_forward(&s.last_search, s.row, s.col)
    } else {
        s.buf.search_backward(&s.last_search, s.row, s.col)
    };
    match res {
        Some((r, c)) => {
            s.row = r;
            s.col = c;
        }
        None => {
            s.message = format!("Pattern not found: {}", s.last_search);
        }
    }
}
