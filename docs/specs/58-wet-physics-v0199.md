# 58: Wet physics, v0.195.3–v0.199

## Status
Phases A and B built (2026-10-19): paper absorbs and the water halo, in
the CPU step (the oracle) and the GPU step; live dabs take the brush tip
(`fluid::WetTip`, `wet_dab_rotation`; deposits are CPU-only). Phase C
(wash fill) to come; D is not needed (below).

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
  the pigment stays within the radius; Wet-on-Wet Live has 1.5.
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

## C. Wash fill (to come)
The Fill tool's Wash: a chamfer distance field from the tap
(`wash-field.ts` and its tests), a spreading front that deposits water
and pigment band by band, walls (per-wash ids, a 6 px guard band) that
stop flow across the region's edge, water-only washes, and the toolbar
options (`wash-options.test.ts`).

## D. Sim-rect shrinking: not ported
happypaint measures the wet extent on the GPU every 12 steps to shrink
its sim rect. onepaint already steps only the 16 px blocks holding state
(spec 35 B), which does the same job.

## Acceptance criteria
1. The GPU step matches the CPU step (`gpu/tests/wet.rs`), with
   mutations to the acceptance gate, the pigment bloom and the slope push
   caught.
2. A soaking dab spreads into dry paper only slowly, a damp one not at
   all; a halo wets past the pigment.
