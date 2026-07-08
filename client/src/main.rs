//! WHA spell simulator — reMarkable 2 client.
//!
//! Thin ink client: captures pen strokes, renders them locally with
//! low-latency DU partial refresh, and on every pen-up ships the whole
//! drawing to the recognition oracle (Node service running the vendored
//! wha-spell-simulator pipeline). Renders the reply as a status line,
//! a fitted-ring overlay, and a region flash when a spell activates.
//!
//! Touch: tap UNDO / CLEAR boxes in the top corners; 4+ fingers at once exits.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

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
use serde::{Deserialize, Serialize};

const SCREEN_W: f32 = DISPLAYWIDTH as f32; // 1404; height is 1872

// ponytail: screen px -> web-canvas px, so the vendored CONFIG's pixel-tuned
// thresholds (ring.minRadius 70 etc.) hold unchanged on a 702x936 canvas.
const CANVAS_SCALE: f32 = 0.5;

// Mirror of the web app's input config, scaled to screen px.
const MIN_POINT_DIST: f32 = 1.4 / CANVAS_SCALE;
const MIN_STROKE_LEN: f32 = 7.0 / CANVAS_SCALE;

const INK_WIDTH: u32 = 3;
const CHROME_H: f32 = 110.0; // top strip: buttons + status text; pen is ignored here
const BTN_W: i32 = 180;
const BTN_H: i32 = 80;

#[derive(Clone, Serialize)]
struct ReqPoint {
    x: f32,
    y: f32,
    pressure: f32,
    t: u64,
}

#[derive(Clone, Serialize)]
struct ReqStroke {
    id: String,
    points: Vec<ReqPoint>,
}

#[derive(Deserialize, Default)]
struct Xy {
    x: f32,
    y: f32,
}

#[derive(Deserialize, Default)]
struct Ring {
    found: bool,
    center: Option<Xy>,
    radius: Option<f32>,
}

#[derive(Deserialize, Default)]
struct GlyphAst {
    ring: Ring,
}

#[derive(Deserialize, Default)]
struct SpellIr {
    valid: bool,
    active: bool,
    status: String,
    element: Option<String>,
    quality: f32,
    stability: f32,
    #[serde(default)]
    warnings: Vec<String>,
    signature: String,
}

#[derive(Deserialize)]
struct Reply {
    #[serde(rename = "glyphAST")]
    glyph_ast: Option<GlyphAst>,
    #[serde(rename = "spellIR")]
    spell_ir: Option<SpellIr>,
    error: Option<String>,
}

fn path_length(points: &[ReqPoint]) -> f32 {
    points
        .windows(2)
        .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
        .sum()
}

