use std::{
    io::Write,
    sync::atomic::{AtomicU32, Ordering},
};

const CHUNK_SIZE: usize = 4096;

/// Returns `true` if the terminal supports the Kitty graphics protocol.
///
/// Checks `$TERM_PROGRAM` for kitty/WezTerm/ghostty, or `$TERM` for `kitty`.
pub fn is_supported() -> bool {
    if let Ok(term) = std::env::var("TERM_PROGRAM") {
        return matches!(term.as_str(), "kitty" | "WezTerm" | "ghostty");
    }
    if let Ok(term) = std::env::var("TERM") {
        return term.contains("kitty");
    }
    false
}

static NEXT_PLACEMENT_ID: AtomicU32 = AtomicU32::new(1);

/// Allocates and returns the next unique placement ID for Kitty graphics.
///
/// IDs are 24-bit (max `0x00FF_FFFF`) and increment atomically.
pub fn next_placement_id() -> u32 {
    NEXT_PLACEMENT_ID.fetch_add(1, Ordering::Relaxed) & 0x00FF_FFFF
}

// --- helpers ---

fn crop_string(src_x: u32, src_y: u32, src_w: u32, src_h: u32) -> String {
    match (src_w > 0 || src_h > 0, src_x > 0 || src_y > 0) {
        (true, _) => format!(",x={},y={},w={},h={}", src_x, src_y, src_w, src_h),
        (false, true) => format!(",x={},y={}", src_x, src_y),
        (false, false) => String::new(),
    }
}

fn chunked_write<W: Write>(w: &mut W, first_control: &str, rest_control: &str, data: &str) {
    let bytes = data.as_bytes();
    let mut offset = 0;
    let len = bytes.len();
    while offset < len {
        let end = (offset + CHUNK_SIZE).min(len);
        let chunk = std::str::from_utf8(&bytes[offset..end]).unwrap();
        let more = if end < len { 1 } else { 0 };
        if offset == 0 {
            write!(w, "\x1b_G{},m={},q=2;{}\x1b\\", first_control, more, chunk).unwrap();
        } else if rest_control.is_empty() {
            write!(w, "\x1b_Gm={};{}\x1b\\", more, chunk).unwrap();
        } else {
            write!(w, "\x1b_G{},m={};{}\x1b\\", rest_control, more, chunk).unwrap();
        }
        offset = end;
    }
}

// --- raw-data API (base64-encode internally on every call) ---

/// Transmit image data into the terminal's graphic store without creating a
/// visual placement.  Uses `a=t` (transmit-only) so no image appears at the
/// cursor — the caller can later use `a=p` to place the cached data.
pub fn transmit_only_encoded<W: Write>(
    w: &mut W,
    id: u32,
    encoded: &str,
    cols: u32,
    rows: u32,
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
) {
    let crop = crop_string(src_x, src_y, src_w, src_h);
    chunked_write(
        w,
        &format!("a=t,f=100,t=d,i={},c={},r={}{}", id, cols, rows, crop),
        "",
        encoded,
    );
}

/// Transmits multiple animation frames to the terminal's graphic store.
///
/// Each frame is a base64-encoded PNG with a delay in milliseconds.
/// Uses the `a=f` (frame) command for animated GIF support.
pub fn write_animation_frames_encoded<W: Write>(w: &mut W, id: u32, frames: &[(&str, u32)]) {
    for (encoded, delay_ms) in frames {
        chunked_write(
            w,
            &format!("a=f,f=100,i={},z={}", id, delay_ms),
            "a=f",
            encoded,
        );
    }
}

// --- commands ---

/// Starts animation playback for a previously transmitted animated image.
pub fn start_animation<W: Write>(w: &mut W, id: u32) {
    write!(w, "\x1b_Ga=a,i={},s=3,v=1,q=2;\x1b\\", id).unwrap();
}

/// Delete placements only (lowercase d=i). Keeps image data cached so a=p can
/// re-display it without retransmission.
pub fn delete_placements<W: Write>(w: &mut W, id: u32) {
    write!(w, "\x1b_Ga=d,d=i,i={},q=2;\x1b\\", id).unwrap();
}

/// Delete placements AND free image data (uppercase d=I).
pub fn delete_image<W: Write>(w: &mut W, id: u32) {
    write!(w, "\x1b_Ga=d,d=I,i={},q=2;\x1b\\", id).unwrap();
}

/// Deletes all Kitty graphic placements and frees associated image data.
pub fn delete_all<W: Write>(w: &mut W) {
    write!(w, "\x1b_Ga=d,d=a,q=2;\x1b\\").unwrap();
}

/// Place an already-transmitted image at the cursor position without retransmitting data.
/// Each call creates a fresh placement — no placement ID is used so the placement is
/// always positioned at the current cursor when the escape code is emitted.
pub fn place_image<W: Write>(
    w: &mut W,
    id: u32,
    cols: u32,
    rows: u32,
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
) {
    let crop = crop_string(src_x, src_y, src_w, src_h);
    write!(
        w,
        "\x1b_Ga=p,i={},c={},r={}{},q=2;\x1b\\",
        id, cols, rows, crop
    )
    .unwrap();
}
