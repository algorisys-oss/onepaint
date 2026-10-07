# Feature parity with happypaint

onepaint must ship **every** happypaint feature. This file is the checklist.
Tick an item only when it works in the app, its tests pass, and its output
matches happypaint. If a feature changes in happypaint, update its item here.

Legend: `[x]` done, `[~]` partly done (the note says what is missing), `[ ]`
not started.

Sources, all under `../happypaint/frontend/src/`:

- `state/tool-store.ts` (tools)
- `engine/layer.ts` (blend modes)
- `engine/adjustments/adjustment-renderer.ts` (adjustments)
- `engine/brush/brush-presets.ts` (presets)
- `../happypaint/todo.md` (the full history)

## Tools (19)
- [x] brush
- [x] eraser
- [x] fill (flood fill) (tolerance, grow, confined to the selection, alpha lock)
- [x] eyedropper (the tool, Alt-click and Alt-drag sample the composite; touch-and-hold comes with gestures)
- [~] move/transform (scale, rotate, warp, quad homography) (move, scale, rotate, flip and selection lift are done; distort and warp are not)
- [x] pan (space-drag, middle-drag, two-finger scroll)
- [ ] smudge
- [ ] mixer
- [ ] gradient, with output to pixels or a fill layer
- [ ] liquify (all modes)
- [ ] text
- [ ] pen, with output to a shape layer, a path or pixels
- [ ] shape
- [ ] panel (comic panels)
- [x] select-rect
- [x] select-ellipse
- [x] select-lasso
- [x] select-poly
- [x] select-wand

## Brushes
- [ ] All 118 presets, including Manga and the baked Procreate brushes, with identical parameters
- [~] Dynamics: pressure, tilt, twist and velocity mapped to size, opacity and flow (all implemented; hardware tilt and pressure need a tablet backend, see Input)
- [x] Velocity-synthesized pressure for devices without hardware pressure
- [ ] Computed and sampled tips
- [ ] Grain texture
- [ ] Airbrush (time-based ticks)
- [x] Smoothing and the stabilizer
- [ ] Symmetry: mirror and radial
- [ ] Wet/oil media: wet, height (impasto), settle and bevel
- [ ] Smudge and mixer brushes
- [ ] Kubelka–Munk `pigment` brush blend
- [ ] Brush studio: an editor with live preview and a try-it pad
- [ ] Import `.abr`, Procreate `.brush` and `.brushset`, and `.happybrush`

