# Spell Catalogue lore notes

Sources for the four catalogue spells in `device/assets/catalogue.json`.
Descriptions here are written for this project. Nothing is copied from the wiki,
and no artwork is included.

- **Wiki:** [Witch Hat Atelier Wiki](https://witch-hat-atelier.fandom.com/wiki/Spells),
  read on 2026-10-25 in the built-in browser. The plain fetcher got HTTP 402.
  Pages used: *Spells*, *Signs Explained*, *Light Beam* and *Grasping Wind*.
- **Vendored dictionaries:** read-only ground truth in `service/vendor/wha/src/dictionary/`.
  They hold the sigil and sign templates (`sigils.json`, `signs.json`) and two
  named samples (`sample-spells.json`).
- **Readback:** every entry is checked by `service/catalogue-gen.mjs`, which runs
  it through the vendored JS pipeline, and by `device/src/catalogue.rs`, which
  replays it in the Rust port stroke by stroke, as the tablet does, and checks
  for the same signature.

| # | Spell | Sigil | Sign | Canon? | Strokes |
|---|---|---|---|---|---|
| C1 | Fire Shoot | `fire` | `column` | name from the vendored sample | vendored sample + 1 closing stroke |
| C2 | Water Orb | `water` | `levitation` | name from the vendored sample | built from dictionary templates |
| C3 | Converging Wind | `wind-directs-air` | `convergence` | **no**: named for this project | built from dictionary templates |
| C4 | Light Beam | `light` | `column` | **yes**: wiki *Light Beam* | built from dictionary templates |

## C1 Fire Shoot (fire + column)

**What it does:** shoots a jet of flame straight up from the glyph.

**Why:**
- The vendored sample `fire-column` is called "Fire Shoot" and described as
  shooting fire.
- The wiki's *Signs Explained* says column keystones make a spell's magic come out
  as a column or beam above the glyph. If the signs are unbalanced, it leans toward
  the side with more of them (manga ch. 3, p. 16).

**Strokes:** the sample's strokes are used unchanged, scaled to the canvas. One
closing stroke traces the ring line at the end. The sample's ring is drawn as
separate segments, with gaps of up to about 41 px at that scale, so it reads as
a prepared spell until the ring is closed. Closing the ring is what casts it.

## C2 Water Orb (water + levitation)

**What it does:** lifts the water into a ball that floats above the glyph.

**Why:**
- The vendored sample `water-orb` is called "Water Orb" and described as a water
  sigil with levitating signs.
- The wiki's *Signs Explained* says levitation makes a spell's magic float above
  the glyph, or moves the object it is drawn on.

**Strokes:** built from the dictionary's water sigil plus one levitation sign
drawn directly below the seal. The vendored sample was dropped after three fixes:
once its ring was closed, upstream's own pipeline read one of its levitation
signs as a fire sigil ("Multiple sigils detected"). The owner chose the template
rebuild on 2026-10-25.

## C3 Converging Wind (wind + convergence)

**What it does:** draws the wind's force together onto a single point.

**Why:**
- The wiki's *Signs Explained* says convergence focuses a spell's magic down to
  one point, and can make loose particles hold together more firmly.
- The dictionary's `wind-directs-air` is the wind sigil.

**Not a canon spell.** No spell on the wiki pairs wind with convergence alone:
- *Sylph Shoes Seal* alternates convergence and levitation keystones around a wind sigil.
- *Wind Wall* adds inverted column signs and another sign.
- *Grasping Wind* (manga ch. 14) pulls air toward the glyph, but the wiki lists it
  under **Pull** signs, a different sign that is not in the vendored dictionary.

The owner left the name and line to the build (`<ideate>`). They describe what
convergence does and claim no lore.

**Strokes:** built from the dictionary's wind sigil plus one convergence sign
drawn directly below the seal.

## C4 Light Beam (light + column)

**What it does:** sends a beam of light up from the glyph. The neater the
drawing, the steadier the beam.

**Why:**
- The wiki's *Light Beam* page gives this exact pairing: a light sigil ringed by
  column keystones, making an upward beam that holds steadier when the glyph is
  drawn precisely. Agott uses it during drawing practice.
- The wiki also notes that light magic counts as a branch of fire magic
  (manga ch. 29, p. 13).

**Strokes:** built from the dictionary's light sigil plus one column sign drawn
directly below the seal. The wiki's version rings the sigil with several column
keystones. One sign is enough for the recognizer to read it back.

## Still open

- Earth is left out on purpose. `ring-earth` is ambiguous in the ground truth
  (see the CLAUDE.md quirk list).
- Signs drawn from the templates are recognized reliably only in the outer band
  directly below the seal. That's why C2–C4 use one sign each, placed there,
  instead of the wiki's full rings of keystones.
