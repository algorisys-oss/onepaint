# 30: Comic panels (manga P6)

The behaviour is in happypaint's `docs/manga-spec.md` §P6. The sources are
relative to `../happypaint/frontend/src/`:

- `engine/panel/panel-geometry.ts` (geometry, cited as G),
- `engine/panel/panel-meta.ts` (metadata, M),
- `engine/panel/panel-renderer.ts` (the derived mask, R),
- `engine/paint-engine.ts:6606-6895` (the session operations, PE),
- `engine/compositor.ts:1527-1550` (the group clip, C),
- `engine/selection/selection.ts:276-345` (`rasterizeContoursCoverage`, S),
- `components/canvas/paint-canvas.tsx:318-320, 3517-3596, 4222-4265` (the
  tool, PC),
- `components/toolbar/toolbar.tsx:324-331, 1977-1984, 2080-2096` (presets and
  the tool button, TB).

A panel is a **group** carrying a `panel` spec. The group's composite is
clipped to the frame, and the frame's border ink is drawn by one path layer
above all the panels.

## Geometry (`onepaint_panel::geometry`)
Pure functions over `Pt { x, y }` in document px. They assume convex
polygons, which straight cuts of rectangles stay (G:12-16).

- `rect_outline(a, b)`: four corners clockwise from the top-left, whatever
  the corner order (G:25-36).
- `polygon_area`: signed shoelace area. It is positive for a clockwise ring
  on screen (G:44-51).
- `point_in_polygon`: even-odd ray cast (G:58-68).
- `inset_polygon(pts, d)`: offset each edge inward by `d` and re-intersect
  consecutive edges, so corners stay sharp. It returns the input unchanged
  for `d <= 0`. It returns `[]` when the inset consumes the polygon, which
  it detects when any offset edge runs backwards against its original edge
  (G:88-132).
- `split_polygon_by_line(pts, a, b) -> (left, right)`: cut by the infinite
  line. Both halves get the crossing points exactly. A half with fewer than
  three points is `[]`. A degenerate line returns `(pts, [])` (G:142-170).
- `panel_border_outline(outline, gutter)`: the outline inset by `gutter/2`,
  or `None` when nothing is left (G:211-215).
- `nearest_vertex(pts, p, max)`: the index of the nearest vertex within
  `max`, inclusive (G:226-237).
- `distance_to_segment` and `distance_to_polygon_edge`: unsigned distances,
  clamped to the segment ends, with the closing edge included (G:240-266).
- `translate_outline` (G:269-271).
- `grid_outlines(w, h, rows, cols, margin)`: the page split into
  `rows × cols` cells that tile the page inside the margin, edge to edge.
  Rows and columns are clamped to 1..=12. It returns nothing when the margin
  leaves no room (PE:6621-6638).
- `LAYOUT_PRESETS`: 1×2, 2×1, 2×2, 3×1, 3×2 and 4×2, with happypaint's
  tooltips (TB:324-331).

## Coverage (`onepaint_panel::coverage`)
- `rasterize_coverage(contours, x0, y0, w, h)`: a port of
  `rasterizeContoursCoverage` (S:276-345) over any window of the page. It
  uses four sub-scanlines per pixel, exact horizontal span overlap and
  rounding to 1/255, so a tile computes the same coverage as the whole page.
- `panel_mask_coverage(outline, gutter, w, h)`: the whole-page mask, or
  `None` for degenerate input or a frame thinner than its own gutter (G:185-198).
- `tile_coverage(spec, tx, ty)`: `Full`, `Empty` or `Partial(coverage)` for
  one 256² tile. The fast paths are a bounding-box miss (`Empty`) and a tile
  whose four corners are inside the convex inner ring (`Full`). A panel with
  no interior does not clip (R:396-412).

## Metadata (`onepaint_panel::PanelSpec`)
- `outline: Vec<Pt>`, `gutter`, `border`, `borderColor` (M:29-42). Unknown
  keys inside `panel` are kept.
