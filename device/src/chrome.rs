//! Screen chrome: the left toolbar, the status strip and sidebar hit-testing.
//! Pure pixel math on `Fb` (no qtfb), so it can be host-rendered and tested.

use recognizer::spell_builder::SpellIr;

use crate::render::{Fb, BLACK, GRAY, H, W, WHITE};
use crate::spell_view::status_lines;

// Left vertical toolbar, reMarkable-style: pen, eraser, undo, redo, clear, catalogue.
// Every icon box is also its hit box and is at least MIN_TAP px square: a finger
// on e-ink has no hover feedback, so targets are generous and gaps are real gaps.
pub const MIN_TAP: i32 = 130;
pub const SIDEBAR_W: i32 = 154;
/// First drawable column: right of the sidebar and its 2px divider.
pub const CANVAS_LEFT: i32 = SIDEBAR_W + 2;
pub const STATUS_H: i32 = 64;
pub const ICON_X: i32 = 10;
pub const ICON_W: i32 = 132;
pub const ICON_H: i32 = 132;
pub const ICON_GAP: i32 = 12;
pub const ICON_TOP: i32 = 15;
pub const SLOTS: i32 = 6;

const LABELS: [&str; SLOTS as usize] = ["PEN", "ERASE", "UNDO", "REDO", "CLEAR", "SPELLS"];

pub fn icon_y(slot: i32) -> i32 {
    ICON_TOP + slot * (ICON_H + ICON_GAP)
}

/// Which toolbar slot a tap at (x, y) lands in (None = gap, margin or below
/// the toolbar). The hit box is exactly the drawn icon box.
pub fn slot_at(x: i32, y: i32) -> Option<i32> {
    let rel = y - ICON_TOP;
    if rel < 0 || !(ICON_X..ICON_X + ICON_W).contains(&x) {
        return None;
    }
    let slot = rel / (ICON_H + ICON_GAP);
    (slot < SLOTS && rel % (ICON_H + ICON_GAP) < ICON_H).then_some(slot)
}

/// Icon box + caption; active tools invert (black fill, white glyph). Returns
/// the glyph color. Glyphs are centred on (ICON_X + ICON_W / 2, icon_y + 54).
fn icon_frame(fb: &mut Fb, slot: i32, active: bool) -> u16 {
    let y = icon_y(slot);
    let fg = if active {
        fb.fill_rect(ICON_X, y, ICON_W, ICON_H, BLACK);
        WHITE
    } else {
        fb.rect_outline(ICON_X, y, ICON_W, ICON_H, 4, BLACK);
        BLACK
    };
    let label = LABELS[slot as usize];
    let w = label.len() as i32 * 16;
    fb.text(ICON_X + (ICON_W - w) / 2, y + ICON_H - 30, label, 2, fg);
    fg
}

fn glyph_center(slot: i32) -> (i32, i32) {
    (ICON_X + ICON_W / 2, icon_y(slot) + 54)
}

/// Curved undo/redo arrow: three-quarter arc + arrowhead. `flip` mirrors it.
fn draw_arrow_icon(fb: &mut Fb, slot: i32, flip: bool) {
    let fg = icon_frame(fb, slot, false);
    let (cx, cy) = glyph_center(slot);
    let r = 28;
    let (a0, a1) = if flip {
        (-0.75 * std::f64::consts::PI, 0.9 * std::f64::consts::PI)
    } else {
        (-0.25 * std::f64::consts::PI, -1.9 * std::f64::consts::PI)
    };
    fb.arc(cx, cy, r, a0, a1, 5, fg);
    let tip = a1;
    let (tx, ty) = (cx + (tip.cos() * r as f64) as i32, cy + (tip.sin() * r as f64) as i32);
    let side = if flip { -1.0 } else { 1.0 };
    let tangent = tip + side * std::f64::consts::FRAC_PI_2;
    for spread in [-0.5, 0.5] {
        let a = tangent + spread;
        fb.line(tx, ty, tx + (a.cos() * 20.0) as i32, ty + (a.sin() * 20.0) as i32, 5, fg);
    }
}

