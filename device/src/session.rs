//! The page: committed strokes, the redo stack and the snapped-ring
//! bookkeeping, pulled out of main.rs's event loop so it can be host-tested
//! (and fuzzed) without qtfb. Pure data — no drawing, no recognition; main.rs
//! calls these methods and then repaints / requests recognition itself.

use recognizer::geometry::Point;
use recognizer::ring_detector::Ring;
use recognizer::stroke_cleaner::RawStroke;

use crate::catalogue::CANVAS_SCALE;

pub struct ScreenStroke {
    pub id: String,
    pub points: Vec<(f64, f64)>,
}

pub fn to_raw_strokes(strokes: &[ScreenStroke]) -> Vec<RawStroke> {
    strokes
        .iter()
        .map(|s| RawStroke {
            id: s.id.clone(),
            points: s.points.iter().map(|&(x, y)| Point { x: x * CANVAS_SCALE, y: y * CANVAS_SCALE }).collect(),
        })
        .collect()
}

/// Eraser reach (screen px): any stroke point this close to the pen is hit.
pub const ERASE_RADIUS: f64 = 20.0;
/// Finger reach for grabbing a stroke to drag.
pub const GRAB_RADIUS: f64 = 40.0;

pub struct Session {
    pub strokes: Vec<ScreenStroke>,
    pub redo: Vec<ScreenStroke>,
    /// Last ring the recognizer reported (None = none known / invalidated).
    pub previous_ring: Option<Ring>,
    /// Ring stroke ids the screen currently shows snapped to a circle.
    pub snapped_ring_ids: Vec<String>,
    /// Signature of the spell whose activation animation last played.
    pub last_activation: String,
    next_id: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Session { strokes: Vec::new(), redo: Vec::new(), previous_ring: None, snapped_ring_ids: Vec::new(), last_activation: String::new(), next_id: 1 }
    }

    /// Pen-up: keep the stroke, drop the redo history. Returns the new id.
    pub fn commit(&mut self, points: Vec<(f64, f64)>) -> String {
        let id = format!("s{}", self.next_id);
        self.next_id += 1;
        self.strokes.push(ScreenStroke { id: id.clone(), points });
        self.redo.clear();
        id
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.strokes.pop() else { return false };
        self.redo.push(s);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        self.strokes.push(s);
        true
    }

    /// CLEAR is undoable through redo taps.
    pub fn clear(&mut self) {
        self.redo.extend(self.strokes.drain(..));
    }

    /// Eraser: delete the first stroke with a point within ERASE_RADIUS of (x, y).
    pub fn erase_at(&mut self, x: f64, y: f64) -> bool {
        let Some(idx) = self.strokes.iter().position(|s| s.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < ERASE_RADIUS)) else {
            return false;
        };
        let s = self.strokes.remove(idx);
        self.redo.push(s);
        true
    }

    /// Index of the first stroke a finger at (x, y) grabs.
    pub fn grab_at(&self, x: f64, y: f64) -> Option<usize> {
        self.strokes.iter().position(|s| s.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < GRAB_RADIUS))
    }

    pub fn move_stroke(&mut self, idx: usize, dx: f64, dy: f64) {
        for p in &mut self.strokes[idx].points {
            p.0 += dx;
            p.1 += dy;
        }
    }

    /// Spell Catalogue pick: the page is replaced (the first commit clears
    /// redo, so the old drawing is gone, as when drawing after X), the last
    /// ring is forgotten, then the spell's strokes ink one by one.
    pub fn pick_catalogue(&mut self, spell: Vec<Vec<(f64, f64)>>) {
        self.clear();
        self.previous_ring = None;
        self.snapped_ring_ids.clear();
        self.last_activation.clear(); // the spell plays every time
        for points in spell {
            self.commit(points);
        }
    }

    /// Complete-ring stroke ids that a full redraw would snap right now.
    pub fn snapped_ids(&self) -> Vec<String> {
        let ring = self.previous_ring.as_ref();
        ring.filter(|r| r.complete && r.stroke_ids.iter().all(|id| self.strokes.iter().any(|s| &s.id == id)))
            .map(|r| r.stroke_ids.clone())
            .unwrap_or_default()
    }

    /// The page changed outside the pen-up path (undo/redo/clear/erase/drag/
    /// resume): the repaint that follows snaps whatever `snapped_ids` says.
    pub fn resnap(&mut self) {
        self.snapped_ring_ids = self.snapped_ids();
    }

    /// A recognition result landed. True when the set of snapped ring strokes
    /// changed, i.e. the page must be redrawn.
    pub fn apply_ring(&mut self, ring: Option<Ring>) -> bool {
        self.previous_ring = ring;
        let ids = self.snapped_ids();
        let changed = ids != self.snapped_ring_ids;
        if changed {
            self.snapped_ring_ids = ids;
        }
        changed
    }
}
