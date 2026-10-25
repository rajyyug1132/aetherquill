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
