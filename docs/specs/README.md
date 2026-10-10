# Specs

onepaint is built spec-first and then test-first:

1. Write or extend the spec here, with happypaint `file:line` sources and
   acceptance criteria.
2. Turn the acceptance criteria and goldens into failing Rust tests.
3. Implement until the tests pass, along with clippy and fmt.
4. Commit and push, one milestone at a time.

The happypaint references are relative to `../happypaint/frontend/src/`.

| # | Spec | Crates |
|---|---|---|
| 00 | [Slice-1 scope and architecture](00-slice1.md) | all |
| 01 | [Raster tiles and color](01-raster-color.md) | raster, color |
| 02 | [Compositing and blend modes](02-compositor.md) | doc, compose |
| 03 | [Brush, stroke and input](03-brush-stroke.md) | stroke, brush, input |
| 04 | [History](04-history.md) | history |
| 05 | [.happypaint v9 and PNG](05-file-format.md) | formats |
| 06 | [Engine API](06-engine.md) | engine |
| 07 | [Canvas, view and UI](07-canvas-ui.md) | gpu, ui_kit, apps/onepaint |
| 08 | [Selection, fill, eyedropper](08-selection-fill.md) | selection, engine, app |
| 09 | [Transform, import, clipboard, layer DnD](09-transform-import.md) | engine, formats, app |
| 10 | [Adjustments and filters](10-adjustments.md) | filters, compose, engine |
| 11 | [Layer masks](11-masks.md) | compose, engine |
| 12 | [Gradient, shape, quick-shape](12-gradient-shape.md) | filters, stroke, engine |
| 13 | [GPU compositor](13-gpu-compositor.md) | compose, gpu |
| 14 | [Text layers](14-text.md) | text, engine, app |
| 15 | [PSD import and export](15-psd.md) | formats, engine, app |
| 16 | [Smudge, mixer, pigment, symmetry](16-smudge-symmetry.md) | color, brush, engine, app |
| 17 | [Liquify](17-liquify.md) | filters, engine, app |
| 18 | [Remaining filters, generators, fill layers](18-filters.md) | filters, compose, engine, app |
| 19 | [Guides: grid, perspective, snapping](19-guides.md) | stroke, engine, app |
| 20 | [Saved selections](20-saved-selections.md) | selection, engine, app |
| 21 | [Palettes](21-palettes.md) | color, formats, app |
| 22 | [Pen tool and vector paths](22-pen-paths.md) | stroke, engine, app |
| 23 | [Brush import and export](23-brush-import.md) | brush, formats, app |
| 24 | [Animation Assist](24-animation.md) | compose, engine, formats, app |
| 25 | [Export dialog](25-export.md) | formats, engine, app |
| 26 | [Recording, replay, timelapse](26-recording.md) | engine, app |
| 27 | [Brush studio](27-brush-studio.md) | brush, engine, app |
| 28 | [Speech balloons and rich-text editing](28-balloons-rich-text.md) | text, engine, app |
| 29 | [Distort and mesh warp](29-distort-warp.md) | engine, app |
| 30 | [Comic panels](30-panels.md) | panel, compose, engine, app |
| 32 | [CMYK soft-proof, value check, CMYK TIFF](32-cmyk-proof.md) | color, formats, app |
| 34 | [Settings](34-settings.md) | engine, app |
| 35 | [Wet and oil media](35-wet-media.md) | brush, compose, formats, engine, app |
| 36 | [Document paper](36-paper.md) | doc, brush, compose, engine, app |
| 37 | [Dual brush, colour tips, grain movement](37-brush-extras.md) | brush, app |
| 38 | [Cached composite below and above the active layer](38-composite-cache.md) | compose, gpu, engine, app |
| 39 | [Level-of-detail compositing](39-lod-compositing.md) | gpu, app |
| 40 | [Composite sheets past the GPU texture limit](40-composite-sheets.md) | gpu, app |
| 41 | [Opening files in the background](41-background-open.md) | formats, engine, app |
| 42 | [Colour adjustment layers on the GPU](42-gpu-colour-adjustments.md) | filters, compose, gpu |
| 43 | [Paper shading on the GPU](43-gpu-paper.md) | brush, compose, gpu |
