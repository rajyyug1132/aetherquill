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
use shapes::{path_length, snap_stroke};

use qtfb::{QtfbClient, RM2_HEIGHT, RM2_WIDTH};

const _: () = assert!(W == RM2_WIDTH as i32 && H == RM2_HEIGHT as i32);

// ponytail: screen px -> web-canvas px so the vendored CONFIG's pixel-tuned
// thresholds (ring.minRadius 70 etc.) hold unchanged on a 702x936 canvas.
const CANVAS_SCALE: f64 = 0.5;
const MIN_POINT_DIST: f64 = 1.4 / CANVAS_SCALE;
const MIN_STROKE_LEN: f64 = 7.0 / CANVAS_SCALE;

const INK_W: i32 = 3;

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

fn draw_status(fb: &mut Fb, client: &QtfbClient, line1: &str, line2: &str) {
    let left = SIDEBAR_W + 12;
    let width = W - left - 12;
    fb.fill_rect(left, 0, width, STATUS_H - 4, WHITE);
    fb.text(left + 8, 10, line1, 3, BLACK);
    fb.text(left + 8, 42, line2, 2, GRAY);
    let _ = client.update_partial(left, 0, width, STATUS_H);
}

/// Full redraw. When a complete ring is known, its member strokes render as
/// one perfect circle (display-only snap — recognition still sees raw points).
fn redraw_all(fb: &mut Fb, client: &QtfbClient, strokes: &[ScreenStroke], ring: Option<&Ring>, erase_mode: bool) {
    fb.fill_rect(0, 0, W, H, WHITE);
    draw_sidebar(fb, erase_mode);
    fb.fill_rect(SIDEBAR_W, STATUS_H - 2, W - SIDEBAR_W, 2, GRAY);
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
    eprintln!("recognize: {} strokes in {}ms", strokes.len(), t0.elapsed().as_millis());
    let spell = compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE);
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
        flush(client, 80);
    }
    effect_settle(fb, cx, cy, radius, element);
    flush(client, 60);
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
    // Touch-drag stroke moving: (stroke index, touch dev_id, last x, last y).
    let mut moving: Option<(usize, i32, f64, f64)> = None;
    let mut last_touch = Instant::now();
    let mut move_redraw = Instant::now();
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
                        // Pen is an eraser: delete any stroke it touches.
                        let hit = strokes.iter().position(|s| {
                            s.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < 20.0)
                        });
                        if let Some(idx) = hit {
                            redo.push(strokes.remove(idx));
                            erased_any = true;
                            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                        }
                        continue;
                    }
                    if (x - hold_anchor.0).hypot(y - hold_anchor.1) > 6.0 {
                        hold_anchor = (x, y);
                        hold_since = Instant::now();
                    }
                    if !pen_down {
                        pen_down = true;
                        hold_snapped = false;
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
                    let points = std::mem::take(&mut current);
                    if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
                        strokes.push(ScreenStroke { id: format!("s{next_id}"), points });
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
                            draw_sidebar(&mut fb, erase_mode);
                            let _ = client.update_partial(0, 0, SIDEBAR_W, icon_y(4) + ICON_H + 12);
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
            if let Some(snapped) = snap_stroke(&current) {
                current = snapped;
                redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref(), erase_mode);
                for w in current.windows(2) {
                    fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, INK_W, BLACK);
                }
                let _ = client.update_all();
                dirty_rect = None;
            }
        }

        // Pen lift can arrive as silence (no RELEASE) if the window loses
        // focus mid-stroke; commit only when events themselves stop — a pen
        // held still keeps trickling events and belongs to hold-to-snap.
        if pen_down && last_pen_event.elapsed().as_millis() > 600 {
            pen_down = false;
            let points = std::mem::take(&mut current);
            if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
                strokes.push(ScreenStroke { id: format!("s{next_id}"), points });
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
