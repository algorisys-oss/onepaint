# 57: Procedural papers

## Status
Built (2026-10-19): the generator (`brush::paper_field`), the
`paper-<kind>` grains with their looks, papers sized in millimetres
through the document DPI, the mottle and flecks in the visible tooth on
the CPU and the GPU, the presets with legacy ids, Apply paper colour to
Background (`Command::FillLayerSolid`, `Session::background_layer_id`),
and new canvases on the paper's sheet colour. Divergence 52. Not ported:
happypaint's rendered paper swatches in the picker (chips with hints
instead).

## Source
happypaint v0.197.0 (procedural papers) and v0.198.2 (mottle and
flecks); its spec is `../happypaint/docs/procedural-papers-spec.md`.
- `engine/brush/paper-field.ts`: the generator, ported from msurguy's
  Watercolor Playground `paperFieldFS` (MIT).
- `engine/brush/brush-grain.ts:211-262`: `PAPER_GRAINS` (tint, fleck
  colour, mottle), `fleckRatio`, `paperLook`.
- `utils/paper-presets.ts`: the presets, legacy ids, `pickablePapers`,
  `paperPresetId`.
- `engine/paint-engine.ts:3330-3362`: mm to px (`docDpi * unitMm / 25.4`).
- `engine/compositor.ts:257-285`: the visible tooth with mottle and fleck.
- `components/document-properties/document-properties.tsx:189-282`,
  `paint-engine.ts:4480-4523`: Apply paper colour to Background
  (`fillLayerSolid` on the bottom plain paint layer).
- `components/new-document/new-document-dialog.tsx:201-211`: a new
  document's background takes the paper's tint until the user picks one.

## Model
- `Paper` is unchanged (grain id, scale, strength). A procedural paper is
  the grain `paper-<kind>`: its tile is 560² units of 0.18 mm, so its doc
  px per tile is `560 · dpi · 0.18 / 25.4 · scale` (≈1191 px at 300 dpi).
  Classic grains stay sized in doc px.
- Each procedural grain has a look: an RG tile (mottle, fleck; 0.5 is
  neutral), how strongly it mottles, and its fleck colour as a ratio of
  its sheet colour.
- Presets: Smooth, the ten papers (Cold Press, Hot Press, Rough, Khadi,
  Washi, Laid, Canvas, Toned Sand, Kraft, Student) with their tints, and
  Linen. The classic papers (Rough, Fine grain and Canvas, now
  `rough-classic`, `fine-classic`, `canvas-classic`) still load and show
  in the picker only while the document is on one. Stored grain ids
  don't change; `rough` maps to `rough-classic`.

## Generator
Ten height recipes and their looks, on per-band lattice tables (f32, as
happypaint's `Float32Array`s; the rest is f64), each seamless. Tiles are
generated once per grain and cached.

## Where the paper shows
- The dry brush and the wet sim read the height as before, at the
  physical tile size.
- The visible tooth multiplies the composite by the lit shade, then by
  `1 + (mottle − 0.5) · mottle_amount · 2 · strength` and the fleck term
  (`mix(1, fleck colour, −fl)` below 0.5, `1 + 0.12 · fl` above), kept
  within alpha. The GPU paper texture is RGBA32F: height, mottle, fleck.

## Divergences
- Physical papers are sampled bilinearly without mipmaps (happypaint uses
  trilinear mipmaps); the tile is magnified at 150 dpi and above, so this
  only shows on low-DPI documents.

## Acceptance criteria
1. happypaint's `paper-field` tests pass, and the tiles match its bytes
   (FNV-1a per kind; one washi look texel is off by one from a libm
   difference).
2. `paper-presets` tests: the pickable list, legacy ids, every preset's
   grain resolves and is physical exactly when its id starts `paper-`.
3. The GPU tooth matches the CPU oracle with procedural papers, with
   mutations to the mottle and fleck caught.
4. Apply paper colour fills the Background as one undo step, and refuses
   when the bottom layer isn't a plain paint layer.
