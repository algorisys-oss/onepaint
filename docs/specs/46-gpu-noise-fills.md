# 46: Noise, screentone and fill layers on the GPU; adjustments and split caching

## Status
Built. Noise, screentone, solid fill and gradient fill layers composite on
the GPU, on top of the 13 kinds in spec 42. Pattern fills and the "sand"
screentone shape still keep their tiles on the CPU. Sand hashes with
`sin`, whose low bits differ on the GPU.

## Model
- These kinds depend on the pixel's position. `Adjust` now carries its
  tile's origin, and the planner prepares with the document frame, as
  `draw_adjustment` does. The CPU interpreter passes the real pixel to
  `color_px`, so it stays exact.
- Codes:
  - 14, noise: the same PCG hash on integer doc pixels (seeds 51–53),
    mono or per channel;
  - 15, solid fill: the linear colour, alpha 1, ignoring the backdrop;
  - 17, screentone (dot, line, cross; flat, luminance or gradient
    density; soft or hard dots):
    - `tone`'s maths, with the angle's cos and sin, the cell size, the
      gradient direction and its extent, and the ink colour all computed
      on the CPU;
  - 16, gradient fill:
    - `shape_t` ported for all five shapes, with its end points in doc px;
    - the fill's 1024-entry `Ramp` in the adjustment table, read with the
      same manual lerp;
    - the result premultiplied.
- Above level 0, the position is the level pixel times 2^L (a preview).

## Adjustment layers and split caching (spec 38)
- A split keeps the layers below the hot unit as an f16 image. Several
  colour kinds stretch small differences into visible ones: hue and
  saturation near grey (the hue is ill-defined), threshold, posterize and
  levels.
- A random document split at a layer under a top-level hue/saturation
  layer differed from the oracle on 0.5% of its pixels, by up to 0.08.
- So `split` returns no split when the hot unit, or any unit above it, is
  an adjustment layer. Such tiles composite in f32 every frame, which is
  exact.
- Adjustments below the hot unit are part of the cached image, like any
  layer.
- Hard steps can still flip at isolated pixels that sit exactly on the
  step:
  - threshold and posterize, inside groups the planner still splits, where
    the image's rounding crosses the step;
  - screentone at a cell edge, where the GPU may fuse a multiply-add, so
    `floor` picks the other cell.
- In the tests that is at most 0.01% of pixels (4 of 90,000 in the worst
  seed; one pixel unsplit). Changing a screentone shape constant misses on
  75,000.

## Acceptance criteria
1. `gpu_adjust` packs 17 kinds. Sand screentone, pattern fills, spatial
   kinds and generators return `None`.
2. Random documents with all 17 kinds equal `composite_tile` exactly on
   the CPU.
3. On the GPU, at most 0.01% of pixels exceed 2e-3: unsplit, split at an
   adjustment layer and at a plain one, cold and warm, and with paper.
4. An adjustment layer at or above the hot layer gives no split. Below it,
   the split stays.
