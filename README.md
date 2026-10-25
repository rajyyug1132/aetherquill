# Aetherquill

## Draw working spells on a reMarkable 2, Witch Hat Atelier style

Aetherquill turns a reMarkable 2 e-ink tablet into a spell-drawing simulator
in the spirit of [*Witch Hat
Atelier*](https://en.wikipedia.org/wiki/Witch_Hat_Atelier). Draw a spell ring
with the pen, draw a sigil in its center, close the ring — the tablet
recognizes the glyph, compiles it into a spell, and plays an elemental
activation animation right on the page: rising flames, expanding ripples,
curling wind, rising earth, radiant light. It runs as a windowed app inside
the stock reMarkable UI. No phone, no screen glow, no network — just ink on
paper that means something.

Recognition is a 1:1 Rust port of
[wha-spell-simulator](https://github.com/ytnrvdf/wha-spell-simulator)'s
pipeline (try its [live web version](https://ytnrvdf.github.io/wha-spell-simulator)
— same recognizer, same dictionaries). The port is verified module-by-module
against the vendored original JS with 85 tests, including end-to-end and per-sign spellIR parity.

```
draw a ring ──► draw a sigil inside it ──► close the ring
                                                 │
                                                 ▼
                       ring + sigil recognized, spell compiled in-process
                                                 │
                                                 ▼
              ring snaps to a perfect circle · elemental animation plays
                        · spell logged to the on-device grimoire
```

```
reMarkable 2, inside stock xochitl (windowed via xovi + AppLoad + qtfb)
  pen ──► qtfb input events ──► ink to shared-memory fb (~25Hz coalesced refresh)
              │ pen-up
              ▼  in-process call, no network
        recognizer::classify_drawing() ──► recognizer::compile_spell()
              │
              ▼
  status line · perfect-circle ring snap · per-element activation animation
```

## Using it (on a tablet)

**You need:** a reMarkable 2 on OS 3.27.x, SSH access over USB
(`ssh root@10.11.99.1`; password under Settings → About → Copyrights),
and the [Vellum](https://github.com/vellum-dev/vellum) package manager stack.

1. **Install the runtime stack** (all of it lives in `/home/root` only —
   rollback is `rm -rf /home/root/.vellum /home/root/xovi` + reboot):

   ```sh
   # on the tablet, via SSH — install Vellum per its README, then:
   vellum update
   vellum add xovi xovi-extensions appload
   /home/root/xovi/rebuild_hashtable
   reboot
   ```

   ⚠️ xovi does **not** auto-start after a reboot: run
   `/home/root/wha/start-xovi.sh` (installed by Aetherquill's `install.sh`) or
   `/home/root/xovi/start` after each boot — the screen flickers as the UI
   reloads with AppLoad.

2. **Install Aetherquill** from a release bundle (GitHub release, or build
   it yourself on any OS with Rust + bash — plain Windows works, no WSL/Docker):

   ```sh
   rustup target add armv7-unknown-linux-musleabihf
   bash scripts/make-bundle.sh 0.3.6        # -> dist/aetherquill-0.3.6.tar.gz

   scp dist/aetherquill-0.3.6.tar.gz root@10.11.99.1:/home/root/
   ssh root@10.11.99.1 "cd /home/root && tar xzf aetherquill-0.3.6.tar.gz && sh aetherquill-0.3.6/install.sh"
   ```

   `install.sh` writes only `/home/root/xovi/exthome/appload/aetherquill`
   and `/home/root/wha`; `sh install.sh --uninstall` removes both. First run:
   follow [INSTALL-CHECKLIST.md](INSTALL-CHECKLIST.md).

3. **Cast**: open AppLoad in the tablet UI, launch Aetherquill, draw a big
   ring, draw a sigil (fire, water, wind, earth, or light) in its center.
   Sample layouts: `service/vendor/wha/src/dictionary/sample-spells.json`.

### Controls

| Do this | And |
|---|---|
| Draw with the pen | Ink appears; recognition runs on every pen-up |
| Close a ring | It snaps to a perfect circle |
| Hold the pen still ~0.6s at a stroke's end | Stroke snaps to a perfect line / triangle / rectangle / circle |
| Sidebar: pen / eraser | Switch tool (eraser deletes strokes the pen touches) |
| Sidebar: ↶ / ↷ | Undo / redo (erasures and CLEAR are recoverable too) |
| Sidebar: ✕ | Clear the page |
| Sidebar: catalogue (bottom) | Pick one of 4 pre-drawn spells (Fire Shoot, Water Orb, Converging Wind, Light Beam): the page clears, its strokes ink in and it casts. Undo removes the strokes. Sources: [lore.md](lore.md) |
| One-finger drag on a stroke | Move it; recognition re-runs on drop |
| Close the window from AppLoad | Exit — stock UI keeps running throughout |

## Hacking on it

Two crates, split on purpose:

```
recognizer/  the ported recognition pipeline — pure Rust, zero device deps,
             compiles + tests on any OS (85 tests incl. parity)
device/      the RM2 app: qtfb client, sidebar UI, shape snapping, effects.
             lib part (shapes.rs, render.rs) is host-testable; the binary
             is ARM-only
service/     parity ground truth: vendor/wha/ (unmodified upstream JS — never
             edit), parity-gen.mjs (regenerates fixtures), test.js
TODOS.md     tracked deferred work
CLAUDE.md    durable architecture decisions, device state, phase-gate history
```

```sh
# recognition pipeline — the core, fully testable off-device
cd recognizer && cargo test          # 85 tests incl. end-to-end + sign parity

# device logic that doesn't need a device
cd device && cargo test --lib        # shape-snapping emulation tests
cargo run --example effect_preview   # renders all 5 elemental effects to a PPM

# JS-side parity ground truth
cd service && node --test            # 3 tests
node service/parity-gen.mjs          # regenerate fixtures after upstream changes
node service/catalogue-gen.mjs       # regenerate device/assets/catalogue.json (JS-verified)
```

The recognizer port is deliberately 1:1 with the JS — same function names
(snake_cased), same structure, quirks reproduced faithfully (see LOOP.md for
the porting recipe). Don't "improve" it; its only virtue is matching upstream
so the parity tests stay meaningful.

## Contributing

Issues and PRs welcome. Ground rules:

- Open an issue first for anything non-trivial.
- `service/vendor/wha/` is never edited — it's the parity ground truth.
- Changes to `recognizer/` must keep all recognizer tests (85, incl. parity) green; if upstream
  moved, regenerate fixtures with `parity-gen.mjs` in the same PR.
- Device-touching changes: prove what you can off-device first (host tests /
  `effect_preview`), and note what was actually verified on hardware.
- Everything deployed to the tablet must stay under `/home/root` — no system
  partition, no `/etc`, no OTA involvement. That's the project's safety
  property; don't break it.

## Known issues

- xovi doesn't auto-start after reboot — run `/home/root/wha/start-xovi.sh`
  over SSH after each boot (idempotent; does nothing if xovi is already
  loaded). Vellum's `tripletap` package can start it from a power-button
  triple-press, but it installs a systemd unit into `/etc`, outside this
  project's `/home/root`-only rollback guarantee — opt in knowingly.
- The marker's built-in eraser end can't be detected: AppLoad's qtfb bridge
  doesn't forward pen-vs-eraser (`devId` is a TODO upstream), hence the
  eraser is a sidebar toggle.
- Suspend/resume: the app detects a resume (BOOTTIME vs MONOTONIC drift) and
  redraws from its stroke store; a stroke in progress when the tablet slept
  is dropped. Not yet verified on hardware.
- One ring, one primary sigil per drawing (an upstream pipeline limit).
- Modifying signs (column, levitation, convergence) compile exactly as
  upstream and show in the status line; their e-ink overlays are
  Aetherquill's own drawing, since upstream's renderer isn't vendored. Signs
  recognize most reliably drawn in the outer band directly below the seal.
- E-ink animation is ~6 frames at ~12fps with partial refreshes — dramatic,
  not smooth; ghosting varies by panel.

## License

MIT (see [LICENSE](LICENSE)). `service/vendor/wha/` is vendored unmodified
from [ytnrvdf/wha-spell-simulator](https://github.com/ytnrvdf/wha-spell-simulator)
(MIT, see `service/vendor/wha/LICENSE`).

Aetherquill is an unofficial fan project. *Witch Hat Atelier* and related
names, artwork, and terminology belong to their respective rights holders;
nothing here is official or endorsed.
