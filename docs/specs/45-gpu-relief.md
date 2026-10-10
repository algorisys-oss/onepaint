# 45: Impasto relief on the GPU

## Status
Built. Layers with impasto height composite on the GPU. Before this,
every tile of such a layer went to the CPU.

Follows spec 35 C (relief) and spec 13 (GPU compositor). The lighting is
happypaint's `compositor.ts:149-165`, as `compose::relief::shade` does it.

## Model
- Relief lights a layer's pixels after its live stroke and before its mask,
  opacity and blend. Each pixel reads the height one pixel left, right, up
  and down; taps clamp to the document. `relief::light_px` is that maths
  for one pixel, and `shade` and the program's CPU interpreter both call
  it.
- `Content.relief: Option<Arc<Relief>>` holds the height tiles for the tile
  and the four that share an edge with it. No tap reaches a corner tile. It
  is `None` when none of the five has height, so the result is unchanged.
- The CPU interpreter equals `composite_tile` exactly.
- Segment keys include the tile position and the document size, because
  taps clamp at the document's edge.
- GPU:
  - `F_RELIEF`; the five slots are packed two to a word in the op's `d`
    vector, with 0xffff meaning no tile;
  - `relief_h` picks the slot by which edge the tap crosses;
  - cache-fill jobs now carry their tile's real origin (the shader ignores
    it when writing a cache slot), so relief in a cached segment taps the
    right place;
  - above level 0 the taps are one level pixel apart, which gives a
    preview (spec 39).

## Acceptance criteria
1. Random documents with height on about half the paint layers equal
   `composite_tile` exactly on the CPU, including tiles at the document's
   edge.
2. On the GPU they match the oracle within 2e-3, unsplit and split, cold
   and warm (relief inside cached segments). Dropping the left-neighbour
   tap fails at the x = 256 seam.
3. By eye: the `ONEPAINT_IMPASTO=1` demo on the GPU path and on the CPU
   compositor differ by at most 1/255.
