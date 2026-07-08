# WHA Spell Simulator for reMarkable 2

Core concept (unchanged, don't relitigate): draw a spell ring, draw a sigil in
the center, the app recognizes ring + primary sigil (fire/water/wind/earth/
light) + modifying signs and shows the spell activating on the e-ink screen.

## Architecture (revised 2026-07-08, CEO review — supersedes the original tethered-oracle plan)

- **Recognition IS being ported to Rust.** No Node, no toltec, no runtime
  network dependency, on the user's explicit call: they have a real RM2 on
  OS 3.27.1.0 and prioritized "don't risk my device" over build effort.
  `service/vendor/wha/` (unmodified upstream JS) is retained ONLY as a
  **parity-test ground truth** — not shipped to the device. Never delete it;
  the Rust port is checked against its output.
- Split target: `recognizer/` crate — pure logic (geometry, parser, compiler),
  **zero libremarkable dependency**, must compile and `cargo test` on a plain
  Windows/any-OS Rust toolchain with no ARM cross-compiling. `device/` crate —
  thin libremarkable binary linking `recognizer`, RM2-only.
- Target device is reMarkable 2, OS 3.27.1.0, rm2fb-dependent (RM2 has no
  native libremarkable framebuffer support). The riddle reference repo
  targets the Paper Pro — its display/quill code does not apply.
- **rm2fb acquisition: extract the two `.so` files from toltec's OS-matched
  `display` package archive directly (no `opkg`, no toltec bootstrap, no
  `/etc` writes).** Self-building rm2fb from ddvk/remarkable2-framebuffer
  source does NOT reduce risk here — the hard part is version-pinned memory
  offsets into xochitl's binary, which only a community-validated,
  OS-matched build has been checked against. Do not "helpfully" self-build
  the shim instead.
- **Mandatory safety gate before ANY on-device deploy:** manually verify OS
  3.27.1.0 against toltec's current compatibility info. If unlisted/unclear,
  stop and wait — do not proceed on a guess.
- rm2fb hook failure at startup: fail fast, rely on the existing shell
  exit-trap (`scripts/run-on-device.sh`, `trap restore EXIT INT TERM`) to
  restore xochitl. Do not add in-app detection/handling for this — decided
  against during review as unneeded extra surface area.
- **Mandatory phased rollout, each phase gates the next:**
  1. Read-only device recon (OS version, SSH, free space) — zero risk.
  2. `recognizer/` crate: port module-by-module, parity-tested against
     `service/vendor/wha` on a curated fixture set (ring + all 5 elements +
     a few signs — mirrors `service/test.js`'s synthesis helpers). Fully
     testable on this machine, no device needed.
  3. `device/` crate: wire `recognizer` into the existing UI code
     (status bar, ring overlay, activation flash, UNDO/CLEAR, 4-finger
     exit) — delete the TCP oracle client, it's dead code once this lands.
  4. **Hard gate:** deploy a minimal "hello ink" smoke-test binary first to
     prove the rm2fb + deploy + restore loop works, before the real app ever
     touches the device. Must pass and be manually confirmed.
  5. Deploy the full ported client.
- Everything lives under `/home/root/wha/` on the device — no system
  partition, no `/etc`, no OTA involvement. Rollback = `rm -rf
  /home/root/wha` + reboot. This is the actual safety property; keep it true
  for any future addition (watchdog unit, grimoire log included).
- Accepted scope additions: a watchdog systemd unit (home-dir-only, force-
  restores xochitl if the app hangs) and an on-device "grimoire" spell-history
  log (JSON, home-dir-only). Suspend/resume-safe redraw was explicitly
  deferred — see TODOS.md.
- Sequencing: ring + primary sigil + element recognition is the MVP gate for
  the port; the full sign vocabulary (levitation/force/spread/etc. semantic
  math) may land as a fast-follow within the same effort, not indefinitely
  deferred — don't let it silently slip.

## Stale from the original plan (kept for historical context only)

- The original README/scripts describe a tethered Node oracle over TCP.
  That architecture is superseded by the above — do not resurrect it without
  a fresh explicit decision. README needs a rewrite before the port ships.
