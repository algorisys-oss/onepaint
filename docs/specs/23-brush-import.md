# 23: Brush import and export

The sources, relative to `../happypaint/frontend/src/`:

- `storage/brush-import/abr.ts` (Photoshop `.abr` parser)
- `storage/brush-import/abr-mapping.ts` (ABR descriptor → brush settings)
- `storage/brush-import/bplist.ts` (binary plist reader)
- `storage/brush-import/procreate.ts` (Procreate `.brush` / `.brushset`)
- `storage/brush-import/zip.ts` (zip reader and stored writer)
- `storage/happybrush.ts` (`.happybrush` encode and decode)
- `state/brush-import.ts` (the import pipeline and Procreate mapping)
- `engine/brush/tip-mask.ts`, `engine/brush/brush-grain.ts:236-254`
  (bitmap → 128² tip mask, bitmap → 256² grain)
- `state/custom-brushes.ts:139-169` (the custom library)

Crates: `formats::brushes` (parsing, mapping, the user-library files),
`brush` (the runtime registry), `apps/onepaint` (popover, persistence).

## Photoshop `.abr` (`abr.ts`)

Big-endian throughout. `version:i16` decides the layout.

**v1 / v2** (`abr.ts:163-211`): `count:i16`, then per brush
`type:i16, size:i32` and a record; the reader always jumps to the record's
end, so a malformed record is skipped with a warning.

| Type | Record |
|---|---|
| 1 computed | `misc:i32, spacing:i16, [v2: unicode name], aa:u8, diameter, roundness, angle, hardness: i16` → hardness `/100` clamped 0..1, spacing `max(0.02, /100)` |
| 2 sampled | `misc:i32, spacing:i16, [v2: name], aa:u8, 4×i16 bounds, 4×i32 bounds (top, left, bottom, right)`, then sampled data |
| other | skipped, warning `skipped unknown v{n} brush type {t}` |

**Sampled data** (`abr.ts:130-161`): `depth:i16, compressed:u8`. Width or
height ≤ 0, or more than 8192² pixels, is skipped with a warning; depth must
be 8 or 16 (16 keeps the high byte). Uncompressed is raw rows; compressed is
`height` u16 row byte counts, then one PackBits run per row
(`abr.ts:60-74`, `-128` is a no-op).

**v6–v10, subversion 1 or 2** (`abr.ts:452-493`): a sequence of
`'8BIM' key:4 len:i32 data` blocks. A lost signature resyncs by scanning
forward for `8BIM`.

- `samp` (`abr.ts:213-237`): records `len:i32` padded to 4. A 37-byte id
  field holds a Pascal-string UUID; then skip 10 bytes (subversion 1) or
  264 (subversion 2); then `4×i32` bounds and sampled data.
- `patt` (`abr.ts:375-450`): Photoshop patterns, `len:i32` padded to 4:
  `version:i32 (=1), mode:i32, h:i16, w:i16, unicode name, pascal id`,
  a 768-byte palette for indexed mode, then a virtual-memory array list
  (`version:i32 (=3), length, rect, channels:i32`, then up to
  `channels + 2` arrays of `written, len, pixelDepth, rect, depth:i16,
  compression:u8, data`). Compression 0 is raw, 1 is zlib (or raw deflate).
  Three planes become luminance `0.299r + 0.587g + 0.114b` rounded;
  one plane is used as is. At most 4096² pixels.
- `desc` (`abr.ts:245-320`): an Action Descriptor tree (optional
  `16` version prefix). Value types `Objc GlbO VlLs doub UntF TEXT enum long
  comp bool type GlbC alis tdta`; any other type fails the walk. Keys are
  trimmed (`'Nm  '` → `Nm`). onepaint holds the tree as `serde_json::Value`
  with the same shapes as happypaint (`UntF` → `{unit, value}`,
  `enum` → `{enumType, value}`, `alis`/`tdta` → `null`).

**Linking presets to tips** (`abr.ts:501-539`): the root's `Brsh` list holds
presets; those whose `Brsh` sub-object has `sampledData` are sampled
presets. A tip whose UUID equals the trimmed `sampledData` takes that
preset's descriptor and `Nm`. When some tips stay unlinked and the sampled
preset count equals the tip count, the rest link by position. If the walk
fails entirely, a flat scan for `Nm  TEXT` names (`abr.ts:327-352`) is used
only when its count equals the tip count; otherwise the warning
`brush names not recovered from desc block`.

Anything else (`version` outside 1, 2, 6–10 or a subversion other than 1 or
2) is an error: `abr: unsupported version v.s`.

### Descriptor mapping (`abr-mapping.ts:88-203`)

Applied over the import defaults (below) and the sampled size.

