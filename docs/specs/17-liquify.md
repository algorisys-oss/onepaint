# 17: Liquify (slice 10)

The sources are `engine/liquify/liquify-renderer.ts` and
`engine/paint-engine.ts:7983-8080`, relative to `../happypaint/frontend/src/`.

## Warp (`filters::liquify`)
Each dab works at pixel centres `P = (x+.5, y+.5)`.

1. Distance: `dd = |P − c| / max(r, 1)`.
2. Pixels with `dd ≥ 1` are unchanged.
3. Weight: `f = 1 − dd²`, then `w = f²·strength`.
4. The source position depends on the mode:

| Mode | Source position |
|---|---|
| push | `P − w·(c − prev)` |
| twirl-left / twirl-right | `P` rotated about `c` by `±0.6·w` rad |
| pinch | `P + 0.5·w·(P − c)` |
| expand | `P − 0.5·w·(P − c)` |
| reconstruct | `mix(cur(P), orig(P), w)` (no warp) |

**Sampling:**
- Bilinear on premultiplied RGBA, at `src − .5` in texel space, with
  clamp-to-edge.
- The source position is clamped to the document.
- The source is the layer as it was **before this dab**, so the
  distortion accumulates dab by dab.

## Session
- **Begin** snapshots the layer (`Arc` tiles), which is `orig` for
  reconstruct.
  - The radius is `max(2, size/2)`.
  - The strength is clamped to 0..1.
  - It is refused on hidden, locked and adjustment layers. Text layers are
    rasterised first.
- **Extend:** each segment `last → p` is split into
  `min(64, max(1, ceil(len/spacing)))` dabs, with `spacing = max(1, .3·r)`.
  Dab i is centred at step i, with `prev` at step i−1.
- **End** records one Tiles change. **Cancel** restores the snapshot.
- **Defaults:** mode push, strength 0.5. The size is the brush size.

## Acceptance
- **Push:** a vertical edge pushed sideways moves in the push direction.
- **Pinch:** the area pulls in toward the centre, so a ring's radius
  grows. **Expand** does the opposite.
- **Twirl:** a horizontal line through the centre rotates.
- **Reconstruct:** at strength 1, repeated dabs return to the original
  within 1e-3.
- Strength 0 is the identity.
- It is one undo step, and cancel restores the layer exactly.
