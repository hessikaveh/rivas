use std::collections::{HashMap, VecDeque};

use arboard::Clipboard;

use super::buffer::Buffer;

#[derive(Clone)]
pub struct HistoryEntry {
    pub buffer: Buffer,
    pub row: usize,
    pub col: usize,
}

pub struct History {
    pub undo_stack: VecDeque<HistoryEntry>,
    pub redo_stack: VecDeque<HistoryEntry>,
}

impl History {
    const MAX_STACK: usize = 200;

    pub fn new() -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
        }
    }

    pub fn push(&mut self, entry: HistoryEntry) {
        self.undo_stack.push_back(entry);
        if self.undo_stack.len() > Self::MAX_STACK {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, current: HistoryEntry) -> Option<HistoryEntry> {
        let entry = self.undo_stack.pop_back()?;
        self.redo_stack.push_back(current);
        Some(entry)
    }

    pub fn redo(&mut self, current: HistoryEntry) -> Option<HistoryEntry> {
        let entry = self.redo_stack.pop_back()?;
        self.undo_stack.push_back(current);
        Some(entry)
    }

    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

pub struct Registers {
    pub map: HashMap<char, String>,
    pub clipboard: Option<Clipboard>,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            clipboard: Clipboard::new().ok(),
        }
    }

    pub fn yank(&mut self, reg: char, text: String) {
        self.map.insert(reg, text.clone());
        self.map.insert('"', text.clone());

        if reg == '"' {
            if let Some(cb) = self.clipboard.as_mut() {
                let _ = cb.set_text(text);
            }
        }
    }

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
