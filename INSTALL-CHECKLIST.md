# First on-device run: checklist

⛔ **Run this yourself.** Nothing here has been verified on hardware. Every
box below is a claim the off-device tests can't prove. Tick each one, and
note anything odd in the "Notes" column. Stop at the first ✗ and use the
rollback section.

**Before you start:** RM2 on OS 3.27.x (Settings → About; 3.27.3.0 is the one
the stack was checked against). It's connected over USB, `ssh root@10.11.99.1`
works, and the Vellum stack (xovi + xovi-extensions + appload) is installed
(README step 1). You have the bundle `aetherquill-0.3.2.tar.gz`, from the
GitHub release or from `bash scripts/make-bundle.sh 0.3.2`.

## 1. Install

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 1.1 | `ssh root@10.11.99.1 "df -h /home"` | several GB free | | |
| 1.2 | `scp aetherquill-0.3.2.tar.gz root@10.11.99.1:/home/root/` | copies, no error | | |
| 1.3 | `ssh root@10.11.99.1 "cd /home/root && tar xzf aetherquill-0.3.2.tar.gz && sh aetherquill-0.3.2/install.sh"` | prints `installed to /home/root/xovi/exthome/appload/aetherquill` | | |
| 1.4 | `ssh root@10.11.99.1 "ls -l /home/root/xovi/exthome/appload/aetherquill /home/root/wha"` | `aetherquill` (rwx) + manifest; `start-xovi.sh` + `spells/` | | |
| 1.5 | `ssh root@10.11.99.1 /home/root/wha/start-xovi.sh` | "already loaded", or the screen flickers and xochitl comes back | | |
| 1.6 | Open AppLoad in the tablet UI | "Aetherquill" is listed | | |
| 1.7 | Launch it | white canvas, left sidebar (pen, eraser, ↶, ↷, ✕), "Draw a spell ring to begin" | | |

## 2. Cast all five elements

For each: tap ✕ to clear, draw a big ring (about half the screen width),
draw the sigil in the center, then close the ring. Sigil shapes are in
`service/vendor/wha/src/dictionary/sample-spells.json` or the
[web demo](https://ytnrvdf.github.io/wha-spell-simulator).

| # | Element | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 2.1 | fire | ring snaps to a circle · status `Active spell - fire  q..% s..%` · rising-flame animation · `fire` under the seal | | |
| 2.2 | water | same, ripples + droplets, `water` | | |
| 2.3 | wind | same, curling spiral arms, `wind` | | |
| 2.4 | earth | same, rising square debris, `earth` (hard to trigger: earth's 22-stroke template is fussy upstream too) | | |
| 2.5 | light | same, pulsing beams, `light` | | |
| 2.6 | `ssh root@10.11.99.1 "tail -n 5 /home/root/wha/spells/log.jsonl"` | one JSON line per activation above | | |

## 3. Cast one modifying sign

Clear, draw ring + fire sigil, then draw **one** sign (column, levitation or
convergence) in the outer band **directly below the seal**, about ¾ of the
way from the center to the ring, and smaller than the sigil. This is the
placement upstream recognizes reliably. Close the ring last.

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 3.1 | sign: ______ | status line 2 starts with the sign name and a %, e.g. `levitation 62%  force 91 spread 53 range 66 grav 74` | | |
| 3.2 | watch the activation | the fire effect plus the sign overlay: column = ladder-like pillar above the seal, levitation = chevrons climbing upward, convergence = spokes aimed at a point inside the ring | | |
| 3.3 | settle frame | `fire` with the sign name in smaller text below it | | |
| 3.4 | sign not recognized? | line 2 shows a warning such as `symbol_near_layer_boundary` · move the sign further into the outer band and retry | | |

## 4. Suspend / resume

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 4.1 | leave a cast spell on screen, press the power button (sleep), wait ≥10 s, wake | canvas comes back intact: strokes, snapped ring, status line. No garbage, and the animation does not replay | | |
| 4.2 | after waking, draw one more stroke | ink appears where you draw, recognition still runs | | |
| 4.3 | (optional) `ssh root@10.11.99.1 "journalctl -n 50 | grep -i resume"`, or AppLoad's log if it captures stderr | `resume detected — redrawing from N strokes` | | |
| 4.4 | reboot the tablet, then `/home/root/wha/start-xovi.sh` | xovi starts, AppLoad and Aetherquill come back | | |

## 5. Rollback (do this once even if everything passed)

| # | Do | Expect | ✓/✗ | Notes |
|---|---|---|---|---|
| 5.1 | close Aetherquill from AppLoad | stock UI, notebooks untouched | | |
| 5.2 | `ssh root@10.11.99.1 "sh /home/root/aetherquill-0.3.2/install.sh --uninstall"` | `removed ... /home/root/wha` | | |
| 5.3 | `ssh root@10.11.99.1 "ls /home/root/xovi/exthome/appload/; ls /home/root/wha"` | no `aetherquill` dir; `/home/root/wha` gone | | |
| 5.4 | reboot | stock tablet; AppLoad no longer lists Aetherquill | | |
| 5.5 | `ssh root@10.11.99.1 "rm -rf /home/root/aetherquill-0.3.2 /home/root/aetherquill-0.3.2.tar.gz"` | bundle leftovers gone | | |
| 5.6 | re-install (step 1.3) if you're keeping it | — | | |

Full stack rollback, including xovi/Vellum: `rm -rf /home/root/.vellum /home/root/xovi` + reboot (README step 1).

## When you're done

Tell Claude which rows failed. Only then can anything be called "verified on
hardware". Update CLAUDE.md's phase history with the date.
