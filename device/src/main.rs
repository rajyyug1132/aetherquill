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

use qtfb::{QtfbClient, RM2_HEIGHT, RM2_WIDTH};

const W: i32 = RM2_WIDTH as i32;
const H: i32 = RM2_HEIGHT as i32;

const WHITE: u16 = 0xFFFF;
const BLACK: u16 = 0x0000;
const GRAY: u16 = 0x8410; // mid-gray in RGB565

// ponytail: screen px -> web-canvas px so the vendored CONFIG's pixel-tuned
// thresholds (ring.minRadius 70 etc.) hold unchanged on a 702x936 canvas.
const CANVAS_SCALE: f64 = 0.5;
const MIN_POINT_DIST: f64 = 1.4 / CANVAS_SCALE;
const MIN_STROKE_LEN: f64 = 7.0 / CANVAS_SCALE;

const INK_W: i32 = 3;
const CHROME_H: i32 = 110;
const BTN_W: i32 = 180;
const BTN_H: i32 = 80;

struct Fb<'a> {
    px: &'a mut [u16],
}

impl Fb<'_> {
    fn set(&mut self, x: i32, y: i32, c: u16) {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            self.px[(y * W + x) as usize] = c;
        }
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        for yy in y..(y + h) {
            for xx in x..(x + w) {
                self.set(xx, yy, c);
            }
        }
    }

    fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, t: i32, c: u16) {
        self.fill_rect(x, y, w, t, c);
        self.fill_rect(x, y + h - t, w, t, c);
        self.fill_rect(x, y, t, h, c);
        self.fill_rect(x + w - t, y, t, h, c);
    }

    /// Thick line as stamped disks along the segment.
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, thickness: i32, c: u16) {
        let (dx, dy) = ((x1 - x0) as f64, (y1 - y0) as f64);
        let steps = dx.hypot(dy).ceil().max(1.0) as i32;
        let r = thickness / 2;
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let cx = x0 + (dx * t) as i32;
            let cy = y0 + (dy * t) as i32;
            for oy in -r..=r {
                for ox in -r..=r {
                    if ox * ox + oy * oy <= r * r {
                        self.set(cx + ox, cy + oy, c);
                    }
                }
            }
        }
    }

    /// Circle outline (midpoint-ish via angle stepping — plenty for an overlay).
    fn circle(&mut self, cx: i32, cy: i32, radius: i32, thickness: i32, c: u16) {
        let steps = (radius.max(8) * 8) as usize;
        for i in 0..steps {
            let a = std::f64::consts::TAU * i as f64 / steps as f64;
            for t in 0..thickness {
                let r = (radius + t) as f64;
                self.set(cx + (a.cos() * r) as i32, cy + (a.sin() * r) as i32, c);
            }
        }
    }

    /// 8x8 bitmap font scaled up; returns text width in px.
    fn text(&mut self, x: i32, y: i32, s: &str, scale: i32, c: u16) -> i32 {
        let mut cx = x;
        for ch in s.chars() {
            let glyph = font8x8::legacy::BASIC_LEGACY.get(ch as usize).copied().unwrap_or([0; 8]);
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) != 0 {
                        self.fill_rect(cx + col * scale, y + row as i32 * scale, scale, scale, c);
                    }
                }
            }
            cx += 8 * scale;
        }
        cx - x
    }
}

struct ScreenStroke {
    id: String,
    points: Vec<(f64, f64)>,
}

