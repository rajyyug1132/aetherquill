//! Full-pipeline parity test: every scenario in fixtures/pipeline.json, run
//! through the REAL dictionary (not hand-built test entries), classify_drawing
//! -> compile_spell, checked against the actual JS pipeline's output.
//!
//! This is the test that proves the port is done, not just that each module
//! is individually correct — a wiring mistake between modules (wrong config
//! passed, wrong field threaded through) would pass every unit test above
//! and still fail here.

use recognizer::config::{COMPILER, EFFECT_SIZE, INPUT, LAYERS, RECOGNITION, RING};
use recognizer::dictionaries::load_dictionary;
use recognizer::drawing_classifier::classify_drawing;
use recognizer::geometry::Point;
use recognizer::spell_builder::compile_spell;
use recognizer::stroke_cleaner::RawStroke;

#[test]
fn full_pipeline_matches_js_on_every_fixture_scenario() {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/pipeline.json"))
        .expect("fixtures/pipeline.json — regenerate with: node service/parity-gen.mjs");
    let scenarios: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let dictionary = load_dictionary();

    let mut checked = 0;
    for scenario in scenarios.as_array().unwrap() {
        let name = scenario["name"].as_str().unwrap();
        let strokes: Vec<RawStroke> = scenario["strokes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let points: Vec<Point> = s["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| Point { x: p["x"].as_f64().unwrap(), y: p["y"].as_f64().unwrap() })
                    .collect();
                RawStroke { id: s["id"].as_str().unwrap().to_string(), points }
            })
            .collect();

        let result = classify_drawing(&strokes, None, &dictionary, "0.1.0-test", &INPUT, &RING, &LAYERS, &RECOGNITION);
        let spell = compile_spell(&result.glyph_ast, &COMPILER, &EFFECT_SIZE);
        let expected_ir = &scenario["spellIR"];

        assert_eq!(spell.valid, expected_ir["valid"].as_bool().unwrap(), "{name}: valid");
        assert_eq!(spell.active, expected_ir["active"].as_bool().unwrap(), "{name}: active");
        let expected_element = expected_ir["element"].as_str();
        assert_eq!(spell.element.as_deref(), expected_element, "{name}: element");
        checked += 1;
    }

    assert!(checked >= 10, "expected to check ~11 fixture scenarios, got {checked}");
}