| ABR | onepaint preset |
|---|---|
| `Brsh.Dmtr` > 0 | `size = clamp(round(Dmtr), 1, 512)` |
| `Brsh.Spcn`, `Brsh.Intr` | `spacing = clamp(Spcn/100, 0.02, 4)`; `Intr == false` → `0.05` |
| `Brsh.Angl, Rndn, flipX, flipY` | baked into the tip bitmap once (see tip transform) |
| `useTipDynamics`: `szVr` control, `minimumDiameter` | `dynamics.size` = control binding `[min, 1]` + random jitter `[max(min, 1−j), 1]`; pressure control → `sizePressure = 1 − min` |
| `angleDynamics` | direction / initial direction → `rotation {source: angle, jitter}`; jitter only → `{source: none, jitter}` |
| `roundnessDynamics`, `minimumRoundness` | `dynamics.ratio = [random, min max(0.05, minRound, 1 − j)]` |
| `tiltScale` | `tiltStrength = min(1, tiltScale/200)` |
| `usePaintDynamics`: `opVr`, `prVr` | `dynamics.flow` (one binding per sensor, floor 0.02); a pressure binding sets `flowPressure = 1` |
| `useScatter`: `scatterDynamics`, `Cnt` | `scatter = min(2, jitter × 0.5)`; `Cnt > 1` divides spacing (floor 0.02) |
| `Wtdg` | `settle {edgeDarken 0.6, granulate 0}` (kept as a preset field) |
| `Rpt` | `airbrush {rate 15}`, `wash = false` |
| `Nose` | `grain {speckle, scale 40, strength 0.3, move 1}` |
| `dualBrush.useDualBrush` | `maskTip {id: <the dual tip's imported id>, strength 0.7}` when that tip is in the same file |
| `useTexture`, `useColorDynamics`, `useBrushPose` | reported as unmapped |

Controls in `bVTy` order: off, fade, pressure, tilt, wheel, rotation,
initial direction, direction → sensors none, fade, pressure, tilt, twist,
twist, angle, angle. A control binding with `min ≥ 1` is dropped.

**Tip transform** (`tip-mask.ts:82-128`): rotate by `−Angl` degrees,
squash the height by `Rndn/100` (floor 0.01), flip, into the rotated
bounding box. onepaint resamples bilinearly; happypaint uses Canvas2D's
high-quality filter (approximated, divergence 20).

## Procreate `.brush` / `.brushset` (`procreate.ts`, `bplist.ts`)

Both are zips (stored or deflate). Entries are grouped by folder; a group
needs `Shape.png` or `Brush.archive`, so `QuickLook/` and loose metadata
drop out. Name: the archive's `name`, else the folder name (minus
`.brush`) unless it is a UUID-like `[0-9a-f-]{32,}`, else the file stem.

**bplist00** (`bplist.ts:18-130`): 32-byte trailer (offset size, ref size,
object count, top, table offset). Types: null/bool, int (8-byte signed),
real (f32/f64), date, data, ASCII (Latin-1), UTF-16BE string, UID, array,
set, dict (string keys only). Extended lengths via a following int.
Memoised by object ref; a cycle resolves to null instead of looping.
NSKeyedArchiver: `$top.root` is a UID into `$objects`; the root's values are
dereferenced one level and `$null` reads as null.

**Parameter mapping** (`state/brush-import.ts:367-416`), all optional:

| Archive key | onepaint preset |
|---|---|
| `paintSize` | `size = clamp(round(paintSize × 250), 8, 512)` |
| `plotSpacing` > 0 | `spacing = clamp(v, 0.02, 4)` |
| `paintOpacity` | `opacity = clamp(v, 0.05, 1)` |
| `dynamicsPressureSize` | `sizePressure = clamp01` |
| `dynamicsPressureOpacity` | `flowPressure = clamp01` |
| `dynamicsJitterSize` > 0 | `dynamics.size = [random, min max(0.05, 1 − v)]` |
| `dynamicsJitterOpacity` > 0 | `dynamics.flow = [random, min max(0.05, 1 − v)]` |
| `plotJitter` > 0 | `scatter = min(2, v)` |
| `shapeScatter` > 0 | `rotation.jitter = min(1, v)` |
| `|shapeRotation|` > 0.25 | `rotation.source = angle` |
| `textureScale`, `textureDepth` (with `Grain.png`) | `grain {scale round(30 + clamp01 × 170) or 90, strength clamp(0.05, 1) or 0.35, move 0}` |

`Shape.png` becomes the tip with automatic polarity (`tip-mask.ts:161-180`):
more than 15 % transparent pixels, or a dark mean, means light = paint;
otherwise dark = paint. Coverage is `a·lum` or `a·(1 − lum)`.
`Grain.png` becomes a grain named `<brush> grain`.

## `.happybrush` (`storage/happybrush.ts`)

A stored zip:

- `manifest.json`: `{format: "happybrush", version: 1, name, settings}`,
  where `settings` is happypaint's `BrushSettings` (onepaint's
  `BrushPreset` without `id`, `name` and `category`).
