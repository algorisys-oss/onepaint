# 44: Partial panel clips on the GPU

## Status
Built. Tiles along a panel frame's edge composite on the GPU. Before this,
every tile a panel frame crossed went to the CPU, which on a manga page
means every tile along every border.

Follows spec 30 (comic panels) and spec 13 (GPU compositor).

## Model
- `Op::Coverage(Arc<Coverage>)` multiplies the top register by the frame's
  coverage. In a panel group's program it goes after the children and
  before the riders, as `composite_nodes` applies it.
- A `Coverage` holds the exact f32 coverage, which the CPU interpreter uses
  so `run` equals `composite_tile` bit for bit. It also holds an f16 tile
  with the coverage in every channel, which the GPU reads as a source
  (`top *= load(slot).x`).
- **`PlanCache`** (in `compose::program`, owned by `engine::Session`) maps
  the panel spec's JSON hash and tile to the clip: full, empty or partial.
  - A frame plan then neither rasterizes a border tile again nor makes a
    new coverage tile. The tile keeps its `Arc`, so the GPU atlas keeps it
    resident.
  - It holds at most 4096 entries, then starts over.
  - `plan_tiles_cached` takes it. `plan_tiles`, `plan_tile` and
    `plan_tile_split` plan without it.
  - It is state the session owns and passes in, not a global, so the
    domain crates stay pure.
- Ghost pegs (out-of-pegs onion offsets) still keep their tiles on the CPU.
  Their content is resampled across tiles, which needs an intermediate
  image pass.

## Acceptance criteria
1. Pages with a slanted panel, a nested rectangular panel, a clipped rider
   and random art plan every tile on the GPU path. Their programs equal
   `composite_tile` exactly. At least one tile carries `Op::Coverage`.
2. With a cache, a second plan reuses the same coverage tiles (same
   `Arc`s) and gives the same result.
3. On the GPU those pages match the CPU oracle within 2e-3, cold and from
   the cache. Making the coverage op a no-op fails the test.
4. By eye: the `ONEPAINT_PANELS=1` demo on the GPU path and with
   `ONEPAINT_CPU_COMPOSE=1` differ by at most 1/255 per channel.
