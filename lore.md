# Spell Catalogue lore notes

Sources for the twelve catalogue spells in `device/assets/catalogue.json`.
Descriptions here are written for this project. Nothing is copied from the wiki,
and no artwork is included.

- **Wiki:** [Witch Hat Atelier Wiki](https://witch-hat-atelier.fandom.com/wiki/Spells).
  The plain fetcher gets HTTP 402, so pages were opened in the built-in browser.
  Opened on 2026-10-25 for C5-C12: *Spells*, *Signs Explained*, *Light Beam*,
  *Pyreball Seal*, *Phantasmal Fireball*, *Watershot Seal*, *Water Bolt*,
  *Floatglow Lamp*, *Skysoaring Seal*, *Sylph Shoes Seal*, *Rising Platform of
  Water* and *Light Tracer*. Opened in an earlier pass for C1-C4: *Grasping
  Wind* and *Wind Wall* (not re-opened). *Water Bolt*, *Phantasmal Fireball* and
  *Light Tracer* were opened only to rule them out.
- **Vendored dictionaries:** read-only ground truth in `service/vendor/wha/src/dictionary/`.
  They hold the sigil and sign templates (`sigils.json`, `signs.json`) and two
  named samples (`sample-spells.json`).
- **Readback:** every entry is checked by `service/catalogue-gen.mjs`, which runs
  it through the vendored JS pipeline, and by `device/src/catalogue.rs`, which
  replays it in the Rust port stroke by stroke, as the tablet does, and checks
  for the same signature.

| # | Spell | Sigil | Sign | Canon? | Strokes |
|---|---|---|---|---|---|
| C1 | Fire Shoot | `fire` | `column` | name from the vendored sample | vendored sample ring + signs, template sigil, 1 closing stroke |
| C2 | Water Orb | `water` | `levitation` | name from the vendored sample | built from dictionary templates |
| C3 | Converging Wind | `wind-directs-air` | `convergence` | **no**: named for this project | built from dictionary templates |
| C4 | Light Beam | `light` | `column` | **yes**: wiki *Light Beam* | built from dictionary templates |
| C5 | Pyreball Seal | `fire` | `levitation` | **yes**: wiki *Pyreball Seal* | built from dictionary templates |
| C6 | Watershot Seal | `water` | `column` | **yes**: wiki *Watershot Seal* | built from dictionary templates |
| C7 | Skysoaring Seal | `wind-directs-air` | `levitation` | **yes**: wiki *Skysoaring Seal* | built from dictionary templates |
| C8 | Converging Flame | `fire` | `convergence` | **no**: named for this project | built from dictionary templates |
| C9 | Converging Water | `water` | `convergence` | **no**: named for this project | built from dictionary templates |
| C10 | Wind Pillar | `wind-directs-air` | `column` | **no**: named for this project | built from dictionary templates |
| C11 | Floating Light | `light` | `levitation` | **no**: named for this project | built from dictionary templates |
| C12 | Converging Light | `light` | `convergence` | **no**: named for this project | built from dictionary templates |

## C1 Fire Shoot (fire + column)

**What it does:** shoots a jet of flame straight up from the glyph.

**Why:**
- The vendored sample `fire-column` is called "Fire Shoot" and described as
  shooting fire.
- Per the wiki's *Signs Explained*, column keystones send the magic straight up
  off the glyph in a pillar. Lopsided placement tilts it toward the busier side
  (manga ch. 3, p. 16).

**Strokes:** the sample's ring and column signs are used unchanged, scaled to
the canvas. Its fire sigil is swapped for the dictionary's fire template at the
same size: the sample's own sigil read as ambiguous (too close to light) on
about 3% of jittered redraws in the P4 eval. The owner approved the swap on
2026-10-25. One closing stroke traces the ring line at the end. The sample's ring is drawn as
separate segments, with gaps of up to about 41 px at that scale, so it reads as
a prepared spell until the ring is closed. Closing the ring is what casts it.

## C2 Water Orb (water + levitation)

**What it does:** lifts the water into a ball that floats above the glyph.

**Why:**
- The vendored sample `water-orb` is called "Water Orb" and described as a water
  sigil with levitating signs.
- Per the wiki's *Signs Explained*, levitation keeps the conjured element
  hovering over the glyph. Drawn on an object, it moves that object instead.

**Strokes:** built from the dictionary's water sigil plus one levitation sign
drawn directly below the seal. The vendored sample was dropped after three fixes:
once its ring was closed, upstream's own pipeline read one of its levitation
signs as a fire sigil ("Multiple sigils detected"). The owner chose the template
rebuild on 2026-10-25.

## C3 Converging Wind (wind + convergence)

**What it does:** draws the wind's force together onto a single point.

**Why:**
- Per the wiki's *Signs Explained*, convergence narrows a spell onto a single
  spot, and can pack loose material tighter.
- The dictionary's `wind-directs-air` is the wind sigil.

**Not a canon spell.** No spell on the wiki pairs wind with convergence alone:
- *Sylph Shoes Seal* alternates convergence and levitation keystones around a wind sigil.
- *Wind Wall* (Alaira's spell, debut manga ch. 28; checked 2026-10-25 as a
  possible C3) raises a ring of air that's hard to push through. Its glyph
  combines the air sigil with three sign types: inverted columns on one side,
  convergence on the other, and a semicircular sign near the sigil (ch. 28,
  p. 19). The semicircular sign isn't in the vendored dictionary, so a faithful
  Wind Wall can't be drawn here. The owner kept Converging Wind.
- *Grasping Wind* (manga ch. 14) pulls air toward the glyph, but the wiki lists it
  under **Pull** signs, a different sign that is not in the vendored dictionary.

The owner left the name and line to the build (`<ideate>`). They describe what
convergence does and claim no lore.

**Strokes:** built from the dictionary's wind sigil plus one convergence sign
drawn directly below the seal.

## C4 Light Beam (light + column)

**What it does:** raises a shaft of light from the glyph. Careful linework
keeps it from wavering.

**Why:**
- The wiki's *Light Beam* page names this pairing directly (light sigil, column
  keystones around it) and says drawing accuracy affects how stable the light
  is. It shows up in Agott's practice sessions.
