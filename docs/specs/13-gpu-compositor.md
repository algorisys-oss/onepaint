# 13: GPU compositor

## Status
Built (first cut). The parts:

- `compose::program::plan_tile` flattens a tile's tree into `TileProgram`
  ops: `Layer`, `PushZero`, `PushContent`, `Clip` and `Pop`.
  `compose::program::run` is a CPU interpreter of the same encoding, and
  its tests check it against `composite_tile`.
- `gpu::compose::GpuCompositor` interprets the programs in
  `shaders/compose.wgsl`, one invocation per pixel. Source tiles live in
  `Rgba16Float` array atlases (up to 8), keyed on `Arc<Tile>` identity and
  evicted LRU within the budget (default 1 GiB). The Kubelka–Munk code
  (`shaders/pigment.wgsl`) sits behind an override constant, so only
  batches that use pigment pay for it.
- `Session::plan` returns programs for the tiles the GPU can do and the
  keys that still need `Session::composite`. The canvas sends up to 256
  tiles a frame, visible ones first.
- Strokes in progress, masks and mask strokes, inherit alpha, clip families,
  nested groups (depth ≤ 8), ghost tints and fully covered panel clips run
  on the GPU.
- The CPU still composites tiles that have visible adjustment layers,
  relief (`height`), paper, partial panel clips or ghost pegs, as well as
  the present-only view modes. Setting `ONEPAINT_CPU_COMPOSE` forces the
  CPU path.
- Tests: `crates/gpu/tests/compose.rs` (random documents and strokes
  against the oracle, atlas eviction, re-upload of changed tiles).
- Bench (`crates/gpu/benches/compose.rs`, Iris Xe): a full 4096² × 20-layer
  recomposite takes 130 ms on the GPU and 847 ms on the CPU, and planning
  takes 0.3 ms. That is still short of the 50 ms target below.

## Why
The CPU compositor is memory-bound (spec 00, performance notes): a full
recomposite of a 4096², 20-layer document takes about 1.5 s. Progressive
compositing hides this, but the target of 8k × 100 layers at 60 fps needs the
layer data to stay resident on the GPU and the composite to run there.

## Design
1. **Tile atlas.** Use 2D-array textures of 256² `Rgba16Float` slots, in
   several atlases if `max_texture_array_layers` (≥ 256; 2048 on Iris Xe)
   requires it.
   - A slot table maps `(layer, tx, ty) → (atlas, slot)`.
   - Uploads are dirty-tracked by `Arc<Tile>` pointer identity, the same way
     photocraft's `crates/gpu` does it.
   - Masks use `R16Float` slots in a separate atlas.
2. **Residency.** An LRU over slots keeps memory within a budget
   (default 1 GiB; configurable). Tiles that aren't resident are uploaded
   on demand before the tile's composite is dispatched.
3. **Program per output tile.** The layer tree is flattened into a small
   instruction list:
   - `PUSH_GROUP`
   - `LAYER{slot, mask_slot, mode, opacity, flags}`
   - `CLIP{slot, mask_slot, opacity}` (clip family)
   - `ADJUST{kind, params_index}`
   - `POP_GROUP{mode, opacity}`

   Each output tile gets its own list in a storage buffer. Layers without a
   tile there are skipped.
4. **Compute shader.** One invocation per pixel interprets the list with a
   register stack (group depth ≤ 8). Blend maths are ported 1:1 from
   `compose::blend` (`bc`, SetLum/SetSat/ClipColor, the sRGB transfer for
   non-normal modes) and the per-pixel adjustments from `filters`. Curves
   and gradient-map LUTs are bound as a 256×N texture. The result is written
   straight into the canvas composite texture (`Rgba16Float` storage).
5. **Spatial adjustment layers** (blur, sharpen) become feasible: a
   separate pass over a padded backdrop region, which removes divergence 9.
6. **Strokes.** The stroke buffer is uploaded as a temporary layer slot, and
   `StrokeMode` maps to an instruction flag. Later, stamping on the GPU
   (instanced quads into the stroke slot) removes the CPU dab rasteriser
   from the hot path.

## Verification (spec-first)
- A headless test renders random documents through both the CPU oracle
  (`compose`) and the GPU path. The documents mix all 19 modes, groups,
  clips, masks, inherit alpha and adjustments.
- Every pixel must agree within 1/255 (8-bit display) and 2e-3 (linear).
- Benchmarks: 4096² × 20 layers full recomposite under 50 ms, and an opacity
  drag at 60 fps on 8k × 100.

## Fallback
When the device lacks storage-texture `Rgba16Float` support or the budget
is exceeded, use the existing CPU path with progressive compositing.

## Open decisions for the user
- The GPU memory budget on integrated GPUs (shared RAM).
- Whether to drop the CPU copy of tiles once GPU-resident (it halves RAM but
  needs readback for saving, the eyedropper and filters).
