# 43: Paper shading on the GPU

## Status
Built. A document with paper composites on the GPU. Before this, paper
sent every tile of the document to the CPU.

Follows specs 36 (document paper) and 13 (GPU compositor). The shading is
happypaint's `compositor.ts:214-233`, as `brush::paper::shade_tile`
already does it on the CPU.

## Model
- The shading is a factor on rgb (alpha untouched). It depends only on the
  position and the paper grain:
  - the height and the lit normal from four neighbouring heights;
  - `shade^2.2`, darken-only;
  - pixels whose rgb is all zero are left alone.
- `brush::paper::shade_at` is that factor. `shade_tile` and the program's
  CPU interpreter both call it.
- `Op::Paper { shade, tx, ty }` is the last op of every tile's program when
  the document has paper with a visible amount above 0 and a known grain.
  It follows every unit, so a split (spec 38) leaves it in the tail, after
  the suffix image.
- GPU (`compose.wgsl`):
  - the grain is an R32Float texture at binding 16, uploaded when the
    batch's paper grain changes (keyed on the grain's pixels);
  - `paper_h` is `TipMask::sample_repeat` by hand: wrap, then bilinear;
  - `paper_k` is `shade_at`;
  - the op carries the tile size in doc px and the amount;
  - above level 0 the level's pixel centre stands in for its 2^L × 2^L
    block (a preview, refined to level 0 when idle, as spec 39 does).

## Acceptance criteria
1. Programs for papered random documents (three grains, several scales and
   strengths, with adjustment layers) end in `Op::Paper` and equal
   `composite_tile` exactly on the CPU.
2. On the GPU they match the oracle within 2e-3, unsplit and split.
   Changing the shader's exponent from 2.2 to 2.0 fails the test.
