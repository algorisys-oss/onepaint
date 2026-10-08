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
- [x] pen: path layers (stroke/fill), anchor/handle editing, path menu (selection, fill, stroke with simulated pressure, mask, free transform, rasterize); Move keeps path layers vector under any affine map and text layers under rotate + uniform scale
- [x] shape (line, rect, ellipse and 3–12-sided polygon, stroked with the brush, optional fill)
- [x] panel (comic panels): draw a frame, cut along a line, drag corners to reshape and frame lines to move, with live gutter and border width
- [x] select-rect
- [x] select-ellipse
- [x] select-lasso
- [x] select-poly
- [x] select-wand

## Brushes
- [x] All 118 presets, including Manga and the baked Procreate brushes, with identical parameters (all 124 load with verbatim parameters, tips and grains, and every preset field is painted: wet, impasto, settle, fluid, dual brush, colour tips, grain movement)
- [x] Dynamics: pressure, tilt, twist and velocity mapped to size, opacity and flow (hardware pressure, tilt and twist arrive from the tablet backends, see Input)
- [x] Velocity-synthesized pressure for devices without hardware pressure
- [x] Document paper (spec 36): Smooth, Rough, Fine grain, Canvas, Linen with texture scale; dry brushes bite the tooth by their paper response, the canvas and exports show it, wet paint soaks and granulates into it
- [x] Computed and sampled tips (happypaint's generators baked to PNG)
- [x] Grain texture, with movement (spec 37)
- [x] Fill shape mode (F): a brush stroke fills its outline, with symmetry
- [x] Dual brush (mask tip), colour (RGBA) tips with recolour, spacing jitter, hue shift and two-colour load (spec 37)
- [x] Airbrush (time-based ticks)
- [x] Smoothing and the stabilizer
- [x] Symmetry: mirror and radial (angle, centre, 2–24 spokes; Y toggles)
- [x] Wet/oil media (spec 35): wet presets pick up and drag paint; impasto height with relief lighting, blender levelling, eraser carving and `.happypaint` round trip; Bevel & Emboss from alpha; watercolour settle (dried rim, granulation); live watercolour and live oils that bloom, flow, re-wet and dry after the pen lifts, with the tilt pad and Dry beside the tabs (GPU compute, CPU fallback)
- [x] Smudge and mixer brushes
- [x] Kubelka–Munk pigment mixing (mixer reservoir and deposit, and the `pigment` blend mode)
- [x] Brush studio: an editor with live preview and a try-it pad (spec 27; Wet/Oil, Load/Pigment, Mix, Blender, Fluid and Paper controls; plus dual brush, colour tip, Space·J, Hue→Shift, Tip col and grain Movement, spec 37)
- [x] Import `.abr`, Procreate `.brush` and `.brushset`, and `.happybrush`; export `.happybrush` (spec 23; an ABR dual brush and wet edges are kept as `maskTip` and `settle` but not painted yet, see divergence 7)

## Layers
- [x] Add, delete, reorder, duplicate and merge
- [x] Nested groups (with drag-and-drop into and out of groups)
- [x] Opacity, visibility and color labels
- [x] Layer masks (add, which can come from the selection; paint, enable, apply, delete)
- [x] Clipping masks (clip families)
- [x] Alpha lock
- [x] Off-canvas pixel overflow (transforms keep off-canvas content; saved as the `overflow` chunk)
- [x] Text layers: multiple rich-text objects and balloons, vector until baked (per-run bold, italic, underline, strikethrough, size, font and colour on a selection; spec 28)
- [x] Vector path layers, plus path ops
- [x] Adjustment layers and fill layers (colour, fill and spatial kinds all composite live)
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
- [x] color-to-alpha, replace-color, selective-color (pipette samples the target from the canvas; Selective Colour+ ranges can be added and removed)
- [~] radial-blur, tilt-shift, iris-blur, field-blur, spin-blur (no on-canvas handles; centres and pins are sliders)
- [~] **Distort:** twirl, pinch, ripple, polar-coordinates (no on-canvas handles)
- [x] **Stylize:** bloom, chromatic-aberration, halftone, tone, glitch, mosaic, crystallize, emboss, oil-paint, wind, line-art (Sobel/XDoG)
- [~] **Render:** light-rays, lens-flare, clouds, fog, smoke, nebula, starfield, lightning (no on-canvas handles; the lightning raster replaces Canvas2D)
- [x] **Fill:** gradient-fill, solid-fill, pattern-fill (built-ins and imported image tiles, stored as PNG data URLs fit within 512 px)
- [x] Smart-filter stacks (`layer.filters`)

## Selection
- [x] Rect, ellipse, lasso, polygon and magic wand
- [x] Add, subtract and intersect
- [x] Feather, grow and smooth
- [x] Saved selections (save, load with replace/add/subtract, delete; kept in the file; follow canvas/image size)
- [x] Marching-ants overlay
- [x] Fill, stroke and adjust within a selection, auto-rasterizing vector layers first (paint, smudge, liquify, fill, gradient, clear and filters rasterize text/path layers; Move keeps them vector)

## Canvas and guides
- [~] Pan, zoom and rotate at 60fps or better on large documents (pan, zoom and rotate all work; not yet measured on 8k documents)
- [x] Grid (size, subdivisions, snap to grid)
- [x] Symmetry guides (axis/spoke overlay)
- [x] 1–5-point perspective with stroke snapping (soft and hard, vanishing-point editing, 5-point fish-eye arcs)
- [x] Horizontal/vertical snap
- [x] Rulers, guide lines (drag from rulers, edit/swing/delete, snapping, undo) and measure
- [x] Quick-shape: hold to straighten a stroke into a line, ellipse or polygon
- [x] Speed lines (focus and speed lines as editable path layers, matching happypaint's geometry)
- [x] Reference window (image or canvas snapshot, pan/zoom, live colour pick; Alt+R)
- [x] Multi-document tabs (New/Open add a tab unless the current drawing is blank; Ctrl+Alt+arrows switch; closing asks when unsaved)

## Input
- [~] Pen pressure, tilt, twist and the eraser end:
  - [ ] Windows Ink/Wintab (pressure through winit's pen input; tilt, twist and the eraser end not yet)
  - [ ] macOS
  - [ ] X11
  - [x] Wayland (tablet protocol: pressure, tilt, twist, eraser end; spec 07)
- [x] Coalesced (high-rate) pen samples (Wayland: every tablet sample is a stroke point)
- [x] Multi-touch gestures with Procreate's vocabulary:
  - pinch to zoom and rotate, two-finger pan
  - two-finger tap to undo (hold two fingers to repeat), three-finger tap to redo
  - touch-and-hold for the eyedropper (400 ms, with a loupe)
  - three-finger swipe down for the copy/paste menu
  - four-finger tap to toggle the UI
- [x] Palm rejection while a pen is active (fingers ignored for 1.5 s after pen input)
- [x] Full keyboard shortcuts (the happypaint keybindings map), rebindable in Settings (spec 34)

## History and recording
- [x] Undo and redo with tile snapshots and pixel deltas (copy-on-write tile snapshots)
- [x] Record a session as a command log, then replay it (own `.onepaint-rec`; exact replay)
- [~] Export a timelapse as video (animated GIF; no MP4)

## Animation
- [~] Animation Assist: flipbook frames, holds, background/foreground, onion skins (tint, front, muted), playback (loop/ping-pong/once), timeline strip; exposure-sheet tracks composite and onion per track, timeline markers and a play range (playback and range export), the exposure-sheet grid (convert, retime, add sheets and tracks); out-of-pegs and batch timing ops are not built
- [x] Frame export (animated GIF, PNG sequence zip)

## Color
- [x] Color picker
- [x] Palettes, with import (.swatches, .aco, .ase, .gpl) and export (.swatches)
- [x] Median-cut palette extraction (from the canvas or an image file)
- [ ] ICC profiles
- [~] CMYK soft-proof and press profiles (built-in coated press, print preview, paper and ink simulation, gamut warning, Ctrl+Y value check; user ICC press profiles not yet; spec 32)
- [x] TIFF CMYK export

## Files
- [x] `.happypaint` v9: read and write (v1–v9 read; v9 write; JSON-level compatible)
- [x] Autosave and the gallery (crash-recovery autosave; a gallery of recent drawings with thumbnails, shown at launch)
- [x] `.happypaint-book` (open as tabs; save all tabs as a book)
- [ ] `.happypaint-rec`
- [~] PSD: read and write layers, groups, masks, blend modes, clipping, DPI and simple adjustments (8/16-bit RGB/grey read, PSB read; text exported as pixels)
- [~] PNG, JPEG, WebP, TIFF and Netpbm, keeping ICC profiles and DPI (import all; export PNG/JPEG/TIFF/PPM with scale and DPI; no ICC embedding yet)
- [~] GIF, MP4, WebM and PNG-zip export (GIF and PNG-zip; no MP4/WebM)
- [x] Clipboard: copy and paste images (in-place paste, the system clipboard, layer via copy/cut)
- [ ] Camera and screen capture as a reference or import

## Cloud
- [ ] Google Drive provider, with desktop OAuth (loopback + PKCE)

## Comics
- [x] Panel layouts and panel tool: six page presets; panels clip their art to the frame, inset by half the gutter; one Millipen border layer that follows every edit; round-trips in `.happypaint`
- [x] Balloons (in text layers): speech, rounded, shout and thought shapes, tail, fill and rim (spec 28; the tail is aimed by angle, as in happypaint)

## Settings
- [x] Settings dialog, persisted as `settings.json` in the config dir (spec 34):
  - documents: tab strip on/off, maximum open tabs
  - brush: maximum size (10–10000), size relative to canvas or screen, force pressure for size / opacity
  - cursor: ring, crosshair, both or dot; paint colour; thickness 1–4
  - history: undo steps (5–1000)
- [x] Cursor traced from the tip shape (`cursorTipShape`), square cursor for square tips
- [x] Keyboard shortcut rebinding (spec 34)
- [x] Timeline metric: frames or seconds (spec 34)
