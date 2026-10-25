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

    /// An empty page has cast nothing: the same spell drawn again must cast.
    fn forget_cast_if_empty(&mut self) {
        if self.strokes.is_empty() {
            self.last_activation.clear();
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.strokes.pop() else { return false };
        self.redo.push(s);
        self.forget_cast_if_empty();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        self.strokes.push(s);
        true
    }

    /// CLEAR is undoable through redo taps. Reversed, so redo pops the first
    /// stroke first and the page comes back in drawing order.
    pub fn clear(&mut self) {
        self.redo.extend(self.strokes.drain(..).rev());
        self.forget_cast_if_empty();
    }

    /// Eraser: delete the first stroke with a point within ERASE_RADIUS of (x, y).
    pub fn erase_at(&mut self, x: f64, y: f64) -> bool {
        let Some(idx) = self.strokes.iter().position(|s| s.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < ERASE_RADIUS)) else {
            return false;
        };
        let s = self.strokes.remove(idx);
        self.redo.push(s);
        self.forget_cast_if_empty();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(x: f64, y: f64) -> Vec<(f64, f64)> {
        vec![(x, y), (x + 30.0, y)]
    }

    fn ids(v: &[ScreenStroke]) -> Vec<&str> {
        v.iter().map(|s| s.id.as_str()).collect()
    }

    fn ring_over(ids: &[&str]) -> Ring {
        Ring { found: true, complete: true, stroke_ids: ids.iter().map(|s| s.to_string()).collect(), ..Ring::default() }
    }

    // README Controls: "Pick a ready-made spell ... Undo removes it".
    #[test]
    fn catalogue_pick_is_one_undo_step() {
        let mut s = Session::new();
        s.pick_catalogue(vec![seg(200.0, 200.0), seg(200.0, 300.0), seg(200.0, 400.0)]);
        assert_eq!(s.strokes.len(), 3);
        assert!(s.undo());
        assert!(s.strokes.is_empty(), "one Undo must remove the whole spell, left {:?}", ids(&s.strokes));
        assert!(s.redo());
        assert_eq!(ids(&s.strokes), ["s1", "s2", "s3"]);

        // A stroke drawn after the pick is its own undo step.
        s.commit(seg(200.0, 600.0));
        assert!(s.undo());
        assert_eq!(s.strokes.len(), 3);
        assert!(s.undo());
        assert!(s.strokes.is_empty());
    }

    // README: "Erasing and clearing can be undone too".
    #[test]
    fn clear_then_redo_restores_the_page_in_drawing_order() {
        let mut s = Session::new();
        for y in [200.0, 300.0, 400.0] {
            s.commit(seg(200.0, y));
        }
        s.clear();
        assert!(s.strokes.is_empty());
        while s.strokes.len() < 3 {
            assert!(s.redo());
        }
        assert_eq!(ids(&s.strokes), ["s1", "s2", "s3"], "redo after CLEAR restored the strokes reversed");
        assert!(s.undo());
        assert_eq!(ids(&s.strokes), ["s1", "s2"], "Undo must remove the last-drawn stroke");
    }

    // README: "Each cast is appended to log.jsonl": an empty page has cast
    // nothing, so drawing the same spell again is a new cast.
    #[test]
    fn emptying_the_page_forgets_the_last_activation() {
        for how in 0..3 {
            let mut s = Session::new();
            s.commit(seg(200.0, 200.0));
            s.last_activation = "sig".into();
            match how {
                0 => s.clear(),
                1 => {
                    s.undo();
                }
                _ => {
                    s.erase_at(210.0, 200.0);
                }
            }
            assert!(s.strokes.is_empty());
            assert!(s.last_activation.is_empty(), "case {how}: drawing the same spell again would not cast");
        }
        // Still something on the page: the spell is still the one that cast.
        let mut s = Session::new();
        s.commit(seg(200.0, 200.0));
        s.commit(seg(200.0, 300.0));
        s.last_activation = "sig".into();
        s.undo();
        assert_eq!(s.last_activation, "sig");
    }

    // README: "One-finger drag on a stroke | Move it". The finger keeps the
    // stroke it grabbed even if an undo/erase from a second finger shifts the list.
    #[test]
    fn a_grabbed_stroke_is_the_one_that_moves_after_the_page_changes() {
        let mut s = Session::new();
        for y in [200.0, 500.0, 800.0] {
            s.commit(seg(200.0, y));
        }
        let grab = s.grab_at(210.0, 500.0).unwrap();
        assert!(s.erase_at(210.0, 200.0));
        s.move_stroke(grab, 7.0, 0.0);
        let x_of = |s: &Session, id: &str| s.strokes.iter().find(|t| t.id == id).unwrap().points[0].0;
        assert_eq!(x_of(&s, "s2"), 207.0, "the grabbed stroke did not move");
        assert_eq!(x_of(&s, "s3"), 200.0, "a different stroke moved instead");
    }

    // README: "Close a ring | It snaps to a perfect circle" - and a dragged
    // ring stroke must be seen moving, not hidden behind the stale circle.
    #[test]
    fn dragging_a_ring_stroke_unsnaps_the_ring_but_other_strokes_do_not() {
        let mut s = Session::new();
        for y in [200.0, 300.0, 600.0] {
            s.commit(seg(200.0, y));
        }
        assert!(s.apply_ring(Some(ring_over(&["s1", "s2"]))));
        assert_eq!(s.snapped_ring_ids, ["s1", "s2"]);

        let sigil = s.grab_at(210.0, 600.0).unwrap();
        s.move_stroke(sigil, 5.0, 0.0);
        assert_eq!(s.snapped_ids(), ["s1", "s2"], "moving the sigil leaves the ring alone");

        let ring_stroke = s.grab_at(210.0, 200.0).unwrap();
        s.move_stroke(ring_stroke, 5.0, 0.0);
        assert!(s.snapped_ids().is_empty(), "the snapped circle would stay put while its stroke moves");
        assert!(s.snapped_ring_ids.is_empty());
    }
}
