//! Screen chrome: the left toolbar, the status strip and sidebar hit-testing.
//! Pure pixel math on `Fb` (no qtfb), so it can be host-rendered and tested.

use recognizer::spell_builder::SpellIr;

use crate::render::{Fb, BLACK, H, W, WHITE};
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

/// UNDO and REDO: one-shot actions, drawn without a box so they read
/// differently from the modal tools and modes. Same 132 px hit box.
pub const ONE_SHOT: [i32; 2] = [2, 3];

/// Extra space between the toolbar's groups.
pub const GROUP_GAP: i32 = 56;
/// Slot 4: destructive, so it sits alone at the bottom edge.
pub const CLEAR_SLOT: i32 = 4;

/// Top of each slot's box. Grouped by function (xochitl / GoodNotes style):
/// PEN ERASE | UNDO REDO | SPELLS from the top, and CLEAR alone at the
/// bottom edge, far from UNDO so a slip can't reach it.
pub fn icon_y(slot: i32) -> i32 {
    let step = ICON_H + ICON_GAP;
    match slot {
        0 | 1 => ICON_TOP + slot * step,
        2 | 3 => ICON_TOP + slot * step + GROUP_GAP,
        5 => ICON_TOP + 4 * step + 2 * GROUP_GAP,
        CLEAR_SLOT => H - ICON_TOP - ICON_H,
        _ => panic!("no toolbar slot {slot}"),
    }
}

/// Which toolbar slot a tap at (x, y) lands in (None = gap, margin or empty
/// sidebar). The hit box is exactly the drawn icon box.
pub fn slot_at(x: i32, y: i32) -> Option<i32> {
    if !(ICON_X..ICON_X + ICON_W).contains(&x) {
        return None;
    }
    (0..SLOTS).find(|&s| (icon_y(s)..icon_y(s) + ICON_H).contains(&y))
}

