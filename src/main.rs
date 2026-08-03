use crate::output::graphics_manager;
use crate::output::kitty;
use anyhow::Result;
use clap::Parser;
use iocraft::prelude::*;
use std::fs;
use std::io::{IsTerminal, Read, Write, stdin};
use std::path::PathBuf;
mod assets;
mod components;
mod debug;
mod document;
mod lib_file_cache;
mod output;
mod theme;

use crate::components::document::Document;
use crate::lib_file_cache::FileListCache;
use skim::prelude::{Skim, SkimItemReader, SkimOptionsBuilder};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, PartialEq)]
enum AppAction {
    #[default]
    Quit,
    SearchFile,
}

#[derive(Parser)]
#[command(
    name = "rivas",
    about = "Terminal markdown viewer and editor with pixel perfect rendering"
)]
struct Cli {
    /// Markdown file to view (reads stdin if omitted)
    file: Option<PathBuf>,
    /// Theme: dark, light
    #[arg(short, long, default_value = "dark")]
    theme: String,
    /// Enable debug mode: JSONL log to rivas-debug.jsonl + visual overlay annotations
    #[arg(long)]
    debug: bool,
    /// Debug JSON logging only — writes rivas-debug.jsonl without visual overlays
    #[arg(long)]
    debug_json_only: bool,
    /// Show debug layout annotations (visual overlay boxes). Can combine with --debug or --debug-json-only
    #[arg(long)]
    debug_annotations: bool,
    /// Math rendering mode. `unicode` (default) shows formulas as Unicode
    /// glyphs that work in any terminal. `image` rasterizes them via the Kitty
    /// graphics protocol for pixel-perfect layout (needs Kitty/WezTerm/Ghostty).
    #[arg(long, value_enum, default_value_t = crate::assets::math::MathMode::Image)]
    math: crate::assets::math::MathMode,
    /// Force Kitty graphics protocol even if the terminal is not recognized.
    /// Use this if your terminal supports Kitty but is not auto-detected.
    #[arg(long)]
    force_kitty: bool,
    /// Override the pixel width of a terminal cell. Graphics are shown at their
    /// natural pixel size mapped onto this cell, so this directly grows/shrinks
    /// images, diagrams and formulas. Use it to correct a terminal whose
    /// `TIOCGWINSZ` pixel size is wrong (e.g. a secondary display). If only one
    /// axis is given, the other defaults to its standard value (16).
    #[arg(long)]
    cell_width: Option<u16>,
    /// Override the pixel height of a terminal cell (see `--cell-width`).
    /// If only one axis is given, the other defaults to 8.
    #[arg(long)]
    cell_height: Option<u16>,
}

/// Whether debug JSON logging is enabled (from --debug or --debug-json-only)
fn debug_json_enabled(cli: &Cli) -> bool {
    cli.debug || cli.debug_json_only
}

