# 03: Brush, stroke and input

Sources: `engine/brush/brush-engine.ts` (BE), `engine/stroke/stroke.ts`,
`engine/stroke/smoothing.ts`, `engine/stroke/stabilizer.ts`,
`input/pressure.ts`, `engine/brush/brush-presets.ts`,
`engine/brush/stamp-renderer.ts` and `util/rng.ts`.

## StrokePoint
`{x, y, pressure 0..1, tilt_x, tilt_y (−1..1), twist (degrees), t (ms)}`, in
document pixels.

The pipeline for each input point is: raw → stabilizer (optional) → EMA
smoother → spacing sampler → dab emission.

## Pressure provider (`input/pressure.ts:276-333`)
- **Velocity synth**, for mouse or for a pen whose pressure reads as
  "flat". Coordinates are in screen px.
  - The first call records the point and returns `ema = 0.7`.
  - Each later call:
    - `dt = max(1, t−last.t)`
    - `speed = |d|/dt`
    - `target = clamp(1−0.18·speed, 0.25, 1)`
    - `ema += (target−ema)·0.25`
- **Hardware:** `pow(clamp(raw, 0.02, 1), 0.7)`.
- **Choosing the mode for a pen:**
  - If the first raw value is within 0.02 of 0.5, the pen counts as
    "flat" and uses the velocity synth.
  - A flat pen upgrades to hardware once a raw value leaves that band.
  - Hardware never downgrades.

## EMA smoother (`smoothing.ts:132-166`)
- `tau = clamp01(strength)·60` ms. The first point passes through
  unchanged.
- `dt = (t && lastT) ? t−lastT : 16`; if `dt ≤ 0`, set `dt = 4`.
- `follow = tau≤0 ? 1 : 1−exp(−dt/tau)`, and `tiltFollow = min(1, 2·follow)`.
- Update the smoothed state:
  - x and y move toward the input by `follow`.
  - Pressure, tilt_x and tilt_y move toward the input by `tiltFollow`.
  - t passes through.
  - twist is dropped (becomes 0).

## Stabilizer (`stabilizer.ts:197-245`)
Pulled string, with `len = clamp01(strength)·120`.

- **First point:** the anchor becomes a copy of the point, and the anchor
  is emitted.
- **Within the string** (`d ≤ len`, or `d == 0`): the anchor takes the
  point's pressure, tilt and t, and nothing is emitted.
- **Beyond the string:** the anchor moves `(d−len)/d` of the way toward
  the point and is emitted.
- **finish():** emits the last point, unless it is already at the anchor.
  This output skips the EMA.

## Spacing sampler (`stroke.ts:60-113`)
- The first point is always a stamp, and
  `distToNext = max(0.5, gap(first))`.
- For each later point:
  - `dist = |p−prev|`. If `dist == 0`, skip the point and keep `prev`.
  - Walk along the segment: while `remaining ≥ distToNext` (at most 4096
    stamps):
    - Step forward by `distToNext` and lerp the stamp at
      `t = acc/dist`. All fields are lerped, and twist takes the shortest
      arc.
    - `distToNext = max(0.5, gap(stamp))`.
  - Carry the leftover: `distToNext = max(0.5, distToNext − remaining)`,
    then `prev = p`.
- `gap = spacing · 2 · radius_for_spacing(stamp)`, i.e. spacing times the
  dab diameter. Round tips need no roundness factor.

## Dynamics (the legacy scalar path)
- `cp = pressureCurve==1 ? clamp01(p) : clamp01(p)^pressureCurve`
- `radius = max(0.5, size·max(0.15, 1−sizeP+sizeP·cp)/2)`
- `flow = clamp01(flow·(1−flowP+flowP·cp))`
- **Speed:** `speedSmooth += (dlen/dt − speedSmooth)·0.3`, and
  `speed = min(1, speedSmooth/5)`.
  - `radius *= 1−speedSize·speed`
  - `alpha *= 1−speedFlow·speed`
- **Opacity linearization** (`BE:787-800`): skip it when `lin ≤ 0`, wash is
  set, or `a ≥ 0.999`. Otherwise:
  - `n = max(1, 1/max(0.02, spacing))`
  - `L = 1−(1−a)^(1/n)`
  - `a' = a + (L−a)·lin`, with `lin = 0.9` by default
- **Tilt shape:** if `tiltStrength > 0` and `mag ≥ 1e-4`:
  - `rotation = atan2(tiltY, tiltX)`
  - `aspect = 1 − mag·tiltStrength·0.7`
- **Dynamics bindings:** the curve sensors (pressure, random, …) use
  piecewise linear or Fritsch–Carlson smooth curves, combined by multiply.
  Value = `min + (max−min)·curve(sensor)`.
- **Taper:** entry and exit radius factors, measured as distance along the
  stroke.
