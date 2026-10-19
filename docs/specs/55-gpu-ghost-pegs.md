# 55: Ghost pegs on the GPU

## Status
Built (2026-10-19). The last large CPU fallback in `todo.md` §2.1.

## Why
An onion-skin ghost out of its pegs (spec 24) shows a neighbouring frame
moved, turned or scaled about the document's centre: each pixel samples the
frame's unmoved content through the peg's inverse (`compose::pegged`,
happypaint `compositor.ts` `pegMatrix`). The planner refused such groups,
so every tile under a pegged ghost composited on the CPU, every frame of a
playback or a scrub.

## Model
A pegged group is a spatial source of its own (`SpatialSrc::of_peg`), on
spec 52's machinery:

- **Planner:** the group's children are planned, as a document of their
  own, on the content tiles the tile's inverse-mapped corners reach
  (`peg_span`, shared with the oracle). The group's register starts as that
  content resampled (`Op::PushContent`), and its panel clip, riders, tint
  and pop follow as for any group. A tile no content reaches pushes a clear
  register.
- **CPU interpreter:** runs the content programs, quantizes them to f16
  (as the GPU's cache images are) and resamples them bilinearly, clear
  outside the tiles composited, as the oracle does.
- **GPU:** the content programs fill segment cache slots, `assemble` lays
  them out in a scratch image, and `blur_out` mode 6 resamples it through
  the inverse into the filtered tile, which the composite reads.
- **Limits:** the scratch image holds 3 × 3 tiles, so a peg that shrinks
  the content far enough to reach more (scale below about 0.5 with a turn)
  stays on the CPU. Above level 0 the resampling reads the level's content
  images: a preview, as spec 39's.

## Tests
- `pegged_ghosts_plan_and_match_the_tree_compositor` (compose): random
  documents in a ghost group with a random peg, tint and opacity, within
  2e-3 of the oracle.
- `pegged_ghosts_match_the_tree_compositor` (gpu), and
  `pegged_ghosts_preview_within_the_bound` above level 0.
- `a_peg_reaching_past_three_tiles_stays_on_the_cpu` (compose),
  `pegged_onion_ghosts_plan_on_the_gpu_path` (engine).
