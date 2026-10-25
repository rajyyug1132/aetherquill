//! Direct Wacom digitizer input (evdev), so live ink skips the
//! xochitl -> AppLoad -> qtfb socket hop. AppLoad still delivers its own pen
//! events; they're used only to (a) calibrate screen -> window coordinates
//! and (b) confirm our window had focus for a stroke (the digitizer reports
//! the pen even while you write in a notebook behind the app).
//! Pure decoding/state — the fd + ioctl live in main.rs. Host-testable.

use crate::render::{H, W};

/// Kernel `struct input_event` on the RM2's 32-bit ARM kernel:
/// sec u32, usec u32, type u16, code u16, value i32 = 16 bytes. Parsed by hand:
/// musl's libc struct uses 64-bit time_t and would misread it.
pub const EVENT_SIZE: usize = 16;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const BTN_TOUCH: u16 = 0x14a;

/// Typical RM2 digitizer maxima; main.rs reads the real ones via EVIOCGABS.
pub const DEFAULT_MAX_X: i32 = 20967;
pub const DEFAULT_MAX_Y: i32 = 15725;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PenEdge {
    Down,
    Move,
    Up,
}

/// One pen report in screen pixels (0..W, 0..H).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenSample {
    pub edge: PenEdge,
    pub x: f64,
    pub y: f64,
}

pub struct PenDecoder {
    max_x: f64,
    max_y: f64,
    raw_x: i32,
    raw_y: i32,
    touching: bool,
    was_touching: bool,
}

impl PenDecoder {
    pub fn new(max_x: i32, max_y: i32) -> Self {
        PenDecoder { max_x: max_x.max(1) as f64, max_y: max_y.max(1) as f64, raw_x: 0, raw_y: 0, touching: false, was_touching: false }
    }

    /// Digitizer is rotated 270° on RM1/RM2 (libremarkable
    /// get_wacom_placement): screen x follows raw Y, screen y follows
    /// max_x - raw X.
    pub fn to_screen(&self, raw_x: i32, raw_y: i32) -> (f64, f64) {
        (raw_y as f64 * W as f64 / self.max_y, (self.max_x - raw_x as f64) * H as f64 / self.max_x)
    }

    /// Feed whole 16-byte records; emits one sample per SYN_REPORT that has
    /// the pen on the glass (or just lifted). Hover reports emit nothing.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<PenSample>) {
        for rec in bytes.chunks_exact(EVENT_SIZE) {
            let kind = u16::from_le_bytes([rec[8], rec[9]]);
            let code = u16::from_le_bytes([rec[10], rec[11]]);
            let value = i32::from_le_bytes([rec[12], rec[13], rec[14], rec[15]]);
            match (kind, code) {
                (EV_ABS, ABS_X) => self.raw_x = value,
                (EV_ABS, ABS_Y) => self.raw_y = value,
                (EV_KEY, BTN_TOUCH) => self.touching = value != 0,
                (EV_SYN, SYN_REPORT) => {
                    let edge = match (self.was_touching, self.touching) {
                        (false, true) => Some(PenEdge::Down),
                        (true, true) => Some(PenEdge::Move),
                        (true, false) => Some(PenEdge::Up),
                        (false, false) => None,
                    };
                    self.was_touching = self.touching;
                    if let Some(edge) = edge {
                        let (x, y) = self.to_screen(self.raw_x, self.raw_y);
                        out.push(PenSample { edge, x, y });
                    }
                }
                _ => {}
            }
        }
    }
}

/// Screen px -> window px, per axis `w = scale * s + offset`. Starts as
/// identity (fullscreen window at the origin, the normal AppLoad case) and is
/// refit from (digitizer touch-down, AppLoad pen-press) pairs.
pub struct PenCalibration {
    pairs: Vec<((f64, f64), (f64, f64))>,
    sx: f64,
    ox: f64,
    sy: f64,
    oy: f64,
}

const CALIBRATION_PAIRS: usize = 16;
/// Below this screen-px spread one axis can't pin a scale; keep scale, fit offset.
const MIN_SCALE_SPREAD: f64 = 200.0;

