use crate::components::highlight::UseHighlight;
use crate::components::raw_buffer::{RawBuffer, RawState};
use crate::theme;
use iocraft::prelude::Color;
use iocraft::prelude::*;

/// Properties for the [`CodeBlock`] component.
#[derive(Default, Props)]
pub struct CodeBlockProps {
    /// Programming language for syntax highlighting (e.g., `"rust"`, `"python"`).
    pub language: Option<String>,
    /// The source code content to render.
    pub code: String,
    /// Optional raw buffer + cursor for the Normal-mode source view.
    pub raw: Option<RawState>,
}

/// Renders a syntax-highlighted fenced code block.
///
/// Uses `syntect` for highlighting; the highlight scheme follows the active
/// app theme (dark palettes get a dark scheme, light ones a light scheme).
/// The language label is displayed in the top-right corner.
#[component]
pub fn CodeBlock(props: &CodeBlockProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let lang_label = props.language.clone().unwrap_or_else(|| "code".to_string());

    let token = props.language.as_deref().unwrap_or("text");
    // Rendered view highlights the code itself with the language token.
    // The Normal-mode raw view shows the full source span (including fences);
    // we keep the *language* highlighting for the code lines and style the
    // fence lines explicitly, so highlight lines pair 1:1 with raw lines
    // (highlight[i] ↔ text[i+1]) and RawBuffer never mispairs them.
    let highlighted = hooks.use_cached_highlight(&props.code, token, theme::fg());

    element! {
        View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2, margin_bottom: 1, background_color: theme::dark_bg()) {
            View() {
                Text(content: lang_label, color: theme::blue())
            }
            #(if props.raw.is_some() {
                let raw = props.raw.clone().map(|mut raw| {
                    // Map highlight lines onto raw text lines: when the span
                    // includes the fences, fence rows get a dim style and code
                    // rows keep the language-colored spans.
                    let lines: Vec<&str> = raw.text.split('\n').collect();
                    let n = lines.len();
                    let fenced = lines.first().map_or(false, |l| l.trim_start().starts_with("```"));
                    let mut hl: Vec<Vec<(String, Color)>> = Vec::with_capacity(n);
                    for (idx, line) in lines.iter().enumerate() {
                        if fenced && (idx == 0 || idx == n - 1) {
                            hl.push(vec![(line.to_string(), theme::comment())]);
                        } else {
                            let h_idx = if fenced { idx - 1 } else { idx };
                            hl.push(
                                highlighted
                                    .get(h_idx)
                                    .cloned()
                                    .unwrap_or_else(|| vec![(line.to_string(), theme::fg())]),
                            );
                        }
                    }
                    raw.highlight = Some(hl);
                    raw
                });
                Some(element! {
                    View(flex_direction: FlexDirection::Column) {
                        RawBuffer(raw: raw, color: theme::fg())
                    }
                }.into_any())
            } else {
                Some(element! {
                    View(flex_direction: FlexDirection::Column) {
                        #(highlighted.iter().map(|line_spans| {
                            element! {
                                View(flex_direction: FlexDirection::Row) {
                                    #(line_spans.iter().map(|(text, color)| {
                                        element! { Text(content: text.clone(), color: *color) }.into_any()
                                    }))
                                }
                            }
                            .into_any()
                        }))
                    }
                }.into_any())
            }.into_iter())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::highlight::{SS, TS};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::Instant;
    use syntect::easy::HighlightLines;

    const SAMPLE_RUST: &str = r#"
