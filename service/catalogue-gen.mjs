// Generates device/assets/catalogue.json: the four Spell Catalogue entries the
// tablet can ink with one tap. Every entry is run through the REAL vendored JS
// pipeline here and its result recorded under `expected`, so the Rust side
// (device/src/catalogue.rs tests) must reproduce it exactly. Regenerate with:
//   node service/catalogue-gen.mjs
//
// Coordinates are web-canvas px (screen px x 0.5, the device's CANVAS_SCALE),
// centred on the drawing area. Stroke order is the order the tablet inks them.
// Names/lines are written for this project; lore sources are in lore.md.

import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { CONFIG } from "./vendor/wha/src/config.js";
import { classifyDrawing } from "./vendor/wha/src/parser/drawingClassifier.js";
import { compileSpell } from "./vendor/wha/src/compiler/spellBuilder.js";
import { loadDictionary } from "./dictionary.js";

const dictionary = loadDictionary();
const samples = JSON.parse(readFileSync(new URL("./vendor/wha/src/dictionary/sample-spells.json", import.meta.url)));

// Device drawing area in canvas px: x from the sidebar (110/2) to 1404/2,
// y from the status strip (64/2) to 1872/2.
const CENTER = { x: (55 + 702) / 2, y: (32 + 936) / 2 };
const SAMPLE_SIZE = 650; // normalized 0..1 sample strokes -> canvas px (650 fits the 647x904 area; larger = jitter-sturdier)

// --- stroke synthesis (same shapes as service/test.js / parity-gen.mjs) ---
function ringPoints(cx, cy, radius) {
  const points = [];
  for (let i = 0; i <= 130; i += 1) {
    const angle = (i / 128) * Math.PI * 2;
    points.push({ x: cx + Math.cos(angle) * radius, y: cy + Math.sin(angle) * radius });
  }
  return points;
}

function templatePoints(kind, id, cx, cy, size) {
  const entry = dictionary[kind].find((e) => e.id === id || e.element === id);
  if (!entry) throw new Error(`${id} not in ${kind} dictionary`);
  return entry.strokeTemplate.strokes.map((s) => s.map((p) => ({ x: cx - size / 2 + p.x * size, y: cy - size / 2 + p.y * size })));
}

// The vendored sample rings are sketched from separate segments: besides the
// ~7.5° opening, several junctions are 9-41 px apart, wider than
// topologicalFloodFill's 3 px stroke radius can bridge, so "complete" (a
// topological closure) never fires. One closing stroke tracing the fitted
// ring line closes all of them, like re-inking the ring to cast. (The tablet
// redraws a complete ring as one perfect circle anyway.)
function closingStroke(ring) {
  const points = [];
  for (let deg = 0; deg <= 370; deg += 2) {
    const a = (deg * Math.PI) / 180;
    points.push({ x: ring.center.x + Math.cos(a) * ring.radius, y: ring.center.y - Math.sin(a) * ring.radius });
  }
  return points;
}

function run(pointLists) {
  let tick = 0;
  const strokes = pointLists.map((pts, i) => {
    const points = pts.map((p) => ({ x: p.x, y: p.y, pressure: 0.5, t: tick++ }));
    return { id: `c${i}`, points, startedAt: points[0].t, endedAt: points.at(-1).t };
  });
  const pipeline = classifyDrawing({ strokes, previousRing: null, dictionary, config: CONFIG });
  const spellIR = compileSpell({ glyphAST: pipeline.glyphAST, dictionary, config: CONFIG });
  return { pipeline, spellIR };
}

function centred(pointLists) {
  const all = pointLists.flat();
  const bx = (Math.min(...all.map((p) => p.x)) + Math.max(...all.map((p) => p.x))) / 2;
  const by = (Math.min(...all.map((p) => p.y)) + Math.max(...all.map((p) => p.y))) / 2;
  return pointLists.map((pts) => pts.map((p) => ({ x: p.x - bx + CENTER.x, y: p.y - by + CENTER.y })));
}

function fromSample(id) {
  const sample = samples.find((s) => s.id === id);
  const strokes = centred(sample.strokes.map((s) => s.map((p) => ({ x: p.x * SAMPLE_SIZE, y: p.y * SAMPLE_SIZE }))));
  return [...strokes, closingStroke(run(strokes).pipeline.ring)];
}

function synthesized(sigil, sign) {
  // Sign in the outer band directly below the seal: the placement upstream
  // recognizes for the upright templates (probed for parity-gen.mjs). Ring is
  // drawn last so the spell activates once, on the final stroke.
  const cx = 350, cy = 450;
  return centred([...templatePoints("sigils", sigil, cx, cy, 130), ...templatePoints("signs", sign, cx, cy + 210, 44), ringPoints(cx, cy, 260)]);
}

// C2 is template-built: the vendored water-orb sample, once its ring is
// closed, has one levitation sign misread as a fire sigil by upstream's own
// pipeline ("Multiple sigils detected"); dropped after 3 fixes (owner's call).
const entries = [
  { id: "fire-shoot", name: "Fire Shoot", line: "Fire with column signs: a jet of flame shoots up.", sigil: "fire", sign: "column", strokes: fromSample("fire-column") },
  { id: "water-orb", name: "Water Orb", line: "Water with levitation signs: water floats as an orb.", sigil: "water", sign: "levitation", strokes: synthesized("water", "levitation") },
  { id: "converging-wind", name: "Converging Wind", line: "Wind with a convergence sign: air focused to one point.", sigil: "wind-directs-air", sign: "convergence", strokes: synthesized("wind-directs-air", "convergence") },
  { id: "light-beam", name: "Light Beam", line: "Light with a column sign: a beam rises from the glyph.", sigil: "light", sign: "column", strokes: synthesized("light", "column") },
];

const failures = [];
const catalogue = entries.map(({ strokes, ...meta }) => {
  const { pipeline, spellIR } = run(strokes);
  const signs = pipeline.glyphAST.signs.map((s) => s.id);
  const element = dictionary.sigils.find((s) => s.id === meta.sigil).element;
  // A catalogue spell the recognizer can't read back is a broken demo.
  if (!spellIR.active || spellIR.element !== element || spellIR.primaryManifestation !== meta.sign) {
    failures.push(`${meta.id}: JS reads ${spellIR.status} ${spellIR.element} ${spellIR.primaryManifestation} [${signs}] ${pipeline.glyphAST.warnings}`);
  }
  return {
    ...meta,
    strokes: strokes.map((pts) => pts.map((p) => [p.x, p.y])),
    expected: { status: spellIR.status, element: spellIR.element, primaryManifestation: spellIR.primaryManifestation, signs, signature: spellIR.signature },
  };
});

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

const out = new URL("../device/assets/catalogue.json", import.meta.url);
mkdirSync(new URL("../device/assets/", import.meta.url), { recursive: true });
writeFileSync(out, JSON.stringify(catalogue, null, 1) + "\n");
for (const c of catalogue) {
  console.log(`  ${c.id}: ${c.expected.status} ${c.expected.element} ${c.expected.primaryManifestation} [${c.expected.signs}] (${c.strokes.length} strokes)`);
}
