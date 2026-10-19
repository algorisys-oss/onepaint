# 46: Noise, screentone and fill layers on the GPU; adjustments and split caching

## Status
Built. Noise, screentone, solid, gradient and pattern fill layers
composite on the GPU, on top of the 13 kinds in spec 42. One case still
keeps its tiles on the CPU: the "sand" screentone shape, which hashes with
`sin`, whose low bits differ on the GPU. Imported pattern images (up to
512², 4 MiB of table) go in the adjustment table like the built-in 32²
tiles (2026-10-19); one that doesn't decode stays on the CPU, empty.

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
  - 18, pattern fill: the built-in 32 × 32 tile (`pattern_pixels`), or
    the imported image, in the adjustment table, sampled with `sample_repeat`'s wrap and bilinear,
    after the offset, rotation and scale;
  - 16, gradient fill:
    - `shape_t` ported for all five shapes, with its end points in doc px;
    - the fill's 1024-entry `Ramp` in the adjustment table, read with the
      same manual lerp;
    - the result premultiplied.
- Above level 0, the position is the level pixel times 2^L (a preview).

## Prepared once per layer
- With the session's `PlanCache`, an adjustment's prepared form and GPU
  packing are built once per kind, parameters and document frame (keyed
  on a hash of the kind and the params' JSON).
- Every tile shares them through `Arc`s; only `Adjust.at` is per tile.
  Before, every tile and frame rebuilt the curves LUT, the gradient ramp
  (1024 stop evaluations) or the pattern tile (16,384 coverage samples).
- On the GPU, a batch stores a shared adjustment once, found by address.

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
1. `gpu_adjust` packs 18 kinds. Sand screentone, undecodable imported
   patterns, spatial kinds and generators return `None`. The random test
   documents include imported, translucent, odd-sized pattern tiles.
2. Random documents with all 18 kinds equal `composite_tile` exactly on
   the CPU, planned with and without the cache. With the cache, each
   layer's adjustment is one shared `Arc` across tiles.
3. On the GPU, at most 0.01% of pixels exceed 2e-3: unsplit, split at an
   adjustment layer and at a plain one, cold and warm, and with paper.
4. An adjustment layer at or above the hot layer gives no split. Below it,
   the split stays.
