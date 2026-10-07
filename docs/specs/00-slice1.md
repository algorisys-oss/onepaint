# 00: Slice 1 (a paintable vertical slice)

## Goal
The app opens, and the user can do all of the following:

- Paint with pressure using the brush and the eraser.
- Pick a color.
- Add, delete, reorder and hide layers, and change their opacity and blend mode.
- Undo and redo.
- Pan, zoom and rotate the canvas.
- Export a PNG.
- Open and save `.happypaint`.

The UI uses the `ui_kit` look (`docs/design-system.md`).

## Architecture decisions for slice 1

1. **CPU tiles are the source of truth.** Pixels live in `raster::Surface`:
   sparse, copy-on-write `Arc` tiles of 256², stored as **RGBA16F,
   premultiplied, linear light**. That is happypaint's default document
   depth (`engine/renderer/gl-utils.ts:66`).
   - The GPU tile uploads take f16 directly, with no conversion.
   - Undo snapshots are `Arc` clones.
   - Everything stays testable against a deterministic CPU reference.
2. **Brush dabs are rasterized on the CPU** into a per-stroke buffer
   (`Surface`), then flattened into the layer at `endStroke`. This matches
   happypaint's stroke-texture model.
3. **Compositing is CPU, incremental and per tile** (`compose`), and it runs
   in parallel with rayon. The composite is cached per tile, and only dirty
   tiles are recomposited. During a stroke that means only the touched
   tiles.
4. **The GPU does display.** `gpu` keeps the composite as a GPU texture and
   updates only dirty tiles. A present pass draws it through the view
   transform (pan, zoom, rotate) over the checkerboard, with the
   linear→sRGB conversion done on present.
5. **Later (slice 2+):** GPU stamping and GPU layer compositing go behind
   the same `compose` and `brush` API, verified against the CPU oracle.
   This is how we reach the 8k × 100-layer target. Slice 1 is measured
   with benches so the switch can be justified with numbers.

## Out of scope for slice 1
Everything is still required for parity; it just comes in a later slice.

- Selections, fill, gradient, smudge/mixer/wet, liquify, text, vector,
  adjustments and filters.
- The `pigment` blend mode, which needs the Kubelka–Munk port.
- 8-bit and 32F document depths, plus the 8-bit fixed-multiply quirk.
- Grain/paper texture, image tips, scatter, colour jitter and airbrush
  ticks.
- Masks: they are loaded and saved, and used in compositing, but there is
  no painting into a mask yet.
- PSD, video, cloud, animation and multi-touch gestures.

## Acceptance
- `cargo test --workspace` and clippy (`-D warnings`) pass.
- `./dev-start.sh` opens the app, and all the goals above work by hand.
- `samples/examples/*.happypaint` from happypaint open, and their
  composite matches `*.preview.png` within a tolerance.

## Performance notes (measured 2026-10-08, release, Iris Xe laptop, 12 threads)

| Case | Full recomposite |
|---|---|
| 4096², 20 full-canvas layers (7 multiply), exact `powf` curves | 4.25 s |
| Same, with table-driven sRGB curves (`fast_*`, error under 1e-4) | 1.48 s |
| Same, all normal | 0.81 s |
| 8192², 10 full layers | 2.67 s |

- The CPU path is memory-bound: it reads each f16 layer tile per output tile
  (about 2.3 ns per pixel per layer).
- **Mitigation shipped:** progressive compositing. Each frame composites
  dirty tiles within a 10 ms budget, visible tiles nearest the centre first,
  so the UI never blocks.
- **The real fix:** GPU-resident layer tiles with GPU compositing. That is
  the next performance milestone, and it is required for the
  8k × 100-layer target.
