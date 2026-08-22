use iocraft::prelude::*;

use crate::theme;

/// Determines how an operator (delete, change, yank) selects text relative to the motion.
///
/// Used by [`EditorState::execute_operator`](crate::components::editor::EditorState::execute_operator)
/// to decide which characters are included in the affected range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionType {
    /// The motion target character is **included** in the range (e.g., `dfx` includes `x`).
    Inclusive,
    /// The motion target character is **excluded** from the range (e.g., `dw` stops before the next word).
    Exclusive,
    /// The entire line(s) are affected, regardless of column positions (e.g., `dd`, `yy`).
    Line,
}

/// The editor's current input mode, analogous to Vim modes.
///
/// Determines how key presses are interpreted. The mode is displayed in the
/// status bar and controls cursor rendering behavior.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Mode {
    /// Default mode. Keys are interpreted as commands/motions (e.g., `hjkl`, `dw`, `dd`).
    #[default]
    Normal,
    /// Text is inserted at the cursor position. Typed characters become buffer content.
    Insert,
    /// Character-wise selection mode. Motions extend the selection range.
    Visual,
    /// Command-line mode (`:` prefix). Typed text is interpreted as an ex-command.
    Command,
    /// Incremental search mode. Stores the search direction.
    Search {
        /// `true` for forward search (`/`), `false` for backward search (`?`).
        forward: bool,
    },
}

impl Mode {
    /// Returns the short label displayed in the status bar (e.g., `"NORMAL"`, `"INSERT"`).
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::Command => "COMMAND",
            Mode::Search { forward: true } => "SEARCH↓",
            Mode::Search { forward: false } => "SEARCH↑",
        }
    }

    /// Returns the theme color used for the status bar indicator in this mode.
    pub fn color(&self) -> Color {
        match self {
            Mode::Normal => theme::blue(),
            Mode::Insert => theme::green(),
            Mode::Visual => theme::magenta(),
            Mode::Command | Mode::Search { .. } => theme::yellow(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_labels() {
        assert_eq!(Mode::Normal.label(), "NORMAL");
        assert_eq!(Mode::Insert.label(), "INSERT");
        assert_eq!(Mode::Visual.label(), "VISUAL");
        assert_eq!(Mode::Command.label(), "COMMAND");
        assert_eq!((Mode::Search { forward: true }).label(), "SEARCH↓");
        assert_eq!((Mode::Search { forward: false }).label(), "SEARCH↑");
    }
}
