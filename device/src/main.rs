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

use std::collections::VecDeque;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
use recognizer::dictionaries::load_dictionary;
use recognizer::symbol_recognizer::Dictionary;
use recognizer::drawing_classifier::classify_drawing;
use recognizer::ring_detector::Ring;
use recognizer::spell_builder::{compile_spell, SpellIr};
use recognizer::stroke_cleaner::RawStroke;

mod grimoire;
mod qtfb;

use device::session::{to_raw_strokes, ScreenStroke, Session};
use device::{catalogue, chrome, lag, pen, render, resume, shapes, spell_view};
use chrome::{slot_at, SIDEBAR_W, STATUS_H};

use render::{effect_frame, effect_settle, Fb, BLACK, EFFECT_FRAMES, GRAY, H, W, WHITE};
use shapes::{path_length, snap_stroke};
use spell_view::EffectParams;

/// Opens the Wacom digitizer read-only and non-blocking (no EVIOCGRAB:
/// xochitl keeps receiving the pen). None -> fall back to AppLoad's pen events.
fn open_digitizer() -> Option<(libc::c_int, pen::PenDecoder)> {
    let name_path = (0..8)
        .map(|i| format!("/sys/class/input/event{i}/device/name"))
        .find(|p| std::fs::read_to_string(p).is_ok_and(|n| n.trim() == "Wacom I2C Digitizer"))?;
    let dev = format!("/dev/input/{}\0", name_path.split('/').nth(4)?);
    let fd = unsafe { libc::open(dev.as_ptr() as *const libc::c_char, libc::O_RDONLY | libc::O_NONBLOCK) };
    if fd < 0 {
        return None;
    }
    // EVIOCGABS(axis) = _IOR('E', 0x40 + axis, struct input_absinfo [6 x i32]).
    let abs_max = |axis: u32, fallback: i32| {
        let mut info = [0i32; 6]; // value, minimum, maximum, fuzz, flat, resolution
        let rc = unsafe { libc::ioctl(fd, (0x8018_4540u32 + axis) as _, info.as_mut_ptr()) };
        if rc == 0 && info[2] > 0 { info[2] } else { fallback }
    };
    let (max_x, max_y) = (abs_max(0, pen::DEFAULT_MAX_X), abs_max(1, pen::DEFAULT_MAX_Y));
    // EVIOCSCLOCKID = _IOW('E', 0xa0, int): this fd's event timestamps on
    // CLOCK_MONOTONIC (per client; xochitl's own fd is untouched), so the lag
    // log can compare them with clock_gettime.
    let clock: libc::c_int = libc::CLOCK_MONOTONIC;
    let rc = unsafe { libc::ioctl(fd, 0x4004_45a0u32 as _, &clock) };
    eprintln!("digitizer: {} max_x={max_x} max_y={max_y} monotonic_ts={}", dev.trim_end_matches('\0'), rc == 0);
    Some((fd, pen::PenDecoder::new(max_x, max_y)))
}

fn read_digitizer(fd: libc::c_int, decoder: &mut pen::PenDecoder, out: &mut Vec<pen::PenSample>) {
    let mut buf = [0u8; pen::EVENT_SIZE * 64];
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 {
            return;
        }
        decoder.feed(&buf[..n as usize], out);
        if (n as usize) < buf.len() {
            return;
        }
    }
}

/// Appends to lag::LOG_PATH only if it already exists (touch to enable).
fn lag_log(line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(lag::LOG_PATH) {
        let _ = writeln!(f, "{line}");
    }
}

fn pen_event(input_type: i32, x: f64, y: f64) -> qtfb::InputEvent {
    qtfb::InputEvent { input_type, dev_id: 0, x: x.round() as i32, y: y.round() as i32, d: 0 }
}

/// Seconds on a Linux clock (BOOTTIME keeps counting through suspend,
/// MONOTONIC doesn't — resume.rs compares the two).
fn clock_s(clock: libc::clockid_t) -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(clock, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 * 1e-9
}

