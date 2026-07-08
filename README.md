# Aetherquill

**Draw spells on a reMarkable 2.**

Aetherquill is a spell-drawing simulator, in the spirit of [*Witch Hat
Atelier*](https://en.wikipedia.org/wiki/Witch_Hat_Atelier), built for a
reMarkable 2's e-ink screen. Draw a ring with the pen, draw a sigil in its
center, and the tablet recognizes the glyph, compiles it into a spell, and
shows it activating on the page — a status line, a clean fitted-ring overlay,
and a region flash with the element name.

No app, no phone, no screen glow. Just ink on paper that means something.

```
draw a ring ──► draw a sigil inside it ──► close the ring
                                                  │
                                                  ▼
                          ring + sigil + signs recognized, spell compiled
                                                  │
                                                  ▼
                     status line · ring overlay · activation flash on the page
```

## How it works

Recognition — ring detection, sigil/sign matching, spell compilation — comes
from [wha-spell-simulator](https://github.com/ytnrvdf/wha-spell-simulator),
reused rather than reinvented: it's pure, DOM-free JS with its own dictionary
of sigils, signs, and sample spell layouts. Aetherquill vendors that pipeline
unmodified (`service/vendor/wha/`, MIT) as the ground truth for a native Rust
port that runs on the tablet itself, with no phone or laptop required once
installed. See [Architecture](#architecture--roadmap) below for exactly what's
built today versus what's in flight.

The on-device pen/framebuffer layer is a from-scratch Rust client built on
[`libremarkable`](https://github.com/canselcik/libremarkable). The takeover
launch pattern (stop the stock UI, draw directly to the e-ink panel, always
restore the stock UI on exit) follows the convention established by
[riddle](https://github.com/MaximeRivest/riddle), an enchanted-diary app for
the reMarkable Paper Pro — riddle's display internals are Paper-Pro-specific
and don't transfer, but its exit-safety pattern does.

## Architecture & roadmap

**Current state (implemented, tested):** a tethered prototype. The Rust
client on the tablet captures ink and ships completed drawings to a small
recognition server running on a host machine over the local network; the
server runs the vendored JS pipeline unmodified and returns the compiled
spell. This is fast to iterate on and fully covered by automated tests, but
it needs a host machine within reach every session.

```
reMarkable 2 (client/, Rust + libremarkable)      host machine (service/, Node)
  pen ──► ink capture ──► local e-ink render          │
              │ pen-up                                 │
              ▼  newline-framed JSON over TCP           │
        ─────────────────────────────────────────►  classifyDrawing()
                                                      compileSpell()
        ◄─────────────────────────────────────────  {glyphAST, spellIR}
  status bar · ring overlay · activation flash
```

**In progress (see [TODOS.md](TODOS.md) and the architecture notes in
[CLAUDE.md](CLAUDE.md)):** a full Rust port of the recognition pipeline,
split into a dependency-free `recognizer` crate (portable, unit-tested
against the vendored JS pipeline as ground truth) and a `device` crate (the
RM2-specific binary). Once this lands, Aetherquill runs standalone on the
tablet — no host machine, no network dependency. The rollout is deliberately
staged and safety-gated: every step lives under the device's user-writable
home directory, nothing touches the OS partition, and rollback is a single
directory delete plus a reboot.

## Repository layout

```
service/    recognition pipeline: server.js (TCP), dictionary.js (loader),
            test.js (parity suite), vendor/wha/ (unmodified upstream — never edit)
client/     the on-device Rust app (tethered prototype, today)
scripts/    deploy + takeover launch scripts (always restore the stock UI on exit)
TODOS.md    tracked deferred work
CLAUDE.md   durable architecture decisions and constraints
```

## Quickstart (tethered prototype)

```sh
# 1. sanity-check the recognition pipeline — no tablet needed
cd service && node --test

# 2. start the recognition server on your host machine (listens on :7777)
node service/server.js

# 3. build + deploy the client (from WSL/Linux; needs Docker for `cross`)
cd client && cross build --release --target armv7-unknown-linux-gnueabihf
cd .. && ./scripts/deploy.sh

# 4. run it — the script stops the stock UI and restores it on exit
ssh root@10.11.99.1 /home/root/wha/run-on-device.sh
```

Draw a large ring, then a sigil (fire, water, wind, earth, or light) in its
center. Closing the ring with a valid sigil activates the spell. Sample
layouts live in `service/vendor/wha/src/dictionary/sample-spells.json`, or
try the [live web version](https://ytnrvdf.github.io/wha-spell-simulator) of
the upstream simulator — same recognizer, same dictionaries.

### Prerequisites

- **Tablet**: reMarkable 2, SSH access, [toltec](https://toltec-dev.org/)
  with the `display` (rm2fb) package installed.
- **Host**: Node ≥ 18. For the client build: Rust +
  [`cross`](https://github.com/cross-rs/cross) (needs Docker), or WSL with
  the `armv7-unknown-linux-gnueabihf` target and a matching GCC linker.
  Plain Windows MSVC Rust cannot link the ARM binary.

## Controls (on device)

| Do this | And |
|---|---|
| Draw with the pen | Ink appears instantly; recognition runs on every pen-up |
| Tap **UNDO** (top-left) | Remove the last stroke |
| Tap **CLEAR** (top-right) | Wipe the page |
| Tap with 4+ fingers | Exit — the stock UI restarts automatically |

## Known limits (prototype)

- Feedback is text + overlay + flash; no particle effects (e-ink can't do 60fps).
- One ring, one primary sigil per drawing (an upstream pipeline limit).
- The tethered client needs a host machine reachable on the network — the
  standalone Rust port removes this, see [Architecture & roadmap](#architecture--roadmap).

## License

MIT (see [LICENSE](LICENSE)). `service/vendor/wha/` is vendored unmodified
from [ytnrvdf/wha-spell-simulator](https://github.com/ytnrvdf/wha-spell-simulator)
(MIT, see `service/vendor/wha/LICENSE`).

Aetherquill is an unofficial fan project. *Witch Hat Atelier* and related
names, artwork, and terminology belong to their respective rights holders;
nothing here is official or endorsed.
