# 11: Layer masks (slice 5)

Sources are relative to `../happypaint/frontend/src/`. PE means
`engine/paint-engine.ts`.

- **Storage:** the coverage is in R. A new mask is white (fully visible,
  PE:4072). Mask files write every tile, and a missing tile means coverage 0.
- **Add mask:**
  - With no selection, the mask is white.
  - With a selection, the mask is the selection's coverage (Photoshop's
    "reveal selection").
  - It is one undo step.
- **Painting** while the mask is the edit target:
  - The brush colour becomes its luminance, `0.299r + 0.587g + 0.114b`
    (PE:798-801), on sRGB values. That luminance is the coverage, with no
    transfer curve.
  - The eraser paints white and never alpha-erases.
  - The stroke goes onto the mask with alpha locked, so the mask alpha
    stays 1.
  - Live preview and undo work the same way as for layer pixels.
- **Enable/disable:** `maskEnabled`. A disabled mask is ignored when
  compositing.
- **Apply:** bake `pixels × mask.r`, then remove the mask.
- **Delete:** remove the mask without applying it.
- **Edit target:** clicking the mask thumbnail makes the mask the target,
  and clicking the layer thumbnail switches back. The target resets to the
  layer pixels when the active layer changes.

## Acceptance
- Adding a mask with no selection gives a white mask; the composite doesn't
  change.
- Painting black on the mask hides the layer there. Undo restores it.
- The eraser on the mask reveals again (white).
- Adding a mask from a selection hides everything outside the selection.
- Apply bakes the mask in, so the pixels themselves are hidden and the mask
  is gone. Delete restores full visibility.
- Masks round-trip through `.happypaint` (already covered by spec 05).
