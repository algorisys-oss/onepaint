# 02: Document model and compositing

Sources: `engine/compositor.ts` (C), `engine/layer.ts`, `engine/document.ts`
and `engine/color/color.ts`.

## Document model (`doc`)
- `Document { id, name, width, height, dpi (default 300), color_space
  ("srgb" | "display-p3"), depth (Depth16F for slice 1), created_at,
  updated_at, root: Vec<Node>, extra: serde_json::Map }`.
- `nodes[0]` is the **bottom** layer.
- `Node = Group | Layer`.
  - Common fields: `id, name, visible, opacity (0..1), blend_mode`, plus
    opaque `extra` JSON that is preserved on load and save.
  - A layer adds `clipped, locked, alpha_lock, inherit_alpha,
    mask: Option<Surface>, mask_enabled (default true), pixels: Surface`.
  - A group adds `children` and `collapsed`.
- A new document gets a "Background" layer filled with white, plus a
  transparent "Layer 1" above it (`document.ts:28-46`). The background is an
  ordinary layer.
- `BlendMode`: the 20 kebab-case strings in `layer.ts:14-35`, including
  `pigment`, which is brush-only.

## Blend math (shader path; C:316-411)
These are the inputs, both premultiplied:

- `sp = src·opacity`
- `bp = backdrop`

1. In a float (linear) document, every mode except **normal** and **add** first
   converts both inputs to premultiplied sRGB:
   `premLin2Srgb(p) = (lin2srgb(p.rgb/p.a)·p.a, p.a)`. The output is
   converted back with `premSrgb2Lin`. In other words, blend modes are
   always computed in gamma space.
2. Unpremultiply: `Cs = as>0 ? sp.rgb/as : 0` and `Cb = ab>0 ? bp.rgb/ab : 0`.
3. `B = blend(mode, Cb, Cs)`, then:
   - `ao = as + ab(1−as)`
   - `co = as(1−ab)Cs + as·ab·B + (1−as)ab·Cb` (premultiplied)

Per-channel `bc(b,s)`. Port these exactly, including the cases that differ
from W3C:

| Mode | Formula |
|---|---|
| multiply | `b·s` |
| screen | `b+s−b·s` |
| overlay | `b≤.5 ? 2bs : 1−2(1−b)(1−s)` |
| darken | `min` |
| lighten | `max` |
| color-dodge | `s≥1 ? 1 : min(1, b/(1−s))` |
| color-burn | `s≤0 ? 0 : 1−min(1,(1−b)/s)` |
| hard-light | `s≤.5 ? 2bs : 1−2(1−b)(1−s)` |
| soft-light | `s≤.5 ? b−(1−2s)b(1−b) : b+(2s−1)(d−b)`, where `d = b≤.25 ? ((16b−12)b+4)b : √b` |
| difference | `|b−s|` |
| exclusion | `b+s−2bs` |
| linear-burn | `max(0,b+s−1)` |
| divide | `s≤0 ? 1 : min(1,b/s)` |
| add | `b+s` (shader) |
| normal | `s` |

Non-separable modes use luma `(0.3, 0.59, 0.11)`, with the W3C
`ClipColor`/`SetLum`/`SetSat` and their `1e-6` guards:

- hue: `SetLum(SetSat(s, sat(b)), lum(b))`
- saturation: `SetLum(SetSat(b, sat(s)), lum(b))`
- color: `SetLum(s, lum(b))`
- luminosity: `SetLum(b, lum(s))`

## Fixed modes (float document)
These are done in linear, on premultiplied values:

- normal: `o = s + d(1−s.a)`
- add: `o.rgb = s.rgb + d.rgb` (no clamp in float); `o.a = s.a + d.a(1−s.a)`

## Layer semantics (C:1462-1741, 1888-2091)
The walk goes bottom to top.

- **Groups are always isolated.** Composite the children into a
  transparent buffer, then blend that buffer with the group's mode and
  opacity. Skip the group if it is not visible, its opacity is 0 or it has
  no children. Clipped siblings that follow a group clip to it, and a
  hidden group hides them too.
- **Mask:** `content = tex · mask.r`, applied before opacity. Skipped when
  `mask_enabled == false`.
- **Clip family:** a base plus the clipped, non-group layers that directly
  follow it.
  - A clipped layer at the bottom, or one with no base, renders as a
    normal layer.
  - If the base is hidden or its opacity is 0, the whole family is hidden.
  - Build the family: start transparent, draw the masked base at opacity 1
    with normal blending, then draw each visible clipped layer (masked) as
    `o = s·opacity·d.a + d(1−s.a·opacity)`. That keeps the base alpha.
  - **A clipped layer's own blend mode is ignored.**
  - Finally composite the family with the base's mode and opacity.
- **Inherit alpha:** draw the content, multiply it by the backdrop alpha,
  then composite it with the layer's mode and opacity.
- **Alpha lock** only affects painting (see spec 03).

## Display
- The checkerboard is fixed to the screen, with squares of
  `max(2, 10·dpr)` device px. Its colors are display sRGB values
  `0.85` and `0.64`. The composite is OETF-encoded to sRGB and then laid
  over the checker with source-over.
- The stage color comes from a theme token.

## Acceptance tests (goldens computed from the formulas above)
- `bc` for every separable mode at (b,s) in {0, .25, .5, .75, 1}²,
  including these special cases:
  - dodge(0,1) = 1
  - burn(1,0) = 0
  - divide(x,0) = 1
- Non-separable: hue/sat/color/lum of a pure red source over a pure blue
  backdrop match a hand-computed W3C result.
- Source-over normal: opaque red at 0.5 opacity over transparent gives
  premultiplied (0.5, 0, 0, 0.5).
- Multiply in a float document matches `premSrgb2Lin(W3C(premLin2Srgb(...)))`.
- Clip family: a clipped layer never extends past the base alpha, and its
  own blend mode is ignored.
- Groups are isolated: a multiply child inside a group does not see
  content outside the group.
- A mask with r = 0.5 halves the coverage.
- Incremental compositing: recompositing only the dirty tiles gives the
  same result as a full composite.
