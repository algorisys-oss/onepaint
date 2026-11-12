# 08: Selection, fill and the eyedropper (slice 2)

Sources are relative to `../happypaint/frontend/src/`. PE means
`engine/paint-engine.ts`, PC means `components/canvas/paint-canvas.tsx`,
and S means `engine/selection/selection.ts`.

## Model (`selection` crate)
- `Mask`: one `u8` of coverage per document pixel, `w × h`. 255 means
  selected. Soft values are allowed.
- **Engine state:**
  - `base: Option<Mask>`, the committed selection. `None` means no
    selection.
  - A live preview while a selection tool is dragging.
  - `feather` radius, default 0. Feather is live and non-destructive: it
    applies only to the mask used for clipping and display, never to `base`.
    Changing it is not undoable.
- **Combine** (S:46-62):
  - replace → `src`
  - add → `max`; with no base, `src`
  - intersect → `min`
  - subtract → `src==0 ? base : src==255 ? 0 : round(base·(255−src)/255)`
  - subtract or intersect with no base gives an empty mask.
- **Mode from modifiers** (S:27-32): Shift+Alt → intersect, Alt → subtract,
  Shift → add, otherwise the sticky mode (default replace). The mode is
  locked when the pointer goes down.
- **Commit:** a non-empty result becomes the base. An empty result clears
  the selection.
- **Select all:** fill with 255.
- **Deselect:** `None`.
- **Invert:** `255−v`. Inverting a full selection clears it. With no base,
  invert does nothing.

## Shapes
- **`rasterize_polygon`** (S:97-132): hard 0/255, even-odd fill.
  - Sample each row at `cy = y+0.5`. An edge crosses the row when
    `(a.y≤cy) != (b.y≤cy)`; the crossing is at
    `x = a.x + (cy−a.y)/(b.y−a.y)·(b.x−a.x)`.
  - Sort the crossings and pair them. Each span fills
    `round(x0) .. round(x1)−1`.
  - Clamp to the canvas. Rows run from `floor(minY)` to `ceil(maxY)`.
- **Rect:** the four corners. **Ellipse:** the bbox from start to end,
  approximated with 64 segments.
- **Shift** makes a square or circle: the side is the larger of `|dx|` and
  `|dy|`, anchored at the drag start.
- **Lasso:** raw points, closed implicitly, committed on release.
- **Polygon:**
  - Each click adds a vertex. Shift snaps the segment to 45°.
  - A click within `12/scale` doc px of the first vertex closes the polygon
    (when there are more than 2 points). Enter also commits.
  - Escape cancels. Committing with 2 points or fewer cancels.

## Magic wand and fill (PE:1302-1342, 1515-1691)
- **Matching:** per-channel Chebyshev distance on **premultiplied** RGBA.
  All four channels must satisfy `|v−seed| ≤ tol·FULL`. In onepaint, FULL is
  1.0, compared on the linear f16 pixels.
- **Flood:** 4-connected, using an explicit stack. With `contiguous=false`,
  every matching pixel is selected.
- **Sample source:** the wand samples the active layer, or the composite
  when `sample_all` is set. It returns early if the layer is hidden.
  - Defaults: `tol=0.15`, `contiguous=true`, `sample_all=false`.
  - The wand ignores the existing selection when matching. Its result is
    hard (255) and is then combined by the mode.
- **Fill tool:**
  - Same matching as the wand; always contiguous and always on the active
    layer. Defaults: `tol=0.15`, `grow=1` (0..8).
  - Pixels are confined to `sel>0`.
  - `grow` repeats a 4-neighbour dilation inside the bbox±1 and inside
    `sel>0`.
  - **Write:** the pixel becomes the opaque colour (an overwrite).
    - With alpha lock, `rgb = c·dstA` and alpha is kept.
    - With a soft selection, `after = before + (after−before)·sel/255`.
  - Blocked when the layer is hidden or locked. One undo entry.
- **Fill selection** (Alt+Backspace): premultiplied source-over of the
  colour at coverage `sel/255`. Alpha lock keeps alpha.