pub fn draw_sidebar(fb: &mut Fb, erase_mode: bool) {
    fb.fill_rect(0, 0, SIDEBAR_W, H, WHITE);
    fb.fill_rect(SIDEBAR_W - 2, 0, 2, H, GRAY);

    // Slot 0: pen - nib pointing down-left.
    let fg = icon_frame(fb, 0, !erase_mode);
    let (cx, cy) = glyph_center(0);
    fb.line(cx - 12, cy + 12, cx + 28, cy - 28, 12, fg);
    fb.line(cx - 12, cy + 12, cx - 28, cy + 30, 4, fg);

    // Slot 1: eraser - tilted block with a wipe line under it.
    let fg = icon_frame(fb, 1, erase_mode);
    let (cx, cy) = glyph_center(1);
    fb.line(cx - 18, cy + 8, cx + 14, cy - 22, 24, fg);
    fb.line(cx - 34, cy + 30, cx + 34, cy + 30, 4, fg);

    // Slots 2-3: undo / redo arrows.
    draw_arrow_icon(fb, 2, false);
    draw_arrow_icon(fb, 3, true);

    // Slot 4: clear - X.
    let fg = icon_frame(fb, 4, false);
    let (cx, cy) = glyph_center(4);
    fb.line(cx - 26, cy - 26, cx + 26, cy + 26, 6, fg);
    fb.line(cx + 26, cy - 26, cx - 26, cy + 26, 6, fg);

    // Slot 5: Spell Catalogue - a page of listed spells.
    let fg = icon_frame(fb, 5, false);
    let (cx, cy) = glyph_center(5);
    fb.rect_outline(cx - 26, cy - 32, 52, 66, 4, fg);
    for row in 0..3 {
        fb.fill_rect(cx - 16, cy - 16 + row * 16, 32, 4, fg);
    }
}

/// Fits `text` into `max_chars`: whole if it fits, else cut back to a word
/// boundary (when one is near) and ended with "...".
pub fn fit(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let mut cut = &chars[..max_chars.saturating_sub(3)];
    if let Some(i) = cut.iter().rposition(|&c| c == ' ') {
        if cut.len() - i <= 14 {
            cut = &cut[..i];
        }
    }
    let kept: String = cut.iter().collect();
    format!("{}...", kept.trim_end_matches([' ', '|']))
}

/// Spell line: state, element, quality/stability — or just the status when the
/// drawing isn't a valid spell yet ("No ring detected", "Invalid spell"...).
pub fn headline(status: &str, valid: bool, element: Option<&str>, quality: f64, stability: f64) -> String {
    match element {
        Some(e) if valid => {
            let state = status.strip_suffix(" spell").unwrap_or(status);
            let mut name = e.to_string();
            name[..1].make_ascii_uppercase();
            format!("{state}: {name}  quality {:.0}%  stability {:.0}%", quality * 100.0, stability * 100.0)
        }
        _ => status.to_string(),
    }
}

/// The two status-strip lines for a recognized drawing.
pub fn spell_status(ir: &SpellIr) -> (String, String) {
    let (_, detail) = status_lines(ir);
    (headline(&ir.status, ir.valid, ir.element.as_deref(), ir.quality, ir.stability), detail)
}

// Status strip text box: right of the sidebar, 8px inside the strip's padding.
const STATUS_LEFT: i32 = SIDEBAR_W + 12;
const STATUS_WIDTH: i32 = W - STATUS_LEFT - 12;
const SPELL_SCALE: i32 = 3;
const DETAIL_SCALE: i32 = 2;
pub const SPELL_LINE_CHARS: usize = ((STATUS_WIDTH - 16) / (8 * SPELL_SCALE)) as usize;
pub const DETAIL_LINE_CHARS: usize = ((STATUS_WIDTH - 16) / (8 * DETAIL_SCALE)) as usize;