use qtfb::{QtfbClient, RM2_HEIGHT, RM2_WIDTH};

const _: () = assert!(W == RM2_WIDTH as i32 && H == RM2_HEIGHT as i32);

use catalogue::CANVAS_SCALE;
const MIN_POINT_DIST: f64 = 1.4 / CANVAS_SCALE;
const MIN_STROKE_LEN: f64 = 7.0 / CANVAS_SCALE;

const INK_W: i32 = 3;

/// Coalesced ink refresh interval. Per-segment updates (~200Hz pen) flood the
/// qtfb socket; 40ms (25Hz) was the first safe value, 16ms (~60Hz) the current.
const INK_FLUSH_MS: u128 = 16;


fn draw_status(fb: &mut Fb, client: &QtfbClient, line1: &str, line2: &str) {
    let (x, y, w, h) = chrome::draw_status(fb, line1, line2);
    let _ = client.update_partial(x, y, w, h);
}

/// Full redraw. When a complete ring is known, its member strokes render as
/// one perfect circle (display-only snap — recognition still sees raw points).
fn redraw_all(fb: &mut Fb, client: &QtfbClient, strokes: &[ScreenStroke], ring: Option<&Ring>, erase_mode: bool) {
    fb.fill_rect(0, 0, W, H, WHITE);
    chrome::draw_chrome(fb, erase_mode);
    let ring = ring.filter(|r| r.complete && r.stroke_ids.iter().all(|id| strokes.iter().any(|s| &s.id == id)));
    let ring_ids: &[String] = ring.map_or(&[], |r| &r.stroke_ids);
    for stroke in strokes {
        if ring_ids.contains(&stroke.id) {
            continue;
        }
        for w in stroke.points.windows(2) {
            fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, INK_W, BLACK);
        }
    }
    if let Some(r) = ring {
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
    spell: SpellIr,
}

fn recognize(dictionary: &Dictionary, raw: &[RawStroke], previous_ring: Option<&Ring>) -> SpellOutcome {
    let t0 = Instant::now();
    let result = classify_drawing(raw, previous_ring, dictionary, env!("CARGO_PKG_VERSION"), &INPUT, &RING, &LAYERS, &RECOGNITION);
    let spell = compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!("recognize: {} strokes in {ms:.0}ms", raw.len());
    lag_log(&format!("{{\"recognize_ms\":{ms:.1},\"strokes\":{}}}", raw.len()));
    SpellOutcome { ring: if result.ring.found { Some(result.ring) } else { None }, spell }
}

type RecognitionJob = (u64, Vec<RawStroke>, Option<Ring>);

/// Recognition on a worker thread. On the RM2 one classify + compile takes
/// ~0.35 s (~1 s with a sketchy multi-segment ring), measured on the tablet
/// 2026-10-25; on the event loop that froze the ink after every pen-up. Each
/// request carries the whole stroke set; the worker skips requests a newer one
/// supersedes, and the loop applies only the newest generation's result.
struct Recognizer {
    jobs: mpsc::Sender<RecognitionJob>,
    results: mpsc::Receiver<(u64, SpellOutcome)>,
    generation: u64,
    /// True while the worker classifies (the lag log tags samples flushed then).
    busy: Arc<AtomicBool>,
}

impl Recognizer {
    fn spawn() -> Self {
        let (jobs, job_rx) = mpsc::channel::<RecognitionJob>();
        let (result_tx, results) = mpsc::channel();
        let busy = Arc::new(AtomicBool::new(false));
        let worker_busy = busy.clone();
        std::thread::spawn(move || {
            let dictionary = load_dictionary();
            while let Ok(mut job) = job_rx.recv() {
                while let Ok(newer) = job_rx.try_recv() {
                    job = newer;
                }
                let (generation, raw, previous_ring) = job;
                worker_busy.store(true, Ordering::Relaxed);
                let outcome = recognize(&dictionary, &raw, previous_ring.as_ref());
                worker_busy.store(false, Ordering::Relaxed);
                if result_tx.send((generation, outcome)).is_err() {
                    return;
                }
            }
        });
        Recognizer { jobs, results, generation: 0, busy }
    }

