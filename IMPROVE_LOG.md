# Improve log — catalogue PERFECT loop (2026-10-25)

Rows: P1 all suites green · P2 armv7 release < 1.5 MB · P3 catalogue 4/4 Rust = JS ·
P4 jitter eval 400/400 (`cargo test --release --lib -- --ignored jittered`) ·
P5 fresh judge >= 9/10 twice · P6 INSTALL-CHECKLIST.md complete, no hardware claims.

| Pass | Weekly | Change | P1 | P2 | P3 | P4 | P5 | P6 | Kept? |
|---|---|---|---|---|---|---|---|---|---|
| 0 (baseline) | 81% | — | ✅ recognizer 85 · device 29 · JS 3 | ✅ 1,037,012 B | ✅ 4/4 | ❌ 394/400 (fire-shoot 94) | — not run | ❌ no catalogue / pen / focus rows, 0.3.3 | — |
| 1 | 81% | sample scale 600 → 650 px (fits the 647×904 area) | ✅ | ✅ 1,037,012 B | ✅ 4/4 | ❌ 397/400 (fire-shoot 97) | — | ❌ | ✅ kept |
| 2 | 82% | re-sample strokes to ≤2 px spacing | ✅ | ✅ 1,194,580 B (+157 KB) | ✅ 4/4 | ❌ 397/400 (no gain) | — | ❌ | ❌ reverted |
| 3 | 82% | C1 sigil → dictionary fire template (owner-approved); INSTALL-CHECKLIST rewritten for 0.3.4 (pen, catalogue, focus, uninstall-deletes-log) | ✅ 85 · 29 · 3 | ✅ 1,061,844 B | ✅ 4/4 | ✅ 400/400 | pending | ✅ (to be judged) | ✅ kept |
| 3 judges | 82% | P5 round 1 (sonnet, medium) | | | | | A 9/10 · B 8/10 ❌ (eval #[ignore]d, one-shot not stroke-by-stroke, tuned on the held-out seeds) | | |
| 4 | 82% | eval hardening: 2 fresh never-tuned seeds × 10 redraws replayed stroke-by-stroke like the device (80/80) + 1 jittered redraw/spell in the default suite | ✅ 85 · 30 · 3 | ✅ 1,061,836 B | ✅ 4/4 | ✅ 400/400 + fresh 80/80 | pending round 2 | ✅ | ✅ kept |
| 4 judges | 83% | P5 round 2 | | | | | A 9/10 · B 8/10 ❌ (both: lift by 400 ms timer; B: pick-replaces-page undisclosed) | | |
| 5 | 83% | real finger-up via INPUT_TOUCH_RELEASE (timer kept as fallback); bin reuses lib modules (0 warnings); checklist discloses pick-replaces-page + timing blank | ✅ 85 · 30 · 3 | ✅ 1,063,828 B | ✅ 4/4 | ✅ 400/400 + fresh 80/80 | pending round 3 | ✅ | ✅ kept |

# Improve log, 5-lane pass (2026-10-25, owner lifted the weekly-limit stop)

Rows: P1 suites · P2 armv7 < 2 MB · P3 catalogue N/N Rust = JS, N >= 8 · P4 5 held-out seeds × 20 jittered redraws/spell, 100% ·
P5 animation >= 2× frames at the same total time, no frame > 120 ms in effect_preview · P6 0 open bugs from the fuzz run, each fix tested ·
P7 fresh judge (other model, medium) >= 9/10 twice · P8 INSTALL-CHECKLIST lists every HUMAN check.

| Pass | Weekly | Change | P1 | P2 | P3 | P4 | P5 | P6 | P7 | P8 |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 (baseline, HEAD f6360de) | 93% | — | ✅ 85 · 33 · 3 | ✅ 1,177,264 B | ❌ 4/4 (N < 8) | ✅ 400/400 + fresh 80/80 | ❌ 10 frames × 80 ms | — no fuzz | — | ❌ |
| 1 (lanes A–E merged + lead syncs) | 93%+ | A: 20 eased frames × 40 ms, effects clipped to the canvas (sidebar-wipe bug, test first), dirty rect per frame · B: 12 sigil × sign spells, 4 canon from wiki pages opened this run · C: session.rs + 160k-action fuzz, 6 bugs each with a failing test first · D: chrome.rs, 132 px targets, status headline, catalogue paging · E: FEATURES.md · lead: render clip edge from chrome (156), catalogue re-centred off the wider sidebar, render_feedback refresh clamped | ✅ 85 · 56 · 3 | ✅ 1,539,072 B | ✅ 12/12 | ✅ 1200/1200 + fresh 240/240 | ✅ 20 × 40 ms = 800 ms, worst host frame 43.4 ms | ✅ 0 violations / 160,000 actions, 8 seeds | pending | ✅ rows 2.5, 3.7–3.9, 5.1b, 5.11–5.19, 6a, 6b |
| 2 (judge fixes) | 93%+ | lag log: drop pending timestamps on resume / discarded stroke / hold-to-snap (no bogus p95 after them) · catalogue drawing-area test bound 114 → chrome::CANVAS_LEFT (156) · real-catalogue "past the last spell → None" hit test restored · render_feedback final refresh pad 90 → 120 (covers the settle labels) · FEATURES idea 1 rewritten (CLEAR case fixed by lane C) | ✅ device 56 | ✅ 1,538,952 B | ✅ | (unchanged code path) | (unchanged) | ✅ | pending round 2 | ✅ |

### Raw outputs behind pass 1 (verbatim)

P4, `cd device && cargo test --release --lib -- --ignored jittered --nocapture` (held-out seeds 5 × 20 = 100/spell; fresh 2 × 10 = 20/spell):

```
P4 fire-shoot: 100/100
P4 water-orb: 100/100
P4 converging-wind: 100/100
P4 light-beam: 100/100
P4 pyreball-seal: 100/100
P4 watershot-seal: 100/100
P4 skysoaring-seal: 100/100
P4 converging-flame: 100/100
P4 fresh fire-shoot: 20/20
P4 fresh water-orb: 20/20
P4 converging-water: 100/100
P4 wind-pillar: 100/100
P4 fresh converging-wind: 20/20
P4 floating-light: 100/100
P4 fresh light-beam: 20/20
P4 fresh pyreball-seal: 20/20
P4 converging-light: 100/100
P4 fresh watershot-seal: 20/20
P4 fresh skysoaring-seal: 20/20
P4 fresh converging-flame: 20/20
P4 fresh converging-water: 20/20
P4 fresh wind-pillar: 20/20
P4 fresh floating-light: 20/20
P4 fresh converging-light: 20/20
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 57 filtered out; finished in 87.17s
```

P5, `cd device && cargo run --release --example effect_preview` (host render time + scheduled interval; panel refresh time is NOT measured here):

```
P5 effect timing (host render; scheduled interval 40 ms)
  r150 fire  frames 20 interval 40 ms render mean 0.13 ms max 0.23 ms worst frame 40.2 ms flush 540x540
  r150 water frames 20 interval 40 ms render mean 0.30 ms max 0.33 ms worst frame 40.3 ms flush 540x540
  r150 wind  frames 20 interval 40 ms render mean 0.60 ms max 0.78 ms worst frame 40.8 ms flush 540x540
  r150 earth frames 20 interval 40 ms render mean 0.13 ms max 0.18 ms worst frame 40.2 ms flush 540x540
  r150 light frames 20 interval 40 ms render mean 0.11 ms max 0.18 ms worst frame 40.2 ms flush 540x540
  r519 fire  frames 20 interval 40 ms render mean 0.38 ms max 0.66 ms worst frame 40.7 ms flush 1186x1284
  r519 water frames 20 interval 40 ms render mean 0.97 ms max 1.90 ms worst frame 41.9 ms flush 1186x1284
  r519 wind  frames 20 interval 40 ms render mean 1.59 ms max 1.87 ms worst frame 41.9 ms flush 1186x1284
  r519 earth frames 20 interval 40 ms render mean 0.33 ms max 0.59 ms worst frame 40.6 ms flush 1186x1284
  r519 light frames 20 interval 40 ms render mean 0.31 ms max 0.59 ms worst frame 40.6 ms flush 1186x1284
P5 ok: 20 frames (>= 20) x 40 ms = 800 ms total, no frame > 120 ms
```

P2, `ls -l device/target/armv7-unknown-linux-musleabihf/release/aetherquill` after `cargo build --release --target armv7-unknown-linux-musleabihf`: 1538952 bytes (pass 2).
