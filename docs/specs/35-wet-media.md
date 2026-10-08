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
| C | `impasto {depth}` | A per-layer height field, lit as relief at composite time; bevel from alpha | **done** |
| D | `settle {edgeDarken, granulate}` | A flatten-time pass on dry watercolour strokes | **done** |
| B | `fluid {water, pigment, rewet, blow, oil}` | A live, time-stepped wet simulation | **done** (CPU) |

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

## C. Impasto (`brush::height`, `compose::relief`, `engine` height ops)
- **Field:** `Layer.height: Option<Surface>`. The height sits in every
  channel, so empty tiles prune and undo reuses the tile machinery. `None`
  means flat.
- **Pigment** (`paint-medium.ts`):
  - A paint stroke's opacity is capped at `opacity · (0.05 + 0.95·load)`.
  - Its body is `depth · load`.
  - `load` is `wet.load`, default 1, read even when `wetness` is 0.
  - The cap never applies to the eraser or to a layer mask.
- **Building relief:**
  - Each dry dab stamps `body · 0.1 · alpha`, symmetry copies included.
  - Each wet-mixer dab stamps `body · 0.14 · deposit`.
  - A stamp is an additive dome, `(1 − smoothstep(.5, 1, d)) · amount`.
  - Relief is not clipped by the selection; the alpha gate hides it outside
    the paint.
- **Blender** (`wet.blend > 0`):
  - Each dab moves the field toward its 5-tap average, less
    `blend · 0.12`, weighted by `(1 − smoothstep(.35, 1, d)) · strength`.
  - It only levels an existing field and never creates one.
  - Blend wins over impasto.
- **Eraser:** carves `h *= 1 − alpha · opacity` at stroke end.
- **Undo:** paint and relief land in one undo step. Cancel puts the field
  back. Clear layer clears it too.
- **Lighting:** the compositor shades the layer before its mask, opacity and
  blend mode, using the formula from `compositor.ts:149-165` in doc space:
  - `amt = smoothstep(.02, .25, max(hl, hr, hu, hd))`
  - `n = normalize((hl − hr)·9, (hu − hd)·9, 1)`
  - The light is `normalize(−.45, −.6, .66)`.
  - `rgb *= mix(1, .62 + .62·diff, amt)`
  - `rgb += spec²² · .3 · amt · a`
  - Taps clamp at the document edge. Transparent pixels are never shaded.
  - Export includes relief; merge-down does not.
- **Merge down:** paint merges unlit. The upper layer's height adds onto
  the lower one's when the upper layer shows; otherwise the lower field is
  kept.
- **Canvas size and image size:** the field is translated or resampled with
  the pixels.
- **File** (`height-codec.ts`):
  - `node.height` is a full-canvas PNG with `q = clamp(h/16)·65535`,
    R = high byte, G = low byte.
  - A flat field is omitted.
  - An unreadable chunk is kept verbatim.
- **Bevel & Emboss:**
  - `Command::BevelFromAlpha {depth, softness}` replaces the field with
    happypaint's chamfer distance transform, then a dome, then three box
    blurs. The blur radius is `round(min(w,h)·0.01·(1 + 6·softness))`, and
    the alpha threshold is 8/255.
  - `ClearBevel` drops the field.
  - A large shape's dome is tall (R = inscribed radius × depth, often far
    over the 16 the file stores). The ×9 normal saturates, so the lit and
    shaded halves meet in a crease along the medial axis. Same maths as
    happypaint; kept.
  - Layer panel ▸ Bevel 0–150% and Soften 0–100% apply on release (not
    saved, as in happypaint). Bevel 0 clears.
- **Not yet:**
  - Free transform, liquify, flip and filters leave the field where it was.
  - Metallic layers.
  - The dry-stamp paper tooth fill (`fill = max(alpha·.55, smoothstep(.02,
    .25, h))`) waits for paper texture.

## D. Settle (`brush::settle`)
- **When:** paint strokes with a `settle` block (`edgeDarken` or
  `granulate` > 0). It runs at stroke end on the finished stroke buffer,
  before the flatten and after the tail dabs. Erase and mask strokes never
  settle. The live preview shows the unsettled stroke, as in happypaint.
