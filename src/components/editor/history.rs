use std::collections::{HashMap, VecDeque};

use arboard::Clipboard;

use super::buffer::Buffer;

/// A snapshot of the editor state at a point in time, used for undo/redo.
///
/// Each entry captures the complete buffer content and cursor position so that
/// undo/redo can restore the editor to an exact previous state.
#[derive(Clone)]
pub struct HistoryEntry {
    /// The full buffer content at the time this snapshot was taken.
    pub buffer: Buffer,
    /// Cursor row (0-indexed line number).
    pub row: usize,
    /// Cursor column (0-indexed character index within the line).
    pub col: usize,
}

/// Manages the undo/redo history for the editor.
///
/// Maintains two stacks: one for undoable states and one for redoable states.
/// The stack is capped at [`History::MAX_STACK`] entries to bound memory usage.
/// Performing a new edit clears the redo stack.
pub struct History {
    /// States that can be restored via undo, ordered oldest-first.
    pub undo_stack: VecDeque<HistoryEntry>,
    /// States that were undone and can be restored via redo, ordered most-recently-undone first.
    pub redo_stack: VecDeque<HistoryEntry>,
}

impl History {
    /// Maximum number of undo snapshots retained.
    const MAX_STACK: usize = 200;

    /// Creates an empty history with no entries.
    pub fn new() -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
        }
    }

    /// Records a new snapshot, pushing it onto the undo stack.
    ///
    /// Evicts the oldest entry if the stack exceeds [`MAX_STACK`](History::MAX_STACK).
    /// Clears the redo stack, since a new edit invalidates any redo path.
    pub fn push(&mut self, entry: HistoryEntry) {
        self.undo_stack.push_back(entry);
        if self.undo_stack.len() > Self::MAX_STACK {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
    }

    /// Pops the most recent undo entry, pushing the `current` state onto the redo stack.
    ///
    /// Returns `None` if the undo stack is empty (nothing to undo).
    pub fn undo(&mut self, current: HistoryEntry) -> Option<HistoryEntry> {
        let entry = self.undo_stack.pop_back()?;
        self.redo_stack.push_back(current);
        Some(entry)
    }

    /// Pops the most recent redo entry, pushing the `current` state onto the undo stack.
    ///
    /// Returns `None` if the redo stack is empty (nothing to redo).
    pub fn redo(&mut self, current: HistoryEntry) -> Option<HistoryEntry> {
        let entry = self.redo_stack.pop_back()?;
        self.undo_stack.push_back(current);
        Some(entry)
    }

    /// Returns `true` if there are entries available to undo.
    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if there are entries available to redo.
    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

/// Named registers for yank/paste operations, analogous to Vim's `"` register.
///
/// Yanking stores text in both the specified register and the default `"` register.
/// Pasting from `"` also attempts to read from the system clipboard.
pub struct Registers {
    /// Map of register characters to their stored text content.
    pub map: HashMap<char, String>,
    /// Optional handle to the system clipboard for cross-application paste.
    pub clipboard: Option<Clipboard>,
}

impl Registers {
    /// Creates empty registers with a system clipboard handle (if available).
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            clipboard: Clipboard::new().ok(),
        }
    }

    /// Stores `text` in the given register and the default `"` register.
    ///
    /// If `reg` is `"`, the text is also written to the system clipboard.
    pub fn yank(&mut self, reg: char, text: String) {
        self.map.insert(reg, text.clone());
        self.map.insert('"', text.clone());

        if reg == '"' {
            if let Some(cb) = self.clipboard.as_mut() {
                let _ = cb.set_text(text);
            }
        }
    }

    /// Resolves the text content for a register.
    ///
    /// For the default `"` register, prefers the system clipboard if it contains
    /// non-empty text. Falls back to the in-memory map.
    pub fn resolve_paste_text(&mut self, reg: char) -> String {
        if reg == '"' {
            if let Some(cb) = self.clipboard.as_mut() {
                if let Ok(text) = cb.get_text() {
                    if !text.is_empty() {
                        return text;
                    }
                }
            }
        }
        self.map.get(&reg).cloned().unwrap_or_default()
    }
}
