use iocraft::prelude::*;

use super::{EditorState, Mode};

mod cmdline;
mod insert;
mod normal;
mod search;
mod visual;

pub fn handle_key(s: &mut EditorState, code: KeyCode, ctrl: bool) -> bool {
    s.message.clear();
    match s.mode.clone() {
        Mode::Insert => insert::handle_insert(s, code, ctrl),
        Mode::Command => cmdline::handle_cmdline(s, code),
        Mode::Search { forward } => search::handle_search(s, code, forward),
        Mode::Visual => visual::handle_visual(s, code),
        Mode::Normal => normal::handle_normal(s, code, ctrl),
    }
}
