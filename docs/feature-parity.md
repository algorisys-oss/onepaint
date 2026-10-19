# Feature parity with happypaint

onepaint must ship **every** happypaint feature. This file is the checklist.
Tick an item only when it works in the app, its tests pass, and its output
matches happypaint. If a feature changes in happypaint, update its item here.

Legend: `[x]` done, `[~]` partly done (the note says what is missing), `[ ]`
not started.

This list covers happypaint up to v0.188. What v0.188–v0.201.7 added (layer
styles, 8 blend modes and Pass Through, six adjustments, the new PSD codec,
absorbing paper, the menu bar and more) is in
`docs/parity-delta-2026-10-10.md`; fold it in here as each item lands.

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
- [x] pan (the Pan button, H, space-drag, middle-drag, two-finger scroll)
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
- [x] Every tool reachable from the toolbar, guarded by a test (happypaint's
  `components/toolbar/tool-reachability.test.ts`; the five selection shapes
  share the Select button, with written exemptions)

## Brushes
- [x] All 118 presets, including Manga and the baked Procreate brushes, with identical parameters (all 124 load with verbatim parameters, tips and grains, and every preset field is painted: wet, impasto, settle, fluid, dual brush, colour tips, grain movement)
- [x] Dynamics: pressure, tilt, twist and velocity mapped to size, opacity and flow (hardware pressure, tilt and twist arrive from the tablet backends, see Input)
- [x] Velocity-synthesized pressure for devices without hardware pressure
- [x] Document paper (specs 36, 57): Smooth, the ten procedural papers in millimetres (Cold Press, Hot Press, Rough, Khadi, Washi, Laid, Canvas, Toned Sand, Kraft, Student) with their sheet colours, mottle and flecks, Linen, and the classic papers for older documents; texture scale; Apply paper colour to Background; dry brushes bite the tooth by their paper response, the canvas and exports show it, wet paint soaks and granulates into it
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
- [x] Import `.abr`, Procreate `.brush` and `.brushset`, and `.happybrush`; export `.happybrush` (spec 23)

## Layers
- [x] Add, delete, reorder, duplicate and merge
- [x] Nested groups (with drag-and-drop into and out of groups)
- [x] Opacity, visibility and color labels
- [x] Layer masks (add, which can come from the selection; paint, enable, apply, delete)
- [x] Mask density and feather, and Blend If on layers, on the GPU, in PSD and in Properties (happypaint v0.202.0)
- [x] Vector masks: live paths with combine/subtract/intersect/exclude, invert and apply, from the Pen's Create Vector Mask, mapped by transforms, on the GPU and in PSD `vmsk` (happypaint v0.202.1)
- [x] Clipping masks (clip families)
- [x] Alpha lock
- [x] Off-canvas pixel overflow (transforms keep off-canvas content; saved as the `overflow` chunk)
- [x] Text layers: multiple rich-text objects and balloons, vector until baked (per-run bold, italic, underline, strikethrough, size, font and colour on a selection; spec 28)
- [x] Vector path layers, plus path ops
- [x] Adjustment layers and fill layers (colour, fill and spatial kinds all composite live)
- [x] Per-layer smart-filter stacks (add from any filter, toggle, edit, remove, bake; cached; exported)
- [x] Layer styles on layers and groups: all 10 effects, Fill opacity, Global Light, built-in and user presets, `.asl` import, Bake, on the GPU and in PSD `lfx2`/`lmfx` (happypaint v0.196; spec 56)

## Blend modes (29)
The modes are, in the layer menu's order (`layer-panel/blend-modes.ts`):

- normal, dissolve
- darken, multiply, color-burn, linear-burn, darker-color
- lighten, screen, color-dodge, add (linear dodge), lighter-color
- overlay, soft-light, hard-light, vivid-light, linear-light, pin-light, hard-mix
- difference, exclusion, subtract, divide
- hue, saturation, color, luminosity
- pass-through (groups only)
- pigment (brush only)

