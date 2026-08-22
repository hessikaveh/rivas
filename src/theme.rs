use iocraft::prelude::Color;
use std::sync::atomic::{AtomicUsize, Ordering};

const fn rgb(hex: u32) -> Color {
    Color::Rgb {
        r: (hex >> 16) as u8,
        g: ((hex >> 8) & 0xff) as u8,
        b: (hex & 0xff) as u8,
    }
}

/// A full UI palette. All colors are resolved at render time through the
/// accessor functions below (`theme::bg()`, `theme::fg()`, ...) so switching
/// the active theme takes effect on the next frame.
pub struct Theme {
    pub name: &'static str,
    /// Whether this palette is intended for dark terminals.
    pub dark: bool,
    pub bg: Color,
    pub fg: Color,
    pub dark_bg: Color,
    pub status_bg: Color,
    pub border: Color,
    pub red: Color,
    pub orange: Color,
    pub yellow: Color,
    pub green: Color,
    pub teal: Color,
    pub cyan: Color,
    pub blue: Color,
    pub magenta: Color,
    pub comment: Color,
    pub dark_grey: Color,
    /// Background used behind debug overlay annotations.
    pub dbg_bg: Color,
}

// ── Built-in palettes ─────────────────────────────────────────────────────────

pub static TOKYO_NIGHT: Theme = Theme {
    name: "tokyo-night",
    dark: true,
    bg: rgb(0x1a1b26),
    fg: rgb(0xa9b1d6),
    dark_bg: rgb(0x16161e),
    status_bg: rgb(0x1f2335),
    border: rgb(0x3b4261),
    red: rgb(0xf7768e),
    orange: rgb(0xff9e64),
    yellow: rgb(0xe0af68),
    green: rgb(0x9ece6a),
    teal: rgb(0x73daca),
    cyan: rgb(0x7dcfff),
    blue: rgb(0x7aa2f7),
    magenta: rgb(0xbb9af3),
    comment: rgb(0x565f89),
    dark_grey: rgb(0x414868),
    dbg_bg: rgb(0x231c12),
};

pub static DRACULA: Theme = Theme {
    name: "dracula",
    dark: true,
    bg: rgb(0x282a36),
    fg: rgb(0xf8f8f2),
    dark_bg: rgb(0x21222c),
    status_bg: rgb(0x191a21),
    border: rgb(0x44475a),
    red: rgb(0xff5555),
    orange: rgb(0xffb86c),
    yellow: rgb(0xf1fa8c),
    green: rgb(0x50fa7b),
    teal: rgb(0x00b7b0),
    cyan: rgb(0x8be9fd),
    blue: rgb(0xbd93f9),
    magenta: rgb(0xff79c6),
    comment: rgb(0x6272a4),
    dark_grey: rgb(0x44475a),
    dbg_bg: rgb(0x2c2418),
};

pub static NORD: Theme = Theme {
    name: "nord",
    dark: true,
    bg: rgb(0x2e3440),
    fg: rgb(0xd8dee9),
    dark_bg: rgb(0x242933),
    status_bg: rgb(0x3b4252),
    border: rgb(0x434c5e),
    red: rgb(0xbf616a),
    orange: rgb(0xd08770),
    yellow: rgb(0xebcb8b),
    green: rgb(0xa3be8c),
    teal: rgb(0x8fbcbb),
    cyan: rgb(0x88c0d0),
    blue: rgb(0x81a1c1),
    magenta: rgb(0xb48ead),
    comment: rgb(0x616e88),
    dark_grey: rgb(0x4c566a),
    dbg_bg: rgb(0x33302a),
};

pub static GRUVBOX_DARK: Theme = Theme {
    name: "gruvbox-dark",
    dark: true,
    bg: rgb(0x282828),
    fg: rgb(0xebdbb2),
    dark_bg: rgb(0x1d2021),
    status_bg: rgb(0x32302f),
    border: rgb(0x504945),
    red: rgb(0xfb4934),
    orange: rgb(0xfe8019),
    yellow: rgb(0xfabd2f),
    green: rgb(0xb8bb26),
    teal: rgb(0x8ec07c),
    cyan: rgb(0x83a598),
    blue: rgb(0x83a598),
    magenta: rgb(0xd3869b),
    comment: rgb(0x928374),
    dark_grey: rgb(0x665c54),
    dbg_bg: rgb(0x322d22),
};

pub static CATPPUCCIN_MOCHA: Theme = Theme {
    name: "catppuccin-mocha",
    dark: true,
    bg: rgb(0x1e1e2e),
    fg: rgb(0xcdd6f4),
    dark_bg: rgb(0x11111b),
    status_bg: rgb(0x181825),
    border: rgb(0x45475a),
    red: rgb(0xf38ba8),
    orange: rgb(0xfab387),
    yellow: rgb(0xf9e2af),
    green: rgb(0xa6e3a1),
    teal: rgb(0x94e2d5),
    cyan: rgb(0x89dceb),
    blue: rgb(0x89b4fa),
    magenta: rgb(0xcba6f7),
    comment: rgb(0x6c7086),
    dark_grey: rgb(0x45475a),
    dbg_bg: rgb(0x30281a),
};

