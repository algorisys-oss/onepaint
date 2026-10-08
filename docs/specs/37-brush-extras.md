# 37: Dual brush, colour tips, grain movement, spacing jitter, colour drift

The sources are under `../happypaint/frontend/src/engine/brush/`:
`brush-engine.ts:173-450` (dab emission and the RNG order),
`stamp-renderer.ts:138-268` (the stamp shader) and `brush-tips.ts:291-409`
(the colour daubs). No happypaint tests cover these features; onepaint's are
in `crates/brush/tests/tip_extras.rs`.

## Dual brush (`maskTip {id, strength}`)
- **Shape:** a second image tip, sampled in the same dab-local frame as the
  main tip, so it follows radius, aspect, rotation and symmetry.
- **Formula:** `a *= mix(1, mask, strength)`, after the tip and before
  grain, paper and selection.
- **Ignored:** auto tips (round, soft, square) and strength 0.
- **Imports:** ABR imports link the dual tip at 70% (spec 23).
- **Presets:** `textured-round` (bristle, 85%).

## Colour (RGBA) tips (`oil-daub`, `impasto-daub`)
- **Assets:** straight sRGB pixels baked by `scripts/export-brushes`
  (`*.rgb.png`), plus the tip's coverage-weighted Rec.601 luma
  (`COLOR_TIPS`).
- **Recolour:** `tipRecolor {hue (turns, ±0.5), sat, val (0–2)}` is an HSV
  edit of the tip pixels.
- **Baked mode** (`useTipColor`, as in `rgba-oil`): the tip's own colour.
  The brush colour and its colour dynamics are ignored.
- **Tint mode** (as in `thick-paint`):
  `base = c·mix(1, clamp(L/lum, 0, 2), .85) + (tip − L)·.85·.5·mix(.25, 1, sat(c))`,
  in sRGB, then linearised.

## Grain movement (`grain.move`, 0–1)
- **Offset:** each dab's grain offset is `(sample − first sample)·move`,
  taken before scatter and shared by symmetry copies.
- **Sampling:** the grain is read at `docPos − offset`.
- **Presets:** clouds 0.15, smoke 0.2, nebula 0.1.

## Spacing jitter (`spacingJitter`, 0–1)
- **Gap:** `gap·max(0.15, 1 + (r·2 − 1)·sj)`.
- **Draws:** one per spacing call (first point, then after each stamp),
  drawn while sampling and before the batch's per-dab draws, which is
  happypaint's order.
- **Presets:** starfield, lightning, foliage and grass.

## Colour drift
- **Drive:** `drive` is the fade (dab index/50) or pressure
  (`hueShiftSource`).
- **Two-colour load:** `toColor` lerps the sRGB colour toward it by `drive`
  before HSV.
- **Hue shift:** `hueShift` adds `shift·360°·drive` after the hue jitter.
- **RNG:** neither consumes random numbers.

## Brush studio
- **Jitter:** Space·J, Hue→Shift, and Tip col (toggle; its swatch takes the
  current paint colour).
- **Grain:** Movement.
- **Dual brush:** a second-tip picker and Strength.
- **Colour tip:** Tip colour, Tip hue, Tip sat and Tip bright, for colour
  tips only.
