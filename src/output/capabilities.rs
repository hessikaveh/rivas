use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};

/// Global kitty support flag. Set once during `TermCaps::detect()`, can be
/// overridden by `--force-kitty`.
static HAS_KITTY: AtomicBool = AtomicBool::new(false);

/// Returns `true` if the terminal supports the Kitty graphics protocol.
/// Checks the global flag set during capability detection (or overridden by
/// `--force-kitty`).
pub fn has_kitty() -> bool {
    HAS_KITTY.load(Ordering::Relaxed)
}

/// Force-set the global kitty support flag. Used by `--force-kitty`.
pub fn force_kitty() {
    HAS_KITTY.store(true, Ordering::Relaxed);
}

/// CLI override for the cell pixel size, applied before `detect()`. Because the
/// `TIOCGWINSZ`-derived cell size is unreliable on some terminals / secondary
/// displays (it can be wrong or stale), `--cell-width`/`--cell-height` let the
/// user (or a test script) pin it explicitly.
static CELL_W_PX: AtomicU16 = AtomicU16::new(0);
static CELL_H_PX: AtomicU16 = AtomicU16::new(0);

pub fn override_cell_size(w: u16, h: u16) {
    CELL_W_PX.store(w, Ordering::Relaxed);
    CELL_H_PX.store(h, Ordering::Relaxed);
}

/// Live, in-app multiplier applied to the natural size of every graphic so the
/// user can resize images/diagrams on the fly (default 1.0). Stored fixed-point
/// (×1000) to avoid floats in atomics and to make monotonic nudges exact.
static GRAPHICS_SCALE_X1000: AtomicU32 = AtomicU32::new(1000);

/// Bounds multipliers are clamped to (no 0x / absurd sizes).
pub const GRAPHICS_SCALE_MIN: f32 = 0.25;
pub const GRAPHICS_SCALE_MAX: f32 = 4.0;

/// Returns the current runtime graphics scale multiplier.
pub fn graphics_scale() -> f32 {
    GRAPHICS_SCALE_X1000.load(Ordering::Relaxed) as f32 / 1000.0
}

/// Sets the runtime graphics scale multiplier (clamped), then returns whether
/// it actually changed. Callers should recompute cached graphic dimensions and
/// re-render so the change shows immediately.
pub fn set_graphics_scale(scale: f32) -> bool {
    let v = (scale.clamp(GRAPHICS_SCALE_MIN, GRAPHICS_SCALE_MAX) * 1000.0).round() as u32;
    if GRAPHICS_SCALE_X1000.load(Ordering::Relaxed) == v {
        return false;
    }
    GRAPHICS_SCALE_X1000.store(v, Ordering::Relaxed);
    true
}

/// Terminal capability information detected at startup.
#[derive(Clone, Debug)]
pub struct TermCaps {
    /// Width of a single cell in pixels (for Kitty image sizing).
    pub cell_w_px: u16,
    /// Height of a single cell in pixels (for Kitty image sizing).
    pub cell_h_px: u16,
}

impl Default for TermCaps {
    fn default() -> Self {
        Self {
            cell_w_px: 8,
            cell_h_px: 16,
        }
    }
}

impl TermCaps {
    /// Detects terminal capabilities by querying the terminal for cell pixel size
    /// and Kitty graphics support.
    ///
    /// Sets the global Kitty flag based on `$TERM`/`$TERM_PROGRAM` detection.
    pub fn detect() -> Result<Self> {
        let ow = CELL_W_PX.load(Ordering::Relaxed);
        let oh = CELL_H_PX.load(Ordering::Relaxed);
        let (cell_w_px, cell_h_px) = if ow > 0 && oh > 0 {
            (ow, oh)
        } else {
            cell_pixel_size().unwrap_or((8, 16))
        };
        let kitty = detect_kitty();
        HAS_KITTY.store(kitty, Ordering::Relaxed);
        Ok(Self {
            cell_w_px,
            cell_h_px,
        })
    }
}

#[cfg(windows)]
fn cell_pixel_size() -> Option<(u16, u16)> {
    None
}

#[cfg(unix)]
fn cell_pixel_size() -> Option<(u16, u16)> {
    unsafe {
        let mut ws: std::mem::MaybeUninit<libc::winsize> = std::mem::MaybeUninit::uninit();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, ws.as_mut_ptr()) == 0 {
            let ws = ws.assume_init();
            if ws.ws_xpixel > 0 && ws.ws_col > 0 {
                return Some((ws.ws_xpixel / ws.ws_col, ws.ws_ypixel / ws.ws_row));
            }
        }
    }
    None
}

fn detect_kitty() -> bool {
    let t = std::env::var("TERM").unwrap_or_default();
    let p = std::env::var("TERM_PROGRAM").unwrap_or_default();
    t.contains("kitty")
        || ["kitty", "wezterm", "ghostty"]
            .iter()
            .any(|k| p.eq_ignore_ascii_case(k))
}
