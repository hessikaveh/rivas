use iocraft::prelude::*;

use super::super::{EditorState, Mode};

/// Handles key presses in Command-line mode (`:`).
///
/// Characters are appended to the command buffer (`cmd_buf`).
/// Enter executes the command via [`EditorState::execute_command`].
/// Backspace on an empty buffer cancels the command. Esc cancels without executing.
///
/// Returns `true` only if the executed command signals a quit.
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
