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
    let firstLine = true;
    socket.setNoDelay(true);
    socket.on("error", (error) => console.error(`client error: ${error.message}`));
    const lines = createInterface({ input: socket });
    // readline attaches its own "error" listener to the socket and re-emits
    // on the Interface; without a listener here that re-emit is unhandled
    // and crashes the whole process (observed: a client disconnecting mid-write).
    lines.on("error", () => {});
    lines.on("line", (line) => {
      // A browser (or any HTTP client) speaks HTTP to this raw TCP JSON-line
      // server; answer the first request line with one plaintext HTTP response
      // instead of parsing every header as JSON and spewing errors.
      if (firstLine && /^(GET|POST|HEAD|PUT|DELETE|OPTIONS|PATCH) .* HTTP\/\d/.test(line)) {
        const body = "This is the WHA recognition oracle: a TCP server speaking newline-delimited JSON, not HTTP. Connect a wha client, not a browser.\n";
        socket.end(
          `HTTP/1.1 426 Upgrade Required\r\nContent-Type: text/plain\r\nContent-Length: ${Buffer.byteLength(body)}\r\nConnection: close\r\n\r\n${body}`
        );
        return;
      }
      firstLine = false;
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
      if (socket.writable) {
        socket.write(JSON.stringify(reply) + "\n");
      }
    });
  });

  server.listen(port, () => console.log(`wha oracle listening on :${port}`));
  return server;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  serve(Number(process.env.WHA_ORACLE_PORT ?? 7777));
}
