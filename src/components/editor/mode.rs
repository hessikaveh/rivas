use iocraft::prelude::*;

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionType {
    Inclusive,
    Exclusive,
    Line,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    Command,
    Search {
        forward: bool,
    },
}

impl Mode {
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

    pub fn color(&self) -> Color {
        match self {
            Mode::Normal => theme::BLUE,
            Mode::Insert => theme::GREEN,
            Mode::Visual => theme::MAGENTA,
            Mode::Command | Mode::Search { .. } => theme::YELLOW,
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
