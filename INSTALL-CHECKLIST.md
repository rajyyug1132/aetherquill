# First on-device run: checklist (0.3.5)

⛔ **Run this yourself.** Nothing in this build has been verified on hardware.
Every row below is something the off-device tests can't prove. Tick each one
and note anything odd in the "Notes" column. Stop at the first ✗ and go to the
rollback section.

**Before you start:**
- RM2 on OS 3.27.x (Settings → About). 3.27.3.0 is the build the runtime stack
  was checked against.
- Connected over USB, with `ssh root@10.11.99.1` working.
- Vellum stack installed: xovi, xovi-extensions and appload (README step 1).
- The bundle `aetherquill-0.3.5.tar.gz`, built with `bash scripts/make-bundle.sh 0.3.5`
  or taken from a GitHub release.
- **`--uninstall` deletes your spell log.** To keep it, copy it off first:
  `scp root@10.11.99.1:/home/root/wha/spells/log.jsonl .`

## 1. Install

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 1.1 | `ssh root@10.11.99.1 "df -h /home"` | several GB free | | |
| 1.2 | close Aetherquill if it's open, then `scp aetherquill-0.3.5.tar.gz root@10.11.99.1:/home/root/` | copies, no error | | |
| 1.3 | `ssh root@10.11.99.1 "cd /home/root && tar xzf aetherquill-0.3.5.tar.gz && sh aetherquill-0.3.5/install.sh"` | prints `installed to /home/root/xovi/exthome/appload/aetherquill` | | |
| 1.4 | `ssh root@10.11.99.1 "ls -l /home/root/xovi/exthome/appload/aetherquill /home/root/wha"` | `aetherquill` (rwx) and the manifest; `start-xovi.sh` and `spells/` | | |
| 1.5 | `ssh root@10.11.99.1 /home/root/wha/start-xovi.sh` | "already loaded", or the screen flickers and xochitl comes back | | |
| 1.6 | open AppLoad in the tablet UI | "Aetherquill" is listed | | |
| 1.7 | launch it (about 1 s longer than before: it sets the fast refresh mode once at start) | white canvas; left sidebar with 6 icons (pen, eraser, ↶, ↷, ✕, catalogue page); "Draw a spell ring to begin" | | |

