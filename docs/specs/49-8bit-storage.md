# 49: 8-bit documents store 8-bit tiles

## Status
Built (A of the 8-bit work). Spec 50, gamma-space compositing for 8-bit
documents (B), follows on the same storage.

## Why
Every document worked in RGBA16F, so an 8-bit document took twice the
memory it does in happypaint. It also kept precision that happypaint
throws away at every edit.

## happypaint (the spec)
Paths are under `happypaint/frontend/src`.
- 8-bit layers are `RGBA8` textures holding **premultiplied sRGB bytes**:
  the shaders write `vec4(rgb, 1) * a` with `rgb` in sRGB (`color.ts:9-13`,
  `stamp-renderer.ts:253`).
- `gl-utils.ts:106-130` picks one format per context.
- Masks are RGBA in the document format, with **raw coverage** (r = g = b =
  c, a = 1), and no transfer curve (`paint-engine.ts:1337-1353`).
- Undo snapshots hold the same bytes (`readLayerRegion`,
  `paint-engine.ts:9511-9525`). Depth is chosen in the New Document
  dialog (8-bit, 16-bit float, 32-bit float) and never converted.

## Model
### Tile storage (`raster`)
`Tile` has private storage in one of three forms:

| Store | Bytes / px | Holds | Decode to premultiplied linear f32 |
|---|---|---|---|
| `F16` | 8 | premultiplied linear (as before) | as is |
| `Srgb8` | 4 | premultiplied sRGB bytes `c8 = round(l2s(c/a)·a·255)`, `a8 = round(a·255)` | `a = a8/255`, `c = s2l(c8/a8)·a` (a 256 × 256 table) |
| `Unorm8` | 4 | raw bytes `round(v·255)` (masks) | `v = b/255` |

- **Reads** (`get`, `to_f32`, `f16_data`, `is_empty`, `is_opaque`) work on
  every store.
- **Writes** (`set`) encode into the tile's own store.
- `from_f32`, `filled` and `default` make `F16` tiles.
- `to_srgb8()` and `to_unorm8()` make 8-bit copies.
- Decode then encode gives back the same bytes, so storing a tile again
  never drifts.

### When tiles become 8-bit (`engine`)
- In an 8-bit document, every history entry is quantized as it is
  recorded:
  - its after-state tiles become `Srgb8` (layer pixels) or `Unorm8`
    (masks), both in the entry and in the live document;
  - height fields (impasto) stay `F16`, because they are not colour;
  - a document- or tree-wide entry quantizes all its layers.
- In between, a stroke in progress works in `F16`. So does its buffer, as
  happypaint's GPU registers are float. It is quantized once, when it
  lands.
- Opening an 8-bit document quantizes it. Pixels come from 8-bit straight
  PNGs, which premultiplied 8-bit storage rounds as happypaint's
  premultiplied upload does.
- `16-bit-float` and `32-bit-float` documents are unchanged (32F still runs
  as 16F, divergence 2).

### Elsewhere
- The history byte budget counts actual tile bytes.
- GPU uploads decode to f16 (`f16_data`). Spec 49 halves CPU memory; 8-bit
  atlases on the GPU (halving VRAM) are a later step.
- Compositing is unchanged: linear light (spec 50 changes it).
- Recordings (`onepaint-rec`) still hold tiles as f16; replaying an 8-bit
  document requantizes them when they land.
- The New Document dialog gets a depth choice: 8-bit, 16-bit float (the
  default, as in happypaint) and 32-bit float.

## Acceptance criteria
1. **raster:**
   - `Srgb8` and `Unorm8` round-trip any byte pattern exactly
     (decode → encode);
   - `F16 → Srgb8` errs by less than half a byte step in sRGB;
   - `set` on an 8-bit tile stores the encoded pixel;
   - `f16_data` and `to_f32` agree.
2. **engine:**
   - in an 8-bit document, layer pixels are `Srgb8` and masks `Unorm8`
     after a stroke, a fill, a filter, a transform, liquify, smudge and a
     merge;
   - undo and redo restore exactly;
   - a 16-bit-float document keeps `F16` everywhere;
   - an 8-bit document saved and reopened keeps its pixels and depth.
3. A stroke in an 8-bit document composites within 2.5/255 (sRGB; mean
   0.3/255) of the same stroke in a 16-bit-float document. Soft edges
   round both alpha and premultiplied colour, and over white each costs up
   to half a step; happypaint's 8-bit layers do the same.
4. The New Document dialog creates 8-bit documents (kittest).
5. Memory: a 2048² 8-bit page with 10 painted layers holds half the tile
   bytes of the same page at 16-bit float.
