# onepaint

A **native, cross-platform raster painting app in Rust**, written as a
port of **happypaint** (`../happypaint`). happypaint is the production
SolidJS + WebGL2 app at happypaint.art. onepaint is not a line-by-line
translation. It is an **API-to-API rewrite**: every engine boundary in
happypaint becomes a Rust crate or trait with the same responsibilities and
behaviour, written the way idiomatic Rust would do it.

Targets: Linux (X11 and Wayland), Windows and macOS. A WASM/WebGPU build
is a later goal. iPad and Android are non-goals for now.

> Status: greenfield. This file records the **intended** architecture. When
> the code disagrees with this file, fix one of them in the same change.

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

When happypaint has a bug or quirk, keep it compatible unless the user
says otherwise, and record each deliberate divergence in `docs/divergences.md`.

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
| UI | **egui / eframe**, with the `wgpu` backend | Docked panels via `egui_dock`. The UI stays a thin layer over the engine. |
| GPU | **wgpu**, with **WGSL** shaders | The canvas is an `egui_wgpu` paint callback, so it never round-trips through egui textures. |
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
  history/     undo/redo: tile snapshots plus pixel deltas
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

## Commands

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p onepaint
```

These must stay green before every commit.

## Porting workflow (per subsystem)

1. Read the happypaint module, its tests, and the relevant `docs/graphics`
   note.
2. Port the tests to Rust and watch them fail.
3. Implement the CPU version (pure crate), then make the tests pass.
4. Add the GPU path where happypaint uses one, and test it against the CPU oracle.
5. Wire it into `engine::Command` and the UI.
6. Update this file and `docs/` if a seam or a decision changed.

Suggested order: raster → doc → compose → history → stroke/brush (CPU) →
gpu canvas and stamp → input/tablet → minimal UI (brush, eraser,
layers, undo) → `.happypaint` read/write → selection, fill and filters →
text → PSD and video → cloud.
