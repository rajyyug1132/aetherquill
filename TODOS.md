# TODOs

Deferred work. History: the rm2fb watch item (dead since the 2026-07-10
xovi/AppLoad/qtfb pivot) and suspend/resume redraw (shipped: device/src/resume.rs)
were removed 2026-10-25.

## xovi start after reboot — owner decision
**What:** xovi doesn't survive a reboot. Today: `/home/root/wha/start-xovi.sh`
over SSH after each boot (idempotent, home-dir-only).
**Option:** Vellum's `tripletap` package starts xovi on a power-button
triple-press, but installs a systemd unit into `/etc/systemd/system` — that
breaks the "everything under /home/root" rollback property. Adopt only on an
explicit decision; record it in CLAUDE.md if taken.
**Effort:** S. **Priority:** P2.

## Sign-to-effect visual mapping
**What:** spellIR numbers for signs are upstream-exact (parity-tested), but
the e-ink overlays (column pillar, levitation chevrons, convergence spokes)
are Aetherquill's own — upstream's particle renderer isn't vendored. If the
upstream renderer is ever vendored, port its sign behaviour 1:1.
**Effort:** M. **Priority:** P3.
