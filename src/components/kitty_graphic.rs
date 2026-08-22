use crate::components::scroll::ScrollPosition;
use crate::debug;
use crate::output::graphics_manager::{
    GfxRect, GfxSource, IMAGE_HEIGHT_CACHE, ReleaseGuard, acquire, detach, dims, gfx_error, place,
    release,
};
use iocraft::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Unique id generator for graphics components. Each occurrence of an image,
/// formula or diagram gets its own terminal graphic id so that placing/
/// detaching one occurrence never affects another that shares the same content.
static INSTANCE_ID: AtomicU64 = AtomicU64::new(0);
pub fn next_instance_id() -> u64 {
    INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Layout snapshot returned by [`UseKittyGraphic::use_kitty_graphic`].
pub struct KittyGraphic {
    /// Currently known rendered width in columns (0 while loading).
    pub cols: u32,
    /// Currently known rendered height in rows (0 while loading).
    pub rows: u32,
    /// Stable layout height from the height cache (never collapses to 0 while
    /// loading), used to size the spacer box.
    pub declared_rows: u32,
    /// Load error, if any.
    pub error: Option<String>,
}

/// Shared implementation behind every Kitty-graphics component (`KittyImage`,
/// `KittyMath`, `KittyMermaid`): acquires/releases the terminal graphic when
/// its key changes, polls the manager for dimensions and errors, places or
/// detaches pixels based on the component rect with scroll-stable positioning,
/// and releases the graphic on unmount.
///
/// Returns a plain-value snapshot each render for the component's own layout
/// (spacer sizing, error text, debug annotations).
pub trait UseKittyGraphic {
    fn use_kitty_graphic(
        &mut self,
        base_key: String,
        vw: u32,
        vh: u32,
        scroll_offset: Option<i32>,
        fallback_rows: u32,
        make_source: impl FnOnce(u32, u32, u32) -> GfxSource,
    ) -> KittyGraphic;
}

impl UseKittyGraphic for Hooks<'_, '_> {
    fn use_kitty_graphic(
        &mut self,
        base_key: String,
        vw: u32,
        vh: u32,
        scroll_offset: Option<i32>,
        fallback_rows: u32,
        make_source: impl FnOnce(u32, u32, u32) -> GfxSource,
    ) -> KittyGraphic {
        let instance = self.use_ref(|| next_instance_id());
        let key = format!("{}#{}", base_key, *instance.read());
        let (cached_cols, cached_rows) = dims(&key).unwrap_or((0, 0));
        let declared_rows = IMAGE_HEIGHT_CACHE
            .get(&base_key)
            .map(|(_, h)| h)
            .unwrap_or(fallback_rows);

        let rect = self.use_component_rect();
        let (term_width, term_height) = self.use_terminal_size();
        let mut drawn_at = self.use_state(|| (-1i32, -1i32));
        let mut cols = self.use_ref(|| cached_cols);
        let mut rows = self.use_ref(|| cached_rows);
        let mut error_msg = self.use_state(|| None::<String>);
        let mut acquired_key = self.use_ref(|| String::new());
        let cur_key = self.use_ref(|| Arc::new(Mutex::new(String::new())));
        let caps_cache = self.use_ref(|| crate::output::capabilities::TermCaps::detect().ok());
        let mut scroll_pos = ScrollPosition::new();

        // Acquire (or re-acquire on key change). The manager caches by `key`, so
        // a remounted component reuses the already-transmitted terminal image.
        if acquired_key.read().is_empty() || *acquired_key.read() != key {
            if !acquired_key.read().is_empty() {
                release(acquired_key.read().clone());
            }
            let mc = vw.saturating_sub(crate::theme::CONTENT_H_INSET).max(1);
            let mr = vh.saturating_sub(crate::theme::CONTENT_V_INSET).max(1);
            let caps = caps_cache.read().clone().unwrap_or_default();
            let max_w =
                crate::output::graphics_manager::raster_max_width(mc, caps.cell_w_px.max(1) as u32);
            acquire(key.clone(), make_source(max_w, mc, mr));
            *cur_key.read().lock().unwrap() = key.clone();
            acquired_key.set(key.clone());
            error_msg.set(None);
            if dims(&key).is_none() {
                cols.set(0);
                rows.set(0);
            }
        }

        // Poll the manager for dimensions / error and reactively update layout.
        if let Some((c, r)) = dims(&key) {
            if *cols.read() != c || *rows.read() != r {
                cols.set(c);
                rows.set(r);
                // Force a re-evaluation so a Place is emitted now that we know size.
                drawn_at.set((-1, -1));
            }
        }
        if let Some(err) = gfx_error(&key) {
            if error_msg.read().is_none() {
                error_msg.set(Some(err));
            }
        }

        if let Some(r) = rect {
            let x = r.left;
            let y_raw = r.top;
            // Scroll-invariant y: `rect.top` lags one frame behind scrolls, so
            // add back the scroll offset that was in effect at measurement time.
            let so = scroll_offset.unwrap_or_else(|| scroll_pos.captured_scroll_offset());
            scroll_pos.update(y_raw, so);
            let y = scroll_pos.y(so);

            let pos = (x, y);
            if pos != drawn_at.get() {
                drawn_at.set(pos);

                let img_cols = *cols.read() as i32;
                let img_rows = *rows.read() as i32;

                let visible_cols = img_cols.min(term_width as i32 - x).max(0);
                let visible_rows = img_rows.min(term_height as i32 - y - 3).max(0);

                let top_clip_rows = if y < 0 { (-y + 1).min(img_rows) } else { 0 };
                let actual_vis_rows = (visible_rows - top_clip_rows).max(0);
                let render_y = if y < 1 { 1 } else { y };

                let visible = x >= 0 && actual_vis_rows > 0 && visible_cols > 0;

                let rect_cmd = GfxRect {
                    x,
                    y: render_y,
                    vis_cols: visible_cols,
                    vis_rows: actual_vis_rows,
                    src_y_offset: top_clip_rows,
                };

                if visible {
                    place(key.clone(), rect_cmd);
                    debug::log_event(&debug::DebugEvent::ImagePlace {
                        ts: debug::elapsed_ms(),
                        id: 0,
                        x,
                        y: render_y,
                        cols: visible_cols,
                        rows: actual_vis_rows,
                        src_y_offset: top_clip_rows,
                    });
                } else {
                    detach(key.clone());
                    debug::log_event(&debug::DebugEvent::ImageDetach {
                        ts: debug::elapsed_ms(),
                        id: 0,
                        reason: "scrolled_offscreen".into(),
                    });
                }
            }
        }

        // Release the terminal-side image when the component unmounts.
        let _release_guard = self.use_ref({
            let ck = cur_key.read().clone();
            move || ReleaseGuard { key: ck }
        });

        KittyGraphic {
            cols: *cols.read(),
            rows: *rows.read(),
            declared_rows,
            error: error_msg.read().clone(),
        }
    }
}