impl Default for PenCalibration {
    fn default() -> Self {
        PenCalibration { pairs: Vec::new(), sx: 1.0, ox: 0.0, sy: 1.0, oy: 0.0 }
    }
}

impl PenCalibration {
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        (self.sx * x + self.ox, self.sy * y + self.oy)
    }

    pub fn add_pair(&mut self, screen: (f64, f64), window: (f64, f64)) {
        self.pairs.push((screen, window));
        if self.pairs.len() > CALIBRATION_PAIRS {
            self.pairs.remove(0);
        }
        let xs: Vec<(f64, f64)> = self.pairs.iter().map(|(s, w)| (s.0, w.0)).collect();
        let ys: Vec<(f64, f64)> = self.pairs.iter().map(|(s, w)| (s.1, w.1)).collect();
        (self.sx, self.ox) = fit_axis(&xs, self.sx);
        (self.sy, self.oy) = fit_axis(&ys, self.sy);
    }
}

fn fit_axis(pairs: &[(f64, f64)], current_scale: f64) -> (f64, f64) {
    let n = pairs.len() as f64;
    let mean_s = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_w = pairs.iter().map(|p| p.1).sum::<f64>() / n;
    let (min_s, max_s) = pairs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let scale = if max_s - min_s >= MIN_SCALE_SPREAD {
        let cov: f64 = pairs.iter().map(|p| (p.0 - mean_s) * (p.1 - mean_w)).sum();
        let var: f64 = pairs.iter().map(|p| (p.0 - mean_s).powi(2)).sum();
        (cov / var).clamp(0.5, 2.0)
    } else {
        current_scale
    };
    (scale, mean_w - scale * mean_s)
}

/// Commit a digitizer stroke only if AppLoad also saw pen input for it, i.e.
/// our window had focus. AppLoad's events lag the digitizer, so a release is
/// held up to `CONFIRM_WINDOW_S` waiting for that confirmation.
#[derive(Debug, Default)]
pub struct FocusGate {
    confirmed: bool,
    release_pending_since: Option<f64>,
}

pub const CONFIRM_WINDOW_S: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GateAction {
    Commit,
    Discard,
}

impl FocusGate {
    /// Digitizer touch-down. A release still waiting for confirmation is
    /// committed first (rapid taps), so strokes never merge.
    pub fn on_down(&mut self) -> Option<GateAction> {
        self.confirmed = false;
        self.release_pending_since.take().map(|_| GateAction::Commit)
    }

    /// Any AppLoad pen event. Returns Commit if a held release can now land.
    pub fn on_qtfb_pen(&mut self) -> Option<GateAction> {
        self.confirmed = true;
        self.release_pending_since.take().map(|_| GateAction::Commit)
    }

    /// Digitizer lift: commit now if confirmed, else hold.
    pub fn on_up(&mut self, now: f64) -> Option<GateAction> {
        if self.confirmed {
            Some(GateAction::Commit)
        } else {
            self.release_pending_since = Some(now);
            None
        }
    }