- [x] The CPU oracle, `.happypaint` and PSD keys for all 29; Pass Through groups composite their children onto the backdrop and fade with it below 100%, and stay isolated (as Normal) when a panel frame, ghost or clipped layer needs them as one picture (happypaint v0.200.1)
- [~] All of them on the GPU, matching the CPU oracle (all 28 modes, Pass Through groups, groups, clip families, masks, inherit alpha, ghost tints and strokes in progress composite on the GPU, as do colour adjustment, noise, screentone and fill layers, relief, paper and panel clips (specs 42–46); view modes run in the present pass (spec 48). Tiles with sand screentone, imported pattern images or ghost pegs still use the CPU compositor; spatial adjustment layers are rendered on the CPU, reused mid-stroke, and composite on the GPU as pixels)
- [x] Knockout None / Shallow / Deep on layers: punches to the group's base, or to the Background at the top level; on the GPU, in PSD (`knko`) and in Properties (happypaint v0.202.2)

## Adjustments and filters (52 kinds)
Each kind works destructively, as an adjustment layer, and as a smart filter.
The exception is fill layers, which only work as layers.

All 52 kinds have CPU references (specs 10 and 18) and are in the sectioned
adjustments menu. The colour kinds work destructively with a live preview,
are selection-aware, and work as live adjustment layers. The spatial kinds
work as destructive filters and as live adjustment layers (rendered over the
whole document before compositing). The generators render into a new layer as one undo step. Fill layers and
screentone are added as live layers.

