use std::ops::Range;

use super::model::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Parses a Markdown source string into a [`Document`].
///
/// Enables extensions for math (`$...$`), strikethrough (`~~...~~`),
/// task lists (`- [ ]`), and tables.
pub fn parse_markdown(source: &str) -> Document {
    let options = Options::ENABLE_MATH
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES;
    let parser = Parser::new_ext(source, options);

    let events: Vec<(Event, Range<usize>)> = parser.into_offset_iter().collect();
    let mut pos = 0;
    let blocks = parse_blocks(&events, &mut pos);
    Document { blocks }
}

/// If a list of inlines contains exactly one Image or display-Math element,
/// promote it to the corresponding block-level node. Otherwise wrap in a Paragraph.
fn inlines_to_block(inlines: Vec<Inline>, span: (usize, usize)) -> Block {
    if inlines.len() == 1 {
        let mut inlines = inlines;
        match inlines.remove(0) {
            Inline::Image { alt, url } => {
                return Block::Image {
                    alt,
                    url,
                    title: None,
                    span,
                };
            }
            Inline::Math(m) => {
                return Block::Math {
                    content: m,
                    display: true,
                    span,
                };
            }
            other => {
                return Block::Paragraph {
                    content: vec![other],
                    span,
                };
            }
        }
    }
    Block::Paragraph {
        content: inlines,
        span,
    }
}

// ── Inline termination modes ────────────────────────────────────────────────

/// Controls when `parse_inlines_until` stops collecting.
#[derive(Clone, Copy, PartialEq)]
enum StopCondition {
    /// Stop when an `Event::End(_)` is encountered (and consume it).
    /// Used inside Paragraph, Heading, Strong, Emphasis, Link, etc.
    OnEndTag,
    /// Stop (without consuming) when a non-inline event is encountered.
    /// Used for stray inline events that appear outside any block-level tag.
    OnBlockBoundary,
}

/// Unified inline parser. Collects inline events and returns them.
///
/// - `OnEndTag`: consumes the matching `End` event and returns.
/// - `OnBlockBoundary`: stops (without consuming) when a block-level event is seen.
fn parse_inlines_until(
    events: &[(Event, Range<usize>)],
    pos: &mut usize,
    stop: StopCondition,
) -> Vec<Inline> {
    let mut inlines = Vec::new();
    // Stack of open inline HTML tags. Each entry records where in `inlines`
    // the tag's content begins, the style to apply on close, and the tag name.
    let mut html_stack: Vec<(usize, Option<HtmlStyle>, String)> = Vec::new();
    while *pos < events.len() {
        match &events[*pos].0 {
            // ── Leaf inline events ──────────────────────────────────────
            Event::Text(t) => {
                inlines.push(Inline::Text(t.to_string()));
                *pos += 1;
            }
            Event::Code(c) => {
                inlines.push(Inline::Code(c.to_string()));
                *pos += 1;
            }
            Event::InlineMath(m) | Event::DisplayMath(m) => {
                inlines.push(Inline::Math(m.to_string()));
                *pos += 1;
            }
            Event::SoftBreak => {
                inlines.push(Inline::SoftBreak);
                *pos += 1;
            }
            Event::HardBreak => {
                inlines.push(Inline::HardBreak);
                *pos += 1;
            }
            Event::InlineHtml(html) => {
                handle_html_tag(html.as_ref(), &mut inlines, &mut html_stack);
                *pos += 1;
            }
            Event::FootnoteReference(label) => {
                inlines.push(Inline::FootnoteRef(label.to_string()));
                *pos += 1;
            }

            // ── Nested inline tags (always recurse with OnEndTag) ───────
            Event::Start(Tag::Strong) => {
                *pos += 1;
                inlines.push(Inline::Bold(parse_inlines(events, pos, None)));
            }
            Event::Start(Tag::Emphasis) => {
                *pos += 1;
                inlines.push(Inline::Italic(parse_inlines(events, pos, None)));
            }
            Event::Start(Tag::Strikethrough) => {
                *pos += 1;
                inlines.push(Inline::Strikethrough(parse_inlines(events, pos, None)));
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let url = dest_url.to_string();
                *pos += 1;
                inlines.push(Inline::Link {
                    text: parse_inlines(events, pos, None),
                    url,
                });
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let url = dest_url.to_string();
                *pos += 1;
                let alt_nodes = parse_inlines(events, pos, None);
                inlines.push(Inline::Image {
                    alt: inlines_to_text(&alt_nodes),
                    url,
                });
            }

            // ── Termination ─────────────────────────────────────────────
            Event::End(_) if stop == StopCondition::OnEndTag => {
                *pos += 1;
                return inlines;
            }

            // Any other event — for OnBlockBoundary, stop without consuming;
            // for OnEndTag, skip unknown events.
            _ => {
                if stop == StopCondition::OnBlockBoundary {
                    break;
                }
                *pos += 1;
            }
        }
    }
    inlines
}

// ── HTML tag handling ───────────────────────────────────────────────────────

/// Styling that a supported inline HTML tag should apply to its content.
#[derive(Clone, Copy, PartialEq)]
enum HtmlStyle {
    Bold,
    Italic,
    Underline,
    Strike,
    Sub,
    Sup,
    Code,
}

/// A single parsed HTML tag, e.g. `<b class="x">`, `</b>`, `<br/>`, `<img …>`.
struct HtmlTag {
    name: String,
    closing: bool,
    self_closing: bool,
    attrs: String,
}

