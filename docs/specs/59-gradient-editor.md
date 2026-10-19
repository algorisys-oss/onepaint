# 59: Gradient editor, presets and fill handles

## Status
Built (2026-10-19): the stop model and saved state
(`filters::gradient_model`), the editor (`ui_kit::gradient_editor`), the
Gradient tool's panel and a Gradient fill layer's Properties
(`apps/onepaint/src/gradient_ui.rs`; `gradients.json` in the config
folder; `ONEPAINT_POPOVER=gradient`). On-canvas handles for gradient and
pattern fills are in spec 60.

## Source
happypaint v0.188.0 (gradient presets, stops, fill layers) and v0.191.0
(pattern fills): `utils/gradient.ts`, `utils/gradient-fill.ts`,
`state/gradient-store.ts`, `state/gradient-target.ts`,
`components/color-picker/gradient-section.tsx`,
`engine/adjustments/filter-handles.ts`, and their tests.

## Model
- A tool stop takes the foreground or background swatch, or a fixed
  colour; it has an opacity and an optional midpoint (0.05..0.95) for the
  segment to the next stop.
- Presets: Foreground to Transparent and Foreground to Background (fixed),
  then the user's, saved with the tool's shape, stops and the preset they
  came from. A corrupt file degrades field by field.
- A Gradient fill layer stores resolved stops: editing it pins Fg/Bg
  stops to the swatches of that moment. A new fill layer keeps its stops
  unreversed with the Reverse flag; a click (a ramp under 4 px) runs it
  across the canvas (`default_ends`).

## Editor
Click the bar to add a fixed stop of the colour already there, drag a
handle (3 px dead zone) to move it, double-click to delete (two stay);
drag a midpoint diamond to bend a segment, double-click to straighten.
The selected stop: Fg / Bg / Colour (a fill layer copies a swatch once),
opacity, Delete; Reverse; preset swatches with × on the user's own and +
to save. Any edit of the tool's stops leaves its preset.

## Acceptance criteria
1. happypaint's `gradient`, `gradient-fill` and `gradient-store` tests.
2. The tool takes a preset and a new stop from the panel; a Gradient
   fill layer is edited through the same editor, one undo step per edit,
   without touching the tool's settings.
