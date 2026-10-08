# 28: Speech balloons and rich-text editing

Extends [14](14-text.md). Sources are relative to `../happypaint/frontend/src/`:
`engine/text/text-renderer.ts` (TR), `engine/text/balloon.test.ts` (BT),
`components/text-panel/text-panel.tsx` (TP), `state/text-store.ts` (TS),
`components/canvas/paint-canvas.tsx` (PC), `utils/rich-text-html.ts` (RH),
plus `docs/manga-spec.md` §P3 and `docs/rich-text-spec.md`.

## Balloon model (TR:33-60)
`TextSpec.balloon?: BalloonSpec` is a typed field (it used to sit in `extra`).
- `shape`: `oval` | `rounded` | `burst` | `cloud`. The UI calls them Speech,
  Rounded, Shout and Thought (TP:300-304). An unknown shape draws as an oval,
  like TR's `else` branch.
- `fill` (hex); `stroke?` (hex; absent means no outline); `strokeWidth?`
  (default 2 when there is a stroke, TR:580).
- `pad?`: padding as a multiple of the font size (default 0.55).
- `tail?: {x, y, width?}`: the tip in the object's **local block
  coordinates**, so it rotates with the object. The width defaults to 24.
- It serializes as camelCase JSON inside `texts[]`, so files round-trip with no
  format change.

## Geometry (pure, `onepaint_text::balloon`)
- `spec_font_size(spec)` (TR:442): `fontSizePx`, or the largest run size when
  there are runs. A run without a size counts as the object's size.
- `balloon_pad(spec, size)` (TR:452): `size·(pad ?? 0.55) + (stroke ? (strokeWidth ?? 2)/2 : 0)`;
  0 without a balloon.
- `balloon_extra_local_points(spec)` (TR:467): with no tail, none. Otherwise the
  tip and the tip ± `half` on x and y, where `half = max(0, (width ?? 24)/2)`.
- `spec_aabb(spec, blockW, blockH, pad, extra)` (TR:411): the padded block
  corners plus the extra points, rotated about (x, y), as an AABB.
- `balloon_paths(spec, blockW, blockH)` (TR:510-584) returns the closed
  sub-paths in local space. Let `p = spec_font_size·(pad ?? 0.55)`,
  `x0 = y0 = −p`, `w = blockW + 2p`, `h = blockH + 2p`, and let `(cx, cy)` be the
  centre.
  - **oval:** an ellipse with radii `w/2, h/2` about the centre.
  - **rounded:** a rectangle with corner radius `r = min(w, h)·0.28` (canvas
    `arcTo`, so the corners are quarter circles).
  - **burst:** 32 vertices at `t = i/32·2π`, with radius factor `1.22` on even
    `i` and `0.86` on odd `i`, around the ellipse.
  - **cloud:** 12 circles of radius `rr = min(w, h)·0.19` centred on the ellipse
    at `t = i/12·2π`, plus the ellipse itself.
  - **tail:** the triangle `(c + n, tip, c − n)`, where `n = perp(tip − c)/|tip − c| · max(1, width ?? 24)/2`.
- Ellipses and arcs are flattened to polygons with chord error ≤ 0.1 px.

## Drawing (TR:590-605)
- **Order:** bgColor, then the balloon (fill, then stroke), then the glyph
  outline, the fill and the decorations. `bgColor` and `balloon` both draw if
  both are set. The UI makes them exclusive (TP:285-299).
- **Fill:** coverage is the **union** of all sub-paths (body, cloud lobes and
  tail), antialiased analytically from a signed distance field:
  `sdf = min over sub-paths`, `fill α = clamp(0.5 − sdf, 0, 1)`.
- **Stroke:** centred on the union outline, `α = clamp(sw/2 + 0.5 − |sdf|, 0, 1)`
  in the stroke colour, composited over the fill. The joins are round
  (`lineJoin = 'round'`).
- **Divergence:** Canvas2D fills the combined path with the nonzero rule, and
  TR's tail triangle winds the opposite way to the body, so happypaint leaves
  a transparent wedge from the balloon centre to the wall. Its `stroke()` also
  traces every sub-path, so the tail base and the cloud's inner ellipse and
  lobe arcs show as lines inside the balloon. Both contradict TR's own comment
  ("one combined path leaves no seam … stroking it outlines the union") and
  manga-spec P3. onepaint draws the intended union; see divergences #11.
