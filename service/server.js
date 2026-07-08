import { createServer } from "node:net";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

import { CONFIG } from "./vendor/wha/src/config.js";
import { classifyDrawing } from "./vendor/wha/src/parser/drawingClassifier.js";
import { compileSpell } from "./vendor/wha/src/compiler/spellBuilder.js";
import { loadDictionary } from "./dictionary.js";

const dictionary = loadDictionary();

export function recognize(strokes, previousRing = null) {
  const pipeline = classifyDrawing({ strokes, previousRing, dictionary, config: CONFIG });
  const spellIR = compileSpell({ glyphAST: pipeline.glyphAST, dictionary, config: CONFIG });
  // pipeline.ring is the unrounded ring the classifier wants back as previousRing.
  return { glyphAST: pipeline.glyphAST, spellIR, rawRing: pipeline.ring };
}

// Protocol: one JSON object per line, both directions.
// Request:  {"strokes": [{"id": "s1", "points": [{"x":..,"y":..,"pressure":..,"t":..}, ...]}, ...]}
// Response: {"glyphAST": {...}, "spellIR": {...}} or {"error": "..."}
export function serve(port) {
  const server = createServer((socket) => {
    let previousRing = null;
    socket.setNoDelay(true);
    socket.on("error", (error) => console.error(`client error: ${error.message}`));
    createInterface({ input: socket }).on("line", (line) => {
      let reply;
      try {
        const { strokes = [] } = JSON.parse(line);
        if (strokes.length === 0) {
          previousRing = null;
        }
        const { glyphAST, spellIR, rawRing } = recognize(strokes, previousRing);
        previousRing = rawRing;
        reply = { glyphAST, spellIR };
      } catch (error) {
        reply = { error: String(error?.message ?? error) };
      }
      socket.write(JSON.stringify(reply) + "\n");
    });
  });

  server.listen(port, () => console.log(`wha oracle listening on :${port}`));
  return server;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  serve(Number(process.env.WHA_ORACLE_PORT ?? 7777));
}
