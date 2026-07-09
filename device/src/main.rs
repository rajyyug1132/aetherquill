//! WHA spell simulator — reMarkable 2 client, standalone build.
//!
//! Thin ink client: captures pen strokes, renders them locally with
//! low-latency DU partial refresh, and on every pen-up runs the whole
//! drawing through the `recognizer` crate directly — no network, no Node,
//! no toltec. Renders the result as a status line, a fitted-ring overlay,
//! and a region flash when a spell activates.
//!
//! Touch: tap UNDO / CLEAR boxes in the top corners; 4+ fingers at once exits.
//!
//! NOT YET BUILT ON HARDWARE OR EVEN `cargo check`ED: libremarkable only
//! targets Linux/ARM, and this machine has no ARM toolchain, WSL, or Docker
//! (see CLAUDE.md — the same caveat already applied to the tethered client/
//! this replaces). Written carefully against the verified `recognizer` API
//! (83 passing tests) and libremarkable 0.6's documented API, matching the
//! structure of the already-unverified `client/src/main.rs` it supersedes.
//! First real compile happens on the phased device-deploy path (LOOP.md
//! `device-crate` note / CLAUDE.md phase 3+).

use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};

use libremarkable::appctx::ApplicationContext;
use libremarkable::cgmath::{Point2, Vector2};
use libremarkable::framebuffer::common::{
    color, display_temp, dither_mode, mxcfb_rect, waveform_mode, DISPLAYWIDTH, DRAWING_QUANT_BIT,
};
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use libremarkable::framebuffer::{Framebuffer, FramebufferDraw, FramebufferRefresh};
use libremarkable::input::multitouch::MultitouchEvent;
use libremarkable::input::wacom::{WacomEvent, WacomPen};
use libremarkable::input::InputEvent;

use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
use recognizer::dictionaries::{load_dictionary, Dictionary};
use recognizer::drawing_classifier::classify_drawing;
use recognizer::geometry::Point;
use recognizer::ring_detector::Ring;
use recognizer::spell_builder::compile_spell;
use recognizer::stroke_cleaner::RawStroke;

const SCREEN_W: f32 = DISPLAYWIDTH as f32; // 1404; height is 1872

// ponytail: screen px -> web-canvas px, so the vendored CONFIG's pixel-tuned
// thresholds (ring.minRadius 70 etc.) hold unchanged on a 702x936 canvas —
// same reasoning as the tethered client this replaces.
const CANVAS_SCALE: f32 = 0.5;

const MIN_POINT_DIST: f32 = 1.4 / CANVAS_SCALE;
const MIN_STROKE_LEN: f32 = 7.0 / CANVAS_SCALE;

const INK_WIDTH: u32 = 3;
const CHROME_H: f32 = 110.0; // top strip: buttons + status text; pen is ignored here
const BTN_W: i32 = 180;
const BTN_H: i32 = 80;

/// A stroke in screen px, as captured from the pen. Converted to
/// recognizer::geometry::Point (web-canvas px) only when snapshotting for
/// recognition — kept in screen px here since that's what drawing needs.
#[derive(Clone)]
struct ScreenStroke {
    id: String,
    points: Vec<(f32, f32)>,
}

fn path_length(points: &[(f32, f32)]) -> f32 {
    points.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum()
}

fn to_raw_strokes(strokes: &[ScreenStroke]) -> Vec<RawStroke> {
    strokes
        .iter()
        .map(|s| RawStroke {
            id: s.id.clone(),
            points: s.points.iter().map(|&(x, y)| Point { x: (x * CANVAS_SCALE) as f64, y: (y * CANVAS_SCALE) as f64 }).collect(),
        })
        .collect()
}

fn draw_chrome(fb: &mut Framebuffer) {
    let w = SCREEN_W as i32;
    fb.fill_rect(Point2 { x: 0, y: 0 }, Vector2 { x: SCREEN_W as u32, y: CHROME_H as u32 }, color::WHITE);
    fb.draw_rect(Point2 { x: 12, y: 15 }, Vector2 { x: BTN_W as u32, y: BTN_H as u32 }, 3, color::BLACK);
    fb.draw_text(Point2 { x: 52.0, y: 68.0 }, "UNDO", 34.0, color::BLACK, false);
    fb.draw_rect(Point2 { x: w - 12 - BTN_W, y: 15 }, Vector2 { x: BTN_W as u32, y: BTN_H as u32 }, 3, color::BLACK);
    fb.draw_text(Point2 { x: (w - 12 - BTN_W) as f32 + 34.0, y: 68.0 }, "CLEAR", 34.0, color::BLACK, false);
    fb.draw_line(Point2 { x: 0, y: CHROME_H as i32 }, Point2 { x: w, y: CHROME_H as i32 }, 1, color::GRAY(0x80));
}

