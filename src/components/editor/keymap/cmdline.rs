use iocraft::prelude::*;

use super::super::{EditorState, Mode};

pub fn handle_cmdline(s: &mut EditorState, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc => {
            s.mode = Mode::Normal;
            s.cmd_buf.clear();
            false
        }
        KeyCode::Enter => s.execute_command(),
        KeyCode::Backspace => {
            if s.cmd_buf.is_empty() {
                s.mode = Mode::Normal;
            } else {
                s.cmd_buf.pop();
            }
            false
        }
        KeyCode::Char(c) => {
            s.cmd_buf.push(c);
            false
        }
        _ => false,
    }
}
