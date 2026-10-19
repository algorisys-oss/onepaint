# 56: Layer styles

## Status
Phases 1 and 2 built (2026-10-19): `doc::styles` (the model, sanitizer,
contours, reach, Global Light, with happypaint's model tests) and
`compose::styles`, the oracle: styled layers and groups render in the tree
compositor; tiles with styles stay on the CPU (the planner refuses them)
until phase 3. Divergence 51. Phase 4 in part: `LayerProps::styles`
(one undo step per drag or click; dirty rects grow by the reach), the
built-in styles, and the Layer Style editor in Properties for pixel, text
and path layers and groups; `ApplyGlobalLight` (linked effects across the
document, a drag one undo step), `BakeStyles` (from the unmasked pixels,
effects drawn Normal, the mask kept, as happypaint), and Merge Down bakes
both layers' styles as far as they reach. Still to come in phase 4: user
presets and gradient-stop editing. Phase 5 in part: PSD effects
(`formats::psd_effects` on a general descriptor reader, `psd_desc`):
`lfx2`/`lmfx` both ways in ag-psd's byte order, `iOpa`, Global Light in
resources 1037/1049, contours by name or nearest curve, happypaint's
psd-effects tests. Pattern overlays and bevel textures aren't written (as
happypaint). `.asl` import waits for user presets. happypaint v0.196.0–0.196.8; the source of truth is
`engine/styles/layer-styles.ts` (model) and `style-renderer.ts` (WebGL),
with `docs/layer-styles-spec.md` in happypaint.

## What it is
Photoshop-style effects on a pixel, text or path layer, or a group:
drop and inner shadow, outer and inner glow, stroke (outside, inside,
centre), colour, gradient and pattern overlay, satin, and bevel & emboss
(inner, outer, emboss, pillow; contour and texture). Each effect has its
own blend mode and opacity; stroke, shadows and colour and gradient
overlays repeat up to four times; Fill opacity fades the layer's own
pixels and leaves its effects. A file without styles is unchanged.

## Model (`doc`)
- `Layer::styles` and `Group::styles`: `Option<LayerStyles>`, serialised
  as happypaint's `styles` key (`{fillOpacity, effects: [...]}`, camelCase,
  effect `kind` in kebab case). Unstyled nodes write no key.
- `doc::styles`: the effect types, `default_effect`, the sanitizer
  (clamps, at most four of the repeatable kinds, one of the rest; missing
  `blendMode` reads as Normal, as files before v0.196.2 look), the seven
  contours, `shadow_offset`, `effect_reach` and Global Light
  (`global_light_of`, `with_global_light`). happypaint's
  `layer-styles.test.ts` ports to Rust tests first.

## Rendering
The effects are a function of the layer's (or group's) own composite,
its signed distance field, and, for non-Normal effect modes, the backdrop
under the layer. Spec 52 already composites a layer from content filtered
over its tiles ± a reach, so styles reuse that machinery: a styled node's
content is a **styled source** whose inputs are the node's own content
programs on the tiles within `effect_reach + 4`.

- **Distance field.** happypaint flood-fills (JFA+1, rg/ba offsets to the
  nearest inside/outside pixel, `alpha >= 0.5` inside, ±½ at the edge,
  clamped at reach + 4, refined to `0.5 − alpha` within a pixel of the
  edge). onepaint computes the **exact** Euclidean distance (Felzenszwalb's
  two-pass transform) on the CPU and the same in a compute pass, so the
  oracle and the GPU agree to rounding; JFA's rare misses are not kept
  (a divergence: a few edge pixels of wide effects differ by less than a
  pixel of distance).
- **Style pass.** A port of `STYLE_SRC`: the steps in happypaint's order
  (drop shadows, outer glow, outside strokes, then pattern, gradient and
  colour overlays, satin, inner glow, inner shadows and inside strokes
  into the body, the body over, then the bevel's shadow and highlight and
  centre strokes), with `fall`, the contours, the bevel's Sobel normal and
  light, and each effect's blend over `over(stack, backdrop)`.
- **Oracle** (`compose::styles`): the pass on the CPU over a tile ± reach,
  per tile, from the node's composite; the tree compositor uses it for
  styled nodes. The CPU interpreter runs it from the content programs.
- **GPU** (`gpu::styles`, `styles.wgsl`): the content fills cache slots
  (spec 52's backdrop fill), the distance transform and the style pass run
  as compute passes over the assembled region, and the result is the
  node's filtered tile.
- **Non-local inputs.** Gradient overlays span the document, and pattern
  and bevel-texture tiles are anchored to it: they take the document
  frame, as fills do (spec 46). The backdrop for non-Normal effect modes is
  the composite below the node on the tile (the program's register before
  the node), not a whole-document copy.
- **Live strokes.** happypaint keeps the field stale mid-stroke. onepaint
  recomputes per frame on the GPU where the reach allows (it is per tile,
  not per document); the CPU path follows happypaint.

### Phase 3 in detail (GPU)
- **Styled source.** A styled node's content is a `SpatialSrc` with a
  `styled` part: the styles, the node's own **content programs** on the
  tiles of its region (tile ± `margin`), and a **backdrop program**, the
  layers below it in its container on this tile (what `eff` blends
  against). A layer's content plan is the layer alone (opacity 1, Normal,
  no styles); its mask is in it, so the source's content carries no mask
  of its own. A group's is its children, and its register starts as the
  styled content (`PushContent`, as ghost pegs). Riders, inherit alpha,
  knockout and Pass Through containers stay on the CPU.
- **CPU interpreter.** Runs the programs, quantizes them to f16 as the GPU
  cache does, and calls the oracle's style pass.
- **GPU.** The content programs and the backdrop fill cache slots (spec
  52's stages); `assemble` lays the content out; the distance field is the
  exact transform in two compute passes (one invocation per column, then
  per row, Felzenszwalb's envelope in private arrays; the region is at
  most 768² px, so reach ≤ 124 px runs on the GPU and wider stays on the
  CPU); the style pass ports `STYLE_SRC` with the effects in a storage
  buffer and writes the filtered tile.

## Engine and UI
- Commands: `SetLayerStyles { id, styles }` (one undo step; a preview
  variant while the dialog drags), `ApplyGlobalLight`, `BakeStyles`;
  merge-down bakes both layers.
- The Layer Style dialog (`ui_kit` components): the effect list with
  enable boxes and +/− for repeatable kinds, each effect's controls,
  Fill opacity, Global Light, the built-in presets (Sticker, Ink Outline,
  Soft Shadow, Neon, Embossed, Gold) and user presets in the settings.
- Formats: `.happypaint` through the model; PSD `lfx2` (and `lmfx`) both
  ways, Fill opacity as `iOpa`, Global Light in resources 1037/1049;
  `.asl` import.

## Phases
1. Model, sanitizer, contours, reach, Global Light; `.happypaint` round
   trip; happypaint's model tests.
2. The CPU oracle: exact distance field and the style pass for layers and
   groups in the tree compositor; pixel tests per effect from the formulas.
3. Planner and GPU: styled sources, the distance and style passes,
   checked against the oracle with mutations.
4. Engine commands and the dialog, presets.
5. PSD `lfx2`/`lmfx`, `.asl` import.

## Acceptance criteria
1. happypaint's `layer-styles`, `style-steps`, `style-presets`,
   `psd-effects` and `asl-file` tests pass in Rust.
2. Each effect's pixels match the formulas at hand-checked points.
3. The GPU matches the oracle within the usual tolerance.
4. A `.happypaint` file with styles opens, renders and saves back unchanged.