/// Draws the two status lines (each truncated to fit the strip) right of the
/// sidebar. Returns the dirty rect (x, y, w, h) for the caller to push.
pub fn draw_status(fb: &mut Fb, line1: &str, line2: &str) -> (i32, i32, i32, i32) {
    fb.fill_rect(STATUS_LEFT, 0, STATUS_WIDTH, STATUS_H - 4, WHITE);
    fb.text(STATUS_LEFT + 8, 8, &fit(line1, SPELL_LINE_CHARS), SPELL_SCALE, BLACK);
    fb.text(STATUS_LEFT + 8, 38, &fit(line2, DETAIL_LINE_CHARS), DETAIL_SCALE, BLACK);
    (STATUS_LEFT, 0, STATUS_WIDTH, STATUS_H)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn center(slot: i32) -> (i32, i32) {
        (ICON_X + ICON_W / 2, icon_y(slot) + ICON_H / 2)
    }

    #[test]
    fn every_slot_center_maps_to_its_slot() {
        for slot in 0..SLOTS {
            let (x, y) = center(slot);
            assert_eq!(slot_at(x, y), Some(slot));
        }
    }

    #[test]
    fn every_hit_box_is_at_least_min_tap() {
        for slot in 0..SLOTS {
            // Measure the hit box by probing, not by trusting the consts.
            let y0 = icon_y(slot);
            let ys: Vec<i32> = (0..H).filter(|&y| slot_at(ICON_X + 5, y) == Some(slot)).collect();
            let xs: Vec<i32> = (0..W).filter(|&x| slot_at(x, y0 + 5) == Some(slot)).collect();
            assert_eq!((ys[0], *ys.last().unwrap()), (y0, y0 + ICON_H - 1), "slot {slot}");
            assert!(ys.len() as i32 >= MIN_TAP && xs.len() as i32 >= MIN_TAP, "slot {slot}: {}x{}", xs.len(), ys.len());
        }
    }

    #[test]
    fn gaps_margins_and_below_the_toolbar_map_to_none() {
        for slot in 0..SLOTS {
            let gap_y = icon_y(slot) + ICON_H + ICON_GAP / 2;
            assert_eq!(slot_at(center(slot).0, gap_y), None, "gap after slot {slot}");
            assert_eq!(slot_at(ICON_X - 1, icon_y(slot) + 10), None, "left margin");
            assert_eq!(slot_at(ICON_X + ICON_W, icon_y(slot) + 10), None, "right margin");
        }
        assert_eq!(slot_at(center(0).0, ICON_TOP - 1), None, "above the first icon");
        assert_eq!(slot_at(center(0).0, icon_y(SLOTS) + 10), None, "below the last icon");
        assert_eq!(slot_at(SIDEBAR_W + 50, center(0).1), None, "canvas side");
    }

    #[test]
    fn fit_keeps_short_text_and_cuts_long_text_cleanly() {
        assert_eq!(fit("Active: Fire", 20), "Active: Fire");
        let long = "column 100%  force 80 spread 50 range 60 grav 20  |  ring not closed, the sigil is too close to the edge";
        let out = fit(long, 40);
        assert!(out.chars().count() <= 40 && out.ends_with("..."), "{out}");
        assert!(!out.contains(" ..."), "cut back to a word boundary: {out}");
        assert_eq!(fit("x".repeat(100).as_str(), 10).chars().count(), 10);
    }

    #[test]
    fn headline_names_state_element_quality_and_stability() {
        let h = headline("Prepared spell", true, Some("water"), 1.0, 1.0);
        assert_eq!(h, "Prepared: Water  quality 100%  stability 100%");
        assert!(h.chars().count() <= SPELL_LINE_CHARS, "worst case fits unscaled: {h}");
        assert_eq!(headline("Active spell", true, Some("fire"), 0.8, 0.9), "Active: Fire  quality 80%  stability 90%");
        assert_eq!(headline("No ring detected", false, None, 0.0, 0.0), "No ring detected");
        assert_eq!(headline("Invalid spell", false, Some("fire"), 0.5, 0.5), "Invalid spell");
    }

    #[test]
    fn status_text_stays_inside_the_strip() {
        let mut px = vec![WHITE; (W * H) as usize];
        let mut fb = Fb { px: &mut px };
        let long = "m".repeat(200);
        let (x, y, w, h) = draw_status(&mut fb, &long, &long);
        assert!(x >= CANVAS_LEFT && x + w <= W && y == 0 && h == STATUS_H);
        for yy in 0..H {
            for xx in 0..W {
                if fb.px[(yy * W + xx) as usize] != WHITE {
                    assert!((x..x + w).contains(&xx) && yy < STATUS_H - 4, "ink outside the strip at {xx},{yy}");
                }
            }
        }
    }

    #[test]
    fn toolbar_fits_with_room_for_two_more_slots() {
        assert!(icon_y(SLOTS + 1) + ICON_H <= H);
        assert!(ICON_X + ICON_W + ICON_X <= SIDEBAR_W - 2, "icons sit inside the sidebar, clear of the divider");
        assert_eq!(CANVAS_LEFT, SIDEBAR_W + 2);
    }
}
