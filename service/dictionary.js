import { readFileSync } from "node:fs";

// fs-based stand-in for vendor/wha's dictionaryLoader.js, which uses browser fetch().
function readJson(name) {
  return JSON.parse(readFileSync(new URL(`./vendor/wha/src/dictionary/${name}`, import.meta.url), "utf8"));
}

export function loadDictionary() {
  return {
    sigils: readJson("sigils.json"),
    signs: readJson("signs.json"),
    sampleSpells: readJson("sample-spells.json")
  };
}
