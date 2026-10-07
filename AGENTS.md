# onepaint

A **native, cross-platform raster painting app in Rust**, written as a
port of **happypaint** (`../happypaint`). happypaint is the production
SolidJS + WebGL2 app at happypaint.art. onepaint is not a line-by-line
translation. It is an **API-to-API rewrite**: every engine boundary in
happypaint becomes a Rust crate or trait with the same responsibilities and
behaviour, written the way idiomatic Rust would do it.

Targets: Linux (X11 and Wayland), Windows and macOS. A WASM/WebGPU build
is a later goal. iPad and Android are non-goals for now.

**Scope: full feature parity with happypaint, built for maximum performance.**
Every happypaint feature ships. The list is in `docs/feature-parity.md`.
Parity covers features and output, not how happypaint implements them. Pick
the fastest design Rust and wgpu allow, even when it differs from
happypaint's:

- Do the work on the GPU wherever it is pixel-parallel.
- Use compute shaders where they beat fragment passes.
- Keep pixels resident on the GPU. Avoid readbacks.
- Use rayon for CPU work.
- Use SIMD in hot loops.
- Make stroke and composite paths allocation-free.

The performance targets:

- Pen-to-pixel latency under one frame.
- 60fps or better pan, zoom and rotate on 8k×8k documents with 100+ layers.
- Filter previews that stay interactive.

> Status: slice 1 (a paintable vertical slice) is done; see `README.md`,
> `docs/specs/00-slice1.md` and `docs/feature-parity.md`. When the code
> disagrees with this file, fix one of them in the same change.
>
> Slice 1 architecture: the CPU tiles (`raster`) are the source of truth;
> stamping and compositing run on the CPU per dirty tile (rayon); `gpu`
> displays the composite. GPU stamping and compositing go behind the same
> APIs next, verified against the CPU oracle.

---

## Development process

Follow `../happypaint/LOOP.md` (40 rules across 7 tiers: read before
write, scope lock, TDD, plan first, self-review and so on). Ignore any rule
that clearly does not apply to the task.

## Source of truth for behaviour

happypaint is the **spec**. Before porting a subsystem, read:

- `../happypaint/docs/architecture.md`: the engine/UI split, the seams and
  file formats.
- `../happypaint/docs/graphics/01–07`: algorithm and maths notes, with
  `file:line` citations into the TS source.
- The TS module itself under `../happypaint/frontend/src/`.
- Its `*.test.ts` files. Port them as Rust tests first, then make them pass.

happypaint defines **what** each feature does and what it outputs. It does
not define how to implement it. Treat the TS code as a reference, not a
template: do not mirror its WebGL state machine, its god-object facade or
its CPU readbacks. When happypaint has a bug or quirk that users can see,
keep it unless the user says otherwise. Record every deliberate divergence in
`docs/divergences.md`.

Prior art: `storytold/photocraft` (MIT/Apache-2.0, egui + wgpu) shows how
the GUI and graphics can be done. You may study it, and copy from it only
with attribution in the file header. Never copy its branding. It is very young
(created 2026-09-30) and changes quickly, so **do not depend on it as a
crate**.

---

## Tech stack (decided unless noted)

| Concern | Choice | Notes |
|---|---|---|
| Language | Rust stable, edition 2024, Cargo workspace | `unsafe` is forbidden by default. Allow it only per module, with a `// SAFETY:` comment. |
| UI | **egui / eframe**, with the `wgpu` backend, plus our own `ui_kit` design system | The UI must be **modern and beautiful**; see `docs/design-system.md`. Bootstrap `ui_kit` from photocraft's `crates/ui-egui` (theme, widgets, icons, tab strip, dock), keeping attribution. Never ship stock egui visuals. The UI stays a thin layer over the engine. |
| GPU | **wgpu**, with **WGSL** shaders | GPU-first engine: layers live as GPU tiles. The canvas is an `egui_wgpu` paint callback, so it never round-trips through egui textures. Use compute passes for filters, flood fill, magic wand and selection feathering where they are faster. |
| Shaders | `.wgsl` files under `crates/gpu/shaders/` | Port happypaint's ~62 GLSL ES 3.0 programs. naga can translate them as a first pass, but hand-review every result. |
| Pixels | 256² copy-on-write `Arc` tiles, premultiplied alpha | Formats are RGBA8, RGBA16F and RGBA32F, matching happypaint. |
| Parallel CPU | `rayon` | |
| Text | `cosmic-text` (or `parley`) plus `swash` | Replaces Canvas2D text. |
| Images | `image`, `png`, `zune-jpeg`, `tiff`, `image-webp` | |
| PSD | custom `formats::psd` | No Rust crate can write PSD. |
| Video/GIF | `gif` and `color_quant`; an `mp4` muxer with `openh264` | Replaces gifenc, mp4-muxer and MediaRecorder. |
| Tablet | per-OS backends behind one `PenEvent` type | `octotablet` covers Wayland and Windows Ink. Add native macOS and X11 backends as needed. This choice is **not final yet**. |
| Dialogs/clipboard | `rfd`, `arboard` | |
| Settings/autosave | `directories` with a ron/json config; autosave in the app data dir | Replaces localStorage and IndexedDB. |
| Serialization | `serde` | |
| Tests | `cargo test`; `insta` snapshots; the CPU compositor as GPU oracle | |

Every new dependency needs a one-line justification in the PR or commit message.

---

## Workspace layout (target)

