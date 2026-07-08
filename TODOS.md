# TODOs

Deferred work from the 2026-07-08 CEO review (Rust-port architecture decision).

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
