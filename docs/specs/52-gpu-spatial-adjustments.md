# 52: Spatial adjustment layers on the GPU

## Status
In progress (2026-10-18). Phase 1: Gaussian blur, motion blur and sharpen
at the top level.

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
- **Engine:** the GPU path plans the document with such layers live, so a
  stroke below them redraws them every frame; tiles that fall back to the
  CPU use the rendered document as before.

## Phases
1. Oracle in `compose`; `Op::Spatial` with the CPU interpreter and program
   tests; Gaussian blur, motion blur, sharpen on the GPU; engine wiring.
2. The other blur kinds, then distort and stylize kinds, each with its GPU
   pass checked against the oracle.
3. Spatial layers inside groups (the group's register as the backdrop).

## Acceptance criteria
1. Programs with `Op::Spatial` run on the CPU equal the oracle composite
   exactly, on random documents with blur, motion blur and sharpen layers.
2. The GPU matches the CPU oracle within the usual tolerance.
3. Painting under a blur layer updates the blur during the stroke.
