use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

use crate::assets::asset_cache::{AssetCache, ImageData};
use crate::assets::svg::rasterize_svg_to_png;
use anyhow::Result;
use tylax::latex_to_typst;
use typst::{
    Library, LibraryExt, compile,
    foundations::Bytes,
    layout::PagedDocument,
    syntax::{FileId, Source},
    text::{Font, FontBook},
    utils::LazyHash,
};
use unicodeit;

static MATH_CACHE: std::sync::LazyLock<AssetCache> = std::sync::LazyLock::new(AssetCache::new);

/// How math formulas are rendered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum MathMode {
    /// Render as Unicode glyphs inline in the terminal (default, no images).
    #[default]
    Unicode,
    /// Render as a rasterized image via the Kitty graphics protocol.
    Image,
}

static MATH_MODE: AtomicU8 = AtomicU8::new(0);

/// Set the global math rendering mode. Called from CLI and at runtime (`:math`).
pub fn set_math_mode(mode: MathMode) {
    MATH_MODE.store(mode as u8, Ordering::Relaxed);
}

/// Toggle between Unicode and Image math mode. Returns the new mode.
pub fn toggle_math_mode() -> MathMode {
    let new = match MATH_MODE.load(Ordering::Relaxed) {
        1 => MathMode::Unicode,
        _ => MathMode::Image,
    };
    MATH_MODE.store(new as u8, Ordering::Relaxed);
    new
}

/// Current math rendering mode (defaults to `Unicode`).
pub fn math_mode() -> MathMode {
    match MATH_MODE.load(Ordering::Relaxed) {
        1 => MathMode::Image,
        _ => MathMode::Unicode,
    }
}

/// Convert a LaTeX expression to a Unicode text representation. Used as the
/// default (image-free) math renderer. Results are cached per expression.
/// How a Unicode-rendered math expression should be drawn.
#[derive(Debug, Clone)]
pub enum MathRender {
    /// A plain (possibly multi-line) text representation.
    Text(String),
    /// A matrix / cases environment laid out as a grid with bracket glyphs.
    Matrix {
        rows: Vec<Vec<String>>,
        col_widths: Vec<usize>,
        kind: MatrixKind,
    },
}

/// Render LaTeX as a [`MathRender`], preserving matrices as structured grid
/// data so the UI can lay them out with aligned borders.
pub fn render_math_unicode_ast(latex: &str) -> MathRender {
    if let Ok(cache) = UNICODE_MATH_CACHE.lock() {
        if let Some(cached) = cache.map.get(latex) {
            return cached.clone();
        }
    }

    let parts = preprocess_latex(latex);
    // Convert LaTeX symbol/accents/superscripts that `unicodeit` knows about
    // (e.g. \alpha -> α, x^2 -> x², \infty -> ∞). Matrix cells are already
    // converted because they recurse through here via `render_cell`.
    let parts: Vec<MathPart> = parts
        .into_iter()
        .map(|p| match p {
            MathPart::Text(t) => MathPart::Text(convert_math_text(&t)),
            other => other,
        })
        .collect();
    // A whole-expression matrix is surfaced structurally; anything mixed
    // (e.g. text with an inline matrix) is flattened to box-art text.
    let render = if parts.len() == 1 {
        match &parts[0] {
            MathPart::Text(t) => MathRender::Text(t.clone()),
            MathPart::Matrix {
                rows,
                col_widths,
                kind,
            } => MathRender::Matrix {
                rows: rows.clone(),
                col_widths: col_widths.clone(),
                kind: *kind,
            },
        }
    } else {
        MathRender::Text(flatten(parts))
    };

    if let Ok(mut cache) = UNICODE_MATH_CACHE.lock() {
        cache.map.insert(latex.to_string(), render.clone());
        cache.order.push_back(latex.to_string());
        while cache.order.len() > UNICODE_MATH_CACHE_CAP {
            if let Some(oldest) = cache.order.pop_front() {
                cache.map.remove(&oldest);
            } else {
                break;
            }
        }
    }
    render
}

