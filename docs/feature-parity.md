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
- [x] move/transform (scale, rotate, warp, quad homography)
- [x] pan (space-drag, middle-drag, two-finger scroll)
- [x] smudge
- [x] mixer (Wet/Load/Mix, the 12 Photoshop combinations, Keep colour, Clean; no Alt-click patch load yet)
- [x] gradient, with output to pixels or a fill layer (5 shapes, colour→clear or →BG, reverse, selection-aware)
- [x] liquify (push, twirl left/right, pinch, expand, reconstruct)
- [x] text (create, edit, move, style, wrap, outline, background, rasterise)
- [~] pen: path layers (stroke/fill), anchor/handle editing, path menu (selection, fill, stroke with simulated pressure, mask, rasterize); no free-transform path or speed lines yet
- [x] shape (line, rect, ellipse and 3–12-sided polygon, stroked with the brush, optional fill)
- [ ] panel (comic panels)
- [x] select-rect
- [x] select-ellipse
- [x] select-lasso
- [x] select-poly
- [x] select-wand

## Brushes
- [~] All 118 presets, including Manga and the baked Procreate brushes, with identical parameters (all 124 load with verbatim parameters, tips and grains; the 14 wet/oil presets paint without wet media)
- [~] Dynamics: pressure, tilt, twist and velocity mapped to size, opacity and flow (all implemented; hardware tilt and pressure need a tablet backend, see Input)
- [x] Velocity-synthesized pressure for devices without hardware pressure
- [x] Computed and sampled tips (happypaint's generators baked to PNG)
- [x] Grain texture
- [x] Airbrush (time-based ticks)
- [x] Smoothing and the stabilizer
- [x] Symmetry: mirror and radial (angle, centre, 2–24 spokes; Y toggles)
- [ ] Wet/oil media: wet, height (impasto), settle and bevel
- [x] Smudge and mixer brushes
- [x] Kubelka–Munk pigment mixing (mixer reservoir and deposit, and the `pigment` blend mode)
- [ ] Brush studio: an editor with live preview and a try-it pad
- [x] Import `.abr`, Procreate `.brush` and `.brushset`, and `.happybrush`; export `.happybrush` (spec 23; an ABR dual brush and wet edges are kept as `maskTip` and `settle` but not painted yet, see divergence 7)

## Layers
- [x] Add, delete, reorder, duplicate and merge
- [x] Nested groups (with drag-and-drop into and out of groups)
- [x] Opacity, visibility and color labels
- [x] Layer masks (add, which can come from the selection; paint, enable, apply, delete)
- [x] Clipping masks (clip families)
- [x] Alpha lock
- [ ] Off-canvas pixel overflow
- [~] Text layers: multiple rich-text objects and balloons, vector until baked (rich runs render; balloons are kept in files but not drawn; no rich-text editing UI yet)
- [x] Vector path layers, plus path ops
- [~] Adjustment layers and fill layers (colour and fill kinds composite live; spatial kinds are kept but not rendered)
- [x] Per-layer smart-filter stacks (add from any filter, toggle, edit, remove, bake; cached; exported)

## Blend modes (20)
The modes are:

- normal, multiply, screen, overlay, add
- darken, lighten, color-dodge, color-burn
- hard-light, soft-light, difference, exclusion
- linear-burn, divide
- hue, saturation, color, luminosity
- pigment (brush only)

- [~] All of them on the GPU, matching the CPU oracle (all 19 layer modes are done on the CPU and the composite is shown on the GPU; GPU compositing is not done yet)

## Adjustments and filters (48 kinds)
Each kind works destructively, as an adjustment layer, and as a smart filter.
The exception is fill layers, which only work as layers.

All 48 kinds have CPU references (specs 10 and 18) and are in the sectioned
adjustments menu. The colour kinds work destructively with a live preview,
are selection-aware, and work as live adjustment layers. The spatial kinds
work as destructive filters only; as layers they are kept but not rendered
yet. The generators render into a new layer as one undo step. Fill layers and
screentone are added as live layers.

- [x] brightness-contrast, hue-saturation, black-white, invert, threshold, posterize, levels, curves, color-balance, gradient-map
- [x] gaussian-blur, motion-blur, sharpen, noise
- [~] color-to-alpha, replace-color, selective-color (no canvas eyedropper for the target yet; the panel edits the first selective-colour sample only)
- [~] radial-blur, tilt-shift, iris-blur, field-blur, spin-blur (no on-canvas handles; centres and pins are sliders)
- [~] **Distort:** twirl, pinch, ripple, polar-coordinates (no on-canvas handles)
- [x] **Stylize:** bloom, chromatic-aberration, halftone, tone, glitch, mosaic, crystallize, emboss, oil-paint, wind, line-art (Sobel/XDoG)
- [~] **Render:** light-rays, lens-flare, clouds, fog, smoke, nebula, starfield, lightning (no on-canvas handles; the lightning raster replaces Canvas2D)
- [~] **Fill:** gradient-fill, solid-fill, pattern-fill (imported pattern images are not decoded; the built-ins are)
- [x] Smart-filter stacks (`layer.filters`)

## Selection
- [x] Rect, ellipse, lasso, polygon and magic wand
- [x] Add, subtract and intersect
- [x] Feather, grow and smooth
- [x] Saved selections (save, load with replace/add/subtract, delete; kept in the file; follow canvas/image size)
- [x] Marching-ants overlay
- [~] Fill, stroke and adjust within a selection, auto-rasterizing vector layers first (fill, clear and painting are clipped; adjustments and vector layers come later)

## Canvas and guides
- [~] Pan, zoom and rotate at 60fps or better on large documents (pan, zoom and rotate all work; not yet measured on 8k documents)
- [x] Grid (size, subdivisions, snap to grid)
- [x] Symmetry guides (axis/spoke overlay)
- [~] 1–5-point perspective with stroke snapping (soft and hard; vanishing-point editing; fish-eye curved mode still to do)
- [x] Horizontal/vertical snap
- [x] Rulers, guide lines (drag from rulers, edit/swing/delete, snapping, undo) and measure
- [x] Quick-shape: hold to straighten a stroke into a line, ellipse or polygon
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
- [~] Full keyboard shortcuts (the happypaint keybindings map) (all default bindings except F silhouette fill, multi-document tabs, markers and mark in/out; no rebinding UI yet)

## History and recording
- [x] Undo and redo with tile snapshots and pixel deltas (copy-on-write tile snapshots)
- [x] Record a session as a command log, then replay it (own `.onepaint-rec`; exact replay)
- [~] Export a timelapse as video (animated GIF; no MP4)

## Animation
- [~] Animation Assist: flipbook frames, holds, background/foreground, onion skins (tint, front, muted), playback (loop/ping-pong/once), timeline strip; exposure-sheet tracks composite and onion per track, but the track grid UI, markers and out-of-pegs are not built
- [x] Frame export (animated GIF, PNG sequence zip)

## Color
- [x] Color picker
- [x] Palettes, with import (.swatches, .aco, .ase, .gpl) and export (.swatches)
- [x] Median-cut palette extraction (from the canvas or an image file)
- [ ] ICC profiles
- [ ] CMYK soft-proof and press profiles
- [ ] TIFF CMYK export

## Files
- [x] `.happypaint` v9: read and write (v1–v9 read; v9 write; JSON-level compatible)
- [ ] Autosave and the gallery
- [ ] `.happypaint-book`
- [ ] `.happypaint-rec`
- [~] PSD: read and write layers, groups, masks, blend modes, clipping, DPI and simple adjustments (8/16-bit RGB/grey read, PSB read; text exported as pixels)
- [~] PNG, JPEG, WebP, TIFF and Netpbm, keeping ICC profiles and DPI (import all; export PNG/JPEG/TIFF/PPM with scale and DPI; no ICC embedding yet)
- [~] GIF, MP4, WebM and PNG-zip export (GIF and PNG-zip; no MP4/WebM)
- [x] Clipboard: copy and paste images (in-place paste, the system clipboard, layer via copy/cut)
- [ ] Camera and screen capture as a reference or import

## Cloud
- [ ] Google Drive provider, with desktop OAuth (loopback + PKCE)

## Comics
- [ ] Panel layouts and panel tool
- [ ] Balloons (in text layers)