- The wiki also notes that light magic counts as a branch of fire magic
  (manga ch. 29, p. 13).

**Strokes:** built from the dictionary's light sigil plus one column sign drawn
directly below the seal. The wiki's version rings the sigil with several column
keystones. One sign is enough for the recognizer to read it back.

## C5 Pyreball Seal (fire + levitation)

**What it does:** lifts the flame into a ball that hovers over the glyph.

**Why:** the wiki's *Pyreball Seal* page describes a fire sigil with levitation
keystones around it, making a floating fireball (debut manga ch. 8). *Signs
Explained* says levitation keeps the magic hovering above the glyph.

**Strokes:** dictionary fire sigil plus one levitation sign directly below the
seal. The wiki ring of several keystones is cut to one, same as C2-C4.

## C6 Watershot Seal (water + column)

**What it does:** sends a stream of water straight up from the glyph.

**Why:** the wiki's *Watershot Seal* page is a water sigil ringed by column
keystones, and shoots straight up when they are even (manga ch. 3, p. 16). With
one sign there is nothing to unbalance it.

**Strokes:** dictionary water sigil plus one column sign directly below the seal.

## C7 Skysoaring Seal (wind + levitation)

**What it does:** the wind pushes along the sign's direction, enough to carry
a sail-sized thing.

**Why:** the wiki's *Skysoaring Seal* page gives a wind sigil with a single
levitation sign (manga ch. 4). The page says "a wind sigil" and does not name
the variant, so the dictionary's `wind-directs-air` stands in. The wiki's sign
is long and runs across the glyph. Ours is the short dictionary template, so the
shape differs even though the pair matches.

**Strokes:** dictionary wind sigil plus one levitation sign directly below the seal.

## C8-C10, C12 non-canon pairs (fire/water/light + convergence, wind + column)

No wiki spell uses these pairs alone, so the names describe the effect.
**Converging Flame** (C8), **Converging Water** (C9), **Converging Light** (C12)
and **Wind Pillar** (C10) claim no lore. Per *Signs Explained*, convergence
focuses a spell onto a single point and column makes the magic rise in a pillar.

**Strokes:** the dictionary sigil plus one convergence (or column) sign
directly below the seal.

## C11 Floating Light (light + levitation)

**Not a canon spell.** The closest is the wiki's *Floatglow Lamp*, a light
sigil ringed by alternating float and column signs (it makes the object float
and glow). That uses two sign types, and float is a different sign from
levitation, so the name is not borrowed. Ours is light with levitation only:
a glow that hovers over the glyph.

**Strokes:** dictionary light sigil plus one levitation sign directly below the seal.

## Dropped combos

None. All twelve sigil x sign pairs (fire, water, wind-directs-air, light x
column, levitation, convergence) passed the JS gen, the Rust replay, and the
P4 eval (100/100 on held-out seeds, 20/20 on fresh seeds).

## Still open

- Earth is left out on purpose. `ring-earth` is ambiguous in the ground truth
  (see the known ground-truth quirks in LOOP.md).
- Signs drawn from the templates are recognized reliably only in the outer band
  directly below the seal. That's why C2–C12 use one sign each, placed there,
  instead of the wiki's full rings of keystones.