/// Whether visual debug annotations should be shown
fn debug_annotations_enabled(cli: &Cli) -> bool {
    cli.debug_annotations || cli.debug
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    debug::init(debug_json_enabled(&cli), debug_annotations_enabled(&cli));
    crate::assets::math::set_math_mode(cli.math);

    let (mut content, mut file_path) = match &cli.file {
        // CASE 1: User provided a path
        Some(path) => {
            if !path.exists() {
                // Create the file (and parent directories) if it doesn't exist
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::File::create(path)?;
                (String::new(), Some(path.clone()))
            } else {
                (fs::read_to_string(path)?, Some(path.clone()))
            }
        }
        // CASE 2: No path provided - Check for Pipe vs. New Default File
        None => {
            if !stdin().is_terminal() {
                // Data is being piped in (e.g., `echo "# Hi" | rivas`)
                let mut s = String::new();
                stdin().read_to_string(&mut s)?;
                (s, None)
            } else {
                // No pipe and no arg: Open a default new file
                let default_path = PathBuf::from("untitled.md");
                if !default_path.exists() {
                    fs::File::create(&default_path)?;
                }
                (String::new(), Some(default_path))
            }
        }
    };

    // Terminal capability check
    match (cli.cell_width, cli.cell_height) {
        (Some(w), Some(h)) => output::capabilities::override_cell_size(w, h),
        (Some(w), None) => output::capabilities::override_cell_size(w, 16),
        (None, Some(h)) => output::capabilities::override_cell_size(8, h),
        (None, None) => {}
    }
    let _caps = output::capabilities::TermCaps::detect()?;
    debug::log_event(&debug::DebugEvent::TermCaps {
        ts: debug::elapsed_ms(),
        cell_w: _caps.cell_w_px,
        cell_h: _caps.cell_h_px,
        overridden: cli.cell_width.is_some() || cli.cell_height.is_some(),
    });
    if cli.force_kitty {
        output::capabilities::force_kitty();
    }
    if !output::capabilities::has_kitty() {
        if cli.math == crate::assets::math::MathMode::Image {
            log::warn!(
                "Terminal does not support Kitty graphics; falling back to Unicode math mode."
            );
            crate::assets::math::set_math_mode(crate::assets::math::MathMode::Unicode);
        } else {
            log::warn!(
                "Terminal does not support Kitty graphics; images and diagrams will show as text."
            );
        }
    }

    let action = Arc::new(Mutex::new(AppAction::Quit));

    loop {
        *action.lock().unwrap() = AppAction::Quit;

        smol::block_on(
            element!(App(
                file_path: file_path.clone(),
                content: content.as_str(),
                action: action.clone(),
                debug: debug_json_enabled(&cli),
                debug_annotations: debug_annotations_enabled(&cli),
            ))
            .fullscreen(),
        )?;

        let next_action = action.lock().unwrap().clone();
        match next_action {
            AppAction::Quit => break,
            AppAction::SearchFile => {
                if let Some(selected) = run_fuzzy_finder() {
                    // Auto-save current file if it exists and content is modified
                    if let Some(ref path) = file_path {
                        if let Ok(on_disk) = fs::read_to_string(path) {
                            if on_disk != content {
                                let _ = fs::write(path, &content);
                            }
                        }
                    }
                    if let Ok(new_content) = fs::read_to_string(&selected) {
                        content = new_content;
                        file_path = Some(selected);
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Default, Props)]
struct AppProps<'a> {
    file_path: Option<PathBuf>,
    content: &'a str,
    action: Arc<Mutex<AppAction>>,
    debug: bool,
    debug_annotations: bool,
}

#[component]
fn App<'a>(props: &AppProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let mut system = hooks.use_context_mut::<SystemContext>();
    let should_exit = hooks.use_state(|| false);
    let path = props.file_path.clone().unwrap_or_default();
    let _path_name = path
        .to_str()
        .filter(|name| !name.is_empty())
        .unwrap_or("untitled.md")
        .to_string();
    let content = hooks.use_state(|| props.content.to_string());
    let mouse_captured = hooks.use_state(|| false);
    let cursor_offset = hooks.use_ref(|| 0usize);

    hooks.use_terminal_events({
        let mut should_exit = should_exit;
        let action = props.action.clone();
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) if kind != KeyEventKind::Release => {
                let ctrl = modifiers.contains(KeyModifiers::CONTROL);
                if ctrl && code == KeyCode::Char('p') {
                    *action.lock().unwrap() = AppAction::SearchFile;
                    should_exit.set(true);
                }
            }
            _ => {}
        }
    });

    if should_exit.get() {
        if kitty::is_supported() {
            let mut stdout = std::io::stdout().lock();
            kitty::delete_all(&mut stdout);
            let _ = stdout.flush();
        }
        system.exit();
    }

    system.set_mouse_capture(mouse_captured.get());
    // Flush any pending kitty graphic commands buffered by the background
    // graphics thread.  Must happen here (main render thread) so the writes
    // are atomic w.r.t. iocraft/crossterm output.
    graphics_manager::flush_output();
    let current_content = content.read().clone();
    let on_change = hooks.use_async_handler(move |next_content: String| {
        let mut content = content.clone();
        async move {
            content.set(next_content);
        }
    });
    let on_quit = hooks.use_async_handler(move |()| {
        let mut should_exit = should_exit.clone();
        async move {
            should_exit.set(true);
        }
    });

    element! {
        View(flex_direction: FlexDirection::Column, width, height) {
            Document(
                content: current_content,
                file_path: path,
                viewport_height: height.saturating_sub(1) as u32,
                viewport_width: width as u32,
                keyboard_navigation: Some(true),
                follow_ref: None,
                cursor_offset: Some(cursor_offset),
                debug: props.debug,
                debug_annotations: props.debug_annotations,
                on_change,
                on_quit,
            )
            View(width: 100pct, height: 1, background_color: theme::STATUS_BG, flex_direction: FlexDirection::Row) {
                View(background_color: theme::DARK_GREY) {
                    Text(content: " :q ", color: theme::FG)
                }
                Text(content: " Quit ")
                View(background_color: theme::DARK_GREY) {
                    Text(content: " C-p ", color: theme::FG)
                }
                Text(content: " Find ")
                View(background_color: theme::DARK_GREY) {
                    Text(content: " j/k ", color: theme::FG)
                }
                Text(content: " Scroll ")
                View(background_color: theme::DARK_GREY) {
                    Text(content: " gg/G ", color: theme::FG)
                }
                Text(content: " Top/Bottom ")
                View(background_color: theme::DARK_GREY) {
                    Text(content: " :math ", color: theme::FG)
                }
                Text(content: " Math mode ")
                View(background_color: theme::DARK_GREY) {
                    Text(content: " 🔍 [ ] ", color: theme::FG)
                }
                View(flex_grow: 1.0) {}
            }
        }
    }
}

// Global file cache with 5 second TTL (revalidates on each Ctrl+P)
lazy_static::lazy_static! {
    static ref FILE_CACHE: FileListCache = FileListCache::new();
}

fn visit_dirs(dir: &std::path::Path, files: &mut Vec<String>) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && (name.starts_with('.')
                        || name == "target"
                        || name == "node_modules"
                        || name == "build"
                        || name == "dist"
                        || name == "venv"
                        || name == "env")
                {
                    continue;
                }
                visit_dirs(&path, files)?;
            } else if let Some(path_str) = path.to_str() {
                let relative_path = path_str.strip_prefix("./").unwrap_or(path_str).to_string();
                files.push(relative_path);
            }
        }
    }
    Ok(())
}

/// Get local files with caching (5 second TTL)
fn get_local_files() -> Vec<String> {
    // Check cache first (5 second TTL)
    if let Some(cached) = FILE_CACHE.get(5) {
        return cached;
    }

    // Cache miss - rescan filesystem
    let mut files = Vec::new();
    let _ = visit_dirs(std::path::Path::new("."), &mut files);

    // Store in cache for next call
    FILE_CACHE.set(files.clone());

    files
}

fn run_fuzzy_finder() -> Option<PathBuf> {
    let files = get_local_files();
    let input = files.join("\n");

    let options = SkimOptionsBuilder::default()
        .height("40%".to_string())
        .multi(false)
        .prompt("Find File> ".to_string())
        .build()
        .unwrap();

    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(std::io::Cursor::new(input));

    let output = Skim::run_with(options, Some(items)).ok()?;

    if output.is_abort {
        return None;
    }

    let selected = output.selected_items.first()?;
    Some(PathBuf::from(selected.output().to_string()))
}