/// Icon box + caption; active tools invert (black fill, white glyph). Returns
/// the glyph color. Glyphs are centred on (ICON_X + ICON_W / 2, icon_y + 54).
fn icon_frame(fb: &mut Fb, slot: i32, active: bool) -> u16 {
    let y = icon_y(slot);
    let fg = if active {
        fb.fill_rect(ICON_X, y, ICON_W, ICON_H, BLACK);
        WHITE
    } else if ONE_SHOT.contains(&slot) {
        BLACK // one-shot actions: open glyph + label, no box, never look "on"
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
    fb.fill_rect(SIDEBAR_W - 2, 0, 2, H, BLACK); // pure black: e-ink dithers gray

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

    // Group dividers: short bold rules in the gaps between groups.
    for y in [icon_y(2) - (ICON_GAP + GROUP_GAP) / 2, icon_y(5) - (ICON_GAP + GROUP_GAP) / 2, icon_y(CLEAR_SLOT) - (ICON_GAP + GROUP_GAP) / 2] {
        fb.fill_rect(ICON_X + 24, y - 2, ICON_W - 48, 4, BLACK);
    }

    // Slot 4: clear - X, in a double frame (destructive).
    let fg = icon_frame(fb, CLEAR_SLOT, false);
    let y = icon_y(CLEAR_SLOT);
    fb.rect_outline(ICON_X + 9, y + 9, ICON_W - 18, ICON_H - 18, 2, BLACK);
    let (cx, cy) = glyph_center(CLEAR_SLOT);
    fb.line(cx - 26, cy - 26, cx + 26, cy + 26, 6, fg);
    fb.line(cx + 26, cy - 26, cx - 26, cy + 26, 6, fg);

    draw_spells_button(fb, false);
}

/// Slot 5, SPELLS: a page of listed spells. Inverted (solid black, white
/// glyph) while the catalogue panel is open, like an active tool, so the
/// open state reads at a glance. Returns its box for a partial refresh.
pub fn draw_spells_button(fb: &mut Fb, open: bool) -> (i32, i32, i32, i32) {
    fb.fill_rect(ICON_X, icon_y(5), ICON_W, ICON_H, WHITE);
    let fg = icon_frame(fb, 5, open);
    let (cx, cy) = glyph_center(5);
    fb.rect_outline(cx - 26, cy - 32, 52, 66, 4, fg);
    for row in 0..3 {
        fb.fill_rect(cx - 16, cy - 16 + row * 16, 32, 4, fg);
    }
    (ICON_X, icon_y(5), ICON_W, ICON_H)
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
    fb.fill_rect(SIDEBAR_W, STATUS_H - 2, W - SIDEBAR_W, 2, BLACK);
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
        assert_eq!(slot_at(center(0).0, icon_y(5) + ICON_H + 10), None, "below SPELLS");
        assert_eq!(slot_at(center(0).0, icon_y(CLEAR_SLOT) - 10), None, "above CLEAR");
        assert_eq!(slot_at(SIDEBAR_W + 50, center(0).1), None, "canvas side");
    }

    #[test]
    fn spells_button_inverts_while_the_panel_is_open() {
        let mut px = vec![WHITE; (W * H) as usize];
        let ink = |px: &[u16]| (icon_y(5)..icon_y(5) + ICON_H).flat_map(|y| (ICON_X..ICON_X + ICON_W).map(move |x| (y * W + x) as usize)).filter(|&i| px[i] == BLACK).count();
        let boxed = draw_spells_button(&mut Fb { px: &mut px }, true);
        let open = ink(&px);
        assert_eq!(boxed, (ICON_X, icon_y(5), ICON_W, ICON_H));
        draw_spells_button(&mut Fb { px: &mut px }, false);
        let closed = ink(&px);
        let area = (ICON_W * ICON_H) as usize;
        assert!(open > area / 2 && closed < area / 3, "open {open} vs closed {closed} of {area}");
    }

    #[test]
    fn one_shot_actions_have_no_box_but_keep_their_hit_box() {
        let mut px = vec![WHITE; (W * H) as usize];
        draw_sidebar(&mut Fb { px: &mut px }, false);
        // The box's top-left corner: drawn for modal tools/modes, empty for UNDO/REDO.
        let corner = |slot: i32| px[((icon_y(slot) + 1) * W + ICON_X + 1) as usize] == BLACK;
        assert!(corner(0) && corner(1) && corner(5) && corner(CLEAR_SLOT), "boxed: PEN ERASE SPELLS CLEAR");
        assert!(!corner(2) && !corner(3), "UNDO/REDO unboxed");
        for slot in ONE_SHOT {
            assert_eq!(slot_at(ICON_X + 2, icon_y(slot) + 2), Some(slot), "corner still hits slot {slot}");
        }
    }

    #[test]
    fn groups_are_separated_and_clear_is_isolated() {
        let bottom = |s: i32| icon_y(s) + ICON_H;
        // Within a group: the normal gap. Between groups: a wider one.
        assert_eq!(icon_y(1) - bottom(0), ICON_GAP);
        assert_eq!(icon_y(3) - bottom(2), ICON_GAP);
        assert!(icon_y(2) - bottom(1) >= ICON_GAP + GROUP_GAP);
        assert!(icon_y(5) - bottom(3) >= ICON_GAP + GROUP_GAP);
        // CLEAR: on screen, alone at the bottom, far from UNDO/REDO.
        assert_eq!(bottom(CLEAR_SLOT), H - ICON_TOP);
        assert!(icon_y(CLEAR_SLOT) - bottom(3) >= 600, "a slip from REDO can't reach CLEAR");
        // No two boxes overlap.
        for a in 0..SLOTS {
            for b in a + 1..SLOTS {
                assert!(bottom(a) <= icon_y(b) || bottom(b) <= icon_y(a), "slots {a} and {b} overlap");
            }
        }
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
        // Free space between SPELLS and the bottom CLEAR holds two more boxes
        // plus a group gap (the stacked layout's icon_y(SLOTS + 1) probe).
        let free = icon_y(CLEAR_SLOT) - (icon_y(5) + ICON_H);
        assert!(free >= 2 * (ICON_H + ICON_GAP) + GROUP_GAP, "free {free}");
        assert!(ICON_X + ICON_W + ICON_X <= SIDEBAR_W - 2, "icons sit inside the sidebar, clear of the divider");
        assert_eq!(CANVAS_LEFT, SIDEBAR_W + 2);
    }
}
