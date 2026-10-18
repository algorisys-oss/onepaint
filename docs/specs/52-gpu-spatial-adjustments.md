# 52: Spatial adjustment layers on the GPU

## Status
Phase 1 built (2026-10-18): Gaussian blur, motion blur and sharpen, live on
the GPU when the layer is the document's only spatial layer and sits at the
top level. Phase 2a: emboss, mosaic, ripple, halftone and crystallize too
(`kind_px` in `spatial.wgsl`, a port of `stylize.rs` / `distort.rs`).
Everything else renders on the CPU as before.

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
  layer (`live_spatial`: the document's only spatial layer, top level, a
  GPU kind, reach within a tile) unrendered, so a stroke below it redraws
  it every frame. Tiles that fall back to the CPU, and every other spatial
  layer, use the rendered document as before. Dirty rects grow by a
  spatial layer's reach, as for feathered masks.
- **Limits of phase 1:** stacked spatial layers and ones in groups render
  on the CPU; a program with a spatial layer runs unsplit (the fill pass
  can't read filtered tiles); reduced levels scale the radius and round the
  region to level pixels.
- `ONEPAINT_BLUR=12` adds a blur layer for screenshots.

## Phases
1. Oracle in `compose`; `Op::Spatial` with the CPU interpreter and program
   tests; Gaussian blur, motion blur, sharpen on the GPU; engine wiring.
2. The other kinds, each with its GPU pass checked against the oracle.
   - 2a ✓ emboss, mosaic, ripple, halftone, crystallize (one pass).
   - 2b bloom, oil paint, line art (several passes over the region).
   - Not planned for the tile path: wind and glitch hash with `sin` of
     large arguments, which GPUs round differently (divergence 17), and the
     kinds with a whole-canvas reach (radial, spin, tilt-shift, iris and
     field blur, twirl, pinch, polar, chromatic aberration) need the whole
     backdrop at once, a design of their own.
3. Spatial layers inside groups (the group's register as the backdrop).

## Acceptance criteria
1. Programs with `Op::Spatial` run on the CPU equal the oracle composite
   exactly, on random documents with blur, motion blur and sharpen layers.
2. The GPU matches the CPU oracle within the usual tolerance.
3. Painting under a blur layer updates the blur during the stroke.