- It is stored verbatim in `Group::extra["panel"]`, so `.happypaint` files
  round-trip it with no format change. Only the recipe is stored, never the
  mask (M:17-21).
- `PANEL_META_FIELDS = ["panel"]` (M:50). `panel_of(&Group)` and
  `set_panel(&mut Group, Option<PanelSpec>)`. Absent stays absent.

## Compositing (`onepaint_compose`)
In the group branch, the group's composite is multiplied by its panel
coverage **before** the clipped riders go on top of it (C:1540-1548). A tile
the panel misses skips the group entirely, and a fully covered tile skips
the multiply. Every group is clipped, at any depth, as in happypaint.

## Session (`engine::panel_ops`)
Each operation is one undo step, and each one rebuilds the border layer in
the same step.

- `CreatePanelGrid { rows, cols, gutter, margin, border }`: a group "Panel N"
  per cell, each with an empty "Art" paint layer. They are pushed at the top
  of the root (PE:6621-6685).
- `CreatePanel { a, b, gutter, border }`: one frame from a drag rectangle. An
  area under 4 px² is rejected (PE:6746-6770).
- `SplitPanel { id, a, b }`: the group keeps the `left` half and its art. A
  new empty panel takes the `right` half, directly above it. A cut that
  misses is rejected (PE:6780-6820).
- `SetPanelOutline { id, outline }`: a live edit that coalesces until
  `SealHistory`, which replaces happypaint's set and commit pair. Fewer than
  three points, or an area under 4, is ignored (PE:6835-6880).
- `SetPanelStyle { id, gutter, border }`: a live edit that coalesces. With
  `id = None` it applies to every panel on the page. (happypaint has no UI for
  this yet.)
- Queries: `panel_frames()` lists `(id, outline)` in stack order for the top
  level panels, and `panel_at(p)` returns the topmost frame containing `p`
  (PE:6885-6895).
- **Borders** (PE:6705-6740): one top-level path layer, "Panel borders",
  holds a closed subpath per panel with `border > 0`, stroked with the
  `manga-millipen` brush along `panel_border_outline`. Panels with different
  widths or colours get separate path objects. When no panel has a border,
  the layer is deleted. An existing layer keeps its place; a new one goes at
  the top.

## UI (`apps/onepaint/src/panel_ui.rs`)
- There is a `Tool::Panel` button on the top bar. Clicking it again opens
  the panel options, which hold the layout presets, Gutter and Border sliders
  (live on every panel), and the margin.
- One press means one of four gestures, chosen by what was grabbed (PC:3533-3553):
  1. a corner within 12 screen px reshapes the frame;
  2. the frame line moves it (the art stays put);
  3. inside a frame, the drag is a cut line;
  4. on bare page, the drag draws a new frame.
- Create and split commit on release, and only when the drag is at least
  4 doc px. Reshape and move apply live and seal on release. A tap on a handle
  restores the original outline (PC:3577-3596).
- The overlay draws every frame's corner handles while the tool is active.
  It previews a create as a dashed rectangle and a split as the extended
  dashed cut line (PC:4222-4265).

## Acceptance
- All the cases in `panel-geometry.test.ts` and `panel-meta.test.ts` are
  ported as Rust tests. The source-pin cases in `panel-carriers.test.ts`
  become behaviour tests: a round trip, the border layer outside the groups,
  and a zero-width border drawing nothing.
- Tile coverage equals the whole-page coverage on every tile.
- Compositing: paint across two split panels. Pixels in the gutter and
  outside the frames are transparent, pixels inside keep their colour, and
  the border ink is on the inset ring.
- `.happypaint` round trip: the panel spec, a group that is both a panel and
  an animation track, and a document with no panels writes no `panel` key.
- Undo: grid, create, split, a coalesced outline drag and a style edit each
  undo in one step.
- kittest: a preset makes a page, a drag inside a panel splits it, a drag on
  bare page creates one, a corner drag reshapes, and the slider changes the
  gutter.