- [x] brightness-contrast, hue-saturation, black-white, invert, threshold, posterize, levels, curves, color-balance, gradient-map
- [x] exposure, vibrance, photo-filter, channel-mixer (happypaint v0.199): CPU and GPU, panels, PSD `expA`/`vibA`/`phfl`/`mixr` both ways (Photo Filter's colour decoded as in v0.204.7)
- [x] gaussian-blur, motion-blur, sharpen, noise
- [x] color-to-alpha, replace-color, selective-color (pipette samples the target from the canvas; Selective Colour+ ranges can be added and removed)
- [x] radial-blur, tilt-shift, iris-blur, field-blur, spin-blur, with on-canvas handles (centres, the focus band, the ellipse, pins; spec 60)
- [x] **Distort:** twirl, pinch, ripple, polar-coordinates (centre and reach handles; spec 60)
- [x] **Stylize:** bloom, chromatic-aberration, halftone, tone, glitch, mosaic, crystallize, emboss, oil-paint, wind, line-art (Sobel/XDoG)
- [x] **Render:** light-rays, lens-flare, clouds, fog, smoke, nebula, starfield, lightning (centre handles for light rays and lens flare; the lightning raster replaces Canvas2D)
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
- [x] Export a timelapse as video (MP4 or animated GIF)

## Animation
- [x] Animation Assist: flipbook frames, holds, background/foreground, onion skins (tint, front, muted), playback (loop/ping-pong/once), timeline strip; exposure-sheet tracks composite and onion per track, timeline markers and a play range (playback and range export), the exposure-sheet grid (convert, retime, add sheets and tracks), batch timing ops (set duration, split, in-betweens, duplicate, delete) and out of pegs
- [x] Frame export (animated GIF, PNG sequence zip)

## Color
- [x] Color picker
- [x] Palettes, with import (.swatches, .aco, .ase, .gpl) and export (.swatches)
- [x] Median-cut palette extraction (from the canvas or an image file)
- [x] ICC profiles (sRGB/Display-P3 tags on PNG and JPEG export, embedded RGB profiles converted on import; spec 32)
- [x] CMYK soft-proof and press profiles (built-in coated press or a loaded ICC press, print preview, paper and ink simulation, gamut warning, Ctrl+Y value check; spec 32)
- [x] TIFF CMYK export

## Files
- [x] `.happypaint` v9: read and write (v1–v9 read; v9 write; JSON-level compatible)
- [x] Autosave and the gallery (crash-recovery autosave; a gallery of recent drawings with thumbnails, shown at launch)
- [x] `.happypaint-book` (open as tabs; save all tabs as a book)
- [ ] `.happypaint-rec`
- [~] PSD: read and write layers, groups, masks, blend modes, clipping, DPI, fill layers (solid and gradient live, patterns as pixels), and live adjustment layers: invert, posterize, threshold, brightness-contrast, levels, curves, hue-saturation, color-balance (midtones), black-white (Photoshop's weights and tint not kept yet), gradient map (solid; stop colours in RGB, HSB, Lab or grey), exposure, vibrance, photo filter and channel mixer (8/16-bit RGB/grey read, PSB read; text exported as pixels). An 8-bit RGB PSD saves back byte for byte when untouched, and keeps untouched layers' records (styles, text, smart objects) when edited (spec 51). Locks (`lspf`), colour labels (`lclr`) and guides (resource 1032) both ways; moving a guide rewrites only that resource. Every colour mode (Bitmap, Grayscale, Indexed, CMYK through the file's profile or the press, Multichannel, Duotone, Lab) and depth (1, 8, 16, 32-bit) reads (spec 61). Solid Color and Gradient fill layers both ways as live fills (`SoCo`, `GdFl`; patterns and shape layers stay pixels, as happypaint). Missing against happypaint's codec: writing in the file's own mode and depth, PSB export, and untouched round trips of non-8-bit-RGB files.
- [x] PNG, JPEG, WebP, TIFF and Netpbm, keeping ICC profiles and DPI (import all, converting embedded RGB profiles; export PNG/JPEG/TIFF/PPM with scale, DPI and ICC tags)
- [x] GIF, MP4, WebM and PNG-zip export (GIF, MP4 and PNG-zip; WebM is covered by MP4, divergence 20)
- [x] Clipboard: copy and paste images (in-place paste, the system clipboard, layer via copy/cut)
- [ ] Camera and screen capture as a reference or import

## Cloud
- [ ] Google Drive provider, with desktop OAuth (loopback + PKCE)

## Comics
- [x] Panel layouts and panel tool: the layouts open when the tool is picked; six page presets; panels clip their art to the frame, inset by half the gutter; one Millipen border layer that follows every edit; round-trips in `.happypaint`
- [x] Balloons (in text layers): speech, rounded, shout and thought shapes, tail, fill and rim (spec 28; the tail is aimed by angle, as in happypaint)

## Workspace and panels
Source: `components/toolbar/toolbar.tsx`, `components/dock/`, `state/dock-store.ts`.

- [x] Movable toolbar: drag it by its grip, double-click the grip to switch
  between horizontal and vertical, dock it to the left or right edge as a
  rail, resize it by whole columns (divergence 41)
- [x] Dockable panels (layers, properties, brushes, reference, colour,
  palettes, align, plus guides and tool options): float, dock left or right,
  stack or tab, collapse, resize, fold, and tab floating windows together;
  the layout persists as `layout.json` (divergences 42, 43)
- [x] Properties panel: the selected adjustment or fill layer's settings in a
  panel of their own, docked right by default
- [x] Align and distribute layers: align edges and centres to the canvas or
  the selection, distribute centres (3+ layers), equal gaps across and down;
  Ctrl/⌘- and Shift-click pick layers; one undo step; text and paths stay vector
- [x] Toolbars never overlap on narrow windows; below 900 px the tool bar is a
  strip above the status bar and docked panels open as popovers
- [x] Status bar (`components/status-bar`): save state, tool, brush and size,
  colour with ColorDrop, fill threshold and grow, layer, cursor, measure,
  document properties, layer count, rotation dial, zoom −/%/+, undo/redo,
  vertical/horizontal symmetry and move axis, edit vanishing points, fill
  mode, value check, version; narrow windows fold details into "…"
- [x] ColorDrop: drag the colour onto the canvas to fill, with a live preview;
  hold to lock the seed and slide to set the threshold; one undo step

## Help
- [x] Shortcuts and gestures reference
- [x] Guide (happypaint's help sections): tool bar, status bar, ColorDrop,
  panels and docking, Panel (manga), manga and comics, Properties, align and
  distribute

## Settings
- [x] Settings dialog, persisted as `settings.json` in the config dir (spec 34):
  - documents: tab strip on/off, maximum open tabs
  - brush: maximum size (10–10000), size relative to canvas or screen, force pressure for size / opacity
  - cursor: ring, crosshair, both or dot; paint colour; thickness 1–4
  - history: undo steps (5–1000)
- [x] Cursor traced from the tip shape (`cursorTipShape`), square cursor for square tips
- [x] Keyboard shortcut rebinding (spec 34)
- [x] Timeline metric: frames or seconds (spec 34)