## Modifiers (S:377-472)
- **Feather** (happypaint v0.209.0, S:386-505): the radius is a Gaussian's
  **sigma** in doc px (Photoshop's scale), 0–1000, rounded. Three box passes
  per axis whose variances (r(r+1)/3 each) add up to σ² (`gauss_box_radii`),
  clamped to the edge, over the selection's bounds grown by the three radii
  only. Output is 0 at ≤0, 255 at ≥1, else `round(v·255)`.
- **Grow / shrink:** a separable square max/min filter whose window is
  clipped at the canvas edge. `r = min(200, |round(δ)|)`; δ=0 does nothing.
- **Smooth:** `r = clamp(round(px), 1, 100)`, feather by σ = √(r(r+1)/3)
  (the strength of the single box blur it used before v0.209.0), then
  threshold `≥128 → 255`.
- **Anti-alias** (Ellipse, Lasso, Polygon; on by default): the committed
  shape is rasterized with 4× vertical supersampling into 0..255 coverage.
  The rectangle never is. The ellipse uses enough segments for ≤ ~0.1 px
  flats (64–2048).

## Clipping
- The dab coverage `a *= mask` uses the **feathered** mask. The eraser is
  clipped the same way.
- **Clear** (Delete/Backspace): `layer × (1 − mask)`, or the whole layer
  when there is no selection.
- Every in-selection operation uses the **feathered** mask (v0.209.0):
  bucket fill and ColorDrop, wash, Fill mode, path fill, fill selection,
  clear, copy/cut and the transform's lift. Edits to the selection itself
  (combine, grow, invert, smooth, save) use the unfeathered base.

## Marching ants (compositor.ts:290-308)
- A pixel is on the outline when `m ≥ 0.5` and the minimum of its four
  neighbours at ±`band` texels is `≤ 0.5`.
  `band = max(1, 2/(scale·dpr))` doc px.
- **Stripes:** `d = mod(x + y + t·24, 12)` in screen px. `d < 6` is black,
  otherwise white. Opaque.

## History
These are undoable as full before/after mask snapshots:

- shape, select all, wand, invert, deselect, grow/shrink, smooth

Feather is not undoable.

## Eyedropper
- Samples the merged composite at `floor(point)`, 1 px.
- Alpha 0 leaves the colour unchanged.
- The result is unpremultiplied and converted to sRGB.
- Triggered by the tool or by Alt-press. Dragging keeps sampling.

## Shortcuts
| Key | Action |
|---|---|
| M | rect select |
| L | lasso |
| W | wand |
| G | fill |
| I | eyedropper |
| Ctrl+A | select all |
| Ctrl+D | deselect |
| Ctrl+Shift+I | invert |
| Delete / Backspace | clear inside the selection |
| Alt+Backspace | fill the selection with the colour |
| Enter / Escape | finish / cancel a polygon |

## Golden tests (from happypaint)
- **Combine:** `a=[200,100,255,0,128]`, `b=[100,200,128,128,255]`.
  - add → `[200,200,255,128,255]`
  - intersect → `[100,100,128,0,128]`
  - subtract → `[122,22,127,0,0]`
  - With a null base: add → `b`; subtract and intersect → zeros.
- `invert` is self-inverse.
- **Dilate / erode** equal the naive square-window max/min. Eroding a full
  mask by r=4 leaves it full.
  - `dilate([0,0,0,64,128,255,255,255,255], 9×1, r=1)` gives
    `[0,0,64,128,255,255,255,255,255]`.
- **Feather:**
  - A hard edge feathered by R (10, 50, 100) follows the normal CDF: ~50% on
    the edge, ~16% at R outside it, < 5% at 2R, 0 at 4R, 255 at 4R inside.
  - Cropping to the selection does not change the result.
  - 9×1 all 102, σ=2: all 102.
- Smooth outputs only 0 or 255.
- **Polygon on an 8×8 canvas:** rect (2,2)-(6,6).
  - Inside: (3,3) and (5,5).
  - Outside: (0,0), (7,7), (1,3) and (6,3).
  - Nested contours (1,1,7,7) + (3,3,5,5): (4,4)=0 and (2,2)=255 (even-odd).
- **Wand:** a contiguous region stops at a colour edge; non-contiguous mode
  picks up a disjoint island; tolerance bounds the match.
