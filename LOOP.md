# LOOP.md — Autonomous Port Protocol

Protocol for an unattended Claude session (Sonnet 5 via `/loop`) to finish the
Rust port. **Read `CLAUDE.md` first every iteration** — it holds the durable
architecture decisions this protocol implements. One iteration = one unchecked
task below, nothing more.

## Launch

```
claude --model claude-sonnet-5
> /loop continue Aetherquill: follow LOOP.md exactly — one unchecked task per iteration
```

Self-paced dynamic loop. The loop ENDS when: every task is checked, a
`## Blocked` section exists below, or a hard stop fires.

## Hard stops (non-negotiable)

- **Never SSH to, deploy to, or touch the reMarkable device.** Phases T1/T5/T6
  in CLAUDE.md are ⛔ HUMAN-gated. The loop's last task is the README rewrite;
  device work is a human's.
- **Never edit anything under `service/vendor/wha/`** — parity ground truth.
- **Never force-push, never amend pushed commits, never store secrets.**
- `git push` fails on auth → stop, write `## Blocked`, report.
- A task's tests still red after 3 distinct fix attempts → stop, write
  `## Blocked` with the failing output, do NOT check the task off.

## Task checklist (do the FIRST unchecked one, in order)

- [x] `parity-fixtures` — `service/parity-gen.mjs` + `recognizer/fixtures/pipeline.json` (done this session)
- [x] `layerMapper` → `recognizer/src/layer_mapper.rs` (ported early — coordinateNormalizer dependency)
- [x] `coordinateNormalizer` → `recognizer/src/coordinate_normalizer.rs` (protocol dry run, this session)
- [x] `strokeGrouper` → `recognizer/src/stroke_grouper.rs`
- [x] `templateNormalizer` → `recognizer/src/template_normalizer.rs`
- [x] `templateMatcher` → `recognizer/src/template_matcher.rs`
- [x] `topologicalFloodFill` → `recognizer/src/topological_flood_fill.rs`
- [x] `signRotation` → `recognizer/src/sign_rotation.rs`
- [x] `symbolRecognizer` → `recognizer/src/symbol_recognizer.rs`
- [x] `ringDetector` → `recognizer/src/ring_detector.rs` (owns the `Ring` type — moved from coordinate_normalizer.rs)
- [x] `glyphWarnings` → `recognizer/src/glyph_warnings.rs`
- [x] `drawingClassifier` → `recognizer/src/drawing_classifier.rs` (pipeline entry)
- [x] `semanticRules` → `recognizer/src/semantic_rules.rs`
- [x] `spellDirection` → `recognizer/src/spell_direction.rs`
- [x] `spellQuality` → `recognizer/src/spell_quality.rs`
- [x] `spellBuilder` → `recognizer/src/spell_builder.rs`
- [x] `dictionaries` — embed `sigils.json`/`signs.json` via `include_str!` + serde; parse once at startup
- [x] `end-to-end` — Rust test: every `pipeline.json` scenario through `classify_drawing` → `compile_spell`, parity on `glyphAST`/`spellIR` fields
- [x] `device-crate` — new `device/` crate: UI from `client/src/main.rs` + `recognizer` linked, TCP oracle client deleted. **UNVERIFIED** — `cargo check` fails inside libremarkable's `epoll` dep (Linux-only `libc` constants) before reaching our code at all; needs `cross build --release --target armv7-unknown-linux-gnueabihf` (Docker) or the real ARM toolchain
- [x] `watchdog-grimoire` — home-dir-only watchdog (backgrounded shell loop, not a real systemd unit — see run-on-device.sh comment) + JSONL spell-history log module in `device/`
- [x] `readme-rewrite` — drop superseded tethered-oracle sections; describe the standalone architecture
- [ ] ⛔ HUMAN: T1 device recon · T5 rm2fb extraction + hello-ink smoke test · T6 deploy

## Per-iteration recipe

1. Read `CLAUDE.md`, then read the target JS module in
   `service/vendor/wha/src/` **completely** before writing any Rust.
2. Port 1:1: same function names (snake_cased), same structure, same order,
   load-bearing comments carried over. No redesigns, no "improvements" — the
   port's only virtue is matching the JS. Types live where they're first
   needed; reuse `geometry::Point`/`Bounds` and `stroke_cleaner::*`.
3. Add the module to `recognizer/src/lib.rs`.
4. Tests: unit tests for edge cases (empty/degenerate inputs) PLUS parity
   assertions against `recognizer/fixtures/pipeline.json` where the module's
   stage output is recorded (`cleanedStrokes`, `ring`, `classifications`,
   `candidates`, `recognitions`, `glyphAST`, `spellIR`). Tolerances:
   **1e-6** for unrounded stages (cleanedStrokes, ring, candidates, spellIR),
   **2e-3** for rounded stages (classifications, recognitions, glyphAST — the
   JS rounds these to 3 decimals). Reading the fixture needs serde_json as a
   dev-dependency (add it when first needed).
   - Known ground-truth quirks (do NOT "fix" these; Rust must reproduce them):
     `ring-earth` is ambiguous/invalid (earth's 22-stroke template mutilates
     under minStrokeLength filtering); `spellIR.activatedAt` is nulled in
     fixtures (wall-clock).
5. Run tests (MSVC env is required, this exact incantation works):
   ```
   cmd /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && set PATH=%USERPROFILE%\.cargo\bin;%PATH% && cd /d "D:\Side project witch hat atlier\recognizer" && cargo test'
   ```
6. All green → check the task off in this file, then:
   ```
   git add -A && git commit -m "recognizer: port <module>.js, N tests passing" (+ Co-Authored-By trailer) && git push
   ```
7. Fixtures only change if `service/parity-gen.mjs` changes; regenerate with
   `node service/parity-gen.mjs` and commit the diff together with the reason.

## Environment facts (verified this session)

- Windows 11, no WSL/Docker. Rust 1.96.1 MSVC + VS Build Tools installed.
- `cargo`/`rustc` are NOT on the default PATH — use the incantation above.
- Node v24 on PATH. Use `python`, never `python3` (Store stub).
- Repo: https://github.com/rajyyug1132/aetherquill, branch `master`, gh CLI authed.

## Blocked

(none — loop may run)
