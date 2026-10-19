# Handoff

Last updated 2026-10-19. Read this first when picking the work back up,
then `docs/parity-delta-2026-10-18.md` (the v0.204.7 run's list and
decisions), `todo.md` and `AGENTS.md`. **Keep this file current after every
step**, so a dropped session can resume from it.

## State (2026-10-19, morning) — read this first
Rules: commit **and push** per milestone (the user authorized pushing);
test with `RAYON_NUM_THREADS=2 cargo test -p X -j 2 -- --test-threads=2`,
one crate at a time (the user saw 10 GB / all cores otherwise); build with
`cargo -j 2`; run `cargo clean` when free disk drops under 8 GB.

Overnight (2026-10-18 → 19), all pushed, newest last:
- Spec 52 3b: stacked spatial layers on the GPU (up to three; planning
  grows ninefold per layer). Found on the way: every pass of a submit had
  shared one params/jobs/ops buffer set, so fill passes ran the last pass's
  jobs; each pass now has its own.
- Spec 53: whole-canvas filter layers on the GPU. A lone twirl, pinch,
  polar, chromatic aberration, radial, spin, tilt-shift, iris or field blur
  layer runs live (`gpu::whole`, `whole.wgsl`): changed backdrop tiles fill
  the segment cache and are copied into a backdrop image, one pass filters
  it, composites read the result. Up to 4096² (the gallery 2048²).
  Mid-stroke, refilters over ~8 ms of work (radial 4096²: 173 ms; tilt-shift
  2048²: 56 ms) keep the layer's image until the stroke ends, as the CPU
  path does. Fixed on the way: the image rounded the level size up, so
  clamped taps read a half-off-document texel.
- Spec 54: tiles whose sources can't be resident go to the CPU next frame
  (`take_failed`) instead of drawing with layers missing; a lost GPU device
  writes the autosave at once and asks for a restart (eframe can't make a
  new device).
- Spec 55 (2026-10-19, after "continue"): ghost groups out of their pegs
  composite on the GPU, as a spatial source resampling the group's content
  through the peg's inverse (`SpatialSrc::of_peg`, `blur_out` mode 6).
- Black & White (hue weights, tint) and Color Balance (shadows,
  midtones, highlights, Preserve Luminosity) on happypaint v0.201.3's
  formulas: files from happypaint now look the same here. CPU, GPU, PSD
  both ways, data-driven panels (`filters::ui`, with a `Heading`
  control); `ONEPAINT_ADJUST=<kind>` shows a kind's Properties panel.
- Selective Color, Photoshop's (`cmyk-selective-color`, v0.201.5): nine
  ranges × C/M/Y/K, Relative or Absolute, CPU, GPU (code 23), PSD selc,
  a range-picker panel.
- `scripts/bench-gate/gate.sh save|check`: the perf-budget gate for
  AGENTS.md's 5% rule, on criterion's saved baselines (checked: the 9
  engine hot-path benches came out within 5% of themselves).

Earlier on 2026-10-18 (pushed): the happypaint v0.204.7 parity items, four
more adjustment kinds, live PSD adjustments and byte-for-byte round trips
(spec 51), 8-bit GPU atlases (spec 49), spec 52 phases 1–3a.

Next, unless you redirect:
1. Imported pattern fills and sand screentone on the GPU (`todo.md` §2.1).
2. Oil paint on the GPU would need a different quadrant rule (divergence),
   so it waits for your call.

Open decisions for the user: workspace layout (happypaint's menu bar /
title-bar header, divergences 41–42), licence (happypaint is PolyForm NC,
onepaint AGPL), vector support scope.

## Earlier session (2026-10-10, evening, unattended)
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
- **8-bit documents (the user chose A, then B):**
  - spec 49 stores them as 8-bit tiles, half the CPU memory;
  - spec 50 makes them composite in gamma space like happypaint, with
    `color::Space` at every boundary;
  - the New Canvas dialog offers 8-bit, 16-bit float (the default) and
    32-bit float;
  - `ONEPAINT_NEW=WxH@8` starts an 8-bit page.
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
1. **8-bit atlases on the GPU**, so 8-bit documents also halve VRAM.
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
- The GPU memory budget on integrated GPUs: 1 GiB of source tiles plus
  **384 MiB** of segment cache (raised from 256 MiB tonight), both counted
  with mips. 256 MiB thrashed on a 205-tile drag; 384 MiB took the split
  drag frame from 54 ms to 16.5 ms (spec 38).
- Whether to drop the CPU tile copy once tiles are resident on the GPU.
- Vector support: its scope, and when it comes relative to the rest.

## Gotchas
- The disk is about 96% full (19 GB free after a `cargo clean` on
  2026-10-18; a full build takes ~17 GB). Run `cargo clean` if
  free space drops below 8 GB. RAM is tight: build with `-j 2`.
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
