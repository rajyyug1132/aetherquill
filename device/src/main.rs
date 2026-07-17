//! Aetherquill — WHA spell simulator for the reMarkable 2.
//!
//! Runs WINDOWED under AppLoad (xovi): AppLoad sets QTFB_KEY and hands us a
//! qtfb framebuffer window inside a living xochitl — no takeover, no rm2fb,
//! no stopped UI. Display + input both flow through the qtfb socket; the
//! recognition pipeline is the `recognizer` crate, in-process. Pure Rust +
//! libc: builds as a static armv7 musl binary from any host.
//!
//! Draw a ring, draw a sigil inside, close the ring — the spell activates.
//! Tap UNDO / CLEAR boxes in the top chrome strip. Close the window from
//! AppLoad to exit (server disconnect ends the event loop).

use std::time::Instant;

use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
use recognizer::dictionaries::load_dictionary;
use recognizer::symbol_recognizer::Dictionary;
use recognizer::drawing_classifier::classify_drawing;
use recognizer::geometry::Point;
use recognizer::ring_detector::Ring;
use recognizer::spell_builder::compile_spell;
use recognizer::stroke_cleaner::RawStroke;

mod grimoire;
mod qtfb;
mod render;
mod shapes;

use render::{effect_frame, effect_settle, Fb, BLACK, EFFECT_FRAMES, GRAY, H, W, WHITE};
use shapes::{adjust_shape, path_length, point_in_poly, snap_stroke_kind, straighten, SnapKind};

use qtfb::{QtfbClient, RM2_HEIGHT, RM2_WIDTH};

const _: () = assert!(W == RM2_WIDTH as i32 && H == RM2_HEIGHT as i32);

// ponytail: screen px -> web-canvas px so the vendored CONFIG's pixel-tuned
// thresholds (ring.minRadius 70 etc.) hold unchanged on a 702x936 canvas.
const CANVAS_SCALE: f64 = 0.5;
const MIN_POINT_DIST: f64 = 1.4 / CANVAS_SCALE;
const MIN_STROKE_LEN: f64 = 7.0 / CANVAS_SCALE;

const INK_W: i32 = 3;
const ERASE_SIZES: [f64; 3] = [14.0, 28.0, 56.0];
// ponytail: single-threaded app; (size_idx, select_mode) for the eraser menu
// lives in a static so redraw_all call sites stay unchanged.
static mut ERASE_UI: (usize, bool) = (1, false);

struct ScreenStroke {
    id: String,
    points: Vec<(f64, f64)>,
}

fn to_raw_strokes(strokes: &[ScreenStroke]) -> Vec<RawStroke> {
    strokes
        .iter()
        .map(|s| RawStroke {
            id: s.id.clone(),
            points: s.points.iter().map(|&(x, y)| Point { x: x * CANVAS_SCALE, y: y * CANVAS_SCALE }).collect(),
        })
        .collect()
}

// Left vertical toolbar, reMarkable-style: pen, eraser, undo, redo, clear.
const SIDEBAR_W: i32 = 110;
const STATUS_H: i32 = 64;
const ICON_X: i32 = 13;
const ICON_W: i32 = 84;
const ICON_H: i32 = 80;
const ICON_GAP: i32 = 12;

fn icon_y(slot: i32) -> i32 {
    15 + slot * (ICON_H + ICON_GAP)
}