/// The bracket style for a matrix environment.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MatrixKind {
    /// `bmatrix` — square brackets
    #[default]
    Bracket,
    /// `Bmatrix` — braces
    Brace,
    /// `pmatrix` — parentheses
    Paren,
    /// `vmatrix` — vertical bars
    Vbar,
    /// `matrix` — no brackets
    None,
}

/// Internal: a piece of preprocessed output (text or a matrix).
#[derive(Clone)]
enum MathPart {
    Text(String),
    Matrix {
        rows: Vec<Vec<String>>,
        col_widths: Vec<usize>,
        kind: MatrixKind,
    },
}

/// Brace-aware rewrites for constructs `unicodeit` leaves as raw LaTeX.
/// Handles `\frac`/`\sqrt` (recursively) and matrix environments. Everything
/// else passes through untouched. Returns a list of parts so matrices can be
/// laid out structurally by the UI instead of as pre-formatted text.
fn preprocess_latex(latex: &str) -> Vec<MathPart> {
    let chars: Vec<char> = latex.chars().collect();
    preprocess_inner(&chars, 0, chars.len())
}

/// Recursive worker over the char slice `[start, end)`. Recurses into the
/// arguments of `\frac`/`\sqrt`/`\begin` so nested constructs are rewritten
/// before `unicodeit` sees them.
fn preprocess_inner(chars: &[char], start: usize, end: usize) -> Vec<MathPart> {
    let mut parts: Vec<MathPart> = Vec::new();
    let mut text = String::new();
    let mut i = start;
    while i < end {
        // Superscript/subscript operators: consume their argument (a group, a
        // single token, or a `\command`) and emit it as Unicode super/sub
        // script characters so the caret never leaks into the output.
        if chars[i] == '^' || chars[i] == '_' {
            let sup = chars[i] == '^';
            if let Some((cs, ce, skip)) = read_token(chars, i + 1) {
                let arg = flatten(preprocess_inner(chars, cs, ce));
                let converted = convert_math_text(&arg);
                let mut mapped = String::new();
                // Keep the caret when the script contains a letter that has no
                // Unicode sub/superscript variant, so it isn't misread as a
                // product (e.g. `ω_β₀`). Drop it when everything converts
                // (e.g. `xⁿ⁺¹`) or it's a non-letter symbol (e.g. `∫₀∞`).
                let mut needs_caret = false;
                for ch in converted.chars() {
                    let m = map_script(ch, sup);
                    if ch.is_alphabetic() && m == ch {
                        needs_caret = true;
                    }
                    mapped.push(m);
                }
                if needs_caret {
                    // Use a private-use sentinel so the later `unicodeit` pass
                    // does not re-merge the caret into a script variant.
                    text.push(if sup { SUP_MARK } else { SUB_MARK });
                }
                text.push_str(&mapped);
                i = skip;
            } else {
                i += 1;
            }
            continue;
        }
        if chars[i] == '\\' && i + 1 < end {
            // Line break / row separator: render as a space in inline math.
            if chars[i + 1] == '\\' {
                text.push(' ');
                i += 2;
                continue;
            }
            let cmd_start = i + 1;
            let mut j = cmd_start;
            while j < end && chars[j].is_alphabetic() {
                j += 1;
            }
            let cmd: String = chars[cmd_start..j].iter().collect();
            if matches!(cmd.as_str(), "frac" | "dfrac" | "tfrac") {
                if let Some((ns, ne)) = read_group(chars, j) {
                    if let Some((ds, de)) = read_group(chars, ne + 1) {
                        let num = flatten(preprocess_inner(chars, ns, ne));
                        let den = flatten(preprocess_inner(chars, ds, de));
                        flush(&mut text, &mut parts);
                        parts.push(MathPart::Text(format!("{}/{}", num, den)));
                        i = de + 1;
                        continue;
                    }
                }
            } else if cmd == "sqrt" {
                let mut k = j;
                let mut root = String::new();
                if k < end && chars[k] == '[' {
                    if let Some((rs, re)) = read_bracket(chars, k) {
                        root = flatten(preprocess_inner(chars, rs, re));
                        k = re;
                    }
                }
                if let Some((bs, be)) = read_group(chars, k) {
                    let body = flatten(preprocess_inner(chars, bs, be));
                    flush(&mut text, &mut parts);
                    if root.is_empty() {
                        parts.push(MathPart::Text(format!("√({})", body)));
                    } else {
                        parts.push(MathPart::Text(format!("{}(√({}))", root, body)));
                    }
                    i = be + 1;
                    continue;
                }
            } else if matches!(
                cmd.as_str(),
                "text"
                    | "textbf"
                    | "textit"
                    | "mathrm"
                    | "mathbf"
                    | "mathit"
                    | "mathsf"
                    | "texttt"
                    | "textrm"
                    | "textsf"
                    | "textmd"
                    | "mathnormal"
                    | "operatorname"
            ) {
                // Drop the command name and render just its (math) argument.
                if chars.get(j) == Some(&'{') {
                    if let Some((gs, ge)) = read_group(chars, j) {
                        let inner = flatten(preprocess_inner(chars, gs, ge));
                        text.push_str(&convert_math_text(&inner));
                        i = ge + 1;
                        continue;
                    }
                }
                i = j;
                continue;
            } else if matches!(
                cmd.as_str(),
                "lim"
                    | "limsup"
                    | "liminf"
                    | "max"
                    | "min"
                    | "deg"
                    | "sup"
                    | "inf"
                    | "gcd"
                    | "det"
                    | "ker"
                    | "dim"
                    | "Pr"
                    | "arg"
                    | "hom"
                    | "id"
                    | "bmod"
            ) {
                // Operator names: emit the word (roman in real TeX).
                text.push_str(&cmd);
                i = j;
                continue;
            } else if cmd == "xrightarrow" || cmd == "xleftarrow" {
                text.push(if cmd == "xrightarrow" { '→' } else { '←' });
                if chars.get(j) == Some(&'{') {
                    if let Some((gs, ge)) = read_group(chars, j) {
                        let inner = flatten(preprocess_inner(chars, gs, ge));
                        let converted = convert_math_text(&inner);
                        for ch in converted.chars() {
                            text.push(map_script(ch, true));
                        }
                        i = ge + 1;
                        continue;
                    }
                }
                i = j;
                continue;
            } else if cmd == "begin" {
                if let Some((es, ee)) = read_group(chars, j) {
                    let env: String = chars[es..ee].iter().collect();
                    if let Some(kind) = classify_matrix(&env) {
                        if let Some((bs, be)) = read_env_body(chars, ee + 1, &env) {
                            let needle: String = format!("\\end{{{}}}", env);
                            let body_end = be - needle.chars().count();
                            flush(&mut text, &mut parts);
                            parts.push(build_matrix(chars, bs, body_end, kind));
                            i = be;
                            continue;
                        }
                    }
                }
            }
            flush(&mut text, &mut parts);
            if cmd.is_empty() {
                // Escaped literal (e.g. `\&`, `\#`, `\$`, `\_`, `\{`, `\}`, `\ `).
                // Emit the literal character instead of leaking the backslash.
                if let Some(c) = chars.get(cmd_start) {
                    match c {
                        '&' | '#' | '%' | '$' | '_' | '{' | '}' | ' ' => {
                            text.push(*c);
                            i = cmd_start + 1;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            text.push('\\');
            text.push_str(&cmd);
            i = j;
            continue;
        }
        text.push(chars[i]);
        i += 1;
    }
    flush(&mut text, &mut parts);
    parts
}

/// Flushes the accumulated text buffer into a `Text` part (if non-empty).
fn flush(text: &mut String, parts: &mut Vec<MathPart>) {
    if !text.is_empty() {
        let t = std::mem::take(text);
        parts.push(MathPart::Text(t));
    }
}

/// Collapses a part list into a single string (matrices become box-art text).
/// Used for inline contexts and the cached string API.
fn flatten(parts: Vec<MathPart>) -> String {
    parts
        .into_iter()
        .map(|p| match p {
            MathPart::Text(t) => t,
            MathPart::Matrix {
                rows,
                col_widths,
                kind,
            } => matrix_to_string(&rows, &col_widths, kind),
        })
        .collect()
}

/// Converts a raw LaTeX text fragment to Unicode: `unicodeit` handles most
/// symbols/accents/simple scripts, then we fix any remaining `^`/`_` markers
/// that `unicodeit` leaves behind (e.g. `^\infty` -> `∞`, `^{n+1}` -> `ⁿ⁺¹`).
/// Private-use sentinels marking a sub/superscript whose caret was kept to
/// disambiguate from multiplication (e.g. `ω_β₀`). They survive the
/// `unicodeit` pass (which would otherwise re-merge the caret into a script
/// variant) and are restored to literal `_`/`^` at the very end.
const SUB_MARK: char = '\u{E000}';
const SUP_MARK: char = '\u{E001}';

fn convert_math_text(t: &str) -> String {
    let s = apply_scripts(&unicodeit::replace(t));
    // Restore forced-caret sentinels to real carets for the final text.
    s.replace(SUP_MARK, "^").replace(SUB_MARK, "_")
}

/// `unicodeit` only superscripts content that maps to a precomposed character
/// (e.g. `^2` -> `²`), leaving a literal `^` when it can't (e.g. `^\infty`).
/// This walks the string and converts trailing `^`/`_` runs into Unicode
/// superscripts/subscripts, dropping the caret when no variant exists.
fn apply_scripts(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '^' || c == '_') && i + 1 < chars.len() {
            let sup = c == '^';
            let (content, next) = if chars[i + 1] == '{' {
                let mut depth = 0i64;
                let mut k = i + 1;
                while k < chars.len() {
                    match chars[k] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                (chars[i + 2..k].iter().collect::<String>(), k + 1)
            } else {
                (chars[i + 1..=i + 1].iter().collect::<String>(), i + 2)
            };
            for ch in content.chars() {
                out.push(map_script(ch, sup));
            }
            i = next;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Map a character to its superscript (`sup == true`) or subscript variant.
/// When no variant exists the character is kept as-is (the caret is dropped).
fn map_script(ch: char, sup: bool) -> char {
    if sup {
        match ch {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            'a' => 'ᵃ',
            'b' => 'ᵇ',
            'c' => 'ᶜ',
            'd' => 'ᵈ',
            'e' => 'ᵉ',
            'f' => 'ᶠ',
            'g' => 'ᵍ',
            'h' => 'ʰ',
            'i' => 'ⁱ',
            'j' => 'ʲ',
            'k' => 'ᵏ',
            'l' => 'ˡ',
            'm' => 'ᵐ',
            'n' => 'ⁿ',
            'o' => 'ᵒ',
            'p' => 'ᵖ',
            'r' => 'ʳ',
            's' => 'ˢ',
            't' => 'ᵗ',
            'u' => 'ᵘ',
            'v' => 'ᵛ',
            'w' => 'ʷ',
            'x' => 'ˣ',
            'y' => 'ʸ',
            'z' => 'ᶻ',
            'A' => 'ᴬ',
            'B' => 'ᴮ',
            'D' => 'ᴰ',
            'E' => 'ᴱ',
            'G' => 'ᴳ',
            'H' => 'ᴴ',
            'I' => 'ᴵ',
            'J' => 'ᴶ',
            'K' => 'ᴷ',
            'L' => 'ᴸ',
            'M' => 'ᴹ',
            'N' => 'ᴺ',
            'O' => 'ᴼ',
            'P' => 'ᴾ',
            'R' => 'ᴿ',
            'T' => 'ᵀ',
            'U' => 'ᵁ',
            'V' => 'ⱽ',
            'W' => 'ᵂ',
            '+' => '⁺',
            '-' => '⁻',
            '\u{2212}' => '⁻',
            '=' => '⁼',
            '(' => '⁽',
            ')' => '⁾',
            '<' => '˂',
            '>' => '˃',
            _ => ch,
        }
    } else {
        match ch {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            'a' => 'ₐ',
            'e' => 'ₑ',
            'o' => 'ₒ',
            'x' => 'ₓ',
            'h' => 'ₕ',
            'k' => 'ₖ',
            'l' => 'ₗ',
            'm' => 'ₘ',
            'n' => 'ₙ',
            'p' => 'ₚ',
            's' => 'ₛ',
            't' => 'ₜ',
            '+' => '₊',
            '-' => '₋',
            '\u{2212}' => '₋',
            '=' => '₌',
            '(' => '₍',
            ')' => '₎',
            _ => ch,
        }
    }
}

/// Reads the argument of a `^`/`_` operator starting at `idx`: a `{...}` group,
/// a `\command`, or a single character. Returns `(content_start, content_end,
/// skip_to)` where `[content_start, content_end)` is the argument text (without
/// surrounding braces) and `skip_to` is the index to resume scanning after.
fn read_token(chars: &[char], idx: usize) -> Option<(usize, usize, usize)> {
    if chars.get(idx) == Some(&'{') {
        if let Some((s, e)) = read_group(chars, idx) {
            // s points just inside `{`, e is the closing `}` index.
            return Some((s, e, e + 1));
        }
        return None;
    }
    if chars.get(idx) == Some(&'\\') {
        let mut j = idx + 1;
        while j < chars.len() && (chars[j].is_alphabetic() || chars[j] == '_') {
            j += 1;
        }
        return Some((idx, j, j));
    }
    if idx < chars.len() {
        return Some((idx, idx + 1, idx + 1));
    }
    None
}

/// Reads a `{...}` group starting at `idx` (must point at `{`).
/// Returns the (inner_start, inner_end) range of the content between braces,
/// properly handling nested braces.
fn read_group(chars: &[char], idx: usize) -> Option<(usize, usize)> {
    if chars.get(idx) != Some(&'{') {
        return None;
    }
    let mut depth: i64 = 1;
    let mut i = idx + 1;
    let inner_start = i;
    while i < chars.len() {
        match chars[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((inner_start, i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Reads a `[...]` group starting at `idx` (must point at `[`).
/// Returns the (inner_start, inner_end) range of the content.
fn read_bracket(chars: &[char], idx: usize) -> Option<(usize, usize)> {
    if chars.get(idx) != Some(&'[') {
        return None;
    }
    let mut i = idx + 1;
    let inner_start = i;
    while i < chars.len() {
        if chars[i] == ']' {
            return Some((inner_start, i + 1));
        }
        i += 1;
    }
    None
}

/// Returns the body range between `\begin{name}` (whose group ends at
/// `env_end`) and the matching `\end{name}`. The returned end is just past
/// the `\end{name}` command so the caller can skip it.
fn read_env_body(chars: &[char], env_end: usize, name: &str) -> Option<(usize, usize)> {
    let s: String = chars[env_end..].iter().collect();
    let needle = format!("\\end{{{}}}", name);
    if let Some(pos) = s.find(&needle) {
        Some((env_end, env_end + pos + needle.chars().count()))
    } else {
        None
    }
}

fn classify_matrix(env: &str) -> Option<MatrixKind> {
    match env {
        "bmatrix" => Some(MatrixKind::Bracket),
        "Bmatrix" => Some(MatrixKind::Brace),
        "pmatrix" => Some(MatrixKind::Paren),
        "vmatrix" => Some(MatrixKind::Vbar),
        "matrix" => Some(MatrixKind::None),
        _ => None,
    }
}

/// Parses a matrix/cases body into `MathPart::Matrix`, splitting rows on `\\`
/// and cells on `&` at depth 0 (respecting nested braces and environments),
/// then aligning columns by display width.
fn build_matrix(chars: &[char], start: usize, end: usize, kind: MatrixKind) -> MathPart {
    let mut rows: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut cur: Vec<(usize, usize)> = Vec::new();
    let mut cell_start = start;
    let mut brace = 0i64;
    let mut env = 0i64;
    let mut i = start;
    while i < end {
        // Row separator: \\ (only at depth 0)
        if chars[i] == '\\' && i + 1 < end && chars[i + 1] == '\\' && brace == 0 && env == 0 {
            cur.push((cell_start, i));
            rows.push(std::mem::take(&mut cur));
            cell_start = i + 2;
            i += 2;
            continue;
        }
        // \begin{...} / \end{...} adjust environment depth and are skipped
        if chars[i] == '\\' && i + 5 < end {
            let w: String = chars[i + 1..i + 6].iter().collect();
            if w == "begin" || w == "end" {
                let mut k = i + 6;
                while k < end && chars[k] != '{' {
                    k += 1;
                }
                let mut d = 0i64;
                while k < end {
                    match chars[k] {
                        '{' => d += 1,
                        '}' => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                if w == "begin" {
                    env += 1;
                } else {
                    env -= 1;
                }
                i = k + 1;
                continue;
            }
        }
        match chars[i] {
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            '&' if brace == 0 && env == 0 => {
                cur.push((cell_start, i));
                cell_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    cur.push((cell_start, i));
    rows.push(cur);
    // Drop fully-empty trailing rows (e.g. a dangling row separator)
    rows.retain(|r| !(r.len() == 1 && r[0].0 >= r[0].1));

    if rows.is_empty() {
        return MathPart::Text(String::new());
    }

    let mut grid: Vec<Vec<String>> = Vec::new();
    let mut colw: Vec<usize> = Vec::new();
    for row in &rows {
        let mut grow = Vec::new();
        for (ci, &(cs, ce)) in row.iter().enumerate() {
            let cell =
                convert_math_text(&flatten(preprocess_inner(chars, cs, ce)).trim()).to_string();
            let w = unicode_width::UnicodeWidthStr::width(cell.as_str());
            if ci >= colw.len() {
                colw.push(0);
            }
            colw[ci] = colw[ci].max(w);
            grow.push(cell);
        }
        grid.push(grow);
    }

    MathPart::Matrix {
        rows: grid,
        col_widths: colw,
        kind,
    }
}

/// Renders a parsed matrix as a multiline box-art string (used for inline
/// contexts and the string API). The structured `MathRender::Matrix` variant
/// is preferred by the UI for pixel-aligned borders.
fn matrix_to_string(rows: &[Vec<String>], col_widths: &[usize], kind: MatrixKind) -> String {
    let nrows = rows.len();
    let (l_open, l_mid, l_close, r_open, r_mid, r_close) = bracket_glyphs(kind);

    let mut res = String::new();
    for (ri, row) in rows.iter().enumerate() {
        let left = if ri == 0 {
            l_open.clone()
        } else if ri == nrows - 1 {
            l_close.clone()
        } else {
            l_mid.clone()
        };
        let right = if ri == 0 {
            r_open.clone()
        } else if ri == nrows - 1 {
            r_close.clone()
        } else {
            r_mid.clone()
        };
        res.push_str(&left);
        res.push(' ');
        for ci in 0..col_widths.len() {
            let cell = row.get(ci).map(|s| s.as_str()).unwrap_or("");
            let w = col_widths[ci];
            let pad = w.saturating_sub(unicode_width::UnicodeWidthStr::width(cell));
            res.push_str(cell);
            for _ in 0..pad {
                res.push(' ');
            }
            if ci + 1 < col_widths.len() {
                res.push(' ');
            }
        }
        res.push(' ');
        res.push_str(&right);
        if ri + 1 < nrows {
            res.push('\n');
        }
    }
    res
}

/// Returns the (open, middle, close) bracket glyphs for each side, mirroring
/// LaTeX's matrix environment brackets.
pub(crate) fn bracket_glyphs(kind: MatrixKind) -> (String, String, String, String, String, String) {
    match kind {
        MatrixKind::Bracket => (
            "⎡".into(),
            "⎢".into(),
            "⎣".into(),
            "⎤".into(),
            "⎥".into(),
            "⎦".into(),
        ),
        MatrixKind::Paren => (
            "⎛".into(),
            "⎜".into(),
            "⎝".into(),
            "⎞".into(),
            "⎟".into(),
            "⎠".into(),
        ),
        MatrixKind::Brace => (
            "⎧".into(),
            "⎨".into(),
            "⎩".into(),
            "⎫".into(),
            "⎬".into(),
            "⎭".into(),
        ),
        MatrixKind::Vbar => (
            "|".into(),
            "|".into(),
            "|".into(),
            "|".into(),
            "|".into(),
            "|".into(),
        ),
        MatrixKind::None => (
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        ),
    }
}

/// Maximum number of Unicode-rendered math expressions to keep cached. High
/// enough to hold every distinct expression in a large document.
/// — scrolling, typing, re-layout — hit the cache instead of re-converting.
const UNICODE_MATH_CACHE_CAP: usize = 8192;

/// Bounded FIFO cache for Unicode math conversions. Evicts the oldest entry
/// one at a time when over capacity; it never wipes the whole cache.
struct UnicodeMathCache {
    map: HashMap<String, MathRender>,
    order: VecDeque<String>,
}

static UNICODE_MATH_CACHE: LazyLock<Mutex<UnicodeMathCache>> = LazyLock::new(|| {
    Mutex::new(UnicodeMathCache {
        map: HashMap::new(),
        order: VecDeque::new(),
    })
});

pub fn render_math(
    latex: &str,
    display: bool,
    max_width: u32,
    dark_theme: bool,
) -> Result<(Vec<u8>, u32, u32)> {
    let mut hasher = DefaultHasher::new();
    latex.hash(&mut hasher);
    display.hash(&mut hasher);
    max_width.hash(&mut hasher);
    dark_theme.hash(&mut hasher);
    let cache_key = hasher.finish();

    if let Some(ImageData::Png(data, w, h)) = MATH_CACHE.get(cache_key) {
        return Ok((data, w, h));
    }

    let typst_math = latex_to_typst(latex);

    let source = build_typst_source(&typst_math, display, dark_theme);

    let document = compile_typst(&source)?;

    let svg = typst_svg::svg(&document.pages[0]);
    let result = rasterize_svg_to_png(&svg, max_width)?;
    MATH_CACHE.insert(
        cache_key,
        ImageData::Png(result.0.clone(), result.1, result.2),
    );
    Ok(result)
}

fn build_typst_source(typst_math: &str, display: bool, dark_theme: bool) -> String {
    let color = if dark_theme { "white" } else { "black" };
    let size = if display { "16pt" } else { "14pt" };
    let margin = if display { "12pt" } else { "4pt" };
    let block = if display { "true" } else { "false" };

    format!(
        "{MITEX_PRELUDE}\n\
         #set page(width: auto, height: auto, margin: {margin}, fill: none)\n\
         #set text(font: \"New Computer Modern\", fill: {color}, size: {size})\n\
         #math.equation(block: {block}, ${typst_math}$)"
    )
}

const MITEX_PRELUDE: &str = r#"
#let textmath(body) = text(body)
#let mitexdisplay(body) = body
#let mitexsqrt(first, second: none) = if second == none { math.sqrt(first) } else { math.root(first, second) }
#let mitexoverbrace(body) = math.overbrace(body)
#let mitexunderbrace(body) = math.underbrace(body)
#let mitexcolor(color, body) = body
#let colortext(color, body) = body
#let colorbox(color, body) = body
#let mitexmathbf(body) = math.bold(body)
#let zws = ""
"#;

static FONTS: OnceLock<(FontBook, Vec<Font>)> = OnceLock::new();

struct MathWorld {
    source: Source,
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

impl MathWorld {
    fn new(source_text: &str) -> Self {
        let (book, fonts) = FONTS.get_or_init(load_bundled_fonts);

        Self {
            source: Source::detached(source_text),
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book.clone()),
            fonts: fonts.clone(),
        }
    }
}

impl typst::World for MathWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> typst::diag::FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            Err(typst::diag::FileError::NotFound(
                id.vpath().as_rootless_path().into(),
            ))
        }
    }

    fn file(&self, _: FileId) -> typst::diag::FileResult<Bytes> {
        Err(typst::diag::FileError::NotFound("not found".into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _: Option<i64>) -> Option<typst::foundations::Datetime> {
        None
    }
}

fn compile_typst(source: &str) -> Result<PagedDocument> {
    let world = MathWorld::new(source);
    let result = compile(&world);

    result.output.map_err(|errors| {
        let messages: Vec<String> = errors.iter().map(|e| e.message.to_string()).collect();
        anyhow::anyhow!("typst compilation failed: {}", messages.join("; "))
    })
}

//
// Embedded fonts (portable)
//

static FONT_DEJAVU_MATH: &[u8] = include_bytes!("../assets/fonts/DejaVuMathTeXGyre.ttf");
static FONT_DEJAVU_SANS: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
static FONT_DEJAVU_SANS_BOLD: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");
static FONT_DEJAVU_SANS_MONO: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

fn load_bundled_fonts() -> (FontBook, Vec<Font>) {
    let mut fonts = Vec::new();

    for font_data in [
        FONT_DEJAVU_MATH,
        FONT_DEJAVU_SANS,
        FONT_DEJAVU_SANS_BOLD,
        FONT_DEJAVU_SANS_MONO,
    ] {
        let bytes = Bytes::new(font_data);
        for font in Font::iter(bytes) {
            fonts.push(font);
        }
    }
    for font_data in typst_assets::fonts() {
        let bytes = Bytes::new(font_data);
        for font in Font::iter(bytes) {
            fonts.push(font);
        }
    }

    let book = FontBook::from_fonts(&fonts);
    (book, fonts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_png(data: &[u8], width: u32, height: u32) {
        assert!(data.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(width > 0);
        assert!(height > 0);
    }

    fn assert_has_non_white_or_transparent_pixels(data: &[u8]) {
        let image = image::load_from_memory(data).unwrap().to_rgba8();
        assert!(image.pixels().any(|pixel| {
            let [r, g, b, a] = pixel.0;
            a < 255 || r < 250 || g < 250 || b < 250
        }));
    }

    #[test]
    fn loads_bundled_fonts() {
        let (book, fonts) = load_bundled_fonts();
        let families: Vec<_> = book
            .families()
            .map(|(family, _)| family.to_string())
            .collect();

        assert!(!fonts.is_empty());
        assert!(
            families
                .iter()
                .any(|family| family == "DejaVu Math TeX Gyre")
        );
        assert!(
            families
                .iter()
                .any(|family| family == "New Computer Modern")
        );
    }

    #[test]
    fn renders_common_math_to_png() {
        let (png, width, height) =
            render_math(r"\frac{1}{2} + \sqrt{x}", true, 800, false).expect("math should render");

        assert_png(&png, width, height);
        assert_has_non_white_or_transparent_pixels(&png);
    }

    #[test]
    fn dark_theme_math_uses_transparent_page() {
        let (png, width, height) =
            render_math(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", true, 800, true)
                .expect("math should render");

        assert_png(&png, width, height);
        assert_has_non_white_or_transparent_pixels(&png);
    }

    #[test]
    fn max_width_limits_rendered_png_width() {
        let (png, width, height) = render_math(r"x + y + z + \int_0^1 t^2 dt", true, 40, false)
            .expect("math should render");

        assert_png(&png, width, height);
        assert!(width <= 40);
    }
}

#[cfg(test)]
mod toggle_test {
    use super::*;
    #[test]
    fn toggle_math_mode_works() {
        set_math_mode(MathMode::Unicode);
        assert_eq!(math_mode(), MathMode::Unicode);
        let new = toggle_math_mode();
        assert_eq!(new, MathMode::Image);
        assert_eq!(math_mode(), MathMode::Image);
        let new = toggle_math_mode();
        assert_eq!(new, MathMode::Unicode);
        assert_eq!(math_mode(), MathMode::Unicode);
    }
}