- **Rim:**
  - Taps are bilinear alpha on two 8-tap rings, at 4 px and 14 px
    (diagonals at r/√2).
  - Taps clamp to the document edge.
  - `rim = clamp(max((a − ring)/a · 2))`
  - `na = a · (1 + edge · 0.9 · rim)`
- **Soft halo:**
  - The gate is `smoothstep(.5, .15, hardness)`.
  - `r = a/mx`, where `mx` is the maximum over all taps.
  - `band = smoothstep(.95, .65, r) · smoothstep(.03, .18, r) · gate`
  - `na = max(na, mix(a, mx·(1 + .9·edge), band))`
- **Granulation:**
  - Uses the brush grain at `grain.scale` doc px, fixed to the canvas
    (no offset or rotation), repeat-sampled.
  - `na *= 1 + granulate · .55 · (2g − 1)`
- **Result:** `na` is clamped to `[.45·a, 1]`. Premultiplied RGB scales by
  `na/a`, so the hue is unchanged.

## B. Live fluid (`brush::fluid`, `engine` wet ops)
- **Routing:** a Brush-tool stroke with a `fluid` block feeds the
  simulation (PC:5683). Opacity scales `pigment` and `rewet`. Layer masks,
  alpha-locked, locked, hidden and adjustment layers take no wet paint.
- **Dabs** (`wet-dab.ts`, all ported with its tests):
  - Spacing is `max(1.5, size·0.06)` with smoothing 0.35.
  - The reservoir empties with travel:
    `0.25 + 0.75·exp(−travel/(size·20·(0.5 + water)))`. Oils always stay
    full.
  - Water, pigment and re-wet per dab follow `dabAmounts`.
  - Each batch shoves the paint along the brush:
    `min(10·(1 + 2·blow), len·0.6·(1 + 4·blow))` px. Symmetry mirrors the
    shove.
  - Live oils stamp relief `impasto·0.12·(0.4 + 0.6p)` per dab.
- **Deposit**, in order:
  1. **Drag:** from a snapshot of the batch region; the last dab wins.
  2. **Re-wet lift:** `col += layer·L·cov`, then `layer ×= 1 − L·cov`.
  3. **Water:** MAX into the water channel.
  4. **Pigment:** KM pre-mix against the paint under the batch, then
     premultiplied source-over.
- **Step** (16 ms, at most 4 per tick; a stall drops the backlog): settle
  the old state into the layer, then update the water, pigment and
  velocity, both from the old state. The constants and maths follow
  `SETTLE_RATE_GLSL` and `UPDATE_FRAG` exactly:
  - KM is folded neighbour by neighbour.
  - Advection adopts the fastest wet neighbour.
  - The slope force has a dead zone.
  - Fingering noise and vorticity confinement are included.
  - Oils use their own viscous constants.
- **Drying:**
  - `WetClock` drops the water level by 0.003 per step, then runs a
    90-step tail, then dries.
  - Oils never dry on their own.
  - Drying settles everything with the force rim and commits the whole
    session (first deposit to dry) as one undo step, relief included.
- **What dries the session:** the Dry button, any document-editing
  command (undo, layers, transform, filters…), a non-fluid stroke, the
  gradient, saving and exporting. A fluid stroke on another layer, or in
  the other medium, dries the old session first. Blow alone needs wet paint
  on the active layer.
- **Display:**
  - The canvas shows the suspended paint over the layer as a live overlay.
  - The damp veil is `(0, .08, .33)·a` with `a = 0.24·smoothstep(.02, .5,
    water)`, or 0 for oils.
  - The veil is display only and never in an export.
- **Replay:** `RecEvent::WetSteps` records step counts, and `SetWetTilt` and
  `DryWet` are commands, so a recording replays the drying exactly.
  Recordings now also snapshot height tiles.
- **Performance** (CPU, release, 12 threads; 600 px wash, about 8 ms per
  step):
  - Work is limited to 16-px blocks holding state and their neighbours.
  - Rows run in parallel.
  - KM runs only where colours differ by more than 1/512.
  - The rim-gate rings stop once the pixel is fully wet.
  - The fingering noise is cached.
  - The allocation pads only new area (happypaint re-pads the whole union,
    which compounds).
- **Paper:** the dry-brush gate, absorption, resistance and granulation
  follow the document paper (spec 36).
- **Not yet:**
  - The tilt pad UI (the command exists).
  - Fluid controls in the brush studio.
  - A GPU compute path for large washes.
