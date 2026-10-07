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

**Slice 2 (selection and fill) is complete.** It includes:

- Rect, ellipse, lasso, polygon and magic-wand selection.
- New, add, subtract and intersect modes.
- Feather, grow, shrink and smooth.
- Marching ants.
- Painting clipped to the selection.
- The paint bucket, the eyedropper tool, clear and fill selection.

**Slice 3 (move and arrange) is complete.** It includes:

- Move and free transform: scale, rotate, flip and selection lift.
- Copy, cut and paste, with the system clipboard and in-place paste.
- Layer via copy and layer via cut.
- Place image and file drop; PNG, JPEG and WebP open as documents.
- Drag-and-drop layer reordering, including into and out of groups.
- Canvas size (with an anchor) and image size.
- Background autosave and crash recovery.

**Slice 4 (adjustments) is complete.** It includes 14 adjustments and
filters:

- Brightness/contrast, hue/saturation, B&W, invert, threshold, posterize,
  levels, curves, colour balance and gradient map.
- Gaussian blur, motion blur, sharpen and noise.

Each one has a live preview and respects the selection, and the colour kinds
also work as adjustment layers.

**Slice 5 (masks and drawing tools) is complete.** It includes:

- Layer masks: add, paint, enable, apply, delete.
- The gradient tool, with five shapes.
- The shape tool: line, rect, ellipse and polygon, with optional fill.
- Quick-shape: hold at the end of a stroke to straighten it.

Slices 6 and 7 add:

- The full happypaint brush library: 124 presets with tips, paper grain
  and airbrush.
- Text layers (T).
- Layer colour labels.
- Progressive compositing for big documents.

Slice 9 adds smudge (S), the mixer brush (Shift+B, with Kubelka–Munk
pigment mixing), symmetry (Y: mirror, quad and radial) and liquify
(push, twirl, pinch, expand, reconstruct).

Slice 15 adds the pen tool (P) and vector path layers with Photoshop's path
menu: make selection, fill, stroke (with simulated pressure) and mask from path.
Slice 10b merges the remaining 34 happypaint filters, generators and fill
layers.

Slices 13 and 14 add saved selections (kept with the document) and swatch
palettes: import .swatches/.aco/.ase/.gpl, export .swatches, and new palettes
from the canvas or a photo.

Slice 11 adds drawing guides, saved with the document: a grid with snapping,
H/V snap, and 1–5-point perspective with soft or hard-line snapping and
draggable vanishing points (Shift+P), plus rulers (Shift+R) to drag guide
lines out of, an edit-guides mode (Shift+G) and a measure tool.

Slice 8 adds PSD: open `.psd`/`.psb` (layers, groups, masks, blend modes,
clipping) and File ▸ Export PSD….

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
| Pick colour | Alt-click, or the eyedropper (I) |
| Select | M rect, L lasso, W wand (more in the select popover); Shift add, Alt subtract |
| Select all / deselect / invert | Ctrl+A / Ctrl+D / Ctrl+Shift+I |
| Fill | G (bucket); Alt+Backspace fills the selection |
| Clear | Delete / Backspace (inside the selection) |
| Pan | Space-drag, middle-drag, or two-finger scroll |
| Zoom | Ctrl+wheel or pinch |
| Rotate | R-drag (snaps near 0°, 90°, 180° and 270°) |
| Fit / 100% | Ctrl+0 / Ctrl+1 |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z |
| New / open / save / export PNG | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+E |
| Move / transform | V or Ctrl+T; drag handles; Enter applies, Esc cancels |
| Copy / cut / paste | Ctrl+C / Ctrl+X / Ctrl+V; Ctrl+J layer via copy, Ctrl+Shift+J via cut |
| Smudge / mixer | S / Shift+B (click again for options) |
| Symmetry on/off | Y (guides button for the mode, spokes, angle and centre) |
| Rulers / edit guides / edit perspective | Shift+R / Shift+G / Shift+P |
| Pen | P: click corners, drag curves, Enter finishes, Esc cancels |
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
