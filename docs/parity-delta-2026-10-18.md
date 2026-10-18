# Parity delta: happypaint v0.201.7 → v0.204.7

What happypaint added after `parity-delta-2026-10-10.md`, and the order
onepaint ports it in. Fold each item into `feature-parity.md` as it lands.

## Decisions taken without the owner (2026-10-18)

- **Licence.** happypaint moved from AGPL-3.0-only to PolyForm
  Noncommercial 1.0.0 in v0.204.6. onepaint stays AGPL until its owner
  decides; nothing here depends on it.
- **Git.** Commit per milestone, do not push.
- **Build.** The machine has little free memory and disk: `cargo -j 2`,
  one crate's tests at a time.
- **Blend modes first.** The v0.200.1 modes were half done in the tree, so
  they landed before the v0.202 work (aaed348).

## Compositing and layers

- [x] The Photoshop blend modes and Pass Through groups (v0.200.1).
- [x] Knockout (v0.202.2): None / Shallow / Deep. A knocked-out layer punches
      through to the group's base (Shallow) or the document's (Deep); Deep
      inside a nested group stops at that group. PSD `knko` both ways,
      Properties select. Done: the CPU oracle (`crates/compose/tests/knockout.rs`),
      `Op::Knockout`/`Op::Snapshot` on the GPU (the oracle suites draw random
      knockouts), PSD `knko`, `LayerProps::knockout`, the Properties select.
- [x] Mask density and feather, and Blend If (v0.202.0): the oracle
      (`crates/compose/tests/look.rs`), the GPU, PSD (mask parameters, blend
      ranges), `LayerProps`, Properties (Density, Feather, a folding Blend If
      section on `ui_kit::range_bar`).
      - A tile with a Blend If at or above the hot layer runs unsplit (spec
        38), as with adjustment layers: its tone steps flip on the f16
        cached backdrop.
      - Feather (`compose::feather`) is happypaint's 65-tap blur, σ = the
        feather. Each tile's feathered mask is built on the CPU from the
        mask tiles in reach and cached in the session's `PlanCache`; the GPU
        reads it as an ordinary mask tile. While the mask is being painted
        it shows unfeathered, as happypaint's live preview does. Mask edits
        redraw as far as the blur reaches. A GPU blur pass would make big
        feathers cheaper to rebuild (todo).
- [x] Vector masks (v0.202.1): `doc::vector_mask`, the per-tile rasterizer
      (`compose::vmask`, divergence 47), CPU oracle and GPU (ops are five
      vec4s now, `e.x` the coverage slot), PSD `vmsk`/`vsms`, Create Vector
      Mask (the path menu's "Mask from path" now makes a vector mask, as
      happypaint's does), Free Transform / crop / resize / scale map the
      knots (Free Transform in the same undo step, not when unlinked),
      Properties (Apply, Invert, Delete, path count) and a layer-row tag.

## PSD

- [ ] `phfl` Photo Filter colour decode (v0.204.7): v3 holds D50 XYZ
      (1.0 = 32768); v2 Lab binary colours rescaled; also fixes Gradient Map
      stops stored as Lab or grey.

## Input

- [ ] A pointer pressed outside the canvas is remembered until it lifts and
      never "healed" into a stroke (v0.204.7, `pointer-router.ts`).

## UI

- [ ] Mix colour mode (v0.203.0, v0.204.7): 2–6 wells, rows × cols grid,
      OKLab or RGB mixing, save as palette.
- [ ] Side sliders anywhere (v0.204.0): edges, floating, with the tools; a
      typed value entry from the end icon (v0.204.7).
- [ ] Align acts on the active layer when nothing is multi-selected
      (v0.204.1).
- [ ] Tooltips on tool icons (v0.202.3), Save / Save As (v0.204.2), the
      header as a title bar (v0.204.3): check against onepaint's chrome.

## Notes: vector masks (happypaint v0.202.1, read 2026-10-18)

- Model (`engine/vector-mask.ts`): `VectorMaskSpec { paths, invert?, disabled?,
  unlinked? }`, `VectorPath { open?, op: combine|subtract|intersect|exclude,
  rule: even-odd|non-zero, knots }`, knot = `[inX, inY, x, y, outX, outY]`,
  absolute doc px. Layers only; `.happypaint` key `"vectorMask"`, false flags
  omitted. Not the Pen's `PathSpec` (handles relative there).
- Raster: Canvas2D fill per path (always closed, even `open`), then combine
  with A = acc, B = path: combine `B + A(1−B)`, subtract `A(1−B)`, intersect
  `AB`, exclude `A(1−B) + B(1−A)`; acc starts full when path 0 is subtract
  or intersect; invert last. Multiplies after the raster mask's density and
  feather: `content · m · v` (density/feather never touch `v`). Used by every
  `applyMask` path: knockout, adjustments, clip bases, clipped adjustments,
  merge down; inactive when disabled or empty.
- PSD `vmsk` (= `vsms`): u32 version 3, u32 flags (1 invert, 2 notLink,
  4 disable), 26-byte records: 0/3 closed/open length (u16 count, i16 op
  index into [exclude, combine, subtract, intersect], u16 rule 2 = non-zero
  else even-odd), 1/2 closed knots, 4/5 open knots (six i32 8.24 fixed,
  y before x, × height/width), 6 fill rule, 7 clipboard, 8 initial fill.
  Write order: version, flags, 6 (zeros), 8, then length + knots per path.
  Shape layers (vector fill + mask) stay pixels; layers only.
- Engine: Pen "Create Vector Mask" masks the plain paint layer directly
  below the path layer (notices "Create Vector Mask masks the paint layer
  directly below the path — there isn’t one", "That path encloses no area");
  each subpath of ≥ 2 anchors → combine/even-odd path, absolute handles.
  No knot editing. Free Transform (affine only, not distort/warp, not when
  unlinked) maps every knot point, same undo step; crop/resize translate,
  Scale Image scales. Duplicate copies it.
- UI: Properties "Vector mask" (shown when present): Delete ("Remove the
  vector mask (the layer's pixels are untouched)"), Apply = !disabled ("Keep
  the paths but stop hiding with them (Photoshop's Shift-click)"), Invert
  ("Show outside the paths instead of inside"), "N paths". Layer row badge
  "Vector mask (Properties panel)" unless disabled.
- Tests to port: `vector-mask.test.ts` (rect ops at y=20, x = 10/25/35:
  subtract [255,0,0], intersect [0,255,0], exclude [255,0,255]; first
  subtract; scale 2; translate; conversion `[25,8,30,10,35,12]`; triangle),
  `psd-roundtrip.test.ts:288-350`, `look-carriers.test.ts:72-85`.
