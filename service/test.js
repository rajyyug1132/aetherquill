import assert from "node:assert/strict";
import test from "node:test";

import { recognize } from "./server.js";
import { loadDictionary } from "./dictionary.js";

// Synthesizes a drawing the way the on-device client reports one: strokes of
// {x, y, pressure, t} points on a ~702x936 canvas (screen px / 2).

let tick = 0;

function stroke(id, rawPoints) {
  const points = rawPoints.map((point) => ({ x: point.x, y: point.y, pressure: 0.5, t: tick++ }));
  return { id, points, startedAt: points[0].t, endedAt: points[points.length - 1].t };
}

function ringStroke(cx, cy, radius) {
  const points = [];
  for (let i = 0; i <= 130; i += 1) {
    const angle = (i / 128) * Math.PI * 2;
    points.push({ x: cx + Math.cos(angle) * radius, y: cy + Math.sin(angle) * radius });
  }
  return stroke("ring", points);
}

function sigilStrokes(id, cx, cy, size) {
  const entry = loadDictionary().sigils.find((sigil) => sigil.id === id);
  assert.ok(entry, `${id} sigil in dictionary`);
  return entry.strokeTemplate.strokes.map((templateStroke, index) =>
    stroke(
      `${id}-${index}`,
      templateStroke.map((point) => ({
        x: cx - size / 2 + point.x * size,
        y: cy - size / 2 + point.y * size
      }))
    )
  );
}

test("empty drawing yields no ring", () => {
  const { glyphAST, spellIR } = recognize([]);
  assert.equal(glyphAST.ring.found, false);
  assert.equal(spellIR.valid, false);
  assert.equal(spellIR.status, "No ring detected");
});

test("ring alone is an invalid spell (no sigil)", () => {
  const { glyphAST, spellIR } = recognize([ringStroke(350, 450, 260)]);
  assert.equal(glyphAST.ring.found, true);
  assert.equal(spellIR.valid, false);
});

test("ring with centered fire sigil compiles an active fire spell", () => {
  const strokes = [ringStroke(350, 450, 260), ...sigilStrokes("fire", 350, 450, 130)];
  const { glyphAST, spellIR } = recognize(strokes);
  assert.equal(glyphAST.ring.found, true);
  // Wire contract the Rust client deserializes: numeric center/radius on the ring.
  assert.equal(typeof glyphAST.ring.center.x, "number");
  assert.equal(typeof glyphAST.ring.radius, "number");
  assert.equal(typeof spellIR.signature, "string");
  assert.equal(glyphAST.primarySigil?.id, "fire");
  assert.equal(spellIR.valid, true);
  assert.equal(spellIR.element, "fire");
  assert.equal(spellIR.active, true, `expected active spell, got status: ${spellIR.status}`);
});
