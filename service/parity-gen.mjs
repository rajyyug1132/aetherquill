// Generates cross-language parity fixtures for the Rust port (recognizer/).
//
// Runs curated drawing scenarios through the REAL vendored JS pipeline and
// records every intermediate stage, so each Rust module test can compare its
// stage's output against ground truth. Regenerate with:  node service/parity-gen.mjs
//
// Determinism notes:
// - spellIR.activatedAt is performance.now() → nulled here.
// - classifications/recognitions/glyphAST are roundedDeep(3 digits) by the JS
//   pipeline; Rust tests must compare those with tolerance 2e-3, unrounded
//   stages (cleanedStrokes, ring, candidates, spellIR numbers) with 1e-6.

import { mkdirSync, writeFileSync } from "node:fs";
import { CONFIG } from "./vendor/wha/src/config.js";
import { classifyDrawing } from "./vendor/wha/src/parser/drawingClassifier.js";
import { compileSpell } from "./vendor/wha/src/compiler/spellBuilder.js";
import { loadDictionary } from "./dictionary.js";

const dictionary = loadDictionary();

// --- stroke synthesis (same shapes as service/test.js) ---
let tick = 0;
function stroke(id, rawPoints) {
  const points = rawPoints.map((p) => ({ x: p.x, y: p.y, pressure: 0.5, t: tick++ }));
  return { id, points, startedAt: points[0].t, endedAt: points[points.length - 1].t };
}

function ringStroke(cx, cy, radius, closed = true) {
  const points = [];
  const last = closed ? 130 : 100; // open ring stops at ~77% of the circle
  for (let i = 0; i <= last; i += 1) {
    const angle = (i / 128) * Math.PI * 2;
    points.push({ x: cx + Math.cos(angle) * radius, y: cy + Math.sin(angle) * radius });
  }
  return stroke(`ring`, points);
}

function templateStrokes(kind, id, cx, cy, size) {
  // sigil ids don't always equal the element name (e.g. wind-directs-air) — match either
  const entry = dictionary[kind].find((e) => e.id === id || e.element === id);
  if (!entry) throw new Error(`${id} not in ${kind} dictionary`);
  return entry.strokeTemplate.strokes.map((templateStroke, index) =>
    stroke(
      `${id}-${index}`,
      templateStroke.map((p) => ({ x: cx - size / 2 + p.x * size, y: cy - size / 2 + p.y * size }))
    )
  );
}

const ELEMENTS = ["fire", "water", "wind", "earth", "light"];
const SIGNS = dictionary.signs.slice(0, 3).map((s) => s.id); // a few signs, per review scope

const scenarios = [
  { name: "empty", strokes: [] },
  { name: "ring-only", strokes: [ringStroke(350, 450, 260)] },
  { name: "open-ring-fire", strokes: [ringStroke(350, 450, 260, false), ...templateStrokes("sigils", "fire", 350, 450, 130)] },
  ...ELEMENTS.map((el) => ({
    name: `ring-${el}`,
    strokes: [ringStroke(350, 450, 260), ...templateStrokes("sigils", el, 350, 450, 130)],
  })),
  ...SIGNS.map((sign) => ({
    name: `ring-fire-${sign}`,
    strokes: [
      ringStroke(350, 450, 260),
      ...templateStrokes("sigils", "fire", 350, 450, 130),
      // signs sit in the outer layer, offset from center
      ...templateStrokes("signs", sign, 350 + 190, 450, 70),
    ],
  })),
];

const fixtures = scenarios.map(({ name, strokes }) => {
  tick = 0; // stable t values per scenario
  const pipeline = classifyDrawing({ strokes, previousRing: null, dictionary, config: CONFIG });
  const spellIR = compileSpell({ glyphAST: pipeline.glyphAST, dictionary, config: CONFIG });
  spellIR.activatedAt = null;
  const { cleanedStrokes, ring, classifications, candidates, recognitions, glyphAST } = pipeline;
  return {
    name,
    strokes,
    cleanedStrokes,
    ring,
    classifications,
    candidates: candidates.map(({ strokes: _s, ...rest }) => rest),
    recognitions,
    glyphAST,
    spellIR,
  };
});

mkdirSync(new URL("../recognizer/fixtures/", import.meta.url), { recursive: true });
const out = new URL("../recognizer/fixtures/pipeline.json", import.meta.url);
writeFileSync(out, JSON.stringify(fixtures, null, 1));
console.log(`wrote ${fixtures.length} scenarios to recognizer/fixtures/pipeline.json`);
for (const f of fixtures) {
  console.log(`  ${f.name}: ring=${f.glyphAST.ring.found} element=${f.spellIR.element} valid=${f.spellIR.valid}`);
}