    fn request(&mut self, strokes: &[ScreenStroke], previous_ring: Option<Ring>) {
        self.generation += 1;
        let _ = self.jobs.send((self.generation, to_raw_strokes(strokes), previous_ring));
    }

    /// The latest request's result, once it has arrived; stale ones are dropped.
    fn latest(&self) -> Option<SpellOutcome> {
        let mut newest = None;
        while let Ok((generation, outcome)) = self.results.try_recv() {
            if generation == self.generation {
                newest = Some(outcome);
            }
        }
        newest
    }
}

/// Spell activation: plays the per-element effect animation (rising flames,
/// ripples, curling wind, rising earth, radiant light — ported from the
/// upstream particle renderer into 1-bit e-ink frames in `render::effect_frame`).
/// Each frame refreshes only the annulus around the seal, so the sigil inside
/// is untouched and the effect genuinely moves. Modifying signs shape it via
/// `params` (reach/weight/overlay from the compiled SpellIr — spell_view.rs).
fn animate_activation(fb: &mut Fb, client: &QtfbClient, cx: i32, cy: i32, radius: i32, element: &str, params: &EffectParams) {
    // Dirty rect only (clamped to screen + canvas by render). Pace on absolute
    // deadlines so the total stays EFFECT_FRAMES x EFFECT_FRAME_MS however long
    // a frame takes to draw. The refresh mode is NOT touched here (rm-appload
    // sleeps 1 s per mode change).
    let flush = |client: &QtfbClient, settle: bool, wait_until: Instant| {
        if let Some((x, y, w, h)) = render::effect_dirty_rect(cx, cy, radius, element, params, settle) {
            let _ = client.update_partial(x, y, w, h);
        }
        std::thread::sleep(wait_until.saturating_duration_since(Instant::now()));
    };
    let start = Instant::now();
    let tick = std::time::Duration::from_millis(render::EFFECT_FRAME_MS);
    for f in 0..EFFECT_FRAMES {
        effect_frame(fb, cx, cy, radius, element, f, params);
        flush(client, false, start + tick * (f as u32 + 1));
    }
    effect_settle(fb, cx, cy, radius, element, params);
    flush(client, true, Instant::now() + std::time::Duration::from_millis(60));
    // The fast waveform leaves every frame's ghost behind (seen on the RM2
    // after a Fire Shoot: flame tongues and pillar outlines on a clean
    // buffer); one full-quality refresh wipes them.
    let _ = client.request_full_refresh();
}