/// Map a lowercased tag name to the inline style it applies. Unknown tags get
/// `None`: their content is kept but the tags themselves are stripped.
fn html_style(name: &str) -> Option<HtmlStyle> {
    match name {
        "b" | "strong" => Some(HtmlStyle::Bold),
        "i" | "em" => Some(HtmlStyle::Italic),
        "u" | "ins" => Some(HtmlStyle::Underline),
        "s" | "del" | "strike" => Some(HtmlStyle::Strike),
        "sub" => Some(HtmlStyle::Sub),
        "sup" => Some(HtmlStyle::Sup),
        "code" | "kbd" => Some(HtmlStyle::Code),
        _ => None,
    }
}

/// Wraps a run of inlines in the `Inline` node described by `style`.
fn wrap_html_style(style: HtmlStyle, children: Vec<Inline>) -> Inline {
    match style {
        HtmlStyle::Bold => Inline::Bold(children),
        HtmlStyle::Italic => Inline::Italic(children),
        HtmlStyle::Underline => Inline::Underline(children),
        HtmlStyle::Strike => Inline::Strikethrough(children),
        HtmlStyle::Sub => Inline::Subscript(children),
        HtmlStyle::Sup => Inline::Superscript(children),
        HtmlStyle::Code => Inline::Code(inlines_to_text(&children).trim().to_string()),
    }
}

/// Parses a raw `<…>` tag string into its parts. Returns `None` for comments,
/// processing instructions, CDATA, or otherwise malformed input that should be
/// dropped entirely.
fn parse_html_tag(raw: &str) -> Option<HtmlTag> {
    let r = raw.trim();
    if !r.starts_with('<') || !r.ends_with('>') {
        return None;
    }
    let inner = &r[1..r.len() - 1];
    let mut rest = inner.trim();
    if rest.starts_with('!') || rest.starts_with('?') {
        return None;
    }
    let closing = rest.starts_with('/');
    if closing {
        rest = rest[1..].trim_start();
    }
    let mut name = String::new();
    for c in rest.chars() {
        if c.is_ascii_alphanumeric() || c == ':' || c == '-' {
            name.push(c.to_ascii_lowercase());
        } else {
            break;
        }
    }
    if name.is_empty() {
        return None;
    }
    let mut attrs = rest[name.len()..].trim();
    let self_closing = attrs.ends_with('/');
    attrs = attrs.trim_end_matches('/').trim();
    Some(HtmlTag {
        name,
        closing,
        self_closing,
        attrs: attrs.to_string(),
    })
}

/// Handles a single inline HTML tag against a linear inline stream with a
/// stack of open tags. Each open tag records the inline index its content
/// begins at; the matching close tag wraps that range into a styled node.
/// Unknown tags (style `None`) are transparent: opening records a stack entry
/// and closing just pops it, leaving the content inline.
///
/// Returns `true` when the tag emits a hard line break (`<br>`), so fragment
/// callers can collapse whitespace at the start of the following line.
fn handle_html_tag(
    tag_raw: &str,
    inlines: &mut Vec<Inline>,
    stack: &mut Vec<(usize, Option<HtmlStyle>, String)>,
) -> bool {
    let Some(tag) = parse_html_tag(tag_raw) else {
        return false;
    };
    if tag.closing {
        if let Some((_, _, top_name)) = stack.last()
            && *top_name == tag.name
        {
            let (marker, style, _) = stack.pop().unwrap();
            if let Some(style) = style {
                let children: Vec<Inline> = inlines.drain(marker..).collect();
                inlines.push(wrap_html_style(style, children));
            }
        }
        return false;
    }
    match tag.name.as_str() {
        "br" => {
            inlines.push(Inline::HardBreak);
            return true;
        }
        "img" => {
            if let Some(url) = extract_attr(&tag.attrs, "src") {
                let alt = extract_attr(&tag.attrs, "alt").unwrap_or_default();
                inlines.push(Inline::Image { alt, url });
            }
            return false;
        }
        "hr" => return false,
        _ => {}
    }
    if tag.self_closing {
        return false;
    }
    stack.push((inlines.len(), html_style(&tag.name), tag.name));
    false
}

/// Pulls the value of a named attribute out of a tag's attribute string.
fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let low = attrs.to_lowercase();
    let mut idx = 0;
    let bytes = low.as_bytes();
    while idx < bytes.len() {
        while idx < bytes.len() && (bytes[idx].is_ascii_whitespace() || bytes[idx] == b'/') {
            idx += 1;
        }
        let start = idx;
        while idx < bytes.len() && (bytes[idx].is_ascii_alphanumeric() || bytes[idx] == b'-') {
            idx += 1;
        }
        if idx == start {
            idx += 1;
            continue;
        }
        let attr_name = &low[start..idx];
        idx += bytes[idx..]
            .iter()
            .take_while(|c| c.is_ascii_whitespace())
            .count();
        if attr_name == name && bytes.get(idx) == Some(&b'=') {
            idx += 1;
            // skip to the value
            while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
                idx += 1;
            }
            if bytes.get(idx) == Some(&b'"') || bytes.get(idx) == Some(&b'\'') {
                let q = bytes[idx];
                let start = idx + 1;
                idx = start;
                while idx < bytes.len() && bytes[idx] != q {
                    idx += 1;
                }
                return Some(decode_entities(&attrs[start..idx]));
            } else {
                let start = idx;
                while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() && bytes[idx] != b'>' {
                    idx += 1;
                }
                return Some(decode_entities(&attrs[start..idx]));
            }
        }
        // skip past this attribute's value before scanning the next one
        while idx < bytes.len() && bytes[idx] != b'=' && !bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if bytes.get(idx) == Some(&b'=') {
            idx += 1;
            if bytes.get(idx) == Some(&b'"') || bytes.get(idx) == Some(&b'\'') {
                let q = bytes[idx];
                idx += 1;
                while idx < bytes.len() && bytes[idx] != q {
                    idx += 1;
                }
                idx += 1;
            } else {
                while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
            }
        }
    }
    None
}

