//! Host preview of the screen chrome: sidebar, status strip and the paged
//! Spell Catalogue panel, drawn by the same code the tablet runs.
//! Run: `cargo run --release --example ui_preview` -> ui_preview_*.ppm here.
//!   canvas : first catalogue spell inked, status line from the real pipeline
//!   p1/p2  : catalogue panel pages (padded to 12 dummy names when the real
//!            catalogue is short, so paging is always exercised)
//!   short  : the catalogue exactly as shipped, eraser selected
//! Unverified on hardware: this is the framebuffer, not what e-ink shows.

use device::catalogue::{self, CatalogueSpell, Expected, CANVAS_SCALE};
use device::chrome;
use device::render::{Fb, BLACK, H, W, WHITE};
use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
use recognizer::dictionaries::load_dictionary;
use recognizer::drawing_classifier::classify_drawing;
use recognizer::geometry::Point;
use recognizer::spell_builder::compile_spell;
use recognizer::stroke_cleaner::RawStroke;

fn write_ppm(path: &str, px: &[u16]) {
    let mut ppm = format!("P6\n{W} {H}\n255\n").into_bytes();
    for &v in px {
        // Same 1-bit-ish view as effect_preview, but keep mid-gray visible.
        let g = match v {
            0xFFFF => 255u8,
            0x0000 => 0,
            _ => 128,
        };
        ppm.extend_from_slice(&[g, g, g]);
    }
    std::fs::write(path, ppm).unwrap();
    println!("wrote {path} ({W}x{H})");
}

fn blank(erase_mode: bool, line1: &str, line2: &str) -> Vec<u16> {
    let mut px = vec![WHITE; (W * H) as usize];
    let mut fb = Fb { px: &mut px };
    chrome::draw_chrome(&mut fb, erase_mode);
    chrome::draw_status(&mut fb, line1, line2);
    px
}

fn dummy_spells(n: usize) -> Vec<CatalogueSpell> {
    const NAMES: [&str; 12] = [
        "Fire Shoot", "Water Orb", "Converging Wind", "Light Beam", "Rising Earth", "Spark Burst",
        "Tide Veil", "Gale Ring", "Stone Wall", "Sun Lance", "Mist Step", "Ember Bloom",
    ];
    (0..n)
        .map(|i| CatalogueSpell {
            id: format!("dummy-{i}"),
            name: NAMES[i % NAMES.len()].to_string(),
            line: format!("Dummy spell {} with a deliberately long description to test how the row copes", i + 1),
            strokes: vec![],
            expected: Expected { status: String::new(), element: String::new(), primary_manifestation: String::new(), signs: vec![], signature: String::new() },
        })
        .collect()
}

fn main() {
    let real = catalogue::load();

    // canvas: inked spell + real status lines.
    let strokes = real[0].screen_strokes();
    let raw: Vec<RawStroke> = strokes
        .iter()
        .enumerate()
        .map(|(i, s)| RawStroke { id: format!("s{}", i + 1), points: s.iter().map(|&(x, y)| Point { x: x * CANVAS_SCALE, y: y * CANVAS_SCALE }).collect() })
        .collect();
    let result = classify_drawing(&raw, None, &load_dictionary(), "preview", &INPUT, &RING, &LAYERS, &RECOGNITION);
    let ir = compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE);
    let (l1, l2) = chrome::spell_status(&ir);
    println!("status: {l1} / {l2}");
    let mut px = blank(false, &l1, &l2);
    {
        let mut fb = Fb { px: &mut px };
        for s in &strokes {
            for w in s.windows(2) {
                fb.line(w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, 3, BLACK);
            }
        }
    }
    write_ppm("ui_preview_canvas.ppm", &px);

    // Paging previews.
    let paged = if real.len() > catalogue::ROWS_PER_PAGE { real } else { dummy_spells(12) };
    let pages = catalogue::page_count(paged.len());
    println!("catalogue: {} spells, {} per page, {pages} pages", paged.len(), catalogue::ROWS_PER_PAGE);
    for page in 0..pages.min(2) {
        let mut px = blank(false, "Draw a spell ring to begin", "");
        catalogue::draw_panel(&mut Fb { px: &mut px }, &paged, page);
        write_ppm(&format!("ui_preview_p{}.ppm", page + 1), &px);
    }

    // As shipped, short enough for one page; eraser selected, a long status.
    let short = catalogue::load();
    let mut px = blank(true, "Prepared: Water  quality 100%  stability 100%", &"levitation 100%  force 40 spread 50 range 42 grav 0  |  ring is slightly open near the top".to_string());
    catalogue::draw_panel(&mut Fb { px: &mut px }, &short, 0);
    write_ppm("ui_preview_short.ppm", &px);
}
