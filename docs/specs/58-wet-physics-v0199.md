# 58: Wet physics, v0.195.3–v0.199

## Status
Built (2026-10-19). A: paper absorbs and the water halo, in the CPU step
(the oracle) and the GPU step. B: live dabs take the brush tip
(`fluid::WetTip`, `wet_dab_rotation`; deposits are CPU-only). C: wash
fill (`fluid::wash`, `fluid::walls`, `Command::WashFill`, the Fill tool's
Wash options; `ONEPAINT_WASH=1` for screenshots). D is not needed
(below).

## Source
happypaint `engine/wet/wet-renderer.ts`, `wet-dab.ts`, `wash-field.ts`,
`wet-extent.ts`, `wet-model.ts`; commits `79d1aa5` (v0.195.3, tip-shaped
dabs), `efd48b1` (v0.198.0, wash fill), `3529d25` (v0.198.1, water-only
washes, the wall guard band), `b5bdbcc`/`4cffe2a` (v0.199.0–1, paper
absorbs), `675646a` (v0.199.2, halo), `c309f01`/`1b5082c` (v0.199.10–11,
sim-rect shrinking).

## A. Paper absorbs and the halo
- Advection: after the fastest-neighbour trace, `accept =
  max(smoothstep(0.3, 1, w_here), smoothstep(0.15, 0.6, downhill))` and
  the trace scales by `mix(0.05, 1, accept)`; `downhill` is the trace
  along the tilt.
- Water moves only between wet pixels: `w_flux = Σ smoothstep(0.3, 1,
  min(w, wn)) · (wn − w)` replaces `Σ wn / 8 − w` (same 0.5 · resist ·
  oil factor, ×0.125).
- Pigment drift: `lam ·= 1 + 0.8 · smoothstep(0.2, 1.5, min(w, wn))`.
- Slope push on open paper ×4 (×3 inside a wash's walls, phase C).
- `fluid.halo` (1..3, absent = 1): the water dab reaches `radius · halo`,
  the pigment stays within the radius; Wet-on-Wet Live has 1.5. The
  brush studio's Fluid section has a Halo slider (1.0×–3.0×).
- `wetModel`: new drawings are stamped 2 (`CURRENT_WET_MODEL`); like
  happypaint, every drawing runs the current physics.

## B. Tip-shaped dabs
Water and pigment deposits take the brush tip: the local position turns
by the dab's rotation, its minor axis divides by the roundness (0.05..1),
and an image tip's mask is sampled over its square; other tips keep the
round falloff (water 0.55, pigment 0.45), squashed. Drag, re-wet lift and
relief stay round. The rotation is the dry stamp's source (stroke
direction held through moves under 0.25 px, twist, pen tilt or none)
plus its offset and the tip angle, no jitter or spin; symmetry maps it.
A plain round tip keeps the disc.

## C. Wash fill
- The region is the Fill tool's colour match from the tap (the active
  layer, or all layers), grown, inside the selection; the selection's
  coverage weighs the wash.
- `fluid::wash::wash_field`: a chamfer distance (5-7-11, knight's moves
  only through the region) from the tap and from the region's edge (the
  document border isn't one), with a 2-octave positional wobble (8 px).
- Walls: each wash stamps a fresh id (1..255, cycling) over its region
  and a 6 px guard band around every boundary between ids. In the band
  the update step won't advect from across a wall and reads neighbours
  across one as itself; inside a wash the slope push is ×3 (open paper
  ×4). Walls last the session. On the GPU they ride behind the
  active-blocks buffer (8 storage buffers per stage).
- Each step first lays a band: water `ramp(to + 10 − r)` (MAX) at level
  `0.5 + 2·water`, then pigment between `from` and `to`, thinned by
  `grade` with distance, KM pre-mixed with the paint under it; the
  front starts 32 px before the tap and moves `speed` px per step to
  32 px past the farthest pixel. Water only skips the pigment. The
  session doesn't dry while a wash spreads; the GPU steps one at a time
  then, between CPU bands.
- Fill tool: Wash, Water only, All layers; Water, Pigment, Graded, Soft
  (0..24 px) and Speed (2..50 px per step, `2·25^s`).

## D. Sim-rect shrinking: not ported
happypaint measures the wet extent on the GPU every 12 steps to shrink
its sim rect. onepaint already steps only the 16 px blocks holding state
(spec 35 B), which does the same job.

## Acceptance criteria
1. The GPU step matches the CPU step (`gpu/tests/wet.rs`), walls
   included, with mutations to the acceptance gate, the pigment bloom,
   the slope push, the wall taps and the wash flow caught. A whole wash
   session dries within 5e-4 mean (float order drifts over ~1000 steps).
2. A soaking dab spreads into dry paper only slowly, a damp one not at
   all; a halo wets past the pigment.
3. happypaint's `wetDabRotation`, `wash-field` and `wash-options` tests.
4. A wall stops paint at the region's edge; a wash spreads from the tap,
   water ahead of pigment, stays in its region, dries and undoes as one
   step.
