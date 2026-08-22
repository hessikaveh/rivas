use crate::debug;
use crate::output::capabilities::TermCaps;
use crate::output::kitty;
use crate::{
    assets::images::{ImageData, load_image},
    assets::math::render_math,
    assets::mermaid::render_mermaid_to_png,
};
use base64::Engine;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Sender},
};

/// Buffered kitty terminal output.  The background graphics thread writes here
/// instead of directly to stdout, and the main render thread drains it via
/// `flush_output()`.  This prevents byte-level interleaving with iocraft/crossterm
/// output on the same fd.
static PENDING_OUTPUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// A placement request for an already-cached image. The manager turns this into
/// a lightweight `a=p` (no retransmission) once the pixels are in the terminal.
#[derive(Clone, Copy)]
pub struct GfxRect {
    pub x: i32,
    pub y: i32,
    pub vis_cols: i32,
    pub vis_rows: i32,
    pub src_y_offset: i32,
}

/// Describes how to produce the image pixels for a given key. The manager owns
/// the loader pool, so components only describe *what* to load, never *how*.
pub enum GfxSource {
    Image {
        url: String,
        base_dir: Option<PathBuf>,
        max_w: u32,
        max_cols: u32,
        max_rows: u32,
    },
    Mermaid {
        source: String,
        max_w: u32,
        max_cols: u32,
        max_rows: u32,
    },
    Math {
        content: String,
        display: bool,
        max_w: u32,
        max_cols: u32,
        max_rows: u32,
    },
}

impl GfxSource {
    fn max_cols(&self) -> u32 {
        match self {
            Self::Image { max_cols, .. }
            | Self::Mermaid { max_cols, .. }
            | Self::Math { max_cols, .. } => *max_cols,
        }
    }
    fn max_rows(&self) -> u32 {
        match self {
            Self::Image { max_rows, .. }
            | Self::Mermaid { max_rows, .. }
            | Self::Math { max_rows, .. } => *max_rows,
        }
    }
}

/// Global cache of real image dimensions (cols, rows) keyed by the same key the
/// components use. Lets the virtual-scrolling height estimator reuse real
/// heights and keeps a single source of truth (owned here, not in components).
pub struct ImageHeightCache {
    heights: Mutex<HashMap<String, (u32, u32)>>,
    generation: AtomicU64,
}

