# 35: Wet and oil media

The sources are under `../happypaint/`:
- the notes `docs/wet-engine-spec.md` and `docs/oil-paint-medium-spec.md`
- `docs/graphics/02-brush-engine.md` (wet sections) and `01-rendering-and-compositing.md:311-355` (relief, bevel)
- the code in `frontend/src/engine/wet/`, `brush/smudge-renderer.ts` and `paint-engine.ts:2840-3460`

happypaint has four media systems. Only two of them share any state (the
height field):

| | Preset field | What it is | Status |
|---|---|---|---|
| A | `wet {wetness, load, mix, pigment, tip, blend}` | Stroke-synchronous mixer: pickup, reservoir, contamination | **done** |
| C | `impasto {depth}` | A per-layer height field, lit as relief at composite time; bevel from alpha | next |
| D | `settle {edgeDarken, granulate}` | A flatten-time pass on dry watercolour strokes | after C |
| B | `fluid {water, pigment, rewet, blow, oil}` | A live, time-stepped wet simulation | last |

## A. Wet presets (`brush::WetMedia`, `engine` stroke routing)
- **Accessor:** `BrushPreset::wet()` reads the `wet` block, which stays
  verbatim in `extra`. It returns `None` unless `wetness > 0`. Missing keys
  default to `{wetness 0, load 1, mix 0.5}`.
- **Routing:** a Brush-tool stroke with a wet preset runs on the mixer path
  (`brush::smudge`, spec 16), as `paint-canvas.tsx:5706-5758` does.
- **Dab and pressure:**
  - The disc is a classic 0.35-hardness disc every 0.25 × size.
  - The brush tip shapes the dab only when `wet.tip` is set.
  - Strength is the brush opacity.
  - Size and flow pressure come from the brush, after the force-pressure
    settings (spec 34).
- **Undo:** one Tiles change per stroke.
- **Blend mode:** the wet path composites source-over and ignores the
  preset's `blendMode`, as in happypaint (`beginSmudge` takes none).
- **Layer masks:** a wet brush painting a layer mask uses the dry stamp,
  because mask paint is luminance coverage, not pigment.
- **Not yet:** the brush-studio preview still draws wet presets dry.
