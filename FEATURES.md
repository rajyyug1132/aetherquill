# Feature ideas

Nine ideas, each grounded in the repo at 0.3.6. None is built or checked on hardware.

## 1. Recast after undo and redraw

- **What:** emptying the page (CLEAR, undo to empty, erase to empty) now forgets the last cast, so redrawing the spell casts again (`session.rs`, test `emptying_the_page_forgets_the_last_activation`). One case is left: undo just the sigil and draw it again, and the signature matches the last cast, so nothing plays and nothing is logged. Decide whether that counts as a new cast. If so, forget the last cast whenever a stroke is removed and the spell stops being active.
- **User value:** People correcting a sigil get the animation and log line for the spell they finished, not silence.
- **Effort:** S. One reset on the session's undo/erase path, plus a host test.
- **Safety property:** none. No new files or paths. It only changes when the existing log line and animation fire.
- **Depends on unverified hardware behavior:** yes, mildly. The replayed animation on the panel has not been seen for this case.

## 2. Richer grimoire lines

- **What:** The log line holds element, quality, stability and an opaque signature. Add the sign names that were compiled in, a `source` field ("hand" or "catalogue"), and the recognition time in milliseconds, which `recognize()` already measures and prints to stderr. Keep the existing fields so older lines still parse. Move the line format into a pure function so a host test can check it.
- **User value:** `tail` on the log reads as a cast history. The timing field records recognition time for real casts, so the README's "about 0.35 s per pen-up" can be checked against use.
- **Effort:** S. One format function, a source flag set by the catalogue pick, and the timing carried through the recognition result.
- **Safety property:** none. Same file and directory; `--uninstall` still removes it.
- **Depends on unverified hardware behavior:** yes. The log write has not been confirmed on the tablet (INSTALL-CHECKLIST 3.6 is unchecked), and the timing values would be new data from the tablet.

## 3. Casts page on the tablet

- **What:** A sidebar panel, laid out like the Spell Catalogue, that lists the newest log lines (element and sign names), newest first. It reads `/home/root/wha/spells/log.jsonl` with serde_json, which `device/Cargo.toml` already depends on, and skips any line that does not parse. A missing or half-written file shows an empty list.
- **User value:** The grimoire can be read on the tablet instead of over SSH.
- **Effort:** M. A new panel, a last-N window, and a reader that tolerates a missing or partial file. Row hit-testing can reuse `catalogue::row_at`.
- **Safety property:** none. Read-only on the log. No writes and no new paths.
- **Depends on unverified hardware behavior:** yes. The log has not been seen on the tablet (INSTALL-CHECKLIST 3.6), and it stores epoch seconds. Until the tablet's clock is confirmed, show order only, no clock times.

## 4. Keep a drawn spell as a personal catalogue entry

- **What:** A save action in the sidebar writes the current strokes, in canvas coordinates and the catalogue's stroke format, to `/home/root/wha/spells/saved/`. It saves only after the recognizer reads those strokes back and gets an active spell, and the read-back signature is stored with the entry. Saved entries appear in the catalogue list beside the four built-in ones. With no keyboard, names are generated from the element and a number.
- **User value:** A cast that worked can be brought back and cast again, the way the built-in entries already work.
- **Effort:** M. Save UI, file I/O, merging the two lists, and making `Expected` in `catalogue.rs` optional, since it is required today and saved entries have no JS verdict.
- **Safety property:** low. Writes stay under `/home/root/wha`, and `rm -rf` removes them. The read-back gate is Rust-only, so the JS-parity rule that protects built-in entries does not cover saved ones. They are checked against the Rust output only.
- **Depends on unverified hardware behavior:** yes. Stroke replay on the tablet is unverified (INSTALL-CHECKLIST 5.2 to 5.5). The checklist already flags how slowly the 49-stroke entry inks, and a hand-drawn spell can have more strokes than that.

## 5. Eraser end from the raw digitizer

- **What:** `device/src/pen.rs` decodes only BTN_TOUCH from the digitizer. The README says the marker's eraser end is not detected and attributes that to AppLoad's qtfb bridge. The app already reads the Wacom digitizer directly, so if the tablet reports the eraser end as its own tool code, `pen.rs` could map it to erase mode while it is in contact. The sidebar toggle stays as the fallback for when the digitizer is not open.
- **User value:** Erase with the other end of the pen, as with a pencil, without switching tools.
- **Effort:** M. The decoding and its tests in `pen.rs` are small. The rest is in `main.rs`: temporary erase while the end is down, no ink during that time, restore on lift, and the fallback path.
- **Safety property:** none. Reads an input device the app already opens without a grab. Writes no files.
- **Depends on unverified hardware behavior:** yes. The repo does not say whether the digitizer reports the eraser end under a separate code. A person could check with a read-only dump of raw events on the tablet. If the code never appears, the idea is dead.

