use crate::theme;
use iocraft::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;
use syntect::{easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet};

static SS: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
static TS: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Properties for the [`CodeBlock`] component.
#[derive(Default, Props)]
pub struct CodeBlockProps {
    /// Programming language for syntax highlighting (e.g., `"rust"`, `"python"`).
    pub language: Option<String>,
    /// The source code content to render.
    pub code: String,
}

/// Renders a syntax-highlighted fenced code block.
///
/// Uses `syntect` for highlighting with the base16-ocean-dark theme.
/// The language label is displayed in the top-right corner.
#[component]
pub fn CodeBlock(props: &CodeBlockProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let lang_label = props.language.clone().unwrap_or_else(|| "code".to_string());

    let mut highlighted = hooks.use_ref(|| Vec::<Vec<(String, Color)>>::new());
    let mut prev_hash = hooks.use_ref(|| 0u64);

    let new_hash = {
        let mut hasher = DefaultHasher::new();
        props.language.hash(&mut hasher);
        props.code.hash(&mut hasher);
        hasher.finish()
    };

    if *prev_hash.read() != new_hash {
        prev_hash.set(new_hash);

        let syntax = props
            .language
            .as_deref()
            .and_then(|l| SS.find_syntax_by_token(l))
            .unwrap_or_else(|| SS.find_syntax_plain_text());
        let theme = &TS.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        let mut lines = Vec::new();
        for line in props.code.lines() {
            match highlighter.highlight_line(line, &SS) {
                Ok(regions) => {
                    let spans = regions
                        .iter()
                        .map(|(style, text)| {
                            let color = Color::Rgb {
                                r: style.foreground.r,
                                g: style.foreground.g,
                                b: style.foreground.b,
                            };
                            (text.to_string(), color)
                        })
                        .collect();
                    lines.push(spans);
                }
                Err(_) => {
                    lines.push(vec![(line.to_string(), theme::FG)]);
                }
            }
        }
        highlighted.set(lines);
    }

    element! {
        View(flex_direction: FlexDirection::Column, padding_left: 2, padding_right: 2, margin_bottom: 1, background_color: theme::DARK_BG) {
            View() {
                Text(content: lang_label, color: theme::BLUE)
            }
            View(flex_direction: FlexDirection::Column) {
                #(highlighted.read().iter().map(|line_spans| {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

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
