use std::ops::Range;

use super::model::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

pub fn parse_markdown(source: &str) -> Document {
    let options = Options::ENABLE_MATH
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_TABLES;
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
            Event::InlineHtml(_) => {
                *pos += 1; // ignore
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

        _ => {
            skip_to_end(events, pos);
            None
        }
    }
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
}