```
crates/
  raster/      tiled Surface, Tile, pixel formats, premultiply. No GPU.
  color/       color spaces, Kubelka–Munk pigment, CMYK, ICC, median-cut
  doc/         pure-data Document, layer tree, masks, clipping, object specs
  history/     undo/redo stacks (entry cap, byte budget, transactions)
  stroke/      smoothing, stabilizer, bezier, quick-shape, symmetry
  brush/       presets, dynamics, tips, grain, stamp planning (CPU side)
  selection/   rasterize, feather, grow, magic wand, flood fill
  compose/     CPU reference compositor: blend modes, clipping, groups (the oracle)
  gpu/         wgpu compositor, stamp/smudge/wet/liquify/filter passes, WGSL
  filters/     adjustment and filter params plus CPU reference impls (47 kinds)
  text/        rich text layout and raster
  formats/     .happypaint v9 (read and write), PSD, PNG/JPEG/TIFF, ABR, Procreate brushes, recording
  input/       PenEvent, pointer router, gestures, palm rejection, pressure synthesis
  engine/      Session facade: command API, tools, events. No UI imports.
  cloud/       provider trait; Google Drive via a loopback OAuth/PKCE flow
  ui_kit/      design system: theme tokens, fonts, icons, motion, all widgets
apps/
  onepaint/        eframe app (UI only)
  onepaint-cli/    headless: render, convert and replay recordings
```

Dependency rule: `engine` depends on the domain crates, and `apps/*`
depends on `engine`. **No domain crate depends on egui or on the apps.**
Only `gpu` (and the app) may depend on wgpu.

---

## API-to-API mapping (happypaint → onepaint)

| happypaint (TS, `frontend/src/`) | onepaint (Rust) |
|---|---|
| `engine/paint-engine.ts` facade (8.9k lines) | `engine::Session`. Split it by concern and do not port it as a god-object. |
| `*-store.ts` handlers (`DocumentHandler`, `HistoryHandler`, `SelectionHandler`, `AdjustmentHandler`, …) | the `engine::Command` enum, run through `Session::execute(cmd) -> Result<Events>` |
| `PaintEngineOptions` callbacks (`onNeedsRender`, `onLayersChanged`, …) | the `engine::Event` enum, which the UI drains each frame |
| `PaintSink` (`beginStroke`/`continueStroke`/`airbrushTick`/`endStroke`) | the `engine::StrokeSink` trait |
| recorder `applyCommand()` | the same `Command` enum, serde-serializable, so it also serves as the replay, scripting and IPC format |
| `storage/cloud/types.ts` | the `cloud::Provider` trait |
| `engine/compositor.ts` plus GLSL | `compose` (CPU oracle) plus `gpu` (wgpu) |
| `engine/history/tile-grid.ts` | `history`, built on `raster` COW tiles |
| `input/pointer-router.ts` | `input::Router`, fed by winit and the tablet backends |

Keep the command and event names close to happypaint's, so a reader can
grep across both repos.

---

## Conventions

- File and module names are `snake_case`, as in Rust. Types are
  `PascalCase`.
- Keep modules small and single-purpose. Prefer composition. No file
  should exceed about 1,500 lines (happypaint's `paint-engine.ts` shows why).
- Domain crates are **pure and deterministic**: no globals, no wall-clock
  reads, no I/O except in `formats` and `cloud`.
- Every GPU pass has a CPU reference in `compose` or `filters`, and its test
  asserts the two match within 1/255 (8-bit) or 1e-3 (float).
- Errors: `thiserror` in libraries, `anyhow` only in `apps/*`. No `unwrap()`
  outside tests, except where an invariant is proven by a comment.
- Shaders live in `.wgsl` files, not in Rust string literals.
- **UI:** app code uses only `ui_kit` components and theme tokens. It never
  uses raw egui widgets, hard-coded colors or magic spacing. Each new
  component needs snapshot tests in both themes. The full rules are in
  `docs/design-system.md`.

## Commands

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p onepaint          # or ./dev-start.sh
```

The UI is tested with `egui_kittest`: tests in `apps/onepaint/src/main.rs`
drive real pointer and key events. The GPU present pass has a headless
render test (`crates/gpu/tests/render.rs`), which is skipped when no
adapter is present.

To check the UI by eye, use the `ONEPAINT_SCREENSHOT` environment hook
(see `apps/onepaint/src/debug.rs`). It captures only onepaint's own window;
never take a screenshot of the whole desktop.

These must stay green before every commit.

Write commit messages, PR descriptions and code comments as the human
author would. **Never** add AI/LLM attribution or hints: no
`Co-Authored-By` trailers for assistants, and no "Generated with …" lines.

Hot paths (stroke, composite, canvas present, filter preview) get
`criterion` benches in `benches/`. A change that slows a bench by more than 5%
needs a justification.

## Porting workflow (per subsystem)

1. Read the happypaint module, its tests, and the relevant `docs/graphics`
   note.
2. Port the tests to Rust and watch them fail.
3. Implement the CPU version (pure crate), then make the tests pass.
4. Add the GPU path where happypaint uses one, and test it against the CPU oracle.
5. Wire it into `engine::Command` and the UI.
6. Tick the item in `docs/feature-parity.md`. Update this file and `docs/`
   if a seam or a decision changed.

Suggested order: raster → doc → compose → history → stroke/brush (CPU) →
gpu canvas and stamp → input/tablet → minimal UI (brush, eraser,
layers, undo) → `.happypaint` read/write → selection, fill and filters →
text → PSD and video → cloud.
