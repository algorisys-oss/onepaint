# 42: Colour adjustment layers on the GPU

## Status
Built for the 13 colour kinds that depend only on the pixel's colour:
- brightness/contrast, hue/saturation, black & white, invert;
- threshold, posterize, levels, curves;
- colour balance, gradient map;
- colour to alpha, replace colour, selective colour.

Noise, screentone and the three fill kinds still keep their tiles on the
CPU, as do spatial kinds that aren't rendered to pixels first.

Follows spec 13 (GPU compositor) and spec 38 (split programs).
happypaint runs these as WebGL shaders (`adjustment-renderer.ts`). The
maths here follows the CPU reference, `filters::Prepared::color_px`.

## Why
Any visible colour adjustment layer sent every tile under it to the CPU
compositor. One curves layer over a large page made the whole page
composite on the CPU, and a live filter preview did the same.

## Model

### Packing (`filters::gpu`)
`gpu_adjust(kind, params)` returns `None` for kinds that stay on the CPU.
Otherwise it returns:
- a kind code (1–13);
- 68 floats of parameters;
- for curves and gradient maps, the same 256-entry `Lut` the CPU samples.

Selective colour packs its show-range flag and sample count, then up to
eight samples as (target HSV, reach) and (delta HSV, falloff). These are
the samples `sc_samples` derives, with hue in turns.

### Program (`compose::program`)
- `Op::Adjust { adjust, mask, mode, opacity }` mirrors `draw_adjustment`:
  adjust the top register, multiply by the mask's R, then blend back with
  the layer's mode and opacity.
- An invisible layer, or one whose mask has no tile here, plans to
  nothing, as on the CPU. Non-colour kinds plan to nothing too, because
  the tree compositor skips them.
- A colour kind without a GPU form makes the tile `Unsupported`.
- At the top level an adjustment is its own unit, and it never folds into
  a suffix image (it reads the backdrop). It can be the hot unit.
- The CPU interpreter runs `Prepared::color_px`, so `run` equals
  `composite_tile` exactly.
- Segment keys hash the code, params, LUT and mask.

### GPU (`compose.wgsl`)
- Each batch uploads a table of vec4s: each distinct adjustment's params
  once, plus its LUT as 256 vec4s (u8 / 255), bound at 15.
- The op holds the params offset, the LUT offset and the code.
- `adjust()` ports each kind line for line: unpremultiply, convert to sRGB
  (the "work" space), apply the kind, convert back and premultiply. The LUT
  lookup is the same texel-centred bilinear read as `lut_sample`.
- The CPU converts with tables (`fast_*`, error under 1e-4), the GPU with
  the exact curve. Both stay well inside the oracle tolerance.

## Acceptance criteria
1. `gpu_adjust` packs exactly the 13 kinds. Noise, tone, fills and spatial
   kinds return `None`.
2. Programs with random adjustment layers (all 13 kinds, any mode and
   opacity, masked or not, in groups and at the top level) equal
   `composite_tile` exactly on the CPU, over 60 seeds.
3. Noise, tone and fill adjustment layers still plan as `None`.
4. On the GPU, those documents match the CPU oracle within 2e-3 (spec 13),
   over 40 seeds. Split at an adjustment layer and at a plain one, they
   match too, cold and warm. Flipping the sign of the hue shift in the
   shader fails the test.

## Out of scope
- Noise, screentone and fill layers on the GPU, which depend on position.
- Spatial adjustment layers (a padded backdrop pass).