- **Bounds** (TR:654, TR:681): the local canvas covers the balloon pad, the
  stroke and the tail points. `quad()` grows to `±balloon_pad` and the extra
  points when a balloon is set (BT "textObjectQuad grows"). Without a balloon
  it still hugs the block, even with `bgColor` (BT "bgColor keeps its
  historical box").

## Rich-text runs (`onepaint_text::runs`)
The run model is TR:15-31 and `docs/rich-text-spec.md`. Runs store only the
fields that **differ** from the object's base style (RH:8-11). If every
character matches the base, the result is `runs: None`. `spec.text` is always
the joined run text (TR:129).

The pure edits work on char ranges `[a, b)`:
- `split_at(runs, i)`: split the run that contains char `i`. Afterwards a run
  boundary lies at `i`.
- `merge(runs)`: join adjacent runs with the same style and drop empty runs
  (RH:93-105).
- `apply_style(spec, a..b, edit)`: split at `a` and `b`, edit the runs inside,
  then normalise (merge, strip fields equal to the base, collapse to `None`).
  The edits, as PC's toolbar offers them (PC:7360-7410, PC:358-460):
  - **Bold, Italic, Underline, Strikethrough:** toggles, following
    `execCommand` semantics. If every char in the range already has the
    attribute it is turned off, otherwise it is turned on.
  - **Colour(hex):** `foreColor`.
  - **Font(stack):** `fontName`.
  - **ScaleSize(k):** A− and A+ scale each fragment's own resolved size by
    `1/1.1` or `1.1`, rounded to 0.01 with a 2 px floor (PC:377-420).
- `replace_text(spec, new)`: retargets the runs after a plain edit in the field.
  It diffs the common prefix and suffix. Inserted text takes the style of the
  char before it, or the char after it at the start (contenteditable
  behaviour). Deleted text drops its runs.
- `style_at(spec, a..b)`: the resolved style that is common to the range, used
  for the toolbar state.
- A collapsed range (`a == b`) is a no-op, like PC's `stepSelectionSize`.

## Rendering runs
- Run underline and strikethrough draw per fragment, in the run's colour:
  underline at `baseline + 0.10·size`, strikethrough at
  `baseline − 0.28·size`, `max(1, 0.06·size)` thick (TR:319-323).
- When runs are present the object-level `underline` is ignored
  (rich-text-spec rule 2).

## UI (`apps/onepaint/src/text_ui.rs`)
- **Selection toolbar** (PC:7360): B, I, U, S, A− and A+, a "Text" colour
  button (the current colour) and a font menu, applied to the field's
  selection. The toggles show the selection's common state.
  - With no selection they act on the whole object through
    `apply_to_object`. It sets the object field (`bold`, `italic`, `color`,
    `fontFamily`, `fontSizePx`) and drops the matching run overrides. Size
    keeps run ratios. Underline throughout collapses to the object
    `underline`. Strikethrough becomes a run over everything.
  - The field previews the runs (colour, weight, italic, decorations,
    relative size) and keeps them in step as the user types (`replace_text`).
- **Not ported yet:** PC's real colour picker for the selection (onepaint's
  colour buttons take the current colour, like the rest of this panel). The
  tail-tip drag handle is still open in happypaint too (manga-spec P3b), as is
  joining balloons.
- **Balloon row** (TP:285-344):
  - **Shape menu** (None/Speech/Rounded/Shout/Thought). Picking a shape clears
    `bgColor` and creates a default balloon if there is none: fill `#ffffff`,
    stroke `#000000`, width 3 (TS:138-139).
  - **Swatches:** the "Fill" and "Rim" buttons take the current colour; the
    outline width is a drag value. Picking "Background" removes the balloon.
  - **Tail:** an angle slider (0-360°, default 90°) places the tip at
    `fontSize·2.2` from the block origin, with width `max(8, 0.5·fontSize)`
    (TP:85-97). "No tail" clears it.

## Acceptance
- Geometry ports BT: the pad (none, proportional, half the stroke, stroke width
  without a colour), `spec_font_size`, the tail points, `spec_aabb` growth and
  rotation, and a tail inside the block.
- **Goldens:** happypaint's own `rasterizeTexts`, run in Chromium for every
  shape, with fill and with stroke (except the cloud stroke and the tail, which
  diverge). onepaint's local raster differs on fewer than 1% of the shape's
  pixels, with tolerance at the antialiased edge. The cloud stroke outlines
  only the union (its inner lobe arcs stay fill).
- **Tail:** the tip region is filled. The balloon centre is filled (the union,
  not happypaint's wedge).
- **Rendering:** with a balloon behind text, the balloon pixels are the fill
  colour, the stroke ring is the stroke colour and the glyphs draw on top. The
  balloon grows the ink bounds and the `quad`.
- **Run edits:** splits, merges, toggles, the colour of a sub-range, size
  scaling, `replace_text` inheritance, and collapse back to `None`.
- **Round trip:** a spec with a balloon and runs survives `.happypaint`
  save and open unchanged.
- **UI:** kittest. Typing, selecting a range and clicking B gives a bold run;
  typing keeps the runs in step; picking Speech gives the default balloon and
  None removes it. `tail_from_angle` and `tail_angle` are tested directly.
