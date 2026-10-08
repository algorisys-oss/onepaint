# 27: Brush studio

The sources, relative to `../happypaint/frontend/src/`:

- `components/toolbar/toolbar.tsx:499-1442` (`BrushControls`, every section
  and control), `:403-422` (`BrushPreviewView`), `:425-491` (`BrushTryView`),
  `:1567-1620` (the panel body and its Maximize button), `:3051-3101` (the
  studio dialog), `:1680-1685` (Esc)
- `components/brush-panel/brush-preview.ts` (the sample stroke)
- `components/brush-panel/brush-try.ts` (the try-it pad)
- `components/brush-panel/curve-editor.tsx` (pressure curve editor)
- `state/brush-store.ts:283-420` (adopting a preset), `:789-793`
  (`resetBrush`), `:809-834` (curve display fallbacks), `:861-1040`
  (`effectiveBrush`: how the sliders fold into a `BrushSettings`)
- `state/custom-brushes.ts:94-131, 171-184` (custom brushes: id scheme,
  add, patch, remove)
- `components/brush-panel/brush-library.tsx:156-160, 426-444` ("Save
  current" / new brush name)
- `styles/global.css:1847-2045` (studio layout)
- Tests: `state/brush-tip-form.test.ts`, `state/brush-stabilizer-bar.test.ts`

Crates: `brush` (`studio` module: the parameter table, the preview stroke,
custom-brush helpers in `user`), `engine` (`Command::SetBrushPreset`),
`apps/onepaint` (`brush_studio.rs`, the button in the brush popover).

## Opening and closing

happypaint opens a compact "Brush" panel from the toolbar; its Maximize
button (`toolbar.tsx:1593-1600`, "Maximize — try the brush") opens the
studio: a modal dialog `min(1100px, 94vw) × min(820px, 94vh)` over a
`rgba(0,0,0,.55)` backdrop (`global.css:1847-1880`). Header "Brush studio"
with Restore and Close. Left: the try-it pad filling the stage. Right: a
340 px aside with the preview and "Brushes & presets…" fixed at the top and
the controls scrolling below. Esc and a backdrop click un-maximize
(`toolbar.tsx:3053, 1680-1682`). Below 720 px wide the body stacks.

onepaint: the brush library popover gets a **Brush studio…** button that
opens the same dialog (`Popover::BrushStudio`). Esc and Close close it; a
backdrop click does nothing (the studio is the only editor, there is no
compact panel to restore to). The compact popover keeps Size, Opacity and
Flow.

## Editing model

happypaint never mutates a preset: the store holds flat override fields and
`effectiveBrush()` folds them onto the base preset (`brush-store.ts:861-1040`).
onepaint holds the folded result directly: the engine's current
`BrushPreset` *is* the edited brush. Every control edits a clone and sends
`Command::SetBrushPreset { preset }`, which replaces the current brush (an
eraser preset goes to the eraser tool, as `SetBrush` does). It is not an
undoable action, and it serialises like every command, so recordings
replay studio edits. Strokes snapshot the brush at stroke start.

**Reset** (`resetBrush`, `:789-793`) re-applies the stored version of the
current id: the built-in preset, or the saved custom brush. In onepaint this
is `SetBrush { preset_id }`, since `preset()` resolves both.

The header shows **Edited** when the current brush differs from its stored
version.

## Custom brushes

happypaint has "Save current" in the library (`brush-library.tsx:156-160`):
`addCustomBrush(name || 'My brush', effectiveBrush())`, the name trimmed and
`'Brush'` when blank (`custom-brushes.ts:98-104`), id
`b_<time36>_<rand36>` (`:94-96`), then the new brush is selected. Delete has
no confirmation (`brush-library.tsx:419-426`). There is no rename and no
overwrite.

onepaint:

- **Save as new** duplicates the current (edited) brush into the user
  library with name `<name> copy` (editable before saving), category
  `My brushes` (`CUSTOM_CATEGORY`; an eraser keeps `Eraser` so it lists with
  the erasers), and id `b_<n>` where `n` is the first free number (domain
  crates may not read the clock). The new brush becomes current.
- **Save** writes the edits back over a custom brush (onepaint addition;
  built-ins are read-only, so Save is only offered for custom brushes).
- **Rename** edits a custom brush's name in place.
- **Delete** stays in the library popover (spec 23).

All three persist through `BrushLibrary` into `library.json` (spec 23).
Tips that a copy shares with an imported brush stay referenced.

## Sections and controls

Stored value = slider / 100 unless noted; "%" displays `round(v·100)%`.
Ranges from `toolbar.tsx`; clamps from `brush-store.ts`.

**General** (`:584-747`)

| Label | Field | Range |
|---|---|---|
| Size | `size` | 1..1000 px |
| Opacity | `opacity` | 1..100 % |
| Blend | `blendMode` (in `extra`) | normal, multiply, … , pigment |
| Tip | `tipId` | round, soft, square, the image tips, user tips |
| Angle | `tipAngleDeg` | 0..360°, omitted at 0 |
| Roundness | `tipRoundness` | 5..100 %, omitted at 100 |
| Wash | `wash` | toggle |

**Stroke** (`:749-846`)

| Label | Field | Range |
|---|---|---|
| Spacing | `spacing` | 2..100 % (clamp 0.02..2) |
| Smooth | `smoothing` | 0..100 % |
| Stabilizer | `stabilizer.enabled` | toggle; Amount 0..100 % when on |
| Even build | `opaqueLinearize` | 0..100 % (default 0.9) |
| Airbrush | `airbrush.rate` | 0..60 dabs/s; 0 = Off (field omitted) |
| Taper in / out | `taper.entry` / `exit` | 0..200 px; omitted when both 0 |

**Shape** (`:848-951`)

| Label | Field | Range |
|---|---|---|
| Hardness | `hardness` | 0..100 % |
| Flow | `flow` | 1..100 % |
| Tilt | `tiltStrength` | 0..100 % |
| Pressure | `pressureCurve` | firmness 0..100: `firm = round(50 + 50·ln c / ln 3)`, `c = 3^((f−50)/50)`, c in 1/3..3; shows Soft (c < 0.92), Firm (c > 1.08) or Lin (`:379-381`) |
| Squash | `dynamics.ratio` = speed `[(0,1),(1,1−v)]` | 0..100 % |
| Spin | `dynamics.spin` = speed `[(0,0),(1,v)]` | 0..100 % |
| Speed→Size | `speedSize` | 0..100 % |
| Speed→Opacity | `speedFlow` | 0..100 % |

Squash and Spin read back as the slider value only when the binding has that
synthesised shape; otherwise they show 0 and leave the preset's own binding
alone until moved (`brush-store.ts:950-958`).

**Pressure** (`:953-1034`)

- Size·P (`sizePressure`) and Flow·P (`flowPressure`), 0..100 %.
- Three curve editors, Size, Flow and Hardness ← Pressure. Each shows the
  preset's pressure binding, or the defaults `[(0,.15),(1,1)]`,
  `[(0,.3),(1,1)]`, `[(0,1),(1,1)]` (`brush-store.ts:10-23, 809-834`).
  Editing one replaces that channel's pressure binding (other sensors on the
  channel are kept). Smooth / Linear sets `interp`.

