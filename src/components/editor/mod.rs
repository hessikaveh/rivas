pub mod buffer;
pub mod history;
pub mod keymap;
pub mod mode;

use crate::assets::math::{self, MathMode};

pub use buffer::Buffer;
pub use keymap::handle_key;
pub use mode::{Mode, MotionType};

use history::{History, HistoryEntry, Registers};

// ─────────────────────────────────────────────────────────────────────────────
// EditorState — pure logic, no iocraft types
// ─────────────────────────────────────────────────────────────────────────────

pub struct EditorState {
    pub buf: Buffer,
    pub row: usize,
    pub col: usize,
    pub col_want: usize,
    pub mode: Mode,
    pub cmd_buf: String,
    pub count_buf: String,
    pub operator: Option<char>,
    pub pending: Option<char>,
    pub last_find: Option<(char, bool)>,
    pub registers: Registers,
    pub visual_start: (usize, usize),
    pub history: History,
    pub filename: String,
    pub modified: bool,
    pub message: String,
    pub last_search: String,
    pub search_forward: bool,
    pub view_height: usize,
    pub view_width: usize,
    pub needs_rerender: bool,
}

impl EditorState {
    pub fn new(filename: String, content: &str) -> Self {
        Self {
            buf: Buffer::new(content),
            row: 0,
            col: 0,
            col_want: 0,
            mode: Mode::Normal,
            cmd_buf: String::new(),
            count_buf: String::new(),
            operator: None,
            pending: None,
            last_find: None,
            registers: Registers::new(),
            visual_start: (0, 0),
            history: History::new(),
            filename,
            modified: false,
            message: String::new(),
            last_search: String::new(),
            search_forward: true,
            view_height: 20,
            view_width: 80,
            needs_rerender: false,
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.count_buf.parse::<usize>().unwrap_or(1).max(1)
    }

    pub(crate) fn push_undo(&mut self) {
        self.history.push(HistoryEntry {
            buffer: self.buf.clone(),
            row: self.row,
            col: self.col,
        });
    }

    pub(crate) fn undo(&mut self) {
        let current = HistoryEntry {
            buffer: self.buf.clone(),
            row: self.row,
            col: self.col,
        };
        if let Some(e) = self.history.undo(current) {
            self.buf = e.buffer;
            self.row = e.row;
            self.col = e.col;
            self.modified = true;
            self.message = "Undo".into();
        } else {
            self.message = "Already at oldest change".into();
        }
    }

    pub(crate) fn redo(&mut self) {
        let current = HistoryEntry {
            buffer: self.buf.clone(),
            row: self.row,
            col: self.col,
        };
        if let Some(e) = self.history.redo(current) {
            self.buf = e.buffer;
            self.row = e.row;
            self.col = e.col;
            self.modified = true;
            self.message = "Redo".into();
        } else {
            self.message = "Already at newest change".into();
        }
    }

    pub(crate) fn clamp(&mut self) {
        let n = self.buf.line_count();
        if self.row >= n {
            self.row = n - 1;
        }
        self.col = self
            .buf
            .clamp_col(self.row, self.col, self.mode == Mode::Insert);
    }

    pub fn absolute_byte_offset(&self) -> usize {
        let mut offset = 0;
        for i in 0..self.row {
            offset += self.buf.line(i).len() + 1;
        }
        offset += self.buf.byte_offset(self.row, self.col);
        offset
    }

    pub fn absolute_byte_offset_at(&self, row: usize, col: usize) -> usize {
        let mut offset = 0;
        for i in 0..row {
            offset += self.buf.line(i).len() + 1;
        }
        offset += self.buf.byte_offset(row, col);
        offset
    }

    pub(crate) fn yank(&mut self, reg: char, text: String) {
        self.registers.yank(reg, text);
    }

    pub(crate) fn paste_after(&mut self, reg: char) {
        let text = self.registers.resolve_paste_text(reg);
        if text.is_empty() {
            return;
        }
        if text.ends_with('\n') {
            let lns: Vec<String> = text
                .trim_end_matches('\n')
                .split('\n')
                .map(|s| s.to_string())
                .collect();
            let at = self.row + 1;
            for (i, l) in lns.into_iter().enumerate() {
                self.buf.insert_line(at + i, l);
            }
            self.row = at;
            self.col = self.buf.first_non_blank(self.row);
        } else {
            let col = (self.col + 1).min(self.buf.char_count(self.row));
            let (r, c) = self.buf.insert_text(self.row, col, &text);
            self.row = r;
            self.col = c;
        }
        self.modified = true;
        self.clamp();
    }

    pub(crate) fn paste_before(&mut self, reg: char) {
        let text = self.registers.resolve_paste_text(reg);
        if text.is_empty() {
            return;
        }
        if text.ends_with('\n') {
            let lns: Vec<String> = text
                .trim_end_matches('\n')
                .split('\n')
                .map(|s| s.to_string())
                .collect();
            for (i, l) in lns.into_iter().enumerate() {
                self.buf.insert_line(self.row + i, l);
            }
            self.col = self.buf.first_non_blank(self.row);
        } else {
            let (r, c) = self.buf.insert_text(self.row, self.col, &text);
            self.row = r;
            self.col = c;
        }
        self.modified = true;
        self.clamp();
    }

    pub(crate) fn apply_motion(
        &self,
        motion: char,
        target: Option<char>,
    ) -> Option<(usize, usize)> {
        let (r, c) = (self.row, self.col);
        let nlines = self.buf.line_count();
        let count = self.count();
        match motion {
            'h' => Some((r, c.saturating_sub(count))),
            'l' => Some((r, (c + count).min(self.buf.char_count(r).saturating_sub(1)))),
            'j' => Some(((r + count).min(nlines - 1), c)),
            'k' => Some((r.saturating_sub(count), c)),
            '0' => Some((r, 0)),
            '^' => Some((r, self.buf.first_non_blank(r))),
            '$' => Some((r, self.buf.char_count(r).saturating_sub(1))),
            'w' => {
                let mut p = (r, c);
                for _ in 0..count {
                    p = self.buf.word_forward(p.0, p.1);
                }
                Some(p)
            }
            'b' => {
                let mut p = (r, c);
                for _ in 0..count {
                    p = self.buf.word_backward(p.0, p.1);
                }
                Some(p)
            }
            'e' => {
                let mut p = (r, c);
                for _ in 0..count {
                    p = self.buf.word_end(p.0, p.1);
                }
                Some(p)
            }
            'G' => {
                let dr = if self.count_buf.is_empty() {
                    nlines - 1
                } else {
                    (self.count() - 1).min(nlines - 1)
                };
                Some((dr, self.buf.first_non_blank(dr)))
            }
            '{' => {
                let mut row = r.saturating_sub(1);
                while row > 0 && !self.buf.line(row).trim().is_empty() {
                    row -= 1;
                }
                Some((row, 0))
            }
            '}' => {
                let mut row = (r + 1).min(nlines - 1);
                while row < nlines - 1 && !self.buf.line(row).trim().is_empty() {
                    row += 1;
                }
                Some((row, 0))
            }
            'f' | 't' => target.and_then(|ch| {
                self.buf
                    .find_forward(r, c, ch, motion == 't')
                    .map(|nc| (r, nc))
            }),
            'F' | 'T' => target.and_then(|ch| {
                self.buf
                    .find_backward(r, c, ch, motion == 'T')
                    .map(|nc| (r, nc))
            }),
            _ => None,
        }
    }

    pub(crate) fn execute_operator(
        &mut self,
        op: char,
        dest: (usize, usize),
        motion_type: MotionType,
        reg: char,
    ) {
        let (r1, c1, r2, c2) = if (self.row, self.col) <= dest {
            (self.row, self.col, dest.0, dest.1)
        } else {
            (dest.0, dest.1, self.row, self.col)
        };
        if op != 'y' {
            self.push_undo();
        }
        if motion_type == MotionType::Line {
            let mut yanked = String::new();
            for row in r1..=r2 {
                yanked.push_str(self.buf.line(row));
                yanked.push('\n');
            }
            self.yank(reg, yanked);
            if op != 'y' {
                self.buf.lines.drain(r1..=r2);
                if self.buf.lines.is_empty() {
                    self.buf.lines.push(String::new());
                }
                self.row = r1.min(self.buf.line_count() - 1);
                self.col = self.buf.first_non_blank(self.row);
                if op == 'c' {
                    self.buf.insert_line(self.row, String::new());
                    self.col = 0;
                    self.mode = Mode::Insert;
                } else {
                    self.clamp();
                }
            }
        } else if r1 == r2 {
            let chars: Vec<char> = self.buf.line(r1).chars().collect();
            let end = if motion_type == MotionType::Exclusive {
                c2.min(chars.len())
            } else {
                (c2 + 1).min(chars.len())
            };
            let yanked: String = chars[c1..end].iter().collect();
            self.yank(reg, yanked);
            if op != 'y' {
                self.buf.replace_range_on_line(r1, c1, end, "");
                self.col = c1.min(self.buf.char_count(r1).saturating_sub(1));
                if op == 'c' {
                    self.mode = Mode::Insert;
                }
            }
        } else {
            let mut yanked = String::new();
            let h_byte = self.buf.byte_offset(r1, c1);
            yanked.push_str(&self.buf.line(r1)[h_byte..]);
            yanked.push('\n');
            for row in (r1 + 1)..r2 {
                yanked.push_str(self.buf.line(row));
                yanked.push('\n');
            }
            let end_c2 = if motion_type == MotionType::Exclusive {
                c2
            } else {
                c2 + 1
            };
            let t_byte = self
                .buf
                .byte_offset(r2, end_c2.min(self.buf.char_count(r2)));
            yanked.push_str(&self.buf.line(r2)[..t_byte]);
            self.yank(reg, yanked);
            if op != 'y' {
                let tail = self.buf.line(r2)[t_byte..].to_string();
                let h_byte2 = self.buf.byte_offset(r1, c1);
                let head = self.buf.line(r1)[..h_byte2].to_string();

                self.buf.lines.drain(r1..=r2);
                let merged_line = format!("{}{}", head, tail);
                if self.buf.lines.is_empty() {
                    self.buf.lines.push(merged_line);
                } else {
                    self.buf.lines.insert(r1, merged_line);
                }

                self.row = r1;
                self.col = c1;
                self.clamp();
                if op == 'c' {
                    self.mode = Mode::Insert;
                }
            }
        }
        if op != 'y' {
            self.modified = true;
        }
    }

    pub(crate) fn delete_lines(&mut self, count: usize, reg: char) {
        self.push_undo();
        let mut yanked = String::new();
        for _ in 0..count {
            let s = self
                .buf
                .delete_line(self.row.min(self.buf.line_count() - 1));
            yanked.push_str(&s);
            yanked.push('\n');
        }
        if self.row >= self.buf.line_count() {
            self.row = self.buf.line_count() - 1;
        }
        self.col = self.buf.first_non_blank(self.row);
        self.yank(reg, yanked);
        self.modified = true;
    }

    pub(crate) fn yank_lines(&mut self, count: usize, reg: char) {
        let mut yanked = String::new();
        for i in 0..count {
            let r = (self.row + i).min(self.buf.line_count() - 1);
            yanked.push_str(self.buf.line(r));
            yanked.push('\n');
        }
        self.yank(reg, yanked);
        self.message = format!("{} line{} yanked", count, if count != 1 { "s" } else { "" });
    }

    pub(crate) fn execute_command(&mut self) -> bool {
        let cmd = self.cmd_buf.clone();
        self.cmd_buf.clear();
        self.mode = Mode::Normal;
        if let Ok(n) = cmd.parse::<usize>() {
            self.row = (n.saturating_sub(1)).min(self.buf.line_count() - 1);
            self.col = self.buf.first_non_blank(self.row);
            return false;
        }
        match cmd.trim() {
            "w" | "write" => {
                match std::fs::write(&self.filename, self.buf.to_text()) {
                    Ok(_) => {
                        self.modified = false;
                        self.message = format!("\"{}\" written", self.filename);
                    }
                    Err(e) => {
                        self.message = format!("E: {}", e);
                    }
                }
                false
            }
            "q" => {
                if self.modified {
                    self.message = "No write since last change (use :q! to override)".into();
                    false
                } else {
                    true
                }
            }
            "q!" => true,
            "wq" | "x" => match std::fs::write(&self.filename, self.buf.to_text()) {
                Ok(_) => true,
                Err(e) => {
                    self.message = format!("E: {}", e);
                    false
                }
            },
            "wq!" => match std::fs::write(&self.filename, self.buf.to_text()) {
                Ok(_) => true,
                Err(e) => {
                    self.message = format!("E: {}", e);
                    false
                }
            },
            "math" => {
                let new = math::toggle_math_mode();
                self.message = format!(
                    "Math mode: {}",
                    match new {
                        MathMode::Unicode => "unicode",
                        MathMode::Image => "image",
                    }
                );
                self.needs_rerender = true;
                false
            }
            "math unicode" => {
                math::set_math_mode(MathMode::Unicode);
                self.message = "Math mode: unicode".into();
                self.needs_rerender = true;
                false
            }
            "math image" => {
                math::set_math_mode(MathMode::Image);
                self.message = "Math mode: image".into();
                self.needs_rerender = true;
                false
            }
            other => {
                self.message = format!("E: Not an editor command: {}", other);
                false
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use iocraft::prelude::*;

    // ── Helpers ──────────────────────────────────────────────────────────

    fn ed(content: &str) -> EditorState {
        EditorState::new("test".to_string(), content)
    }

    fn key(s: &mut EditorState, c: char) {
        handle_key(s, KeyCode::Char(c), false);
    }

    fn ctrl(s: &mut EditorState, c: char) {
        handle_key(s, KeyCode::Char(c), true);
    }

    fn esc(s: &mut EditorState) {
        handle_key(s, KeyCode::Esc, false);
    }

    fn enter(s: &mut EditorState) {
        handle_key(s, KeyCode::Enter, false);
    }

    fn backspace(s: &mut EditorState) {
        handle_key(s, KeyCode::Backspace, false);
    }

    fn delete_key(s: &mut EditorState) {
        handle_key(s, KeyCode::Delete, false);
    }

    fn arrow(s: &mut EditorState, code: KeyCode) {
        handle_key(s, code, false);
    }

    fn keys(s: &mut EditorState, chars: &str) {
        for c in chars.chars() {
            key(s, c);
        }
    }

    // ═════════════════════════════════════════════════════════════════════
    // 2. Basic Motions h/j/k/l
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_l_moves_right() {
        let mut s = ed("hello");
        key(&mut s, 'l');
        assert_eq!(s.col, 1);
        key(&mut s, 'l');
        assert_eq!(s.col, 2);
    }

    #[test]
    fn motion_h_moves_left() {
        let mut s = ed("hello");
        s.col = 3;
        key(&mut s, 'h');
        assert_eq!(s.col, 2);
    }

    #[test]
    fn motion_h_stops_at_zero() {
        let mut s = ed("hello");
        key(&mut s, 'h');
        assert_eq!(s.col, 0);
    }

    #[test]
    fn motion_l_stops_at_end() {
        let mut s = ed("hi");
        keys(&mut s, "llll");
        assert_eq!(s.col, 1);
    }

    #[test]
    fn motion_j_moves_down() {
        let mut s = ed("aaa\nbbb\nccc");
        key(&mut s, 'j');
        assert_eq!(s.row, 1);
        key(&mut s, 'j');
        assert_eq!(s.row, 2);
    }

    #[test]
    fn motion_k_moves_up() {
        let mut s = ed("aaa\nbbb\nccc");
        s.row = 2;
        key(&mut s, 'k');
        assert_eq!(s.row, 1);
    }

    #[test]
    fn motion_j_stops_at_last_line() {
        let mut s = ed("aaa\nbbb");
        keys(&mut s, "jjj");
        assert_eq!(s.row, 1);
    }

    #[test]
    fn motion_k_stops_at_first_line() {
        let mut s = ed("aaa\nbbb");
        keys(&mut s, "kkk");
        assert_eq!(s.row, 0);
    }

    #[test]
    fn motion_j_with_count() {
        let mut s = ed("a\nb\nc\nd\ne");
        keys(&mut s, "3j");
        assert_eq!(s.row, 3);
    }

    #[test]
    fn motion_l_with_count() {
        let mut s = ed("hello world");
        keys(&mut s, "3l");
        assert_eq!(s.col, 3);
    }

    #[test]
    fn motion_j_clamps_col_to_shorter_line() {
        let mut s = ed("hello\nhi\nworld");
        s.col = 4;
        s.col_want = 4;
        key(&mut s, 'j');
        assert_eq!(s.row, 1);
        assert_eq!(s.col, 1);
    }

    #[test]
    fn motion_j_restores_col_on_longer_line() {
        let mut s = ed("hello\nhi\nworld");
        s.col = 4;
        s.col_want = 4;
        keys(&mut s, "jj");
        assert_eq!(s.row, 2);
        assert_eq!(s.col, 4);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 3. Word Motions w/b/e
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_w_basic() {
        let mut s = ed("hello world");
        key(&mut s, 'w');
        assert_eq!(s.col, 6);
    }

    #[test]
    fn motion_w_at_end_of_line_goes_to_next_line() {
        let mut s = ed("hello\nworld");
        s.col = 4;
        key(&mut s, 'w');
        assert_eq!(s.row, 1);
        assert_eq!(s.col, 0);
    }

    #[test]
    fn motion_w_over_punctuation() {
        let mut s = ed("hello.world");
        key(&mut s, 'w');
        assert_eq!(s.col, 5, "w should stop at punctuation boundary '.'");
    }

    #[test]
    fn motion_b_basic() {
        let mut s = ed("hello world");
        s.col = 8;
        key(&mut s, 'b');
        assert_eq!(s.col, 6);
    }

    #[test]
    fn motion_b_to_previous_line() {
        let mut s = ed("hello\nworld");
        s.row = 1;
        s.col = 0;
        key(&mut s, 'b');
        assert_eq!(s.row, 0);
    }

    #[test]
    fn motion_b_over_punctuation() {
        let mut s = ed("hello.world");
        s.col = 8;
        key(&mut s, 'b');
        assert_eq!(s.col, 6, "b should stop at start of word after punct");
    }

    #[test]
    fn motion_e_basic() {
        let mut s = ed("hello world");
        key(&mut s, 'e');
        assert_eq!(s.col, 4);
    }

    #[test]
    fn motion_e_over_punctuation() {
        let mut s = ed("hello.world");
        key(&mut s, 'e');
        assert_eq!(s.col, 4, "e should stop at end of word before punct");
    }

    #[test]
    fn motion_w_with_count() {
        let mut s = ed("one two three four");
        keys(&mut s, "2w");
        assert_eq!(s.col, 8);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 4. Line Motions 0/^/$
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_zero_goes_to_start() {
        let mut s = ed("hello");
        s.col = 3;
        key(&mut s, '0');
        assert_eq!(s.col, 0);
    }

    #[test]
    fn motion_caret_goes_to_first_non_blank() {
        let mut s = ed("   hello");
        key(&mut s, '^');
        assert_eq!(s.col, 3);
    }

    #[test]
    fn motion_dollar_goes_to_end() {
        let mut s = ed("hello");
        key(&mut s, '$');
        assert_eq!(s.col, 4);
    }

    #[test]
    fn motion_dollar_on_empty_line() {
        let mut s = ed("");
        key(&mut s, '$');
        assert_eq!(s.col, 0);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 5. $ Sticky Column
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn dollar_sticky_column_with_j() {
        let mut s = ed("hello\nhi\nworld");
        key(&mut s, '$');
        assert_eq!(s.col, 4);
        key(&mut s, 'j');
        assert_eq!(s.row, 1);
        assert_eq!(
            s.col, 1,
            "After $+j, cursor should be at end of shorter line"
        );
    }

    #[test]
    fn dollar_sticky_persists_through_multiple_jk() {
        let mut s = ed("hello\nhi\nworld");
        key(&mut s, '$');
        keys(&mut s, "jj");
        assert_eq!(s.row, 2);
        assert_eq!(s.col, 4, "After $+jj, cursor should be at end of 'world'");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 6. G and gg Motions
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_g_g_goes_to_first_line() {
        let mut s = ed("aaa\nbbb\nccc");
        s.row = 2;
        keys(&mut s, "gg");
        assert_eq!(s.row, 0);
    }

    #[test]
    fn motion_capital_g_goes_to_last_line() {
        let mut s = ed("aaa\nbbb\nccc");
        key(&mut s, 'G');
        assert_eq!(s.row, 2);
    }

    #[test]
    fn motion_count_g_goes_to_line_number() {
        let mut s = ed("aaa\nbbb\nccc\nddd");
        keys(&mut s, "2G");
        assert_eq!(s.row, 1);
    }

    #[test]
    fn motion_gg_with_count() {
        let mut s = ed("aaa\nbbb\nccc\nddd");
        s.row = 3;
        keys(&mut s, "2gg");
        assert_eq!(s.row, 0);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 7. Paragraph Motions { and }
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_close_brace_next_blank_line() {
        let mut s = ed("hello\nworld\n\nfoo");
        key(&mut s, '}');
        assert_eq!(s.row, 2);
    }

    #[test]
    fn motion_open_brace_prev_blank_line() {
        let mut s = ed("hello\n\nworld\nfoo");
        s.row = 3;
        key(&mut s, '{');
        assert_eq!(s.row, 1);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 8. Find Motions f/t/F/T and ;/,
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_f_finds_char_forward() {
        let mut s = ed("hello world");
        keys(&mut s, "fo");
        assert_eq!(s.col, 4);
    }

    #[test]
    fn motion_t_stops_before_char() {
        let mut s = ed("hello world");
        keys(&mut s, "to");
        assert_eq!(s.col, 3);
    }

    #[test]
    fn motion_capital_f_finds_backward() {
        let mut s = ed("hello world");
        s.col = 8;
        keys(&mut s, "Fl");
        assert_eq!(s.col, 3);
    }

    #[test]
    fn motion_capital_t_stops_after_backward() {
        let mut s = ed("hello world");
        s.col = 8;
        keys(&mut s, "Tl");
        assert_eq!(s.col, 4);
    }

    #[test]
    fn motion_semicolon_repeats_find() {
        let mut s = ed("abcabc");
        keys(&mut s, "fa");
        assert_eq!(s.col, 3);
        key(&mut s, ';');
        assert_eq!(s.col, 3);
    }

    #[test]
    fn motion_comma_reverses_find() {
        let mut s = ed("abcabc");
        s.col = 4;
        keys(&mut s, "fa");
    }

    #[test]
    fn motion_f_not_found_stays() {
        let mut s = ed("hello");
        keys(&mut s, "fz");
        assert_eq!(s.col, 0);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 9. Insert Mode Entry
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn insert_i_enters_insert_at_cursor() {
        let mut s = ed("hello");
        s.col = 2;
        key(&mut s, 'i');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.col, 2);
    }

    #[test]
    fn insert_capital_i_goes_to_first_non_blank() {
        let mut s = ed("   hello");
        s.col = 5;
        key(&mut s, 'I');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.col, 3);
    }

    #[test]
    fn insert_a_appends_after_cursor() {
        let mut s = ed("hello");
        s.col = 2;
        key(&mut s, 'a');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.col, 3);
    }

    #[test]
    fn insert_capital_a_goes_to_end() {
        let mut s = ed("hello");
        key(&mut s, 'A');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.col, 5);
    }

    #[test]
    fn insert_o_opens_line_below() {
        let mut s = ed("hello\nworld");
        key(&mut s, 'o');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.row, 1);
        assert_eq!(s.buf.line_count(), 3);
        assert_eq!(s.buf.line(1), "");
    }

    #[test]
    fn insert_capital_o_opens_line_above() {
        let mut s = ed("hello\nworld");
        s.row = 1;
        key(&mut s, 'O');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.row, 1);
        assert_eq!(s.buf.line_count(), 3);
        assert_eq!(s.buf.line(1), "");
    }

    #[test]
    fn insert_s_deletes_char_and_inserts() {
        let mut s = ed("hello");
        s.col = 1;
        key(&mut s, 's');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.buf.line(0), "hllo");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 10. Insert Mode Editing
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn insert_typing_characters() {
        let mut s = ed("");
        key(&mut s, 'i');
        handle_key(&mut s, KeyCode::Char('h'), false);
        handle_key(&mut s, KeyCode::Char('i'), false);
        assert_eq!(s.buf.line(0), "hi");
        assert_eq!(s.col, 2);
    }

    #[test]
    fn insert_enter_splits_line() {
        let mut s = ed("helloworld");
        key(&mut s, 'i');
        s.col = 5;
        enter(&mut s);
        assert_eq!(s.buf.line(0), "hello");
        assert_eq!(s.buf.line(1), "world");
        assert_eq!(s.row, 1);
        assert_eq!(s.col, 0);
    }

    #[test]
    fn insert_backspace_deletes_backward() {
        let mut s = ed("hello");
        key(&mut s, 'i');
        s.col = 3;
        backspace(&mut s);
        assert_eq!(s.buf.line(0), "helo");
        assert_eq!(s.col, 2);
    }

    #[test]
    fn insert_backspace_at_start_joins_with_prev_line() {
        let mut s = ed("hello\nworld");
        key(&mut s, 'i');
        s.row = 1;
        s.col = 0;
        backspace(&mut s);
        assert_eq!(s.buf.line_count(), 1);
        assert_eq!(s.buf.line(0), "helloworld");
        assert_eq!(s.row, 0);
        assert_eq!(s.col, 5);
    }

    #[test]
    fn insert_delete_key() {
        let mut s = ed("hello");
        key(&mut s, 'i');
        s.col = 2;
        delete_key(&mut s);
        assert_eq!(s.buf.line(0), "helo");
        assert_eq!(s.col, 2);
    }

    #[test]
    fn insert_arrow_keys() {
        let mut s = ed("hello");
        key(&mut s, 'i');
        s.col = 2;
        arrow(&mut s, KeyCode::Left);
        assert_eq!(s.col, 1);
        arrow(&mut s, KeyCode::Right);
        assert_eq!(s.col, 2);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 11. Exiting Insert Mode
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn esc_exits_insert_mode() {
        let mut s = ed("hello");
        key(&mut s, 'i');
        assert_eq!(s.mode, Mode::Insert);
        esc(&mut s);
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn esc_from_insert_moves_cursor_left() {
        let mut s = ed("hello");
        key(&mut s, 'a');
        s.col = 3;
        esc(&mut s);
        assert_eq!(s.col, 2);
    }

    #[test]
    fn ctrl_c_exits_insert_mode() {
        let mut s = ed("hello");
        key(&mut s, 'i');
        ctrl(&mut s, 'c');
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn esc_from_insert_updates_col_want() {
        let mut s = ed("hello\nworld\nfoo");
        key(&mut s, 'i');
        s.col = 3;
        esc(&mut s);
        assert_eq!(s.col, 2);
        assert_eq!(
            s.col_want, 2,
            "col_want should be updated when exiting insert mode"
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // 12. Delete x/X
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn x_deletes_char_under_cursor() {
        let mut s = ed("hello");
        s.col = 1;
        key(&mut s, 'x');
        assert_eq!(s.buf.line(0), "hllo");
    }

    #[test]
    fn x_with_count() {
        let mut s = ed("hello");
        keys(&mut s, "3x");
        assert_eq!(s.buf.line(0), "lo");
    }

    #[test]
    fn x_yanks_deleted_char() {
        let mut s = ed("hello");
        s.col = 1;
        key(&mut s, 'x');
        assert_eq!(s.registers.map.get(&'"'), Some(&"e".to_string()));
    }

    #[test]
    fn capital_x_deletes_char_before_cursor() {
        let mut s = ed("hello");
        s.col = 2;
        key(&mut s, 'X');
        assert_eq!(s.buf.line(0), "hllo");
        assert_eq!(s.col, 1);
    }

    #[test]
    fn capital_x_at_start_does_nothing() {
        let mut s = ed("hello");
        s.col = 0;
        key(&mut s, 'X');
        assert_eq!(s.buf.line(0), "hello");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 13. dd (Delete Lines)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn dd_deletes_current_line() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "dd");
        assert_eq!(s.buf.to_text(), "bbb\nccc");
    }

    #[test]
    fn dd_on_last_line() {
        let mut s = ed("aaa\nbbb");
        s.row = 1;
        keys(&mut s, "dd");
        assert_eq!(s.buf.to_text(), "aaa");
        assert_eq!(s.row, 0);
    }

    #[test]
    fn dd_on_only_line() {
        let mut s = ed("hello");
        keys(&mut s, "dd");
        assert_eq!(s.buf.to_text(), "");
        assert_eq!(s.row, 0);
    }

    #[test]
    fn dd_with_count() {
        let mut s = ed("aaa\nbbb\nccc\nddd");
        keys(&mut s, "2dd");
        assert_eq!(s.buf.to_text(), "ccc\nddd");
    }

    #[test]
    fn dd_yanks_line_with_newline() {
        let mut s = ed("aaa\nbbb");
        keys(&mut s, "dd");
        assert_eq!(s.registers.map.get(&'"'), Some(&"aaa\n".to_string()));
    }

    #[test]
    fn count_before_dd() {
        let mut s = ed("a\nb\nc\nd\ne");
        keys(&mut s, "3dd");
        assert_eq!(s.buf.to_text(), "d\ne");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 14. d{motion}
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn d_w_deletes_word() {
        let mut s = ed("hello world");
        keys(&mut s, "dw");
        assert_eq!(s.buf.line(0), "world");
    }

    #[test]
    fn d_dollar_deletes_to_end_of_line() {
        let mut s = ed("hello world");
        s.col = 5;
        keys(&mut s, "d$");
        assert_eq!(s.buf.line(0), "hello");
    }

    #[test]
    fn d_zero_deletes_to_start() {
        let mut s = ed("hello world");
        s.col = 6;
        s.col_want = 6;
        keys(&mut s, "d0");
        assert_eq!(s.buf.line(0), "world");
    }

    #[test]
    fn d_e_deletes_to_end_of_word() {
        let mut s = ed("hello world");
        keys(&mut s, "de");
        assert_eq!(s.buf.line(0), " world");
    }

    #[test]
    fn d_f_deletes_to_found_char() {
        let mut s = ed("hello world");
        keys(&mut s, "df ");
        assert_eq!(s.buf.line(0), "world");
    }

    #[test]
    fn d_gg_deletes_to_first_line() {
        let mut s = ed("aaa\nbbb\nccc");
        s.row = 2;
        s.col = 0;
        s.col_want = 0;
        keys(&mut s, "dgg");
        assert_eq!(s.buf.line_count(), 1);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 15. cc / c{motion}
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn cc_clears_line_enters_insert() {
        let mut s = ed("hello\nworld");
        keys(&mut s, "cc");
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.buf.line(0), "");
        assert_eq!(s.col, 0);
    }

    #[test]
    fn cc_with_count_should_clear_multiple_lines() {
        let mut s = ed("aaa\nbbb\nccc\nddd");
        keys(&mut s, "3cc");
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.buf.line(0), "", "cc with count should clear current line");
        assert_eq!(
            s.buf.line_count(),
            2,
            "3cc should remove 3 lines, leaving 2 (blank + ddd)"
        );
    }

    #[test]
    fn c_w_changes_word() {
        let mut s = ed("hello world");
        keys(&mut s, "cw");
        assert_eq!(s.mode, Mode::Insert);
    }

    #[test]
    fn c_dollar_changes_to_eol() {
        let mut s = ed("hello world");
        s.col = 5;
        keys(&mut s, "c$");
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.buf.line(0), "hello");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 16. yy / y{motion} and Paste
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn yy_yanks_line() {
        let mut s = ed("hello\nworld");
        keys(&mut s, "yy");
        assert_eq!(s.registers.map.get(&'"'), Some(&"hello\n".to_string()));
        assert_eq!(s.buf.to_text(), "hello\nworld");
    }

    #[test]
    fn yy_with_count() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "2yy");
        assert_eq!(s.registers.map.get(&'"'), Some(&"aaa\nbbb\n".to_string()));
    }

    #[test]
    fn y_w_yanks_word() {
        let mut s = ed("hello world");
        keys(&mut s, "yw");
        let yanked = s.registers.map.get(&'"').cloned().unwrap_or_default();
        assert!(yanked.starts_with("hello"), "yw should yank 'hello' area");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 17. p/P Paste
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn p_paste_linewise_after() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "dd");
        key(&mut s, 'p');
        assert_eq!(s.buf.line(0), "bbb");
        assert_eq!(s.buf.line(1), "aaa");
        assert_eq!(s.buf.line(2), "ccc");
    }

    #[test]
    fn capital_p_paste_linewise_before() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "dd");
        key(&mut s, 'P');
        assert_eq!(s.buf.line(0), "aaa");
        assert_eq!(s.buf.line(1), "bbb");
    }

    #[test]
    fn p_paste_charwise_after() {
        let mut s = ed("hllo");
        s.registers.map.insert('"', "e".to_string());
        s.col = 0;
        key(&mut s, 'p');
        assert_eq!(s.buf.line(0), "hello");
    }

    #[test]
    fn p_paste_with_count_single_undo() {
        let mut s = ed("hello");
        s.registers.map.insert('"', "x".to_string());
        keys(&mut s, "3p");
        assert_eq!(s.buf.line(0), "hxxxello");
        key(&mut s, 'u');
        assert_eq!(
            s.buf.line(0),
            "hello",
            "Single undo should revert all 3 pastes from 3p"
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // 18. Count with Operators
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn count_before_operator_3dw() {
        let mut s = ed("one two three four");
        keys(&mut s, "3dw");
    }

    #[test]
    fn operator_count_d3w() {
        let mut s = ed("one two three four");
        keys(&mut s, "d3w");
        assert_eq!(s.buf.line(0), "four", "d3w should delete 3 words");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 19. Undo / Redo
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn undo_reverses_delete() {
        let mut s = ed("hello");
        keys(&mut s, "dd");
        assert_eq!(s.buf.to_text(), "");
        key(&mut s, 'u');
        assert_eq!(s.buf.to_text(), "hello");
    }

    #[test]
    fn redo_reverses_undo() {
        let mut s = ed("hello");
        keys(&mut s, "dd");
        key(&mut s, 'u');
        assert_eq!(s.buf.to_text(), "hello");
        ctrl(&mut s, 'r');
        assert_eq!(s.buf.to_text(), "");
    }

    #[test]
    fn undo_with_count() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "dd");
        keys(&mut s, "dd");
        assert_eq!(s.buf.to_text(), "ccc");
        keys(&mut s, "2u");
        assert_eq!(s.buf.to_text(), "aaa\nbbb\nccc");
    }

    #[test]
    fn undo_stack_empty_shows_message() {
        let mut s = ed("hello");
        key(&mut s, 'u');
        assert_eq!(s.message, "Already at oldest change");
    }

    #[test]
    fn redo_stack_empty_shows_message() {
        let mut s = ed("hello");
        ctrl(&mut s, 'r');
        assert_eq!(s.message, "Already at newest change");
    }

    #[test]
    fn undo_clears_redo_on_new_change() {
        let mut s = ed("hello\nworld");
        keys(&mut s, "dd");
        key(&mut s, 'u');
        keys(&mut s, "dd");
        ctrl(&mut s, 'r');
        assert_eq!(s.message, "Already at newest change");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 20. Visual Mode
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn v_enters_visual_mode() {
        let mut s = ed("hello");
        key(&mut s, 'v');
        assert_eq!(s.mode, Mode::Visual);
        assert_eq!(s.visual_start, (0, 0));
    }

    #[test]
    fn visual_esc_returns_to_normal() {
        let mut s = ed("hello");
        key(&mut s, 'v');
        esc(&mut s);
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn visual_d_deletes_selection() {
        let mut s = ed("hello world");
        key(&mut s, 'v');
        keys(&mut s, "llll");
        key(&mut s, 'd');
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.buf.line(0), " world");
    }

    #[test]
    fn visual_y_yanks_selection() {
        let mut s = ed("hello world");
        key(&mut s, 'v');
        keys(&mut s, "llll");
        key(&mut s, 'y');
        assert_eq!(s.mode, Mode::Normal);
        let yanked = s.registers.map.get(&'"').cloned().unwrap_or_default();
        assert_eq!(yanked, "hello");
        assert_eq!(s.buf.to_text(), "hello world");
    }

    #[test]
    fn visual_c_changes_selection() {
        let mut s = ed("hello world");
        key(&mut s, 'v');
        keys(&mut s, "llll");
        key(&mut s, 'c');
        assert_eq!(s.mode, Mode::Insert);
        assert_eq!(s.buf.line(0), " world");
    }

    #[test]
    fn visual_motions_extend_selection() {
        let mut s = ed("hello\nworld");
        key(&mut s, 'v');
        key(&mut s, 'j');
        assert_eq!(s.row, 1);
        assert_eq!(s.mode, Mode::Visual);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 21. Replace (r)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn r_replaces_char_under_cursor() {
        let mut s = ed("hello");
        s.col = 1;
        keys(&mut s, "rx");
        assert_eq!(s.buf.line(0), "hxllo");
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn r_at_various_positions() {
        let mut s = ed("abc");
        keys(&mut s, "rX");
        assert_eq!(s.buf.line(0), "Xbc");
        s.col = 2;
        keys(&mut s, "rZ");
        assert_eq!(s.buf.line(0), "XbZ");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 22. Join Lines (J)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn join_lines_basic() {
        let mut s = ed("hello\nworld");
        key(&mut s, 'J');
        assert_eq!(
            s.buf.line(0),
            "hello world",
            "J should add a space when joining lines"
        );
    }

    #[test]
    fn join_lines_preserves_indent() {
        let mut s = ed("hello\n    world");
        key(&mut s, 'J');
        assert_eq!(
            s.buf.line(0),
            "hello world",
            "J should strip leading whitespace from joined line"
        );
    }

    #[test]
    fn join_lines_at_last_line_does_nothing() {
        let mut s = ed("hello");
        key(&mut s, 'J');
        assert_eq!(s.buf.to_text(), "hello");
    }

    #[test]
    fn join_lines_with_count() {
        let mut s = ed("a\nb\nc\nd");
        keys(&mut s, "3J");
        assert_eq!(s.buf.line_count(), 2);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 23. Toggle Case (~)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn tilde_toggles_case_lowercase() {
        let mut s = ed("hello");
        key(&mut s, '~');
        assert_eq!(s.buf.line(0), "Hello");
        assert_eq!(s.col, 1);
    }

    #[test]
    fn tilde_toggles_case_uppercase() {
        let mut s = ed("Hello");
        key(&mut s, '~');
        assert_eq!(s.buf.line(0), "hello");
    }

    #[test]
    fn tilde_multiple() {
        let mut s = ed("hello");
        keys(&mut s, "~~~~~");
        assert_eq!(s.buf.line(0), "HELLO");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 24. Indent / Dedent (>> / <<)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn indent_line() {
        let mut s = ed("hello");
        keys(&mut s, ">>");
        assert_eq!(s.buf.line(0), "    hello");
    }

    #[test]
    fn dedent_line() {
        let mut s = ed("    hello");
        keys(&mut s, "<<");
        assert_eq!(s.buf.line(0), "hello");
    }

    #[test]
    fn dedent_partial() {
        let mut s = ed("  hello");
        keys(&mut s, "<<");
        assert_eq!(s.buf.line(0), "hello");
    }

    #[test]
    fn indent_with_count() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "2>>");
        assert_eq!(s.buf.line(0), "    aaa");
        assert_eq!(s.buf.line(1), "    bbb");
        assert_eq!(s.buf.line(2), "ccc");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 25. Search
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn search_forward_basic() {
        let mut s = ed("hello world hello");
        key(&mut s, '/');
        assert_eq!(s.mode, Mode::Search { forward: true });
        for c in "world".chars() {
            handle_key(&mut s, KeyCode::Char(c), false);
        }
        enter(&mut s);
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.col, 6);
    }

    #[test]
    fn search_backward_basic() {
        let mut s = ed("hello world hello");
        s.col = 12;
        key(&mut s, '?');
        assert_eq!(s.mode, Mode::Search { forward: false });
        for c in "hello".chars() {
            handle_key(&mut s, KeyCode::Char(c), false);
        }
        enter(&mut s);
        assert_eq!(s.col, 0);
    }

    #[test]
    fn search_n_repeats_forward() {
        let mut s = ed("aaa bbb aaa bbb");
        key(&mut s, '/');
        for c in "bbb".chars() {
            handle_key(&mut s, KeyCode::Char(c), false);
        }
        enter(&mut s);
        assert_eq!(s.col, 4);
        key(&mut s, 'n');
        assert_eq!(s.col, 12);
    }

    #[test]
    fn search_capital_n_reverses_direction() {
        let mut s = ed("aaa bbb aaa bbb");
        s.col = 12;
        key(&mut s, '/');
        for c in "aaa".chars() {
            handle_key(&mut s, KeyCode::Char(c), false);
        }
        enter(&mut s);
        key(&mut s, 'N');
    }

    #[test]
    fn search_not_found_shows_message() {
        let mut s = ed("hello");
        key(&mut s, '/');
        for c in "xyz".chars() {
            handle_key(&mut s, KeyCode::Char(c), false);
        }
        enter(&mut s);
        assert!(s.message.contains("Pattern not found"));
    }

    #[test]
    fn search_esc_cancels() {
        let mut s = ed("hello");
        key(&mut s, '/');
        handle_key(&mut s, KeyCode::Char('x'), false);
        esc(&mut s);
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.col, 0);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 26. Command Mode
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn command_mode_enter() {
        let mut s = ed("hello");
        key(&mut s, ':');
        assert_eq!(s.mode, Mode::Command);
    }

    #[test]
    fn command_q_on_unmodified() {
        let mut s = ed("hello");
        key(&mut s, ':');
        handle_key(&mut s, KeyCode::Char('q'), false);
        let quit = handle_key(&mut s, KeyCode::Enter, false);
        assert!(quit);
    }

    #[test]
    fn command_q_on_modified_warns() {
        let mut s = ed("hello");
        s.modified = true;
        key(&mut s, ':');
        handle_key(&mut s, KeyCode::Char('q'), false);
        let quit = handle_key(&mut s, KeyCode::Enter, false);
        assert!(!quit);
        assert!(s.message.contains("No write since last change"));
    }

    #[test]
    fn command_q_bang_force_quits() {
        let mut s = ed("hello");
        s.modified = true;
        key(&mut s, ':');
        handle_key(&mut s, KeyCode::Char('q'), false);
        handle_key(&mut s, KeyCode::Char('!'), false);
        let quit = handle_key(&mut s, KeyCode::Enter, false);
        assert!(quit);
    }

    #[test]
    fn command_invalid_shows_error() {
        let mut s = ed("hello");
        key(&mut s, ':');
        for c in "foobar".chars() {
            handle_key(&mut s, KeyCode::Char(c), false);
        }
        handle_key(&mut s, KeyCode::Enter, false);
        assert!(s.message.contains("Not an editor command"));
    }

    #[test]
    fn command_line_number_jumps() {
        let mut s = ed("aaa\nbbb\nccc\nddd");
        key(&mut s, ':');
        handle_key(&mut s, KeyCode::Char('3'), false);
        handle_key(&mut s, KeyCode::Enter, false);
        assert_eq!(s.row, 2);
    }

    #[test]
    fn command_esc_cancels() {
        let mut s = ed("hello");
        key(&mut s, ':');
        handle_key(&mut s, KeyCode::Char('q'), false);
        esc(&mut s);
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn command_backspace_on_empty_exits() {
        let mut s = ed("hello");
        key(&mut s, ':');
        backspace(&mut s);
        assert_eq!(s.mode, Mode::Normal);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 27. Scrolling
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn ctrl_d_scrolls_half_page_down() {
        let mut s = ed(&"line\n".repeat(50));
        s.view_height = 20;
        ctrl(&mut s, 'd');
        assert_eq!(s.row, 10);
    }

    #[test]
    fn ctrl_u_scrolls_half_page_up() {
        let mut s = ed(&"line\n".repeat(50));
        s.view_height = 20;
        s.row = 20;
        ctrl(&mut s, 'u');
        assert_eq!(s.row, 10);
    }

    #[test]
    fn ctrl_f_scrolls_full_page_down() {
        let mut s = ed(&"line\n".repeat(50));
        s.view_height = 20;
        ctrl(&mut s, 'f');
        assert_eq!(s.row, 20);
    }

    #[test]
    fn ctrl_b_scrolls_full_page_up() {
        let mut s = ed(&"line\n".repeat(50));
        s.view_height = 20;
        s.row = 30;
        ctrl(&mut s, 'b');
        assert_eq!(s.row, 10);
    }

    #[test]
    fn page_down_scrolls() {
        let mut s = ed(&"line\n".repeat(50));
        s.view_height = 20;
        arrow(&mut s, KeyCode::PageDown);
        assert_eq!(s.row, 20);
    }

    #[test]
    fn page_up_scrolls() {
        let mut s = ed(&"line\n".repeat(50));
        s.view_height = 20;
        s.row = 30;
        arrow(&mut s, KeyCode::PageUp);
        assert_eq!(s.row, 10);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 28. ZZ and ZQ
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn zz_saves_and_quits() {
        let mut s = ed("hello");
        s.filename = "/tmp/rivas_test_zz_save".to_string();
        keys(&mut s, "ZZ");
        let mut s2 = ed("hello");
        s2.filename = "/tmp/rivas_test_zz_save2".to_string();
        key(&mut s2, 'Z');
        let quit = handle_key(&mut s2, KeyCode::Char('Z'), false);
        assert!(quit, "ZZ should quit");
    }

    #[test]
    fn zq_quits_without_saving() {
        let mut s = ed("hello");
        key(&mut s, 'Z');
        let quit = handle_key(&mut s, KeyCode::Char('Q'), false);
        assert!(quit, "ZQ should quit without saving");
    }

    // ═════════════════════════════════════════════════════════════════════
    // 29. Multi-line Operator Edge Cases
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn d_j_deletes_two_lines() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "dj");
        assert_eq!(s.buf.line_count(), 1);
        assert_eq!(s.buf.line(0), "ccc");
    }

    #[test]
    fn d_k_deletes_upward() {
        let mut s = ed("aaa\nbbb\nccc");
        s.row = 1;
        s.col_want = 0;
        keys(&mut s, "dk");
        assert_eq!(s.buf.line_count(), 1);
        assert_eq!(s.buf.line(0), "ccc");
    }

    #[test]
    fn d_g_deletes_to_last_line() {
        let mut s = ed("aaa\nbbb\nccc");
        keys(&mut s, "dG");
        assert_eq!(s.buf.line_count(), 1);
        assert_eq!(s.buf.line(0), "");
    }

    #[test]
    fn operator_across_all_lines_no_stray_line() {
        let mut s = ed("aaa\nbbb");
        key(&mut s, 'v');
        key(&mut s, 'j');
        keys(&mut s, "$");
        key(&mut s, 'd');
        assert_eq!(
            s.buf.line_count(),
            1,
            "Deleting all content should leave exactly 1 empty line"
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // 30. Edge Cases
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn empty_buffer_motions() {
        let mut s = ed("");
        key(&mut s, 'j');
        assert_eq!(s.row, 0);
        key(&mut s, 'l');
        assert_eq!(s.col, 0);
        key(&mut s, 'w');
        assert_eq!(s.col, 0);
    }

    #[test]
    fn single_char_buffer() {
        let mut s = ed("a");
        key(&mut s, 'l');
        assert_eq!(s.col, 0);
        key(&mut s, 'x');
        assert_eq!(s.buf.line(0), "");
    }

    #[test]
    fn insert_on_empty_buffer() {
        let mut s = ed("");
        key(&mut s, 'i');
        handle_key(&mut s, KeyCode::Char('h'), false);
        handle_key(&mut s, KeyCode::Char('i'), false);
        esc(&mut s);
        assert_eq!(s.buf.line(0), "hi");
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn initial_state() {
        let s = ed("hello\nworld");
        assert_eq!(s.row, 0);
        assert_eq!(s.col, 0);
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.modified, false);
    }

    // ═════════════════════════════════════════════════════════════════════
    // Additional Operator + Motion Combos
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn y_dollar_yanks_to_end() {
        let mut s = ed("hello world");
        s.col = 6;
        s.col_want = 6;
        keys(&mut s, "y$");
        let yanked = s.registers.map.get(&'"').cloned().unwrap_or_default();
        assert_eq!(yanked, "world");
    }

    #[test]
    fn d_caret_deletes_to_first_non_blank() {
        let mut s = ed("   hello");
        s.col = 6;
        s.col_want = 6;
        keys(&mut s, "d^");
        assert_eq!(s.buf.line(0), "   lo");
    }

    #[test]
    fn d_b_deletes_word_backward() {
        let mut s = ed("hello world");
        s.col = 6;
        s.col_want = 6;
        keys(&mut s, "db");
        assert_eq!(s.buf.line(0), "world");
    }

    // ═════════════════════════════════════════════════════════════════════
    // Insert Mode with Arrows Vertically
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn insert_up_arrow() {
        let mut s = ed("hello\nworld");
        key(&mut s, 'i');
        s.row = 1;
        s.col = 2;
        arrow(&mut s, KeyCode::Up);
        assert_eq!(s.row, 0);
        assert_eq!(s.mode, Mode::Insert);
    }

    #[test]
    fn insert_down_arrow() {
        let mut s = ed("hello\nworld");
        key(&mut s, 'i');
        arrow(&mut s, KeyCode::Down);
        assert_eq!(s.row, 1);
        assert_eq!(s.mode, Mode::Insert);
    }

    #[test]
    fn insert_home_goes_to_start() {
        let mut s = ed("hello");
        key(&mut s, 'i');
        s.col = 3;
        arrow(&mut s, KeyCode::Home);
        assert_eq!(s.col, 0);
    }

    #[test]
    fn insert_end_goes_to_end() {
        let mut s = ed("hello");
        key(&mut s, 'i');
        arrow(&mut s, KeyCode::End);
        assert_eq!(s.col, 5);
    }

    // ═════════════════════════════════════════════════════════════════════
    // Count Parsing
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn count_parsed_correctly() {
        let mut s = ed("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk");
        keys(&mut s, "5j");
        assert_eq!(s.row, 5);
    }

    #[test]
    fn count_zero_after_digit() {
        let mut s = ed(&"line\n".repeat(20));
        keys(&mut s, "10j");
        assert_eq!(s.row, 10);
    }

    #[test]
    fn zero_without_count_goes_to_col_zero() {
        let mut s = ed("hello");
        s.col = 3;
        key(&mut s, '0');
        assert_eq!(s.col, 0);
    }

    // ═════════════════════════════════════════════════════════════════════
    // Absolute Byte Offset
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn absolute_byte_offset_first_line() {
        let s = ed("hello\nworld");
        assert_eq!(s.absolute_byte_offset(), 0);
    }

    #[test]
    fn absolute_byte_offset_second_line() {
        let mut s = ed("hello\nworld");
        s.row = 1;
        s.col = 0;
        assert_eq!(s.absolute_byte_offset(), 6);
    }

    #[test]
    fn absolute_byte_offset_with_col() {
        let mut s = ed("hello\nworld");
        s.row = 1;
        s.col = 3;
        assert_eq!(s.absolute_byte_offset(), 9);
    }

    // ═════════════════════════════════════════════════════════════════════
    // 31. Multi-byte Unicode Characters
    // ═════════════════════════════════════════════════════════════════════

    // -- Buffer basics with Unicode --

    #[test]
    fn unicode_buffer_new_and_line_count() {
        let b = Buffer::new("α β γ");
        assert_eq!(b.line_count(), 1);
        assert_eq!(b.char_count(0), 5);
    }

    #[test]
    fn unicode_buffer_multiline() {
        let b = Buffer::new("中\n文\n字");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.char_count(0), 1);
    }

    #[test]
    fn unicode_buffer_to_text_roundtrip() {
        let text = "hello 🌍\nα β γ\n中文字";
        let b = Buffer::new(text);
        assert_eq!(b.to_text(), text);
    }

    #[test]
    fn unicode_byte_offset_cjk() {
        // "中" is 3 bytes, "文" is 3 bytes
        let b = Buffer::new("中文字");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 1), 3);
        assert_eq!(b.byte_offset(0, 2), 6);
        assert_eq!(b.byte_offset(0, 3), 9);
    }

    #[test]
    fn unicode_byte_offset_emoji() {
        // "🌍" is 4 bytes
        let b = Buffer::new("🌍🌎🌏");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 1), 4);
        assert_eq!(b.byte_offset(0, 2), 8);
    }

    #[test]
    fn unicode_byte_offset_latin_extended() {
        // "é" is 2 bytes, "ñ" is 2 bytes
        let b = Buffer::new("éñü");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 1), 2);
        assert_eq!(b.byte_offset(0, 2), 4);
    }

    #[test]
    fn unicode_byte_offset_mixed() {
        // "a中b" = 1 + 3 + 1 = 5 bytes, 3 chars
        let b = Buffer::new("a中b");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 1), 1); // before '中'
        assert_eq!(b.byte_offset(0, 2), 4); // after '中', before 'b'
        assert_eq!(b.byte_offset(0, 3), 5); // past end
    }

    #[test]
    fn unicode_insert_char() {
        let mut b = Buffer::new("中文字");
        b.insert_char(0, 1, '★');
        assert_eq!(b.line(0), "中★文字");
        assert_eq!(b.char_count(0), 4);
    }

    #[test]
    fn unicode_delete_char() {
        let mut b = Buffer::new("αβγ");
        let ch = b.delete_char(0, 1);
        assert_eq!(ch, Some('β'));
        assert_eq!(b.line(0), "αγ");
    }

    #[test]
    fn unicode_split_line() {
        let mut b = Buffer::new("中文字");
        b.split_line(0, 2);
        assert_eq!(b.line(0), "中文");
        assert_eq!(b.line(1), "字");
    }

    #[test]
    fn unicode_buffer_join_lines() {
        let mut b = Buffer::new("中\n文");
        b.join_lines(0);
        assert_eq!(b.line(0), "中文");
    }

    #[test]
    fn unicode_replace_range_on_line() {
        let mut b = Buffer::new("αβγδ");
        b.replace_range_on_line(0, 1, 3, "XY");
        assert_eq!(b.line(0), "αXYδ");
    }

    #[test]
    fn unicode_insert_text_single_line() {
        let mut b = Buffer::new("αγ");
        let (r, c) = b.insert_text(0, 1, "β");
        assert_eq!(b.line(0), "αβγ");
        assert_eq!(r, 0);
        // insert_text returns cursor ON last inserted char (saturating_sub(1))
        assert_eq!(c, 1);
    }

    #[test]
    fn unicode_insert_text_multiline() {
        let mut b = Buffer::new("α");
        let (_r, _c) = b.insert_text(0, 1, "\nβ\nγ");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line(0), "α");
        assert_eq!(b.line(1), "β");
        assert_eq!(b.line(2), "γ");
    }

    // -- Clamp with Unicode --

    #[test]
    fn unicode_clamp_col_normal() {
        let b = Buffer::new("αβγ");
        assert_eq!(b.clamp_col(0, 10, false), 2); // last valid char index
        assert_eq!(b.clamp_col(0, 1, false), 1);
    }

    #[test]
    fn unicode_clamp_col_insert() {
        let b = Buffer::new("αβγ");
        assert_eq!(b.clamp_col(0, 10, true), 3); // can be at len
    }

    // -- Word motions with Unicode --

    #[test]
    fn unicode_word_forward() {
        let b = Buffer::new("αβ γδ");
        let (r, c) = b.word_forward(0, 0);
        assert_eq!((r, c), (0, 3)); // skip αβ, land on γ
    }

    #[test]
    fn unicode_word_backward() {
        let b = Buffer::new("αβ γδ");
        let (r, c) = b.word_backward(0, 4);
        assert_eq!((r, c), (0, 3)); // start of "γδ"
    }

    #[test]
    fn unicode_word_end() {
        let b = Buffer::new("αβ γδ");
        let (r, c) = b.word_end(0, 0);
        assert_eq!((r, c), (0, 1)); // end of "αβ"
    }

    // -- Search with Unicode --

    #[test]
    fn unicode_search_forward() {
        let b = Buffer::new("hello 中文 world 中文 end");
        let result = b.search_forward("中文", 0, 0);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(r, 0);
        assert_eq!(c, 6); // "hello " = 6 chars, then "中文"
    }

    #[test]
    fn unicode_buffer_search_backward() {
        let b = Buffer::new("α β γ β α");
        // β at cols 2 and 6; searching backward from col 8 finds col 6
        let result = b.search_backward("β", 0, 8);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(c, 6);
    }

    #[test]
    fn unicode_find_forward() {
        let b = Buffer::new("αβγδ");
        assert_eq!(b.find_forward(0, 0, 'γ', false), Some(2));
        assert_eq!(b.find_forward(0, 0, 'γ', true), Some(1));
    }

    #[test]
    fn unicode_find_backward() {
        let b = Buffer::new("αβγδ");
        assert_eq!(b.find_backward(0, 3, 'β', false), Some(1));
    }

    // -- EditorState motions with Unicode --

    #[test]
    fn unicode_motion_l_moves_right() {
        let mut s = ed("中文字");
        key(&mut s, 'l');
        assert_eq!(s.col, 1);
        key(&mut s, 'l');
        assert_eq!(s.col, 2);
    }

    #[test]
    fn unicode_motion_h_moves_left() {
        let mut s = ed("中文字");
        s.col = 2;
        key(&mut s, 'h');
        assert_eq!(s.col, 1);
    }

    #[test]
    fn unicode_motion_dollar_goes_to_end() {
        let mut s = ed("αβγ");
        key(&mut s, '$');
        assert_eq!(s.col, 2); // last char index (3 chars, last is 2)
    }

    #[test]
    fn unicode_motion_zero_goes_to_start() {
        let mut s = ed("中文字");
        s.col = 2;
        key(&mut s, '0');
        assert_eq!(s.col, 0);
    }

    #[test]
    fn unicode_motion_caret_first_non_blank() {
        let mut s = ed("   中文");
        key(&mut s, '^');
        assert_eq!(s.col, 3);
    }

    #[test]
    fn unicode_motion_w_basic() {
        let mut s = ed("中文 abc");
        key(&mut s, 'w');
        assert_eq!(s.col, 3); // 'a' in "abc"
    }

    #[test]
    fn unicode_motion_b_basic() {
        let mut s = ed("abc 中文");
        s.col = 5;
        key(&mut s, 'b');
        assert_eq!(s.col, 4); // start of "中文"
    }

    #[test]
    fn unicode_motion_e_basic() {
        let mut s = ed("中文 abc");
        key(&mut s, 'e');
        assert_eq!(s.col, 1); // end of "中文" -> '文' at col 1
    }

    #[test]
    fn unicode_j_k_vertical() {
        let mut s = ed("中文\nαβ\nγδ");
        key(&mut s, 'j');
        assert_eq!(s.row, 1);
        assert_eq!(s.col, 0);
        key(&mut s, 'j');
        assert_eq!(s.row, 2);
        key(&mut s, 'k');
        assert_eq!(s.row, 1);
    }

    #[test]
    fn unicode_j_clamps_col() {
        let mut s = ed("中文字\nα");
        s.col = 2;
        s.col_want = 2;
        key(&mut s, 'j');
        assert_eq!(s.row, 1);
        assert_eq!(s.col, 0); // α only has 1 char
    }

    #[test]
    fn unicode_count_motion() {
        let mut s = ed("α β γ δ ε");
        keys(&mut s, "2l");
        assert_eq!(s.col, 2);
    }

    // -- Insert mode with Unicode --

    #[test]
    fn unicode_insert_char_typing() {
        let mut s = ed("");
        key(&mut s, 'i');
        handle_key(&mut s, KeyCode::Char('中'), false);
        handle_key(&mut s, KeyCode::Char('文'), false);
        assert_eq!(s.buf.line(0), "中文");
        assert_eq!(s.col, 2); // 2 chars
    }

    #[test]
    fn unicode_insert_emoji() {
        let mut s = ed("");
        key(&mut s, 'i');
        handle_key(&mut s, KeyCode::Char('🎉'), false);
        assert_eq!(s.buf.line(0), "🎉");
        assert_eq!(s.col, 1); // 1 char (even though 4 bytes)
    }

    #[test]
    fn unicode_backspace() {
        let mut s = ed("αβγ");
        key(&mut s, 'i');
        s.col = 2;
        backspace(&mut s);
        assert_eq!(s.buf.line(0), "αγ");
        assert_eq!(s.col, 1);
    }

    #[test]
    fn unicode_backspace_join_lines() {
        let mut s = ed("中文\nαβ");
        key(&mut s, 'i');
        s.row = 1;
        s.col = 0;
        backspace(&mut s);
        assert_eq!(s.buf.line_count(), 1);
        assert_eq!(s.buf.line(0), "中文αβ");
        assert_eq!(s.col, 2); // cursor after '中文'
    }

    #[test]
    fn unicode_delete_key() {
        let mut s = ed("αβγ");
        key(&mut s, 'i');
        s.col = 0;
        delete_key(&mut s);
        assert_eq!(s.buf.line(0), "βγ");
        assert_eq!(s.col, 0);
    }

    #[test]
    fn unicode_enter_splits() {
        let mut s = ed("中文");
        key(&mut s, 'i');
        s.col = 1;
        enter(&mut s);
        assert_eq!(s.buf.line(0), "中");
        assert_eq!(s.buf.line(1), "文");
        assert_eq!(s.row, 1);
    }

    #[test]
    fn unicode_insert_mode_arrows() {
        let mut s = ed("αβγ");
        key(&mut s, 'i');
        s.col = 1;
        arrow(&mut s, KeyCode::Right);
        assert_eq!(s.col, 2);
        arrow(&mut s, KeyCode::Left);
        assert_eq!(s.col, 1);
    }

    // -- Delete/paste/yank with Unicode --

    #[test]
    fn unicode_x_deletes() {
        let mut s = ed("αβγ");
        s.col = 1;
        key(&mut s, 'x');
        assert_eq!(s.buf.line(0), "αγ");
        assert_eq!(s.col, 1);
    }

    #[test]
    fn unicode_x_yanks() {
        let mut s = ed("αβγ");
        s.col = 1;
        key(&mut s, 'x');
        assert_eq!(s.registers.map.get(&'"'), Some(&"β".to_string()));
    }

    #[test]
    fn unicode_X_deletes_back() {
        let mut s = ed("αβγ");
        s.col = 1;
        key(&mut s, 'X');
        assert_eq!(s.buf.line(0), "βγ");
        assert_eq!(s.col, 0);
    }

    #[test]
    fn unicode_dd_deletes_line() {
        let mut s = ed("中文\nαβ");
        keys(&mut s, "dd");
        assert_eq!(s.buf.to_text(), "αβ");
    }

    #[test]
    fn unicode_dd_yanks() {
        let mut s = ed("中文\nαβ");
        keys(&mut s, "dd");
        assert_eq!(s.registers.map.get(&'"'), Some(&"中文\n".to_string()));
    }

    #[test]
    fn unicode_paste_after_charwise() {
        let mut s = ed("αγ");
        s.registers.map.insert('"', "β".to_string());
        s.col = 0;
        key(&mut s, 'p');
        // p pastes after cursor: α -> αβ, then γ stays
        assert_eq!(s.buf.line(0), "αβγ");
    }

    #[test]
    fn unicode_paste_before_charwise() {
        let mut s = ed("βγ");
        s.registers.map.insert('"', "α".to_string());
        s.col = 0;
        key(&mut s, 'P');
        assert_eq!(s.buf.line(0), "αβγ");
    }

    #[test]
    fn unicode_paste_linewise() {
        let mut s = ed("α\nβ");
        keys(&mut s, "dd"); // yank "α\n"
        assert_eq!(s.buf.line(0), "β");
        key(&mut s, 'p');
        assert_eq!(s.buf.line(0), "β");
        assert_eq!(s.buf.line(1), "α");
        assert_eq!(s.buf.line_count(), 2);
    }

    #[test]
    fn unicode_paste_count() {
        let mut s = ed("x");
        s.registers.map.insert('"', "中".to_string());
        s.col = 0;
        keys(&mut s, "3p");
        assert_eq!(s.buf.line(0), "x中中中");
    }

    // -- Visual mode with Unicode --

    #[test]
    fn unicode_visual_select_and_delete() {
        let mut s = ed("αβγδ");
        key(&mut s, 'v');
        keys(&mut s, "ll"); // select αβγ
        key(&mut s, 'd');
        assert_eq!(s.buf.line(0), "δ");
    }

    #[test]
    fn unicode_visual_yank() {
        let mut s = ed("中文abc");
        key(&mut s, 'v');
        keys(&mut s, "l"); // visual mode from col 0, l moves to col 1
        // selection is inclusive: cols 0..=1 = "中文"
        key(&mut s, 'y');
        let yanked = s.registers.map.get(&'"').cloned().unwrap_or_default();
        assert_eq!(yanked, "中文");
        assert_eq!(s.buf.line(0), "中文abc"); // unchanged
    }

    // -- Replace with Unicode --

    #[test]
    fn unicode_replace_char() {
        let mut s = ed("αβγ");
        s.col = 1;
        keys(&mut s, "r中");
        assert_eq!(s.buf.line(0), "α中γ");
        assert_eq!(s.mode, Mode::Normal);
    }

    // -- Case toggle with Unicode --

    #[test]
    fn unicode_tilde_greek() {
        let mut s = ed("αβγ");
        key(&mut s, '~');
        assert_eq!(s.buf.line(0), "Αβγ");
        assert_eq!(s.col, 1); // cursor advances
        key(&mut s, '~');
        assert_eq!(s.buf.line(0), "ΑΒγ");
        assert_eq!(s.col, 2);
        key(&mut s, '~');
        assert_eq!(s.buf.line(0), "ΑΒΓ");
    }

    // -- Operator with Unicode --

    #[test]
    fn unicode_dw_deletes_word() {
        let mut s = ed("中文 abc");
        keys(&mut s, "dw");
        assert_eq!(s.buf.line(0), "abc");
    }

    #[test]
    fn unicode_dollar_delete_to_eol() {
        let mut s = ed("αβγ");
        key(&mut s, '$');
        key(&mut s, 'd');
        key(&mut s, '0');
        // d$ from end of line, then 0 goes back
        // Actually let's do it properly:
    }

    #[test]
    fn unicode_d_dollar_to_eol() {
        let mut s = ed("αβγδ");
        key(&mut s, 'd');
        key(&mut s, '$');
        assert_eq!(s.buf.line(0), ""); // deleted from col 0 to end
    }

    #[test]
    fn unicode_d0_to_start() {
        let mut s = ed("αβγ");
        s.col = 2;
        key(&mut s, 'd');
        key(&mut s, '0');
        assert_eq!(s.buf.line(0), "γ");
    }

    #[test]
    fn unicode_cw_changes_word() {
        let mut s = ed("中文 abc");
        keys(&mut s, "cw");
        assert_eq!(s.mode, Mode::Insert);
        // cw from col 0, w motion goes to col 3 (start of "abc")
        // Exclusive delete removes chars 0..3 = "中文 ", leaving "abc"
        assert_eq!(s.buf.line(0), "abc");
    }

    #[test]
    fn unicode_indent_dedent() {
        let mut s = ed("中文");
        keys(&mut s, ">>");
        assert_eq!(s.buf.line(0), "    中文");
        keys(&mut s, "<<");
        assert_eq!(s.buf.line(0), "中文");
    }

    #[test]
    fn unicode_join_lines() {
        let mut s = ed("中文\nαβ");
        key(&mut s, 'J');
        assert_eq!(s.buf.line(0), "中文 αβ");
    }

    // -- Undo/redo with Unicode --

    #[test]
    fn unicode_undo_insert() {
        let mut s = ed("");
        key(&mut s, 'i');
        handle_key(&mut s, KeyCode::Char('中'), false);
        handle_key(&mut s, KeyCode::Char('文'), false);
        esc(&mut s);
        assert_eq!(s.buf.line(0), "中文");
        key(&mut s, 'u');
        assert_eq!(s.buf.line(0), "");
    }

    #[test]
    fn unicode_redo() {
        let mut s = ed("");
        key(&mut s, 'i');
        handle_key(&mut s, KeyCode::Char('中'), false);
        esc(&mut s);
        key(&mut s, 'u');
        assert_eq!(s.buf.line(0), "");
        ctrl(&mut s, 'r');
        assert_eq!(s.buf.line(0), "中");
    }

    // -- Search mode with Unicode --

    #[test]
    fn unicode_search_and_n() {
        let mut s = ed("中文 αβ 中文 γδ 中文");
        key(&mut s, '/');
        for c in "中文".chars() {
            handle_key(&mut s, KeyCode::Char(c), false);
        }
        enter(&mut s);
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.col, 6); // after "αβ "
        key(&mut s, 'n');
        assert_eq!(s.col, 12); // next "中文"
    }

    #[test]
    fn unicode_search_backward() {
        let mut s = ed("α β γ β α");
        s.col = 8;
        key(&mut s, '?');
        handle_key(&mut s, KeyCode::Char('β'), false);
        enter(&mut s);
        // searching backward from col 8 finds the nearest β at col 6
        assert_eq!(s.col, 6);
    }

    // -- Absolute byte offset with Unicode --

    #[test]
    fn unicode_absolute_byte_offset() {
        let mut s = ed("αβ\n中文字");
        // "αβ" = 4 bytes, "\n" = 1 byte, total 5
        s.row = 1;
        s.col = 2; // 3rd char '字', byte offset 6 from line start
        // total = 4 + 1 + 6 = 11
        assert_eq!(s.absolute_byte_offset(), 11);
    }

    #[test]
    fn unicode_absolute_byte_offset_at() {
        let s = ed("αβ\n中文字");
        // "αβ" = 4 bytes
        assert_eq!(s.absolute_byte_offset_at(1, 0), 5); // 4 + 1 (newline)
    }

    // -- G/gg with Unicode --

    #[test]
    fn unicode_g_goes_to_last() {
        let mut s = ed("中文\nαβ\nγδ");
        key(&mut s, 'G');
        assert_eq!(s.row, 2);
    }

    #[test]
    fn unicode_gg_goes_to_first() {
        let mut s = ed("中文\nαβ\nγδ");
        s.row = 2;
        keys(&mut s, "gg");
        assert_eq!(s.row, 0);
    }

    // -- Mixed ASCII and Unicode --

    #[test]
    fn unicode_mixed_ascii_buffer() {
        let b = Buffer::new("abc中文def🌍");
        assert_eq!(b.char_count(0), 9);
        // bytes: a(1) b(1) c(1) 中(3) 文(3) d(1) e(1) f(1) 🌍(4) = 16
        assert_eq!(b.line(0).len(), 16);
    }

    #[test]
    fn unicode_mixed_insert_delete() {
        let mut b = Buffer::new("a中b文c");
        b.delete_char(0, 1); // delete '中'
        assert_eq!(b.line(0), "ab文c");
        assert_eq!(b.char_count(0), 4);
    }

    #[test]
    fn unicode_mixed_word_motions() {
        let b = Buffer::new("abc 中文 def");
        let (r, c) = b.word_forward(0, 0);
        assert_eq!((r, c), (0, 4)); // skip "abc ", land on '中'
        let (r2, c2) = b.word_forward(0, c);
        assert_eq!((r2, c2), (0, 7)); // skip "中文 ", land on 'd'
    }

    // -- Large multi-byte chars (4 bytes) --

    #[test]
    fn unicode_emoji_buffer_ops() {
        let mut b = Buffer::new("🎉🎊🎈");
        assert_eq!(b.char_count(0), 3);
        b.insert_char(0, 1, '★');
        assert_eq!(b.line(0), "🎉★🎊🎈");
        let ch = b.delete_char(0, 0);
        assert_eq!(ch, Some('🎉'));
        assert_eq!(b.line(0), "★🎊🎈");
    }

    #[test]
    fn unicode_emoji_word_motion() {
        let b = Buffer::new("🎉 hello 🌍");
        let (r, c) = b.word_forward(0, 0);
        assert_eq!((r, c), (0, 2)); // skip emoji + space, land on 'h'
    }

    #[test]
    fn unicode_emoji_search() {
        let b = Buffer::new("hello 🌍 world 🌍 end");
        let result = b.search_forward("🌍", 0, 0);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(r, 0);
        assert_eq!(c, 6); // "hello " = 6 chars
    }

    // -- Paragraph motions with Unicode --

    #[test]
    fn unicode_paragraph_motions() {
        let mut s = ed("中文\nαβ\n\nγδ");
        key(&mut s, '}');
        assert_eq!(s.row, 2, "close-brace should jump to blank line");
        // { from blank line searches backward past non-blank lines to line 0
        key(&mut s, '{');
        assert_eq!(s.row, 0);
    }

    // -- Count parsing with Unicode content --

    #[test]
    fn unicode_count_with_j() {
        let mut s = ed("α\nβ\nγ\nδ\nε");
        keys(&mut s, "3j");
        assert_eq!(s.row, 3);
        assert_eq!(s.buf.line(3), "δ");
    }

    // -- Cursor position tracking with Unicode --

    #[test]
    fn unicode_insert_exit_cursor() {
        let mut s = ed("αβγ");
        key(&mut s, 'i');
        s.col = 2;
        esc(&mut s);
        assert_eq!(s.col, 1); // moved left by 1 char
        assert_eq!(s.col_want, 1);
    }

    #[test]
    fn unicode_append_mode() {
        let mut s = ed("αβγ");
        s.col = 1;
        key(&mut s, 'a');
        assert_eq!(s.col, 2); // after 'β'
        assert_eq!(s.mode, Mode::Insert);
    }

    #[test]
    fn unicode_A_end_of_line() {
        let mut s = ed("αβγ");
        key(&mut s, 'A');
        assert_eq!(s.col, 3); // after last char
        assert_eq!(s.mode, Mode::Insert);
    }

    #[test]
    fn unicode_I_first_non_blank() {
        let mut s = ed("   αβγ");
        key(&mut s, 'I');
        assert_eq!(s.col, 3);
        assert_eq!(s.mode, Mode::Insert);
    }
}
