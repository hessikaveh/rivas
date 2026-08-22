/// Block-level elements in a Markdown document.
///
/// Each variant carries a `span: (usize, usize)` recording the byte range
/// `[start, end)` in the original source text.
#[derive(Debug, Clone)]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
        span: (usize, usize),
    },
    Paragraph {
        content: Vec<Inline>,
        span: (usize, usize),
    },
    Code {
        language: Option<String>,
        code: String,
        span: (usize, usize),
    },
    Mermaid {
        source: String,
        span: (usize, usize),
    },
    Math {
        content: String,
        display: bool,
        span: (usize, usize),
    },
    Quote {
        children: Vec<Block>,
        span: (usize, usize),
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
        span: (usize, usize),
    },
    Table {
        headers: Vec<TableCell>,
        alignments: Vec<Alignment>,
        rows: Vec<Vec<TableCell>>,
        span: (usize, usize),
    },
    ThematicBreak {
        span: (usize, usize),
    },
    Image {
        alt: String,
        url: String,
        title: Option<String>,
        span: (usize, usize),
    },
    Html {
        content: String,
        span: (usize, usize),
    },
    /// A footnote definition (`[^label]: text`), rendered at its source
    /// position with its label and nested content.
    FootnoteDefinition {
        label: String,
        children: Vec<Block>,
        span: (usize, usize),
    },
}

/// A single item in a bullet or ordered list.
#[derive(Debug, Clone)]
pub struct ListItem {
    /// Task-list checkbox state: `Some(true)` = checked, `Some(false)` = unchecked, `None` = not a task.
    pub checked: Option<bool>,
    /// Nested blocks contained in this list item.
    pub content: Vec<Block>,
}

/// A single cell in a Markdown table, containing inline content.
#[derive(Debug, Clone)]
pub struct TableCell {
    /// Inline content of the cell.
    pub content: Vec<Inline>,
}

/// Column alignment in a Markdown table.
#[derive(Debug, Clone, Copy)]
pub enum Alignment {
    /// Left-aligned (`:---`).
    Left,
    /// Center-aligned (`:---:`).
    Center,
    /// Right-aligned (`---:`).
    Right,
    /// No alignment specified (`---`).
    None,
}

impl Block {
    /// Returns the byte-range span `(start, end)` of this block in the source text.
    pub fn span(&self) -> (usize, usize) {
        match self {
            Block::Heading { span, .. } => *span,
            Block::Paragraph { span, .. } => *span,
            Block::Code { span, .. } => *span,
            Block::Mermaid { span, .. } => *span,
            Block::Math { span, .. } => *span,
            Block::Quote { span, .. } => *span,
            Block::List { span, .. } => *span,
            Block::Table { span, .. } => *span,
            Block::ThematicBreak { span } => *span,
            Block::Image { span, .. } => *span,
            Block::Html { span, .. } => *span,
            Block::FootnoteDefinition { span, .. } => *span,
        }
    }
}

/// Inline (phrasing) content within a block element.
///
/// Inlines are the leaf nodes of the document tree — text, formatting,
/// links, images, and breaks.
#[derive(Debug, Clone)]
pub enum Inline {
    /// Plain text.
    Text(String),
    /// Bold text (`**...**` or `__...__`).
    Bold(Vec<Inline>),
    /// Italic text (`*...*` or `_..._`).
    Italic(Vec<Inline>),
    /// Strikethrough text (`~~...~~`).
    Strikethrough(Vec<Inline>),
    /// Underlined text (`<u>...</u>` or `<ins>...</ins>`).
    Underline(Vec<Inline>),
    /// Subscript text (`<sub>...</sub>`), rendered as Unicode subscripts.
    Subscript(Vec<Inline>),
    /// Superscript text (`<sup>...</sup>`), rendered as Unicode superscripts.
    Superscript(Vec<Inline>),
    /// Inline code (`` `...` ``).
    Code(String),
    /// Inline math (`$...$`).
    Math(String),
    /// Hyperlink (`[text](url)`).
    Link { text: Vec<Inline>, url: String },
    /// Inline image (`![alt](url)`).
    Image { alt: String, url: String },
    /// A soft line break (single newline in source).
    SoftBreak,
    /// A hard line break (two trailing spaces or `\` at line end).
    HardBreak,
    /// A footnote reference (`[^label]`), rendered as a superscript marker.
    FootnoteRef(String),
}

/// A parsed Markdown document, consisting of a sequence of top-level blocks.
#[derive(Clone)]
pub struct Document {
    /// The ordered list of block-level elements in the document.
    pub blocks: Vec<Block>,
}

/// Flatten a tree of Inlines into a single plain-text string.
/// Used by both the parser (for alt-text / slugs) and the renderer (for link labels / table cells).
pub fn inlines_to_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) => s.push_str(t),
            Inline::Code(c) | Inline::Math(c) => s.push_str(c),
            Inline::Bold(ch)
            | Inline::Italic(ch)
            | Inline::Strikethrough(ch)
            | Inline::Underline(ch)
            | Inline::Subscript(ch)
            | Inline::Superscript(ch) => s.push_str(&inlines_to_text(ch)),
            Inline::Link { text, .. } => s.push_str(&inlines_to_text(text)),
            Inline::FootnoteRef(_) => {}
            Inline::SoftBreak => s.push(' '),
            Inline::HardBreak => s.push('\n'),
            _ => {}
        }
    }
    s
}
