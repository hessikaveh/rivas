use serde::Serialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Global debug JSON logging flag.
pub static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

/// Global debug annotations (visual overlay) flag.
pub static ANNOTATIONS_MODE: AtomicBool = AtomicBool::new(false);

/// Timestamp of app start for relative ms values.
static mut START: Option<Instant> = None;

/// JSONL log writer, guarded by a Mutex.
static mut LOG_WRITER: Option<Mutex<BufWriter<File>>> = None;

/// Initializes the debug logging system.
///
/// When `logging` is true, creates a `rivas-debug.jsonl` file and records
/// the start timestamp for relative timing. When `annotations` is true,
/// enables visual debug overlays in the renderer.
pub fn init(logging: bool, annotations: bool) {
    DEBUG_MODE.store(logging, Ordering::Relaxed);
    ANNOTATIONS_MODE.store(annotations, Ordering::Relaxed);
    if logging {
        unsafe {
            START = Some(Instant::now());
        }
        let file = File::create("rivas-debug.jsonl").expect("failed to create rivas-debug.jsonl");
        unsafe {
            LOG_WRITER = Some(Mutex::new(BufWriter::new(file)));
        }
    }
}

/// Returns `true` if debug JSON logging is enabled.
pub fn is_enabled() -> bool {
    DEBUG_MODE.load(Ordering::Relaxed)
}

/// Returns `true` if visual debug annotations are enabled.
pub fn are_annotations_enabled() -> bool {
    ANNOTATIONS_MODE.load(Ordering::Relaxed)
}

/// Returns the number of milliseconds elapsed since the app started (or 0 if logging is off).
pub fn elapsed_ms() -> u128 {
    unsafe { START.map(|t| t.elapsed().as_millis()).unwrap_or(0) }
}

/// Appends a debug event as a JSON line to the log file.
///
/// Does nothing if debug logging is not enabled.
pub fn log_event(event: &DebugEvent) {
    if !is_enabled() {
        return;
    }
    let mut payload = serde_json::to_vec(event).unwrap_or_default();
    payload.push(b'\n');
    unsafe {
        if let Some(ref w) = LOG_WRITER {
            if let Ok(mut guard) = w.lock() {
                let _ = guard.write_all(&payload);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Event types
// ─────────────────────────────────────────────────────────────────────────────

/// Cursor position recorded in debug events.
#[derive(Serialize)]
pub struct CursorPos {
    /// Byte offset from the start of the document.
    pub byte: usize,
    /// Line number (0-indexed).
    pub row: usize,
    /// Column index (0-indexed character position within the line).
    pub col: usize,
}

/// Viewport dimensions recorded in debug events.
#[derive(Serialize)]
pub struct ViewportInfo {
    /// Width in columns.
    pub w: u32,
    /// Height in lines.
    pub h: u32,
}

/// A debug event that can be serialized to JSONL for analysis.
#[derive(Serialize)]
#[serde(tag = "event")]
pub enum DebugEvent {
    #[serde(rename = "render_tick")]
    RenderTick {
        ts: u128,
        cursor: CursorPos,
        scroll: i32,
        content_height: i32,
        viewport: ViewportInfo,
        blocks: usize,
        mode: String,
    },
    #[serde(rename = "termcaps")]
    TermCaps {
        ts: u128,
        cell_w: u16,
        cell_h: u16,
        overridden: bool,
    },
    #[serde(rename = "graphics_scale")]
    GraphicsScale { ts: u128, scale: f32 },
    #[serde(rename = "image_load")]
    ImageLoad {
        ts: u128,
        url: String,
        pixel_w: u32,
        pixel_h: u32,
        cell_cols: u32,
        cell_rows: u32,
        load_ms: u128,
    },
    #[serde(rename = "image_place")]
    ImagePlace {
        ts: u128,
        id: u32,
        x: i32,
        y: i32,
        cols: i32,
        rows: i32,
        src_y_offset: i32,
    },
    #[serde(rename = "image_detach")]
    ImageDetach { ts: u128, id: u32, reason: String },
    #[serde(rename = "block_layout")]
    BlockLayout {
        ts: u128,
        idx: usize,
        block_type: String,
        span_start: usize,
        span_end: usize,
        est_height: u32,
    },
    #[serde(rename = "scroll")]
    Scroll { ts: u128, old: i32, new: i32 },
    #[serde(rename = "stick_bottom")]
    StickBottom {
        ts: u128,
        active: bool,
        content_h: i32,
        off: i32,
        target: i32,
        repin: bool,
    },
    #[serde(rename = "cursor_scroll")]
    CursorScroll {
        ts: u128,
        block_top: i32,
        block_bottom: i32,
        scroll_off: i32,
        target: i32,
        viewport_h: i32,
    },
    // ── Kitty protocol events ──────────────────────────────────────────────────
    #[serde(rename = "kitty_transmit")]
    KittyTransmit {
        ts: u128,
        id: u32,
        cols: u32,
        rows: u32,
        crop_x: u32,
        crop_y: u32,
        crop_w: u32,
        crop_h: u32,
        data_size: usize,
        has_animation: bool,
    },
    #[serde(rename = "kitty_place")]
    KittyPlace {
        ts: u128,
        id: u32,
        cols: u32,
        rows: u32,
        crop_x: u32,
        crop_y: u32,
        crop_w: u32,
        crop_h: u32,
    },
    #[serde(rename = "kitty_delete")]
    KittyDelete {
        ts: u128,
        id: u32,
        scope: String, // "placements" or "image" or "all"
    },
}
