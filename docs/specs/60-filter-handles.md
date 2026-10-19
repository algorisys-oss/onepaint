# 60: On-canvas filter handles

## Status
Built (2026-10-19): `filters::handles` (happypaint's geometry and tests)
and the canvas overlay and drags (`apps/onepaint/src/canvas.rs`).

## Source
happypaint `engine/adjustments/filter-handles.ts` and its test,
`components/canvas/filter-handles-overlay.ts`, `paint-canvas.tsx`
`filterTarget` and the press handling around line 5420.

## Behaviour
- Whose handles: the open filter preview's, or else the selected,
  visible adjustment layer's when its kind has a place on the picture,
  unless its mask is the paint target.
- Handles: a centre (radial, spin and iris blur, twirl, pinch, polar
  coordinates, tilt-shift, light rays, lens flare), twirl and pinch's
  reach, tilt-shift's focus and falloff lines and a rotate handle 120
  screen px out, the iris/spin ellipse's two radii and sharp core, Field
  Blur's pins, a gradient fill's start and end (doc fractions −1..2) and a
  pattern fill's origin.
- A press within 12 screen px grabs the nearest handle. An open preview
  owns every press: off the handles it moves the centre there (or adds a
  pin). A layer only claims presses on its handles, plus Field Blur's
  empty presses (a new pin with the last pin's blur, at most 8) and, with
  the Gradient tool, a gradient fill's (restart the ramp, drag its end).
  Alt-click a pin removes it, never the last.
- A drag edits the layer live as one undo step, or the preview.
- Overlay: guides dark under white (dashed where happypaint dashes them),
  handles white discs ringed in accent, Field Blur's pins labelled with
  their blur.

## Acceptance criteria
1. happypaint's `filter-handles` tests.
2. Dragging a gradient fill's end edits it as one undo step and paints
   nothing; an empty press on a Field Blur layer adds a pin.
