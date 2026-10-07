# 16: Smudge, mixer, pigment and symmetry (slice 9)

The sources, relative to `../happypaint/frontend/src/`:

- `engine/brush/symmetry.ts`
- `engine/brush/smudge-renderer.ts` (SR)
- `engine/paint-engine.ts:2723-3070` (PE)
- `engine/color/pigment.ts`

## Pigment mix (`color::pigment`)
`pigment_mix(a, b, t)` takes straight sRGB and runs the full model:

1. A 9-band overlapping Gaussian basis (centres 8.2/3.4/2.0, σ 1.0/0.9/1.7),
   with rows normalised.
2. Two-constant Kubelka–Munk, with `S = max(1e-3, min band²)`.
3. The `(BᵀB)⁻¹Bᵀ` pseudo-inverse.
4. Chroma and hue restoration (0.55, trust sat 0.22, arc soft 0.12).

The maths runs in f64, as in JS.

**Acceptance:**
- It matches 588 happypaint goldens (12 colours² × 7 weights) within 1e-5.
- Yellow + blue gives green.
- `mix(x, x, t) = x`.

## Symmetry (`brush::symmetry`)
A transform is `{m00, m01, m10, m11, cx, cy}` and is applied about the
centre. The centre is given as fractions of the document.

| Mode | Transforms |
|---|---|
| off | `[I]` |
| vertical | `[I, refl(π/2 + angle)]` |
| horizontal | `[I, refl(angle)]` |
| both | `[I, refl(angle), refl(π/2 + angle), rot(π)]` |
| radial | `rot(angle + i·2π/n)` for i < n, with n = max(2, round(count)) and count clamped to 2..24 |

Where `refl(φ) = [[cos 2φ, sin 2φ], [sin 2φ, −cos 2φ]]` and
`rot(a) = [[cos a, −sin a], [sin a, cos a]]`.

- **Brush strokes:** every emitted dab, taper and airbrush dabs included, is
  replicated through every transform, in order.
- A dab's rotation is mapped by the linear part:
  `atan2(m10·c + m11·s, m00·c + m01·s)`.
- **Smudge and mixer:** each dab smears through every transform (the pickup
  and deposit are both transformed).
- **Settings:** symmetry is a session setting (`SetSymmetry`), defaulting to
  `{off, .5, .5, 6, 0}`.

## Smudge engine (`brush::smudge`, CPU port of SR + PE)
**Stroke parameters:**
- size (diameter) and strength (master coverage, capped at 0.98)
- hardness and tip, size/flow pressure depths, spacing
- an optional wet setup: `{mix, load, wetness, pigment, keep}` and the
  reservoir colour

**Sampling:**
- A smoother at 0.3, then constant spacing of
  `max(1, size·clamp(spacing, .02, .25))`, or 0.25 when no spacing is set.
- The first point only primes the sampler.
- Each stamp smears from the previous stamp to the current one.

**Per dab:**
- `depth(a, p) = max(.05, 1 − a + a·clamp(p))`.
- `radius = size/2 · depth(sizeP, p)`.
- `strength = strength · depth(flowP, p)`.

**Pickup:** the layer, as it is before this dab, is sampled bilinearly at
`from + l·radius`, where `l` is the dab-local position in −1..1. Outside the
document it is transparent.

**Deposit at `to`, per pixel:**
1. Coverage:
   - `cov = (1 − smoothstep(h, 1, |l|)) · strength`, times the selection
     coverage.
   - With an image tip, `cov` is also multiplied by the tip sampled with
     `l` rotated by the rake angle. That angle is `atan2(to − from)` and is
     held while the move is under 0.25 px.
   - `h = min(hardness, 1 − min(.5, 1.2/max(1, r)))`.
   - With an image tip the hardness is 1, so `h` is also capped by that AA
     term.
2. Pickup colour:
   - `pconf = smoothstep(.002, .05, p.a)`.
   - `pc = mix(rc, clamp(p.rgb/max(p.a, .002)), pconf)`.
3. Mix:
   - `resW = (1 − mix) · deposit`, since the reservoir alpha is 1.
   - `col = srgb2lin(M(lin2srgb(pc), lin2srgb(rc), resW))`, where M is the
     pigment mix or a lerp.
4. Alpha and output:
   - `al = mix(p.a·wet, 1, resW)`.
   - `src = (col·al·cov, al·cov)`.
5. Write:
   - Premultiplied source-over.
   - With alpha lock: `rgb = src.rgb·dstA + dst.rgb·(1 − srcA)`, keeping
     `a = dstA`.

**Pure smudge tool:** no reservoir (`rc = 0`), `mix = 1`, `wet = 1` and
`deposit = 0`.

**Mixer:**
- The load budget is `load·120`; `deposit = clamp(budget/24)`.
- `budget −= deposit·(1 − mix)` per stamp.
- After every stamp, contamination runs:
  - `rate = .18·wet·mix`.
  - The reservoir moves toward the average of a 4×4 bilinear tap grid over
    the last pickup: `rgb = M(prev, avg, rate)` in sRGB.
  - It is ignored when the average alpha is ≤ .001.
- **Keep colour:** the reservoir carries over to the next stroke unless the
  colour changed or Clean was used.

**History:**
- The layer is edited live; begin snapshots the layer (with `Arc` tiles).
- End records one Tiles change for the touched tiles.
- Cancel restores the snapshot.
- The stroke is refused on hidden, locked, text or adjustment layers.

**Defaults:** smudge strength .4. Mixer wet .5, load .5, mix .5, keep off,
pigment on.

## Acceptance
- **Symmetry:** the transforms match the table. A vertical-mirror stroke
  paints a mirror image (pixel-symmetric within 1e-3). Radial 4 paints four
  rotated copies.
- **Smudge:**
  - Dragging from a red square into white carries red into the white area.
  - Strength 0 changes nothing.
  - It is one undo step, and cancel restores the layer exactly.
  - With alpha lock, no coverage is added outside the shape.
- **Mixer:**
  - With mix 0 and load 1, a stroke on transparent paints the reservoir
    colour.
  - Pigment-mixing yellow on blue gives green.
  - The paint runs out with load: a long stroke's tail is fainter than its
    head when mix < 1 and there's no canvas.

## Inherited behaviour (kept for parity)
The smudge deposit is premultiplied source-over of the pickup, the same as
happypaint's `gl.blendFunc(ONE, ONE_MINUS_SRC_ALPHA)`. On a semi-transparent
area, overlapping dabs compound:

- One dab gives `a + a·cov·(1−a)`.
- So a faint 0.15-alpha band smudged with a 70 px soft brush builds up to
  about 0.97.

Opaque areas behave like a classic smudge. A "replace" blend,
`mix(dst, pickup, cov)`, would keep the alpha flat. That is a behaviour
change, so it waits for a decision.
