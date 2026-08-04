use iocraft::prelude::Color;

pub const BG: Color = Color::Rgb {
    r: 26,
    g: 27,
    b: 38,
}; // #1a1b26 (Tokyo Night background)
pub const FG: Color = Color::Rgb {
    r: 169,
    g: 177,
    b: 214,
}; // #a9b1d6 (Tokyo Night foreground)
pub const DARK_BG: Color = Color::Rgb {
    r: 22,
    g: 22,
    b: 30,
}; // #16161e (Tokyo Night sidebar/darker bg)
pub const STATUS_BG: Color = Color::Rgb {
    r: 31,
    g: 35,
    b: 53,
}; // #1f2335 (Tokyo Night statusline bg)
pub const BORDER: Color = Color::Rgb {
    r: 59,
    g: 66,
    b: 97,
}; // #3b4261 (Tokyo Night border)

pub const RED: Color = Color::Rgb {
    r: 247,
    g: 118,
    b: 142,
}; // #f7768e
pub const ORANGE: Color = Color::Rgb {
    r: 255,
    g: 158,
    b: 100,
}; // #ff9e64
pub const YELLOW: Color = Color::Rgb {
    r: 224,
    g: 175,
    b: 104,
}; // #e0af68
pub const GREEN: Color = Color::Rgb {
    r: 158,
    g: 206,
    b: 106,
}; // #9ece6a
pub const TEAL: Color = Color::Rgb {
    r: 115,
    g: 218,
    b: 202,
}; // #73daca
pub const CYAN: Color = Color::Rgb {
    r: 125,
    g: 207,
    b: 255,
}; // #7dcfff
pub const BLUE: Color = Color::Rgb {
    r: 122,
    g: 162,
    b: 247,
}; // #7aa2f7
pub const MAGENTA: Color = Color::Rgb {
    r: 187,
    g: 154,
    b: 243,
}; // #bb9af3
pub const COMMENT: Color = Color::Rgb {
    r: 86,
    g: 95,
    b: 137,
}; // #565f89
pub const DARK_GREY: Color = Color::Rgb {
    r: 65,
    g: 72,
    b: 104,
}; // #414868

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

// ── Debug overlay colors ──────────────────────────────────────────────────────
pub const DBG_HEADING: Color = BLUE;
pub const DBG_PARAGRAPH: Color = FG;
pub const DBG_CODE: Color = GREEN;
pub const DBG_IMAGE: Color = ORANGE;
pub const DBG_MATH: Color = MAGENTA;
pub const DBG_MERMAID: Color = CYAN;
pub const DBG_QUOTE: Color = YELLOW;
pub const DBG_TABLE: Color = TEAL;
pub const DBG_LIST: Color = GREEN;
pub const DBG_BREAK: Color = COMMENT;
pub const DBG_HTML: Color = RED;
pub const DBG_BG: Color = Color::Rgb {
    r: 35,
    g: 28,
    b: 18,
}; // warm dark overlay bg