fn fibonacci(n: u32) -> u32 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn main() {
    let mut map = HashMap::new();
    for i in 0..10 {
        let val = fibonacci(i);
        map.insert(i, val);
        println!("fib({}) = {}", i, val);
    }

    // Some more code to make it substantial
    let sum: u32 = map.values().sum();
    println!("sum = {}", sum);

    #[cfg(feature = "extra")]
    for (k, v) in &map {
        println!("{} -> {}", k, v);
    }
}
"#;

    /// Simulate the "after" behavior: hash + conditional re-highlight
    fn highlight_cached(
        code: &str,
        language: Option<&str>,
        prev_hash: &mut u64,
    ) -> Vec<Vec<(String, u8, u8, u8)>> {
        let new_hash = {
            let mut hasher = DefaultHasher::new();
            language.hash(&mut hasher);
            code.hash(&mut hasher);
            hasher.finish()
        };

        if *prev_hash != new_hash {
            *prev_hash = new_hash;
            highlight_raw(code, language)
        } else {
            Vec::new() // cache hit — no work
        }
    }

    /// Simulate the "before" behavior: always re-highlight
    fn highlight_raw(code: &str, language: Option<&str>) -> Vec<Vec<(String, u8, u8, u8)>> {
        let syntax = language
            .and_then(|l| SS.find_syntax_by_token(l))
            .unwrap_or_else(|| SS.find_syntax_plain_text());
        let theme = &TS.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();
        for line in code.lines() {
            match highlighter.highlight_line(line, &SS) {
                Ok(regions) => {
                    let spans = regions
                        .iter()
                        .map(|(style, text)| {
                            (
                                text.to_string(),
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            )
                        })
                        .collect();
                    lines.push(spans);
                }
                Err(_) => {
                    lines.push(vec![(line.to_string(), 204, 204, 204)]);
                }
            }
        }
        lines
    }

    /// Simulate the overhead of the cached approach on cache hit (just hash + compare)
    fn hash_only(code: &str, language: Option<&str>) -> u64 {
        let mut hasher = DefaultHasher::new();
        language.hash(&mut hasher);
        code.hash(&mut hasher);
        hasher.finish()
    }

    /// Benchmark: first render (cache miss) — original vs cached
    #[test]
    fn benchmark_first_render() {
        let code = SAMPLE_RUST;
        let language = Some("rust");
        let iterations = 100;

        // "Before" — no caching, always re-highlight
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = highlight_raw(code, language);
        }
        let before = start.elapsed();

        // "After" — cache miss (hash + highlight)
        let start = Instant::now();
        let mut hash = 0u64;
        for i in 0..iterations {
            // Vary content each iteration to force a cache miss
            let content = format!("{}\n// iter {}", code, i);
            let _ = highlight_cached(&content, language, &mut hash);
        }
        let after = start.elapsed();

        eprintln!("=== First render (cache miss) benchmark ===");
        eprintln!(
            "  Before (no cache): {:?} ({:.1} μs/iter)",
            before,
            before.as_nanos() as f64 / iterations as f64 / 1000.0
        );
        eprintln!(
            "  After (cache miss): {:?} ({:.1} μs/iter)",
            after,
            after.as_nanos() as f64 / iterations as f64 / 1000.0
        );
        eprintln!(
            "  Overhead: {:.1}%",
            (after.as_nanos() as f64 / before.as_nanos() as f64 - 1.0) * 100.0
        );
    }

    /// Benchmark: scrolling re-render — original vs cached
    #[test]
    fn benchmark_scroll_rerender() {
        let code = SAMPLE_RUST;
        let language = Some("rust");
        let n_blocks = 50;
        let rerenders = 200; // simulate 200 scroll events

        // Pre-compute cached results
        let mut hashes = vec![0u64; n_blocks];
        for i in 0..n_blocks {
            let _ = highlight_cached(code, language, &mut hashes[i]);
        }

        // "Before" — re-highlight all 50 blocks on every scroll event
        let start = Instant::now();
        for _ in 0..rerenders {
            for _ in 0..n_blocks {
                let _ = highlight_raw(code, language);
            }
        }
        let before = start.elapsed();

        // "After" — cached: just hash comparison per block per scroll
        let start = Instant::now();
        for _ in 0..rerenders {
            for i in 0..n_blocks {
                let _ = highlight_cached(code, language, &mut hashes[i]);
            }
        }
        let after = start.elapsed();

        eprintln!("\n=== Scrolling re-render benchmark ===");
        eprintln!(
            "  Document: {} code blocks, {} scroll events",
            n_blocks, rerenders
        );
        eprintln!(
            "  Before (no cache): {:?} ({:.1} ms/scroll)",
            before,
            before.as_nanos() as f64 / rerenders as f64 / 1_000_000.0
        );
        eprintln!(
            "  After (cache hit):  {:?} ({:.1} ms/scroll)",
            after,
            after.as_nanos() as f64 / rerenders as f64 / 1_000_000.0
        );
        eprintln!(
            "  Speedup: {:.0}x",
            before.as_nanos() as f64 / after.as_nanos() as f64
        );
    }

    /// Benchmark: overhead of hash computation alone per code block
    #[test]
    fn benchmark_hash_overhead() {
        let code = SAMPLE_RUST;
        let language = Some("rust");
        let iterations = 1_000_000;

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = hash_only(code, language);
        }
        let duration = start.elapsed();

        eprintln!("\n=== Hash computation overhead ===");
        eprintln!(
            "  {} iterations: {:?} ({:.1} ns per hash)",
            iterations,
            duration,
            duration.as_nanos() as f64 / iterations as f64
        );
    }

    /// Benchmark: full document render (like editor_explanation.md)
    #[test]
    fn benchmark_full_document() {
        // Simulate editor_explanation.md: 30 code blocks of varying sizes
        let code_small = "let x = 1;";
        let code_medium = SAMPLE_RUST;
        let code_large_str = SAMPLE_RUST.repeat(5); // ~130 lines
        let code_large = code_large_str.as_str();

        let blocks: Vec<(&str, Option<&str>)> = vec![
            (code_small, Some("rust"));
            5  // 5 tiny blocks
        ]
        .into_iter()
        .chain(vec![(code_medium, Some("rust")); 20].into_iter()) // 20 medium
        .chain(vec![(code_large, Some("rust")); 5].into_iter()) // 5 large
        .collect();

        let n_blocks = blocks.len();
        let rerenders = 100;

        // Warm up
        let mut hashes = vec![0u64; n_blocks];
        for (i, (code, lang)) in blocks.iter().enumerate() {
            let _ = highlight_cached(code, *lang, &mut hashes[i]);
        }

        // "Before" — no cache
        let start = Instant::now();
        for _ in 0..rerenders {
            for (code, lang) in &blocks {
                let _ = highlight_raw(code, *lang);
            }
        }
        let before = start.elapsed();

        // "After" — cached
        let start = Instant::now();
        for _ in 0..rerenders {
            for (i, (code, lang)) in blocks.iter().enumerate() {
                let _ = highlight_cached(code, *lang, &mut hashes[i]);
            }
        }
        let after = start.elapsed();

        eprintln!(
            "\n=== Full document scroll benchmark ({} blocks, {} scrolls) ===",
            n_blocks, rerenders
        );
        eprintln!(
            "  Before (no cache): {:?} ({:.1} ms/scroll)",
            before,
            before.as_nanos() as f64 / rerenders as f64 / 1_000_000.0
        );
        eprintln!(
            "  After (cache hit):  {:?} ({:.1} ms/scroll)",
            after,
            after.as_nanos() as f64 / rerenders as f64 / 1_000_000.0
        );
        eprintln!(
            "  Speedup: {:.0}x",
            before.as_nanos() as f64 / after.as_nanos() as f64
        );
    }
}
