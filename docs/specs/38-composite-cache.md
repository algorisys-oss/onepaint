# 38: Cached composite below and above the active layer

## Status
Built: the split (as ranges into the program's ops), the segment cache and
its fill pass, `Session::plan` splitting at the stroked or active layer
with tiles planned in parallel, warm-up on idle frames after the active
layer changes, and over-budget frames that alternate direction.

Measured on Iris Xe with the opacity-drag bench (`ONEPAINT_BENCH_TRACE=1`,
unsplit and split frames alternated; browsers were sharing the GPU, so
treat these as upper bounds):

| Drag frame, 8192² × 100 layers | Unsplit (median) | Split (median / min) |
|---|---|---|
| Distinct tiles fit the atlas | 150 ms | 19.7 / 16.8 ms |
| Past the atlas budget | 336 ms | 28 / 23 ms |

Re-measured on a quiet machine (2026-10-10 evening: no browsers or other
GPU clients, load average 1.4 before and 2.0 during the run, AC power,
"balanced" profile):

| Drag frame, 8192² × 100 layers | Unsplit (median) | Split (median / min) | Split, criterion |
|---|---|---|---|
| Distinct tiles fit the atlas | 144 ms | 54 / 42 ms | 54 ms (GPU only 44 ms) |
| Past the atlas budget | 522 ms | 165 / 54 ms | 52 ms (GPU only 50 ms) |

The iGPU stayed at 500–650 MHz throughout (its maximum is 1200). The
bench waits for the GPU after every frame, so the GPU idles between
submits and the frequency governor never raises the clock. The earlier
19.7 ms was taken while browsers kept the GPU busy and its clock higher.
Pinning the clock needs root, so these numbers compare variants within a
run, not absolute speed; the split is 2.7× faster than unsplit here.

Planning takes 1.8 ms of the split frame; the GPU part (three reads and a
write per pixel over 205 full-resolution tiles) is the rest, near this
iGPU's bandwidth. The next lever is compositing zoomed-out views at
reduced resolution, which is outside this spec. The "past the budget" row
also pays to re-upload the hot layer's tiles, because the bench's
alternating unsplit frames evict them.

Spec 46 adds one rule: a tile whose hot unit, or any unit above it, is an
adjustment layer is not split, because those layers would amplify the f16
rounding of the cached image.

Follows spec 13 (GPU compositor). No happypaint source: happypaint
recomposites every layer on every change, and this is onepaint's own
performance work.

## Why
The 8k × 100-layer opacity-drag bench (`cargo bench -p onepaint-gpu --bench
opacity_drag`, Iris Xe, 2026-10-10) misses the 16.7 ms frame target badly:

| Drag one layer's opacity (205 tiles, ~31 layers deep) | Per frame |
|---|---|
| Distinct tiles fit the 1 GiB atlas | 128 ms |
| Distinct tiles exceed it | 2.4 s |
| Planning alone | 2–3 ms |

Every frame re-blends all ~31 layers under each dirty tile, though only one
of them changed. Past the budget it is worse: the LRU evicts and re-uploads
most of the stack every frame.

The same waste hits painting. A stroke on one layer of a deep document
recomposites the whole stack under the brush on every frame.

## Idea
For each dirty tile, split the layer stack at the **hot unit**: the
top-level node that contains the layer being changed.

- **Prefix:** everything below the hot unit. Its composite does not depend on
  the hot unit, so it is computed once and cached as an image.
- **Suffix:** everything above. When every part of it is independent of
  what lies underneath (below), it flattens to one premultiplied image `S`.
  The final pixel is then `S + (1 − S.a) · x`, where `x` is the result after
  the hot unit.
- **Hot unit:** its ops run every frame, starting from the cached prefix.

A drag frame then reads three inputs per pixel instead of ~31. That should
bring the drag frame close to the target, and the per-frame working set
shrinks to the hot layer's tiles plus two cache images per tile. That also
ends the thrash past the budget.

## Model (`compose::program`)

### The hot layer
`Session::plan` passes a hot layer to the planner:

1. The layer the stroke overlay targets, if a stroke is in progress.
2. Otherwise, the active layer.

The opacity, blend-mode and visibility sliders all act on the active layer,
so a drag always changes the hot layer. Any other change still plans
correctly: the cache keys (below) miss, and the segments are refilled.

### The split
`plan_tile_split(doc, tx, ty, ov, hot)` returns a `TileProgram` with an
optional `Split`:

```rust
pub struct Segment {
    /// Content key: changes whenever anything the segment reads changes.
    pub key: u64,
    /// The sources it reads, held weakly, so a key can never match a tile
    /// that was freed and whose address was reused.
    pub sources: Vec<Weak<Tile>>,
    pub ops: Vec<Op>,
}

pub struct Split {
    pub prefix: Option<Segment>,
    /// Ops run every frame, starting from the prefix image (or transparent).
    pub hot: Vec<Op>,
    /// Applied as `S + (1 − S.a) · x`; `None` runs the remaining ops directly.
    pub suffix: Option<Segment>,
    /// Ops after the hot unit that could not fold into a suffix image.
    pub tail: Vec<Op>,
}
```

The planning rules:

- The **hot unit** is the top-level node (`doc.root` entry) on the path to the
  hot layer: the layer itself, its top-level group, or its clip family (base
  plus clipped layers).
- The **prefix** is every op before the hot unit. Ops are emitted at stack
  depth 1 between top-level units, so the prefix ends with exactly one
  register, which is what gets cached. It is cached only when it reads at
  least 2 sources; otherwise its ops are prepended to `hot`.
- The **suffix** is the run of top-level units after the hot unit, kept
  while each unit is independent of what lies underneath. A unit qualifies
  when it is one of these:
  - a layer with `BlendMode::Normal` and no `inherit_alpha`;
  - a group, which composites in isolation (`PushZero … Pop`), whose `Pop`
    is `Normal` and has no clip;
  - a clip family whose base layer is `Normal` and has no `inherit_alpha`.

  At the first unit that does not qualify, the run stops. That unit and
  everything after it go to `tail` and run every frame. Typical cases are a
  non-normal mode, `inherit_alpha`, or `Pigment`. Like the prefix, the suffix
  is cached only when it reads at least 2 sources.
- The suffix image is the suffix ops run on a transparent register. Because
  Normal "over" is associative, that is exactly the folded image `S`.
- A tile where the hot unit draws nothing still splits. Its hot ops are
  empty, and the frame is the prefix plus the suffix.
- No hot layer, or a tile the planner can't handle, leaves `split = None`.
  Such tiles behave exactly as today.

### Keys
A segment key is a hash, in order, of every op in the segment:

- the op kind, mode, opacity, flags and tint;
- the `Arc::as_ptr` of every source: base, mask, stroke and mask stroke;
- for strokes, their opacity and mode.

A tile edited through copy-on-write gets a new address, so its key changes.
A parameter edit changes the hashed parameters. The cache therefore needs no
invalidation messages: stale entries simply stop matching and age out of the
LRU.

`sources` keeps a `Weak` to each tile, as the atlas already does (spec 13).
The `Weak` keeps a freed tile's allocation alive, so its address cannot be
reused and a new tile can never match a stale key. A `Weak` pins only the
`Arc` header; the pixels live in the tile's `Vec` and are freed.

### CPU semantics (the oracle)
`program::run` gains the split:

```
reg = prefix.map(run_segment).unwrap_or(zero)
reg = run(hot, reg)
if let Some(s) = suffix { reg = over(run_segment(s), reg) }
reg = run(tail, reg)
```

For every document, `run(split program) == run(unsplit program) ==
composite_tile`, up to f32 rounding.

## GPU (`gpu::compose`)

### The segment cache
- One or more `Rgba16Float` 2D-array **cache atlases**, separate from the
  source atlases. They are created with `STORAGE_BINDING | TEXTURE_BINDING`,
  since fill passes write them and composite passes read them.
- A table maps `segment.key → (cache slot, Vec<Weak<Tile>>)`, evicted least
  recently used within its own budget. The default is 256 MiB, which is 512
  slots: one 256-tile frame's prefix and suffix.
- Slots used by the frame being dispatched are never evicted.

### A frame
For each program with a split:

1. **Fill.** For each missing segment, add a job that runs the segment's ops
   on a transparent register and writes the result into its cache slot.
   These jobs read only source atlases.
2. **Composite.** The main job starts its register from the prefix slot (or
   zero) and runs the hot ops. It then applies the suffix slot as a Normal,
   opacity-1 layer, which is exactly `S + (1 − S.a) · x`, runs `tail`, and
   writes to the canvas target.

The fill and composite passes are separate dispatches, because a cache atlas
can't be both written and read in one dispatch.

Reading a cache slot reuses the `Layer` op with a new source kind ("cache
slot") in its source index. No new blend code is needed. The lean pipeline
(spec 13) covers a job whose ops are only plain Normal layers, and a split
drag frame usually qualifies.

### Warm-up
The first frame after the hot layer changes has to fill both segments for
each dirty tile. That costs about one full recomposite of those tiles. To
keep it off the critical frame:

- When the active layer changes, the canvas queues **warm-up fills** for the
  tiles that layer covers, visible ones first.
- Warm-ups run within the existing per-frame budget, after the frame's real
  work, and never delay it.
- A drag that starts before the warm-up finishes fills the rest on demand.
  This is no worse than today.

### Budget overflow (the 2.4 s case)
With the split, a drag frame's working set is the hot layer's tiles plus the
cache slots, far below the budget. Fills and full recomposites can still
exceed it, so `GpuCompositor::run` also:

- **checks the working set first.** Tiles that cannot fit in one pass go to
  the CPU compositor with progressive compositing, instead of being silently
  skipped (today the atlas logs "atlas full; tile skipped");
- **pins the batch's slots** until the batch is dispatched (already true), and
- **alternates the traversal order** of full refreshes frame to frame, so the
  LRU does not evict exactly the tiles the next batch needs (photocraft
  `gpu/src/lib.rs:886-930`).

## Precision
The prefix and suffix are stored in `Rgba16Float`, the same format as the
canvas target. That adds one f16 rounding at each boundary (relative error
≤ 4.9e-4). Tests compare against the oracle with the spec 13 tolerance:
2e-3 linear, 1/255 on 8-bit display.

## Acceptance criteria

### Tests: `compose`
1. **Split equals unsplit.** Random documents mixing all 19 modes, groups
   (depth ≤ 8), clip families, masks, `inherit_alpha`, ghost tints and a
   stroke overlay. Every layer is tried as the hot layer. The split program
   must equal the unsplit one and `composite_tile` within 1e-5.
2. **Suffix folding rules.**
   - Normal layers above the hot layer fold.
   - A Multiply layer above stops the fold; it and everything after it land
     in `tail`.
   - `inherit_alpha` above stops the fold.
   - An isolated Normal group above folds; a Screen group does not.
   - A clip family with a Normal base folds.
   - A prefix or suffix reading fewer than 2 sources is not cached.
3. **The hot unit.** A hot layer inside a group makes the whole top-level
   group the hot unit. A hot clipped layer makes its clip family the hot
   unit.
4. **Keys.**
   - Changing the hot layer's opacity, mode or pixels leaves both keys
     unchanged.
   - Changing any prefix layer's opacity, mode, visibility, mask or pixels
     changes the prefix key only; the same holds for the suffix.
   - Reordering layers across the split changes both keys.

### Tests: `gpu` (skipped without an adapter)
5. A random document with split programs matches the CPU oracle within
   2e-3, cold and warm.
6. **Reuse.** Ten opacity-drag frames on one layer fill each segment once,
   then only reuse it (count fills).
7. **Invalidation.** Painting on a layer below the hot one refills that
   tile's prefix on the next frame; tiles elsewhere are untouched.
8. **Weak safety.** Drop the tiles a cached segment reads and allocate new
   ones; the stale entry never matches.
9. **Overflow.** A frame whose working set exceeds the budget sends the
   remainder to the CPU path; nothing is skipped and the output matches the
   oracle.

### Bench: `opacity_drag`
10. A warm drag frame takes 16.7 ms or less in both the "fits" and "past the
    tile budget" cases on the reference iGPU, and 8 ms or less on a
    discrete GPU.
11. The first frame after the hot layer changes, without warm-up, takes no
    longer than one full recomposite of those tiles.
12. Painting a stroke on layer 50 of 100: the frame time stays within 1.5×
    of the same stroke on a 2-layer document.
13. `compose` (spec 13) does not regress by more than 5%.

## Out of scope (later)
- **Nested splits:** caching inside a large group around a hot layer within
  it. The whole group is the hot unit for now.
- Folding non-normal suffixes. These need the backdrop and can't fold.
- Caching CPU-fallback tiles. They split the same way once the CPU
  compositor runs programs, which is not this spec.

## Open decisions for the user
- **Cache budget.** The default is 256 MiB, on top of the 1 GiB source
  atlases. It could share one budget with them instead.
- **Warm-up when the active layer changes:** on (proposed), or only on the
  first drag frame.
