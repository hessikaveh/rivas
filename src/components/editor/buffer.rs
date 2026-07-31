use std::fmt;

// ─────────────────────────────────────────────────────────────────────────────
// CharClass
// ─────────────────────────────────────────────────────────────────────────────

/// Classification of a character for word-motion purposes.
///
/// Determines word boundaries: a word is a contiguous run of [`Word`](CharClass::Word) or
/// [`Punct`](CharClass::Punct) characters, separated by [`Whitespace`](CharClass::Whitespace).
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub(crate) enum CharClass {
    /// Alphanumeric characters and underscores (`[a-zA-Z0-9_]`).
    Word,
    /// Any non-whitespace, non-alphanumeric character.
    Punct,
    /// Spaces, tabs, and other whitespace.
    Whitespace,
}

/// Returns the [`CharClass`] of a character.
pub(crate) fn char_class(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Buffer
// ─────────────────────────────────────────────────────────────────────────────

/// A multi-line text buffer backed by a `Vec<String>`.
///
/// All public methods use **character indices** (not byte offsets) for column positions,
/// converting internally via [`byte_offset`](Buffer::byte_offset) when needed for
/// string slicing. This makes the API safe for multi-byte Unicode content.
#[derive(Clone, Debug)]
pub struct Buffer {
    pub lines: Vec<String>,
}

impl Buffer {
    /// Creates a new buffer from the given text.
    ///
    /// Splits on `\n`. An empty input produces a single empty line.
    pub fn new(text: &str) -> Self {
        let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines }
    }

    /// Reconstructs the full text content by joining all lines with `\n`.
    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Returns the number of lines in the buffer.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Returns the content of line `row` as a string slice.
    ///
    /// Clamps `row` to the valid range — never panics.
    pub fn line(&self, row: usize) -> &str {
        &self.lines[row.min(self.lines.len().saturating_sub(1))]
    }

    /// Returns a mutable reference to the content of line `row`.
    ///
    /// Clamps `row` to the valid range — never panics.
    pub fn line_mut(&mut self, row: usize) -> &mut String {
        let idx = row.min(self.lines.len().saturating_sub(1));
        &mut self.lines[idx]
    }

    /// Returns the number of Unicode characters (scalar values) in line `row`.
    pub fn char_count(&self, row: usize) -> usize {
        self.line(row).chars().count()
    }

    /// Clamps `col` to a valid position within line `row`.
    ///
    /// In insert mode (`insert = true`), `col` may equal `char_count` (cursor past last char).
    /// In normal mode (`insert = false`), `col` is at most `char_count - 1` (on a character).
    /// Returns `0` for empty lines in normal mode.
    pub fn clamp_col(&self, row: usize, col: usize, insert: bool) -> usize {
        let len = self.char_count(row);
        if insert {
            col.min(len)
        } else if len == 0 {
            0
        } else {
            col.min(len - 1)
        }
    }

    /// Converts a character-index column to a byte offset within line `row`.
    ///
    /// Returns `line.len()` if `col` is at or past the end of the line.
    pub fn byte_offset(&self, row: usize, col: usize) -> usize {
        self.line(row)
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(self.line(row).len())
    }

    /// Inserts a single character at position `col` on line `row`.
    ///
    /// Extends the buffer with empty lines if `row` is beyond the current end.
    pub fn insert_char(&mut self, row: usize, col: usize, ch: char) {
        while row >= self.lines.len() {
            self.lines.push(String::new());
        }
        let byte = self.byte_offset(row, col);
        self.lines[row].insert(byte, ch);
    }

    /// Inserts text at position `col` on line `row`.
    ///
    /// If `text` contains newlines, the line is split and new lines are inserted.
    /// Returns the `(row, col)` of the cursor position after the last inserted character.
    pub fn insert_text(&mut self, row: usize, col: usize, text: &str) -> (usize, usize) {
        if text.is_empty() {
            return (row, col);
        }
        let start_byte = self.byte_offset(row, col);
        let line = &self.lines[row];
        let left = line[..start_byte].to_string();
        let right = line[start_byte..].to_string();

        let parts: Vec<&str> = text.split('\n').collect();
        if parts.len() == 1 {
            let new_line = format!("{}{}{}", left, parts[0], right);
            self.lines[row] = new_line;
            let end_col = col + parts[0].chars().count();
            (row, end_col.saturating_sub(1))
        } else {
            self.lines[row] = format!("{}{}", left, parts[0]);
            let num_parts = parts.len();
            for i in 1..(num_parts - 1) {
                self.lines.insert(row + i, parts[i].to_string());
            }
            let last_line = format!("{}{}", parts[num_parts - 1], right);
            self.lines.insert(row + num_parts - 1, last_line);
            let end_row = row + num_parts - 1;
            let end_col = parts[num_parts - 1].chars().count();
            (end_row, end_col.saturating_sub(1))
        }
    }

    /// Deletes the character at `col` on line `row`.
    ///
    /// Returns the deleted character, or `None` if `col` is past the end of the line.
    pub fn delete_char(&mut self, row: usize, col: usize) -> Option<char> {
        if col >= self.char_count(row) {
            return None;
        }
        let byte = self.byte_offset(row, col);
        Some(self.lines[row].remove(byte))
    }

    /// Splits line `row` at `col`, moving everything from `col` onward to a new line below.
    pub fn split_line(&mut self, row: usize, col: usize) {
        let byte = self.byte_offset(row, col);
        let rest = self.lines[row].split_off(byte);
        self.lines.insert(row + 1, rest);
    }

    /// Joins line `row + 1` onto line `row`, removing the newline between them.
    ///
    /// Does nothing if `row + 1` is beyond the buffer end.
    pub fn join_lines(&mut self, row: usize) {
        if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
        }
    }

    /// Removes line `row` from the buffer, returning its content.
    ///
    /// If this is the only line, clears it instead of removing it (the buffer always has at least one line).
    pub fn delete_line(&mut self, row: usize) -> String {
        if self.lines.len() == 1 {
            let s = self.lines[0].clone();
            self.lines[0].clear();
            s
        } else {
            self.lines.remove(row)
        }
    }

    /// Inserts a new line with the given `content` at position `row`.
    pub fn insert_line(&mut self, row: usize, content: String) {
        self.lines.insert(row, content);
    }

    /// Replaces the character range `[col_start, col_end)` on line `row` with `s`.
    ///
    /// If `col_end` exceeds the line length, the replacement extends to the end.
    pub fn replace_range_on_line(&mut self, row: usize, col_start: usize, col_end: usize, s: &str) {
        let start = self.byte_offset(row, col_start);
        let end = self.byte_offset(row, col_end);
        let mut new = self.lines[row][..start].to_string();
        new.push_str(s);
        new.push_str(&self.lines[row][end..]);
        self.lines[row] = new;
    }

    /// Moves the cursor forward by one word (Vim `w` motion).
    ///
    /// Skips the current word class (word/punct), then skips whitespace.
    /// Wraps to the beginning of the next line at line end.
    /// Returns `(row, col)` of the destination.
    pub fn word_forward(&self, row: usize, col: usize) -> (usize, usize) {
        let chars: Vec<char> = self.line(row).chars().collect();
        if chars.is_empty() {
            if row + 1 < self.line_count() {
                return (row + 1, 0);
            }
            return (row, 0);
        }

        let mut c = col;
        let start_class = char_class(chars[c]);

        if start_class != CharClass::Whitespace {
            while c < chars.len() && char_class(chars[c]) == start_class {
                c += 1;
            }
        }

        while c < chars.len() && char_class(chars[c]) == CharClass::Whitespace {
            c += 1;
        }

        if c >= chars.len() {
            if row + 1 < self.line_count() {
                (row + 1, self.first_non_blank(row + 1))
            } else {
                (row, chars.len().saturating_sub(1))
            }
        } else {
            (row, c)
        }
    }

    /// Moves the cursor backward by one word (Vim `b` motion).
    ///
    /// Skips whitespace backward, then skips the preceding word class.
    /// Wraps to the end of the previous line at line start.
    /// Returns `(row, col)` of the destination.
    pub fn word_backward(&self, row: usize, col: usize) -> (usize, usize) {
        if col == 0 {
            if row > 0 {
                let prev_row = row - 1;
                return (prev_row, self.char_count(prev_row).saturating_sub(1));
            }
            return (0, 0);
        }

        let chars: Vec<char> = self.line(row).chars().collect();
        let mut c = col as isize - 1;

        while c >= 0 && char_class(chars[c as usize]) == CharClass::Whitespace {
            c -= 1;
        }

        if c < 0 {
            if row > 0 {
                let prev_row = row - 1;
                return (prev_row, self.char_count(prev_row).saturating_sub(1));
            }
            return (row, 0);
        }

        let target_class = char_class(chars[c as usize]);
        while c > 0 && char_class(chars[(c - 1) as usize]) == target_class {
            c -= 1;
        }

        (row, c as usize)
    }

    /// Moves the cursor to the end of the current word (Vim `e` motion).
    ///
    /// Skips whitespace forward, then advances to the last character of the word class.
    /// Returns `(row, col)` of the destination.
    pub fn word_end(&self, row: usize, col: usize) -> (usize, usize) {
        let chars: Vec<char> = self.line(row).chars().collect();
        if chars.is_empty() {
            if row + 1 < self.line_count() {
                return self.word_end(row + 1, 0);
            }
            return (row, 0);
        }

        let mut c = col + 1;
        while c < chars.len() && char_class(chars[c]) == CharClass::Whitespace {
            c += 1;
        }

        if c >= chars.len() {
            if row + 1 < self.line_count() {
                return self.word_end(row + 1, 0);
            }
            return (row, chars.len().saturating_sub(1));
        }

        let target_class = char_class(chars[c]);
        while c + 1 < chars.len() && char_class(chars[c + 1]) == target_class {
            c += 1;
        }

        (row, c)
    }

    /// Searches forward on line `row` for `target`, starting after `col`.
    ///
    /// If `before` is true, returns the position just before the match; otherwise,
    /// returns the position of the match itself. Returns `None` if not found on this line.
    pub fn find_forward(
        &self,
        row: usize,
        col: usize,
        target: char,
        before: bool,
    ) -> Option<usize> {
        let chars: Vec<char> = self.line(row).chars().collect();
        for i in (col + 1)..chars.len() {
            if chars[i] == target {
                return Some(if before { i.saturating_sub(1) } else { i });
            }
        }
        None
    }

    /// Searches backward on line `row` for `target`, ending before `col`.
    ///
    /// If `before` is true, returns the position just after the match; otherwise,
    /// returns the position of the match itself. Returns `None` if not found on this line.
    pub fn find_backward(
        &self,
        row: usize,
        col: usize,
        target: char,
        before: bool,
    ) -> Option<usize> {
        if col == 0 {
            return None;
        }
        let chars: Vec<char> = self.line(row).chars().collect();
        for i in (0..col).rev() {
            if chars[i] == target {
                return Some(if before {
                    (i + 1).min(chars.len().saturating_sub(1))
                } else {
                    i
                });
            }
        }
        None
    }

    /// Returns the column of the first non-whitespace character on line `row`.
    ///
    /// Returns `0` if the line is empty or has no leading whitespace.
    pub fn first_non_blank(&self, row: usize) -> usize {
        self.line(row)
            .chars()
            .take_while(|c| c.is_whitespace())
            .count()
    }

    /// Searches forward across the entire buffer for `pat`.
    ///
    /// Starts searching from `(start_row, start_col)`, wrapping around to the beginning
    /// if necessary. Returns `Some((row, col))` of the first match, or `None` if not found.
    /// The `col` in the result is the character index where the pattern begins.
    pub fn search_forward(
        &self,
        pat: &str,
        start_row: usize,
        start_col: usize,
    ) -> Option<(usize, usize)> {
        if pat.is_empty() {
            return None;
        }
        let total = self.line_count();
        for offset in 0..total {
            let row = (start_row + offset) % total;
            let line = self.line(row);
            let from_byte = if offset == 0 {
                let here = self.byte_offset(row, start_col);
                line[here..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| here + i)
                    .unwrap_or(line.len())
            } else {
                0
            };

            if let Some(pos) = line[from_byte..].find(pat) {
                let match_byte = from_byte + pos;
                let col = line[..match_byte].chars().count();
                return Some((row, col));
            }
        }
        None
    }

    /// Searches backward across the entire buffer for `pat`.
    ///
    /// Starts searching from `(start_row, start_col)`, wrapping around to the end
    /// if necessary. Returns `Some((row, col))` of the first match, or `None` if not found.
    /// The `col` in the result is the character index where the pattern begins.
    pub fn search_backward(
        &self,
        pat: &str,
        start_row: usize,
        start_col: usize,
    ) -> Option<(usize, usize)> {
        if pat.is_empty() {
            return None;
        }
        let total = self.line_count();
        for offset in 0..total {
            let row = if start_row >= offset {
                start_row - offset
            } else {
                total - (offset - start_row)
            };
            let line = self.line(row);
            let to_byte = if offset == 0 {
                self.byte_offset(row, start_col)
            } else {
                line.len()
            };

            if let Some(pos) = line[..to_byte].rfind(pat) {
                let col = line[..pos].chars().count();
                return Some((row, col));
            }
        }
        None
    }
}

