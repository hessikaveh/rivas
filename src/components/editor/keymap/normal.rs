use iocraft::prelude::*;

use super::super::mode::MotionType;
use super::super::{EditorState, Mode};
use super::search::do_search;

/// Handles key presses in Normal mode.
///
/// Processes motions (`hjkl`, `w`, `b`, `e`, `0`, `$`, `^`, `G`, `gg`, `{`, `}`),
/// operators (`d`, `c`, `y`, `x`, `X`, `r`, `~`, `>`, `<`, `J`), mode switches
/// (`i`, `I`, `a`, `A`, `o`, `O`, `s`, `v`, `:`, `/`, `?`), and special commands
/// (`u`, `Ctrl-R`, `p`, `P`, `n`, `N`, `;`, `,`, `ZZ`, `ZQ`).
///
/// Returns `true` if the editor should quit.
pub fn handle_normal(s: &mut EditorState, code: KeyCode, ctrl: bool) -> bool {
    if ctrl {
        match code {
            KeyCode::Char('r') => {
                let c = s.count();
                s.count_buf.clear();
                for _ in 0..c {
                    s.redo();
                }
                s.clamp();
                return false;
            }
            KeyCode::Char('d') => {
                let h = (s.view_height / 2).max(1);
                s.row = (s.row + h).min(s.buf.line_count() - 1);
                s.clamp();
                s.count_buf.clear();
                return false;
            }
            KeyCode::Char('u') => {
                let h = (s.view_height / 2).max(1);
                s.row = s.row.saturating_sub(h);
                s.clamp();
                s.count_buf.clear();
                return false;
            }
            KeyCode::Char('f') => {
                s.row = (s.row + s.view_height).min(s.buf.line_count() - 1);
                s.clamp();
                s.count_buf.clear();
                return false;
            }
            KeyCode::Char('b') => {
                s.row = s.row.saturating_sub(s.view_height);
                s.clamp();
                s.count_buf.clear();
                return false;
            }
            _ => {}
        }
    }

    // Resolve pending two-char sequences
    if let Some(pend) = s.pending {
        s.pending = None;
        match pend {
            'g' => {
                if code == KeyCode::Char('g') {
                    let dest = (0, s.buf.first_non_blank(0));
                    if let Some(op) = s.operator.take() {
                        s.execute_operator(op, dest, MotionType::Line, '"');
                    } else {
                        s.row = dest.0;
                        s.col = dest.1;
                        s.col_want = s.col;
                    }
                }
                s.count_buf.clear();
                return false;
            }
            'Z' => {
                if code == KeyCode::Char('Z') {
                    let _ = std::fs::write(&s.filename, s.buf.to_text());
                    return true;
                }
                if code == KeyCode::Char('Q') {
                    return true;
                }
                s.count_buf.clear();
                return false;
            }
            'r' => {
                if let KeyCode::Char(c) = code {
                    s.push_undo();
                    s.buf.delete_char(s.row, s.col);
                    s.buf.insert_char(s.row, s.col, c);
                    s.modified = true;
                }
                s.count_buf.clear();
                return false;
            }
            m @ ('f' | 't' | 'F' | 'T') => {
                if let KeyCode::Char(target) = code {
                    let backward = m == 'F' || m == 'T';
                    s.last_find = Some((target, backward));
                    if let Some(dest) = s.apply_motion(m, Some(target)) {
                        if let Some(op) = s.operator.take() {
                            s.execute_operator(op, dest, MotionType::Inclusive, '"');
                        } else {
                            s.row = dest.0;
                            s.col = dest.1;
                            s.col_want = s.col;
                        }
                    }
                }
                s.count_buf.clear();
                s.clamp();
                return false;
            }
            _ => {
                s.count_buf.clear();
                return false;
            }
        }
    }

    match code {
        // Count digits
        KeyCode::Char(d @ '1'..='9') if s.count_buf.len() < 8 => {
            s.count_buf.push(d);
            return false;
        }
        KeyCode::Char('0') if !s.count_buf.is_empty() => {
            s.count_buf.push('0');
            return false;
        }

        // Enter insert
        KeyCode::Char('i') => {
            s.push_undo();
            s.mode = Mode::Insert;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('I') => {
            s.push_undo();
            s.col = s.buf.first_non_blank(s.row);
            s.mode = Mode::Insert;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('a') => {
            s.push_undo();
            let l = s.buf.char_count(s.row);
            if l > 0 {
                s.col = (s.col + 1).min(l);
            }
            s.mode = Mode::Insert;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('A') => {
            s.push_undo();
            s.col = s.buf.char_count(s.row);
            s.mode = Mode::Insert;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('o') => {
            s.push_undo();
            s.buf.insert_line(s.row + 1, String::new());
            s.row += 1;
            s.col = 0;
            s.mode = Mode::Insert;
            s.modified = true;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('O') => {
            s.push_undo();
            s.buf.insert_line(s.row, String::new());
            s.col = 0;
            s.mode = Mode::Insert;
            s.modified = true;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('s') => {
            s.push_undo();
            if let Some(c) = s.buf.delete_char(s.row, s.col) {
                s.yank('"', c.to_string());
            }
            s.mode = Mode::Insert;
            s.modified = true;
            s.count_buf.clear();
            return false;
        }

        // Visual / command / search
        KeyCode::Char('v') => {
            s.visual_start = (s.row, s.col);
            s.mode = Mode::Visual;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char(':') => {
            s.mode = Mode::Command;
            s.cmd_buf.clear();
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('/') => {
            s.mode = Mode::Search { forward: true };
            s.cmd_buf.clear();
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('?') => {
            s.mode = Mode::Search { forward: false };
            s.cmd_buf.clear();
            s.count_buf.clear();
            return false;
        }

        // Undo / search repeat
        KeyCode::Char('u') => {
            let c = s.count();
            s.count_buf.clear();
            for _ in 0..c {
                s.undo();
            }
            s.clamp();
            return false;
        }
        KeyCode::Char('n') => {
            do_search(s, s.search_forward);
            s.needs_rerender = true;
            s.count_buf.clear();
            return false;
        }
        KeyCode::Char('N') => {
            let fwd = !s.search_forward;
            do_search(s, fwd);
            s.needs_rerender = true;
            s.count_buf.clear();
            return false;
        }

        // Operators
        KeyCode::Char(op @ ('d' | 'c' | 'y')) => {
            if s.operator == Some(op) {
                let count = s.count();
                s.count_buf.clear();
                s.operator = None;
                match op {
                    'd' => s.delete_lines(count, '"'),
                    'y' => s.yank_lines(count, '"'),
                    'c' => {
                        s.push_undo();
                        s.yank_lines(count, '"');
                        for _ in 1..count {
                            if s.row + 1 < s.buf.line_count() {
                                s.buf.delete_line(s.row + 1);
                            } else if s.row > 0 {
                                s.buf.delete_line(s.row);
                                s.row -= 1;
                            }
                        }
                        s.buf.lines[s.row].clear();
                        s.col = 0;
                        s.mode = Mode::Insert;
                        s.modified = true;
                    }
                    _ => {}
                }
            } else {
                s.operator = Some(op);
                return false;
            }
        }

        // x X
        KeyCode::Char('x') | KeyCode::Delete => {
            s.push_undo();
            let count = s.count();
            s.count_buf.clear();
            let mut cut = String::new();
            for _ in 0..count {
                if s.col < s.buf.char_count(s.row) {
                    if let Some(c) = s.buf.delete_char(s.row, s.col) {
                        cut.push(c);
                    }
                }
            }
            if !cut.is_empty() {
                s.yank('"', cut);
            }
            s.clamp();
            s.modified = true;
        }
        KeyCode::Char('X') => {
            s.push_undo();
            if s.col > 0 {
                s.col -= 1;
                if let Some(c) = s.buf.delete_char(s.row, s.col) {
                    s.yank('"', c.to_string());
                }
                s.modified = true;
            }
        }

        // r (replace)
        KeyCode::Char('r') => {
            s.pending = Some('r');
            return false;
        }

        // Paste
        KeyCode::Char('p') => {
            let count = s.count();
            s.count_buf.clear();
            s.push_undo();
            for _ in 0..count {
                s.paste_after('"');
            }
        }
        KeyCode::Char('P') => {
            let count = s.count();
            s.count_buf.clear();
            s.push_undo();
            for _ in 0..count {
                s.paste_before('"');
            }
        }

        // J ~ >> <<
        KeyCode::Char('J') => {
            s.push_undo();
            let c = s.count().saturating_sub(1).max(1);
            s.count_buf.clear();
            for _ in 0..c {
                if s.row + 1 < s.buf.line_count() {
                    let next = s.buf.lines.remove(s.row + 1);
                    let trimmed_next = next.trim_start();
                    if !s.buf.lines[s.row].is_empty()
                        && !s.buf.lines[s.row].ends_with(' ')
                        && !trimmed_next.is_empty()
                    {
                        s.buf.lines[s.row].push(' ');
                    }
                    s.buf.lines[s.row].push_str(trimmed_next);
                }
            }
            s.modified = true;
        }
        KeyCode::Char('~') => {
            s.push_undo();
            if let Some(c) = s.buf.line(s.row).chars().nth(s.col) {
                let tog: String = if c.is_uppercase() {
                    c.to_lowercase().collect()
                } else {
                    c.to_uppercase().collect()
                };
                s.buf.replace_range_on_line(s.row, s.col, s.col + 1, &tog);
                s.col = (s.col + 1).min(s.buf.char_count(s.row).saturating_sub(1));
                s.modified = true;
            }
        }
        KeyCode::Char('>') if s.operator == Some('>') => {
            s.push_undo();
            let c = s.count();
            s.operator = None;
            s.count_buf.clear();
            for i in 0..c {
                let r = (s.row + i).min(s.buf.line_count() - 1);
                s.buf.lines[r].insert_str(0, "    ");
            }
            s.modified = true;
        }
        KeyCode::Char('<') if s.operator == Some('<') => {
            s.push_undo();
            let c = s.count();
            s.operator = None;
            s.count_buf.clear();
            for i in 0..c {
                let r = (s.row + i).min(s.buf.line_count() - 1);
                let sp = s.buf.lines[r]
                    .chars()
                    .take_while(|&c| c == ' ')
                    .count()
                    .min(4);
                s.buf.lines[r] = s.buf.lines[r][sp..].to_string();
            }
            s.modified = true;
        }
        KeyCode::Char('>') => {
            s.operator = Some('>');
            return false;
        }
        KeyCode::Char('<') => {
            s.operator = Some('<');
            return false;
        }

        // Two-char sequences
        KeyCode::Char('g') => {
            s.pending = Some('g');
            return false;
        }
        KeyCode::Char('Z') => {
            s.pending = Some('Z');
            return false;
        }
        KeyCode::Char(m @ ('f' | 't' | 'F' | 'T')) => {
            s.pending = Some(m);
            return false;
        }

        // ; ,
        KeyCode::Char(';') | KeyCode::Char(',') => {
            if let Some((target, was_backward)) = s.last_find {
                let fwd = if code == KeyCode::Char(';') {
                    !was_backward
                } else {
                    was_backward
                };
                let nc = if fwd {
                    s.buf.find_forward(s.row, s.col, target, false)
                } else {
                    s.buf.find_backward(s.row, s.col, target, false)
                };
                if let Some(c) = nc {
                    s.col = c;
                    s.col_want = s.col;
                }
            }
        }

        // G
        KeyCode::Char('G') => {
            if let Some(dest) = s.apply_motion('G', None) {
                if let Some(op) = s.operator.take() {
                    s.execute_operator(op, dest, MotionType::Line, '"');
                } else {
                    s.row = dest.0;
                    s.col = dest.1;
                    s.col_want = s.col;
                }
            }
        }

        // j k (sticky col)
        KeyCode::Char('j') | KeyCode::Down => {
            let count = s.count();
            s.count_buf.clear();
            if let Some(op) = s.operator.take() {
                let dest = ((s.row + count).min(s.buf.line_count() - 1), s.col);
                s.execute_operator(op, dest, MotionType::Line, '"');
            } else {
                for _ in 0..count {
                    if s.row + 1 < s.buf.line_count() {
                        s.row += 1;
                        s.col = s.buf.clamp_col(s.row, s.col_want, false);
                    }
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let count = s.count();
            s.count_buf.clear();
            if let Some(op) = s.operator.take() {
                let dest = (s.row.saturating_sub(count), s.col);
                s.execute_operator(op, dest, MotionType::Line, '"');
            } else {
                for _ in 0..count {
                    if s.row > 0 {
                        s.row -= 1;
                        s.col = s.buf.clamp_col(s.row, s.col_want, false);
                    }
                }
            }
        }

        // All other motions (+ optional operator)
        KeyCode::Char('h')
        | KeyCode::Left
        | KeyCode::Char('l')
        | KeyCode::Right
        | KeyCode::Char('w')
        | KeyCode::Char('b')
        | KeyCode::Char('e')
        | KeyCode::Char('0')
        | KeyCode::Char('^')
        | KeyCode::Char('$')
        | KeyCode::Char('{')
        | KeyCode::Char('}') => {
            let ch = match code {
                KeyCode::Char(c) => c,
                KeyCode::Left => 'h',
                KeyCode::Right => 'l',
                _ => unreachable!(),
            };
            if let Some(dest) = s.apply_motion(ch, None) {
                if let Some(op) = s.operator.take() {
                    let motion_type = match ch {
                        '$' | 'e' => MotionType::Inclusive,
                        '{' | '}' => MotionType::Line,
                        _ => MotionType::Exclusive,
                    };
                    s.execute_operator(op, dest, motion_type, '"');
                } else {
                    s.row = dest.0;
                    s.col = dest.1;
                    s.col_want = s.col;
                }
            }
        }

        KeyCode::PageDown => {
            s.row = (s.row + s.view_height).min(s.buf.line_count() - 1);
            s.clamp();
        }
        KeyCode::PageUp => {
            s.row = s.row.saturating_sub(s.view_height);
            s.clamp();
        }

        _ => {}
    }

    if s.operator.is_none() && s.pending.is_none() {
        s.count_buf.clear();
    }
    s.clamp();
    false
}
