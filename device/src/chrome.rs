//! Screen chrome: the left toolbar, the status strip and sidebar hit-testing.
//! Pure pixel math on `Fb` (no qtfb), so it can be host-rendered and tested.

use crate::render::{Fb, BLACK, GRAY, H, W, WHITE};

// Left vertical toolbar, reMarkable-style: pen, eraser, undo, redo, clear, catalogue.
pub const SIDEBAR_W: i32 = 110;
pub const STATUS_H: i32 = 64;
pub const ICON_X: i32 = 13;
pub const ICON_W: i32 = 84;
pub const ICON_H: i32 = 80;
pub const ICON_GAP: i32 = 12;
pub const SLOTS: i32 = 6;

pub fn icon_y(slot: i32) -> i32 {
    15 + slot * (ICON_H + ICON_GAP)
}

/// Which toolbar slot a sidebar tap lands in (None = gap/below toolbar).
pub fn icon_slot(y: i32) -> Option<i32> {
    let rel = y - 15;
    if rel < 0 {
        return None;
    }
    let slot = rel / (ICON_H + ICON_GAP);
    if slot < SLOTS && rel % (ICON_H + ICON_GAP) < ICON_H {
        Some(slot)
    } else {
        None
    }
}

/// Icon box; active tools invert (black fill, white glyph). Returns glyph color.
fn icon_frame(fb: &mut Fb, slot: i32, active: bool) -> u16 {
    let y = icon_y(slot);
    if active {
        fb.fill_rect(ICON_X, y, ICON_W, ICON_H, BLACK);
        WHITE
    } else {
        fb.rect_outline(ICON_X, y, ICON_W, ICON_H, 3, BLACK);
        BLACK
    }
}

/// Curved undo/redo arrow: three-quarter arc + arrowhead. `flip` mirrors it.
fn draw_arrow_icon(fb: &mut Fb, slot: i32, flip: bool) {
    let fg = icon_frame(fb, slot, false);
    let y = icon_y(slot);
    let (cx, cy, r) = (ICON_X + ICON_W / 2, y + ICON_H / 2 + 4, 18);
    let (a0, a1) = if flip {
        (-0.75 * std::f64::consts::PI, 0.9 * std::f64::consts::PI)
    } else {
        (-0.25 * std::f64::consts::PI, -1.9 * std::f64::consts::PI)
    };
    fb.arc(cx, cy, r, a0, a1, 3, fg);
    let tip = a1;
    let (tx, ty) = (cx + (tip.cos() * r as f64) as i32, cy + (tip.sin() * r as f64) as i32);
    let side = if flip { -1.0 } else { 1.0 };
    let tangent = tip + side * std::f64::consts::FRAC_PI_2;
    for spread in [-0.5, 0.5] {
        let a = tangent + spread;
        fb.line(tx, ty, tx + (a.cos() * 12.0) as i32, ty + (a.sin() * 12.0) as i32, 3, fg);
    }
}

pub fn draw_sidebar(fb: &mut Fb, erase_mode: bool) {
    fb.fill_rect(0, 0, SIDEBAR_W, H, WHITE);
    fb.fill_rect(SIDEBAR_W - 2, 0, 2, H, GRAY);

    // Slot 0: pen - nib pointing down-left.
    let fg = icon_frame(fb, 0, !erase_mode);
    let y = icon_y(0);
    fb.line(ICON_X + 30, y + 52, ICON_X + 56, y + 26, 6, fg);
    fb.line(ICON_X + 30, y + 52, ICON_X + 22, y + 60, 2, fg);

    // Slot 1: eraser - tilted block with a wipe line under it.
    let fg = icon_frame(fb, 1, erase_mode);
    let y = icon_y(1);
    fb.line(ICON_X + 26, y + 44, ICON_X + 44, y + 26, 14, fg);
    fb.line(ICON_X + 22, y + 60, ICON_X + 60, y + 60, 2, fg);

    // Slots 2-3: undo / redo arrows.
    draw_arrow_icon(fb, 2, false);
    draw_arrow_icon(fb, 3, true);

    // Slot 4: clear - X.
    let fg = icon_frame(fb, 4, false);
    let y = icon_y(4);
    fb.line(ICON_X + 26, y + 24, ICON_X + 58, y + 56, 3, fg);
    fb.line(ICON_X + 58, y + 24, ICON_X + 26, y + 56, 3, fg);

    // Slot 5: Spell Catalogue - a page of listed spells.
    let fg = icon_frame(fb, 5, false);
    let y = icon_y(5);
    fb.rect_outline(ICON_X + 24, y + 16, 36, 48, 3, fg);
    for row in 0..3 {
        fb.fill_rect(ICON_X + 31, y + 28 + row * 11, 22, 3, fg);
    }
}

/// Draws the two status lines in the strip right of the sidebar. Returns the
/// dirty rect (x, y, w, h) for the caller to push to the screen.
pub fn draw_status(fb: &mut Fb, line1: &str, line2: &str) -> (i32, i32, i32, i32) {
    let left = SIDEBAR_W + 12;
    let width = W - left - 12;
    fb.fill_rect(left, 0, width, STATUS_H - 4, WHITE);
    fb.text(left + 8, 10, line1, 3, BLACK);
    fb.text(left + 8, 42, line2, 2, GRAY);
    (left, 0, width, STATUS_H)
}

/// Sidebar + the status strip's divider: the static chrome of a fresh page.
pub fn draw_chrome(fb: &mut Fb, erase_mode: bool) {
    draw_sidebar(fb, erase_mode);
    fb.fill_rect(SIDEBAR_W, STATUS_H - 2, W - SIDEBAR_W, 2, GRAY);
}

/// Height of the toolbar region to refresh after a tool change.
pub fn toolbar_h() -> i32 {
    icon_y(SLOTS - 1) + ICON_H + 12
}