fn draw_chrome(fb: &mut Framebuffer) {
    let w = SCREEN_W as i32;
    fb.fill_rect(
        Point2 { x: 0, y: 0 },
        Vector2 { x: SCREEN_W as u32, y: CHROME_H as u32 },
        color::WHITE,
    );
    fb.draw_rect(
        Point2 { x: 12, y: 15 },
        Vector2 { x: BTN_W as u32, y: BTN_H as u32 },
        3,
        color::BLACK,
    );
    fb.draw_text(Point2 { x: 52.0, y: 68.0 }, "UNDO", 34.0, color::BLACK, false);
    fb.draw_rect(
        Point2 { x: w - 12 - BTN_W, y: 15 },
        Vector2 { x: BTN_W as u32, y: BTN_H as u32 },
        3,
        color::BLACK,
    );
    fb.draw_text(
        Point2 { x: (w - 12 - BTN_W) as f32 + 34.0, y: 68.0 },
        "CLEAR",
        34.0,
        color::BLACK,
        false,
    );
    fb.draw_line(
        Point2 { x: 0, y: CHROME_H as i32 },
        Point2 { x: w, y: CHROME_H as i32 },
        1,
        color::GRAY(0x80),
    );
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
    fb.fill_rect(
        Point2 { x: left as i32, y: 6 },
        Vector2 { x: width, y: CHROME_H as u32 - 12 },
        color::WHITE,
    );
    fb.draw_text(Point2 { x: left as f32 + 8.0, y: 48.0 }, line1, 32.0, color::BLACK, false);
    fb.draw_text(
        Point2 { x: left as f32 + 8.0, y: 92.0 },
        line2,
        26.0,
        color::GRAY(0x60),
        false,
    );
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

fn redraw_all(fb: &mut Framebuffer, strokes: &[ReqStroke]) {
    fb.clear();
    draw_chrome(fb);
    for stroke in strokes {
        for w in stroke.points.windows(2) {
            fb.draw_line(
                Point2 { x: w[0].x as i32, y: w[0].y as i32 },
                Point2 { x: w[1].x as i32, y: w[1].y as i32 },
                INK_WIDTH,
                color::BLACK,
            );
        }
    }
    fb.full_refresh(
        waveform_mode::WAVEFORM_MODE_GC16,
        display_temp::TEMP_USE_AMBIENT,
        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
        0,
        true,
    );
}

/// Serialize strokes for the oracle: screen px -> web-canvas px.
fn snapshot(strokes: &[ReqStroke]) -> String {
    let scaled: Vec<ReqStroke> = strokes
        .iter()
        .map(|stroke| ReqStroke {
            id: stroke.id.clone(),
            points: stroke
                .points
                .iter()
                .map(|p| ReqPoint {
                    x: p.x * CANVAS_SCALE,
                    y: p.y * CANVAS_SCALE,
                    pressure: p.pressure,
                    t: p.t,
                })
                .collect(),
        })
        .collect();
    format!(
        "{}\n",
        serde_json::json!({ "strokes": scaled })
    )
}

fn oracle_roundtrip(
    conn: &mut Option<BufReader<TcpStream>>,
    addr: &str,
    request: &str,
) -> Result<Reply, String> {
    for attempt in 0..2 {
        if conn.is_none() {
            let stream = TcpStream::connect(addr).map_err(|e| e.to_string())?;
            stream.set_nodelay(true).ok();
            *conn = Some(BufReader::new(stream));
        }
        let reader = conn.as_mut().unwrap();
        let io = reader
            .get_ref()
            .try_clone()
            .and_then(|mut s| s.write_all(request.as_bytes()))
            .and_then(|_| {
                let mut line = String::new();
                reader.read_line(&mut line).map(|_| line)
            });
        match io {
            Ok(line) if !line.trim().is_empty() => {
                return serde_json::from_str(&line).map_err(|e| e.to_string());
            }
            _ if attempt == 0 => *conn = None, // stale connection: reconnect once
            Ok(_) => return Err("oracle closed connection".into()),
            Err(e) => return Err(e.to_string()),
        }
    }
    unreachable!()
}

fn render_feedback(fb: &mut Framebuffer, reply: &Reply, last_activation: &mut String) {
    if let Some(error) = &reply.error {
        draw_status(fb, "oracle error", error);
        return;
    }
    let default_ir = SpellIr::default();
    let ir = reply.spell_ir.as_ref().unwrap_or(&default_ir);
    let ring = reply.glyph_ast.as_ref().map(|g| &g.ring);

    let line1 = match &ir.element {
        Some(element) if ir.valid => format!(
            "{} — {}   quality {:.0}%  stability {:.0}%",
            ir.status,
            element,
            ir.quality * 100.0,
            ir.stability * 100.0
        ),
        _ => ir.status.clone(),
    };
    let line2 = ir.warnings.first().cloned().unwrap_or_default();
    draw_status(fb, &line1, &line2);

    // Fitted-ring overlay: "the paper accepted your ring".
    if let Some(Ring { found: true, center: Some(c), radius: Some(r) }) = ring {
        let cx = (c.x / CANVAS_SCALE) as i32;
        let cy = (c.y / CANVAS_SCALE) as i32;
        let radius = (r / CANVAS_SCALE) as u32;
        fb.draw_circle(Point2 { x: cx, y: cy }, radius + 6, color::GRAY(0x40));
        fb.draw_circle(Point2 { x: cx, y: cy }, radius + 7, color::GRAY(0x40));

        let activated = ir.active && ir.signature != *last_activation;
        if activated {
            *last_activation = ir.signature.clone();
            for extra in 10..16 {
                fb.draw_circle(Point2 { x: cx, y: cy }, radius + extra, color::BLACK);
            }
            if let Some(element) = &ir.element {
                fb.draw_text(
                    Point2 { x: (cx - 60) as f32, y: (cy + radius as i32 + 70) as f32 },
                    element,
                    52.0,
                    color::BLACK,
                    false,
                );
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
}

fn oracle_worker(ctx: &mut ApplicationContext<'static>, rx: Receiver<String>, addr: String) {
    let fb = ctx.get_framebuffer_ref();
    let mut conn: Option<BufReader<TcpStream>> = None;
    let mut last_activation = String::new();
    while let Ok(mut request) = rx.recv() {
        // ponytail: drop stale snapshots, only the latest drawing matters
        while let Ok(newer) = rx.try_recv() {
            request = newer;
        }
        match oracle_roundtrip(&mut conn, &addr, &request) {
            Ok(reply) => render_feedback(fb, &reply, &mut last_activation),
            Err(e) => draw_status(fb, "oracle unreachable — check WHA_ORACLE_ADDR", &e),
        }
    }
}

struct State {
    strokes: Vec<ReqStroke>,
    current: Vec<ReqPoint>,
    pen_down: bool,
    fingers: HashSet<i32>,
    next_id: u64,
    started: Instant,
    tx: Sender<String>,
}

impl State {
    fn commit_stroke(&mut self, fb: &mut Framebuffer) {
        self.pen_down = false;
        let points = std::mem::take(&mut self.current);
        if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
            self.strokes.push(ReqStroke { id: format!("s{}", self.next_id), points });
            self.next_id += 1;
            self.tx.send(snapshot(&self.strokes)).ok();
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
                let point = ReqPoint {
                    x: position.x,
                    y: position.y,
                    pressure: pressure as f32 / 4096.0,
                    t: self.started.elapsed().as_millis() as u64,
                };
                if !self.pen_down {
                    self.pen_down = true;
                    self.current = vec![point];
                    return;
                }
                let last = self.current.last().unwrap();
                let dist = ((point.x - last.x).powi(2) + (point.y - last.y).powi(2)).sqrt();
                if dist < MIN_POINT_DIST {
                    return;
                }
                let rect = fb.draw_line(
                    Point2 { x: last.x as i32, y: last.y as i32 },
                    Point2 { x: point.x as i32, y: point.y as i32 },
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
                self.tx.send(snapshot(&self.strokes)).ok();
            }
            MultitouchEvent::Release { finger } => {
                self.fingers.remove(&finger.tracking_id);
            }
            _ => {}
        }
    }
}

fn main() {
    let addr =
        std::env::var("WHA_ORACLE_ADDR").unwrap_or_else(|_| "10.11.99.2:7777".to_string());
    let mut app = ApplicationContext::default();
    app.clear(true);

    let fb = app.get_framebuffer_ref();
    draw_chrome(fb);
    draw_status(fb, "Draw a spell ring to begin", "");
    refresh_chrome(fb);

    let (tx, rx) = channel::<String>();
    let worker_ctx = app.upgrade_ref();
    let worker_addr = addr.clone();
    std::thread::spawn(move || oracle_worker(worker_ctx, rx, worker_addr));

    let mut state = State {
        strokes: Vec::new(),
        current: Vec::new(),
        pen_down: false,
        fingers: HashSet::new(),
        next_id: 1,
        started: Instant::now(),
        tx,
    };

    app.start_event_loop(true, true, false, |ctx, event| {
        let fb = ctx.get_framebuffer_ref();
        match event {
            InputEvent::WacomEvent { event } => state.on_wacom(fb, event),
            InputEvent::MultitouchEvent { event } => state.on_touch(fb, event),
            _ => {}
        }
    });
}
