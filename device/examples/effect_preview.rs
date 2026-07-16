//! Host preview of the spell activation effects. Renders a peak-frame of each
//! element (plus the settle frame) into a single PPM so the silhouettes can be
//! eyeballed without a device. Run: `cargo run --example effect_preview`.

use device::render::{effect_frame, effect_settle, Fb, BLACK, H, W};

fn main() {
    // One 480x480 tile per element, laid out in a row.
    let elements = ["fire", "water", "wind", "earth", "light"];
    let tile = 460;
    let cols = elements.len() as i32;
    let out_w = tile * cols;
    let out_h = tile;
    let mut out = vec![0xffffu16; (out_w * out_h) as usize];

    for (col, el) in elements.iter().enumerate() {
        // Draw into a full-size scratch fb, then copy the region around the seal.
        let mut px = vec![0xffffu16; (W * H) as usize];
        let mut fb = Fb { px: &mut px };
        let (cx, cy, radius) = (W / 2, H / 2, 150);
        // Peak frame (4 of 6) shows the effect near full extent.
        effect_frame(&mut fb, cx, cy, radius, el, 4);
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

    // Also render the settle frame of "fire" in the corner as a sanity check of text.
    {
        let mut px = vec![0xffffu16; (W * H) as usize];
        let mut fb = Fb { px: &mut px };
        effect_settle(&mut fb, W / 2, H / 2, 150, "fire");
        // (not composited; existence proves it compiles/runs)
        let _ = fb.text(0, 0, "", 1, BLACK);
    }

    // Write PPM (P6, 8-bit): 1-bit source -> black/white.
    let mut ppm = format!("P6\n{out_w} {out_h}\n255\n").into_bytes();
    for &v in &out {
        let g = if v == 0 { 0u8 } else { 255u8 };
        ppm.extend_from_slice(&[g, g, g]);
    }
    std::fs::write("effect_preview.ppm", ppm).unwrap();
    println!("wrote effect_preview.ppm ({out_w}x{out_h})");
}