/// Decodes the small set of HTML entities that commonly appear in short
/// snippets. Anything unrecognized is left as-is.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{A0}'),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Parses a raw HTML fragment string (e.g. the content of an HTML block) into
/// inline Markdown nodes, applying the supported formatting tags and stripping
/// everything else so the inner text remains visible.
pub fn parse_html_fragment(raw: &str) -> Vec<Inline> {
    let mut inlines = Vec::new();
    let mut stack: Vec<(usize, Option<HtmlStyle>, String)> = Vec::new();
    let mut at_line_start = true;
    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        push_fragment_text(&mut inlines, &rest[..lt], &mut at_line_start);
        let after = &rest[lt..];
        match after.find('>') {
            Some(gt) => {
                let tag_raw = &after[..=gt];
                rest = &after[gt + 1..];
                if handle_html_tag(tag_raw, &mut inlines, &mut stack) {
                    at_line_start = true;
                }
            }
            None => {
                push_fragment_text(&mut inlines, after, &mut at_line_start);
                break;
            }
        }
    }
    if !rest.is_empty() {
        push_fragment_text(&mut inlines, rest, &mut at_line_start);
    }
    trim_fragment_edges(inlines)
}

/// Drops leading/trailing whitespace-only text and line breaks that HTML block
/// formatting leaves around block-level tags such as `<div>` or `<center>`,
/// and trims stray edge whitespace from the first/last text nodes.
fn trim_fragment_edges(mut inlines: Vec<Inline>) -> Vec<Inline> {
    let is_padding = |i: &Inline| match i {
        Inline::HardBreak | Inline::SoftBreak => true,
        Inline::Text(t) => t.trim().is_empty(),
        _ => false,
    };
    while inlines.first().map(is_padding).unwrap_or(false) {
        inlines.remove(0);
    }
    while inlines.last().map(is_padding).unwrap_or(false) {
        inlines.pop();
    }
    if let Some(Inline::Text(t)) = inlines.first_mut() {
        *t = t.trim_start().to_string();
    }
    if let Some(Inline::Text(t)) = inlines.last_mut() {
        *t = t.trim_end().to_string();
    }
    inlines
}

/// Appends a decoded HTML text chunk, collapsing runs of whitespace (including
/// newlines) to a single space — HTML block whitespace semantics. Newlines in
/// the raw block therefore do not each become a hard break; only explicit
/// `<br>` tags (handled by `handle_html_tag`) do. A single trailing space is
/// kept so text preceding an inline element stays separated (e.g. `A
/// <b>bold</b>` -> `A bold`), while leading whitespace only becomes a space
/// when the chunk is not at the start of a line (after a `<br>` or fragment
/// start).
fn push_fragment_text(inlines: &mut Vec<Inline>, text: &str, at_line_start: &mut bool) {
    let decoded = decode_entities(text);
    if decoded.is_empty() {
        return;
    }
    let mut current = String::new();
    let mut pending_space = false;
    for c in decoded.chars() {
        if c.is_whitespace() {
            if !current.is_empty() || !*at_line_start {
                pending_space = true;
            }
        } else {
            if pending_space {
                pending_space = false;
                current.push(' ');
            }
            current.push(c);
        }
    }
    if !current.is_empty() && pending_space {
        current.push(' ');
    }
    if !current.is_empty() {
        *at_line_start = false;
        inlines.push(Inline::Text(current));
    }
}

/// Parse inline events until the matching End tag (the standard entry point).
fn parse_inlines(
    events: &[(Event, Range<usize>)],
    pos: &mut usize,
    stop: Option<StopCondition>,
) -> Vec<Inline> {
    if let Some(stop) = stop {
        return parse_inlines_until(events, pos, stop);
    }
    parse_inlines_until(events, pos, StopCondition::OnEndTag)
}

// ── Block-level parsing ─────────────────────────────────────────────────────

