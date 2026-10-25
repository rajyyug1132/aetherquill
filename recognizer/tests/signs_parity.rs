//! Sign parity: fixtures/signs.json holds one scenario per modifying sign
//! (column, levitation, convergence) placed where upstream actually
//! recognizes it. Checks the recognized sign ids and every sign-driven spellIR
//! field against the JS ground truth — spellIR is unrounded, so 1e-6.

use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
use recognizer::dictionaries::load_dictionary;
use recognizer::drawing_classifier::classify_drawing;
use recognizer::geometry::Point;
use recognizer::spell_builder::compile_spell;
use recognizer::stroke_cleaner::RawStroke;

const TOL: f64 = 1e-6;

fn close(name: &str, field: &str, got: f64, want: &serde_json::Value) {
    let want = want.as_f64().unwrap_or_else(|| panic!("{name}: {field} missing in fixture"));
    assert!((got - want).abs() < TOL, "{name}: {field} got {got}, want {want}");
}

#[test]
fn sign_spell_ir_matches_js_for_every_sign() {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/signs.json"))
        .expect("fixtures/signs.json — regenerate with: node service/parity-gen.mjs");
    let scenarios: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let dictionary = load_dictionary();

    let mut seen_signs = Vec::new();
    for scenario in scenarios.as_array().unwrap() {
        let name = scenario["name"].as_str().unwrap();
        let strokes: Vec<RawStroke> = scenario["strokes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| RawStroke {
                id: s["id"].as_str().unwrap().to_string(),
                points: s["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| Point { x: p["x"].as_f64().unwrap(), y: p["y"].as_f64().unwrap() })
                    .collect(),
            })
            .collect();

        let result = classify_drawing(&strokes, None, &dictionary, "0.1.0-test", &INPUT, &RING, &LAYERS, &RECOGNITION);
        let got_signs: Vec<&str> = result.glyph_ast.signs.iter().filter_map(|s| s.id.as_deref()).collect();
        let want_signs: Vec<&str> = scenario["glyphAST"]["signs"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(got_signs, want_signs, "{name}: recognized signs");
        assert!(!want_signs.is_empty(), "{name}: fixture must contain a recognized sign");
        seen_signs.extend(want_signs.iter().map(|s| s.to_string()));

        // glyphAST is rounded to 3 decimals by the JS — 2e-3.
        let ast = &result.glyph_ast;
        let want_ast = &scenario["glyphAST"];
        let rounded = |field: &str, got: f64, want: &serde_json::Value| {
            let want = want.as_f64().unwrap_or_else(|| panic!("{name}: {field} missing"));
            assert!((got - want).abs() < 2e-3, "{name}: glyphAST.{field} got {got}, want {want}");
        };
        let got_warnings: Vec<String> = ast.warnings.iter().map(|w| w.as_str().to_string()).collect();
        let want_warnings: Vec<String> = want_ast["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect();
        assert_eq!(got_warnings, want_warnings, "{name}: glyphAST.warnings");
        assert_eq!(ast.unknowns.len(), want_ast["unknowns"].as_array().unwrap().len(), "{name}: unknown count");
        rounded("ring.neatness", ast.ring.neatness, &want_ast["ring"]["neatness"]);
        rounded("globalMetrics.neatness", ast.global_metrics.neatness, &want_ast["globalMetrics"]["neatness"]);
        rounded("globalMetrics.radialSymmetry", ast.global_metrics.radial_symmetry, &want_ast["globalMetrics"]["radialSymmetry"]);
        rounded("globalMetrics.instability", ast.global_metrics.instability, &want_ast["globalMetrics"]["instability"]);
        let primary = ast.primary_sigil.as_ref().expect("primary sigil");
        rounded("primarySigil.neatness", primary.neatness, &want_ast["primarySigil"]["neatness"]);
        rounded("primarySigil.confidence", primary.confidence, &want_ast["primarySigil"]["confidence"]);
        for (got, want) in ast.signs.iter().zip(want_ast["signs"].as_array().unwrap()) {
            rounded("sign.neatness", got.neatness, &want["neatness"]);
            rounded("sign.confidence", got.confidence, &want["confidence"]);
            rounded("sign.radiusNorm", got.radius_norm, &want["radiusNorm"]);
            rounded("sign.angleDeg", got.angle_deg, &want["angleDeg"]);
            rounded("sign.sizeNorm", got.size_norm, &want["sizeNorm"]);
        }

        let spell = compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE);
        let ir = &scenario["spellIR"];
        assert_eq!(spell.active, ir["active"].as_bool().unwrap(), "{name}: active");
        assert_eq!(spell.element.as_deref(), ir["element"].as_str(), "{name}: element");
        assert_eq!(spell.primary_manifestation, ir["primaryManifestation"].as_str().unwrap(), "{name}: primaryManifestation");

        // Exact: the signature gates re-activation on device, so it must match.
        assert_eq!(spell.signature, ir["signature"].as_str().unwrap(), "{name}: signature");

        let want_manifestations = ir["manifestations"].as_object().unwrap();
        assert_eq!(spell.manifestations.len(), want_manifestations.len(), "{name}: manifestation count");
        for (id, want) in want_manifestations {
            let got = spell.manifestations.get(id).unwrap_or_else(|| panic!("{name}: missing manifestation {id}"));
            close(name, &format!("manifestations.{id}.strength"), got.strength, &want["strength"]);
            // JS spreads the convergence profile flat onto the manifestation.
            match (&got.convergence, want.get("point").map(|_| want)) {
                (Some(c), Some(w)) => {
                    close(name, "convergence.point.x", c.point.x, &w["point"]["x"]);
                    close(name, "convergence.point.y", c.point.y, &w["point"]["y"]);
                    close(name, "convergence.radius", c.radius, &w["radius"]);
                    close(name, "convergence.rigidity", c.rigidity, &w["rigidity"]);
                }
                (None, None) => {}
                (g, w) => panic!("{name}: convergence presence differs (rust {:?}, js {:?})", g.is_some(), w.is_some()),
            }
        }

        for (field, got) in [
            ("gravity", spell.gravity),
            ("force", spell.force),
            ("spread", spell.spread),
            ("focus", spell.focus),
            ("range", spell.range),
            ("duration", spell.duration),
            ("effectScale", spell.effect_scale),
            ("directionCoherence", spell.direction_coherence),
            ("quality", spell.quality),
            ("stability", spell.stability),
        ] {
            close(name, field, got, &ir[field]);
        }
        let dir = &ir["direction"];
        close(name, "direction.x", spell.direction.x, &dir["x"]);
        close(name, "direction.y", spell.direction.y, &dir["y"]);
        close(name, "direction.z", spell.direction.z, &dir["z"]);
    }

    seen_signs.sort();
    assert_eq!(seen_signs, ["column", "convergence", "levitation"]);
}
