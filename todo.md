# onepaint todo

The working list, grouped by area. `[x]` is done and `[ ]` is pending. The
item-by-item parity checklist with notes is `docs/feature-parity.md`. When
an item changes there, change it here too.

## 1. Feature parity with happypaint

### 1.1 Tools
- [x] Brush, eraser, smudge, mixer
- [x] Fill, gradient (to pixels or a fill layer)
- [x] Eyedropper (tool, Alt-click, Alt-drag, touch-and-hold)
- [x] Move/transform (scale, rotate, warp, quad homography), pan, liquify
- [x] Text, pen (path layers), shape, panel
- [x] Selection tools: rect, ellipse, lasso, polygon, wand
- [ ] Mixer: Alt-click to load the reservoir from a patch (needs a per-pixel
      reservoir in `brush::smudge`; it holds one colour today)
- [x] Pan tool button on the toolbar
- [x] Tool reachability test: every `Tool` has a button or a written exemption

### 1.2 Brushes
- [x] All presets (124 load with verbatim parameters, tips and grains)
- [x] Dynamics: pressure, tilt, twist, velocity; synthesized pressure
- [x] Tips, grain with movement, dual brush, colour tips, fill shape mode
- [x] Airbrush, smoothing, stabilizer, symmetry (mirror and radial)
- [x] Wet media: impasto, settle, live watercolour and oils (GPU and CPU)
- [x] Kubelka–Munk pigment mixing
- [x] Document paper (spec 36)
- [x] Brush studio (spec 27, 37)
- [x] Import `.abr`, `.brush`, `.brushset`, `.happybrush`; export `.happybrush`

### 1.3 Layers
- [x] Add, delete, reorder, duplicate, merge; nested groups with drag-and-drop
- [x] Opacity, visibility, colour labels, alpha lock
- [x] Layer masks, clipping masks (clip families)
- [x] Off-canvas overflow
- [x] Text layers (rich text, balloons), vector path layers with path ops
- [x] Adjustment and fill layers, smart-filter stacks

### 1.4 Blend modes
- [x] All 20 modes on the CPU (19 layer modes plus brush-only pigment)
- [ ] All of them on the GPU in every case (see 2.1 for the fallbacks left)

### 1.5 Adjustments and filters (48 kinds)
- [x] CPU references for all 48, in the sectioned menu
- [x] Colour kinds: destructive, live preview, selection-aware, live layers
- [x] Blur, sharpen, noise; colour-to-alpha, replace and selective colour
- [x] Stylize, fill kinds, smart-filter stacks
- [x] Spatial kinds as live adjustment layers
- [ ] On-canvas handles
  - [ ] Radial, tilt-shift, iris, field and spin blur (centres and pins)
  - [ ] Distort: twirl, pinch, ripple, polar coordinates
  - [ ] Render: light rays, lens flare, clouds, fog, smoke, nebula, starfield, lightning

### 1.6 Selection
- [x] Shapes and the wand; add, subtract, intersect
- [x] Feather, grow, smooth; saved selections; marching ants
- [x] Paint, fill and filter within a selection (auto-rasterizing vector layers)

### 1.7 Canvas and guides
- [x] Pan, zoom, rotate
- [ ] Measure pan, zoom and rotate at 60 fps on 8k × 8k with 100+ layers
- [x] Grid and snapping, symmetry guides, 1–5-point perspective
- [x] Rulers, guide lines, measure; quick-shape; speed lines
- [x] Reference window; multi-document tabs

### 1.8 Input
- [x] Wayland tablets: pressure, tilt, twist, eraser end
- [ ] Windows Ink/Wintab: tilt, twist and eraser end (pressure works)
- [ ] macOS tablet backend
- [ ] X11 tablet backend
- [x] Coalesced pen samples, multi-touch gestures, palm rejection
- [x] Keyboard shortcuts, rebindable

### 1.9 History and recording
- [x] Undo and redo with copy-on-write tile snapshots
- [x] Record and replay sessions (`.onepaint-rec`)
- [x] Timelapse export (MP4, GIF)
- [ ] Read happypaint recordings (`.happypaint-rec`)

### 1.10 Animation
- [x] Animation Assist: frames, holds, onion skins, playback, timeline,
      exposure sheets, markers and range, batch timing ops, out of pegs
