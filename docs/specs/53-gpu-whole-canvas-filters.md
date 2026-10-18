# 53: Whole-canvas filter layers on the GPU

## Status
Phases 1–3 built (2026-10-19): twirl, pinch, polar coordinates, chromatic
aberration, radial and spin blur, and the Blur Gallery (tilt-shift, iris,
field) run live on the GPU. Follows spec 52, whose tile path can't take
these kinds.

## Why
Radial, spin, tilt-shift, iris and field blur, twirl, pinch, polar
coordinates and chromatic aberration read their backdrop anywhere: along a
ray to a centre, through a coordinate map, or from half-resolution blurs of
the whole image (`filters::reach` is `GLOBAL_REACH`). Spec 52 plans a tile's
backdrop over the tile ± reach, so these stay on the CPU: the engine
flattens and filters the whole document, and mid-stroke it shows the stale
result until the stroke ends.

## Model
One **whole backdrop image** per live layer: its container's backdrop, the
size of the document at the current level, filtered once into a **whole
filtered image** that every tile's composite reads.

- **Oracle:** unchanged. `compose::spatial::apply` already filters a
  `GLOBAL_REACH` kind over the whole document (f16 backdrop in, f16 out).
- **Planner:** a live whole-canvas layer becomes `Content::spatial` with a
  `SpatialSrc` whose backdrop is every tile of the document. Planning it per
  tile would be quadratic, so `plan_tiles_cached` plans every whole-canvas
  layer first (`plan_wholes`, by layer id) and every tile shares one
  `Arc<Whole>`: the backdrop programs, kind, params, frame and a key over
  the backdrops' segment keys. Planning it lazily from inside the parallel
  tile loop could deadlock under work-stealing, hence the pass of its own.
  The CPU interpreter filters the whole backdrop once (`OnceLock`) and each
  tile reads its part.
- **GPU** (`gpu::whole`, `whole.wgsl`), before the frame's batches, when
  the layer's key changed at this level:
  1. The backdrop tiles whose segment changed fill segment cache slots, in
     chunks the cache holds, and are copied into the **backdrop image**
     (the image remembers each tile's segment key, so a stroke refills and
     copies only its own tiles).
  2. One pass over the backdrop image writes the filtered image; the Blur
     Gallery runs its half-resolution passes first (five `Rgba32Float`
     images at half size: two scratch, three levels).
  3. Composites read the filtered image at the tile's offset (binding 18,
     `F_SPATIAL | F_WHOLE`).
  Both images are `Rgba16Float` at the level's size and persist across
  frames, so a frame with no change below the layer costs nothing extra.
  (A first version read the backdrop tiles straight from their cache slots
  through a slot grid; it needed every tile resident at once, which a
  device with 256 array layers can't hold at 4096².)
- **Mid-stroke:** a refilter costing more than `INTERACTIVE_READS` texel
  reads (about 8 ms on the integrated GPU it was measured on) keeps the
  layer's image until the stroke ends, as the CPU path keeps its stale
  render; the app says when a stroke is in progress
  (`GpuCompositor::set_interactive`). Measured per refilter: twirl 4096²
  12.5 ms; radial blur 4096² 173 ms; tilt-shift 2048² 56 ms, with bokeh
  111 ms. So the maps stay live mid-stroke; the many-tap blurs wait in
  large documents.
- **Single tiles:** `plan_tile` leaves a whole-canvas layer to the CPU; it
  is planned once per call in `plan_tiles_cached` (before the tiles, so
  parallel tiles never wait on one another).
- **Kinds:** twirl, pinch, polar coordinates and chromatic aberration (one
  bilinear tap or three), radial and spin blur (96 and 64 jittered taps),
  then the Blur Gallery: tilt-shift, iris and field blur (three half-res
  blur levels, optional bokeh disc, a per-pixel blend). Sampling ports
  `filters::tex` exactly: bilinear by hand with clamp-to-edge, no sampler.
  Glitch (`sin` hashes, divergence 17), light rays and lightning stay on the
  CPU.
- **Levels:** above level 0 the image is the level's size, rounded down as
  the mips and the target are (rounding up let clamped taps read an edge
  texel half off the document: polar's corners at level 3); positions are
  fractions of the document, so they carry over, and pixel radii scale by
  2^−level, as in spec 52.
- **Engine:** `live_spatial` admits a whole-canvas layer when it is the
  document's only spatial layer (no stacking yet), top level or in isolated
  groups, and the document is at most `MAX_WHOLE_PX` (4096²: two f16
  images of 128 MiB each at level 0), or for the Blur Gallery
  `MAX_GALLERY_PX` (2048²: its half-size images add 80 MiB). Dirty rects for
  a whole-canvas layer are the whole document, as they are now.

## Phases
1. ✓ Planner pass and `Whole`; the CPU interpreter; program tests against
   the oracle. GPU images and the twirl, pinch, polar and chromatic passes;
   engine wiring.
2. ✓ Radial and spin blur.
3. ✓ The Blur Gallery.
4. Stacking with other spatial layers (a whole layer over or under tile
   kinds), if it turns out to be worth it.

## Precision
GPU trig moves a twirl's taps by about a hundredth of a pixel, which shows
at hard edges, and pinch is singular at its centre (`sin^−amount` as r → 0):
the tests allow 0.15% of pixels past 2e-3, as spec 52 does for cell kinds.

## Acceptance criteria
1. Programs with a whole-canvas layer, run on the CPU, equal the oracle
   composite on random documents.
2. The GPU matches the oracle within the usual tolerance (2e-3, or the
   stepped count where a kind steps).
3. Painting under a twirl layer updates the twirl during the stroke.
4. Documents over `MAX_WHOLE_PX` render as before.
