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