fn refresh_chrome(fb: &mut Framebuffer) {
    fb.partial_refresh(
        &mxcfb_rect { top: 0, left: 0, width: SCREEN_W as u32, height: CHROME_H as u32 + 2 },
        PartialRefreshMode::Async,
        waveform_mode::WAVEFORM_MODE_GC16_FAST,
        display_temp::TEMP_USE_MAX,
        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
        0,
        false,
    );
}

fn draw_status(fb: &mut Framebuffer, line1: &str, line2: &str) {
    let left = (12 + BTN_W + 24) as u32;
    let width = SCREEN_W as u32 - 2 * left;
    fb.fill_rect(Point2 { x: left as i32, y: 6 }, Vector2 { x: width, y: CHROME_H as u32 - 12 }, color::WHITE);
    fb.draw_text(Point2 { x: left as f32 + 8.0, y: 48.0 }, line1, 32.0, color::BLACK, false);
    fb.draw_text(Point2 { x: left as f32 + 8.0, y: 92.0 }, line2, 26.0, color::GRAY(0x60), false);
    fb.partial_refresh(
        &mxcfb_rect { top: 0, left, width, height: CHROME_H as u32 },
        PartialRefreshMode::Async,
        waveform_mode::WAVEFORM_MODE_GC16_FAST,
        display_temp::TEMP_USE_MAX,
        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
        0,
        false,
    );
}

fn redraw_all(fb: &mut Framebuffer, strokes: &[ScreenStroke]) {
    fb.clear();
    draw_chrome(fb);
    for stroke in strokes {
        for w in stroke.points.windows(2) {
            fb.draw_line(
                Point2 { x: w[0].0 as i32, y: w[0].1 as i32 },
                Point2 { x: w[1].0 as i32, y: w[1].1 as i32 },
                INK_WIDTH,
                color::BLACK,
            );
        }
    }
    fb.full_refresh(waveform_mode::WAVEFORM_MODE_GC16, display_temp::TEMP_USE_AMBIENT, dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH, 0, true);
}

/// One recognition pass: cleanStrokes -> detectRing -> ... -> compileSpell,
/// entirely in-process. Replaces the old TCP oracle round-trip.
struct RecognitionResult {
    ring: Option<Ring>,
    status: String,
    element: Option<String>,
    valid: bool,
    active: bool,
    quality: f64,
    stability: f64,
    warnings_first: Option<String>,
    signature: String,
}

fn recognize(dictionary: &Dictionary, strokes: &[ScreenStroke], previous_ring: Option<&Ring>) -> RecognitionResult {
    let raw = to_raw_strokes(strokes);
    let classify_result = classify_drawing(&raw, previous_ring, dictionary, "0.1.0", &INPUT, &RING, &LAYERS, &RECOGNITION);
    let spell = compile_spell(&classify_result.glyph_ast, &COMPILER, &EFFECT_SIZE);

    RecognitionResult {
        ring: if classify_result.ring.found { Some(classify_result.ring) } else { None },
        status: spell.status,
        element: spell.element,
        valid: spell.valid,
        active: spell.active,
        quality: spell.quality,
        stability: spell.stability,
        warnings_first: spell.warnings.first().map(|w| w.as_str().to_string()),
        signature: spell.signature,
    }
}

fn render_feedback(fb: &mut Framebuffer, result: &RecognitionResult, last_activation: &mut String) {
    let line1 = match &result.element {
        Some(element) if result.valid => {
            format!("{} — {}   quality {:.0}%  stability {:.0}%", result.status, element, result.quality * 100.0, result.stability * 100.0)
        }
        _ => result.status.clone(),
    };
    let line2 = result.warnings_first.clone().unwrap_or_default();
    draw_status(fb, &line1, &line2);

    let Some(ring) = &result.ring else { return };
    let cx = (ring.center.x as f32 / CANVAS_SCALE) as i32;
    let cy = (ring.center.y as f32 / CANVAS_SCALE) as i32;
    let radius = (ring.radius as f32 / CANVAS_SCALE) as u32;
    fb.draw_circle(Point2 { x: cx, y: cy }, radius + 6, color::GRAY(0x40));
    fb.draw_circle(Point2 { x: cx, y: cy }, radius + 7, color::GRAY(0x40));

    let activated = result.active && result.signature != *last_activation;
    if activated {
        *last_activation = result.signature.clone();
        for extra in 10..16 {
            fb.draw_circle(Point2 { x: cx, y: cy }, radius + extra, color::BLACK);
        }
        if let Some(element) = &result.element {
            fb.draw_text(Point2 { x: (cx - 60) as f32, y: (cy + radius as i32 + 70) as f32 }, element, 52.0, color::BLACK, false);
        }
    }

    let pad = 90u32;
    let region = mxcfb_rect {
        top: (cy as u32).saturating_sub(radius + pad),
        left: (cx as u32).saturating_sub(radius + pad),
        width: (radius + pad) * 2,
        height: (radius + pad) * 2,
    };
    fb.partial_refresh(
        &region,
        PartialRefreshMode::Async,
        waveform_mode::WAVEFORM_MODE_GC16_FAST,
        display_temp::TEMP_USE_MAX,
        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
        0,
        activated, // spell activation = region flash; the e-ink "spell fires" moment
    );
}