/// Parse events into blocks. Advances `pos` past consumed events.
/// Returns when it hits an `End` event (parent closing) or EOF.
fn parse_blocks(events: &[(Event, Range<usize>)], pos: &mut usize) -> Vec<Block> {
    let mut blocks = Vec::new();
    while *pos < events.len() {
        match &events[*pos].0 {
            Event::Start(tag) => {
                let tag_clone = tag.clone();
                let start_offset = events[*pos].1.start;
                *pos += 1;
                if let Some(block) = parse_block_tag(&tag_clone, events, pos, start_offset) {
                    blocks.push(block);
                }
            }
            Event::End(_) => {
                *pos += 1;
                return blocks;
            }
            Event::Rule => {
                let r = events[*pos].1.clone();
                blocks.push(Block::ThematicBreak {
                    span: (r.start, r.end),
                });
                *pos += 1;
            }
            Event::DisplayMath(m) => {
                let r = events[*pos].1.clone();
                blocks.push(Block::Math {
                    content: m.to_string(),
                    display: true,
                    span: (r.start, r.end),
                });
                *pos += 1;
            }
            Event::Html(h) => {
                let r = events[*pos].1.clone();
                blocks.push(Block::Html {
                    content: h.to_string(),
                    span: (r.start, r.end),
                });
                *pos += 1;
            }
            // Stray inline events outside any block tag — gather into a paragraph
            Event::Text(_)
            | Event::Code(_)
            | Event::InlineMath(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::InlineHtml(_) => {
                let start_offset = events[*pos].1.start;
                let (inlines, end_offset) =
                    parse_inlines_with_end(events, pos, Some(StopCondition::OnBlockBoundary));
                blocks.push(inlines_to_block(inlines, (start_offset, end_offset)));
            }
            _ => {
                *pos += 1;
            }
        }
    }
    blocks
}

fn parse_block_tag(
    tag: &Tag,
    events: &[(Event, Range<usize>)],
    pos: &mut usize,
    start_offset: usize,
) -> Option<Block> {
    match tag {
        Tag::Heading { level, .. } => {
            let (inlines, end_offset) = parse_inlines_with_end(events, pos, None);
            Some(Block::Heading {
                level: heading_to_u8(*level),
                content: inlines,
                span: (start_offset, end_offset),
            })
        }
        Tag::Paragraph => {
            let (inlines, end_offset) = parse_inlines_with_end(events, pos, None);
            Some(inlines_to_block(inlines, (start_offset, end_offset)))
        }
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                CodeBlockKind::Fenced(l) if !l.is_empty() => Some(l.to_string()),
                _ => None,
            };
            let (code, end_offset) = collect_text_with_end(events, pos);
            let span = (start_offset, end_offset);
            match lang.as_deref() {
                Some("mermaid") => Some(Block::Mermaid { source: code, span }),
                Some("math") => Some(Block::Math {
                    content: code,
                    display: true,
                    span,
                }),
                _ => Some(Block::Code {
                    language: lang,
                    code,
                    span,
                }),
            }
        }
        Tag::BlockQuote(_) => {
            let (children, end_offset) = parse_blocks_with_end(events, pos);
            Some(Block::Quote {
                children,
                span: (start_offset, end_offset),
            })
        }
        Tag::List(start) => Some(parse_list(*start, events, pos, start_offset)),
        Tag::Table(aligns) => Some(parse_table(aligns, events, pos, start_offset)),
        Tag::FootnoteDefinition(label) => {
            let (children, end_offset) = parse_blocks_with_end(events, pos);
            Some(Block::FootnoteDefinition {
                label: label.to_string(),
                children,
                span: (start_offset, end_offset),
            })
        }

        // Inline-level tags that appear at block level (e.g. bare `**bold**`).
        // We must wrap them in the correct inline node before wrapping in a paragraph,
        // otherwise the formatting is lost.
        Tag::Strong => {
            let (children, end_offset) = parse_inlines_with_end(events, pos, None);
            Some(Block::Paragraph {
                content: vec![Inline::Bold(children)],
                span: (start_offset, end_offset),
            })
        }
        Tag::Emphasis => {
            let (children, end_offset) = parse_inlines_with_end(events, pos, None);
            Some(Block::Paragraph {
                content: vec![Inline::Italic(children)],
                span: (start_offset, end_offset),
            })
        }
        Tag::Strikethrough => {
            let (children, end_offset) = parse_inlines_with_end(events, pos, None);
            Some(Block::Paragraph {
                content: vec![Inline::Strikethrough(children)],
                span: (start_offset, end_offset),
            })
        }
        Tag::Link { dest_url, .. } => {
            let url = dest_url.to_string();
            let (children, end_offset) = parse_inlines_with_end(events, pos, None);
            Some(Block::Paragraph {
                content: vec![Inline::Link {
                    text: children,
                    url,
                }],
                span: (start_offset, end_offset),
            })
        }
        Tag::Image { dest_url, .. } => {
            let url = dest_url.to_string();
            let (alt_nodes, end_offset) = parse_inlines_with_end(events, pos, None);
            Some(Block::Image {
                alt: inlines_to_text(&alt_nodes),
                url,
                title: None,
                span: (start_offset, end_offset),
            })
        }
        Tag::HtmlBlock => {
            let (content, end_offset) = collect_html_block(events, pos);
            Some(html_fragment_to_block(content, (start_offset, end_offset)))
        }

        _ => {
            skip_to_end(events, pos);
            None
        }
    }
}

/// Collects every `Html` event that makes up an HTML block (one event per line,
/// including trailing newlines), consuming through the matching `End` event.
fn collect_html_block(events: &[(Event, Range<usize>)], pos: &mut usize) -> (String, usize) {
    let mut s = String::new();
    let mut end_offset = 0;
    while *pos < events.len() {
        match &events[*pos].0 {
            Event::Html(h) => {
                s.push_str(h.as_ref());
                end_offset = events[*pos].1.end;
                *pos += 1;
            }
            Event::End(_) => {
                end_offset = events[*pos].1.end;
                *pos += 1;
                break;
            }
            _ => {
                *pos += 1;
            }
        }
    }
    (s, end_offset)
}

