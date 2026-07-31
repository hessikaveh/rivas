/// In-memory cache for rendered image assets (PNG, SVG rasterizations).
pub mod asset_cache;
/// Image loading, path resolution, and terminal image protocol support.
pub mod images;
/// LaTeX math rendering to Unicode text or images.
pub mod math;
/// Mermaid diagram rendering to PNG images.
pub mod mermaid;
/// SVG to PNG rasterization via `resvg`.
pub mod svg;
