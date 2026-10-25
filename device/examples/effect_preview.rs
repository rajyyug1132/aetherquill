//! Host preview of the spell activation effects. Renders a peak-frame of each
//! element (plus the settle frame) into a single PPM so the silhouettes can be
//! eyeballed without a device. Run: `cargo run --example effect_preview`.
//! Row 1: the five elements, no signs. Row 2: fire with each modifying sign
//! overlay (column, levitation, convergence) at typical upstream values.
//! Also writes catalogue_preview.ppm: each Spell Catalogue entry's strokes,
//! recognized + compiled by the real pipeline, at its effect's peak frame.

use device::render::{effect_frame, effect_settle, Fb, BLACK, EFFECT_FRAMES, EFFECT_REACH, H, W};
use device::catalogue::{self, CANVAS_SCALE};
use device::spell_view::{EffectParams, Overlay};
use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
use recognizer::dictionaries::load_dictionary;
use recognizer::drawing_classifier::classify_drawing;
use recognizer::geometry::Point;
use recognizer::spell_builder::compile_spell;
use recognizer::stroke_cleaner::RawStroke;

fn write_ppm(path: &str, px: &[u16], w: i32, h: i32) {
    let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
    for &v in px {
        let g = if v == 0 { 0u8 } else { 255u8 };
        ppm.extend_from_slice(&[g, g, g]);
    }
    std::fs::write(path, ppm).unwrap();
    println!("wrote {path} ({w}x{h})");
}

/// One tile per catalogue spell: strokes inked at screen scale, pipeline run
/// on them exactly as the device does, effect drawn with the compiled params.
fn catalogue_preview() {
    let dictionary = load_dictionary();
    let spells = catalogue::load();
    let (tile, region) = (460, 1380); // each tile min-pools a region x region screen area
    let out_w = tile * spells.len() as i32;
    let mut out = vec![0xffffu16; (out_w * tile) as usize];
    for (col, spell) in spells.iter().enumerate() {
        let mut px = vec![0xffffu16; (W * H) as usize];
        let mut fb = Fb { px: &mut px };
        let strokes = spell.screen_strokes();
        for s in &strokes {
            for w in s.windows(2) {
                fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, 3, BLACK);
            }
        }
        let raw: Vec<RawStroke> = strokes
            .iter()
            .enumerate()
            .map(|(i, s)| RawStroke { id: format!("s{}", i + 1), points: s.iter().map(|&(x, y)| Point { x: x * CANVAS_SCALE, y: y * CANVAS_SCALE }).collect() })
            .collect();
        let result = classify_drawing(&raw, None, &dictionary, "preview", &INPUT, &RING, &LAYERS, &RECOGNITION);
        let ir = compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE);
        let (cx, cy, r) = ((result.ring.center.x / CANVAS_SCALE) as i32, (result.ring.center.y / CANVAS_SCALE) as i32, (result.ring.radius / CANVAS_SCALE) as i32);
        let element = ir.element.clone().unwrap_or_default();
        effect_frame(&mut fb, cx, cy, r, &element, EFFECT_FRAMES * 2 / 3, &EffectParams::from_ir(&ir));
        println!("  {}: {} {} {}", spell.id, ir.status, element, ir.primary_manifestation);
        let step = region / tile;
        for ty in 0..tile {
            for tx in 0..tile {
                let (sx0, sy0) = (cx - region / 2 + tx * step, cy - region / 2 + ty * step);
                let black = (0..step).any(|dy| (0..step).any(|dx| {
                    let (sx, sy) = (sx0 + dx, sy0 + dy);
                    (0..W).contains(&sx) && (0..H).contains(&sy) && px[(sy * W + sx) as usize] == BLACK
                }));
                if black {
                    out[(ty * out_w + col as i32 * tile + tx) as usize] = BLACK;
                }
            }
        }
    }
    write_ppm("catalogue_preview.ppm", &out, out_w, tile);
}

fn main() {
    // One 480x480 tile per element, laid out in a row.
    let elements = ["fire", "water", "wind", "earth", "light"];
    let tile = 460;
    let cols = elements.len() as i32;
    let out_w = tile * cols;
    let out_h = tile * 2;
    let mut out = vec![0xffffu16; (out_w * out_h) as usize];

    for (col, el) in elements.iter().enumerate() {
        // Draw into a full-size scratch fb, then copy the region around the seal.
        let mut px = vec![0xffffu16; (W * H) as usize];
        let mut fb = Fb { px: &mut px };
        let (cx, cy, radius) = (W / 2, H / 2, 150);
        // Peak frame (~2/3 through) shows the effect near full extent.
        effect_frame(&mut fb, cx, cy, radius, el, EFFECT_FRAMES * 2 / 3, &EffectParams::aura());
        // Draw the sigil-ish marker + a name from settle onto the same buffer edge.
        let _ = el;

        // Copy a tile centered on the seal into the output row.
        let half = tile / 2;
        for ty in 0..tile {
            for tx in 0..tile {
                let sx = cx - half + tx;
                let sy = cy - half + ty;
                if (0..W).contains(&sx) && (0..H).contains(&sy) {
                    let v = px[(sy * W + sx) as usize];
                    out[(ty * out_w + col as i32 * tile + tx) as usize] = v;
                }
            }
        }
    }

    let signs = [
        Overlay::Column { dir: (0.0, -1.0) },
        Overlay::Levitation { lift: 0.26 },
        Overlay::Convergence { point: (0.0, 0.34), rigidity: 0.61 },
    ];
    for (col, overlay) in signs.into_iter().enumerate() {
        let mut px = vec![0xffffu16; (W * H) as usize];
        let mut fb = Fb { px: &mut px };
        let (cx, cy, radius) = (W / 2, H / 2, 150);
        let params = EffectParams { reach: EFFECT_REACH + 30, weight: 1, strength: 0.65, spread: 0.4, manifestation: String::new(), overlay };
        effect_frame(&mut fb, cx, cy, radius, "fire", EFFECT_FRAMES * 2 / 3, &params);
        let half = tile / 2;
        for ty in 0..tile {
            for tx in 0..tile {
                let (sx, sy) = (cx - half + tx, cy - half + ty);
                if (0..W).contains(&sx) && (0..H).contains(&sy) {
                    out[((ty + tile) * out_w + col as i32 * tile + tx) as usize] = px[(sy * W + sx) as usize];
                }
            }
        }
    }

    // Also render the settle frame of "fire" in the corner as a sanity check of text.
    {
        let mut px = vec![0xffffu16; (W * H) as usize];
        let mut fb = Fb { px: &mut px };
        effect_settle(&mut fb, W / 2, H / 2, 150, "fire", &EffectParams::aura());
        // (not composited; existence proves it compiles/runs)
        let _ = fb.text(0, 0, "", 1, BLACK);
    }

    write_ppm("effect_preview.ppm", &out, out_w, out_h);
    catalogue_preview();
}
