# 47: Skip layers under an opaque tile

## Status
Built. When the planner uses the session's `PlanCache`, a tile program
leaves out everything under a fully opaque normal layer.

From the photocraft comparison ("skip layers under fully opaque tiles").

## Model
- A layer *occludes* on a tile when all of these hold:
  - it blends Normal at opacity exactly 1, without inherit alpha;
  - it has no mask, live stroke or relief there;
  - its tile's alpha is exactly 1 everywhere (f16 0x3c00).
- Normal blending then gives `c + top · 0 = c` for any finite backdrop, so
  everything the register held before can go. The result is bit for bit
  what the tree compositor gives.
- The planner keeps a stack of where each open register's ops begin. An
  occluding layer cuts the ops back to its register's start: the
  document's start at the top level, or just after the group's push inside
  a group.
- At the top level, the units it cut go too, so a hot layer under an
  opaque one gives no split.
- Opacity is checked once per tile: `PlanCache::opaque` is keyed on the
  tile's address, holds a `Weak` so the address can't be reused, and drops
  dead entries before it grows past 65,536.
- Plans without a cache skip nothing (tests and benches see full programs).
- A layer that is a clip base (it has riders) doesn't occlude, because its
  family composites as a pushed register.

## Acceptance criteria
1. Random documents with opaque layers at the top level and inside a group
   plan, with a cache, to programs that equal `composite_tile` exactly.
   Some of them are shorter than the uncached plan.
2. A hot layer under a top-level opaque layer that isn't a clip base gives
   no split.
