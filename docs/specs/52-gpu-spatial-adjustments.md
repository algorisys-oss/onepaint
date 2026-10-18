# 52: Spatial adjustment layers on the GPU

## Status
Phase 1 built (2026-10-18): Gaussian blur, motion blur and sharpen, live on
the GPU when the layer is the document's only spatial layer and sits at the
top level. Phase 2a: emboss, mosaic, ripple, halftone and crystallize too
(`kind_px` in `spatial.wgsl`, a port of `stylize.rs` / `distort.rs`).
Phases 2b, 3a and 3b: bloom and line art, isolated groups, and stacked
layers. Everything else renders on the CPU as before.

## Why
A spatial adjustment layer (blur, sharpen, distort…) is rendered on the
CPU: the engine flattens everything below it over the whole document,
filters that, and composites the result as a pixel layer
(`Session::render_spatial_adjustments`). It is cached, and mid-stroke the
stale result is reused, so painting under a blur shows the blur only when
the stroke ends; any change below costs a whole-document flatten and
filter.

## Model
The segment cache (spec 38) already holds, per tile, the image of
everything below a unit: for a top-level spatial layer that is exactly its
backdrop.

- **Oracle** (`compose::spatial`): the engine's rendering, moved into
  `compose` so tests and the engine share it. The backdrop is flattened,
  stored as f16 tiles, filtered over its content plus the kind's reach
  (clipped to the document), and stored as f16 tiles again.
- **Planner** (`compose::program`): a visible, unclipped, top-level spatial
  layer whose kind has a GPU form and whose reach fits one tile becomes
  `Op::Spatial`. It carries the backdrop programs of its tile and of the
  neighbours within reach (the layers below it, planned for those tiles),
  the kind and params, and the layer's mask, mode and opacity. Elsewhere
  (in a group, too far a reach, other kinds) the tile stays on the CPU.
- **CPU interpreter:** runs the neighbour backdrops, quantizes them to f16,
  filters the padded region with `apply_spatial_in` and keeps the centre
  tile (f16 again). A region of tile ± reach gives the same pixels as the
  whole-document filter, because the kernels never read past their reach
  except at the document's edge, where both clamp.
- **GPU:** the backdrop programs fill cache slots; a filter pass assembles
  the padded region from them, runs the blur passes (happypaint's 65-tap
  kernel, `filters` `blur_pass`) and writes the filtered tile to a cache
  slot; the composite reads that slot like a layer's pixels (mask, vector
  mask, density, Blend If, mode, opacity).
- **Engine:** `Session::plan` uses `display_doc_gpu`, which leaves the live
  layers (`live_spatial`: every spatial layer, when each is top level or in
  isolated groups, of a GPU kind, with a reach within a tile, at most
  three of them (planning grows ninefold per stacked layer); otherwise
  none, so a stroke never shows a live layer over a stale one) unrendered, so a stroke below it redraws
  it every frame. Tiles that fall back to the CPU, and every other spatial
  layers, use the rendered document as before. Dirty rects grow by a
  spatial layer's reach, as for feathered masks.
- **Limits:** spatial layers in Pass Through groups render on the CPU; a program with a spatial layer runs unsplit (the fill pass
  can't read filtered tiles); reduced levels scale the radius and round the
  region to level pixels.
- `ONEPAINT_BLUR=12` adds a blur layer for screenshots.

## Phases
1. Oracle in `compose`; `Op::Spatial` with the CPU interpreter and program
   tests; Gaussian blur, motion blur, sharpen on the GPU; engine wiring.
2. The other kinds, each with its GPU pass checked against the oracle.
   - 2a ✓ emboss, mosaic, ripple, halftone, crystallize (one pass).
   - 2b ✓ bloom (bright pass, blur, screen) and line art, Sobel and XDoG
     (a luminance pass, blurs at σ and 1.6σ, then the threshold).
   - Oil paint stays on the CPU: its quadrant choice turns on variances
     that cancel to rounding noise, so a GPU picks another quadrant at
     edges and pixels jump by whole values.
   - Cell kinds (mosaic, halftone, crystallize) step at cell edges, where
     WGSL's division (2.5 ulp) can put a pixel in the next cell; the tests
     allow those pixels as they do Hard Mix's, crystallize ten times as
     many (its site hash chains multiply-adds a GPU may fuse).
   - Not planned for the tile path: wind and glitch hash with `sin` of
     large arguments, which GPUs round differently (divergence 17), and the
     kinds with a whole-canvas reach (radial, spin, tilt-shift, iris and
     field blur, twirl, pinch, polar, chromatic aberration) need the whole
     backdrop at once, a design of their own.
3. Groups and stacking.
   - 3a ✓ a spatial layer in an isolated group: its backdrop is the
     group's children below it, planned the same way. In a Pass Through
     group the backdrop reaches outside the group; those tiles stay on the
     CPU.
   - 3b ✓ stacked spatial layers. Filtered tiles live in a texture array
     of their own (`cache_layers / 2` slots, binding 17), so a fill can
     read a lower layer's filtered tiles while it writes an upper layer's
     backdrop to the cache. `plan_spatial` stages the jobs by depth (1 +
     the deepest spatial layer in a backdrop); each stage's fills run, then
     its filter passes, then the next stage. A backdrop tile with a spatial
     layer of its own nests its `SpatialSrc`, so two stacked blurs on a
     tile plan the lower one's 3 × 3 neighbourhood for each of the upper
     one's tiles in reach (shared through the segment and filtered keys).
     Each pass of a submit now has its own params/jobs/ops buffers: the
     fill passes had shared one set, written by `queue.write_buffer` before
     the submit, so every fill ran the last stage's jobs.

## Acceptance criteria
1. Programs with `Op::Spatial` run on the CPU equal the oracle composite
   exactly, on random documents with blur, motion blur and sharpen layers.
2. The GPU matches the CPU oracle within the usual tolerance.
3. Painting under a blur layer updates the blur during the stroke.
