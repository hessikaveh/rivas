use iocraft::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;
use syntect::{easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet};

/// Bundled syntect syntax set (loaded once).
pub static SS: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
/// Bundled syntect theme set (loaded once).
pub static TS: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// The theme used for all syntax highlighting. Follows the active app theme
/// (dark palettes get a dark highlight scheme, light ones a light scheme).
fn highlight_theme() -> &'static syntect::highlighting::Theme {
    if crate::theme::is_dark() {
        &TS.themes["base16-ocean.dark"]
    } else {
        &TS.themes["base16-ocean.light"]
    }
}

/// Highlights each line of `text` under the given syntax token and returns
/// byte-aligned `(text, color)` spans (one vec per logical line).
///
/// Falls back to plain text when the token is unknown and to `fallback` when a
/// line fails to highlight.
pub fn highlight_source(text: &str, token: &str, fallback: Color) -> Vec<Vec<(String, Color)>> {
    let syntax = SS
        .find_syntax_by_token(token)
        .unwrap_or_else(|| SS.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, highlight_theme());

    let mut lines = Vec::new();
    for line in text.lines() {
        match highlighter.highlight_line(line, &SS) {
            Ok(regions) => {
                lines.push(
                    regions
                        .iter()
                        .map(|(style, t)| {
                            let color = Color::Rgb {
                                r: style.foreground.r,
                                g: style.foreground.g,
                                b: style.foreground.b,
                            };
                            (t.to_string(), color)
                        })
                        .collect(),
                );
            }
            Err(_) => {
                lines.push(vec![(line.to_string(), fallback)]);
            }
        }
    }
    lines
}

/// A component hook that caches per-line syntax-highlighted spans, recomputing
/// only when the `(token, text)` pair changes. Call it unconditionally; pass an
/// empty source (or a plain token) when highlighting is not wanted this frame.
pub trait UseHighlight {
    /// Returns syntax-highlighted per-line spans for `text`.
    fn use_cached_highlight(
        &mut self,
        text: &str,
        token: &str,
        fallback: Color,
    ) -> Vec<Vec<(String, Color)>>;
}

impl UseHighlight for Hooks<'_, '_> {
    fn use_cached_highlight(
        &mut self,
        text: &str,
        token: &str,
        fallback: Color,
    ) -> Vec<Vec<(String, Color)>> {
        let new_hash = {
            let mut hasher = DefaultHasher::new();
            token.hash(&mut hasher);
            text.hash(&mut hasher);
            // Recompute when the theme changes (highlight colors follow it).
            crate::theme::index().hash(&mut hasher);
            hasher.finish()
        };

        let mut prev_hash = self.use_ref(|| 0u64);
        let mut cache = self.use_ref(|| Vec::<Vec<(String, Color)>>::new());
        if *prev_hash.read() != new_hash {
            prev_hash.set(new_hash);
            cache.set(highlight_source(text, token, fallback));
        }
        cache.read().clone()
    }
}

/// Convenience for components that only need the base foreground fallback.
pub const MARKDOWN: &str = "markdown";
pub const LATEX: &str = "latex";
pub const HTML: &str = "html";