- **RNG:** mulberry32, seeded per stroke. Each dab draws `random` once.

## Dab shape (`stamp-renderer.ts:37-165`)
- Each dab has a center, a radius, an alpha, a rotation, an aspect and a
  hardness.
- **Local coordinates:** undo the rotation, then divide by
  `(r/√aspect, r·√aspect)`. The result is `local ∈ [−1,1]²`.
- **Anti-aliasing:** `aa = clamp(1.2/max(1, r·√aspect), 0, 0.5)`, and
  `inner = min(hardness, 1−aa)`.
- **Round tip:** `m = 1 − smoothstep(inner, 1, |local|)`.
- **Output:** premultiplied `(lin(color)·a, a)` with `a = m·alpha`, where
  `lin` is srgb2lin because documents are linear.
- **Accumulating into the stroke buffer:** source-over by default. With
  `wash`, use a per-channel max instead.

## Flatten at stroke end (C:2191-2264)
`op` is the brush opacity.

- **Normal:** `dst = src·op + dst·(1 − src.a·op)`.
- **Other modes:** use the blend math from spec 02, with src = stroke·op.
- **Eraser:** `dst *= (1 − src.a·op)`, applied to all channels.
- **Alpha lock:**
  - `rgb = src·op·dst.a + dst.rgb·(1 − src.a·op)`
  - `a = dst.a`
  - Erasing under alpha lock does nothing, and no history entry is
    created.
- **Live preview:** draw the stroke buffer over the layer with the same op.

## Presets (slice 1, `brush-presets.ts`)
Values copied verbatim:

- hard-round
- soft-round
- soft-airbrush (with no airbrush ticks in slice 1)
- pencil (with no grain in slice 1)
- ink
- comic-inker
- studio-pen
- eraser-hard
- eraser-soft

## Acceptance tests (happypaint goldens)
- **G1:** sampler with a fixed gap of 2.4. The points
  `(0,0,p1,t0), (10,0,p1,t10), (10,5,p.5,t20)` give these stamps:
  - `[0,0,1,0]`
  - `[2.4,0,1,2.4]`, `[4.8,…]`, `[7.2,…]`, `[9.6,0,1,9.6]`
  - `[10,2,.8,14]`, `[10,4.4,.56,18.8]`
- **G2:** hard-round gap from `(0,0,p0)` to `(20,0,p1)`. The stamp x values
  are: 0, .5, 1, 1.5, 2.013, 2.578326, 3.201315, 3.887849, 4.64441,
  5.47814, 6.39691, 7.409395, 8.525153, 9.754719, 11.1097, 12.60289,
  14.248384, 16.06172 and 18.060015. The last stamp's pressure is
  0.903001.
- **G3:** smoother at strength 0.25. The inputs `(0,0,.2,t0), (10,0,1,t8),
  (20,0,1,t16), (20,10,.5,t16)` give:
  - `[0,0,.2]`
  - `[4.133538,0,.861366]`
  - `[10.692,0,.975976]`
  - `[12.870739,2.340717,.753151]`
- **G4:** stabilizer at strength 0.5. The inputs `(0,0,.5), (30,0,.6),
  (100,0,.7), (100,50,.8)` give:
  - `[0,0,.5]`
  - nothing
  - `[40,0,.7]`
  - `[53.906723,11.588936,.8]`
  - finish gives `[100,50,.8]`
- **G5:** velocity synth. The (x,t) inputs `(0,0), (5,10), (25,20),
  (125,30), (125,40), (126,60)` give 0.7, 0.7525, 0.724375, 0.605781,
  0.704336 and 0.776002.
- **G6:** hardware pen. Raw values .01, .08, .3 and 1 give 0.064673,
  0.170672, 0.430512 and 1. A flat pen with raw .5, .5, .51, .3 at
  (0,0,0), (5,0,10), (10,0,20), (15,0,30) gives 0.7, 0.7525, 0.791875 and
  0.430512.
- **G7:** the curve `[(0,0), (.5,.8), (1,1)]` at x = 0, .25, .5, .75, 1:
  - linear: 0, .4, .8, .9, 1
  - smooth: 0, .4375, .8, .9375, 1
- **G8:** mulberry32.
  - seed 12345: .979728, .306752, .484205, .817934
  - seed 1: 0.6270739405881613, 0.002735721180215478, 0.5274470399599522
- **G9:** `linearizeAlpha` with lin = .9, as (a, spacing) → result:
  - (.6, .06) → 0.108144
  - (.35, .08) → 0.065488
  - (.85, .15) → 0.307895
  - (.5, .1) → 0.11027
  - (.5, 1.5) → 0.5
- **Dabs:** a hard dab's center is fully covered and the area outside its
  radius is untouched. The eraser removes alpha, and alpha lock keeps
  alpha.