/// True for the block-level HTML tags whose content is treated as verbatim
/// preformatted text. Anything inside clever `/`-free `code` isn't included;
/// the generic wrappers handled are `div` and `p`.
fn is_html_code_wrapper(name: &str) -> bool {
    matches!(name, "pre" | "code" | "samp" | "kbd")
}

/// Reads a language (if any) out of an HTML tag's attributes, following the
/// `class="language-…"` convention used by syntax highlighters.
fn language_from_html_attrs(attrs: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        let Some(ci) = rest.find("class") else {
            return None;
        };
        let after = rest[ci + "class".len()..].trim_start();
        let Some(after_eq) = after.strip_prefix('=') else {
            rest = after;
            continue;
        };
        let after_eq = after_eq.trim_start();
        let mut chars = after_eq.chars();
        let Some(quote) = chars.next() else {
            return None;
        };
        if quote != '"' && quote != '\'' {
            return None;
        }
        let inner = &after_eq[1..];
        let Some(end) = inner.find(quote) else {
            return None;
        };
        for tok in inner[..end].split_whitespace() {
            if let Some(lang) = tok.strip_prefix("language-") {
                if !lang.is_empty() {
                    return Some(lang.to_string());
                }
            }
        }
        return None;
    }
}

/// Recovers a preformatted code block from an HTML fragment that consists
/// solely of balanced `<pre>`/`<code>` tags — optionally wrapped in `<div>` or
/// `<p>` — with no other markup. Returns the verbatim inner text (with
/// newlines and indentation preserved, entities decoded) and the language from
/// the `<code>` tag's `class="language-*"`. Returns `None` for anything that
/// is not a clean preformatted block.
fn html_fragment_to_preformatted(content: &str) -> Option<(String, Option<String>)> {
    let mut rest = content.trim();
    let mut stack: Vec<String> = Vec::new();
    let mut is_code = false;
    let mut language = None;
    loop {
        if !rest.starts_with('<') {
            break;
        }
        let Some(gt) = rest.find('>') else {
            break;
        };
        let tag = parse_html_tag(&rest[..=gt])?;
        if tag.closing || tag.self_closing {
            break;
        }
        if is_html_code_wrapper(&tag.name) {
            is_code = true;
            if tag.name == "code" {
                language = language_from_html_attrs(&tag.attrs);
            }
        } else if tag.name != "div" && tag.name != "p" {
            break;
        }
        stack.push(tag.name.clone());
        rest = &rest[gt + 1..];
    }
    if stack.is_empty() || !is_code {
        return None;
    }
    // The remainder must close the wrappers innermost-first, with only
    // whitespace between consecutive closing tags.
    let mut inner_end = None;
    let mut i = 0usize;
    let mut is_first = true;
    loop {
        if stack.is_empty() {
            break;
        }
        let Some(lt) = rest[i..].find("</") else {
            return None;
        };
        let lt = i + lt;
        if !is_first && !rest[i..lt].chars().all(|c| c.is_whitespace()) {
            return None;
        }
        if is_first {
            inner_end = Some(lt);
        }
        let after = &rest[lt..];
        let Some(gt) = after.find('>') else {
            return None;
        };
        let tag = parse_html_tag(&after[..=gt])?;
        let expected = stack.pop()?;
        if !tag.closing || tag.name != expected {
            return None;
        }
        i = lt + gt + 1;
        is_first = false;
    }
    if !rest[i..].trim().is_empty() {
        return None;
    }
    let inner = &rest[..inner_end?];
    Some((decode_entities(inner.trim()), language))
}

/// Converts an HTML block's raw content into a block. A fragment that consists
/// of a single `<img>` tag is promoted to a [`Block::Image`] so it renders
/// through the image pipeline (Kitty graphics) instead of as plain text. A
/// fragment wrapped in balanced `<pre>`/`<code>` tags becomes a [`Block::Code`]
/// so it renders as a real code block with syntax language support. Anything
/// else keeps its formatting via [`parse_html_fragment`].
fn html_fragment_to_block(content: String, span: (usize, usize)) -> Block {
    let inlines = parse_html_fragment(&content);
    if inlines.len() == 1
        && let Inline::Image { alt, url } = &inlines[0]
    {
        return Block::Image {
            alt: alt.clone(),
            url: url.clone(),
            title: None,
            span,
        };
    }
    if let Some((code, language)) = html_fragment_to_preformatted(&content) {
        return Block::Code {
            language,
            code,
            span,
        };
    }
    Block::Html { content, span }
}

/// Parse a list and its items.
fn parse_list(
    start: Option<u64>,
    events: &[(Event, Range<usize>)],
    pos: &mut usize,
    start_offset: usize,
) -> Block {
    let ordered = start.is_some();
    let mut items = Vec::new();
    let mut end_offset = start_offset;
    while *pos < events.len() {
        match &events[*pos].0 {
            Event::Start(Tag::Item) => {
                *pos += 1;
                let mut checked = None;
                if let Some((Event::TaskListMarker(c), _)) = events.get(*pos) {
                    checked = Some(*c);
                    *pos += 1;
                }
                let content = parse_blocks(events, pos);
                items.push(ListItem { checked, content });
            }
            Event::End(TagEnd::List(_)) => {
                end_offset = events[*pos].1.end;
                *pos += 1;
                break;
            }
            _ => {
                *pos += 1;
            }
        }
    }
    Block::List {
        ordered,
        start,
        items,
        span: (start_offset, end_offset),
    }
}