/// Which toolbar slot a sidebar tap lands in (None = gap/below toolbar).
fn icon_slot(y: i32) -> Option<i32> {
    let rel = y - 15;
    if rel < 0 {
        return None;
    }
    let slot = rel / (ICON_H + ICON_GAP);
    if slot <= 4 && rel % (ICON_H + ICON_GAP) < ICON_H {
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

fn draw_sidebar(fb: &mut Fb, erase_mode: bool) {
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
}

const EMENU_X: i32 = SIDEBAR_W + 6;
const EMENU_W: i32 = 64;
const EMENU_H: i32 = 56;

fn emenu_y(opt: i32) -> i32 {
    icon_y(1) + opt * (EMENU_H + 8)
}

/// Eraser options flyout: three scrub sizes + selection-loop mode.
fn draw_eraser_menu(fb: &mut Fb, size_idx: usize, select_mode: bool) {
    for (opt, label) in ["S", "M", "L", "SEL"].iter().enumerate() {
        let y = emenu_y(opt as i32);
        let active = if opt == 3 { select_mode } else { !select_mode && opt == size_idx };
        let fg = if active {
            fb.fill_rect(EMENU_X, y, EMENU_W, EMENU_H, BLACK);
            WHITE
        } else {
            fb.fill_rect(EMENU_X, y, EMENU_W, EMENU_H, WHITE);
            fb.rect_outline(EMENU_X, y, EMENU_W, EMENU_H, 2, BLACK);
            BLACK
        };
        let scale = if opt == 3 { 2 } else { 3 };
        fb.text(EMENU_X + (EMENU_W - label.len() as i32 * 8 * scale) / 2, y + 16, label, scale, fg);
    }
}

fn draw_status(fb: &mut Fb, client: &QtfbClient, line1: &str, line2: &str) {
    let left = SIDEBAR_W + 12;
    let width = W - left - 12;
    fb.fill_rect(left, 0, width, STATUS_H - 4, WHITE);
    fb.text(left + 8, 10, line1, 3, BLACK);
    fb.text(left + 8, 42, line2, 2, GRAY);
    let _ = client.update_partial(left, 0, width, STATUS_H);
}

/// Scrub-erase: remove the parts of any stroke within `r` of (x,y), splitting
/// survivors into separate strokes (official "regular eraser" semantics).
/// The original stroke goes to `redo` so the erase is undoable.
fn scrub_erase(
    strokes: &mut Vec<ScreenStroke>,
    redo: &mut Vec<ScreenStroke>,
    next_id: &mut u64,
    x: f64,
    y: f64,
    r: f64,
) -> bool {
    let mut changed = false;
    let mut i = 0;
    while i < strokes.len() {
        if !strokes[i].points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < r) {
            i += 1;
            continue;
        }
        changed = true;
        let original = strokes.remove(i);
        let mut segments: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut run: Vec<(f64, f64)> = Vec::new();
        for &p in &original.points {
            if (p.0 - x).hypot(p.1 - y) >= r {
                run.push(p);
            } else if !run.is_empty() {
                segments.push(std::mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            segments.push(run);
        }
        redo.push(original);
        for seg in segments {
            if seg.len() >= 2 && path_length(&seg) >= 10.0 {
                strokes.insert(i, ScreenStroke { id: format!("s{next_id}"), points: seg });
                *next_id += 1;
                i += 1;
            }
        }
    }
    changed
}

/// Full redraw. When a complete ring is known, its member strokes render as
/// one perfect circle (display-only snap — recognition still sees raw points).
fn redraw_all(fb: &mut Fb, client: &QtfbClient, strokes: &[ScreenStroke], ring: Option<&Ring>, erase_mode: bool) {
    fb.fill_rect(0, 0, W, H, WHITE);
    draw_sidebar(fb, erase_mode);
    fb.fill_rect(SIDEBAR_W, STATUS_H - 2, W - SIDEBAR_W, 2, GRAY);
    if erase_mode {
        draw_eraser_menu(fb, unsafe { ERASE_UI.0 }, unsafe { ERASE_UI.1 });
    }
    let ring_ids: &[String] = ring.filter(|r| r.complete).map_or(&[], |r| &r.stroke_ids);
    for stroke in strokes {
        if ring_ids.contains(&stroke.id) {
            continue;
        }
        for w in stroke.points.windows(2) {
            fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, INK_W, BLACK);
        }
    }
    if let Some(r) = ring.filter(|r| r.complete) {
        fb.circle(
            (r.center.x / CANVAS_SCALE) as i32,
            (r.center.y / CANVAS_SCALE) as i32,
            (r.radius / CANVAS_SCALE) as i32,
            INK_W,
            BLACK,
        );
    }
    let _ = client.update_all();
}

struct SpellOutcome {
    ring: Option<Ring>,
    status: String,
    element: Option<String>,
    valid: bool,
    active: bool,
    quality: f64,
    stability: f64,
    warning: Option<String>,
    signature: String,
}

fn recognize(dictionary: &Dictionary, strokes: &[ScreenStroke], previous_ring: Option<&Ring>) -> SpellOutcome {
    let t0 = Instant::now();
    let raw = to_raw_strokes(strokes);
    let result = classify_drawing(&raw, previous_ring, dictionary, "0.2.0", &INPUT, &RING, &LAYERS, &RECOGNITION);
    let spell = compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE);
    eprintln!(
        "recognize: {} strokes in {}ms -> status={:?} element={:?} valid={} active={} ring_complete={}",
        strokes.len(),
        t0.elapsed().as_millis(),
        spell.status,
        spell.element,
        spell.valid,
        spell.active,
        result.ring.complete,
    );
    SpellOutcome {
        ring: if result.ring.found { Some(result.ring) } else { None },
        status: spell.status,
        element: spell.element,
        valid: spell.valid,
        active: spell.active,
        quality: spell.quality,
        stability: spell.stability,
        warning: spell.warnings.first().map(|w| w.as_str().to_string()),
        signature: spell.signature,
    }
}

/// Spell activation: plays the per-element effect animation (rising flames,
/// ripples, curling wind, rising earth, radiant light — ported from the
/// upstream particle renderer into 1-bit e-ink frames in `render::effect_frame`).
/// Each frame refreshes only the annulus around the seal, so the sigil inside
/// is untouched and the effect genuinely moves.
fn animate_activation(fb: &mut Fb, client: &QtfbClient, cx: i32, cy: i32, radius: i32, element: &str) {
    let pad = radius + render::EFFECT_REACH;
    let flush = |client: &QtfbClient, ms: u64| {
        let _ = client.update_partial((cx - pad).max(0), (cy - pad).max(0), pad * 2, pad * 2);
        std::thread::sleep(std::time::Duration::from_millis(ms));
    };
    for f in 0..EFFECT_FRAMES {
        effect_frame(fb, cx, cy, radius, element, f);
        // E-ink needs ~300ms to physically show a frame; faster and xochitl
        // coalesces the updates into one (observed on device: only the settle
        // frame was visible at 80ms pacing).
        flush(client, 320);
    }
    effect_settle(fb, cx, cy, radius, element);
    flush(client, 200);
}

fn render_feedback(fb: &mut Fb, client: &QtfbClient, outcome: &SpellOutcome, last_activation: &mut String) {
    let line1 = match &outcome.element {
        Some(element) if outcome.valid => format!(
            "{} - {}  q{:.0}% s{:.0}%",
            outcome.status,
            element,
            outcome.quality * 100.0,
            outcome.stability * 100.0
        ),
        _ => outcome.status.clone(),
    };
    draw_status(fb, client, &line1, outcome.warning.as_deref().unwrap_or(""));

    let Some(ring) = &outcome.ring else { return };
    let cx = (ring.center.x / CANVAS_SCALE) as i32;
    let cy = (ring.center.y / CANVAS_SCALE) as i32;
    let radius = (ring.radius / CANVAS_SCALE) as i32;

    fb.circle(cx, cy, radius + 6, 2, GRAY);

    let activated = outcome.active && outcome.signature != *last_activation;
    if activated {
        *last_activation = outcome.signature.clone();
        if let Some(element) = &outcome.element {
            grimoire::log_spell(element, outcome.quality, outcome.stability, &outcome.signature);
            animate_activation(fb, client, cx, cy, radius, element);
        }
    }

    let pad = 90;
    let _ = client.update_partial(
        (cx - radius - pad).max(0),
        (cy - radius - pad).max(0),
        (radius + pad) * 2,
        (radius + pad) * 2,
    );
}

fn main() {
    let key: i32 = std::env::var("QTFB_KEY")
        .ok()
        .and_then(|k| k.parse().ok())
        .unwrap_or(245209899); // QTFB_DEFAULT_FRAMEBUFFER

    let mut client = match QtfbClient::connect(key) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aetherquill: qtfb connect failed ({e}) — launch via AppLoad");
            std::process::exit(1);
        }
    };
    let dictionary = load_dictionary();

    // Owned pixel view for the lifetime of the app (single-threaded).
    let px: &'static mut [u16] = unsafe { std::mem::transmute(client.pixels()) };
    let mut fb = Fb { px };

    fb.fill_rect(0, 0, W, H, WHITE);
    draw_sidebar(&mut fb, false);
    fb.fill_rect(SIDEBAR_W, STATUS_H - 2, W - SIDEBAR_W, 2, GRAY);
    let _ = client.update_all();
    draw_status(&mut fb, &client, "Draw a spell ring to begin", "");

    let mut strokes: Vec<ScreenStroke> = Vec::new();
    let mut redo: Vec<ScreenStroke> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    let mut pen_down = false;
    let mut next_id: u64 = 1;
    let mut previous_ring: Option<Ring> = None;
    let mut last_activation = String::new();
    let mut dirty_since = Instant::now();
    // Coalesced refresh: ink lands in shm per-segment, but e-ink update
    // requests flush at most every 40ms — per-segment update_partial floods
    // the qtfb socket, lags the panel, and drops input events.
    let mut dirty_rect: Option<(i32, i32, i32, i32)> = None; // x0,y0,x1,y1
    let mut last_flush = Instant::now();
    // Hold-to-snap (reMarkable "perfect shapes"): pen held still mid-contact
    // for 600ms snaps the in-progress stroke to a line/circle.
    let mut hold_anchor = (0.0_f64, 0.0_f64);
    let mut hold_since = Instant::now();
    let mut hold_snapped = false;
    // Distinguishes "pen still, events trickling" (hold-to-snap) from "pen
    // gone, no events at all" (silence commit) — both stall dirty_since.
    let mut last_pen_event = Instant::now();
    // Auto ring snap: redraw once per newly completed ring.
    let mut snapped_ring_ids: Vec<String> = Vec::new();
    // ERASE toggle (chrome button) — pen deletes strokes instead of inking.
    let mut erase_mode = false;
    let mut erased_any = false;
    let mut erase_size_idx: usize = 1;
    let mut erase_select = false;
    let mut erase_redraw = Instant::now();
    // Touch-drag stroke moving: (stroke index, touch dev_id, last x, last y).
    let mut moving: Option<(usize, i32, f64, f64)> = None;
    let mut last_touch = Instant::now();
    let mut move_redraw = Instant::now();
    // Live shape adjust (official reMarkable UX): after hold-to-snap, keeping
    // the pen down and dragging resizes/rotates the shape; lifting finalizes.
    // (kind, anchor, base points at snap, grab distance at snap)
    let mut adjusting: Option<(SnapKind, (f64, f64), Vec<(f64, f64)>, f64)> = None;
    let mut adjust_redraw = Instant::now();
    // Touch arrives as a stream of PRESS events per contact — debounce chrome
    // buttons so one tap doesn't fire dozens of times.
    let mut last_chrome_tap = Instant::now() - std::time::Duration::from_secs(1);

    loop {
        // Block until the socket is readable (input or disconnect).
        let mut pfd = libc::pollfd { fd: client.raw_fd(), events: libc::POLLIN, revents: 0 };
        // 20ms timeout keeps the coalesced-refresh flush timely mid-stroke.
        unsafe { libc::poll(&mut pfd, 1, 20) };

        let events = match client.drain_events() {
            Ok(events) => events,
            Err(_) => break, // window closed by AppLoad — exit
        };

        for event in events {
            match event.input_type {
                qtfb::INPUT_PEN_PRESS | qtfb::INPUT_PEN_UPDATE => {
                    let (x, y) = (event.x as f64, event.y as f64);
                    last_pen_event = Instant::now();
                    if x < (SIDEBAR_W + 4) as f64 || y < STATUS_H as f64 {
                        continue;
                    }
                    if erase_mode {
                        if x < (SIDEBAR_W + EMENU_W + 12) as f64 {
                            continue; // don't erase under the eraser menu
                        }
                        if erase_select {
                            // Selection loop: capture, echo as thin gray ink.
                            if let Some(&last) = current.last() {
                                fb.line(last.0 as i32, last.1 as i32, x as i32, y as i32, 1, GRAY);
                                let _ = client.update_partial(
                                    last.0.min(x) as i32 - 2, last.1.min(y) as i32 - 2,
                                    (x - last.0).abs() as i32 + 4, (y - last.1).abs() as i32 + 4,
                                );
                            }
                            current.push((x, y));
                            pen_down = true;
                            continue;
                        }
                        if scrub_erase(&mut strokes, &mut redo, &mut next_id, x, y, ERASE_SIZES[erase_size_idx]) {
                            erased_any = true;
                        }
                        if erased_any && erase_redraw.elapsed().as_millis() > 150 {
                            erase_redraw = Instant::now();
                            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                        }
                        continue;
                    }
                    if (x - hold_anchor.0).hypot(y - hold_anchor.1) > 6.0 {
                        hold_anchor = (x, y);
                        hold_since = Instant::now();
                    }
                    if pen_down {
                        if let Some((kind, anchor, ref base, grab)) = adjusting {
                            current = adjust_shape(kind, anchor, base, grab, x, y);
                            if adjust_redraw.elapsed().as_millis() > 120 {
                                adjust_redraw = Instant::now();
                                redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                                for w in current.windows(2) {
                                    fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, INK_W, BLACK);
                                }
                                let _ = client.update_all();
                            }
                            continue;
                        }
                    } else {
                        pen_down = true;
                        hold_snapped = false;
                        adjusting = None;
                        current = vec![(x, y)];
                        eprintln!("pen down at {x},{y}");
                        continue;
                    }
                    let last = *current.last().unwrap();
                    if (x - last.0).hypot(y - last.1) < MIN_POINT_DIST {
                        continue;
                    }
                    fb.line(last.0 as i32, last.1 as i32, x as i32, y as i32, INK_W, BLACK);
                    let (x0, y0) = (last.0.min(x) as i32 - INK_W, last.1.min(y) as i32 - INK_W);
                    let (x1, y1) = (last.0.max(x) as i32 + INK_W, last.1.max(y) as i32 + INK_W);
                    dirty_rect = Some(match dirty_rect {
                        Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
                        None => (x0, y0, x1, y1),
                    });
                    current.push((x, y));
                    dirty_since = Instant::now();
                }
                qtfb::INPUT_PEN_RELEASE => {
                    eprintln!("pen release, {} pts", current.len());
                    if erase_mode {
                        if erase_select && current.len() >= 8 {
                            let a = std::mem::take(&mut current);
                            pen_down = false;
                            let before = strokes.len();
                            strokes.retain(|s| {
                                let inside = s.points.iter().filter(|p| point_in_poly(p.0, p.1, &a)).count();
                                let keep = (inside as f64) < s.points.len() as f64 * 0.8;
                                if !keep {
                                    // moved into redo below via drain pattern not possible in retain;
                                    // acceptable: selection erase is not undoable per-stroke
                                }
                                keep
                            });
                            erased_any |= strokes.len() != before;
                        }
                        pen_down = false;
                        current.clear();
                        if erased_any {
                            erased_any = false;
                            let outcome = recognize(&dictionary, &strokes, None);
                            previous_ring = outcome.ring.clone();
                            snapped_ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
                            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                            render_feedback(&mut fb, &client, &outcome, &mut last_activation);
                        }
                        continue;
                    }
                    if !pen_down {
                        continue;
                    }
                    pen_down = false;
                    let was_adjusting = adjusting.take().is_some();
                    let points = std::mem::take(&mut current);
                    if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
                        let (points, straightened) = match straighten(&points) {
                            Some(line) => (line, true),
                            None => (points, false),
                        };
                        strokes.push(ScreenStroke { id: format!("s{next_id}"), points });
                        if straightened || was_adjusting {
                            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                        }
                        next_id += 1;
                        redo.clear();
                        let outcome = recognize(&dictionary, &strokes, previous_ring.as_ref());
                        previous_ring = outcome.ring.clone();
                        let ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
                        if ring_ids != snapped_ring_ids {
                            snapped_ring_ids = ring_ids;
                            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                        }
                        render_feedback(&mut fb, &client, &outcome, &mut last_activation);
                    } else if !points.is_empty() {
                        // Too short to keep: erase the stray ink dot.
                        redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                    }
                }
                qtfb::INPUT_TOUCH_PRESS => {
                    if pen_down {
                        continue;
                    }
                    // Eraser menu flyout.
                    if erase_mode
                        && (EMENU_X..EMENU_X + EMENU_W).contains(&event.x)
                        && event.y >= emenu_y(0)
                        && event.y < emenu_y(3) + EMENU_H
                    {
                        if last_chrome_tap.elapsed().as_millis() < 250 {
                            continue;
                        }
                        last_chrome_tap = Instant::now();
                        let opt = (event.y - emenu_y(0)) / (EMENU_H + 8);
                        if opt == 3 {
                            erase_select = true;
                        } else {
                            erase_select = false;
                            erase_size_idx = opt as usize;
                        }
                        unsafe { ERASE_UI = (erase_size_idx, erase_select) };
                        draw_eraser_menu(&mut fb, erase_size_idx, erase_select);
                        let _ = client.update_partial(EMENU_X, emenu_y(0), EMENU_W, 4 * (EMENU_H + 8));
                        continue;
                    }
                    // In the canvas: touch-drag moves the stroke under the finger.
                    if event.x >= SIDEBAR_W && event.y >= STATUS_H {
                        let (x, y) = (event.x as f64, event.y as f64);
                        last_touch = Instant::now();
                        match moving {
                            Some((idx, id, lx, ly)) if id == event.dev_id => {
                                let (dx, dy) = (x - lx, y - ly);
                                for p in &mut strokes[idx].points {
                                    p.0 += dx;
                                    p.1 += dy;
                                }
                                moving = Some((idx, id, x, y));
                                if move_redraw.elapsed().as_millis() > 150 {
                                    move_redraw = Instant::now();
                                    redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                                }
                            }
                            None => {
                                let grab = strokes.iter().position(|s| {
                                    s.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < 40.0)
                                });
                                if let Some(idx) = grab {
                                    moving = Some((idx, event.dev_id, x, y));
                                }
                            }
                            _ => {} // another finger - ignore
                        }
                        continue;
                    }
                    if event.x >= SIDEBAR_W {
                        continue; // status strip - nothing tappable
                    }
                    // Sidebar buttons (debounced - touch streams PRESS events).
                    if last_chrome_tap.elapsed().as_millis() < 250 {
                        continue;
                    }
                    last_chrome_tap = Instant::now();
                    let slot = icon_slot(event.y);
                    eprintln!("sidebar tap y={} -> slot {:?}", event.y, slot);
                    match slot {
                        Some(0) | Some(1) => {
                            erase_mode = slot == Some(1);
                            unsafe { ERASE_UI = (erase_size_idx, erase_select) };
                            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                            continue;
                        }
                        Some(2) => {
                            if let Some(s) = strokes.pop() {
                                redo.push(s);
                            }
                        }
                        Some(3) => {
                            if let Some(s) = redo.pop() {
                                strokes.push(s);
                            }
                        }
                        Some(4) => {
                            redo.extend(strokes.drain(..)); // CLEAR is undoable via redo taps
                        }
                        _ => continue,
                    }
                    let outcome = recognize(&dictionary, &strokes, None);
                    previous_ring = outcome.ring.clone();
                    snapped_ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
                    redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                    render_feedback(&mut fb, &client, &outcome, &mut last_activation);
                }
                _ => {}
            }
        }

        // Flush coalesced ink refreshes at ~25Hz.
        if let Some((x0, y0, x1, y1)) = dirty_rect {
            if last_flush.elapsed().as_millis() >= 40 || !pen_down {
                let _ = client.update_partial(x0.max(0), y0.max(0), x1 - x0, y1 - y0);
                dirty_rect = None;
                last_flush = Instant::now();
            }
        }

        // Finger lifted mid-drag: commit the move, re-recognize.
        if moving.is_some() && last_touch.elapsed().as_millis() > 400 {
            moving = None;
            let outcome = recognize(&dictionary, &strokes, previous_ring.as_ref());
            previous_ring = outcome.ring.clone();
            snapped_ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
            render_feedback(&mut fb, &client, &outcome, &mut last_activation);
        }

        // Hold-to-snap: pen still on the panel (events keep arriving inside
        // the 6px anchor) for 600ms — snap the in-progress stroke.
        if pen_down && !hold_snapped && hold_since.elapsed().as_millis() > 600 {
            hold_snapped = true;
            if let Some((snapped, kind)) = snap_stroke_kind(&current) {
                current = snapped;
                redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                for w in current.windows(2) {
                    fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, INK_W, BLACK);
                }
                let _ = client.update_all();
                dirty_rect = None;
                // Enter live adjust: drag without lifting to resize/rotate.
                let anchor = match kind {
                    SnapKind::Line => current[0],
                    SnapKind::Circle { cx, cy, .. } => (cx, cy),
                    SnapKind::Poly => {
                        let n = current.len() as f64;
                        let (ax, ay) = current.iter().fold((0.0, 0.0), |(ax, ay), p| (ax + p.0, ay + p.1));
                        (ax / n, ay / n)
                    }
                };
                let grab = (hold_anchor.0 - anchor.0).hypot(hold_anchor.1 - anchor.1);
                adjusting = Some((kind, anchor, current.clone(), grab));
            }
        }

        // Pen lift can arrive as silence (no RELEASE) if the window loses
        // focus mid-stroke; commit only when events themselves stop — a pen
        // held still keeps trickling events and belongs to hold-to-snap.
        if pen_down && !erase_mode && last_pen_event.elapsed().as_millis() > 600 {
            pen_down = false;
            let was_adjusting = adjusting.take().is_some();
            let points = std::mem::take(&mut current);
            if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
                let (points, straightened) = match straighten(&points) {
                    Some(line) => (line, true),
                    None => (points, false),
                };
                strokes.push(ScreenStroke { id: format!("s{next_id}"), points });
                if straightened || was_adjusting {
                    redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                }
                next_id += 1;
                redo.clear();
                let outcome = recognize(&dictionary, &strokes, previous_ring.as_ref());
                previous_ring = outcome.ring.clone();
                let ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
                if ring_ids != snapped_ring_ids {
                    snapped_ring_ids = ring_ids;
                    redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                }
                render_feedback(&mut fb, &client, &outcome, &mut last_activation);
            }
        }
    }
}
