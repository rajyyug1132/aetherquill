//! 1-bit software framebuffer + drawing primitives, and the per-element spell
//! activation effect frames. Pure pixel math, no qtfb/device deps — so the
//! effects can be rendered and eyeballed on any host (see examples/effect_preview).

use crate::spell_view::{EffectParams, Overlay};

pub const W: i32 = 1404;
pub const H: i32 = 1872;

pub const WHITE: u16 = 0xFFFF;
pub const BLACK: u16 = 0x0000;
pub const GRAY: u16 = 0x8410; // mid-gray in RGB565

/// Frames per activation animation (excludes the final settle frame). 10 x
/// 80ms ~ 0.85s with the settle; was 6 (~0.5s) — lengthened on request.
pub const EFFECT_FRAMES: i32 = 10;
/// How far past the seal radius the effect reaches.
pub const EFFECT_REACH: i32 = 120;

pub struct Fb<'a> {
    pub px: &'a mut [u16],
}

impl Fb<'_> {
    /// Solid disk.
    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, c: u16) {
        for dy in -r..=r {
            let dx = ((r * r - dy * dy) as f64).sqrt() as i32;
            for x in cx - dx..=cx + dx {
                self.set(x, cy + dy, c);
            }
        }
    }

    /// Solid polygon (even-odd scanline fill).
    pub fn fill_polygon(&mut self, pts: &[(i32, i32)], c: u16) {
        if pts.len() < 3 {
            return;
        }
        let y0 = pts.iter().map(|p| p.1).min().unwrap().max(0);
        let y1 = pts.iter().map(|p| p.1).max().unwrap().min(H - 1);
        let mut xs: Vec<f64> = Vec::new();
        for y in y0..=y1 {
            xs.clear();
            let yc = y as f64 + 0.5;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                let (ya, yb) = (a.1 as f64, b.1 as f64);
                if (ya <= yc && yb > yc) || (yb <= yc && ya > yc) {
                    xs.push(a.0 as f64 + (yc - ya) / (yb - ya) * (b.0 - a.0) as f64);
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for pair in xs.chunks_exact(2) {
                for x in (pair[0].round() as i32).max(0)..(pair[1].round() as i32).min(W) {
                    self.set(x, y, c);
                }
            }
        }
    }

    pub fn set(&mut self, x: i32, y: i32, c: u16) {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            self.px[(y * W + x) as usize] = c;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        for yy in y..(y + h) {
            for xx in x..(x + w) {
                self.set(xx, yy, c);
            }
        }
    }

    pub fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, t: i32, c: u16) {
        self.fill_rect(x, y, w, t, c);
        self.fill_rect(x, y + h - t, w, t, c);
        self.fill_rect(x, y, t, h, c);
        self.fill_rect(x + w - t, y, t, h, c);
    }

    /// Thick line as stamped disks along the segment.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, thickness: i32, c: u16) {
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

    /// Arc from a0 to a1 radians (screen-space clockwise-positive).
    pub fn arc(&mut self, cx: i32, cy: i32, radius: i32, a0: f64, a1: f64, thickness: i32, c: u16) {
        let steps = (radius.max(8) * 8) as usize;
        for i in 0..=steps {
            let a = a0 + (a1 - a0) * i as f64 / steps as f64;
            for t in 0..thickness {
                let r = (radius + t) as f64;
                self.set(cx + (a.cos() * r) as i32, cy + (a.sin() * r) as i32, c);
            }
        }
    }

    /// Circle outline (midpoint-ish via angle stepping — plenty for an overlay).
    pub fn circle(&mut self, cx: i32, cy: i32, radius: i32, thickness: i32, c: u16) {
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
    pub fn text(&mut self, x: i32, y: i32, s: &str, scale: i32, c: u16) -> i32 {
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

    /// Whiten the effect annulus (bounding box minus the inner disk that holds
    /// the seal + sigil), then restamp the bold seal ring.
    fn clear_seal_annulus(&mut self, cx: i32, cy: i32, radius: i32, pad: i32) {
        let inner = radius + 4;
        let (x0, y0) = ((cx - pad).max(0), (cy - pad).max(0));
        let (x1, y1) = ((cx + pad).min(W), (cy + pad).min(H));
        for yy in y0..y1 {
            for xx in x0..x1 {
                let d2 = (xx - cx) * (xx - cx) + (yy - cy) * (yy - cy);
                if d2 > inner * inner {
                    self.set(xx, yy, WHITE);
                }
            }
        }
        self.circle(cx, cy, radius + 8, 4, BLACK);
    }
}

/// Deterministic hash → [0,1). Stands in for the upstream Math.random() jitter
/// without an rng dependency; stable per seed so a frame redraws identically.
pub fn noise(seed: u32) -> f64 {
    let n = seed.wrapping_mul(2654435761);
    ((n ^ (n >> 15)) & 0xffffff) as f64 / 0x1000000 as f64
}

/// Draw one animation frame `f` (0..EFFECT_FRAMES) of the element's activation
/// effect into the annulus around a seal at (cx,cy) radius `radius`.
///
/// Drawn for 1-bit fast-waveform e-ink: solid filled shapes and thick lines
/// (thin 1-2 px lines barely register in the fast mode and leave ghosts),
/// frame 0 an ignition band, motion from shapes growing and travelling.
/// Element character follows the upstream particle renderer: fire = rising
/// top-weighted flame tongues + embers, water = expanding rings + falling
/// droplets, wind = unfurling spiral ribbons + gusts, earth = cracks + rising
/// boulders, light = pulsing sunburst + sparkles. Everything stays inside the
/// seal's annulus (radius .. radius + reach) so each frame fully clears it.
pub fn effect_frame(fb: &mut Fb, cx: i32, cy: i32, radius: i32, element: &str, f: i32, params: &EffectParams) {
    let tau = std::f64::consts::TAU;
    let reach = params.reach as f64;
    let w = params.weight;
    let p = f as f64 / (EFFECT_FRAMES - 1) as f64; // progress 0..1
    fb.clear_seal_annulus(cx, cy, radius, radius + params.reach);
    let r0 = (radius + 14) as f64;
    let at = |a: f64, r: f64| (cx + (a.cos() * r) as i32, cy + (a.sin() * r) as i32);

    if f == 0 {
        // Ignition: a solid band hugging the seal.
        fb.circle(cx, cy, radius + 10, 22, BLACK);
        return;
    }

    match element {
        "fire" => {
            for i in 0..16u32 {
                let a = tau * i as f64 / 16.0 + 0.1;
                let up = (-a.sin()).max(0.0);
                let flick = noise(i * 7 + f as u32 * 31);
                let h = (reach * (0.3 + 0.55 * up) * (0.45 + 0.55 * p) * (0.8 + 0.4 * flick)).min(reach - 22.0);
                let half = 0.10 + 0.03 * flick;
                let sway = (noise(i * 13 + f as u32 * 17) - 0.5) * 0.22;
                let tongue = [at(a - half, r0), at(a - half * 0.75 + sway * 0.5, r0 + h * 0.45), at(a + sway, r0 + h), at(a + half * 0.75 + sway * 0.5, r0 + h * 0.45), at(a + half, r0)];
                fb.fill_polygon(&tongue, BLACK);
                if h > 36.0 {
                    // Hollow core so tongues read as flames, not blobs.
                    fb.fill_polygon(&[at(a - half * 0.35, r0 + 6.0), at(a + sway * 0.6, r0 + h * 0.55), at(a + half * 0.35, r0 + 6.0)], WHITE);
                }
            }
            for e in 0..10u32 {
                let a = -std::f64::consts::FRAC_PI_2 + (noise(e * 3) - 0.5) * 1.6;
                let (x, y) = at(a, r0 + reach * (0.35 + 0.45 * ((p + noise(e * 5)) % 1.0)));
                fb.fill_circle(x, y, 3 + (noise(e * 11) * 4.0) as i32 + w, BLACK);
            }
        }
        "water" => {
            for k in 0..3 {
                let u = (p * 1.3 + k as f64 / 3.0) % 1.0;
                fb.circle(cx, cy, (r0 + u * reach * 0.85) as i32, (9.0 * (1.0 - u)) as i32 + 2 + w, BLACK);
            }
            for i in 0..10u32 {
                let a = tau * i as f64 / 10.0 + 0.3;
                let (x, y) = at(a, r0 + reach * (0.12 + 0.45 * p));
                let y = y + (reach * 0.28 * p * p) as i32; // droplets fall as they fly
                fb.fill_circle(x, y, 6 + w, BLACK);
                fb.fill_polygon(&[(x - 4, y - 3), (x, y - 14), (x + 4, y - 3)], BLACK);
            }
        }
        "wind" => {
            for arm in 0..4 {
                let base = tau * arm as f64 / 4.0 + p * 2.2;
                let mut prev = at(base, r0);
                for st in 1..=16 {
                    let u = st as f64 / 16.0;
                    if u > 0.35 + 0.65 * p {
                        break; // ribbons unfurl
                    }
                    let pt = at(base + u * 1.9, r0 + u * reach * 0.85);
                    fb.line(prev.0, prev.1, pt.0, pt.1, ((1.0 - u) * 9.0) as i32 + 3 + w, BLACK);
                    prev = pt;
                }
            }
            for g in 0..6u32 {
                let a = tau * g as f64 / 6.0 - p * 1.5;
                fb.arc(cx, cy, (r0 + reach * (0.3 + 0.4 * noise(g * 9))) as i32, a, a + 0.35, 5 + w, BLACK);
            }
        }
        "earth" => {
            for c in 0..8u32 {
                let a = tau * c as f64 / 8.0 + 0.2;
                let len = reach * 0.75 * (0.3 + 0.7 * p);
                let mut prev = at(a, r0);
                for st in 1..=5u32 {
                    let pt = at(a + (noise(c * 17 + st) - 0.5) * 0.12, r0 + st as f64 / 5.0 * len);
                    fb.line(prev.0, prev.1, pt.0, pt.1, 8 - st as i32 + w, BLACK);
                    prev = pt;
                }
            }
            for i in 0..12u32 {
                let a = tau * i as f64 / 12.0;
                let rise = reach * 0.75 * ((p * 0.9 + noise(i * 7) * 0.4) % 1.0);
                let (bx, by) = at(a, r0 + 10.0 + rise);
                let size = (24.0 - rise / reach * 10.0) * 0.7;
                let rot = noise(i * 3) * tau + p * 2.0;
                let corners: Vec<(i32, i32)> = (0..4).map(|k| { let q = rot + tau * k as f64 / 4.0; (bx + (q.cos() * size) as i32, by + (q.sin() * size) as i32) }).collect();
                fb.fill_polygon(&corners, BLACK);
            }
        }
        _ => {
            let pulse = (p * std::f64::consts::PI).sin();
            for i in 0..20 {
                let a = tau * i as f64 / 20.0;
                let long = i % 2 == 0;
                let len = reach * if long { 0.85 } else { 0.5 } * (0.35 + 0.65 * pulse);
                let half = if long { 0.045 } else { 0.03 };
                fb.fill_polygon(&[at(a - half, r0), at(a, r0 + len), at(a + half, r0)], BLACK);
            }
            for s in 0..8 {
                let (x, y) = at(tau * (s as f64 + 0.5) / 8.0 + p, r0 + reach * 0.6 * pulse.max(0.3));
                let d = 8 + w;
                fb.fill_polygon(&[(x, y - d), (x + d / 2, y), (x, y + d), (x - d / 2, y)], BLACK);
            }
        }
    }
    draw_overlay(fb, cx, cy, radius, f, params);
}

/// Sign manifestation drawn over the element effect, inside the same annulus
/// (never inside the seal, so the sigil survives). Grows over the frames.
fn draw_overlay(fb: &mut Fb, cx: i32, cy: i32, radius: i32, f: i32, params: &EffectParams) {
    let grow = (f + 1) as f64 / EFFECT_FRAMES as f64;
    let reach = params.reach as f64;
    let r0 = (radius + 14) as f64;
    let w = params.weight;
    match params.overlay {
        Overlay::Aura => {}
        // Pillar: two rails + rungs leaving the seal along the spell direction.
        Overlay::Column { dir: (dx, dy) } => {
            let len = reach * (0.5 + 0.5 * params.strength) * grow;
            let (nx, ny) = (-dy, dx);
            let half = radius as f64 * (0.2 + 0.3 * params.spread);
            for side in [-1.0, 1.0] {
                let (ox, oy) = (nx * half * side, ny * half * side);
                let start = (cx as f64 + dx * r0 + ox, cy as f64 + dy * r0 + oy);
                let end = (start.0 + dx * len, start.1 + dy * len);
                fb.line(start.0 as i32, start.1 as i32, end.0 as i32, end.1 as i32, 7 + w, BLACK);
            }
            let rungs = (len / 28.0) as i32;
            for k in 1..=rungs {
                let t = r0 + k as f64 * 28.0;
                let (mx, my) = (cx as f64 + dx * t, cy as f64 + dy * t);
                fb.line((mx - nx * half) as i32, (my - ny * half) as i32, (mx + nx * half) as i32, (my + ny * half) as i32, 4 + w, BLACK);
            }
        }
        // Chevrons stacked above the seal; the stack climbs further the weaker
        // upstream gravity is.
        Overlay::Levitation { lift } => {
            let count = 2 + (params.strength * 3.0).round() as i32;
            let climb = reach * lift * grow;
            for k in 0..count {
                let y = cy as f64 - r0 - 10.0 - k as f64 * 22.0 - climb;
                if y < (cy - radius) as f64 - reach {
                    break;
                }
                let half = 16.0 + 24.0 * params.spread - k as f64 * 3.0;
                let (x, y) = (cx, y as i32);
                fb.line(x - half as i32, y + 12, x, y, 5 + w, BLACK);
                fb.line(x, y, x + half as i32, y + 12, 5 + w, BLACK);
            }
        }
        // Spokes from the outer edge, each aimed at the convergence point and
        // stopped at the seal; tighter rigidity = more spokes.
        Overlay::Convergence { point: (px, py), rigidity } => {
            let target = (cx as f64 + px * radius as f64, cy as f64 + py * radius as f64);
            let spokes = 6 + (rigidity * 10.0).round() as i32;
            // Spokes fan over an arc around the side facing the point; full
            // circle at spread 1.
            let facing = py.atan2(px);
            let arc = std::f64::consts::TAU * (0.4 + 0.6 * params.spread);
            for i in 0..spokes {
                let a = facing - arc / 2.0 + arc * (i as f64 + 0.5) / spokes as f64;
                let outer = (cx as f64 + a.cos() * (r0 + reach * 0.9), cy as f64 + a.sin() * (r0 + reach * 0.9));
                let (vx, vy) = (target.0 - outer.0, target.1 - outer.1);
                let dist = vx.hypot(vy);
                // Travel toward the target but stop short of the seal.
                let travel = ((reach * 0.9) * grow).min(dist - radius as f64 * 0.2);
                if travel <= 0.0 {
                    continue;
                }
                let end = (outer.0 + vx / dist * travel, outer.1 + vy / dist * travel);
                let end_r = (end.0 - cx as f64).hypot(end.1 - cy as f64);
                let end = if end_r < r0 { (cx as f64 + (end.0 - cx as f64) / end_r * r0, cy as f64 + (end.1 - cy as f64) / end_r * r0) } else { end };
                fb.line(outer.0 as i32, outer.1 as i32, end.0 as i32, end.1 as i32, 5 + w, BLACK);
            }
        }
    }
}

/// Final settle frame: clear the effect, leave the bold seal + element name
/// (and the sign manifestation under it, when there is one).
pub fn effect_settle(fb: &mut Fb, cx: i32, cy: i32, radius: i32, element: &str, params: &EffectParams) {
    let pad = radius + params.reach;
    fb.clear_seal_annulus(cx, cy, radius, pad);
    fb.text(cx - element.len() as i32 * 3 * 8 / 2, cy + radius + 40, element, 6, BLACK);
    let label = &params.manifestation;
    if label != "aura" {
        fb.text(cx - label.len() as i32 * 3 * 8 / 2, cy + radius + 96, label, 3, BLACK);
    }
}
