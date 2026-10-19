# 63: Loading the Mixer from the canvas

## Status
Built (2026-10-19): Alt-click (or Load from canvas, then a tap) with the
Mixer loads the brush from what is under it.

## Source
happypaint `engine/brush/smudge-renderer.ts` (`RESERVOIR_SIZE`,
`loadPatch`, the stamp and contaminate shaders), `engine/paint-engine.ts`
`loadMixerPatch` (2960–2972), `clearMixerPatch`, `beginSmudge`
(3036–3083), `engine/brush/mixer-patch.ts` and its tests,
`components/canvas/paint-canvas.tsx` 5561–5565 and 2553–2567.

## Model (`brush::reservoir`)
- `Reservoir::Flat` is the old single colour; `Reservoir::Patch` is a
  64² grid of straight colour plus the amount of paint each part of the
  brush carries. A patch taken half over transparency is half empty.
- `from_canvas` loads the square one brush diameter wide around the
  point from the composite (all visible layers, as on screen): each texel
  averages 4 × 4 bilinear taps of its footprint and is un-premultiplied
  (texels under 0.002 coverage are empty). Mean coverage under 0.001
  loads nothing: "Nothing under the brush to load".
- A dab samples the patch over its own square, axis-aligned (it does not
  turn with the tip) and stretched with the dab's radius, filtered like a
  LINEAR clamp-to-edge texture. Each pixel's reservoir weight is
  `(1 − mix) · deposit · texel amount`, so empty parts only smear.
- Contamination drifts every texel toward the pickup's average at the
  stroke's rate; an empty texel takes the pickup colour outright and
  starts to fill. With nothing picked up the patch is left alone.

## Session
- `Command::LoadMixerPatch { x, y, size }` (refused mid-stroke) and
  `Command::ClearMixerPatch`. A colour change and Clean drop the patch;
  changing tool doesn't (it only disarms "tap to load").
- Each stroke start reloads from the patch when one is loaded, else from
  the colour. Keep colour holds the reservoir while that source is
  unchanged: the key is `patch:<generation>` or the colour's hex.
- happypaint recordings replay `loadMixerPatch` and `clearMixerPatch`.

## UI
The Mixer panel shows what the brush holds (a kept reservoir, else the
patch) as an 8 × 8 swatch (`ui_kit::widgets::patch_swatch`), next to Load
from canvas. Errors show as a toast.

## Divergence
54: the patch is built from a box of bilinear taps per texel, not
happypaint's two bilinear blits (the square into 256², then to 64²).
Large brushes alias less; colours agree otherwise.

## Tests
`brush::reservoir` (orientation, un-premultiplying, coverage-weighted
swatch, empty squares, contamination), `brush/tests/smudge.rs`
`a_loaded_patch_paints_its_layout`, `engine/tests/smudge.rs`
`mixer_loads_a_patch_from_the_composite`, and the app's
`mixer_alt_click_loads_the_brush_and_keeps_the_colour`.
