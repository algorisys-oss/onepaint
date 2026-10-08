# 29: Distort and mesh warp (slice 19)

The sources are `engine/paint-engine.ts:1845-1960, 2680-2730` and
`engine/compositor.ts:559` (`quadHomography`), relative to
`../happypaint/frontend/src/`.

## Maths (`engine::warp`)
- **`quad_homography(TL, TR, BR, BL)`:** the unit square onto the quad.
  - It is column-major: `[a, d, g, b, e, h, x0, y0, 1]`.
  - It is affine when the quad is a parallelogram.
  - When `min(1, g+1, h+1, g+h+1) ≤ 1e-3`, it degrades to affine, so w never
    reaches 0 (the sliver-cell guard).
- **`sample_warp`:** bicubic Catmull-Rom over the 4×4 lattice, with indices
  clamped. The edge segments bow slightly, as in happypaint.
- **Rendering:**
  - Each destination cell maps back through its inverse homography to its
    source uv rectangle, with bilinear sampling.
  - Distort is one cell; warp is a 12×12 grid of cells.
  - Pixels are claimed by the first cell that covers them.

## Session
- `BeginDistort` and `BeginWarp` switch the transform in progress. The quad
  or lattice is seeded from the current affine box, so there's no jump. That
  is an improvement on happypaint, which reseeds from the bounds.
- `SetWarp {Quad | Mesh}`.
- Commit and cancel are unchanged: one undo step.

## UI
- Clicking Move again opens the transform options: Free, Distort or Warp,
  plus Apply and Cancel.
- In distort and warp, handles are white dots (12 px hit radius, nearest
  wins). Dragging empty space moves all the points. The mesh draws its
  lattice curves.

## Acceptance
- happypaint's homography cases:
  - rectangle;
  - trapezoid (projective);
  - the sliver cell stays finite;
  - every cell of a pulled 12×12 mesh keeps w > 0.
- The identity lattice samples to the rect (middle segment).
- A distort seeds from a moved box, and a pinched top commits a trapezoid
  with undo.
- A mesh pull bulges the edge. A UI corner drag moves only that corner.