fn path_length(points: &[(f64, f64)]) -> f64 {
    points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum()
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

/// Snap a finished-looking stroke to a perfect line or circle (reMarkable
/// "perfect shapes" style). Returns None when the stroke isn't close enough
/// to either. Winding of circles is preserved — spell direction semantics
/// (sign rotation) read stroke orientation.
fn snap_stroke(points: &[(f64, f64)]) -> Option<Vec<(f64, f64)>> {
    let len = path_length(points);
    if points.len() < 8 || len < 100.0 {
        return None;
    }
    let (p0, pn) = (points[0], points[points.len() - 1]);
    let end_dist = (pn.0 - p0.0).hypot(pn.1 - p0.1);

    // Straight line: path barely longer than the endpoint chord.
    if end_dist / len > 0.95 {
        return Some((0..16).map(|i| {
            let t = i as f64 / 15.0;
            (p0.0 + (pn.0 - p0.0) * t, p0.1 + (pn.1 - p0.1) * t)
        }).collect());
    }

    // Circle: centroid fit, low radius variance, ends near each other.
    let n = points.len() as f64;
    let (cx, cy) = points.iter().fold((0.0, 0.0), |(ax, ay), p| (ax + p.0, ay + p.1));
    let (cx, cy) = (cx / n, cy / n);
    let radii: Vec<f64> = points.iter().map(|p| (p.0 - cx).hypot(p.1 - cy)).collect();
    let mean_r = radii.iter().sum::<f64>() / n;
    let dev = (radii.iter().map(|r| (r - mean_r).powi(2)).sum::<f64>() / n).sqrt();
    let closed = end_dist < mean_r * 0.5;
    if !(closed && mean_r > 30.0 && dev / mean_r < 0.18) {
        return None;
    }
    // Winding via signed area.
    let signed_area: f64 = points.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum();
    let dir = if signed_area >= 0.0 { 1.0 } else { -1.0 };
    let start_angle = (p0.1 - cy).atan2(p0.0 - cx);
    Some((0..=64).map(|i| {
        let a = start_angle + dir * std::f64::consts::TAU * i as f64 / 64.0;
        (cx + mean_r * a.cos(), cy + mean_r * a.sin())
    }).collect())
}

fn draw_chrome(fb: &mut Fb) {
    fb.fill_rect(0, 0, W, CHROME_H, WHITE);
    fb.rect_outline(12, 15, BTN_W, BTN_H, 3, BLACK);
    fb.text(48, 32, "UNDO", 4, BLACK);
    fb.rect_outline(W - 12 - BTN_W, 15, BTN_W, BTN_H, 3, BLACK);
    fb.text(W - 12 - BTN_W + 28, 32, "CLEAR", 4, BLACK);
    fb.fill_rect(0, CHROME_H, W, 2, GRAY);
}

fn draw_status(fb: &mut Fb, client: &QtfbClient, line1: &str, line2: &str) {
    let left = 12 + BTN_W + 24;
    let width = W - 2 * left;
    fb.fill_rect(left, 6, width, CHROME_H - 12, WHITE);
    fb.text(left + 8, 18, line1, 3, BLACK);
    fb.text(left + 8, 60, line2, 2, GRAY);
    let _ = client.update_partial(left, 0, width, CHROME_H);
}

/// Full redraw. When a complete ring is known, its member strokes render as
/// one perfect circle (display-only snap — recognition still sees raw points).
fn redraw_all(fb: &mut Fb, client: &QtfbClient, strokes: &[ScreenStroke], ring: Option<&Ring>) {
    fb.fill_rect(0, 0, W, H, WHITE);
    draw_chrome(fb);
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
            fb.circle(cx, cy, radius + 10, 6, BLACK);
            fb.text(cx - element.len() as i32 * 3 * 8 / 2, cy + radius + 40, element, 6, BLACK);
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
    draw_chrome(&mut fb);
    let _ = client.update_all();
    draw_status(&mut fb, &client, "Draw a spell ring to begin", "");

    let mut strokes: Vec<ScreenStroke> = Vec::new();
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
                    if y < CHROME_H as f64 {
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
                    if !pen_down {
                        continue;
                    }
                    pen_down = false;
                    let points = std::mem::take(&mut current);
                    if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
                        strokes.push(ScreenStroke { id: format!("s{next_id}"), points });
                        next_id += 1;
                        let outcome = recognize(&dictionary, &strokes, previous_ring.as_ref());
                        previous_ring = outcome.ring.clone();
                        let ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
                        if ring_ids != snapped_ring_ids {
                            snapped_ring_ids = ring_ids;
                            redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref());
                        }
                        render_feedback(&mut fb, &client, &outcome, &mut last_activation);
                    } else if !points.is_empty() {
                        // Too short to keep: erase the stray ink dot.
                        redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref());
                    }
                }
                qtfb::INPUT_TOUCH_PRESS => {
                    if pen_down || event.y >= CHROME_H {
                        continue;
                    }
                    if event.x <= 12 + BTN_W {
                        strokes.pop();
                    } else if event.x >= W - 12 - BTN_W {
                        strokes.clear();
                    } else {
                        continue;
                    }
                    let outcome = recognize(&dictionary, &strokes, None);
                    previous_ring = outcome.ring.clone();
                    snapped_ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
                    redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref());
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

        // Hold-to-snap: pen still on the panel (events keep arriving inside
        // the 6px anchor) for 600ms — snap the in-progress stroke.
        if pen_down && !hold_snapped && hold_since.elapsed().as_millis() > 600 {
            hold_snapped = true;
            if let Some(snapped) = snap_stroke(&current) {
                current = snapped;
                redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref());
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
                let outcome = recognize(&dictionary, &strokes, previous_ring.as_ref());
                previous_ring = outcome.ring.clone();
                let ring_ids = previous_ring.as_ref().filter(|r| r.complete).map(|r| r.stroke_ids.clone()).unwrap_or_default();
                if ring_ids != snapped_ring_ids {
                    snapped_ring_ids = ring_ids;
                    redraw_all(&mut fb, &client, &strokes, previous_ring.as_ref());
                }
                render_feedback(&mut fb, &client, &outcome, &mut last_activation);
            }
        }
    }
}