- `tip.png`: optional 128² grayscale, white = coverage. Written when the
  tip is a user tip (`custom:…`); `tipId` becomes `custom:embedded`.
- `grain.png`: optional 256² grayscale. Written for a user grain
  (`customgrain:…`); `grain.id` becomes `customgrain:embedded`.

Decode: entries are looked up by base name. A missing manifest, a format
other than `happybrush` or a non-object `settings` is an error; so is a
version above 1. An empty name becomes `Imported brush`. Images are
resampled to 128² and 256² (R channel). Import (`state/brush-import.ts:573-591`):
an embedded grain gets a fresh grain id; a `customgrain:` reference with no
embedded grain is dropped; a `custom:` tip with no embedded tip falls back
to `round`. Built-in tip and grain ids pass through.

## Bitmaps

- **Tip mask** (`tip-mask.ts:20-73`): bounding box of coverage > 8,
  contain-fitted and centred into 128², rest 0. Empty → the tip is skipped
  with a warning.
- **Grain** (`brush-grain.ts:236-254`): cover-fit and centre into 256²,
  luminance.

Resampling uses a separable triangle (area-like) filter where happypaint
uses Canvas2D (divergence 20).

## Import pipeline (`state/brush-import.ts`)

Defaults for every imported brush (`state/brush-import.ts:320-334`): size
60, spacing 0.12, hardness 0.9, flow 0.9, opacity 1, sizePressure 0.85,
flowPressure 0.3, smoothing 0.15, wash false, tiltStrength 0,
pressureCurve 1, tip `round`, opaqueLinearize 0.9.

- ABR sampled tips: size `clamp(round(max(w, h)), 8, 512)`, then the
  mapping; the name is the descriptor name or `<stem> <n>`.
- ABR computed brushes: round tip, size `clamp(diameter, 8, 512)`,
  hardness, `spacing = min(1, spacing)`, name or `<stem> round <n>`.
  happypaint ignores their angle and roundness, and so does onepaint.
- ABR patterns become grains with no preset.

The file kind is chosen by extension: `.abr`, `.happybrush`, and everything
else is read as Procreate.

## User library

happypaint keeps custom brushes in localStorage plus IndexedDB tip assets
with random ids (`state/custom-brushes.ts`). onepaint:

- **Registry** (`brush::user`): one process-wide `UserLibrary`
  (presets, tips by id, grains by id). `preset(id)`, `tip_for(id)` and
  `grain_for(id)` consult it after the built-ins, so the engine's
  `Command::SetBrush` resolves imported presets with no new command.
- **Ids are content hashes** (FNV-1a 64): tips `custom:<hash>`, grains
  `customgrain:<hash>`, presets `imported-<hash>` over name, tip and
  settings. Importing the same file twice therefore replaces rather than
  duplicates (divergence 21).
- All imported presets get the category `Imported`.
- **Persistence**: `<data dir>/brushes/library.json`
  (`{version: 1, presets, grains: [{id, name}], tips: [id]}`) plus
  `tips/<hash>.png` and `grains/<hash>.png`, written atomically after each
  import or delete. Tests and scripted runs pass no folder and never touch
  it.

## App

The brush popover gets an `Imported` category chip (shown when the library
is not empty), "Import brushes…" (a file dialog for `.abr .brush .brushset
.happybrush`, with a toast saying how many brushes and grains came in), and
"Export brush…" (the current brush as `.happybrush`). A right click on an
imported brush deletes it.

## Acceptance criteria

1. `parse_abr` on the bun-built `v1.abr`, `v2.abr` and `v6.abr`
   (`scripts/brush-fixtures/make.ts`) matches happypaint's `parseAbr`
   golden: tip names, sizes, UUIDs and every coverage byte; computed
   brushes; pattern pixels; warnings.
2. `map_abr_descriptor` on the v6 chalk descriptor matches happypaint's
   `mapAbrDescriptor` golden (settings, tip transform, dual UUID, unmapped).
3. The bplist reader decodes the fixture archives to the same params as
   happypaint (including a UTF-16 name, ints, reals, bools and null); the
   writer round-trips.
4. `.brush` gives one brush with the archive name and mapped params, a
   shape tip and a grain; `.brushset` gives two brushes (`Ink Pen`, `Inky`)
   and skips the thumbnail and metadata.
5. The happypaint-made `.happybrush` decodes with its settings, embedded tip
   and grain; encode → decode round-trips onepaint presets, tips and grains.
6. The user library survives save → load in a temp dir.
7. After registering an imported library, `Command::SetBrush` selects the
   imported preset and a stroke paints with its image tip (coverage differs
   from the round tip).
8. A kittest drives the popover: the `Imported` chip lists the imported
   brush and clicking it selects it.
