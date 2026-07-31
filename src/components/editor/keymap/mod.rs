use iocraft::prelude::*;

use super::{EditorState, Mode};

mod cmdline;
mod insert;
mod normal;
mod search;
mod visual;

/// Dispatches a key press to the appropriate mode handler.
///
/// Clears the status message, then delegates to the mode-specific handler:
/// - [`Mode::Insert`] → [`insert::handle_insert`]
/// - [`Mode::Command`] → [`cmdline::handle_cmdline`]
/// - [`Mode::Search`] → [`search::handle_search`]
/// - [`Mode::Visual`] → [`visual::handle_visual`]
/// - [`Mode::Normal`] → [`normal::handle_normal`]
///
/// Returns `true` if the editor should quit (e.g., `:q`, `ZZ`).
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
