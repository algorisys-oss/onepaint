# Feature parity with happypaint

onepaint must ship **every** happypaint feature. This file is the checklist.
Tick an item only when it works in the app, its tests pass, and its output
matches happypaint. If a feature changes in happypaint, update its item here.

Sources, all under `../happypaint/frontend/src/`:

- `state/tool-store.ts` (tools)
- `engine/layer.ts` (blend modes)
- `engine/adjustments/adjustment-renderer.ts` (adjustments)
- `engine/brush/brush-presets.ts` (presets)
- `../happypaint/todo.md` (the full history)

## Tools (19)
- [ ] brush
- [ ] eraser
- [ ] fill (flood fill)
- [ ] eyedropper
- [ ] move/transform (scale, rotate, warp, quad homography)
- [ ] pan
- [ ] smudge
- [ ] mixer
- [ ] gradient, with output to pixels or a fill layer
- [ ] liquify (all modes)
- [ ] text
- [ ] pen, with output to a shape layer, a path or pixels
- [ ] shape
- [ ] panel (comic panels)
- [ ] select-rect
- [ ] select-ellipse
- [ ] select-lasso
- [ ] select-poly
- [ ] select-wand

## Brushes
- [ ] All 118 presets, including Manga and the baked Procreate brushes, with identical parameters
- [ ] Dynamics: pressure, tilt, twist and velocity mapped to size, opacity and flow
- [ ] Velocity-synthesized pressure for devices without hardware pressure
- [ ] Computed and sampled tips
- [ ] Grain texture
- [ ] Airbrush (time-based ticks)
- [ ] Smoothing and the stabilizer
- [ ] Symmetry: mirror and radial
- [ ] Wet/oil media: wet, height (impasto), settle and bevel
- [ ] Smudge and mixer brushes
- [ ] Kubelka–Munk `pigment` brush blend
- [ ] Brush studio: an editor with live preview and a try-it pad
- [ ] Import `.abr`, Procreate `.brush` and `.brushset`, and `.happybrush`

## Layers
- [ ] Add, delete, reorder, duplicate and merge
- [ ] Nested groups
- [ ] Opacity, visibility and color labels
- [ ] Layer masks
- [ ] Clipping masks (clip families)
- [ ] Alpha lock
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

- [ ] All of them on the GPU, matching the CPU oracle

## Adjustments and filters (47 kinds)
Each kind works destructively, as an adjustment layer, and as a smart filter.
The exception is fill layers, which only work as layers.

- [ ] **Tonal:**
  - brightness-contrast, hue-saturation, black-white, invert
  - threshold, posterize, levels, curves
  - color-balance, gradient-map, color-to-alpha, replace-color, selective-color
- [ ] **Blur and sharpen:**
  - gaussian-blur, motion-blur, radial-blur
  - tilt-shift, iris-blur, field-blur, spin-blur
  - sharpen
- [ ] **Distort:** twirl, pinch, ripple, polar-coordinates
- [ ] **Stylize:**
  - noise, bloom, chromatic-aberration, halftone, tone, glitch
  - mosaic, crystallize, emboss, oil-paint, wind
  - line-art (Sobel/XDoG)
- [ ] **Render:** light-rays, lens-flare, clouds, fog, smoke, nebula, starfield, lightning
- [ ] **Fill:** gradient-fill, solid-fill, pattern-fill (pattern-fill must also load patterns from a URL)

## Selection
- [ ] Rect, ellipse, lasso, polygon and magic wand
- [ ] Add, subtract and intersect
- [ ] Feather, grow and smooth
- [ ] Saved selections
- [ ] Marching-ants overlay
- [ ] Fill, stroke and adjust within a selection, auto-rasterizing vector layers first

## Canvas and guides
- [ ] Pan, zoom and rotate at 60fps or better on large documents
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
- [ ] Full keyboard shortcuts (the happypaint keybindings map)

## History and recording
- [ ] Undo and redo with tile snapshots and pixel deltas
- [ ] Record a session as a command log, then replay it
- [ ] Export a timelapse as video

## Animation
- [ ] Animation Assist: onion skin and exposure-sheet tracks
- [ ] Frame export

## Color
- [ ] Color picker
- [ ] Palettes, with import and export
- [ ] Median-cut palette extraction
- [ ] ICC profiles
- [ ] CMYK soft-proof and press profiles
- [ ] TIFF CMYK export

## Files
- [ ] `.happypaint` v9: read and write, byte-compatible
- [ ] Autosave and the gallery
- [ ] `.happypaint-book`
- [ ] `.happypaint-rec`
- [ ] PSD: read and write layers, groups, masks, blend modes and text
- [ ] PNG, JPEG, WebP, TIFF and Netpbm, keeping ICC profiles and DPI
- [ ] GIF, MP4, WebM and PNG-zip export
- [ ] Clipboard: copy and paste images
- [ ] Camera and screen capture as a reference or import

## Cloud
- [ ] Google Drive provider, with desktop OAuth (loopback + PKCE)

## Comics
- [ ] Panel layouts and panel tool
- [ ] Balloons (in text layers)
