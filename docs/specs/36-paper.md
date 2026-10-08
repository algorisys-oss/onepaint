# 36: Document paper

The sources are under `../happypaint/frontend/src/`:
- `engine/paint-engine.ts:194-200, 3128-3153` (`PaperSettings`, `setPaper`)
- `utils/paper-presets.ts`
- `engine/brush/stamp-renderer.ts:183-225` (the tooth bites)
- `engine/compositor.ts:214-233` (the visible tooth)
- `engine/wet/wet-renderer.ts` (`PAPER_*`, `dryGate`)

## Model (`doc::paper`)
- **`Paper { grainId, scale?, strength }`:** saved as `doc.paper` in
  `.happypaint`. Absent means smooth paper.
- **Ranges:** scale is 0.25 to 4 (absent = 1); strength is 0 to 1.
- **Presets:**

  | Preset | Grain | Strength | Scale |
  |---|---|---|---|
  | Smooth | none | – | – |
  | Rough | `rough-tooth` | 0.85 | 1 |
  | Fine grain | `paper` | 0.6 | 0.8 |
  | Canvas | `canvas` | 0.75 | 1.3 |
  | Linen | `linen` | 0.7 | 2.6 |

- **Preset matching:** the preset is found by grain. Legacy `rough` maps to
  Rough; anything else is Smooth.
- **Texture:** papers use the brush grain tiles (256²), anchored at doc
  (0, 0) with no offset or rotation. They are repeat-sampled bilinearly, and
  one tile spans `256·scale` doc px.
- **Undo and replay:** `Command::SetPaper` is not an undo step, as in
  happypaint. Recordings capture it like any command, and a recording's
  start document carries the paper too (happypaint's recorder header leaves
  the initial paper out).

## Dry strokes: the tooth bites (`brush::paper::PaperTooth`)
- **Which strokes:** paint strokes only, never erase or a layer mask. A brush
  with `paperResponse` 0 (inks, pens, markers at 0.2) skips the pass. The
  wet-mixer path never bites.
- **Fill:** `fill = max(layerAlpha·0.55, smoothstep(.02, .25, height))`,
  read from the layer's current paint and relief under the dab. Paint
  already down fills the tooth.
- **Bite:** `bite = strength·0.55·response·(1 − fill)`.
- **Coverage:** `a *= max(smoothstep(bite − .25, bite + .05, paper), .12)`,
  applied after the brush grain term.

## The visible tooth (`brush::paper::shade_tile`, applied in `compose`)
- **Normal:** `n = normalize((l − r)·2, (u − d)·2, 1)` from paper taps 1 px
  apart, lit from `normalize(−.45, −.6, .66)`.
- **Shade:**
  `shade = mix(1, .55 + .45·(.35 + .65·lambert)·(.4 + .6·h), strength·0.42)`.
- **Applied:** `rgb *= shade^2.2` in linear (happypaint's float-document
  path), darken only, alpha untouched.
- **Scope:** once per composited tile, after the layer tree. It is on screen
  and in exports, thumbnails and the eyedropper. Merge-down and filter
  sources are unshaded (`surface = false`).

## Wet paint
- **What the sim gets:** the raw paper strength.
- **Absorption:** `soak = 1 + s·1.6·p` scales evaporation.
- **Resistance:** `resist = 1 − s·0.5·p` scales water diffusion.
- **Granulation:** `dc *= 1 + s·1.2·(2p − 1)`, re-applied once on force-dry.
- **Dry-brush gate:** on water and pigment deposits,
  `smoothstep(thr ± .08, p)` with `thr = grainThr − .6·(1 − dry·s)`.
- **Changing paper mid-session:** it updates the live wet paint at once.

## App
- **New canvas:** the dialog has a Paper row (default Smooth).
- **Paper panel:** Menu ▸ Paper… switches the preset live. For textured
  papers it adds Texture scale, 25–400% in steps of 5, applied on release.
  Switching preset resets the scale.
