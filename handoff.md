# Handoff

Last updated 2026-10-10 (evening). Read this first when picking the work back up,
then `todo.md` (the working list) and `AGENTS.md` (the rules).

## Where things stand
- `main` is clean and pushed.
- Build, `clippy -D warnings`, fmt and `cargo test --workspace` are all green.
- Feature parity is nearly complete. `todo.md` lists what is left, and
  `docs/feature-parity.md` has the per-item notes; deliberate differences
  from happypaint are in `docs/divergences.md` (now up to 46).

## Last session (2026-10-10, evening, unattended)
Items 1–5 from the earlier list; every step pushed as it landed.

- **Spec 40, composite sheets** (`gpu::sheets`): the canvas composite and
  the selection are texture arrays of sheets.
  - A document within the GPU texture limit is one sheet, and nothing
    changes for it.
  - Past the limit, each axis is cut into 256-aligned sheets.
  - The present pass filters across seams by hand.
  - The app now asks for the adapter's own texture limits. egui asks for
    8192, so documents from 8193 to 16384 px had been blank.
  - `ONEPAINT_MAX_SHEET=256` shows sheets on small documents.
- **Spec 41, opening in the background** (`apps/onepaint/src/loading.rs`):
  - `.happypaint`, PSD and images decode on a worker thread, with a
    progress card and Cancel after 150 ms;
  - `formats::Progress` carries the progress and the cancel flag;
  - `.happypaint` tiles decode in parallel: 10.7 s down to 3.6 s for a
    328 MB file;
  - books still open on the UI thread.
- **GPU fallbacks moved off the CPU**, each checked against the CPU oracle,
  with a mutation that the test catches:
  - spec 42: 13 colour adjustment kinds (`filters::gpu` packs them,
    `Op::Adjust`);
  - spec 43: paper shading (`Op::Paper`, the grain as an R32Float
    texture);
  - spec 44: partial panel clips (`Op::Coverage`, cached across frames in
    the session's `PlanCache`);
  - spec 45: impasto relief (the height tiles of the tile and its four edge
    neighbours);
  - spec 46: noise, screentone (not the "sand" shape), solid, gradient
    and built-in pattern fills. With the session's `PlanCache`, each
    adjustment is prepared once per layer and shared by every tile;
  - spec 48: grayscale, print preview and the gamut warning, applied in the
    present pass with the proof LUT as a 3D texture (as happypaint does).
    They no longer force the CPU path or recomposite when toggled.
  - Spec 46 also adds a rule: a tile with an adjustment layer at or above
    the hot layer is not split. The f16 cached image under it, amplified by
    hue near grey, threshold or levels, was visibly off.
- **Spec 47:** the planner skips layers under a fully opaque normal tile,
  exactly, using an opacity flag cached per tile.
- **Segment cache at 384 MiB** (it was 256): a 205-tile drag had been
  refilling 76 segments a frame. On the quiet machine the split drag frame
  went from 54 to 16.5 ms, and to 14.5 ms back to back (spec 38).
- **Still on the CPU:** imported pattern images, sand screentone, spatial
  adjustment layers and ghost pegs (`todo.md` §2.1). The last two need a
  multi-pass design: a padded backdrop pass, and intermediate images
  resampled through the peg transform.
- **A review of the evening's commits** found four bugs, all fixed with
  tests where possible:
  - warm-up filled relief segments with a 1 × 1 document size;
  - a background load could land on a document opened after it started;
  - the proof LUT and paper grain textures were cached by an address
    that could be reused.
- **Debug hooks:** `ONEPAINT_NEW=WxH` (a blank document of that size;
  16384 × 12000 checked, one sheet and 4 × 3) and
  `ONEPAINT_VIEW=gray|proof|warn`.
- **Item 1, quiet measurements** (specs 38 and 39 have them):
  - the split drag frame was 54 ms against 144 ms unsplit. That exposed a
    segment-cache thrash; with the cache at 384 MiB it is **16.5 ms**,
    under the 60 fps budget even at the low clock;
  - level 3 was 6.2 ms;
  - the iGPU sat at 500–650 MHz, because the governor doesn't raise the
    clock while the bench waits after every frame. So they are relative,
    and the earlier 19.7 ms was taken at a higher clock. Absolute numbers
    need the clock pinned (root) or a bench that keeps the GPU busy.
- **Item 3, 8-bit storage:** waiting for your decision (below).

## Next up (suggested order)
1. **Your decision on 8-bit storage** (below), then build it.
2. **Spatial adjustment layers on the GPU** (a padded backdrop pass), then
   ghost pegs.
3. **The rest of the photocraft list** (`todo.md` §2.1):
   - working-set checks before a full refresh;
   - bounded submits and device-loss handling;
   - a perf-budget gate;
   - skipping layers under opaque tiles.
4. **Vector support** (`todo.md` §3.1): discuss its scope first, then write
   a spec.

## Open decisions for the user
- **8-bit storage for 8-bit documents (item 3).** In happypaint an 8-bit
  document also composites in gamma space (divergence 2), so this changes
  what users see, not just the memory used. The options:
  - **A, store 8-bit sRGB and keep linear compositing.** RAM and VRAM
    halve. Every edit re-quantizes the layer to 8 bits, as happypaint does,
    so soft glazes band a little. Blending is unchanged.
  - **B, full happypaint 8-bit mode:** A, plus gamma-space blending and its
    multiply quirk. This closes divergence 2, but 8-bit documents' blend
    modes will look different from today.
  - **C, VRAM only:** the GPU atlases hold 8-bit sRGB for 8-bit documents,
    and the CPU tiles stay f16. VRAM halves, RAM doesn't, and the output
    is unchanged apart from rounding on the GPU path.
  - I'd do A, then B as a separate step if you want parity.
- The GPU memory budget on integrated GPUs: 1 GiB of source tiles plus
  **384 MiB** of segment cache (raised from 256 MiB tonight), both counted
  with mips. 256 MiB thrashed on a 205-tile drag; 384 MiB took the split
  drag frame from 54 ms to 16.5 ms (spec 38).
- Whether to drop the CPU tile copy once tiles are resident on the GPU.
- Vector support: its scope, and when it comes relative to the rest.

## Gotchas
- The disk is about 91% full. Run `cargo clean` if free space drops below
  8 GB. RAM is tight, so build with `-j4`.
- GPU tests and benches skip silently when no adapter is present. Check
  for the "skipping" line before trusting a green run.
- **GPU timings are noisy here.** Browsers share the iGPU and its clock
  drops. Compare variants in the same run (the bench trace alternates
  them) and quote medians.
- `PIGMENT_WGSL` (in `gpu::wet`) is shared by the wet and compose shaders.
- **Debug scenes** (`apps/onepaint/src/debug.rs`): `ONEPAINT_DOCK=1|float`
  (docked or floating panels), `ONEPAINT_PANELS=1` (a manga page), plus the
  older hooks. `ONEPAINT_CPU_COMPOSE=1` forces the CPU compositor, which is
  useful for diffing screenshots.
- **A locked screen stalls the app on Wayland:** it gets no frames, so
  screenshot runs hang. Run them with `env -u WAYLAND_DISPLAY` (X11).
- Commit messages carry no AI attribution.