/// Parse a table (headers + body rows).
fn parse_table(
    aligns: &[pulldown_cmark::Alignment],
    events: &[(Event, Range<usize>)],
    pos: &mut usize,
    start_offset: usize,
) -> Block {
    let alignments: Vec<Alignment> = aligns
        .iter()
        .map(|a| match a {
            pulldown_cmark::Alignment::Left => Alignment::Left,
            pulldown_cmark::Alignment::Right => Alignment::Right,
            pulldown_cmark::Alignment::Center => Alignment::Center,
            pulldown_cmark::Alignment::None => Alignment::None,
        })
        .collect();
    let mut headers = Vec::new();
    let mut rows: Vec<Vec<TableCell>> = Vec::new();
    let mut in_head = false;
    let mut end_offset = start_offset;
    while *pos < events.len() {
        match &events[*pos].0 {
            Event::Start(Tag::TableHead) => {
                in_head = true;
                *pos += 1;
            }
            Event::End(TagEnd::TableHead) => {
                in_head = false;
                *pos += 1;
            }
            Event::Start(Tag::TableRow) => {
                if !in_head {
                    rows.push(Vec::new());
                }
                *pos += 1;
            }
            Event::End(TagEnd::TableRow) => {
                *pos += 1;
            }
            Event::Start(Tag::TableCell) => {
                *pos += 1;
                let inlines = parse_inlines(events, pos, None);
                let cell = TableCell { content: inlines };
                if in_head {
                    headers.push(cell);
                } else if let Some(row) = rows.last_mut() {
                    row.push(cell);
                }
            }
            Event::End(TagEnd::Table) => {
                end_offset = events[*pos].1.end;
                *pos += 1;
                break;
            }
            _ => {
                *pos += 1;
            }
        }
    }
    Block::Table {
        headers,
        alignments,
        rows,
        span: (start_offset, end_offset),
    }
}

// ── Utility functions ───────────────────────────────────────────────────────

/// Collect raw text from events until the matching End tag.
fn collect_text(events: &[(Event, Range<usize>)], pos: &mut usize) -> String {
    let mut s = String::new();
    while *pos < events.len() {
        match &events[*pos].0 {
            Event::Text(t) => {
                s.push_str(t.as_ref());
                *pos += 1;
            }
            Event::End(_) => {
                *pos += 1;
                return s;
            }
            _ => {
                *pos += 1;
            }
        }
    }
    s
}

