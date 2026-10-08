# Handoff

Last updated 2026-10-08. Read this first when picking the work back up,
then `todo.md` (the working list) and `AGENTS.md` (the rules).

## Where things stand
- `main` is clean and pushed. The last commit is `2cf2a65`.
- Build, `clippy -D warnings`, fmt and `cargo test --workspace` are all green.
- Feature parity is nearly complete. `todo.md` lists what is left, and
  `docs/feature-parity.md` has the per-item notes.

## Last session: GPU compositor (spec 13)
- `66e3c98`: the canvas composites on the GPU.
  - `compose::program::plan_tile` flattens a tile's layer tree into ops,
    and `compose::program::run` is a CPU interpreter of the same ops.
  - `gpu::compose::GpuCompositor` runs those ops in `shaders/compose.wgsl`
    over `Rgba16Float` atlases. The atlases are keyed on `Arc<Tile>`
    identity and evicted LRU within a 1 GiB budget.
  - `Session::plan` splits the dirty tiles into GPU programs and keys that
    stay on the CPU. The canvas sends up to 256 tiles a frame.
  - `ONEPAINT_CPU_COMPOSE=1` forces the old CPU path.
- `27cd918`: added a lean pipeline (override constant `GENERAL = false`)
  for jobs made only of plain normal layers.
  - Full recomposite of 4096² × 20 layers on Iris Xe: 130 ms → 60 ms
    (CPU: 847 ms).
  - Texture loads alone take about 55 ms on this GPU, so the spec's 50 ms
    target is at the hardware floor here.
  - Bench: `cargo bench -p onepaint-gpu --bench compose`.
- `2cf2a65`: added `todo.md` and fixed two stale parity notes.

## Next up (suggested order)
1. **Move the remaining GPU-compositor fallbacks off the CPU** (`todo.md`
   §2.1). The bail-outs are the `Err(Unsupported)` returns in
   `crates/compose/src/program.rs`. Start with colour adjustment layers
   (curves and gradient maps as LUT textures), then relief, paper,
   partial panel clips and ghost pegs.
   - Each new op needs a CPU-interpreter case, a WGSL case and an oracle
     test in `crates/gpu/tests/compose.rs`.
   - Keep new ops in the general pipeline only, so the lean one stays lean.
2. **Write the opacity-drag bench**: 60 fps on 8k × 100 layers.
3. **GPU stamping** (`todo.md` §2.2), with instanced dabs checked against
   the CPU rasteriser.
4. **Vector support** (`todo.md` §3.1). Write a spec first. The goal is to
   make onepaint good for both vector and raster work.

## Open decisions for the user
- The GPU memory budget on integrated GPUs (it is 1 GiB now).
- Whether to drop the CPU tile copy once tiles are resident on the GPU.
- Vector support: its scope, and when it comes relative to the remaining
  parity items.

## Gotchas
- The disk is about 91% full. Run `cargo clean` if free space drops below
  8 GB. RAM is tight, so build with `-j4`.
- GPU tests and benches skip silently when no adapter is present. Check
  for the "skipping" line before trusting a green run.
- `PIGMENT_WGSL` (in `gpu::wet`) is shared by the wet and compose shaders.
- Commit messages carry no AI attribution.
