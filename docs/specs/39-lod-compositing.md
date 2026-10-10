# 39: Level-of-detail compositing

## Status
Built. Composite pyramid, atlas and cache mips (built when a level above 0
first needs them), per-level freshness, the present pass sampling the
display level, and idle refinement of previews at level 0.

The opacity-drag bench (8192² × 100 layers, Iris Xe, with the machine
loaded: load average 7–9 and browsers on the GPU) gave, in one run:

| Split drag frame | Plan + GPU |
|---|---|
| Level 0 (full resolution) | 77 ms |
| Level 3 (about fit-to-window) | 7.1 ms |

On the quiet re-measurement (spec 38's conditions: iGPU held at 500–650
MHz by its governor), the level-3 split drag frame took 6.2 ms when the
tiles fit the atlas and 4.6 ms past it, against 54 ms at level 0.

About 2 ms of the level-3 frame is planning. On a quieter run, level 0 had
measured 19.7 ms.

By eye: the demo page at 46% (level 1) matches the CPU compositor to
within one level once idle refinement has run.

Follows specs 13 (GPU compositor) and 38 (composite cache).
onepaint's own performance work; happypaint composites at full resolution.

## Why
After spec 38, a drag frame on 8192² × 100 layers costs about 18 ms of GPU
time: three reads and a write per pixel over 205 full-resolution tiles,
near the iGPU's bandwidth. At fit-to-window an 8k page shows at about 1/10
scale, so almost all of those pixels are thrown away by the present pass.
That pass then reads up to 36 texels per screen pixel to box-filter them.

Compositing at the level being displayed cuts the work by about 4^L at
level L (64× at 1/8 scale) and makes zoomed-out display cheaper and smoother.

## Model

### Levels
- Level `L` is 1/2^L scale: a 256² document tile is (256 >> L)² pixels.
  `L_MAX = 5` (8 × 8 pixels per tile).
- **The display level** is `D = clamp(floor(log2(1 / s)), 0, L_MAX)`, where
  `s` is document pixels to framebuffer pixels (view scale × pixels per
  point). Between levels the present pass minifies by at most 2×, which
  bilinear filtering handles without shimmer.

### Freshness (`apps/onepaint`, `canvas::Levels`)
Per level, the set of tiles whose content there is stale:

- An edit (`take_dirty_tiles`) or a document reset marks a tile stale at
  **every** level.
- Compositing a tile at level `L` on the GPU, then downsampling it
  (below), makes it fresh at `L..=L_MAX`.
- A CPU-composited tile is uploaded at level 0 and downsampled, so it is
  fresh at every level.
- Each frame, the canvas composites the tiles stale at `D`, visible first,
  within the frame budget it already has. Tiles stale only at finer levels
  wait until the view zooms in.

### Composite pyramid (`gpu`)
- The canvas composite texture gets `L_MAX + 1` mip levels.
- **Downsample pass** (`shaders/mip.wgsl`): for a list of tiles, each level
  `l` is the 2 × 2 box average of level `l − 1`, in premultiplied linear
  light. That average is the exact display filter. One dispatch per level;
  each reads level `l − 1` and writes level `l`. These are different
  subresources, so one pass may do both.
- After each frame's composites and uploads, the touched tiles are
  downsampled from their level up to `L_MAX`.

### Sources at a level
- The tile atlases get `L_MAX + 1` mip levels per slot. After a batch's
  uploads, the downsample pass fills levels 1..=`L_MAX` of the new slots,
  stroke buffers included.
- A level-`L` job reads `textureLoad(atlas, p, slot, L)`, with `p` in
  `0..(256 >> L)`, and writes mip `L` of the target. Its dispatch is
  `max(1, (256 >> L) / 8)²` workgroups.
- **Segment images (spec 38)** are keyed on `(segment key, L)` and stored in
  mip `L` of the cache texture, which gets mips too. Every pass is
  dispatched for a single level, so the fill and composite passes bind the
  storage view of that level.
- The mips add a third to the atlas memory. The budgets (1 GiB of sources,
  256 MiB of cache) are counted including the mips.

### Present
`canvas.wgsl` samples the composite at the display level `D` with
`textureSampleLevel`, linear filtering, instead of the up-to-36-tap box
filter. At `s >= 1`, `D = 0` and nothing changes.

## Precision
Blending at reduced resolution is not the same as blending first and then
downsampling, because "over" multiplies by alpha. Level `L > 0` is
therefore a **preview**: close, but not exact.

- **Level 0 stays exact** against the CPU oracle (2e-3, spec 13). It is
  what the user sees at 100% and above. Export, the eyedropper and filters
  use the CPU composite, as before.
- On a level-`L` composite of uniform-colour tiles, every box average equals
  its texels, so it matches the downsampled oracle within f16 rounding.
- On random documents, a level-`L` composite differs most at hard edges:
  masks, non-normal modes and noisy content. Across random documents the
  mean error is ≤ 0.015 and the 99th percentile ≤ 0.15 (the worst pixel
  reaches about 0.35 at level 1). Tests guard the mean and percentile.
- **Refinement.** Because of that, a tile composited above level 0 is a
  "rough" preview. Once nothing is stale at the display level, idle frames
  composite the rough tiles at level 0, visible first, and downsample them.
  The settled view is therefore exact, as with Krita's Instant Preview. The
  CPU path composites at level 0 and downsamples, so it is always exact.

## Acceptance criteria
1. **Freshness** (`canvas::Levels`, pure unit tests):
   - A dirty tile is stale at every level.
   - Compositing it at L leaves it stale only at levels below L.
   - A CPU upload makes it fresh everywhere.
   - A reset marks every tile stale at every level.
   - `D` follows the formula above at the scales 2, 1, 0.75, 0.5, 0.3, 0.1
     and 0.01.
2. **Downsample pass:** a level-0 composite downsampled to every level
   matches the box-averaged CPU oracle within f16 rounding.
3. **Atlas mips:** a level-L composite of uniform-colour tiles matches the
   oracle at level L within 2e-3, cold and with segment caching.
4. **Preview bound:** random documents (all modes, groups, clips, masks,
   strokes) at L = 1..3 keep a mean error ≤ 0.015 and a 99th percentile
   ≤ 0.15 against the downsampled oracle; idle refinement brings them back
   to level-0 exactness.
5. **No regression:** at `D = 0` the canvas output is unchanged; the
   spec 13 and 38 GPU tests still pass.
6. **Bench** (`opacity_drag`, a new level-3 variant): a drag frame at L = 3
   takes 4 ms or less on the reference iGPU.
7. **By eye:** zoomed-out views show no shimmer on thin lines while panning,
   and zooming in refreshes the finer level visible-first, without blank
   tiles that stay blank.

## Out of scope
- Drawing a stale finer level from a fresh coarser one while it refreshes.
  The finer level shows its previous content until refreshed, as level 0
  does today.
- Tiling the composite for documents above the GPU texture limit: spec 40.