impl ImageHeightCache {
    /// Creates an empty height cache.
    pub fn new() -> Self {
        Self {
            heights: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
        }
    }
    /// Retrieves the cached `(cols, rows)` dimensions for a graphic key.
    pub fn get(&self, key: &str) -> Option<(u32, u32)> {
        self.heights.lock().ok().and_then(|m| m.get(key).copied())
    }
    /// Stores the display dimensions for a graphic key.
    ///
    /// Increments the generation counter if the value actually changed,
    /// signaling to components that a re-render may be needed.
    pub fn set(&self, key: &str, cols: u32, rows: u32) {
        let changed = {
            let mut m = self.heights.lock().ok();
            match m {
                Some(ref mut m) => {
                    let existing = m.get(key).copied();
                    m.insert(key.to_string(), (cols, rows));
                    existing != Some((cols, rows))
                }
                None => false,
            }
        };
        if changed {
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// Returns the current generation counter, which increments on each dimension change.
    ///
    /// Components can poll this to detect when cached dimensions need updating.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
}

lazy_static::lazy_static! {
    pub static ref IMAGE_HEIGHT_CACHE: ImageHeightCache = ImageHeightCache::new();
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal state
// ─────────────────────────────────────────────────────────────────────────────

enum EntryStatus {
    Loading,
    Ready(
        #[allow(dead_code)] Arc<String>,
        #[allow(dead_code)] Vec<(Arc<String>, u32)>,
        #[allow(dead_code)] bool, /* has_animation */
    ),
    Error(String),
}

struct Entry {
    kitty_id: u32,
    status: EntryStatus,
    refcount: usize,
    desired: Option<GfxRect>,
    visible: bool,
    cell_cols: u32,
    cell_rows: u32,
    raster_w: u32,
    raster_h: u32,
    max_cols: u32,
    max_rows: u32,
    last_used: u64,
}

struct LoadedData {
    data: Arc<String>,
    frames: Vec<(Arc<String>, u32)>,
    pixel_w: u32,
    pixel_h: u32,
    has_animation: bool,
    max_cols: u32,
    max_rows: u32,
}

enum Cmd {
    Acquire {
        key: String,
        source: GfxSource,
    },
    Loaded {
        key: String,
        result: Result<LoadedData, String>,
    },
    Place {
        key: String,
        rect: GfxRect,
    },
    Detach {
        key: String,
    },
    Release {
        key: String,
    },
}

const CACHE_CAP: usize = 128;

/// Single owner of all kitty graphics I/O. One thread, one channel, one registry
/// of terminal-cached images. Components never touch stdout for images.
/// Manages terminal graphics lifecycle: acquiring, placing, detaching, and releasing images.
///
/// Runs a background thread that processes commands sequentially, ensuring thread-safe
/// access to the terminal's graphics state.
pub struct GraphicsManager {
    tx: Sender<Cmd>,
    registry: Arc<Mutex<HashMap<String, Entry>>>,
}

lazy_static::lazy_static! {
    static ref MANAGER: GraphicsManager = GraphicsManager::new();
}

/// Returns the global singleton graphics manager.
pub fn graphics() -> &'static GraphicsManager {
    &MANAGER
}

fn encode(raw: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(raw)
}

/// How many times the terminal cell's pixel size the raster is produced at.
/// The raster is stretched to fit the destination cell box by the terminal, so a
/// 2× raster keeps lines crisp on HiDPI screens. It does **not** affect the
/// on-screen size in columns/rows (those come from [`compute_dims`]), so an
/// unreliable `TIOCGWINSZ` pending pixel size never makes images tiny/large.
const RASTER_SCALE: u32 = 2;
/// Hard cap on the generated raster width (px), to bound memory even when a
/// terminal reports an absurdly large cell pixel size.
const MAX_RASTER_PX: u32 = 4096;

/// Pixel width to rasterize a graphic that will be shown across `max_cols`
/// cells in a column a cell is `cell_w_px` wide.
pub fn raster_max_width(max_cols: u32, cell_w_px: u32) -> u32 {
    (max_cols.max(1) as u64 * cell_w_px.max(1) as u64 * RASTER_SCALE as u64)
        .min(MAX_RASTER_PX as u64) as u32
}

/// Computes the display size in terminal cells for a rasterized `pixel_w ×
/// pixel_h` graphic, so that it shows at its **natural size** — its own pixels
/// mapped onto a nominal terminal cell — and is **never stretched**. If that
/// natural size is larger than the available `max_cols × max_rows` cell box it
/// is scaled *down* (preserving aspect ratio) to fit; if it fits, it is left at
/// its true size so small images and inline math stay small.
///
/// iocraft itself does all layout in cell coordinates and has no pixel notion;
/// a pixel→cell conversion is only needed here. We use the cell size from
/// `--cell-width`/`--cell-height` when pinned, otherwise the standard nominal
/// 8×16 (16 cells/inch is effectively universal), so sizing stays deterministic
/// even if `TIOCGWINSZ` reports a wrong or stale pixel size on a secondary
/// display.
fn compute_dims(
    pixel_w: u32,
    pixel_h: u32,
    caps: Option<&TermCaps>,
    max_cols: u32,
    max_rows: u32,
) -> (u32, u32) {
    let mc = max_cols.max(1) as f64;
    let mr = max_rows.max(1) as f64;
    if pixel_w == 0 || pixel_h == 0 {
        return (1, 1);
    }
    // One cell is `cell_w_px` wide and `cell_h_px` tall (default 8×16). A
    // graphic's natural size in cells is its pixel size over the cell size.
    // Note: RASTER_SCALE is deliberately NOT applied here. The raster is
    // generated at higher resolution purely for crispness; the terminal
    // stretches those pixels to fill the placed cells, so it must not change
    // how many cells a graphic occupies.
    let cw = caps.map(|c| c.cell_w_px.max(1) as f64).unwrap_or(8.0);
    let ch = caps.map(|c| c.cell_h_px.max(1) as f64).unwrap_or(16.0);
    // The user can scale every graphic live at runtime; apply that multiplier.
    let scale = crate::output::capabilities::graphics_scale() as f64;
    let nat_cols = (pixel_w as f64 / cw).round().max(1.0) * scale;
    let nat_rows = (pixel_h as f64 / ch).round().max(1.0) * scale;

    // If it already fits, show it at its natural size (no upscaling / no stretch).
    if nat_cols <= mc && nat_rows <= mr {
        return (nat_cols as u32, nat_rows as u32);
    }
    // Otherwise scale down uniformly so it fits within the box, aspect intact.
    // One axis is always the limiting one; make that axis fill the box exactly.
    let sx = mc / nat_cols;
    let sy = mr / nat_rows;
    if sx < sy {
        let cols = mc as u32;
        let rows = (nat_rows * sx).round().max(1.0).min(mr) as u32;
        (cols, rows)
    } else {
        let rows = mr as u32;
        let cols = (nat_cols * sy).round().max(1.0).min(mc) as u32;
        (cols, rows)
    }
}

/// Selects the source (pixel) sub-rectangle of the rasterized image that maps
/// onto the currently visible cell window, so the terminal stretches exactly
/// that region to fill the placed cells. When the whole image is visible this
/// is the full raster (no crop); when scrolled/clipped it is the proportional
/// vertical band. The full width is always kept (a slightly narrower placement
/// simply downscales), so the image is never chopped horizontally.
#[allow(clippy::too_many_arguments)]
fn source_crop(
    raster_w: u32,
    raster_h: u32,
    _full_cols: u32,
    full_rows: u32,
    vis_cols: u32,
    vis_rows: u32,
    src_y_offset_cells: u32,
) -> (u32, u32, u32, u32) {
    let _ = vis_cols;
    let fr = full_rows.max(1) as u64;
    let rw = raster_w as u64;
    let rh = raster_h as u64;
    let crop_w = rw;
    let crop_h = if vis_rows >= full_rows {
        rh
    } else {
        (rh * vis_rows as u64 / fr).min(rh)
    };
    let src_y_px = (rh * src_y_offset_cells as u64 / fr).min(rh.saturating_sub(crop_h));
    (0, src_y_px as u32, crop_w as u32, crop_h as u32)
}

fn load(source: GfxSource) -> Result<LoadedData, String> {
    match source {
        GfxSource::Image {
            url,
            base_dir,
            max_w,
            max_cols,
            max_rows,
        } => {
            let data =
                load_image(&url, base_dir.as_deref(), max_w).map_err(|e| format!("{:#}", e))?;
            let w = data.width();
            let h = data.height();
            let (b64, frames) = match data {
                ImageData::Png(raw, _, _) => (Arc::new(encode(&raw)), Vec::new()),
                ImageData::Gif { frames, .. } => {
                    let first = Arc::new(encode(&frames[0].0));
                    let rest = frames[1..]
                        .iter()
                        .map(|(p, d)| (Arc::new(encode(p)), *d))
                        .collect();
                    (first, rest)
                }
            };
            let has_animation = !frames.is_empty();
            Ok(LoadedData {
                data: b64,
                frames,
                pixel_w: w,
                pixel_h: h,
                has_animation,
                max_cols,
                max_rows,
            })
        }
        GfxSource::Mermaid {
            source,
            max_w,
            max_cols,
            max_rows,
        } => {
            let (png, w, h) =
                render_mermaid_to_png(&source, max_w).map_err(|e| format!("{:#}", e))?;
            Ok(LoadedData {
                data: Arc::new(encode(&png)),
                frames: Vec::new(),
                pixel_w: w,
                pixel_h: h,
                has_animation: false,
                max_cols,
                max_rows,
            })
        }
        GfxSource::Math {
            content,
            display,
            max_w,
            max_cols,
            max_rows,
        } => {
            let (png, w, h) = render_math(&content, display, max_w, crate::theme::is_dark())
                .map_err(|e| format!("{:#}", e))?;
            Ok(LoadedData {
                data: Arc::new(encode(&png)),
                frames: Vec::new(),
                pixel_w: w,
                pixel_h: h,
                has_animation: false,
                max_cols,
                max_rows,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn transmit_at(
    id: u32,
    data: &Arc<String>,
    frames: &[(Arc<String>, u32)],
    has_anim: bool,
    rect: GfxRect,
    raster_w: u32,
    raster_h: u32,
    full_cols: u32,
    full_rows: u32,
) {
    let vis_cols = rect.vis_cols.max(0) as u32;
    let vis_rows = rect.vis_rows.max(0) as u32;
    let (sx, sy, sw, sh) = source_crop(
        raster_w,
        raster_h,
        full_cols,
        full_rows,
        vis_cols,
        vis_rows,
        rect.src_y_offset.max(0) as u32,
    );

    // a=t transmits image data into the terminal's graphic store without
    // creating any visual placement — no cursor movement, no scroll damage.
    let mut buf = PENDING_OUTPUT.lock().unwrap();
    kitty::transmit_only_encoded(
        &mut *buf,
        id,
        data.as_str(),
        vis_cols,
        vis_rows,
        sx,
        sy,
        sw,
        sh,
    );
    if has_anim {
        let fr: Vec<(&str, u32)> = frames.iter().map(|(s, d)| (s.as_str(), *d)).collect();
        kitty::write_animation_frames_encoded(&mut *buf, id, &fr);
        kitty::start_animation(&mut *buf, id);
    }

    debug::log_event(&debug::DebugEvent::KittyTransmit {
        ts: debug::elapsed_ms(),
        id,
        cols: vis_cols,
        rows: vis_rows,
        crop_x: sx,
        crop_y: sy,
        crop_w: sw,
        crop_h: sh,
        data_size: data.len(),
        has_animation: has_anim,
    });
}

#[allow(clippy::too_many_arguments)]
fn place_at(id: u32, rect: GfxRect, raster_w: u32, raster_h: u32, full_cols: u32, full_rows: u32) {
    let vis_cols = rect.vis_cols.max(0) as u32;
    let vis_rows = rect.vis_rows.max(0) as u32;
    let (sx, sy, sw, sh) = source_crop(
        raster_w,
        raster_h,
        full_cols,
        full_rows,
        vis_cols,
        vis_rows,
        rect.src_y_offset.max(0) as u32,
    );

    let mut buf = PENDING_OUTPUT.lock().unwrap();
    write!(buf, "\x1b7").unwrap();
    write!(buf, "\x1b[{};{}H", rect.y + 1, rect.x + 1).unwrap();
    kitty::delete_placements(&mut *buf, id);
    kitty::place_image(&mut *buf, id, vis_cols, vis_rows, sx, sy, sw, sh);
    write!(buf, "\x1b8").unwrap();

    debug::log_event(&debug::DebugEvent::KittyPlace {
        ts: debug::elapsed_ms(),
        id,
        cols: vis_cols,
        rows: vis_rows,
        crop_x: sx,
        crop_y: sy,
        crop_w: sw,
        crop_h: sh,
    });
}

fn evict(reg: &mut HashMap<String, Entry>, tick: u64) {
    // First, free any terminal images whose refcount has dropped to 0. These
    // are no longer referenced by any mounted component, so their pixels must be
    // released from the terminal immediately — otherwise they linger on screen /
    // accumulate in the terminal's graphic store even in small documents.
    let mut freed: Vec<String> = reg
        .iter()
        .filter(|(_, e)| e.refcount == 0)
        .map(|(k, _)| k.clone())
        .collect();
    freed.sort_by_key(|k| reg.get(k).map(|e| e.last_used).unwrap_or(0));
    let mut buf = PENDING_OUTPUT.lock().unwrap();
    let mut to_remove = Vec::new();
    for k in freed {
        if let Some(e) = reg.get(&k) {
            if e.refcount == 0 {
                kitty::delete_image(&mut *buf, e.kitty_id);
                to_remove.push(k);
            }
        }
    }
    for k in to_remove {
        reg.remove(&k);
    }

    // Then, if we are still over the cap, evict the least-recently-used
    // zero-refcount entries until back under the cap.
    if reg.len() <= CACHE_CAP {
        let _ = tick;
        return;
    }
    let mut released: Vec<(u64, String)> = reg
        .iter()
        .filter(|(_, e)| e.refcount == 0)
        .map(|(k, e)| (e.last_used, k.clone()))
        .collect();
    released.sort_by_key(|(t, _)| *t);
    for (_, k) in released {
        if reg.len() <= CACHE_CAP {
            break;
        }
        if let Some(e) = reg.get(&k) {
            if e.refcount == 0 {
                kitty::delete_image(&mut *buf, e.kitty_id);
                reg.remove(&k);
            }
        }
    }
    let _ = tick;
}

impl GraphicsManager {
    fn new() -> Self {
        let registry: Arc<Mutex<HashMap<String, Entry>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel::<Cmd>();
        let reg2 = registry.clone();
        let tx2 = tx.clone();
        std::thread::spawn(move || Self::run(rx, reg2, tx2));
        Self { tx, registry }
    }

    /// Recomputes the display size of every cached, ready graphic against the
    /// current runtime scale and its original box. Called synchronously after a
    /// user changes the scale so that the next render places them at the new
    /// size immediately. Components detect the new `cell_cols`/`cell_rows`
    /// (via [`dims`](GraphicsManager::dims)) on their next pass and re-place.
    fn refresh_dims(&self) {
        let caps = TermCaps::detect().ok();
        let mut reg = self.registry.lock().unwrap();
        for (key, e) in reg.iter_mut() {
            if !matches!(e.status, EntryStatus::Ready(..)) || e.raster_w == 0 || e.raster_h == 0 {
                continue;
            }
            let (c, r) = compute_dims(
                e.raster_w,
                e.raster_h,
                caps.as_ref(),
                e.max_cols.max(1),
                e.max_rows.max(1),
            );
            if (c, r) != (e.cell_cols, e.cell_rows) {
                e.cell_cols = c;
                e.cell_rows = r;
                let height_key = key
                    .rsplit_once('#')
                    .map(|(k, _)| k.to_string())
                    .unwrap_or_else(|| key.clone());
                IMAGE_HEIGHT_CACHE.set(&height_key, c, r);
            }
        }
    }

    fn run(rx: mpsc::Receiver<Cmd>, registry: Arc<Mutex<HashMap<String, Entry>>>, tx: Sender<Cmd>) {
        let mut tick: u64 = 0;
        while let Ok(cmd) = rx.recv() {
            tick += 1;
            match cmd {
                Cmd::Acquire { key, source } => {
                    let mut reg = registry.lock().unwrap();
                    if let Some(e) = reg.get_mut(&key) {
                        e.refcount += 1;
                        e.last_used = tick;
                        continue;
                    }
                    // Remember the box the graphic was sized against so a later
                    // runtime scale change can recompute its display size.
                    let max_cols = source.max_cols();
                    let max_rows = source.max_rows();
                    let kitty_id = kitty::next_placement_id();
                    reg.insert(
                        key.clone(),
                        Entry {
                            kitty_id,
                            status: EntryStatus::Loading,
                            refcount: 1,
                            desired: None,
                            visible: false,
                            cell_cols: 0,
                            cell_rows: 0,
                            raster_w: 0,
                            raster_h: 0,
                            max_cols,
                            max_rows,
                            last_used: tick,
                        },
                    );
                    drop(reg);
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let result = load(source);
                        let _ = tx.send(Cmd::Loaded { key, result });
                    });
                }
                Cmd::Loaded { key, result } => {
                    let loaded = match result {
                        Ok(d) => d,
                        Err(e) => {
                            if let Ok(mut reg) = registry.lock() {
                                if let Some(en) = reg.get_mut(&key) {
                                    en.status = EntryStatus::Error(e);
                                    en.last_used = tick;
                                }
                            }
                            continue;
                        }
                    };
                    let caps = TermCaps::detect().ok();
                    let (cell_cols, cell_rows) = compute_dims(
                        loaded.pixel_w,
                        loaded.pixel_h,
                        caps.as_ref(),
                        loaded.max_cols,
                        loaded.max_rows,
                    );

                    let (kitty_id, visible, desired, data, frames, has_anim, raster_w, raster_h) = {
                        let mut reg = registry.lock().unwrap();
                        let en = match reg.get_mut(&key) {
                            Some(en) => en,
                            None => continue,
                        };
                        en.status = EntryStatus::Ready(
                            loaded.data.clone(),
                            loaded.frames.clone(),
                            loaded.has_animation,
                        );
                        en.cell_cols = cell_cols;
                        en.cell_rows = cell_rows;
                        en.raster_w = loaded.pixel_w;
                        en.raster_h = loaded.pixel_h;
                        en.last_used = tick;
                        // FIX B1: Strip instance_id suffix for height cache key
                        // so estimate_block_height() can find the stored height.
                        let height_key = key
                            .rsplit_once('#')
                            .map(|(k, _)| k.to_string())
                            .unwrap_or_else(|| key.clone());
                        IMAGE_HEIGHT_CACHE.set(&height_key, cell_cols, cell_rows);
                        (
                            en.kitty_id,
                            en.visible,
                            en.desired,
                            loaded.data.clone(),
                            loaded.frames.clone(),
                            loaded.has_animation,
                            loaded.pixel_w,
                            loaded.pixel_h,
                        )
                    };

                    let rect = desired.unwrap_or(GfxRect {
                        x: 0,
                        y: 0,
                        vis_cols: cell_cols as i32,
                        vis_rows: cell_rows as i32,
                        src_y_offset: 0,
                    });
                    transmit_at(
                        kitty_id, &data, &frames, has_anim, rect, raster_w, raster_h, cell_cols,
                        cell_rows,
                    );
                    // If a Place command was received before loading completed,
                    // place the now-cached image at the target position.
                    if visible {
                        place_at(kitty_id, rect, raster_w, raster_h, cell_cols, cell_rows);
                    }
                    debug::log_event(&debug::DebugEvent::ImageLoad {
                        ts: debug::elapsed_ms(),
                        url: key,
                        pixel_w: loaded.pixel_w,
                        pixel_h: loaded.pixel_h,
                        cell_cols,
                        cell_rows,
                        load_ms: 0,
                    });
                }
                Cmd::Place { key, rect } => {
                    let (kitty_id, ready, raster_w, raster_h, full_cols, full_rows) = {
                        let mut reg = registry.lock().unwrap();
                        let en = match reg.get_mut(&key) {
                            Some(e) => e,
                            None => continue,
                        };
                        en.desired = Some(rect);
                        en.visible = true;
                        en.last_used = tick;
                        (
                            en.kitty_id,
                            matches!(en.status, EntryStatus::Ready(..)),
                            en.raster_w,
                            en.raster_h,
                            en.cell_cols,
                            en.cell_rows,
                        )
                    };
                    if ready {
                        place_at(kitty_id, rect, raster_w, raster_h, full_cols, full_rows);
                    }
                }
                Cmd::Detach { key } => {
                    let (kitty_id, ready) = {
                        let mut reg = registry.lock().unwrap();
                        let en = match reg.get_mut(&key) {
                            Some(e) => e,
                            None => continue,
                        };
                        en.desired = None;
                        en.visible = false;
                        en.last_used = tick;
                        (en.kitty_id, matches!(en.status, EntryStatus::Ready(..)))
                    };
                    if ready {
                        let mut buf = PENDING_OUTPUT.lock().unwrap();
                        kitty::delete_placements(&mut *buf, kitty_id);
                        debug::log_event(&debug::DebugEvent::KittyDelete {
                            ts: debug::elapsed_ms(),
                            id: kitty_id,
                            scope: "placements".into(),
                        });
                    }
                }
                Cmd::Release { key } => {
                    let mut reg = registry.lock().unwrap();
                    if let Some(en) = reg.get_mut(&key) {
                        en.refcount = en.refcount.saturating_sub(1);
                        en.last_used = tick;
                    } else {
                        continue;
                    }
                    evict(&mut reg, tick);
                }
            }
        }

        // Channel closed (app exit): free every cached image in the terminal.
        if let Ok(reg) = registry.lock() {
            let mut buf = PENDING_OUTPUT.lock().unwrap();
            for en in reg.values() {
                kitty::delete_image(&mut *buf, en.kitty_id);
            }
        }
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn dims(&self, key: &str) -> Option<(u32, u32)> {
        self.registry
            .lock()
            .ok()
            .and_then(|r| r.get(key).map(|e| (e.cell_cols, e.cell_rows)))
    }

    pub fn error(&self, key: &str) -> Option<String> {
        self.registry.lock().ok().and_then(|r| {
            r.get(key).and_then(|e| match &e.status {
                EntryStatus::Error(s) => Some(s.clone()),
                _ => None,
            })
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API used by components
// ─────────────────────────────────────────────────────────────────────────────

/// Requests acquisition of a graphic resource (transmits image data to the terminal).
pub fn acquire(key: String, source: GfxSource) {
    graphics().send(Cmd::Acquire { key, source });
}

/// Places a previously acquired graphic at the specified terminal rectangle.
pub fn place(key: String, rect: GfxRect) {
    graphics().send(Cmd::Place { key, rect });
}

/// Detaches a graphic from its current placement (hides it without freeing data).
pub fn detach(key: String) {
    graphics().send(Cmd::Detach { key });
}

/// Releases a graphic completely (removes placement and frees terminal memory).
pub fn release(key: String) {
    graphics().send(Cmd::Release { key });
}

/// Recomputes all cached graphics' display sizes for the current runtime scale.
/// Call after [`capabilities::set_graphics_scale`] to apply it immediately.
pub fn refresh_graphics() {
    graphics().refresh_dims();
}

/// Returns the cached display dimensions `(cols, rows)` for a graphic key.
pub fn dims(key: &str) -> Option<(u32, u32)> {
    graphics().dims(key)
}

/// Returns the error message for a graphic key, if it failed to load.
pub fn gfx_error(key: &str) -> Option<String> {
    graphics().error(key)
}

/// Drain the pending kitty output buffer to stdout.  Must be called from the
/// main render thread (inside the iocraft component tree) so that cursor-
/// positioning escape sequences are written atomically with respect to
/// iocraft/crossterm output.
pub fn flush_output() {
    let data = {
        let mut buf = PENDING_OUTPUT.lock().unwrap();
        if buf.is_empty() {
            return;
        }
        std::mem::take(&mut *buf)
    };
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(&data);
    let _ = stdout.flush();
}

/// RAII guard that releases the image key when the component unmounts. Stored as
/// a hook so it fires on drop, ensuring the terminal-side cached data is freed
/// (via LRU eviction) once no component references the key.
pub struct ReleaseGuard {
    pub key: Arc<Mutex<String>>,
}

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        let k = self.key.lock().unwrap().clone();
        if !k.is_empty() {
            release(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(w: u16, h: u16) -> TermCaps {
        TermCaps {
            cell_w_px: w,
            cell_h_px: h,
        }
    }

    // ── compute_dims (natural size, downscale-only) ─────────────────────

    #[test]
    fn small_image_keeps_natural_size() {
        // A 400×200 image at a nominal 8×16 cell is 50×13 cells. It must NOT
        // be stretched to fill the 94-col box.
        let (cols, rows) = compute_dims(400, 200, Some(&caps(8, 16)), 94, 40);
        assert_eq!(cols, 50, "small image must stay its natural width");
        assert_eq!(rows, 13, "small image must stay its natural height");
    }

    #[test]
    fn inline_math_stays_small() {
        // Inline formulas are a handful of pixels wide; they must render at a
        // few cells, not balloon to the full screen.
        let (cols, rows) = compute_dims(96, 48, Some(&caps(8, 16)), 94, 40);
        assert!(cols <= 12, "inline math must stay small, got {cols} cols");
        assert!(rows <= 4, "inline math must stay short, got {rows} rows");
    }

    #[test]
    fn wide_image_downscales_to_fit_box() {
        // 3200×1800 into a 94×40 box: downscales to width 94, aspect intact.
        let (cols, rows) = compute_dims(3200, 1800, Some(&caps(8, 16)), 94, 40);
        assert_eq!(cols, 94, "wide image fills the width when it exceeds it");
        assert!(rows <= 40, "rows must not exceed max_rows");
        // aspect of 3200/1800 ≈ 94/26.4
        assert!((rows as i64 - 26).abs() <= 1);
    }

    #[test]
    fn tall_image_downscales_by_height() {
        let (cols, rows) = compute_dims(600, 2400, Some(&caps(8, 16)), 94, 40);
        assert_eq!(
            rows, 40,
            "tall image must fill the height when it exceeds it"
        );
        assert!(cols <= 94);
        assert!(cols > 0);
    }

    #[test]
    fn square_image_fits_box() {
        let (cols, rows) = compute_dims(1000, 1000, Some(&caps(8, 16)), 80, 40);
        assert!(cols <= 80 && rows <= 40);
        assert!(cols >= 1 && rows >= 1);
        assert!(
            !(cols == 80 && rows == 40),
            "square must not be stretched to the box"
        );
    }

    #[test]
    fn nominal_cells_used_when_no_caps() {
        // Default 8×16 cells => 800px wide natural is 100 cols, exceeds the
        // 80-col box, so it downscales to the full width (aspect intact).
        let (cols, rows) = compute_dims(800, 400, None, 80, 40);
        assert_eq!(cols, 80, "800px natural at 8px/cell, capped to the box");
        assert_eq!(rows, 20, "aspect-preserving downscale");
    }

    #[test]
    fn never_exceeds_box() {
        for (w, h) in [
            (3200, 1800),
            (500, 5000),
            (10000, 200),
            (1, 9999),
            (9999, 1),
        ] {
            let (cols, rows) = compute_dims(w, h, Some(&caps(16, 32)), 120, 50);
            assert!(cols <= 120 && rows <= 50, "[{w}x{h}] -> {cols}x{rows}");
            assert!(cols >= 1 && rows >= 1);
        }
    }

    #[test]
    fn zero_pixels_returns_unit() {
        assert_eq!(compute_dims(0, 100, None, 80, 40), (1, 1));
        assert_eq!(compute_dims(100, 0, None, 80, 40), (1, 1));
    }

    #[test]
    fn runtime_scale_upscales_small_image() {
        // A 200×100 image is naturally 25×6 cells. At 2.0 it grows to 50×12
        // (still under the 94×40 box, so the natural size is returned).
        let prev = crate::output::capabilities::graphics_scale();
        crate::output::capabilities::set_graphics_scale(2.0);
        let (cols, rows) = compute_dims(200, 100, Some(&caps(8, 16)), 94, 40);
        assert_eq!(cols, 50, "2x scale should double the natural width");
        assert_eq!(rows, 12, "2x scale should double the natural height");
        crate::output::capabilities::set_graphics_scale(prev);
    }

    #[test]
    fn runtime_scale_never_exceeds_box() {
        let prev = crate::output::capabilities::graphics_scale();
        crate::output::capabilities::set_graphics_scale(4.0);
        for (w, h) in [(3200, 1800), (400, 200), (1000, 1000)] {
            let (cols, rows) = compute_dims(w, h, Some(&caps(8, 16)), 94, 40);
            assert!(cols <= 94 && rows <= 40, "[{w}x{h}] -> {cols}x{rows}");
            assert!(cols >= 1 && rows >= 1);
        }
        crate::output::capabilities::set_graphics_scale(prev);
    }

    #[test]
    fn graphics_scale_is_clamped() {
        let prev = crate::output::capabilities::graphics_scale();
        assert!(crate::output::capabilities::set_graphics_scale(999.0));
        assert!(
            crate::output::capabilities::graphics_scale()
                <= crate::output::capabilities::GRAPHICS_SCALE_MAX
        );
        crate::output::capabilities::set_graphics_scale(prev);
    }

    #[test]
    fn raster_max_width_is_capped() {
        assert!(raster_max_width(80, 8) <= MAX_RASTER_PX);
        // 2x rasterization of an 80-col layout at 8px cells = 1280
        assert_eq!(raster_max_width(80, 8), 1280);
        // Absurd cell sizes still bounded
        assert!(raster_max_width(100, 10_000) <= MAX_RASTER_PX);
    }

    // ── source_crop (proportional visible band) ────────────────────────────

    #[test]
    fn full_visibility_uses_whole_raster() {
        assert_eq!(
            source_crop(3200, 1800, 94, 26, 94, 26, 0),
            (0, 0, 3200, 1800)
        );
    }

    #[test]
    fn scrolled_clip_is_proportional() {
        // half the rows visible starting 1 row in (of 26) on a 3200x1800 raster
        let (sx, sy, sw, sh) = source_crop(3200, 1800, 100, 40, 100, 20, 3);
        assert_eq!(sx, 0);
        assert_eq!(sw, 3200, "full width when not clipped horizontally");
        assert_eq!(sh, 900, "half of 1800 for 20/40 rows");
        assert_eq!(sy, 135, "3/40 of 1800 rows");
        // the visible band must stay inside the raster
        assert!(sy + sh <= 1800);
    }

    #[test]
    fn horizontal_clip_keeps_full_width() {
        // Even when the visible window is narrower than the full diagram, the
        // full raster width is used and the terminal downscales the placement,
        // so the image is never chopped horizontally.
        let (sx, _sy, sw, sh) = source_crop(2000, 1000, 100, 50, 40, 50, 0);
        assert_eq!(sx, 0);
        assert_eq!(sw, 2000, "full raster width always kept");
        assert_eq!(sh, 1000, "full height");
    }

    #[test]
    fn height_cache_key_matches_estimator_format_for_images() {
        let cache = ImageHeightCache::new();
        let estimator_key = format!("{}:{}", 209, "../rivas.png");
        assert_eq!(estimator_key, "209:../rivas.png");
        cache.set(&estimator_key, 90, 40);
        assert_eq!(cache.get(&estimator_key), Some((90, 40)));
    }

    #[test]
    fn height_cache_key_matches_estimator_format_for_math() {
        let cache = ImageHeightCache::new();
        let estimator_key = format!("math:{}:{}:{}", 209, false, "E = mc^2");
        assert_eq!(estimator_key, "math:209:false:E = mc^2");
        cache.set(&estimator_key, 6, 1);
        assert_eq!(cache.get(&estimator_key), Some((6, 1)));
    }

    #[test]
    fn height_cache_key_matches_estimator_format_for_mermaid() {
        let cache = ImageHeightCache::new();
        let estimator_key = format!("mermaid:{}:{}", 209, "flowchart LR\n  A --> B");
        cache.set(&estimator_key, 42, 3);
        assert_eq!(cache.get(&estimator_key), Some((42, 3)));
    }

    #[test]
    fn height_cache_store_strips_instance_id() {
        let full_key = "209:../rivas.png#6";
        let estimator_key = full_key
            .rsplit_once('#')
            .map(|(k, _)| k)
            .unwrap_or(full_key);
        assert_eq!(estimator_key, "209:../rivas.png");
        let cache = ImageHeightCache::new();
        cache.set(estimator_key, 90, 40);
        assert_eq!(cache.get(estimator_key), Some((90, 40)));
        assert_eq!(cache.get(full_key), None);
    }

    #[test]
    fn height_cache_generation_increments_on_change() {
        let cache = ImageHeightCache::new();
        let gen0 = cache.generation();
        cache.set("key1", 10, 5);
        let gen1 = cache.generation();
        assert!(gen1 > gen0);
        cache.set("key1", 10, 5);
        assert_eq!(cache.generation(), gen1);
        cache.set("key1", 10, 6);
        assert!(cache.generation() > gen1);
    }

    #[test]
    fn height_cache_no_instance_id_in_key() {
        let key = format!("{}:{}", 209, "../rivas.png");
        assert!(
            !key.contains('#'),
            "Image height cache key should not contain instance_id"
        );
    }
}