    /// Called every loop: a held release with no confirmation expires.
    pub fn tick(&mut self, now: f64) -> Option<GateAction> {
        match self.release_pending_since {
            Some(t) if now - t >= CONFIRM_WINDOW_S => {
                self.release_pending_since = None;
                Some(GateAction::Discard)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: u16, code: u16, value: i32) -> [u8; EVENT_SIZE] {
        let mut r = [0u8; EVENT_SIZE];
        r[8..10].copy_from_slice(&kind.to_le_bytes());
        r[10..12].copy_from_slice(&code.to_le_bytes());
        r[12..16].copy_from_slice(&value.to_le_bytes());
        r
    }

    fn report(raw_x: i32, raw_y: i32, touch: Option<bool>) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(rec(EV_ABS, ABS_X, raw_x));
        v.extend(rec(EV_ABS, ABS_Y, raw_y));
        if let Some(t) = touch {
            v.extend(rec(EV_KEY, BTN_TOUCH, t as i32));
        }
        v.extend(rec(EV_ABS, 0x18, 1234)); // pressure: ignored
        v.extend(rec(EV_SYN, SYN_REPORT, 0));
        v
    }

    #[test]
    fn rotation_maps_corners_like_libremarkable() {
        let d = PenDecoder::new(DEFAULT_MAX_X, DEFAULT_MAX_Y);
        // raw (max_x, 0) is the screen's top-left; raw (0, max_y) bottom-right.
        assert_eq!(d.to_screen(DEFAULT_MAX_X, 0), (0.0, 0.0));
        assert_eq!(d.to_screen(0, DEFAULT_MAX_Y), (W as f64, H as f64));
    }

    #[test]
    fn stroke_edges_follow_btn_touch_and_hover_is_silent() {
        let mut d = PenDecoder::new(DEFAULT_MAX_X, DEFAULT_MAX_Y);
        let mut out = Vec::new();
        let mut bytes = report(10000, 8000, None); // hover
        bytes.extend(report(10000, 8000, Some(true)));
        bytes.extend(report(9900, 8100, None));
        bytes.extend(report(9800, 8200, Some(false)));
        bytes.extend(report(9700, 8300, None)); // hover after lift
        d.feed(&bytes, &mut out);
        let edges: Vec<PenEdge> = out.iter().map(|s| s.edge).collect();
        assert_eq!(edges, [PenEdge::Down, PenEdge::Move, PenEdge::Up]);
        assert_eq!((out[0].x, out[0].y), d.to_screen(10000, 8000));
    }

    #[test]
    fn partial_trailing_record_is_ignored() {
        let mut d = PenDecoder::new(DEFAULT_MAX_X, DEFAULT_MAX_Y);
        let mut out = Vec::new();
        let mut bytes = report(10000, 8000, Some(true));
        bytes.extend_from_slice(&[0u8; 7]);
        d.feed(&bytes, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn calibration_starts_identity_and_learns_an_offset() {
        let mut c = PenCalibration::default();
        assert_eq!(c.map(100.0, 200.0), (100.0, 200.0));
        c.add_pair((500.0, 700.0), (480.0, 640.0));
        assert_eq!(c.map(500.0, 700.0), (480.0, 640.0));
        assert_eq!(c.map(600.0, 800.0), (580.0, 740.0), "one pair: offset only, scale kept at 1");
    }

    #[test]
    fn calibration_learns_scale_from_spread_pairs() {
        let mut c = PenCalibration::default();
        // Window drawn at half size, offset (100, 50).
        for (sx, sy) in [(200.0, 300.0), (1200.0, 1700.0), (700.0, 900.0)] {
            c.add_pair((sx, sy), (sx * 0.5 + 100.0, sy * 0.5 + 50.0));
        }
        let (x, y) = c.map(1000.0, 1000.0);
        assert!((x - 600.0).abs() < 1e-9 && (y - 550.0).abs() < 1e-9, "{x},{y}");
    }

    #[test]
    fn focus_gate_commits_confirmed_and_discards_unfocused_strokes() {
        let mut g = FocusGate::default();
        // AppLoad saw the stroke mid-way: release commits at once.
        assert_eq!(g.on_down(), None);
        assert_eq!(g.on_qtfb_pen(), None);
        assert_eq!(g.on_up(1.0), Some(GateAction::Commit));

        // Quick tap: AppLoad's press lands after the digitizer lift.
        assert_eq!(g.on_down(), None);
        assert_eq!(g.on_up(2.0), None);
        assert_eq!(g.tick(2.1), None);
        assert_eq!(g.on_qtfb_pen(), Some(GateAction::Commit));

        // Next tap starts before the previous one was confirmed: commit it first.
        assert_eq!(g.on_up(2.5), Some(GateAction::Commit), "still confirmed from the tap above");
        g.on_down();
        assert_eq!(g.on_up(2.6), None);
        assert_eq!(g.on_down(), Some(GateAction::Commit));
        assert_eq!(g.on_qtfb_pen(), None);
        g.on_up(2.7);

        // Window not focused: AppLoad never confirms -> discard after the window.
        g.on_down();
        assert_eq!(g.on_up(3.0), None);
        assert_eq!(g.tick(3.0 + CONFIRM_WINDOW_S - 0.01), None);
        assert_eq!(g.tick(3.0 + CONFIRM_WINDOW_S + 0.01), Some(GateAction::Discard));
        assert_eq!(g.tick(4.0), None, "discard fires once");
    }
}