## 6. Sign band guide and plain warning hints

- **What:** A sidebar toggle, shown when a ring is on the page, draws two faint arcs at the outer band limits from `recognizer/src/config.rs` (normalized radius 0.66 and 0.94), so the user can see where signs read. The arcs are pixels only and never enter the stroke store, so the recognizer never sees them. Separately, the status line shows only the first warning. A small lookup in `spell_view.rs` can turn each warning identifier into a short hint in our own words, such as "sign sits on a band edge: move it into the middle of the outer band."
- **User value:** The placement the README calls most reliable becomes visible, and a warning gives a next step instead of an identifier.
- **Effort:** S. Geometry comes from existing constants. The hint lookup is a pure function with one test per warning identifier.
- **Safety property:** none. Pixels only; no files.
- **Depends on unverified hardware behavior:** yes for the arcs. How faint lines render and ghost on the panel is unknown, and turning the toggle off needs a redraw. No for the hint text, which uses the existing status line.

## 7. Trace practice on a catalogue spell

- **What:** A practice option in the catalogue panel loads one built-in spell and draws its strokes at screen scale as a faint guide. The user traces over it. Each pen-up runs the normal recognition. A trace counts as a match when its compiled signature equals the entry's `expected.signature`, the same value the device tests check. The status line shows match or no match. A miss costs nothing.
- **User value:** Practice against spells with a known answer, mainly for sigil shape and sign placement, with a clear pass signal.
- **Effort:** M. A guide layer that `redraw_all` does not keep, a practice state (which spell, attempts), and the match check.
- **Safety property:** none. In-app state only. Nothing is saved.
- **Depends on unverified hardware behavior:** yes. The guide has to be legible and must not ghost on the panel. The catalogue inking path has not been checked on the tablet (INSTALL-CHECKLIST 5.x), and a live trace comes through the digitizer path, which is its own open question.

## 8. Settle the watchdog

- **What:** LOOP.md ticks `watchdog-grimoire` as done, and CLAUDE.md lists a watchdog among the accepted scope additions. The tree has neither a watchdog nor `scripts/run-on-device.sh`, which both docs cite. CLAUDE.md describes a systemd unit; LOOP.md describes a backgrounded shell loop. The job was to restore xochitl if the app hung. That job was written for the takeover design. The windowed design under AppLoad does not stop xochitl (CLAUDE.md, 2026-07-10 pivot), so the job may not be needed. Either retire the claim in both docs, or build a home-directory script that closes a hung Aetherquill and nothing else, started by `start-xovi.sh`.
- **User value:** The docs stop promising a safety net that is not there. If one is rebuilt, a hung app can be recovered without a reboot.
- **Effort:** S to decide and fix the docs. M to build the home-directory version.
- **Safety property:** low for the home-directory script, which writes nothing outside `/home/root` and only signals Aetherquill. It breaks the rule if built as the systemd unit CLAUDE.md names, since TODOS says a unit has to go in `/etc/systemd/system`. Restarting xochitl would act on the stock UI service, so it is not recommended without a test.
- **Depends on unverified hardware behavior:** yes. Whether a hung AppLoad window blocks the stock UI on 3.27.3.0 is unknown. Only a test on the tablet can answer it.

## 9. Capture failed casts for host replay

- **What:** When a pen-up ends with a ring and no active spell, write the strokes in canvas coordinates (the form the recognizer receives) and the status lines to `/home/root/wha/spells/failed/`, keeping the newest 50 files. A host example, in the style of `effect_preview`, loads a captured file and runs `classify_drawing` and `compile_spell` on it.
- **User value:** A sign that failed on the tablet can be inspected on a laptop, and the drawing can be added to the jitter fixtures. On-device failures become test data instead of guesses.
- **Effort:** M. Serialize the existing stroke data, add the cap, and write the replay example. The capture is small; the example is most of the work.
- **Safety property:** low. New writes are bounded by the 50-file cap under `/home/root/wha/spells`, and `--uninstall` removes them. Drawings stay on the tablet until someone copies them off.
- **Depends on unverified hardware behavior:** yes. The capture records what the recognizer received, so the open question is whether the live pen path (INSTALL-CHECKLIST 2.x, not verified on the tablet) produces clean strokes in the first place.