- [x] Frame export (GIF, PNG sequence zip, MP4)

### 1.11 Colour
- [x] Picker, palettes with import and export, median-cut extraction
- [x] ICC profiles, CMYK soft-proof and press profiles, TIFF CMYK export

### 1.12 Files
- [x] `.happypaint` v1–v9 read, v9 write; `.happypaint-book`
- [x] Autosave and the gallery
- [x] PNG, JPEG, WebP, TIFF, Netpbm with ICC and DPI; GIF, MP4, PNG-zip
- [x] Clipboard copy and paste
- [x] PSD read and write: layers, groups, masks, modes, clipping, DPI, simple adjustments
- [ ] PSD: editable text on export (text is exported as pixels)
- [ ] Camera and screen capture as a reference or import

### 1.13 Cloud
- [ ] Google Drive provider with desktop OAuth (loopback + PKCE)

### 1.14 Comics
- [x] Panel layouts and the panel tool
- [x] Balloons

### 1.15 Workspace and panels
- [x] Movable toolbar: grip drag, horizontal/vertical, dock as a left or right rail
- [x] Dockable panels: float, dock, tab, collapse, resize; persist the layout
- [x] Properties panel for adjustment and fill layers
- [x] Align and distribute layers (engine command plus panel)
- [x] No toolbar overlap on narrow windows
- [x] Status bar parity (symmetry axes and move axis, zoom, rotation dial, fill mode, value check, …)
- [x] ColorDrop from the status-bar swatch

### 1.16 Settings
- [x] Settings dialog and `settings.json`, cursor shapes, shortcut rebinding, timeline metric

## 2. Engine and performance

### 2.1 GPU compositor (spec 13)
- [x] Tile programs (`compose::program`) with a CPU interpreter checked against the oracle
- [x] Compute-pass interpreter with resident tile atlases and LRU eviction
- [x] All 19 modes, groups, clip families, masks, inherit alpha, ghost tints, strokes in progress
- [x] Lean pipeline for plain normal layers (4096² × 20 layers: 130 ms → 60 ms on Iris Xe)
- [x] Oracle tests and a criterion bench
- [ ] Remaining CPU fallbacks on the GPU
  - [x] Colour adjustment layers, pixel-only kinds (spec 42)
  - [x] Noise, solid and gradient fill layers (spec 46)
  - [x] Screentone (not the sand shape) (spec 46)
  - [x] Built-in pattern fill layers (spec 46)
  - [ ] Imported pattern fills and sand screentone
  - [~] Spatial adjustment layers (spec 52): Gaussian blur, motion blur,
        sharpen, emboss, mosaic, ripple, halftone, crystallize, bloom and
        line art layers run live on the GPU, stacked too, at the top level or
        in isolated groups; a lone twirl, pinch, polar, chromatic
        aberration, radial, spin, tilt-shift, iris or field blur layer too
        (spec 53; heavy ones wait mid-stroke in large documents). Oil
        paint, wind, glitch, light rays and ones in Pass Through groups
        still render on the CPU
  - [x] Relief lighting (impasto height) (spec 45)
  - [x] Paper shading (spec 43)
  - [x] Partial panel clips (spec 44)
  - [ ] Ghost pegs (out-of-pegs onion offsets)
  - [x] Present-only view modes (spec 48)
- [x] Opacity-drag bench (`cargo bench -p onepaint-gpu --bench opacity_drag`)
- [ ] Opacity drag at 60 fps on 8k × 100 layers. Baseline on Iris Xe
      (2026-10-10): 128 ms a frame when the distinct tiles fit the 1 GiB
      budget, 2.4 s when they don't (the LRU evicts and re-uploads every
      frame); planning is 2–3 ms of it
  - [x] Cache the composite below and above the active layer per tile, so a
        drag blends three inputs instead of ~30 layers (spec 38): drag frame
        median 210 → 28 ms (fits), 339 → 28 ms (past the budget)
  - [x] Spec 38 warm-up when the active layer changes
  - [x] Cheaper planning: ranges instead of copies, parallel planning
        (5 → 1.8 ms). Drag frame now 19.7 ms median (16.8 min) when the
        tiles fit, 28 ms past the budget
  - [x] Composite zoomed-out views at reduced resolution (spec 39): a
        level-3 drag frame is ~10× cheaper than level 0; previews refine
        to exact when idle
  - [x] Re-measure spec 38/39 on a quiet machine (browsers closed); the iGPU
        clock stays at 500–650 MHz under the bench, so the numbers are relative
  - [x] Back-to-back throughput line in the bench trace (14.5 ms a split frame)
  - [ ] Bench with the GPU clock pinned (needs root; "balanced" keeps it at
        550–700 MHz even under load)
  - [x] Stop the cyclic LRU thrash past the budget: batches already keep
        each dispatch within the atlas; over-budget frames now alternate
        direction (spec 38)
  - [ ] Profile the per-tile GPU cost (≈20 µs per tile-layer; far from the
        memory bandwidth limit)
