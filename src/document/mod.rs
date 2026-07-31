/// In-memory cache for parsed Markdown documents, keyed by file path.
pub mod cache;
/// Markdown document model: blocks, inlines, and metadata types.
pub mod model;
/// Markdown parser that converts source text into a [`Document`](model::Document).
pub mod parser;