**Jitter** (`:1036-1167`)

| Label | Field | Range |
|---|---|---|
| Scatter | `dynamics.scatter` | 0..200 % |
| Size·J | random binding on `dynamics.size`, `min = 1 − v` | 0..100 % |
| Angle·J | `dynamics.rotation.jitter` (source kept, `none` when new) | 0..100 % |
| Hue·J, Sat·J, Val·J | `dynamics.color.hue/sat/val` | 0..100 % |
| Press→Dark | `dynamics.color.pressureDarken` | 0..100 % |

Colour dynamics are dropped when all four are 0 (`:883-900`).

**Grain** (`:1273-1383`)

None plus the built-in and user grains. With a grain: Amount (`strength`,
%), Scale (16..400 px, clamp 16..600), Move (%), Random per stroke (toggle,
`random` 1 or omitted). Defaults when picking a grain: scale 160,
strength 0.5, move 0.

**Search** (`:535-559, 1436-1440`): a field at the top hides every section
whose title and labels don't contain the query, and shows
`No settings match "q".` when nothing does.

## Preview stroke (`brush-preview.ts`)

A `w × h` canvas (happypaint 232 × 72) on white, redrawn on every edit in
the current colour. With `n = max(24, round(w/2))` and `f = i/n`:

- `x = 0.08w + f·0.84w`, `y = 0.5h + sin(2πf)·0.26h`
- `pressure = max(0.04, sin(πf))`, `tiltX = 0.45`, `tiltY = 0`, `t = 12i` ms

Brushes wider than `0.85h` are scaled by `k = 0.85h / size`, and taper and
grain scale by the same `k` (`forPreview`). The stroke is rendered into a
transparent buffer and composited over white with the stroke opacity.

onepaint: `studio::render_preview` rasterises with `StrokeEngine` on the CPU
into a `Surface`; the app converts it to an egui texture and re-renders only
when the brush or colour changes. happypaint's y axis points up; ours points
down, so the S-curve is mirrored.

## Try-it pad (`brush-try.ts`)

Fills the stage, white, radius 8. Pointer down (left button) begins a
stroke with the current brush and colour; moves add points; up ends it.
Mouse pressure is 0.5. Strokes accumulate until **Clear** (top right). A
hint pill "Draw here to try the brush" sits at the bottom centre. The brush
is snapshotted at stroke start.

onepaint renders each stroke into a stroke buffer, composites it onto the
pad at the brush opacity on stroke end, and shows pad + live stroke over
white. Resizing the dialog clears the pad.

## Not ported (see `divergences.md`)

Tip colour (`useTipColor`, tip hue/sat/bright), Fill shape, brush-size
scaling and the global force-pressure toggles, Space·J, Hue→Shift, Tip col,
the Wet, Fluid and Dual brush sections, and Paper: onepaint's stroke engine
does not implement them yet. Their preset values are preserved untouched in
`extra` or the typed fields.

## Tests

- `crates/engine/tests/brush_studio.rs`: `SetBrushPreset` changes strokes,
  routes erasers, is not undoable and round-trips through serde.
- `crates/brush/src/studio.rs`: the preview path matches the formula; a
  parameter edit (size, hardness, grain) changes the rendered preview; every
  parameter's get/set round-trips and clamps; Squash/Spin/Size·J/Angle·J
  build the bindings `effectiveBrush` builds; the firmness mapping; the
  `brush-tip-form` and `brush-stabilizer-bar` behaviours (roundness clamps to
  0.05, angle wraps, NaN ignored, round tips omit the fields, stabilizer
  amount 0 turns it off and keeps the strength).
- `crates/brush/src/user.rs`: duplicate names and ids, rename, overwrite.
- `crates/formats/src/brushes/library.rs`: a saved custom brush reloads
  from a temp dir.
- `apps/onepaint/src/brush_studio.rs`: a kittest opens the studio from the
  brush popover, moves a slider and checks the current brush changed; saves
  a copy and resets.
