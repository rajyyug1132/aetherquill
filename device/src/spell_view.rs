//! SpellIr -> what the tablet shows: the two status lines and the parameters
//! the activation effect is drawn with. Pure data, host-testable.
//!
//! The numbers are upstream's — every field read here is computed by the
//! ported compiler (parity-tested in recognizer/tests/signs_parity.rs). How
//! they turn into 1-bit e-ink line art is Aetherquill's own mapping: upstream's
//! particle renderer isn't vendored, so there's nothing to port it from.
//!   range        -> how far the effect reaches past the seal
//!   force        -> line weight
//!   spread       -> how wide the sign overlay fans out
//!   manifestation-> overlay: column = pillar along `direction`,
//!                   levitation = rising chevrons lifted by (1 - gravity),
//!                   convergence = spokes aimed at the convergence point,
//!                   aura (no signs) = element effect alone

use recognizer::spell_builder::SpellIr;

use crate::render::EFFECT_REACH;

/// Upper bound on `EffectParams::reach`, for sizing the refresh box.
pub const EFFECT_REACH_MAX: i32 = 190;
/// Status detail line: scale-2 font (16px/char) in the 1262px status strip.
pub const STATUS_DETAIL_MAX_CHARS: usize = 78;

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    Aura,
    /// Unit screen vector the pillar rises along.
    Column { dir: (f64, f64) },
    /// 0..1 lift from upstream gravity (1 - gravity), 0 = grounded.
    Levitation { lift: f64 },
    /// Convergence point relative to the seal, in seal radii (upstream's
    /// normalized point), plus profile rigidity 0..1.
    Convergence { point: (f64, f64), rigidity: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectParams {
    pub reach: i32,
    /// Extra line thickness, 0..=2.
    pub weight: i32,
    /// Primary manifestation strength 0..1 (1 for aura).
    pub strength: f64,
    /// Upstream spread 0..1: widens the overlay.
    pub spread: f64,
    /// Upstream primary manifestation id ("aura" when no signs).
    pub manifestation: String,
    pub overlay: Overlay,
}

impl EffectParams {
    /// Baseline: no signs, compiler defaults — draws exactly the pre-sign effect.
    pub fn aura() -> Self {
        EffectParams { reach: EFFECT_REACH, weight: 0, strength: 1.0, spread: 0.5, manifestation: "aura".into(), overlay: Overlay::Aura }
    }

    pub fn from_ir(ir: &SpellIr) -> Self {
        // range 0.42 (compiler base, no signs) -> 120 = the original EFFECT_REACH.
        let reach = (70.0 + ir.range * 120.0).round().clamp(70.0, EFFECT_REACH_MAX as f64) as i32;
        // force 0.34 (base) -> 0, 1.0 -> 2.
        let weight = (((ir.force - 0.34) / 0.66) * 2.0).round().clamp(0.0, 2.0) as i32;
        let primary = ir.manifestations.get(&ir.primary_manifestation);
        let strength = primary.map_or(1.0, |m| m.strength);
        let overlay = match ir.primary_manifestation.as_str() {
            "column" => {
                let (x, y) = (ir.direction.x, ir.direction.y);
                let len = x.hypot(y);
                Overlay::Column { dir: if len > 1e-6 { (x / len, y / len) } else { (0.0, -1.0) } }
            }
            "levitation" => Overlay::Levitation { lift: (1.0 - ir.gravity).clamp(0.0, 1.0) },
            "convergence" => match primary.and_then(|m| m.convergence) {
                Some(c) => Overlay::Convergence { point: (c.point.x, c.point.y), rigidity: c.rigidity },
                None => Overlay::Aura,
            },
            _ => Overlay::Aura,
        };
        EffectParams { reach, weight, strength, spread: ir.spread.clamp(0.0, 1.0), manifestation: ir.primary_manifestation.clone(), overlay }
    }
}

/// (headline, detail). Detail lists the sign-driven IR numbers when a sign
/// manifestation is active, then the first warning.
pub fn status_lines(ir: &SpellIr) -> (String, String) {
    let line1 = match &ir.element {
        Some(element) if ir.valid => format!("{} - {}  q{:.0}% s{:.0}%", ir.status, element, ir.quality * 100.0, ir.stability * 100.0),
        _ => ir.status.clone(),
    };
    let mut parts = Vec::new();
    if ir.valid && ir.primary_manifestation != "aura" && ir.primary_manifestation != "none" {
        let strength = ir.manifestations.get(&ir.primary_manifestation).map_or(0.0, |m| m.strength);
        parts.push(format!(
            "{} {:.0}%  force {:.0} spread {:.0} range {:.0} grav {:.0}",
            ir.primary_manifestation,
            strength * 100.0,
            ir.force * 100.0,
            ir.spread * 100.0,
            ir.range * 100.0,
            ir.gravity * 100.0
        ));
    }
    if let Some(w) = ir.warnings.first() {
        parts.push(w.as_str().to_string());
    }
    let mut line2 = parts.join("  |  ");
    if line2.chars().count() > STATUS_DETAIL_MAX_CHARS {
        line2 = line2.chars().take(STATUS_DETAIL_MAX_CHARS - 3).collect::<String>() + "...";
    }
    (line1, line2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
    use recognizer::dictionaries::load_dictionary;
    use recognizer::drawing_classifier::classify_drawing;
    use recognizer::geometry::Point;
    use recognizer::spell_builder::compile_spell;
    use recognizer::stroke_cleaner::RawStroke;

    /// Runs a recognizer fixture scenario through the same pipeline main.rs uses.
    fn fixture_ir(file: &str, name: &str) -> SpellIr {
        let path = format!("{}/../recognizer/fixtures/{file}", env!("CARGO_MANIFEST_DIR"));
        let scenarios: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let scenario = scenarios.as_array().unwrap().iter().find(|s| s["name"] == name).expect(name);
        let strokes: Vec<RawStroke> = scenario["strokes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| RawStroke {
                id: s["id"].as_str().unwrap().into(),
                points: s["points"].as_array().unwrap().iter().map(|p| Point { x: p["x"].as_f64().unwrap(), y: p["y"].as_f64().unwrap() }).collect(),
            })
            .collect();
        let result = classify_drawing(&strokes, None, &load_dictionary(), "test", &INPUT, &RING, &LAYERS, &RECOGNITION);
        compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE)
    }

    #[test]
    fn plain_sigil_is_aura_with_baseline_effect() {
        let ir = fixture_ir("pipeline.json", "ring-fire");
        let p = EffectParams::from_ir(&ir);
        assert_eq!(p.overlay, Overlay::Aura);
        // Fire's own semantic range (0.44) sits near the compiler base -> near the original reach.
        assert!((p.reach - EFFECT_REACH).abs() <= 5, "range {} -> reach {}", ir.range, p.reach);
        assert_eq!(EffectParams::aura().reach, EFFECT_REACH);
        let (line1, line2) = status_lines(&ir);
        assert!(line1.starts_with("Active spell - fire"), "{line1}");
        assert!(!line2.contains("force"), "no sign numbers without a sign: {line2}");
    }

    #[test]
    fn column_sign_rises_along_spell_direction() {
        let ir = fixture_ir("signs.json", "ring-fire-column-placed");
        let p = EffectParams::from_ir(&ir);
        let Overlay::Column { dir } = p.overlay else { panic!("{:?}", p.overlay) };
        assert!(dir.1 < -0.9, "column sign below the seal pushes the spell up-screen: {dir:?}");
        let plain = EffectParams::from_ir(&fixture_ir("pipeline.json", "ring-fire"));
        assert!(p.reach > plain.reach + 20, "column range {} extends reach vs plain fire", ir.range);
        assert_eq!(p.weight, 2, "force {} is near max", ir.force);
        let (_, line2) = status_lines(&ir);
        assert!(line2.starts_with("column 60%  force 100"), "{line2}");
    }

    #[test]
    fn levitation_sign_lifts_by_inverse_gravity() {
        let ir = fixture_ir("signs.json", "ring-fire-levitation-placed");
        let p = EffectParams::from_ir(&ir);
        let Overlay::Levitation { lift } = p.overlay else { panic!("{:?}", p.overlay) };
        assert!((lift - (1.0 - ir.gravity)).abs() < 1e-12);
        assert!(lift > 0.2, "upstream gravity {} for this fixture", ir.gravity);
        let (_, line2) = status_lines(&ir);
        assert!(line2.starts_with("levitation 62%") && line2.contains("grav 74"), "{line2}");
    }

    #[test]
    fn convergence_sign_carries_its_profile() {
        let ir = fixture_ir("signs.json", "ring-fire-convergence-placed");
        let p = EffectParams::from_ir(&ir);
        let Overlay::Convergence { point, rigidity } = p.overlay else { panic!("{:?}", p.overlay) };
        let c = ir.manifestations["convergence"].convergence.unwrap();
        assert_eq!(point, (c.point.x, c.point.y));
        assert_eq!(rigidity, c.rigidity);
        assert!(point.1 > 0.3, "point sits toward the sign (below center): {point:?}");
        let (_, line2) = status_lines(&ir);
        assert!(line2.starts_with("convergence 68%"), "{line2}");
    }

    #[test]
    fn every_sign_overlay_draws_outside_the_seal_only() {
        use crate::render::{effect_frame, Fb, BLACK, EFFECT_FRAMES, H, W, WHITE};
        let (cx, cy, radius) = (W / 2, H / 2, 200);
        let render = |params: &EffectParams| {
            let mut px = vec![WHITE; (W * H) as usize];
            // A "sigil" inside the seal that every frame must leave alone.
            Fb { px: &mut px }.fill_rect(cx - 40, cy - 40, 80, 80, BLACK);
            effect_frame(&mut Fb { px: &mut px }, cx, cy, radius, "fire", EFFECT_FRAMES - 1, params);
            px
        };
        // Same reach/weight as the aura baseline so the diff isolates the overlay.
        let aura = EffectParams { overlay: Overlay::Aura, ..EffectParams::from_ir(&fixture_ir("signs.json", "ring-fire-column-placed")) };
        let base = render(&aura);
        for name in ["ring-fire-column-placed", "ring-fire-levitation-placed", "ring-fire-convergence-placed"] {
            let params = EffectParams::from_ir(&fixture_ir("signs.json", name));
            let px = render(&EffectParams { reach: aura.reach, weight: aura.weight, ..params });
            let mut added = 0;
            for y in 0..H {
                for x in 0..W {
                    let i = (y * W + x) as usize;
                    let inside = (x - cx).pow(2) + (y - cy).pow(2) < (radius - 2).pow(2);
                    if inside {
                        assert_eq!(px[i], base[i], "{name}: overlay touched the seal interior at {x},{y}");
                    } else if px[i] == BLACK && base[i] != BLACK {
                        added += 1;
                    }
                }
            }
            assert!(added > 200, "{name}: overlay added only {added} px");
        }
    }

    #[test]
    fn detail_line_fits_the_status_strip() {
        let mut ir = fixture_ir("signs.json", "ring-fire-levitation-placed");
        ir.warnings.push(recognizer::glyph_warnings::GlyphWarning::UnsupportedMultipleSigils);
        ir.warnings.rotate_right(1);
        let (_, line2) = status_lines(&ir);
        assert!(line2.chars().count() <= STATUS_DETAIL_MAX_CHARS, "{} chars: {line2}", line2.chars().count());
        assert!(line2.ends_with("..."));
    }

    #[test]
    fn spread_comes_from_the_ir() {
        let ir = fixture_ir("signs.json", "ring-fire-convergence-placed");
        assert_eq!(EffectParams::from_ir(&ir).spread, ir.spread);
    }

    #[test]
    fn invalid_spell_shows_status_and_warning_only() {
        let ir = fixture_ir("pipeline.json", "ring-only");
        let (line1, line2) = status_lines(&ir);
        assert_eq!(line1, "Invalid spell");
        assert_eq!(line2, "missing_primary_sigil");
        assert_eq!(EffectParams::from_ir(&ir).overlay, Overlay::Aura);
    }
}
