# 09: Move/transform, import, clipboard and layer drag-and-drop (slice 3)

Sources are relative to `../happypaint/frontend/src/`. PE means
`engine/paint-engine.ts` and PC means `components/canvas/paint-canvas.tsx`.

## Transform state (PE:1838-1855, 464-476)
- The parameters are `{dx, dy, sx, sy, rotation}`. The pivot is the **document
  centre** C.
- `p' = C + R(rot)·diag(sx,sy)·(p−C) + (dx,dy)`
- As an affine map:
  - `a = cos·sx`, `b = sin·sx`
  - `c = −sin·sy`, `d = cos·sy`
  - `e = cx+dx−(a·cx+c·cy)`, `f = cy+dy−(b·cx+d·cy)`
- A pure translate rounds dx and dy to whole pixels.
- What gets lifted:
  - With a selection: the layer × mask. The marquee is cleared while
    transforming and comes back on cancel.
  - Without a selection: the whole layer. The handle box is the content's
    bounds.
- Hidden or locked layers can't be transformed.

## Handles (`components/canvas/transform-overlay.ts`, `transform-drag.ts`)
- The handles are the 4 corners, 4 edge midpoints, and a rotate knob 28
  screen px out from the top-edge midpoint. The hit radius is 12 screen px.
- Hit order: rotate, then corners, then edges. Anything else is a body move;
  Shift snaps that move to 45° rays.
- **Scaling:**
  - The pivot is the opposite corner or edge midpoint. With Alt it is the box
    centre.
  - A corner scales uniformly: `r = |p−pivot| / |grab−pivot|`.
  - An edge scales one axis by the projection of `p−pivot` onto the
    pivot→handle unit axis, divided by the handle distance.
  - Shift makes edges proportional. Corners are already uniform.
  - A negative ratio mirrors, and `|s|` is kept at 0.02 or more.
  - dx and dy are then solved so the pivot stays where it is on screen.
- **Rotation:** `rot = rot0 + Δatan2` around the transformed box centre,
  which stays fixed. There is no snapping.
- **Commit:** Enter, the Apply button, or switching tool or layer.
  **Cancel:** Esc. With no change, nothing is recorded. Otherwise it is one
  undo step.
- Resampling is bilinear on premultiplied pixels.

## Import and clipboard
- **Open image:** opens as a new single-layer document. *(onepaint addition:
  happypaint opens images only through Place.)*
- **Place image / paste a system image:**
  - Scale is `min(1, dw/iw, dh/ih)`, centred, on a new layer named "Image"
    above the active one.
  - It does **not** enter transform mode.
- **Copy (Ctrl+C):** the active layer. With a selection, the selection's
  bbox × coverage; otherwise the whole layer. A fully transparent result
  copies nothing.
- **Cut (Ctrl+X):** copy, then clear the selection.
- **Paste (Ctrl+V):** prefers a system image. Otherwise it pastes the
  internal region **in place** on a new layer named "Pasted", fitted to the
  document: `w = min(src.w, dw)`, `x = clamp(src.x, 0, dw−w)`.
- **Ctrl+J:** "Layer via Copy" (selection region), or duplicate the layer
  when there's no selection.
- **Ctrl+Shift+J:** "Layer via Cut".

## Layer drag-and-drop (`layer-panel.tsx:285-407`)
- The rows are top-first.
- **On a group row**, when it isn't the dragged node or inside it:
  - `frac < 0.34` → above the group.
  - Otherwise → **into** it, at the top.
- **On another row:** `frac < 0.5` → above, otherwise below.
- A drop inside the dragged subtree is invalid.
- Clipped layers don't travel with their base. It is one undo step.

## Acceptance (goldens from `transform-drag.test.ts`)
DOC 100×100, box {20,30,40,20}, tolerance 1e-6:

| Drag | Result |
|---|---|
| BR corner → (100,70) | `sx=sy=2`, TL pinned at (20,30) |
| Right edge → (100,40) | `sx=2`, `sy=1`, (20,40) pinned |
| Right edge → (0,40) | `sx=−0.5` |
| Right edge + Shift → (100,40) | `2/2` |
| Alt + corner → (80,60) | `2/2`, centre (40,40) fixed |
| Alt + edge → (80,40) | `2/1` |
| Rotation π/6, BR at twice its distance | `scale=2`, rotation unchanged, TL pinned |