## Layers
- [x] Add, delete, reorder, duplicate and merge
- [x] Nested groups (with drag-and-drop into and out of groups)
- [~] Opacity, visibility and color labels (color labels are preserved in files but not shown)
- [~] Layer masks (composited, loaded and saved; you can't paint into a mask yet)
- [x] Clipping masks (clip families)
- [x] Alpha lock
- [ ] Off-canvas pixel overflow
- [ ] Text layers: multiple rich-text objects and balloons, vector until baked
- [ ] Vector path layers, plus path ops
- [ ] Adjustment layers and fill layers
- [ ] Per-layer smart-filter stacks

## Blend modes (20)
The modes are:

- normal, multiply, screen, overlay, add
- darken, lighten, color-dodge, color-burn
- hard-light, soft-light, difference, exclusion
- linear-burn, divide
- hue, saturation, color, luminosity
- pigment (brush only)

- [~] All of them on the GPU, matching the CPU oracle (all 19 layer modes are done on the CPU and the composite is shown on the GPU; GPU compositing is not done yet; `pigment` falls back to normal)

## Adjustments and filters (47 kinds)
Each kind works destructively, as an adjustment layer, and as a smart filter.
The exception is fill layers, which only work as layers.

These 14 are done: destructive with live preview, selection-aware, and as
adjustment layers. The blur/sharpen ones work as destructive filters only; as
layers they are preserved but not rendered yet.

- [x] brightness-contrast, hue-saturation, black-white, invert, threshold, posterize, levels, curves, color-balance, gradient-map
- [x] gaussian-blur, motion-blur, sharpen, noise

Still to do:

- [ ] color-to-alpha, replace-color, selective-color
- [ ] radial-blur, tilt-shift, iris-blur, field-blur, spin-blur
- [ ] **Distort:** twirl, pinch, ripple, polar-coordinates
- [ ] **Stylize:** bloom, chromatic-aberration, halftone, tone, glitch, mosaic, crystallize, emboss, oil-paint, wind, line-art (Sobel/XDoG)
- [ ] **Render:** light-rays, lens-flare, clouds, fog, smoke, nebula, starfield, lightning
- [ ] **Fill:** gradient-fill, solid-fill, pattern-fill
- [ ] Smart-filter stacks (`layer.filters`; preserved in files, not applied yet)

## Selection
- [x] Rect, ellipse, lasso, polygon and magic wand
- [x] Add, subtract and intersect
- [x] Feather, grow and smooth
- [ ] Saved selections
- [x] Marching-ants overlay
- [~] Fill, stroke and adjust within a selection, auto-rasterizing vector layers first (fill, clear and painting are clipped; adjustments and vector layers come later)

## Canvas and guides
- [~] Pan, zoom and rotate at 60fps or better on large documents (pan, zoom and rotate all work; not yet measured on 8k documents)
- [ ] Grid
- [ ] Symmetry guides
- [ ] 1–5-point perspective with stroke snapping
- [ ] Horizontal/vertical snap
- [ ] Rulers
- [ ] Quick-shape: hold to straighten a stroke into a line, ellipse or polygon
- [ ] Speed lines
- [ ] Reference window
- [ ] Multi-document tabs

## Input
- [ ] Pen pressure, tilt, twist and the eraser end:
  - [ ] Windows Ink/Wintab
  - [ ] macOS
  - [ ] X11
  - [ ] Wayland
- [ ] Coalesced (high-rate) pen samples
- [ ] Multi-touch gestures with Procreate's vocabulary:
  - pinch to zoom and rotate, two-finger pan
  - two-finger tap to undo, three-finger tap to redo
  - touch-and-hold for the eyedropper
  - three-finger swipe down for the copy/paste menu
  - four-finger tap to toggle the UI
- [ ] Palm rejection while a pen is active
- [~] Full keyboard shortcuts (the happypaint keybindings map) (the core set: B, E, [ ], undo/redo, save/open/new/export, zoom, Tab)

## History and recording
- [x] Undo and redo with tile snapshots and pixel deltas (copy-on-write tile snapshots)
- [ ] Record a session as a command log, then replay it
- [ ] Export a timelapse as video

## Animation
- [ ] Animation Assist: onion skin and exposure-sheet tracks
- [ ] Frame export

## Color
- [x] Color picker
- [ ] Palettes, with import and export
- [ ] Median-cut palette extraction
- [ ] ICC profiles
- [ ] CMYK soft-proof and press profiles
- [ ] TIFF CMYK export

## Files
- [x] `.happypaint` v9: read and write (v1–v9 read; v9 write; JSON-level compatible)
- [ ] Autosave and the gallery
- [ ] `.happypaint-book`
- [ ] `.happypaint-rec`
- [ ] PSD: read and write layers, groups, masks, blend modes and text
- [~] PNG, JPEG, WebP, TIFF and Netpbm, keeping ICC profiles and DPI (PNG export only)
- [ ] GIF, MP4, WebM and PNG-zip export
- [x] Clipboard: copy and paste images (in-place paste, the system clipboard, layer via copy/cut)
- [ ] Camera and screen capture as a reference or import

## Cloud
- [ ] Google Drive provider, with desktop OAuth (loopback + PKCE)

## Comics
- [ ] Panel layouts and panel tool
- [ ] Balloons (in text layers)