- [ ] From the photocraft comparison (2026-10-10; see handoff.md)
  - [x] Split the canvas composite into sheets for documents above the GPU
        texture limit (spec 40); the app now asks for the adapter's limits
  - [x] Store 8-bit documents as 8-bit tiles (spec 49; halves RAM)
  - [x] 8-bit atlases on the GPU (halves VRAM for 8-bit documents; spec 49)
  - [x] Wet GPU stepper: key its paper buffer on the grain's `Arc`, not its
        address (as specs 43 and 48 now do)
  - [x] Gamma-space compositing for 8-bit documents (spec 50, B)
  - [x] Check a full refresh's working set fits before starting; route the
        rest to the CPU instead of skipping (spec 54)
  - [x] Bounded submits and GPU device-loss handling (spec 54: save at
        once and ask for a restart; eframe can't make a new device)
  - [x] Open files in the background with progress and cancel (spec 41)
  - [ ] Perf budgets with a baseline and a regression gate (`perf/budgets.toml`)
  - [x] Skip layers under fully opaque tiles when planning (spec 47)
- [ ] Decide the GPU memory budget on integrated GPUs (cache now 384 MiB; spec 38)
- [ ] Decide whether to drop the CPU tile copy once GPU-resident

### 2.2 GPU painting
- [x] Live wet media on the GPU (spec 35)
- [ ] GPU stamping (instanced dabs into the stroke slot), checked against the CPU rasteriser
- [ ] GPU smudge, liquify and filter passes with interactive previews
- [ ] Flood fill, magic wand and feathering as compute passes

### 2.3 Platforms
- [x] Linux (X11 and Wayland)
- [ ] Windows: verify the full app and tablet input
- [ ] macOS: verify the full app and tablet input
- [ ] WASM/WebGPU build

## 3. Beyond parity

### 3.1 Vector support
Make onepaint a real tool for both vector and raster work, not only a
painter with path layers.
- [x] Vector path layers with anchor/handle editing and path ops (parity)
- [x] Text and path layers stay vector under Move
- [ ] Scope it: write a spec for the vector model and tools
- [ ] Vector shapes and objects as first-class layer content
- [ ] Resolution-independent rendering at any zoom
- [ ] Boolean operations, node tools, alignment and distribution
- [ ] Vector strokes styled from brushes
- [ ] SVG and PDF import and export

## happypaint v0.188 → v0.201.7 parity delta (2026-10-10)
See `docs/parity-delta-2026-10-10.md`.
- [x] Bugs: new blend modes lost on save (kept by name until implemented);
      untouched transform fades a soft selection; fill layers export to PSD
      empty; damp veil 0.12 in 8-bit; granulation sign
- [ ] 8 blend modes + Pass Through groups
- [ ] Layer styles (v0.196.x)
- [~] New adjustments: Exposure, Vibrance, Photo Filter, Channel Mixer done
      (CPU, GPU, PSD expA/vibA/phfl/mixr, panels; 2026-10-18). Left: CMYK
      Selective Color, Color Lookup; B&W hue weights + tint; Color Balance ranges
- [ ] PSD codec parity (modes, 32-bit, round trip, live adjustments, fills, styles)
- [ ] Wet physics v0.199 (paper absorbs, halo, wash fill, tip-shaped dabs)
- [ ] Ten procedural papers, sheet colour, flecks and mottle
- [ ] Workspace: menu bar, options row, left tool rail, side-slider extras,
      scrollbars, angle dial, in-app dialogs, manual, brush search
- [x] Refresh stale divergences (1, 3, 6, 15, 41), spec 15, feature-parity.md

