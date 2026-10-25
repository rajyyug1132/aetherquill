//! 1-bit software framebuffer + drawing primitives, and the per-element spell
//! activation effect frames. Pure pixel math, no qtfb/device deps — so the
//! effects can be rendered and eyeballed on any host (see examples/effect_preview).

use crate::spell_view::{EffectParams, Overlay};

pub const W: i32 = 1404;
pub const H: i32 = 1872;

pub const WHITE: u16 = 0xFFFF;
pub const BLACK: u16 = 0x0000;
pub const GRAY: u16 = 0x8410; // mid-gray in RGB565

/// Frames per activation animation (excludes the final settle frame).
pub const EFFECT_FRAMES: i32 = 6;
/// How far past the seal radius the effect reaches.
pub const EFFECT_REACH: i32 = 120;

pub struct Fb<'a> {
    pub px: &'a mut [u16],
}

impl Fb<'_> {
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
/// effect into the annulus around a seal at (cx,cy) radius `radius`. Translated
/// from the upstream renderer's particle systems into 1-bit e-ink line art:
/// fire=rising flicker, water=ripples+droplets, wind=curling swirl,
/// earth=rising debris, light=radiant beams.
pub fn effect_frame(fb: &mut Fb, cx: i32, cy: i32, radius: i32, element: &str, f: i32, params: &EffectParams) {
    let tau = std::f64::consts::TAU;
    let reach = params.reach;
    let w = params.weight;
    let pad = radius + reach;
    fb.clear_seal_annulus(cx, cy, radius, pad);

    match element {
        // Fire: tongues rise and flicker above the seal (upstream: particles
        // lift from the ring plane, flicker ∝ 1-stability). Weighted to the top.
        "fire" => {
            let grow = 40.0 + f as f64 * 12.0;
            for i in 0..18 {
                let a = tau * i as f64 / 18.0;
                let up = (-a.sin()).max(0.0); // upper hemisphere reaches higher
                let h = grow * (0.55 + up * 0.85);
                let flick = (noise(i as u32 + f as u32 * 18) - 0.5) * 0.28;
                let r0 = (radius + 26) as f64;
                let (x0, y0) = (cx + (a.cos() * r0) as i32, cy + (a.sin() * r0) as i32);
                let am = a + flick;
                let (xm, ym) = (cx + (am.cos() * (r0 + h * 0.5)) as i32, cy + (am.sin() * (r0 + h * 0.5)) as i32);
                let (xt, yt) = (cx + (a.cos() * (r0 + h)) as i32, cy + (a.sin() * (r0 + h)) as i32);
                fb.line(x0, y0, xm, ym, 4 + w, BLACK);
                fb.line(xm, ym, xt, yt, 2 + w, BLACK);
            }
        }
        // Water: concentric ripples expand outward, droplets arc and fall
        // (upstream: faucet-arc projectiles under gravity + suspended orb).
        "water" => {
            for k in 0..3 {
                let rr = radius + 20 + (f * 16 + k * 30) % reach;
                fb.circle(cx, cy, rr, 2 + w, BLACK);
            }
            for i in 0..6 {
                let a = tau * i as f64 / 6.0;
                let fall = (f as f64 * 14.0 + noise(i) * 20.0) % reach as f64;
                let dx = cx + (a.cos() * (radius + 30) as f64) as i32;
                let dy = cy + (a.sin() * (radius + 30) as f64) as i32 + fall as i32;
                fb.circle(dx, dy, 4, 2 + w, BLACK);
            }
        }
        // Wind: curling streaks orbit and rotate (upstream: velocity curl spins
        // each particle's short stroke). Spiral arms sweep around per frame.
        "wind" => {
            for arm in 0..3 {
                let base = tau * arm as f64 / 3.0 + f as f64 * 0.4;
                let mut prev = (
                    cx + (base.cos() * (radius + 20) as f64) as i32,
                    cy + (base.sin() * (radius + 20) as f64) as i32,
                );
                for s in 1..14 {
                    let t = s as f64 / 13.0;
                    let a = base + t * 1.5; // curl
                    let r = (radius + 20) as f64 + t * reach as f64 * 0.8;
                    let p = (cx + (a.cos() * r) as i32, cy + (a.sin() * r) as i32);
                    fb.line(prev.0, prev.1, p.0, p.1, 3 + w, BLACK);
                    prev = p;
                }
            }
        }
        // Earth: square debris chunks rise from the seal then settle (upstream:
        // rect particles). Blocks climb over frames, nested squares brace it.
        "earth" => {
            for (j, rot) in [0.0_f64, 0.4].iter().enumerate() {
                let r = (radius + 34 + j as i32 * 26) as f64;
                let pts: Vec<(i32, i32)> = (0..4)
                    .map(|k| {
                        let a = rot + tau * k as f64 / 4.0;
                        (cx + (a.cos() * r) as i32, cy + (a.sin() * r) as i32)
                    })
                    .collect();
                for k in 0..4 {
                    let (p, q) = (pts[k], pts[(k + 1) % 4]);
                    fb.line(p.0, p.1, q.0, q.1, 2 + w, BLACK);
                }
            }
            for i in 0..8 {
                let a = tau * i as f64 / 8.0;
                let rise = (f as f64 * 12.0 + noise(i) * 30.0) % (reach as f64 * 0.9);
                let r = (radius + 30) as f64 + rise;
                let (bx, by) = (cx + (a.cos() * r) as i32, cy + (a.sin() * r) as i32);
                let sz = 10 - (rise / reach as f64 * 6.0) as i32; // shrink as they climb
                fb.rect_outline(bx - sz / 2, by - sz / 2, sz.max(4), sz.max(4), 2, BLACK);
            }
        }
        // Light (and anything else): radiant beams pulse outward, thin and long
        // (upstream: bright projectile trails). Beams extend then contract.
        _ => {
            let pulse = ((f as f64 / 5.0 * std::f64::consts::PI).sin() * reach as f64 * 0.9) as i32;
            for i in 0..24 {
                let a = tau * i as f64 / 24.0;
                let r0 = (radius + 22) as f64;
                let r1 = (radius + 22 + pulse.max(20) + if i % 2 == 0 { 24 } else { 0 }) as f64;
                fb.line(
                    cx + (a.cos() * r0) as i32, cy + (a.sin() * r0) as i32,
                    cx + (a.cos() * r1) as i32, cy + (a.sin() * r1) as i32,
                    (if i % 2 == 0 { 3 } else { 1 }) + w,
                    BLACK,
                );
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
                fb.line(start.0 as i32, start.1 as i32, end.0 as i32, end.1 as i32, 3 + w, BLACK);
            }
            let rungs = (len / 28.0) as i32;
            for k in 1..=rungs {
                let t = r0 + k as f64 * 28.0;
                let (mx, my) = (cx as f64 + dx * t, cy as f64 + dy * t);
                fb.line((mx - nx * half) as i32, (my - ny * half) as i32, (mx + nx * half) as i32, (my + ny * half) as i32, 1 + w, BLACK);
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
                fb.line(x - half as i32, y + 12, x, y, 2 + w, BLACK);
                fb.line(x, y, x + half as i32, y + 12, 2 + w, BLACK);
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
                fb.line(outer.0 as i32, outer.1 as i32, end.0 as i32, end.1 as i32, 2 + w, BLACK);
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
    let label = match params.overlay {
        Overlay::Aura => "",
        Overlay::Column { .. } => "column",
        Overlay::Levitation { .. } => "levitation",
        Overlay::Convergence { .. } => "convergence",
    };
    if !label.is_empty() {
        fb.text(cx - label.len() as i32 * 3 * 8 / 2, cy + radius + 96, label, 3, BLACK);
    }
}