fn render_feedback(fb: &mut Fb, client: &QtfbClient, outcome: &SpellOutcome, last_activation: &mut String) {
    let spell = &outcome.spell;
    let (line1, line2) = chrome::spell_status(spell);
    draw_status(fb, client, &line1, &line2);

    let Some(ring) = &outcome.ring else { return };
    let cx = (ring.center.x / CANVAS_SCALE) as i32;
    let cy = (ring.center.y / CANVAS_SCALE) as i32;
    let radius = (ring.radius / CANVAS_SCALE) as i32;

    fb.circle(cx, cy, radius + 6, 2, GRAY);

    let activated = spell.active && spell.signature != *last_activation;
    let params = EffectParams::from_ir(spell);
    if activated {
        *last_activation = spell.signature.clone();
        if let Some(element) = &spell.element {
            grimoire::log_spell(element, spell.quality, spell.stability, &spell.signature);
            animate_activation(fb, client, cx, cy, radius, element, &params);
        }
    }

    // Halo, effect annulus and (for an element) the settle labels, from the
    // same geometry render.rs draws them with; clamped to the canvas/screen.
    let element = spell.element.as_deref().unwrap_or("");
    if let Some((x, y, w, h)) = render::effect_dirty_rect(cx, cy, radius, element, &params, spell.element.is_some()) {
        let _ = client.update_partial(x, y, w, h);
    }
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
    // Fast waveform for the whole session. Set ONCE: rm-appload v0.5.3's
    // server thread sleeps 1s after every MESSAGE_SET_REFRESH_MODE
    // (fbmanagement.cpp safetyWaitForEventLoopToCatchUp), stalling all our
    // updates — toggling it per stroke froze the start of every stroke.
    let _ = client.set_refresh_mode(qtfb::REFRESH_MODE_UFAST);
    let mut recognizer = Recognizer::spawn();

    // Owned pixel view for the lifetime of the app (single-threaded).
    let px: &'static mut [u16] = unsafe { std::mem::transmute(client.pixels()) };
    let mut fb = Fb { px };

    fb.fill_rect(0, 0, W, H, WHITE);
    chrome::draw_chrome(&mut fb, false);
    let _ = client.update_all();
    draw_status(&mut fb, &client, "Draw a spell ring to begin", "");

    // The page (session.rs): strokes, redo stack, last ring, snapped-ring ids.
    let mut session = Session::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    let mut pen_down = false;
    // Coalesced refresh: ink lands in shm per-segment, but e-ink update
    // requests flush at most every INK_FLUSH_MS — per-segment update_partial floods
    // the qtfb socket, lags the panel, and drops input events.
    let mut dirty_rect: Option<(i32, i32, i32, i32)> = None; // x0,y0,x1,y1
    let mut last_flush = Instant::now();
    // Hold-to-snap (reMarkable "perfect shapes"): pen held still mid-contact
    // for 600ms snaps the in-progress stroke to a line/circle.
    let mut hold_anchor = (0.0_f64, 0.0_f64);
    let mut hold_since = Instant::now();
    let mut hold_snapped = false;
    // Distinguishes "pen still, events trickling" (hold-to-snap) from "pen
    // gone, no events at all" (silence commit).
    let mut last_pen_event = Instant::now();
    // ERASE toggle (chrome button) — pen deletes strokes instead of inking.
    let mut erase_mode = false;
    let mut erased_any = false;
    // Touch-drag stroke moving: (stroke id, touch dev_id, last x, last y).
    let mut moving: Option<(String, i32, f64, f64)> = None;
    let mut last_touch = Instant::now();
    let mut move_redraw = Instant::now();
    // Touch arrives as a stream of PRESS events per contact — debounce chrome
    // buttons so one tap doesn't fire dozens of times.
    let mut last_chrome_tap = Instant::now() - std::time::Duration::from_secs(1);
    // Suspend/resume: redraw from the stroke store instead of trusting the panel.
    let mut resume = resume::ResumeDetector::default();
    // Direct digitizer ink (pen.rs): while active, AppLoad's pen events only
    // calibrate coordinates and confirm focus.
    let mut digitizer = open_digitizer();
    let mut calibration = pen::PenCalibration::default();
    let mut gate = pen::FocusGate::default();
    let mut samples: Vec<pen::PenSample> = Vec::new();
    let mut last_digitizer_down: Option<((f64, f64), Instant)> = None;
    let mut digitizer_stroke_live = false;
    let started = Instant::now();
    // Lag log (lag.rs): kernel time of each digitizer PRESS/UPDATE queued into
    // `events` (same order), of each inked sample awaiting a flush, and the
    // current stroke's latencies.
    let mut pen_ts: VecDeque<f64> = VecDeque::new();
    let mut ink_pending: Vec<f64> = Vec::new();
    let mut stroke_lat: Vec<f64> = Vec::new();
    let mut stroke_busy = 0usize;
    let mut lag_stroke = 0u64;

    // Spell Catalogue (catalogue.rs): panel open over the canvas?
    let spell_catalogue = catalogue::load();
    let mut panel = catalogue::Panel::default();
    // After a catalogue pick, the same finger's queued touch events must not
    // drag the freshly inked strokes: ignore canvas touch until it lifts.
    let mut touch_until_lift_ignored = false;

    // Pen-up commit: keep the stroke and queue recognition. Shared by pen
    // release, the silence commit and the Spell Catalogue, which inks through
    // this same path.
    macro_rules! commit_stroke {
        ($points:expr) => {{
            session.commit($points);
            recognizer.request(&session.strokes, session.previous_ring.clone());
        }};
    }

    // A recognition result: snap the ring once per newly completed ring, show
    // feedback (and the activation animation when the signature is new).
    macro_rules! apply_outcome {
        ($outcome:expr) => {{
            let outcome = $outcome;
            if session.apply_ring(outcome.ring.clone()) {
                redraw_all(&mut fb, &client, &session.strokes, session.previous_ring.as_ref(), erase_mode);
            }
            render_feedback(&mut fb, &client, &outcome, &mut session.last_activation);
        }};
    }

    // The page changed outside the pen-up path (undo/redo/clear/erase/drag/
    // resume): repaint now, recognize in the background.
    macro_rules! repaint_and_recognize {
        ($previous_ring:expr) => {{
            session.resnap();
            redraw_all(&mut fb, &client, &session.strokes, session.previous_ring.as_ref(), erase_mode);
            recognizer.request(&session.strokes, $previous_ring);
        }};
    }

    loop {
        // Block until the socket is readable (input or disconnect).
        let mut pfds = [
            libc::pollfd { fd: client.raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: digitizer.as_ref().map_or(-1, |d| d.0), events: libc::POLLIN, revents: 0 },
        ];
        // With ink pending, wake exactly at the flush deadline; else 20ms.
        let timeout = match dirty_rect {
            Some(_) => INK_FLUSH_MS.saturating_sub(last_flush.elapsed().as_millis()) as i32,
            None => 20,
        };
        unsafe { libc::poll(pfds.as_mut_ptr(), 2, timeout) };

        if resume.observe(clock_s(libc::CLOCK_BOOTTIME), clock_s(libc::CLOCK_MONOTONIC)) {
            eprintln!("resume detected — redrawing from {} strokes", session.strokes.len());
            // A stroke cut off by the nap is unreliable: drop it.
            pen_down = false;
            current.clear();
            dirty_rect = None;
            ink_pending.clear();
            stroke_lat.clear();
            stroke_busy = 0;
            moving = None;
            panel.close(); // the repaint below wipes it
            // The result restores the status line; same signature, so no replay.
            repaint_and_recognize!(session.previous_ring.clone());
        }

        let mut events = match client.drain_events() {
            Ok(events) => events,
            Err(_) => break, // window closed by AppLoad — exit
        };

        let mut discard_stroke = false;
        if let Some((fd, decoder)) = digitizer.as_mut() {
            samples.clear();
            read_digitizer(*fd, decoder, &mut samples);
            // Erasing isn't latency-critical: it keeps AppLoad's pen events.
            if !erase_mode {
                let now = started.elapsed().as_secs_f64();
                let mut commit = false;
                events.retain(|e| {
                    let is_pen = matches!(e.input_type, qtfb::INPUT_PEN_PRESS | qtfb::INPUT_PEN_UPDATE | qtfb::INPUT_PEN_RELEASE);
                    if is_pen {
                        if e.input_type == qtfb::INPUT_PEN_PRESS {
                            if let Some((screen, at)) = last_digitizer_down.take() {
                                if at.elapsed().as_millis() < 500 {
                                    calibration.add_pair(screen, (e.x as f64, e.y as f64));
                                }
                            }
                        }
                        commit |= gate.on_qtfb_pen() == Some(pen::GateAction::Commit);
                    }
                    !is_pen
                });
                if commit {
                    events.push(pen_event(qtfb::INPUT_PEN_RELEASE, 0.0, 0.0));
                }
                for s in &samples {
                    let (x, y) = calibration.map(s.x, s.y);
                    match s.edge {
                        pen::PenEdge::Down => {
                            if gate.on_down() == Some(pen::GateAction::Commit) {
                                events.push(pen_event(qtfb::INPUT_PEN_RELEASE, 0.0, 0.0));
                            }
                            last_digitizer_down = Some(((s.x, s.y), Instant::now()));
                            // A touch outside our window (xochitl chrome) isn't ours.
                            digitizer_stroke_live = (0.0..W as f64).contains(&x) && (0.0..H as f64).contains(&y);
                            if digitizer_stroke_live {
                                events.push(pen_event(qtfb::INPUT_PEN_PRESS, x, y));
                                pen_ts.push_back(s.t);
                            }
                        }
                        pen::PenEdge::Move if digitizer_stroke_live => {
                            events.push(pen_event(qtfb::INPUT_PEN_UPDATE, x, y));
                            pen_ts.push_back(s.t);
                        }
                        pen::PenEdge::Up if digitizer_stroke_live => {
                            digitizer_stroke_live = false;
                            if gate.on_up(now) == Some(pen::GateAction::Commit) {
                                events.push(pen_event(qtfb::INPUT_PEN_RELEASE, 0.0, 0.0));
                            }
                        }
                        _ => {}
                    }
                }
                discard_stroke = gate.tick(now) == Some(pen::GateAction::Discard);
            }
        }
        if discard_stroke {
            // The digitizer saw a stroke AppLoad never delivered: our window
            // wasn't focused. Drop it and repaint over its ink.
            eprintln!("digitizer stroke without focus - discarded");
            pen_down = false;
            current.clear();
            dirty_rect = None;
            ink_pending.clear();
            stroke_lat.clear();
            stroke_busy = 0;
            panel.close(); // the repaint below wipes it
            // redraw_all blanks the status strip; the result restores it, the halo and the label (same signature, no replay).
            repaint_and_recognize!(session.previous_ring.clone());
        }

        for event in events {
            match event.input_type {
                qtfb::INPUT_PEN_PRESS | qtfb::INPUT_PEN_UPDATE => {
                    let ts = pen_ts.pop_front();
                    let (x, y) = (event.x as f64, event.y as f64);
                    last_pen_event = Instant::now();
                    if panel.open || x < (SIDEBAR_W + 4) as f64 || y < STATUS_H as f64 {
                        continue;
                    }
                    if erase_mode {
                        // Pen is an eraser: delete any stroke it touches.
                        if session.erase_at(x, y) {
                            erased_any = true;
                            redraw_all(&mut fb, &client, &session.strokes, session.previous_ring.as_ref(), erase_mode);
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
                    ink_pending.extend(ts);
                    let (x0, y0) = (last.0.min(x) as i32 - INK_W, last.1.min(y) as i32 - INK_W);
                    let (x1, y1) = (last.0.max(x) as i32 + INK_W, last.1.max(y) as i32 + INK_W);
                    dirty_rect = Some(match dirty_rect {
                        Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
                        None => (x0, y0, x1, y1),
                    });
                    current.push((x, y));
                }
                qtfb::INPUT_PEN_RELEASE => {
                    eprintln!("pen release, {} pts", current.len());
                    if erase_mode {
                        if erased_any {
                            erased_any = false;
                            repaint_and_recognize!(None);
                        }
                        continue;
                    }
                    if !pen_down {
                        continue;
                    }
                    pen_down = false;
                    let points = std::mem::take(&mut current);
                    if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
                        commit_stroke!(points);
                    } else if !points.is_empty() {
                        // Too short to keep: erase the stray ink dot.
                        // redraw_all blanks the status strip; the result restores it, the halo and the label (same signature, no replay).
                        repaint_and_recognize!(session.previous_ring.clone());
                    }
                }
                // Real finger-up (rm-appload 0.5.3 sends it per lifted point):
                // ends the tap-drag guard and commits a stroke drag now. The
                // 400ms silence checks below remain as the fallback.
                qtfb::INPUT_TOUCH_RELEASE => {
                    touch_until_lift_ignored = false;
                    if moving.is_some() {
                        last_touch = Instant::now() - std::time::Duration::from_millis(500);
                    }
                }
                qtfb::INPUT_TOUCH_PRESS => {
                    if pen_down {
                        continue;
                    }
                    // Panel open: a tap picks a spell; anywhere else closes it.
                    if panel.open && event.x >= SIDEBAR_W {
                        if last_chrome_tap.elapsed().as_millis() < 250 {
                            continue;
                        }
                        last_chrome_tap = Instant::now();
                        let i = match panel.tap(event.x, event.y, spell_catalogue.len()) {
                            catalogue::PanelTap::Turned => {
                                catalogue::draw_panel(&mut fb, &spell_catalogue, panel.page);
                                let _ = client.update_partial(catalogue::PANEL_X, catalogue::PANEL_Y, catalogue::PANEL_W, catalogue::panel_h_for(spell_catalogue.len()));
                                continue;
                            }
                            catalogue::PanelTap::Dismissed => {
                                touch_until_lift_ignored = true;
                                last_touch = Instant::now();
                                // Repaint + recognize: the result brings back the status
                                // line, halo and label the panel covered (same
                                // signature, so no replay).
                                repaint_and_recognize!(session.previous_ring.clone());
                                continue;
                            }
                            catalogue::PanelTap::Pick(i) => i,
                        };
                        // Replace the page (session.pick_catalogue: the old drawing is
                        // gone, the last activation forgotten so the spell plays
                        // every time), then ink the new strokes on screen.
                        touch_until_lift_ignored = true;
                        last_touch = Instant::now();
                        session.pick_catalogue(spell_catalogue[i].screen_strokes());
                        redraw_all(&mut fb, &client, &[], None, erase_mode);
                        let _ = client.request_full_refresh();
                        eprintln!("catalogue: inking {}", spell_catalogue[i].id);
                        for stroke in &session.strokes {
                            let points = &stroke.points;
                            for w in points.windows(2) {
                                fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, INK_W, BLACK);
                            }
                            let (x0, x1) = points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
                            let (y0, y1) = points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
                            let _ = client.update_partial(x0 as i32 - INK_W, y0 as i32 - INK_W, (x1 - x0) as i32 + 2 * INK_W, (y1 - y0) as i32 + 2 * INK_W);
                        }
                        recognizer.request(&session.strokes, None);
                        continue;
                    }
                    if touch_until_lift_ignored && event.x >= SIDEBAR_W {
                        last_touch = Instant::now();
                        continue;
                    }
                    // In the canvas: touch-drag moves the stroke under the finger.
                    if event.x >= SIDEBAR_W && event.y >= STATUS_H {
                        let (x, y) = (event.x as f64, event.y as f64);
                        last_touch = Instant::now();
                        match moving.clone() {
                            Some((sid, id, lx, ly)) if id == event.dev_id => {
                                let (dx, dy) = (x - lx, y - ly);
                                session.move_stroke(&sid, dx, dy);
                                moving = Some((sid, id, x, y));
                                if move_redraw.elapsed().as_millis() > 150 {
                                    move_redraw = Instant::now();
                                    redraw_all(&mut fb, &client, &session.strokes, session.previous_ring.as_ref(), erase_mode);
                                }
                            }
                            None => {
                                if let Some(sid) = session.grab_at(x, y) {
                                    moving = Some((sid, event.dev_id, x, y));
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
                    let slot = slot_at(event.x, event.y);
                    eprintln!("sidebar tap {},{} -> slot {:?}", event.x, event.y, slot);
                    match slot {
                        Some(0) | Some(1) => {
                            erase_mode = slot == Some(1);
                            chrome::draw_sidebar(&mut fb, erase_mode);
                            let _ = client.update_partial(0, 0, SIDEBAR_W, chrome::toolbar_h());
                            continue;
                        }
                        Some(2) => {
                            panel.close();
                            session.undo();
                        }
                        Some(3) => {
                            panel.close();
                            session.redo();
                        }
                        Some(4) => {
                            panel.close();
                            session.clear(); // CLEAR is undoable via redo taps
                        }
                        Some(5) => {
                            if panel.toggle() {
                                catalogue::draw_panel(&mut fb, &spell_catalogue, panel.page);
                                let _ = client.update_partial(catalogue::PANEL_X, catalogue::PANEL_Y, catalogue::PANEL_W, catalogue::panel_h_for(spell_catalogue.len()));
                            } else {
                                // As a dismiss: the result restores what the panel covered.
                                repaint_and_recognize!(session.previous_ring.clone());
                            }
                            continue;
                        }
                        _ => continue,
                    }
                    repaint_and_recognize!(None);
                    if slot == Some(4) {
                        // CLEAR must leave a clean panel, not fast-waveform ghosts.
                        let _ = client.request_full_refresh();
                    }
                }
                _ => {}
            }
        }

        // Flush coalesced ink refreshes every INK_FLUSH_MS.
        if let Some((x0, y0, x1, y1)) = dirty_rect {
            if last_flush.elapsed().as_millis() >= INK_FLUSH_MS || !pen_down {
                let _ = client.update_partial(x0.max(0), y0.max(0), x1 - x0, y1 - y0);
                dirty_rect = None;
                last_flush = Instant::now();
                let sent = clock_s(libc::CLOCK_MONOTONIC);
                if recognizer.busy.load(Ordering::Relaxed) {
                    stroke_busy += ink_pending.len();
                }
                stroke_lat.extend(ink_pending.drain(..).map(|t| (sent - t) * 1000.0));
            }
        }
        // The stroke's last flush is done: log it.
        if !pen_down && dirty_rect.is_none() && !stroke_lat.is_empty() {
            lag_stroke += 1;
            lag_log(&lag::stroke_line(lag_stroke, &mut stroke_lat, stroke_busy));
            stroke_lat.clear();
            stroke_busy = 0;
        }

        // Recognition results land between strokes, never mid-stroke (a ring
        // snap repaints the page and would wipe live ink), and never under an
        // open catalogue panel (a redraw/animation would wipe it on screen
        // while taps still hit-test it). They wait in the channel until then.
        if !pen_down && panel.allows_repaint() {
            if let Some(outcome) = recognizer.latest() {
                apply_outcome!(outcome);
            }
        }

        if touch_until_lift_ignored && last_touch.elapsed().as_millis() > 400 {
            touch_until_lift_ignored = false;
        }

        // Finger lifted mid-drag: commit the move, re-recognize.
        if moving.is_some() && last_touch.elapsed().as_millis() > 400 && panel.allows_repaint() {
            moving = None;
            // A dragged ring stroke makes the old snapped circle wrong: drop it
            // until the result re-snaps.
            session.previous_ring = None;
            repaint_and_recognize!(None);
        }

        // Hold-to-snap: pen still on the panel (events keep arriving inside
        // the 6px anchor) for 600ms — snap the in-progress stroke.
        if pen_down && !hold_snapped && hold_since.elapsed().as_millis() > 600 {
            hold_snapped = true;
            if let Some(snapped) = snap_stroke(&current) {
                current = snapped;
                redraw_all(&mut fb, &client, &session.strokes, session.previous_ring.as_ref(), erase_mode);
                for w in current.windows(2) {
                    fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, INK_W, BLACK);
                }
                let _ = client.update_all();
                dirty_rect = None;
                // The hold already passed: these samples aren't ink lag.
                ink_pending.clear();
            }
        }

        // Pen lift can arrive as silence (no RELEASE) if the window loses
        // focus mid-stroke; commit only when events themselves stop — a pen
        // held still keeps trickling events and belongs to hold-to-snap.
        if pen_down && last_pen_event.elapsed().as_millis() > 600 {
            pen_down = false;
            let points = std::mem::take(&mut current);
            if points.len() >= 2 && path_length(&points) >= MIN_STROKE_LEN {
                commit_stroke!(points);
            }
        }
    }
}
