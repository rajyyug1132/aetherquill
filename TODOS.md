# TODOs

Deferred work from the 2026-07-08 CEO review (Rust-port architecture decision).

## Watch for rm2fb support on OS 3.27.1.0
**What:** Periodically check ddvk/remarkable2-framebuffer + toltec for a
build supporting OS 3.27.1.0 (device build 20260506100933).
**Why:** The entire on-device deploy path (T5/T6) is gated on this —
confirmed incompatible 2026-07-09. All off-device work is done and waiting.
**Where to check:** https://github.com/ddvk/remarkable2-framebuffer/issues +
https://toltec-dev.org/ compat table.
**Effort:** S (a look, monthly-ish). **Priority:** P1 — it's the only blocker.

## Suspend/resume-safe redraw handling
**What:** Handle the RM2 power button's suspend/resume without leaving a corrupted
screen (riddle's takeover mode handles this explicitly for the Paper Pro).
**Why:** Raw-framebuffer takeover apps can leave stale/garbled ink across a
suspend cycle if untreated. Flagged during CEO review as real robustness, not
pure delight — deliberately deferred rather than built now.
**Effort:** S. **Priority:** P2.
**Context:** `client`'s takeover mode stops xochitl and draws directly to the
framebuffer; on resume, redraw the full canvas from the stroke store
(`redraw_all` in the original `main.rs` already does this on UNDO/CLEAR — the
same function works here) instead of trusting the panel's prior contents.
**Depends on:** the Rust port (recognizer + device crates) landing first.
