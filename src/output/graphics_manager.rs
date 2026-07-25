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
    pub cell_w: u32,
    pub cell_h: u32,
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

/// Global cache of real image dimensions (cols, rows) keyed by the same key the
/// components use. Lets the virtual-scrolling height estimator reuse real
/// heights and keeps a single source of truth (owned here, not in components).
pub struct ImageHeightCache {
    heights: Mutex<HashMap<String, (u32, u32)>>,
    generation: AtomicU64,
}

impl ImageHeightCache {
    pub fn new() -> Self {
        Self {
            heights: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
        }
    }
    pub fn get(&self, key: &str) -> Option<(u32, u32)> {
        self.heights.lock().ok().and_then(|m| m.get(key).copied())
    }
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
pub struct GraphicsManager {
    tx: Sender<Cmd>,
    registry: Arc<Mutex<HashMap<String, Entry>>>,
}

lazy_static::lazy_static! {
    static ref MANAGER: GraphicsManager = GraphicsManager::new();
}

pub fn graphics() -> &'static GraphicsManager {
    &MANAGER
}

fn encode(raw: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(raw)
}

fn compute_dims(
    pixel_w: u32,
    pixel_h: u32,
    caps: Option<&TermCaps>,
    max_cols: u32,
    max_rows: u32,
) -> (u32, u32) {
    let cw = caps.map(|c| c.cell_w_px.max(1) as f32).unwrap_or(8.0);
    let ch = caps.map(|c| c.cell_h_px.max(1) as f32).unwrap_or(16.0);
    let mut cols = (pixel_w as f32 / cw).ceil() as u32;
    let mut rows = (pixel_h as f32 / ch).ceil() as u32;
    cols = cols.min(max_cols);
    rows = rows.min(max_rows);
    (cols, rows)
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
            let (png, w, h) =
                render_math(&content, display, max_w, true).map_err(|e| format!("{:#}", e))?;
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

fn transmit_at(
    id: u32,
    data: &Arc<String>,
    frames: &[(Arc<String>, u32)],
    has_anim: bool,
    rect: GfxRect,
) {
    let cell_w = rect.cell_w;
    let cell_h = rect.cell_h;
    let src_y_px = rect.src_y_offset as u32 * cell_h;
    let crop_w_px = rect.vis_cols as u32 * cell_w;
    let crop_h_px = rect.vis_rows as u32 * cell_h;

    // a=t transmits image data into the terminal's graphic store without
    // creating any visual placement — no cursor movement, no scroll damage.
    let mut buf = PENDING_OUTPUT.lock().unwrap();
    kitty::transmit_only_encoded(
        &mut *buf,
        id,
        data.as_str(),
        rect.vis_cols as u32,
        rect.vis_rows as u32,
        0,
        src_y_px,
        crop_w_px,
        crop_h_px,
    );
    if has_anim {
        let fr: Vec<(&str, u32)> = frames.iter().map(|(s, d)| (s.as_str(), *d)).collect();
        kitty::write_animation_frames_encoded(&mut *buf, id, &fr);
        kitty::start_animation(&mut *buf, id);
    }

    debug::log_event(&debug::DebugEvent::KittyTransmit {
        ts: debug::elapsed_ms(),
        id,
        cols: rect.vis_cols as u32,
        rows: rect.vis_rows as u32,
        crop_x: 0,
        crop_y: src_y_px,
        crop_w: crop_w_px,
        crop_h: crop_h_px,
        data_size: data.len(),
        has_animation: has_anim,
    });
}

fn place_at(id: u32, rect: GfxRect) {
    let cell_w = rect.cell_w;
    let cell_h = rect.cell_h;
    let src_y_px = rect.src_y_offset as u32 * cell_h;
    let crop_w_px = rect.vis_cols as u32 * cell_w;
    let crop_h_px = rect.vis_rows as u32 * cell_h;

    let mut buf = PENDING_OUTPUT.lock().unwrap();
    write!(buf, "\x1b7").unwrap();
    write!(buf, "\x1b[{};{}H", rect.y + 1, rect.x + 1).unwrap();
    kitty::delete_placements(&mut *buf, id);
    kitty::place_image(
        &mut *buf,
        id,
        rect.vis_cols as u32,
        rect.vis_rows as u32,
        0,
        src_y_px,
        crop_w_px,
        crop_h_px,
    );
    write!(buf, "\x1b8").unwrap();

    debug::log_event(&debug::DebugEvent::KittyPlace {
        ts: debug::elapsed_ms(),
        id,
        cols: rect.vis_cols as u32,
        rows: rect.vis_rows as u32,
        crop_x: 0,
        crop_y: src_y_px,
        crop_w: crop_w_px,
        crop_h: crop_h_px,
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
                    let cw = caps.as_ref().map(|c| c.cell_w_px as u32).unwrap_or(8);
                    let ch = caps.as_ref().map(|c| c.cell_h_px as u32).unwrap_or(16);

                    let (kitty_id, visible, desired, data, frames, has_anim) = {
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
                        )
                    };

                    let rect = desired.unwrap_or(GfxRect {
                        x: 0,
                        y: 0,
                        vis_cols: cell_cols as i32,
                        vis_rows: cell_rows as i32,
                        src_y_offset: 0,
                        cell_w: cw,
                        cell_h: ch,
                    });
                    transmit_at(kitty_id, &data, &frames, has_anim, rect);
                    // If a Place command was received before loading completed,
                    // place the now-cached image at the target position.
                    if visible {
                        place_at(kitty_id, rect);
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
                    let (kitty_id, ready) = {
                        let mut reg = registry.lock().unwrap();
                        let en = match reg.get_mut(&key) {
                            Some(e) => e,
                            None => continue,
                        };
                        en.desired = Some(rect);
                        en.visible = true;
                        en.last_used = tick;
                        (en.kitty_id, matches!(en.status, EntryStatus::Ready(..)))
                    };
                    if ready {
                        place_at(kitty_id, rect);
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

pub fn acquire(key: String, source: GfxSource) {
    graphics().send(Cmd::Acquire { key, source });
}

pub fn place(key: String, rect: GfxRect) {
    graphics().send(Cmd::Place { key, rect });
}

pub fn detach(key: String) {
    graphics().send(Cmd::Detach { key });
}

pub fn release(key: String) {
    graphics().send(Cmd::Release { key });
}

pub fn dims(key: &str) -> Option<(u32, u32)> {
    graphics().dims(key)
}

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
