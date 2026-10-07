# onepaint

A native, cross-platform raster painting app in Rust. It is a performance-first
rewrite of [happypaint](../happypaint) that aims for full feature parity,
built on **egui** for the UI and **wgpu** for the canvas.

## Status

**Slice 1 (a paintable vertical slice) is complete.** It includes:

- Painting with the brush and eraser:
  - Pressure dynamics, smoothing, the stabilizer and taper.
  - Nine happypaint presets, with their parameters copied verbatim.
- Layers:
  - Add, delete, duplicate, reorder and merge down.
  - Groups.
  - Opacity and visibility.
  - All 19 layer blend modes.
  - Clipping masks, alpha lock and inherit alpha.
- Undo and redo.
- Pan, zoom and rotate on the GPU canvas.
- `.happypaint` open and save (v1–v9 read, v9 write), and PNG export.
- A canvas-first, Procreate-like UI with dark and light themes.

[`docs/feature-parity.md`](docs/feature-parity.md) tracks every happypaint
feature, and [`docs/divergences.md`](docs/divergences.md) lists the known
differences.

## Run

```sh
./dev-start.sh                 # debug build and run
./dev-start.sh --release       # optimised
./dev-start.sh -- file.happypaint
```

The app needs a GPU that wgpu supports (Vulkan, Metal, DX12 or GL).

### Controls

| Action | Input |
|---|---|
| Paint | Drag with the left mouse button or the pen |
| Brush / eraser | B / E |
| Brush size | `[` `]`, or right-drag on the canvas |
| Pick colour | Alt-click |
| Pan | Space-drag, middle-drag, or two-finger scroll |
| Zoom | Ctrl+wheel or pinch |
| Rotate | R-drag (snaps near 0°, 90°, 180° and 270°) |
| Fit / 100% | Ctrl+0 / Ctrl+1 |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z |
| New / open / save / export PNG | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+E |
| Hide the UI | Tab |

## Develop

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p onepaint-engine --example perf --release   # hot-path timings
```

- **Specs** live in [`docs/specs/`](docs/specs/). Each one is written
  before its tests, and the tests come before the code.
- **Agent guidance** is in [`AGENTS.md`](AGENTS.md).
- **Visual checks** use environment variables:

  ```sh
  ONEPAINT_DEMO=1 ONEPAINT_POPOVER=layers ONEPAINT_SCREENSHOT=/tmp/shot.png ONEPAINT_EXIT=1 \
    cargo run -p onepaint
  ```

  The screenshot fires after a few frames. Wayland compositors throttle
  hidden windows, so run the command with the window visible, or force
  XWayland with `env -u WAYLAND_DISPLAY`.

## Licence

AGPL-3.0, matching happypaint. Code adapted from
[photocraft](https://github.com/storytold/photocraft) (MIT/Apache-2.0) is
marked in each file's header.

Bundled assets:

- Inter and JetBrains Mono fonts (SIL OFL)
- Lucide icons (ISC)

Their licence files sit next to them in `crates/ui_kit/assets/`.
