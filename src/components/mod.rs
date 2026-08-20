/// Line-wrapping aware rendering of Markdown blocks with cursor overlay for the editor.
pub mod blocks_renderer;
/// Syntax-highlighted fenced code block rendering.
pub mod code_block;
/// Status-bar cursor position indicator with text preview.
pub mod cursor_info;
/// Top-level editor view combining cursor info and the block renderer.
pub mod document;
/// Vim-style modal editor with Normal, Insert, Visual, Command, and Search modes.
pub mod editor;
/// ATX heading rendering (h1–h6) with optional icons.
pub mod heading;
/// Shared syntect syntax highlighting for raw buffer views.
pub mod highlight;
/// Raw HTML passthrough block rendering.
pub mod html_block;
/// Inline image rendering with Kitty/iterm2 protocol support.
pub mod image;
/// Inline Markdown rendering (bold, italic, code, links, etc.).
pub mod inline_renderer;
/// Bullet and numbered list rendering.
pub mod list_block;
/// LaTeX math block rendering (Unicode or image mode).
pub mod math_block;
/// Mermaid diagram block rendering.
pub mod mermaid_block;
/// Paragraph text rendering with inline Markdown.
pub mod paragraph;
/// Blockquote rendering with left-border styling.
pub mod quote_block;
/// Normal-mode raw buffer view with cursor, rendered inside block components.
pub mod raw_buffer;
/// Scroll viewport management and scroll-into-view behavior.
pub mod scroll;
/// Markdown table rendering with alignment support.
pub mod table_block;
/// Horizontal rule (`---`, `***`, `___`) rendering.
pub mod thematic_break;