/// Formats the buffer as its full text content (lines joined by `\n`).
impl fmt::Display for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_text())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_new_single_line() {
        let b = Buffer::new("hello");
        assert_eq!(b.lines, vec!["hello"]);
        assert_eq!(b.line_count(), 1);
    }

    #[test]
    fn buffer_new_multi_line() {
        let b = Buffer::new("hello\nworld\nfoo");
        assert_eq!(b.lines, vec!["hello", "world", "foo"]);
        assert_eq!(b.line_count(), 3);
    }

    #[test]
    fn buffer_new_empty() {
        let b = Buffer::new("");
        assert_eq!(b.lines, vec![""]);
        assert_eq!(b.line_count(), 1);
    }

    #[test]
    fn buffer_to_text_roundtrip() {
        let text = "line1\nline2\nline3";
        let b = Buffer::new(text);
        assert_eq!(b.to_text(), text);
    }

    #[test]
    fn buffer_char_count() {
        let b = Buffer::new("hello");
        assert_eq!(b.char_count(0), 5);
    }

    #[test]
    fn buffer_clamp_col_normal_mode() {
        let b = Buffer::new("hello");
        assert_eq!(b.clamp_col(0, 10, false), 4);
        assert_eq!(b.clamp_col(0, 2, false), 2);
    }

    #[test]
    fn buffer_clamp_col_insert_mode() {
        let b = Buffer::new("hello");
        assert_eq!(b.clamp_col(0, 10, true), 5);
        assert_eq!(b.clamp_col(0, 2, true), 2);
    }

    #[test]
    fn buffer_clamp_col_empty_line() {
        let b = Buffer::new("");
        assert_eq!(b.clamp_col(0, 0, false), 0);
        assert_eq!(b.clamp_col(0, 5, false), 0);
    }

    #[test]
    fn buffer_byte_offset_ascii() {
        let b = Buffer::new("hello");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 3), 3);
        assert_eq!(b.byte_offset(0, 5), 5);
    }

    #[test]
    fn buffer_insert_char() {
        let mut b = Buffer::new("hllo");
        b.insert_char(0, 1, 'e');
        assert_eq!(b.lines[0], "hello");
    }

    #[test]
    fn buffer_delete_char() {
        let mut b = Buffer::new("hello");
        let ch = b.delete_char(0, 1);
        assert_eq!(ch, Some('e'));
        assert_eq!(b.lines[0], "hllo");
    }

    #[test]
    fn buffer_split_line() {
        let mut b = Buffer::new("helloworld");
        b.split_line(0, 5);
        assert_eq!(b.lines, vec!["hello", "world"]);
    }

    #[test]
    fn buffer_join_lines() {
        let mut b = Buffer::new("hello\nworld");
        b.join_lines(0);
        assert_eq!(b.lines, vec!["helloworld"]);
    }

    #[test]
    fn buffer_delete_line_multi() {
        let mut b = Buffer::new("aaa\nbbb\nccc");
        let removed = b.delete_line(1);
        assert_eq!(removed, "bbb");
        assert_eq!(b.lines, vec!["aaa", "ccc"]);
    }

    #[test]
    fn buffer_delete_line_last_remaining() {
        let mut b = Buffer::new("only");
        let removed = b.delete_line(0);
        assert_eq!(removed, "only");
        assert_eq!(b.lines, vec![""]);
    }

    #[test]
    fn buffer_first_non_blank() {
        let b = Buffer::new("   hello");
        assert_eq!(b.first_non_blank(0), 3);
    }

    #[test]
    fn buffer_first_non_blank_no_indent() {
        let b = Buffer::new("hello");
        assert_eq!(b.first_non_blank(0), 0);
    }

    #[test]
    fn replace_range_on_line() {
        let mut b = Buffer::new("hello world");
        b.replace_range_on_line(0, 6, 11, "rust");
        assert_eq!(b.lines[0], "hello rust");
    }

    #[test]
    fn insert_text_single_line() {
        let mut b = Buffer::new("hd");
        let (r, _c) = b.insert_text(0, 1, "ello worl");
        assert_eq!(b.lines[0], "hello world");
        assert_eq!(r, 0);
    }

    #[test]
    fn insert_text_multi_line() {
        let mut b = Buffer::new("hello");
        let (_r, _c) = b.insert_text(0, 5, "\nworld\nfoo");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line(0), "hello");
        assert_eq!(b.line(1), "world");
        assert_eq!(b.line(2), "foo");
    }

    #[test]
    fn search_forward_finds_first_match() {
        let b = Buffer::new("hello world hello");
        let result = b.search_forward("hello", 0, 0);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(r, 0);
        assert_eq!(c, 12);
    }

    #[test]
    fn search_forward_wraps_around() {
        let b = Buffer::new("hello\nworld\nfoo");
        let result = b.search_forward("hello", 2, 0);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(r, 0);
        assert_eq!(c, 0);
    }

    #[test]
    fn search_backward_finds_match() {
        let b = Buffer::new("hello world hello");
        let result = b.search_backward("hello", 0, 12);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(r, 0);
        assert_eq!(c, 0);
    }

    #[test]
    fn search_empty_pattern_returns_none() {
        let b = Buffer::new("hello");
        assert!(b.search_forward("", 0, 0).is_none());
        assert!(b.search_backward("", 0, 0).is_none());
    }

    #[test]
    fn word_forward_basic() {
        let b = Buffer::new("hello world");
        let (r, c) = b.word_forward(0, 0);
        assert_eq!((r, c), (0, 6));
    }

    #[test]
    fn word_backward_basic() {
        let b = Buffer::new("hello world");
        let (r, c) = b.word_backward(0, 8);
        assert_eq!((r, c), (0, 6));
    }

    #[test]
    fn word_end_basic() {
        let b = Buffer::new("hello world");
        let (r, c) = b.word_end(0, 0);
        assert_eq!((r, c), (0, 4));
    }

    #[test]
    fn find_forward_basic() {
        let b = Buffer::new("hello");
        assert_eq!(b.find_forward(0, 0, 'l', false), Some(2));
        assert_eq!(b.find_forward(0, 0, 'l', true), Some(1));
    }

    #[test]
    fn find_backward_basic() {
        let b = Buffer::new("hello");
        assert_eq!(b.find_backward(0, 4, 'l', false), Some(3));
        assert_eq!(b.find_backward(0, 4, 'l', true), Some(4));
    }

    #[test]
    fn find_forward_not_found() {
        let b = Buffer::new("hello");
        assert_eq!(b.find_forward(0, 0, 'z', false), None);
    }

    #[test]
    fn buffer_display() {
        let b = Buffer::new("hello\nworld");
        assert_eq!(format!("{}", b), "hello\nworld");
    }

    // ═════════════════════════════════════════════════════════════════════
    // Unicode Tests
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn unicode_byte_offset_2byte() {
        let b = Buffer::new("é");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 1), 2); // é is 2 bytes
    }

    #[test]
    fn unicode_byte_offset_3byte() {
        let b = Buffer::new("中");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 1), 3); // 中 is 3 bytes
    }

    #[test]
    fn unicode_byte_offset_4byte() {
        let b = Buffer::new("🎉");
        assert_eq!(b.byte_offset(0, 0), 0);
        assert_eq!(b.byte_offset(0, 1), 4); // 🎉 is 4 bytes
    }

    #[test]
    fn unicode_byte_offset_mixed_sizes() {
        let b = Buffer::new("aé中🎉z");
        assert_eq!(b.byte_offset(0, 0), 0); // a
        assert_eq!(b.byte_offset(0, 1), 1); // é (2 bytes)
        assert_eq!(b.byte_offset(0, 2), 3); // 中 (3 bytes)
        assert_eq!(b.byte_offset(0, 3), 6); // 🎉 (4 bytes)
        assert_eq!(b.byte_offset(0, 4), 10); // z
    }

    #[test]
    fn unicode_char_count() {
        let b = Buffer::new("aé中🎉z");
        assert_eq!(b.char_count(0), 5);
        assert_eq!(b.line(0).len(), 11); // byte length differs
    }

    #[test]
    fn unicode_clamp_col() {
        let b = Buffer::new("中文字");
        assert_eq!(b.clamp_col(0, 10, false), 2);
        assert_eq!(b.clamp_col(0, 2, false), 2);
        assert_eq!(b.clamp_col(0, 10, true), 3);
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
    fn unicode_delete_char_4byte() {
        let mut b = Buffer::new("🎉🎊🎈");
        let ch = b.delete_char(0, 1);
        assert_eq!(ch, Some('🎊'));
        assert_eq!(b.line(0), "🎉🎈");
    }

    #[test]
    fn unicode_split_line() {
        let mut b = Buffer::new("中文文字");
        b.split_line(0, 2);
        assert_eq!(b.line(0), "中文");
        assert_eq!(b.line(1), "文字");
    }

    #[test]
    fn unicode_join_lines() {
        let mut b = Buffer::new("中\n文");
        b.join_lines(0);
        assert_eq!(b.line(0), "中文");
    }

    #[test]
    fn unicode_insert_text() {
        let mut b = Buffer::new("αγ");
        let (r, c) = b.insert_text(0, 1, "β");
        assert_eq!(b.line(0), "αβγ");
        assert_eq!(r, 0);
        // insert_text returns cursor ON last inserted char (saturating_sub(1))
        assert_eq!(c, 1);
    }

    #[test]
    fn unicode_replace_range() {
        let mut b = Buffer::new("αβγδ");
        b.replace_range_on_line(0, 1, 3, "XY");
        assert_eq!(b.line(0), "αXYδ");
    }

    #[test]
    fn unicode_word_forward() {
        let b = Buffer::new("中文 abc");
        let (r, c) = b.word_forward(0, 0);
        assert_eq!((r, c), (0, 3));
    }

    #[test]
    fn unicode_word_backward() {
        let b = Buffer::new("中文 abc");
        let (r, c) = b.word_backward(0, 4);
        assert_eq!((r, c), (0, 3));
    }

    #[test]
    fn unicode_word_end() {
        let b = Buffer::new("中文 abc");
        let (r, c) = b.word_end(0, 0);
        assert_eq!((r, c), (0, 1));
    }

    #[test]
    fn unicode_search_forward() {
        let b = Buffer::new("hello 中文 world");
        let result = b.search_forward("中文", 0, 0);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(r, 0);
        assert_eq!(c, 6);
    }

    #[test]
    fn unicode_buffer_search_backward() {
        let b = Buffer::new("α β γ β α");
        // β positions: col 2 and col 6; searching backward from col 8 finds col 6
        let result = b.search_backward("β", 0, 8);
        assert!(result.is_some());
        let (r, c) = result.unwrap();
        assert_eq!(c, 6);
    }

    #[test]
    fn unicode_find_forward() {
        let b = Buffer::new("αβγδ");
        assert_eq!(b.find_forward(0, 0, 'γ', false), Some(2));
    }

    #[test]
    fn unicode_find_backward() {
        let b = Buffer::new("αβγδ");
        assert_eq!(b.find_backward(0, 3, 'β', false), Some(1));
    }

    #[test]
    fn unicode_first_non_blank() {
        let b = Buffer::new("   中文");
        assert_eq!(b.first_non_blank(0), 3);
    }
}