fn skip_to_end(events: &[(Event, Range<usize>)], pos: &mut usize) {
    let mut depth = 1u32;
    while *pos < events.len() && depth > 0 {
        match &events[*pos].0 {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
        *pos += 1;
    }
}

fn heading_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn parse_inlines_with_end(
    events: &[(Event, Range<usize>)],
    pos: &mut usize,
    stop: Option<StopCondition>,
) -> (Vec<Inline>, usize) {
    let inlines;
    if let Some(stop) = stop {
        inlines = parse_inlines(events, pos, Some(stop));
    } else {
        inlines = parse_inlines(events, pos, None);
    }

    // `pos` now points just past the consumed End event
    let end_offset = if *pos > 0 { events[*pos - 1].1.end } else { 0 };
    (inlines, end_offset)
}

fn parse_blocks_with_end(
    events: &[(Event, std::ops::Range<usize>)],
    pos: &mut usize,
) -> (Vec<Block>, usize) {
    let blocks = parse_blocks(events, pos);
    let end_offset = if *pos > 0 { events[*pos - 1].1.end } else { 0 };
    (blocks, end_offset)
}

fn collect_text_with_end(
    events: &[(Event, std::ops::Range<usize>)],
    pos: &mut usize,
) -> (String, usize) {
    let s = collect_text(events, pos);
    let end_offset = if *pos > 0 { events[*pos - 1].1.end } else { 0 };
    (s, end_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading() {
        let doc = parse_markdown("# Title");
        assert!(matches!(&doc.blocks[0], Block::Heading { level: 1, .. }));
    }

    #[test]
    fn code_block() {
        let doc = parse_markdown("```python\ndef main():\n\t\t\t\tprint('Hello')\n```");
        match &doc.blocks[0] {
            Block::Code {
                language,
                code,
                span,
            } => {
                assert_eq!(language.as_deref(), Some("python"));
                assert!(code.contains("def main()"))
            }
            _ => panic!("Expected a Code Block!"),
        }
    }

    #[test]
    fn mermaid_detection() {
        let doc = parse_markdown("```mermaid\ngraph LR\n A-->B\n```");
        assert!(matches!(&doc.blocks[0], Block::Mermaid { .. }));
    }

    #[test]
    fn display_math() {
        let doc = parse_markdown(
            r#"
$$
e^{i\pi} + 1 = 0
$$
        "#,
        );
        assert!(matches!(&doc.blocks[0], Block::Math { display: true, .. }));
    }

    #[test]
    fn inline_math() {
        let doc = parse_markdown("The equation $x^2$ is famous.");
        if let Block::Paragraph { content, span } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::Math(_))));
            assert_eq!(span, &(0_usize, 29_usize));
        } else {
            panic!("Paragraph is expected!")
        }
    }

    #[test]
    fn image() {
        let doc = parse_markdown("![alt](img.png)");
        assert!(matches!(&doc.blocks[0], Block::Image { .. }));
    }

    #[test]
    fn nested_list() {
        let doc = parse_markdown("- [x] A\n - [ ] B\n - C\n- D");
        assert!(matches!(&doc.blocks[0], Block::List { .. }));
    }

    #[test]
    fn table() {
        let doc = parse_markdown("| A | B |\n|---|---|\n| 1 | 2 |");
        assert!(matches!(&doc.blocks[0], Block::Table { .. }));
    }

    #[test]
    fn bold_paragraph() {
        let doc = parse_markdown("**bold text**");
        if let Block::Paragraph { content, span } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::Bold(_))));
        } else {
            panic!("Expected Paragraph with Bold inline");
        }
    }

    #[test]
    fn nested_formatting() {
        let doc = parse_markdown("***bold italic***");
        if let Block::Paragraph { content, span } = &doc.blocks[0] {
            // pulldown_cmark nests emphasis inside strong (or vice versa)
            let has_nested = content.iter().any(|i| match i {
                Inline::Bold(ch) => ch.iter().any(|c| matches!(c, Inline::Italic(_))),
                Inline::Italic(ch) => ch.iter().any(|c| matches!(c, Inline::Bold(_))),
                _ => false,
            });
            assert!(
                has_nested,
                "Expected nested bold/italic, got: {:?}",
                content
            );
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn link() {
        let doc = parse_markdown("[click here](https://example.com)");
        if let Block::Paragraph { content, span } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::Link { .. })));
        } else {
            panic!("Expected Paragraph with Link");
        }
    }

    #[test]
    fn blockquote() {
        let doc = parse_markdown("> quoted text");
        assert!(matches!(&doc.blocks[0], Block::Quote { .. }));
    }

    #[test]
    fn thematic_break() {
        let doc = parse_markdown("---");
        assert!(matches!(&doc.blocks[0], Block::ThematicBreak { span }));
    }

    #[test]
    fn strikethrough() {
        let doc = parse_markdown("~~deleted~~");
        if let Block::Paragraph { content, span } = &doc.blocks[0] {
            assert!(
                content
                    .iter()
                    .any(|i| matches!(i, Inline::Strikethrough(_)))
            );
        } else {
            panic!("Expected Paragraph with Strikethrough");
        }
    }

    #[test]
    fn inline_html_bold() {
        let doc = parse_markdown("a <b>bold</b> b");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::Bold(_))));
            let text = inlines_to_text(content);
            assert_eq!(text, "a bold b");
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_italic() {
        let doc = parse_markdown("a <i>italic</i> b");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::Italic(_))));
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_underline() {
        let doc = parse_markdown("<u>under</u>");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::Underline(_))));
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_strike() {
        let doc = parse_markdown("<s>gone</s> and <del>del</del>");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(
                content
                    .iter()
                    .any(|i| matches!(i, Inline::Strikethrough(_)))
            );
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_break() {
        let doc = parse_markdown("line1<br>line2");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::HardBreak)));
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_code() {
        let doc = parse_markdown("a <code>x + 1</code> b");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(
                content
                    .iter()
                    .any(|i| matches!(i, Inline::Code(c) if c == "x + 1"))
            );
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_sup_sub() {
        let doc = parse_markdown("H<sub>2</sub>O e<sup>x</sup>");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(i, Inline::Subscript(_))));
            assert!(content.iter().any(|i| matches!(i, Inline::Superscript(_))));
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_nested() {
        let doc = parse_markdown("<b>big <i>and nested</i></b>");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            let has_nested = content.iter().any(|i| match i {
                Inline::Bold(ch) => ch.iter().any(|c| matches!(c, Inline::Italic(_))),
                _ => false,
            });
            assert!(
                has_nested,
                "Expected nested bold/italic, got: {:?}",
                content
            );
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_img() {
        let doc = parse_markdown("see <img src=\"img.png\" alt=\"pic\"> now");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            assert!(content.iter().any(|i| matches!(
                i,
                Inline::Image { alt, url } if alt == "pic" && url == "img.png"
            )));
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn inline_html_unknown_stripped() {
        let doc = parse_markdown("a <span>kept</span> b <!--hidden--> c");
        if let Block::Paragraph { content, .. } = &doc.blocks[0] {
            let text = inlines_to_text(content);
            assert_eq!(text, "a kept b  c");
            let flat = inlines_to_text(content);
            assert!(
                !flat.contains("hidden"),
                "HTML comment should be dropped, got: {}",
                flat
            );
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn html_fragment_block() {
        let inlines = parse_html_fragment("<div>\n<b>Bold</b> and <i>italics</i>\n</div>");
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "Bold and italics");
        assert!(inlines.iter().any(|i| matches!(i, Inline::Bold(_))));
    }

    #[test]
    fn html_fragment_entities() {
        let inlines = parse_html_fragment("a &amp; b &lt;c&gt;");
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "a & b <c>");
    }

    #[test]
    fn html_block_is_not_dropped() {
        let doc = parse_markdown("<div>\n  <b>Bold</b> inside\n</div>\n");
        assert!(
            matches!(&doc.blocks[0], Block::Html { .. }),
            "expected HTML block, got: {:?}",
            doc.blocks[0]
        );
        if let Block::Html { content, .. } = &doc.blocks[0] {
            let inlines = parse_html_fragment(content);
            assert_eq!(inlines_to_text(&inlines), "Bold inside");
            assert!(inlines.iter().any(|i| matches!(i, Inline::Bold(_))));
        }
    }

    #[test]
    fn standalone_html_img_promotes_to_image_block() {
        let doc = parse_markdown("<img src=\"rivas.png\" alt=\"logo\">\n");
        match &doc.blocks[0] {
            Block::Image { alt, url, span, .. } => {
                assert_eq!(url, "rivas.png");
                assert_eq!(alt, "logo");
                assert_eq!(span, &(0, 33));
            }
            other => panic!("Expected Block::Image, got: {:?}", other),
        }
    }

    #[test]
    fn self_closing_html_img_promotes_to_image_block() {
        let doc = parse_markdown("<img src=\"rivas.png\"/>\n");
        match &doc.blocks[0] {
            Block::Image { url, .. } => assert_eq!(url, "rivas.png"),
            other => panic!("Expected Block::Image, got: {:?}", other),
        }
    }

    #[test]
    fn html_block_follows_paragraph() {
        let doc = parse_markdown("<div>hi</div>\n\nnext para\n");
        assert_eq!(
            doc.blocks.len(),
            2,
            "expected 2 blocks, got: {:?}",
            doc.blocks
        );
        assert!(matches!(&doc.blocks[0], Block::Html { .. }));
        assert!(matches!(&doc.blocks[1], Block::Paragraph { .. }));
    }

    #[test]
    fn html_code_block_promotes_to_code_block() {
        let doc = parse_markdown("<pre><code>\nfn main() {}\n</code></pre>\n");
        match &doc.blocks[0] {
            Block::Code { language, code, .. } => {
                assert_eq!(code, "fn main() {}");
                assert_eq!(language, &None);
            }
            other => panic!("Expected Block::Code, got: {:?}", other),
        }
    }

    #[test]
    fn html_code_block_with_language_class() {
        let doc =
            parse_markdown("<pre><code class=\"language-rust\">\nfn main() {}\n</code></pre>\n");
        match &doc.blocks[0] {
            Block::Code { language, code, .. } => {
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(code, "fn main() {}");
            }
            other => panic!("Expected Block::Code, got: {:?}", other),
        }
    }

    #[test]
    fn html_code_block_keeps_newlines() {
        let doc = parse_markdown("<pre><code>\nline one\nline two\n</code></pre>\n");
        match &doc.blocks[0] {
            Block::Code { code, .. } => assert_eq!(code, "line one\nline two"),
            other => panic!("Expected Block::Code, got: {:?}", other),
        }
    }

    #[test]
    fn html_pre_alone_becomes_code_block() {
        let doc = parse_markdown("<pre>\nraw line\n</pre>\n");
        match &doc.blocks[0] {
            Block::Code { code, language, .. } => {
                assert_eq!(code, "raw line");
                assert_eq!(language, &None);
            }
            other => panic!("Expected Block::Code, got: {:?}", other),
        }
    }

    #[test]
    fn html_code_without_pre_becomes_code_block() {
        let doc = parse_markdown("<code>\nhello\n</code>\n");
        match &doc.blocks[0] {
            Block::Code { code, .. } => assert_eq!(code, "hello"),
            other => panic!("Expected Block::Code, got: {:?}", other),
        }
    }

    #[test]
    fn html_div_wrapped_code_block() {
        let doc =
            parse_markdown("<div><pre><code class=\"language-python\">x = 1</code></pre></div>\n");
        match &doc.blocks[0] {
            Block::Code { code, language, .. } => {
                assert_eq!(code, "x = 1");
                assert_eq!(language.as_deref(), Some("python"));
            }
            other => panic!("Expected Block::Code, got: {:?}", other),
        }
    }

    #[test]
    fn html_code_block_decodes_entities() {
        let doc = parse_markdown("<pre><code>a &lt; b &amp;&amp; c &gt; d</code></pre>\n");
        match &doc.blocks[0] {
            Block::Code { code, .. } => assert_eq!(code, "a < b && c > d"),
            other => panic!("Expected Block::Code, got: {:?}", other),
        }
    }

    #[test]
    fn html_div_with_text_stays_html() {
        let doc = parse_markdown("<div>hi</div>\n");
        assert!(
            matches!(&doc.blocks[0], Block::Html { .. }),
            "expected HTML block, got: {:?}",
            doc.blocks[0]
        );
    }

    #[test]
    fn footnote_definition_and_reference_parse() {
        let doc = parse_markdown("Text with note[^1].\n\n[^1]: The note body.\n");
        assert!(matches!(&doc.blocks[0], Block::Paragraph { .. }));
        match &doc.blocks[1] {
            Block::FootnoteDefinition {
                label, children, ..
            } => {
                assert_eq!(label, "1");
                assert_eq!(children.len(), 1);
                assert!(matches!(&children[0], Block::Paragraph { .. }));
            }
            other => panic!("Expected FootnoteDefinition, got: {:?}", other),
        }
        // Reference inside the paragraph becomes an inline marker.
        match &doc.blocks[0] {
            Block::Paragraph { content, .. } => {
                assert!(
                    content
                        .iter()
                        .any(|i| matches!(i, Inline::FootnoteRef(l) if l == "1")),
                    "expected FootnoteRef inline, got: {:?}",
                    content
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn footnote_with_named_label_parses() {
        let doc = parse_markdown("X[^note]\n\n[^note]: body\n");
        match &doc.blocks[1] {
            Block::FootnoteDefinition { label, .. } => assert_eq!(label, "note"),
            other => panic!("Expected FootnoteDefinition, got: {:?}", other),
        }
    }
}