fn recognition_worker(ctx: &mut ApplicationContext<'static>, rx: Receiver<Vec<ScreenStroke>>) {
    let dictionary = load_dictionary();
    let fb = ctx.get_framebuffer_ref();
    let mut previous_ring: Option<Ring> = None;
    let mut last_activation = String::new();

    while let Ok(mut strokes) = rx.recv() {
        // ponytail: drop stale snapshots, only the latest drawing matters.
        while let Ok(newer) = rx.try_recv() {
            strokes = newer;
        }
        let result = recognize(&dictionary, &strokes, previous_ring.as_ref());
        previous_ring = result.ring.clone();
        render_feedback(fb, &result, &mut last_activation);
    }
}

struct State {
    strokes: Vec<ScreenStroke>,
    current: Vec<(f32, f32)>,
    pen_down: bool,
    fingers: HashSet<i32>,
    next_id: u64,
    tx: Sender<Vec<ScreenStroke>>,
}

impl State {
    fn commit_stroke(&mut self, fb: &mut Framebuffer) {
        self.pen_down = false;
        let points = std::mem::take(&mut self.current);
        if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
            self.strokes.push(ScreenStroke { id: format!("s{}", self.next_id), points });
            self.next_id += 1;
            self.tx.send(self.strokes.clone()).ok();
        } else if !points.is_empty() {
            // Too short to keep: erase the stray ink dot.
            redraw_all(fb, &self.strokes);
        }
    }

    fn on_wacom(&mut self, fb: &mut Framebuffer, event: WacomEvent) {
        match event {
            WacomEvent::Draw { position, pressure, .. } => {
                if pressure == 0 || position.y < CHROME_H {
                    return;
                }
                let point = (position.x, position.y);
                if !self.pen_down {
                    self.pen_down = true;
                    self.current = vec![point];
                    return;
                }
                let last = *self.current.last().unwrap();
                let dist = ((point.0 - last.0).powi(2) + (point.1 - last.1).powi(2)).sqrt();
                if dist < MIN_POINT_DIST {
                    return;
                }
                let rect = fb.draw_line(
                    Point2 { x: last.0 as i32, y: last.1 as i32 },
                    Point2 { x: point.0 as i32, y: point.1 as i32 },
                    INK_WIDTH,
                    color::BLACK,
                );
                fb.partial_refresh(
                    &rect,
                    PartialRefreshMode::Async,
                    waveform_mode::WAVEFORM_MODE_DU,
                    display_temp::TEMP_USE_REMARKABLE_DRAW,
                    dither_mode::EPDC_FLAG_EXP1,
                    DRAWING_QUANT_BIT,
                    false,
                );
                self.current.push(point);
            }
            WacomEvent::Hover { .. } => {
                if self.pen_down {
                    self.commit_stroke(fb);
                }
            }
            WacomEvent::InstrumentChange { pen: WacomPen::Touch, state: false } => {
                if self.pen_down {
                    self.commit_stroke(fb);
                }
            }
            _ => {}
        }
    }

    fn on_touch(&mut self, fb: &mut Framebuffer, event: MultitouchEvent) {
        match event {
            MultitouchEvent::Press { finger } => {
                self.fingers.insert(finger.tracking_id);
                if self.fingers.len() >= 4 {
                    // Exit gesture; the run script restores xochitl.
                    std::process::exit(0);
                }
                if self.pen_down || finger.pos.y as f32 >= CHROME_H {
                    return;
                }
                let x = finger.pos.x as i32;
                if x <= 12 + BTN_W {
                    self.strokes.pop();
                } else if x >= SCREEN_W as i32 - 12 - BTN_W {
                    self.strokes.clear();
                } else {
                    return;
                }
                redraw_all(fb, &self.strokes);
                self.tx.send(self.strokes.clone()).ok();
            }
            MultitouchEvent::Release { finger } => {
                self.fingers.remove(&finger.tracking_id);
            }
            _ => {}
        }
    }
}

fn main() {
    let mut app = ApplicationContext::default();
    app.clear(true);

    let fb = app.get_framebuffer_ref();
    draw_chrome(fb);
    draw_status(fb, "Draw a spell ring to begin", "");
    refresh_chrome(fb);

    let (tx, rx) = channel::<Vec<ScreenStroke>>();
    let worker_ctx = app.upgrade_ref();
    std::thread::spawn(move || recognition_worker(worker_ctx, rx));

    let mut state = State { strokes: Vec::new(), current: Vec::new(), pen_down: false, fingers: HashSet::new(), next_id: 1, tx };

    app.start_event_loop(true, true, false, |ctx, event| {
        let fb = ctx.get_framebuffer_ref();
        match event {
            InputEvent::WacomEvent { event } => state.on_wacom(fb, event),
            InputEvent::MultitouchEvent { event } => state.on_touch(fb, event),
            _ => {}
        }
    });
}