pub static SOLARIZED_LIGHT: Theme = Theme {
    name: "solarized-light",
    dark: false,
    bg: rgb(0xfdf6e3),
    fg: rgb(0x657b83),
    dark_bg: rgb(0xeee8d5),
    status_bg: rgb(0xeee8d5),
    border: rgb(0x93a1a1),
    red: rgb(0xdc322f),
    orange: rgb(0xcb4b16),
    yellow: rgb(0xb58900),
    green: rgb(0x859900),
    teal: rgb(0x6c71c4),
    cyan: rgb(0x2aa198),
    blue: rgb(0x268bd2),
    magenta: rgb(0xd33682),
    comment: rgb(0x93a1a1),
    dark_grey: rgb(0xd5cdb4),
    dbg_bg: rgb(0xf3ead0),
};

pub static THEMES: &[&Theme] = &[
    &TOKYO_NIGHT,
    &DRACULA,
    &NORD,
    &GRUVBOX_DARK,
    &CATPPUCCIN_MOCHA,
    &SOLARIZED_LIGHT,
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// All available theme names (in cycling order).
pub fn names() -> impl Iterator<Item = &'static str> {
    THEMES.iter().map(|t| t.name)
}

/// Selects a theme by name. Returns `false` when the name is unknown.
pub fn set_by_name(name: &str) -> bool {
    match THEMES
        .iter()
        .position(|t| t.name.eq_ignore_ascii_case(name))
    {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Switches to the next built-in theme (wraps around).
pub fn cycle() {
    let next = (CURRENT.load(Ordering::Relaxed) + 1) % THEMES.len();
    CURRENT.store(next, Ordering::Relaxed);
}

pub fn current() -> &'static Theme {
    THEMES[CURRENT.load(Ordering::Relaxed)]
}

pub fn index() -> usize {
    CURRENT.load(Ordering::Relaxed)
}

pub fn is_dark() -> bool {
    current().dark
}

macro_rules! accessors {
    ($($fn:ident => $field:ident),* $(,)?) => {$(
        // Not every palette accent has a dedicated consumer.
        #[allow(dead_code)]
        pub fn $fn() -> Color {
            current().$field
        }
    )*};
}

accessors! {
    bg => bg,
    fg => fg,
    dark_bg => dark_bg,
    status_bg => status_bg,
    border => border,
    red => red,
    orange => orange,
    yellow => yellow,
    green => green,
    teal => teal,
    cyan => cyan,
    blue => blue,
    magenta => magenta,
    comment => comment,
    dark_grey => dark_grey,
    dbg_bg => dbg_bg,
}

// ── Debug overlay colors (mapped onto the active palette's accents) ──────────
pub fn dbg_heading() -> Color {
    current().blue
}
pub fn dbg_paragraph() -> Color {
    current().fg
}
pub fn dbg_code() -> Color {
    current().green
}
pub fn dbg_image() -> Color {
    current().orange
}
pub fn dbg_math() -> Color {
    current().magenta
}
pub fn dbg_mermaid() -> Color {
    current().cyan
}
pub fn dbg_quote() -> Color {
    current().yellow
}
pub fn dbg_table() -> Color {
    current().teal
}
pub fn dbg_list() -> Color {
    current().green
}
pub fn dbg_break() -> Color {
    current().comment
}
pub fn dbg_html() -> Color {
    current().red
}

pub const VIEWPORT_BORDER_WIDTH: u32 = 2;
pub const VIEWPORT_SCROLLBAR_WIDTH: u32 = 1;
pub const VIEWPORT_INNER_PADDING: u32 = 4;
pub const BLOCK_PADDING: u32 = 4;
pub const TOTAL_VIEWPORT_OFFSET: u32 =
    VIEWPORT_BORDER_WIDTH + VIEWPORT_SCROLLBAR_WIDTH + VIEWPORT_INNER_PADDING + BLOCK_PADDING;

/// Horizontal room taken up by the viewport chrome (border + left/right padding)
/// that graphics are placed inside, in columns. Used to cap image/diagram width
/// so they always fit the screen regardless of the (unreliable) terminal cell size.
pub const CONTENT_H_INSET: u32 = VIEWPORT_BORDER_WIDTH + VIEWPORT_INNER_PADDING; // 6
/// Vertical room (scroll content top/bottom padding + status bar safety) that
/// graphics must stay clear of, in rows.
pub const CONTENT_V_INSET: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_by_name_matches_case_insensitively_and_rejects_unknown() {
        assert!(set_by_name("Dracula"));
        assert_eq!(index(), 1);
        assert!(set_by_name("TOKYO-NIGHT"));
        assert_eq!(index(), 0);
        assert!(!set_by_name("nope"));
        assert_eq!(index(), 0);
    }

    #[test]
    fn cycle_wraps_around() {
        set_by_name("solarized-light");
        cycle();
        assert_eq!(current().name, "tokyo-night");
    }
}
