# 10: Adjustments and filters (slice 4)

The source is `engine/adjustments/adjustment-renderer.ts` (AR). happypaint has
**no pixel goldens** for these kinds, so onepaint's tests are hand-computed
from the shader formulas below. Those tests become the reference for the
GPU port later.

## Colour pipeline (every colour kind)
Pixels are premultiplied and linear (float document).

1. `c = a>0 ? rgb/a : rgb`
2. `w = lin2srgb(c)`. This is the work space: **adjustments run in sRGB**.
3. Adjust `w`.
4. `out = srgb2lin(w')·a`. Alpha is unchanged.
5. Selection: `out = mix(src, out, keep)`, where `keep` is the feathered
   selection, or 1 when there's none.

The kinds:

| Kind | Formula | Default params | UI |
|---|---|---|---|
| brightness-contrast | `w += b`; `k = c≤0 ? 1+c : 1/max(1−c, 1e-3)`; `w = clamp((w−.5)·k+.5)` | `{brightness 0, contrast 0}` in −1..1 | −100..100 |
| hue-saturation | HSV (GLSL `rgb2hsv` with `1e-10` guards); `h = fract(h+hue/360)`; `s,v *= 1+sat, 1+val`, clamped | `{hue 0, saturation 0, brightness 0}` | hue ±180, others ±100 |
| black-white | `lum = dot(clamp(w), .299/.587/.114)`; `o = mix(w, lum, amount)`, **not clamped** | `{amount 1}` | 0..100 |
| invert | `clamp(1−w)` | none | none |
| threshold | `lum ≥ t ? 1 : 0` on clamped `w` | `{threshold .5}` | 0..100 |
| posterize | `n = max(levels, 2)−1`; `floor(w·n+.5)/n` on clamped `w` | `{levels 4}` | 2..32 |
| levels | `n = clamp((w−inB)/max(inW−inB, 1e-4))`; `n^(1/max(γ, 1e-3))`; `outB + n·(outW−outB)`, clamped | `{0, 1, γ 1, 0, 1}` | γ 0.1..3 |
| curves | 256-entry LUT: `m = curve(i/255)`, then the channel curve on `m`; bytes `round(clamp(v)·255)`; sampled **bilinearly at `v·256−0.5`** (texel centres) | master identity, `smooth` | curve editor |
| color-balance | `w' = 1−|2·lum−1|`; `rgb += (cr, mg, yb)·0.5·w'`, clamped | all 0 | ±100 |
| gradient-map | LUT of the sorted stops (`round(c·255)`), sampled at `lum` like curves | black → white | stops |
| noise | uniform `[−.5, .5)·amount` from the pcg hash of (x, y, seed=17·3); mono or per-channel; clamped | `{amount 0, mono true}` | 0..100 |

## Spatial kinds
These run on premultiplied RGBA, alpha included, with no work-space
conversion.

- **gaussian-blur:** two separable passes. Each pass takes 65 taps at
  `i·R/32` px for `i ∈ [−32, 32]`, with weight `exp(−.5·i²/16²)`, bilinear
  sampling and clamp-to-edge. The radius is treated as at least 1, and
  `R ≤ 0` is the identity.
- **motion-blur:** a single pass of the same kernel along `(cos θ, sin θ)`,
  with radius = distance.
- **sharpen:** an R=2 separable blur `b`, then
  `clamp(s + (s−b)·amount·3)`.
- The selection mask mixes the result into the original, **after** the
  full-layer blur.

## Identity
The destructive preview skips the work when the params are the identity:

- brightness-contrast, hue-saturation, color-balance, levels: all
  defaults.
- black-white: `amount ≤ 0`.
- curves: every point satisfies `|y−x| < 1e-3`.
- gaussian-blur `radius ≤ 0`, motion-blur `distance ≤ 0`, sharpen and
  noise `amount ≤ 0`.
- invert, threshold, posterize and gradient-map are never the identity.

## Destructive filter (PE:8102-8255)
- **Begin:** blocked when the layer is hidden or locked. Keeps the original
  pixels.
- **Update:** recomputes from the original, never from the previous result.
- **Commit:** one undo entry. **Cancel:** restores the original.
- Alpha lock is not consulted.

## Adjustment layers
- The layer's `adjustment = {kind, params}`. It is read from and written to
  `adjustmentKind` and `adjustmentParams`.
- **Compositing:** `adjusted = apply(backdrop)`, then the layer mask, then
  `compositeResult(adjusted, mode, opacity)`.
- onepaint composites per tile, so only the **colour kinds** are live
  adjustment layers. Spatial kinds on layers are preserved but not applied
  yet (see divergences).

## Acceptance
- brightness +0.2 on sRGB grey .5 gives .7, checked through the
  linear↔sRGB round trip.
- contrast ±: grey .5 doesn't move; with contrast .5, w=.75 → 1.0.
- invert and threshold work on hand-picked points.
- posterize with levels 2 rounds to 0 or 1.
- levels with γ 2 maps .25 → .5.
- black-white amount 1 gives the luma.
- The hue-saturation round trip at 0 is the identity (≤1e-5); hue +120°
  turns red into green.
- The curves identity LUT reproduces the input within the 8-bit
  quantisation, and the bilinear texel lookup matches the formula.
- The colour-balance weight is zero at black and white.
- Gaussian blur keeps a flat field flat, conserves mass away from edges, and
  `R=0` is the identity.
- Sharpen of a flat field is the identity.
- Noise is deterministic per pixel, and amount 0 is the identity.
- A filter commit is one undo step and cancel restores exactly.
- An adjustment layer (invert) over a red layer gives cyan, masks scale the
  effect, and it round-trips through `.happypaint`.
