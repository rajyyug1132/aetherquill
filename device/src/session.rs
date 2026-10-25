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
    /// Strokes inked together (a catalogue pick) undo and redo as one step.
    pub group: Option<u64>,
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
    next_group: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Session { strokes: Vec::new(), redo: Vec::new(), previous_ring: None, snapped_ring_ids: Vec::new(), last_activation: String::new(), next_id: 1, next_group: 1 }
    }

    /// Pen-up: keep the stroke, drop the redo history. Returns the new id.
    pub fn commit(&mut self, points: Vec<(f64, f64)>) -> String {
        self.commit_in(points, None)
    }

    fn commit_in(&mut self, points: Vec<(f64, f64)>, group: Option<u64>) -> String {
        let id = format!("s{}", self.next_id);
        self.next_id += 1;
        self.strokes.push(ScreenStroke { id: id.clone(), points, group });
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
        let group = s.group;
        self.redo.push(s);
        while group.is_some() && self.strokes.last().is_some_and(|t| t.group == group) {
            self.redo.extend(self.strokes.pop());
        }
        self.forget_cast_if_empty();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        let group = s.group;
        self.strokes.push(s);
        while group.is_some() && self.redo.last().is_some_and(|t| t.group == group) {
            self.strokes.extend(self.redo.pop());
        }
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

    /// Id of the first stroke a finger at (x, y) grabs. An id, not an index:
    /// a second finger's undo/erase shifts the list mid-drag.
    pub fn grab_at(&self, x: f64, y: f64) -> Option<String> {
        let hit = self.strokes.iter().find(|s| s.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < GRAB_RADIUS));
        hit.map(|s| s.id.clone())
    }

    /// Moves a stroke by id; false when it is gone (undone/erased since the grab).
    /// A snapped ring member leaves the ring, so the drag shows the raw stroke
    /// moving instead of hiding behind the stale circle until the drop.
    pub fn move_stroke(&mut self, id: &str, dx: f64, dy: f64) -> bool {
        let in_snapped_ring = self.snapped_ids().iter().any(|r| r == id);
        let Some(stroke) = self.strokes.iter_mut().find(|s| s.id == id) else { return false };
        for p in &mut stroke.points {
            p.0 += dx;
            p.1 += dy;
        }
        if in_snapped_ring {
            self.previous_ring = None;
            self.snapped_ring_ids.clear();
        }
        true
    }

    /// Spell Catalogue pick: the page is replaced (the first commit clears
    /// redo, so the old drawing is gone, as when drawing after X), the last
    /// ring is forgotten, then the spell's strokes ink one by one.
    pub fn pick_catalogue(&mut self, spell: Vec<Vec<(f64, f64)>>) {
        self.clear();
        self.previous_ring = None;
        self.snapped_ring_ids.clear();
        self.last_activation.clear(); // the spell plays every time
        let group = Some(self.next_group);
        self.next_group += 1;
        for points in spell {
            self.commit_in(points, group);
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
        assert!(s.move_stroke(&grab, 7.0, 0.0));
        let x_of = |s: &Session, id: &str| s.strokes.iter().find(|t| t.id == id).unwrap().points[0].0;
        assert_eq!(x_of(&s, "s2"), 207.0, "the grabbed stroke did not move");
        assert_eq!(x_of(&s, "s3"), 200.0, "a different stroke moved instead");

        // Grabbed stroke undone away mid-drag: the move is a no-op, no panic.
        assert!(s.undo() && s.undo());
        assert!(!s.move_stroke(&grab, 1.0, 0.0));
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
        assert!(s.move_stroke(&sigil, 5.0, 0.0));
        assert_eq!(s.snapped_ids(), ["s1", "s2"], "moving the sigil leaves the ring alone");

        let ring_stroke = s.grab_at(210.0, 200.0).unwrap();
        assert!(s.move_stroke(&ring_stroke, 5.0, 0.0));
        assert!(s.snapped_ids().is_empty(), "the snapped circle would stay put while its stroke moves");
        assert!(s.snapped_ring_ids.is_empty());
    }

    // ---- Deterministic fuzz: README Controls, random action sequences ----

    /// xorshift64*: no dependency; the fixed seeds below make runs reproducible.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() >> 11) as usize % n.max(1)
        }
        fn coord(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
        }
    }

    type Sv = (String, Vec<(f64, f64)>, Option<u64>);
    fn sv(v: &[ScreenStroke]) -> Vec<Sv> {
        v.iter().map(|s| (s.id.clone(), s.points.clone(), s.group)).collect()
    }
    fn state(s: &Session) -> (Vec<Sv>, Vec<Sv>) {
        (sv(&s.strokes), sv(&s.redo))
    }
    fn sorted(mut v: Vec<Sv>) -> Vec<Sv> {
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    const FUZZ_SEEDS: [u64; 8] = [0x5eed_0001, 0xa11c_e5ed, 0x0bad_cafe, 0x1234_5678, 0xfeed_beef, 0xc0ff_ee01, 0x2468_ace0, 0x9e37_79b9];
    const SEQUENCES_PER_SEED: usize = 250;
    const ACTIONS_PER_SEQUENCE: usize = 80;

    /// A point on an existing stroke (70%) so erase/grab usually hit, else anywhere.
    fn target(rng: &mut Rng, s: &Session) -> (f64, f64) {
        if !s.strokes.is_empty() && rng.below(10) < 7 {
            let st = &s.strokes[rng.below(s.strokes.len())];
            let p = st.points[rng.below(st.points.len())];
            (p.0 + rng.coord(-10.0, 10.0), p.1 + rng.coord(-10.0, 10.0))
        } else {
            (rng.coord(120.0, 1400.0), rng.coord(70.0, 1870.0))
        }
    }

    fn random_stroke(rng: &mut Rng) -> Vec<(f64, f64)> {
        let (x, y) = (rng.coord(150.0, 1300.0), rng.coord(100.0, 1800.0));
        (0..2 + rng.below(6)).map(|i| (x + i as f64 * 9.0, y + rng.coord(-20.0, 20.0))).collect()
    }

    #[test]
    fn fuzz_random_sequences_keep_the_readme_controls_true() {
        let mut actions = 0usize;
        for seed in FUZZ_SEEDS {
            let mut rng = Rng(seed);
            for q in 0..SEQUENCES_PER_SEED {
                let mut s = Session::new();
                // id -> points of every stroke that should still exist (strokes + redo).
                let mut alive: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
                let mut moving: Option<(String, f64, f64)> = None;
                for step in 0..ACTIONS_PER_SEQUENCE {
                    actions += 1;
                    let act = rng.below(100);
                    let name = match act {
                        0..=24 => "commit",
                        25..=36 => "undo",
                        37..=48 => "redo",
                        49..=52 => "clear",
                        53..=60 => "erase",
                        61..=64 => "pick",
                        65..=70 => "grab",
                        71..=82 => "drag",
                        83..=85 => "release",
                        86..=88 => "resume",
                        89..=96 => "ring result",
                        _ => "cast",
                    };
                    let ctx = format!("seed {seed:#x} sequence {q} step {step} ({name})");
                    let before = state(&s);
                    let (n, r) = (s.strokes.len(), s.redo.len());
                    match name {
                        "commit" => {
                            let points = random_stroke(&mut rng);
                            alive.retain(|(id, _)| !s.redo.iter().any(|t| &t.id == id));
                            let id = s.commit(points.clone());
                            alive.push((id.clone(), points));
                            assert!(s.redo.is_empty(), "{ctx}: drawing must drop redo");
                            assert_eq!(s.strokes.last().unwrap().id, id, "{ctx}");
                        }
                        "undo" => {
                            let did = s.undo();
                            assert_eq!(did, n > 0, "{ctx}");
                            if did {
                                assert!(s.strokes.len() < n && s.redo.len() > r, "{ctx}");
                                assert_eq!(s.strokes.len() + s.redo.len(), n + r, "{ctx}");
                                if rng.below(2) == 0 {
                                    assert!(s.redo(), "{ctx}");
                                    assert!(state(&s) == before, "{ctx}: undo then redo must round-trip");
                                }
                            } else {
                                assert!(state(&s) == before, "{ctx}");
                            }
                        }
                        "redo" => {
                            let did = s.redo();
                            assert_eq!(did, r > 0, "{ctx}");
                            if did {
                                assert!(s.strokes.len() > n && s.redo.len() < r, "{ctx}");
                                assert_eq!(s.strokes.len() + s.redo.len(), n + r, "{ctx}");
                                if rng.below(2) == 0 {
                                    assert!(s.undo(), "{ctx}");
                                    assert!(state(&s) == before, "{ctx}: redo then undo must round-trip");
                                }
                            } else {
                                assert!(state(&s) == before, "{ctx}");
                            }
                        }
                        "clear" => {
                            s.last_activation = "sig".into(); // a cast, even if the page was empty
                            s.clear();
                            assert!(s.strokes.is_empty() && s.redo.len() == n + r, "{ctx}");
                            assert!(s.last_activation.is_empty(), "{ctx}: same spell must cast again after CLEAR");
                            if n > 0 && rng.below(2) == 0 {
                                // "Erasing and clearing can be undone": redo taps bring the page back as drawn.
                                for _ in 0..n {
                                    if s.strokes.len() == n {
                                        break;
                                    }
                                    assert!(s.redo(), "{ctx}");
                                }
                                assert!(state(&s) == before, "{ctx}: redo after CLEAR must restore the page exactly");
                            }
                        }
                        "erase" => {
                            let (x, y) = target(&mut rng, &s);
                            let expect = s.strokes.iter().find(|t| t.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < ERASE_RADIUS)).map(|t| t.id.clone());
                            let did = s.erase_at(x, y);
                            assert_eq!(did, expect.is_some(), "{ctx}");
                            if let Some(id) = expect {
                                assert!(s.strokes.len() == n - 1 && s.redo.last().unwrap().id == id, "{ctx}");
                                if rng.below(2) == 0 {
                                    assert!(s.redo(), "{ctx}");
                                    assert!(sorted(sv(&s.strokes)) == sorted(before.0.clone()) && sv(&s.redo) == before.1, "{ctx}: erase then redo must restore the stroke");
                                }
                            } else {
                                assert!(state(&s) == before, "{ctx}");
                            }
                        }
                        "pick" => {
                            let spell: Vec<Vec<(f64, f64)>> = (0..2 + rng.below(4)).map(|_| random_stroke(&mut rng)).collect();
                            s.last_activation = "sig".into();
                            s.pick_catalogue(spell.clone());
                            alive = s.strokes.iter().map(|t| (t.id.clone(), t.points.clone())).collect();
                            assert_eq!(s.strokes.len(), spell.len(), "{ctx}");
                            assert!(s.redo.is_empty() && s.previous_ring.is_none() && s.snapped_ring_ids.is_empty(), "{ctx}");
                            assert!(s.last_activation.is_empty(), "{ctx}: a pick plays every time");
                            let picked = state(&s);
                            assert!(s.undo() && s.strokes.is_empty(), "{ctx}: Undo removes the picked spell");
                            assert!(s.redo() && state(&s) == picked, "{ctx}: Redo brings the whole spell back");
                        }
                        "grab" => {
                            let (x, y) = target(&mut rng, &s);
                            let expect = s.strokes.iter().find(|t| t.points.iter().any(|p| (p.0 - x).hypot(p.1 - y) < GRAB_RADIUS)).map(|t| t.id.clone());
                            let got = s.grab_at(x, y);
                            assert_eq!(got, expect, "{ctx}");
                            if let Some(id) = got {
                                moving = Some((id, x, y));
                            }
                        }
                        "drag" => {
                            // The grabbed stroke may have been undone/erased/cleared/replaced since.
                            if let Some((id, lx, ly)) = moving.clone() {
                                let (x, y) = (lx + rng.coord(-30.0, 30.0), ly + rng.coord(-30.0, 30.0));
                                let (dx, dy) = (x - lx, y - ly);
                                let on_page = s.strokes.iter().any(|t| t.id == id);
                                let was_snapped = s.snapped_ids().contains(&id);
                                assert_eq!(s.move_stroke(&id, dx, dy), on_page, "{ctx}");
                                if on_page {
                                    for (aid, pts) in alive.iter_mut().filter(|(aid, _)| *aid == id) {
                                        let _ = aid;
                                        for p in pts.iter_mut() {
                                            p.0 += dx;
                                            p.1 += dy;
                                        }
                                    }
                                    if was_snapped {
                                        assert!(s.snapped_ids().is_empty() && s.snapped_ring_ids.is_empty(), "{ctx}: dragged ring stroke left snapped");
                                    }
                                }
                                moving = Some((id, x, y));
                            }
                        }
                        "release" => {
                            // main.rs on finger-up: drop the ring, repaint, re-recognize.
                            moving = None;
                            s.previous_ring = None;
                            s.resnap();
                            assert!(s.snapped_ring_ids.is_empty(), "{ctx}");
                        }
                        "resume" => {
                            s.resnap();
                            assert_eq!(s.snapped_ring_ids, s.snapped_ids(), "{ctx}");
                            assert!(state(&s) == before, "{ctx}: resume redraw must not change the page");
                        }
                        "ring result" => {
                            let mut members: Vec<&str> = s.strokes.iter().filter(|_| rng.below(3) == 0).map(|t| t.id.as_str()).collect();
                            if rng.below(8) == 0 {
                                members.push("s-gone"); // result for a stroke since removed
                            }
                            let ring = (!members.is_empty() && rng.below(6) != 0).then(|| Ring { complete: rng.below(4) != 0, ..ring_over(&members) });
                            let all_present = ring.as_ref().is_some_and(|rg| rg.complete && rg.stroke_ids.iter().all(|id| s.strokes.iter().any(|t| &t.id == id)));
                            let ring_ids = ring.as_ref().map(|rg| rg.stroke_ids.clone()).unwrap_or_default();
                            let was = s.snapped_ring_ids.clone();
                            let changed = s.apply_ring(ring);
                            let now = s.snapped_ids();
                            assert_eq!(changed, now != was, "{ctx}");
                            assert_eq!(s.snapped_ring_ids, now, "{ctx}");
                            if all_present {
                                assert_eq!(now, ring_ids, "{ctx}: a closed ring snaps");
                            } else {
                                assert!(now.is_empty(), "{ctx}");
                            }
                        }
                        _ => {
                            if !s.strokes.is_empty() {
                                s.last_activation = format!("sig{}", rng.below(3));
                            }
                        }
                    }

                    // Invariants after every action.
                    let mut seen: Vec<&str> = s.strokes.iter().chain(&s.redo).map(|t| t.id.as_str()).collect();
                    let total = seen.len();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(seen.len(), total, "{ctx}: duplicate stroke id");
                    let mut have: Vec<(String, Vec<(f64, f64)>)> = s.strokes.iter().chain(&s.redo).map(|t| (t.id.clone(), t.points.clone())).collect();
                    have.sort_by(|a, b| a.0.cmp(&b.0));
                    alive.sort_by(|a, b| a.0.cmp(&b.0));
                    assert!(have == alive, "{ctx}: a stroke was lost, duplicated or altered (page+redo has {} strokes, expected {})", have.len(), alive.len());
                    assert!(s.snapped_ids().iter().all(|id| s.strokes.iter().any(|t| &t.id == id)), "{ctx}: snapped a stroke that is gone");
                    assert!(!s.strokes.is_empty() || s.last_activation.is_empty(), "{ctx}: empty page remembers a cast");
                }
            }
        }
        println!("fuzz: {} seeds x {} sequences x {} actions = {actions} actions, no violations", FUZZ_SEEDS.len(), SEQUENCES_PER_SEED, ACTIONS_PER_SEQUENCE);
    }
}
