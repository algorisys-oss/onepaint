# 19: Guides: grid, perspective, H/V snap, persistence (slice 11)

The sources, relative to `../happypaint/frontend/src/`:

- `state/guides-store.ts` (GS)
- `components/canvas/guides-overlay.ts` (GO)
- `components/canvas/paint-canvas.tsx:4920-5120` (PC, snapping)
- `storage/document-db.ts:150-185` (`StoredGuides`)

## Model (`engine::Guides`, serde = `StoredGuides`, camelCase)
| Part | Fields and defaults |
|---|---|
| grid | `{enabled false, size 64 (4..512), subdivisions 1 (1..8), snap false, color #808080}` |
| symmetry | `{mode off, cx .5, cy .5, radialCount 6, angle 0}` (spec 16) |
| perspective | `{enabled false, points = preset 2, snap hard, curved false, density 6 (2..32), color #e87828}` |
| other | `snapHV false, visible true, lines [], lineColor #008cff, rulers false, units px, snapGuides true` |

**Perspective presets:** vanishing points (VPs) are document fractions,
and off-document points are normal.

| VPs | Points |
|---|---|
| 1 | `(.5, .5)` |
| 2 | `(−.8, .5), (1.8, .5)` |
| 3 | the 2-point layout `+ (.5, 2.4)` |
| 4 | the 2-point layout `+ (.5, −1.4), (.5, 2.4)` |
| 5 | the 4-point layout `+ (.5, .5)` |

**Persistence:**
- The guides live in the document's `guides` key, which round-trips
  through `.happypaint`.
- They are written only when configured: any guide is on, there are lines,
  the units are not px, visible is false, or the line colour isn't the
  default.
- Opening a document resets the guides to the defaults, then applies the
  stored ones.
- Edit modes are transient.

## Stroke snapping (`stroke::snap`, PC)
A snapper is armed at the stroke origin `o`. It is enabled when any of
these is true: soft perspective, H/V, grid snap (grid enabled and snap on),
or guide snap (guide lines exist).

- **Origin:** with grid snap, `o` snaps to the nearest grid node, at step
  `max(1, size/subdivisions)`.
- **Candidate directions:**
  - With perspective included (soft, or hard): one unit vector toward each
    VP (skipped when nearer than 1e-3). If there is no H/V snap and no VP
    has `|dy| > .94`, a vertical `(0, 1)` is added.
  - With H/V or grid snap: `(1, 0)` and `(0, 1)`.
  - Each guide line's direction.
- **Axis choice:**
  - Wait until the drag has gone `≥ 6` px from `o`.
  - Pick the candidate with the largest `|d·c|`.
  - Keep re-choosing until the drag has gone `≥ 24` px from `o`, then lock.
- **Apply:** project the point onto the line `o + t·c`.
- **Soft perspective** pulls 70% of the way: `p + .7·(proj − p)`.
- **Hard perspective** (brush and eraser): no freehand stroke. A straight
  line from the origin to the cursor's projection on the best axis,
  re-chosen on every move, is re-stroked live and committed on release.
- **Edit modes** (perspective points, symmetry axis): canvas drags move
  the handles instead of painting.
  - VP hit radius: 16 screen px.
  - Dragging moves the VP to the cursor's document fraction.

## Overlay (GO), drawn when `visible`
- **Grid:**
  - Minor lines at `size/subdivisions`, alpha .22, skipping the majors.
  - Major lines every `size`, alpha .45, width 1.
  - Both are clipped to the document.
- **Perspective (straight):**
  - For each VP, a fan to `max(6, density·3)` samples on each document
    edge, alpha .3.
  - A horizon through VPs 0 and 1, alpha .6, width 1.5.
  - VP handles: radius 9, alpha .5, or 1 with a white ring while editing.
- **H/V snap:** dashed centre cross, `rgba(80, 150, 255, .55)`, width 1.5, dashes 6/5.
- **Symmetry:** spec 16.

## Shortcuts
- Y: symmetry.
- Shift+P: edit perspective points.
- The guides button opens the panel.

## Acceptance
- `Guides` deserialises happypaint's `StoredGuides` JSON and re-serialises
  the same keys.
- A document saved with guides reopens with them, and an unconfigured
  document writes no `guides` key.
- Grid node snapping rounds to the finest step.
- **H/V:**
  - A stroke from (0, 0) toward (30, 4) snaps to y = 0.
  - Before 6 px nothing is projected.
  - After 24 px the axis is locked, even when the drag turns.
- **Perspective:**
  - With VPs at (−100, 50) and (300, 50) and the origin at (100, 50), a
    rightward drag snaps onto y = 50.
  - Soft pulls 70%.
  - A vertical drag gets the added upright axis.

## Guide lines, rulers and measure (slice 12)
The sources are `state/guide-lines.ts` and `docs/rulers-guides-spec.md`.

**Geometry (`stroke::guide_lines`), ported with happypaint's test cases:**
- A guide is a point plus an angle, canonicalised to `[0, π)`.
- Supporting operations: signed distance, projection, `nearest_guide`
  (skips locked guides, 7 screen px), `intersects_doc` (the delete test) and
  `snap_point_to_guides` (locked guides still snap).
- Units: px, cm (`dpi/2.54`) and in (`dpi`).
- `measure_between` gives the length and the angle (counter-clockwise, 0°
  pointing right, never −0).
- `nice_step` (1/2/5×10ⁿ), `tick_steps` (an 80 px major target with 5
  minors) and `ruler_ticks_span` (capped).

**Snapping:** a stroke origin is pulled onto the nearest guide within
`7/scale` doc px (after grid snap), and each guide adds its direction as a
candidate.

**History:**
- `EditGuideLines` changes the lines live; `CommitGuideLines` records one
  `GuideLines {before, after}` step if anything changed.
- A drag of many positions is therefore one undo step.

**Rulers (Shift+R):**
- 18 px bars pinned to the view's top and left edges while the canvas is
  unrotated, with ticks over the visible document span.
- Pressing a bar drags out a horizontal guide (top bar) or a vertical one
  (left bar), ahead of every tool.
- Dropping it back on a bar, or anywhere off the document, deletes it.

**Edit guides (Shift+G):**
- Pressing near a guide moves it; Alt-drag swings it about its anchor.
- A drag on empty canvas draws a new guide along the drag.
- Shift snaps angles to 15°.

**Measure:**
- A drag draws a line, with its length (in the chosen units) and angle on
  the canvas and in the status bar.
- It stays until the next measurement or Escape.
- Escape leaves every edit mode.

## Fish-eye (slice 20)
The source is `guides-overlay.ts:138-252` and `paint-canvas.tsx` (fish-eye snapping).

- **Poles:** the 5-point preset gives L, R, T, B and O.
- **Overlay:** `density` arcs per side through L/R and through T/B, bowed
  evenly across `half the diagonal`; `max(4, 2*density)` radial spokes out to
  the field-of-view radius (the mean pole distance); and an emphasised
  equator, meridian and field-of-view circle.
- **Snapping:** from the stroke's anchor, the drag direction picks the arc
  through L/R, the arc through T/B, or the radial line toward O, by the best
  tangent match.
  - Soft mode pulls 70% onto the curve.
  - Hard mode re-strokes the arc from the origin to the cursor's projection.
