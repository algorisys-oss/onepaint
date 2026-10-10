# 50: 8-bit documents composite in gamma space

## Status
Built (B of the 8-bit work; spec 49 is A). Closes divergence 2 for 8-bit
documents.

Not covered here:
- Merge down still composites with the layer's real blend mode. happypaint
  merges advanced modes as Normal; onepaint keeps divergence 8 here.
- The GPU still holds 8-bit tiles as f16 in its atlases (VRAM is not
  halved yet).

## happypaint (the spec)
Paths are under `happypaint/frontend/src`.
- **Storage:** 8-bit documents keep premultiplied sRGB bytes and never
  linearise: `color.ts:9-13` says "8-bit documents stay sRGB-encoded and
  never call these".
- **Colours in:** colours are used as sRGB. Every input point checks
  `isFloatColor(gl) ? rgbToLinear(c) : c`:
  - the stamp shader `u_linear` (`stamp-renderer.ts:253`);
  - fills (`paint-engine.ts:4398, 4421`);
  - wet paint (`3405, 3537`) and the smudge reservoir (`3010`);
  - gradient LUTs (`compositor.ts:2764`, `adjustment-renderer.ts:2722`).
- **Images in:** PNG and image uploads are premultiplied as bytes; there is
  no linearising (`compositor.ts:2916`).
- **Colours out:** the eyedropper only un-premultiplies
  (`paint-engine.ts:9075`). Exports and thumbnails divide by alpha with no
  transfer curve (`pixelsToStraight8`, `9663-9687`).
- **Blending** happens on the stored values (`u_linear = 0`,
  `compositor.ts:338-400`), so every mode, Normal included, is in gamma
  space.
  - Multiply, screen and add use fixed-function GL blending
    (`compositor.ts:2833-2837, 2960-2981`):
    - add: rgb `s + d`, alpha `s.a + d.a·(1 − s.a)`;
    - multiply: `s·d + d·(1 − s.a)` for all four channels. Alpha stays
      `d.a`, and the W3C term `s·(1 − d.a)` is dropped, so a multiply layer
      adds nothing over transparency;
    - screen: `s + d·(1 − s)` per channel, alpha included.
  - Merge down (`drawTextureOnto`) uses only fixed blending, so advanced
    modes merge as Normal there.
- **Present:** no transfer function (`u_oetf` is off). The paper's shade
  is not raised to 2.2 (`u_paperLinear`).
- **Smudge, wet media, adjustments and layer styles** all skip their
  linear wraps (`u_linear` / `toWork` / `fromWork`).

## Model
### The space (`color::Space`)
`Space::Linear` (float documents) or `Space::Gamma` (8-bit documents),
from `Document::space()`. It owns every boundary:

| | Linear | Gamma |
|---|---|---|
| `pixel(Rgba)`: a colour to a premultiplied pixel | linearise, premultiply | premultiply |
| `color(Px)`: a pixel to a straight sRGB colour | un-premultiply, encode | un-premultiply |
| `to_srgb8` / `from_srgb8` (files, exports) | with the transfer curve | without |
| `to_work` / `from_work` (colour filters) | the curve | identity |

### Storage
8-bit pixel tiles are `Unorm8`: raw bytes of premultiplied sRGB, which spec
49's former `Srgb8` store held too, now read as they are.

### Compositing
`blend_px` and the alpha-lock variant take the space.
- **Gamma** skips the sRGB round trip around the separable and
  non-separable modes.
- **Gamma** uses the fixed-function formulas above for multiply, screen
  and add.
- Normal is the same formula on gamma values.
- The CPU tree compositor, the program interpreter and `compose.wgsl` all
  take the document's space: in `Params` on the GPU, and in `TileProgram`
  on the CPU.
- The paper's exponent is 1 in gamma space.
- The present pass decodes premultiplied sRGB to linear for display and
  for the view modes.

### Painting and filters
- Brush, fill, gradient, text, wet and smudge colours enter through
  `space.pixel`. Stamping, smudging and wet media then work on whatever the
  values are, as happypaint's do.
- Colour adjustments take the space from `Frame`: gamma documents skip
  `to_work` and `from_work`, on the CPU and on the GPU.
- Gradient ramps are built without linearising.
- Pigment (Kubelka–Munk) skips its sRGB wrap.

### Exports
- `Session::flatten()` stays in the document's space; the magic wand
  compares those values.
- `flatten_linear()` decodes for exports, thumbnails, recordings and the
  reference snapshot.
- A copied region carries its space, and pasting it into a document of
  another depth converts it.

### Files and the outside
- `.happypaint` reading and writing use `space` for the 8-bit PNG tiles,
  and so do the image, PSD and clipboard import and export, thumbnails and
  the eyedropper.
- An 8-bit `.happypaint` then reads back the same bytes it saved.

## Acceptance criteria
1. **`Space`:** gamma round-trips 8-bit bytes exactly. Linear is unchanged.
2. **Blending in gamma space**, checked against happypaint's formulas on
   known values:
   - 50% grey Normal at opacity 0.5 over white gives 0.75 (sRGB), where
     linear gives 0.80;
   - multiply over a transparent backdrop leaves it transparent;
   - screen and add follow their formulas;
   - the other modes equal `blend_color` on the raw values.
3. **The compositors agree:** for 8-bit random documents, the tree
   compositor, the program interpreter (exactly) and the GPU (within 2e-3)
   agree, split or unsplit.
4. **A painted 8-bit document:**
   - a brush in `#808080` stores 128 at full alpha;
   - the eyedropper reads `#808080` back;
   - export writes 128;
   - saving and reopening keeps the bytes.
5. **Float documents are unchanged:** every existing test passes.
