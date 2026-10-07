# 14: Text layers (slice 7)

The source is `engine/text/text-renderer.ts` (TR), relative to
`../happypaint/frontend/src/`.

## Model
- `TextSpec` (TR:62-108) is kept verbatim as JSON in the layer's `texts`
  array, so files round-trip. The fields:
  - `text`, `runs?`, `fontFamily` (a CSS stack), `fontSizePx`, `color`,
    `bgColor?`, `balloon?`, `outlineWidth?`, `outlineColor?`, `underline?`
  - `align`, `lineHeight` (a multiplier), `bold`, `italic`, `letterSpacing`
    (doc px)
  - `x`, `y` (the **top-left of the block**), `boxWidth?`, `rotation`
    (radians, about (x, y))
- A layer is a text layer when `texts` is non-empty. Its pixels are always
  `render(texts)`.
- **Defaults:**
  - `system-ui, sans-serif`, 32 px, left-aligned
  - line height 1.2, not bold or italic, letter spacing 0, rotation 0

## Rendering (onepaint: cosmic-text + swash)
- Lines are `fontSizePx·lineHeight` tall, and glyphs are centred in the line
  box (half-leading), like CSS.
- With `boxWidth > 0`, text wraps greedily by word at that width; over-long
  words overflow. Otherwise only explicit `\n` breaks lines.
- `blockW = boxWidth ?? max(line width)`. Alignment is applied inside
  `blockW`.
- **Draw order:**
  1. bgColor, padded by `0.18·size`
  2. the outline: coverage dilated by `outlineWidth`, outer half only
  3. the fill
  4. underline at `size·0.92` from the line top, `max(1, 0.06·size)` thick
- Rich `runs` override the bold, italic, colour, size, family, underline and
  strikethrough of their range.
- The CSS family stack maps to the first installed family. Generics map as
  follows: `sans-serif`/`system-ui` → SansSerif, `serif`, `monospace`,
  `cursive`.
- Output is premultiplied linear: sRGB colours are linearised.
- **Balloons:** the spec is preserved but not drawn yet (divergence).

## Engine
- `AddText{spec}` adds to the active text layer, or creates a new text layer
  named "Text" above the active one.
- `UpdateText{id, index, spec}` and `DeleteText{id, index}`: deleting the last
  text leaves a plain empty layer.
- `RasterizeText{id}` drops `texts` and keeps the pixels.
- Each of these is one undo step, and the pixels are re-rendered.
- Pixel operations auto-rasterise a text layer first (TR, PE:4620-4655):
  painting, fill, filters, transform, clear, gradient, shapes.
- **Hit test:** `text_at(x, y)` returns the topmost visible text object whose
  rotated box (with 6 px slack) contains the point.

## UI
- **Text tool (T):**
  - A click on empty canvas opens an editor at that point.
  - A click on an existing text edits it; dragging it moves it.
- **Editor:** a floating panel with a multiline field and the font, size,
  B/I, alignment, line height, outline and background controls. The live
  preview renders through the engine. Done commits; Delete removes the text.

## Acceptance
- Rendering "Hello" at (20, 30) gives coverage inside a box that starts near
  (20, 30). An empty text gives nothing.
- With `boxWidth` set, a long sentence is taller and no wider than
  `boxWidth` (plus slack).
- Right alignment moves the ink right.
- Rotating 90° swaps the bbox axes.
- An outline widens the coverage; bold gives more coverage than regular.
- The engine adds, updates and deletes texts with undo. Painting on a text
  layer rasterises it first, as a separate undo step. Texts survive save
  and open.
