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
