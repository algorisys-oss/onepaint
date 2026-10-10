# Handoff

Last updated 2026-10-10. Read this first when picking the work back up,
then `todo.md` (the working list) and `AGENTS.md` (the rules).

## Where things stand
- `main` is clean and pushed.
- Build, `clippy -D warnings`, fmt and `cargo test --workspace` are all green.
- Feature parity is nearly complete. `todo.md` lists what is left, and
  `docs/feature-parity.md` has the per-item notes; deliberate differences
  from happypaint are in `docs/divergences.md` (now up to 46).

## Last session (2026-10-10)

### Workspace and UI
- **Tool bar** (`apps/onepaint/src/toolbar.rs`): table-driven, every tool
  has a button (Pan added).
  - A test walks `Tool::ALL` and clicks each button. The five selection
    shapes share the Select button, with written exemptions.
  - It floats by its grip, double-clicking the grip flips it between
    horizontal and vertical, and it docks as a full-height rail at a
    window edge.
  - It resizes by whole columns, stays clear of the top-left bar, and below
    900 px becomes a strip above the status bar.
- **Docking** (`layout.rs` for the model, `dock.rs` for the UI): Layers,
  Brushes, Colour, Guides, Tool options, Properties, Align, Palettes and
  Reference can all leave home.
  - They float, dock left or right, and stack or tab in a dock.
  - Docked sections collapse and resize; floating windows fold, resize and
    join into tab groups.
  - The layout is saved as `layout.json` next to `settings.json`, and
    happypaint's dock, float-group and resize tests are ported.
  - Panel bodies size themselves with `dock::fit_width`.
- **Align & distribute:**
  - engine: `crates/engine/src/align.rs` and `Command::AlignLayers`, one
    undo step, with text and path layers kept vector;
  - UI: `align_ui.rs`; Ctrl/⌘- and Shift-click pick layers in the Layers
    panel.
- **Status bar** (`status_bar.rs`): happypaint's items, including
  vertical/horizontal symmetry with move axis, zoom −/100%/+, a rotation
  dial, fill mode and the value check.
- **ColorDrop:** drag the status-bar swatch onto the canvas. The engine side
  is `crates/engine/src/color_drop.rs`.
- **Help** (`help.rs`): a Guide tab (tool bar, status bar, ColorDrop,
  docking, Panel/manga, Properties, Align) next to the shortcuts. The Panel
  tool now opens its page layouts when picked.

### Fixes
- Undo with the Move tool's box open undid nothing after Apply; it now goes
  through.
- The gallery listed recovered autosaves, which then showed as "missing".
  Autosaves are no longer listed, and missing files are pruned.
- Canvas overlays are clipped to the stage.

### Performance (specs 38 and 39)
- Bench: `cargo bench -p onepaint-gpu --bench opacity_drag` (8192² × 100
  layers). `ONEPAINT_BENCH_TRACE=1` alternates unsplit and split frames and
  prints a breakdown.
- **Spec 38, composite cache:**
  - Each dirty tile is split at the stroked or active layer: the layers
    below and the backdrop-independent Normal layers above are cached
    images, keyed by content.
  - Planning (`compose::program::plan_tiles`) runs in parallel.
  - Warm-up fills the cache on idle frames after the active layer changes.
  - Over-budget frames alternate direction.
  - Drag frame: 210 → 19.7 ms median when the tiles fit the atlas, and
    336 → 28 ms past it.
- **Spec 39, level-of-detail compositing:**
  - The canvas composite keeps a 6-level pyramid; zoomed out, a frame
    composites only at the displayed level.
  - Atlas and cache images get mips, filled when first needed.
  - Reduced-level tiles are previews ("rough") until idle frames refine them
    at level 0, so the settled view is exact.
  - Level-3 drag frame: 7.1 ms against 77 ms at level 0, in the same loaded
    run.
- Every number above was taken with browsers sharing the iGPU (its clock
  was down to 550 MHz) and load averages of 4–9. Treat them as upper bounds
  and relative, not absolute.

### Photocraft comparison (read-only study)
Photocraft (`storytold/photocraft`) is not better overall.
- **onepaint is ahead on:**
  - single-pass register compositing;
  - per-tile CPU fallback;
  - the stroke overlay drawn in the GPU program;
  - tile-diff undo.
- **Photocraft is ahead on:**
  - mipmaps (done here now);
  - documents larger than the GPU texture limit;
  - 8-bit storage;
  - working-set checks and device-loss handling;
  - background file loading;
  - a perf-budget harness.

The remaining ideas are in `todo.md` §2.1.

## Next up (suggested order)
1. **Re-measure specs 38 and 39 on a quiet machine** (browsers closed) and
   record the numbers in the specs.
2. **Documents larger than the GPU texture limit:** split the composite into
   tiles.
3. **8-bit storage for 8-bit documents**, which halves RAM and VRAM.
4. **Open large files in the background**, with progress and cancel.
5. **Move the remaining GPU-compositor fallbacks off the CPU** (`todo.md`
   §2.1): adjustment layers, relief, paper, partial panel clips and ghost
   pegs.
6. **Vector support** (`todo.md` §3.1). The user wants first-class vector
   support and to discuss its scope first. Write a spec after that
   discussion.

## Open decisions for the user
- The GPU memory budget on integrated GPUs: 1 GiB of source tiles plus
  256 MiB of segment cache, both counted with mips.
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
- Commit messages carry no AI attribution.