## 2. Pen feel (direct digitizer input)

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 2.1 | draw quick strokes in the middle and near each corner of the canvas | ink lands under the pen tip, not offset, and keeps up better than 0.3.0 | | |
| 2.1b | with a ring on the page, draw several sigil strokes quickly one after another | the pen never freezes between strokes; the status line and any ring snap catch up about 0.35 s after you stop (recognition now runs in the background) | | |
| 2.2 | look at fresh ink while drawing | some graininess or ghosting is expected (fast waveform); note how bad it is | | |
| 2.3 | with Aetherquill still open, switch to a notebook, write a line there, then come back | no stray strokes in the spell canvas | | |
| 2.4 | toggle the eraser and rub out a stroke | it disappears (the eraser still uses AppLoad's pen path) | | |

## 3. Cast all five elements by hand

For each: tap ✕, draw a big ring (about half the screen width), draw the sigil
in the center, then close the ring. Sigil shapes are in
`service/vendor/wha/src/dictionary/sample-spells.json` or the
[web demo](https://ytnrvdf.github.io/wha-spell-simulator).

| # | Element | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 3.1 | fire | ring snaps to a circle; status `Active spell - fire  q..% s..%`; about 0.85 s rising-flame animation; `fire` under the seal | | |
| 3.2 | water | same, with ripples and droplets; `water` | | |
| 3.3 | wind | same, with curling spiral arms; `wind` | | |
| 3.4 | earth | same, with rising square debris; `earth`. Hard to trigger: earth's template is fussy upstream too | | |
| 3.5 | light | same, with pulsing beams; `light` | | |
| 3.6 | `ssh root@10.11.99.1 "tail -n 5 /home/root/wha/spells/log.jsonl"` | one JSON line per activation above | | |

## 4. Cast one sign by hand

Clear, then draw ring + fire sigil. Draw **one** sign (column, levitation or
convergence) in the outer band **directly below the seal**, about ¾ of the way
from the center to the ring and smaller than the sigil. That's the placement
host tests found reliable. Close the ring last.

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 4.1 | sign: ______ | status line 2 starts with the sign name and a %, e.g. `levitation 62%  force 91 spread 53 range 66 grav 74` | | |
| 4.2 | watch the activation | fire effect plus the sign overlay: column = ladder-like pillar, levitation = rising chevrons, convergence = spokes aimed at a point inside the ring | | |
| 4.3 | settle frame | `fire`, with the sign name in smaller text below it | | |
| 4.4 | sign not recognized? | line 2 shows a warning such as `symbol_near_layer_boundary`; move the sign further into the outer band and retry | | |

## 5. Spell Catalogue

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 5.1 | tap the catalogue icon (bottom of the sidebar) with a finger | a "Spell Catalogue" panel with 4 rows: Fire Shoot, Water Orb, Converging Wind, Light Beam, each with one line of text | | |
| 5.2 | tap **Fire Shoot** | page clears; strokes ink in one by one; ring snaps; status `Active spell - fire`; line 2 starts `column`; animation plays once, at the end. A pick **replaces** the page: what you had drawn can't be brought back with ↶/↷ | | |
| 5.3 | tap the icon, then **Water Orb** | same, with `water` and `levitation` | | |
| 5.4 | tap the icon, then **Converging Wind** (49 strokes, the slowest) | same, with `wind` and `convergence`. Write down how long the inking takes (not measured on hardware yet; the pen is unresponsive while it inks) | | |
| 5.5 | tap the icon, then **Light Beam** | same, with `light` and `column` | | |
| 5.6 | pick the same spell twice in a row | the animation plays again the second time | | |
| 5.7 | tap ↶ a few times after a catalogue spell | its strokes come off one per tap. ✕ clears it in one go | | |
| 5.8 | open the panel, then tap ↶, ↷ or ✕ | the panel closes; the next canvas tap does not ink a spell | | |
| 5.9 | open the panel, then tap the canvas outside the rows | the panel closes and nothing is inked | | |
| 5.10 | right after picking a spell, keep the finger down and drag a little | the freshly inked strokes don't move. After lifting, a one-finger drag on a stroke moves it again | | |

## 6. Suspend / resume

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 6.1 | leave a cast spell on screen, press the power button (sleep), wait at least 10 s, wake | canvas comes back intact: strokes, snapped ring, status line. No garbage, and the animation doesn't replay | | |
| 6.2 | after waking, draw one more stroke | ink appears where you draw; recognition still runs | | |
| 6.3 | (optional) `ssh root@10.11.99.1 "journalctl -n 50 \| grep -i resume"`, or AppLoad's log if it captures stderr | `resume detected — redrawing from N strokes` | | |
| 6.4 | reboot the tablet, then `/home/root/wha/start-xovi.sh` | xovi starts, and AppLoad and Aetherquill come back | | |

## 7. Rollback (do this once, even if everything passed)

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 7.1 | close Aetherquill from AppLoad | stock UI; notebooks untouched | | |
| 7.2 | `ssh root@10.11.99.1 "sh /home/root/aetherquill-0.3.5/install.sh --uninstall"` | `removed ... /home/root/wha` (this deletes the spell log) | | |
| 7.3 | `ssh root@10.11.99.1 "ls /home/root/xovi/exthome/appload/; ls /home/root/wha"` | no `aetherquill` dir; `/home/root/wha` gone | | |
| 7.4 | reboot | stock tablet; AppLoad no longer lists Aetherquill | | |
| 7.5 | `ssh root@10.11.99.1 "rm -rf /home/root/aetherquill-0.3.5 /home/root/aetherquill-0.3.5.tar.gz"` | bundle leftovers gone | | |
| 7.6 | re-install (step 1.3) if you're keeping it | — | | |

Full stack rollback, including xovi and Vellum: `rm -rf /home/root/.vellum /home/root/xovi`, then reboot (README step 1).

## When you're done

Tell Claude which rows failed. Only after that can anything be called
"verified on hardware". Record the date in CLAUDE.md's phase history.
