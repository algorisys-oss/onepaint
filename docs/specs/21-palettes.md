# 21: Palettes (slice 14)

The sources, relative to `../happypaint/frontend/src/`:

- `storage/palette-io.ts`
- `engine/color/median-cut.ts`
- `state/color-store.ts:60-345`

## Model (app)
- Palettes are `{id, name, colors: [#rrggbb]}`, and one is active.
- They are persisted as `{palettes, activeId}` JSON in the app config
  folder (`palettes.json`). happypaint uses localStorage for this.
- **First run:** "My palette" with the 12 `DEFAULT_SWATCHES`.

**Operations:**
- create (auto name "Palette N", the first free one), rename, duplicate
  ("<name> copy", inserted after the source and made active), delete;
- deleting the last palette makes a fresh default;
- add a swatch (no duplicates within a palette), remove a swatch, move a
  swatch.

## Import (`formats::palette`), lenient, returning hex
**Procreate `.swatches`:**
- A zip holding `Swatches.json`: one palette object, or an array of them.
- Each palette has a `name` and up to 30 slots, either null or
  `{hue, saturation, brightness}` with all three in 0..1.
- Slots become hex via HSV. Palettes left with no colours are dropped.
- Names: the palette's own name, or the file's base name (with ` 2`, ` 3`…
  for later palettes).

**Photoshop `.aco`:**
- Big-endian version 1 or 2, with a u16 count.
- Each entry is 5 u16: space, w, x, y, z. Version 2 adds `0, nameLen` and a
  UTF-16 name.
- Colour spaces:

| Space | Mapping |
|---|---|
| 0 (RGB) | `/257` |
| 1 (HSB) | hue `/65535·360` |
| 2 (CMYK, 0 = full ink) | `cmyk(1−w/65535, …)` |
| 8 (grey, 0..10000) | `(1 − w/10000)·255` |
| others | skipped |

**Adobe `.ase`:**
- An `ASEF` header, then u32 block count at offset 8.
- Blocks are `type u16, len u32`. Type 1 is a colour: u16 name length, a
  UTF-16 name, a 4-byte model, then f32 channels.
- Models RGB, CMYK and Gray are supported; LAB is skipped.

**GIMP `.gpl`:**
- Requires the `GIMP Palette` header. `Name:` sets the name.
- Lines of `r g b` (≤ 255) are colours. Comments and `Columns:` are
  ignored.

**Errors:** no supported colours is an error with a readable message, and
so is an unknown extension.

**Conversions:**
- CMYK→RGB: `255·(1−c)(1−k)`.
- `rgbBytesToHex` rounds to the nearest byte.

## Export
`.swatches` is a stored zip with `Swatches.json` =
`[{name, swatches: [{hue, saturation, brightness, alpha: 1, colorSpace: 0} × ≤30, null-padded to 30]}]`.

## Median cut (`color::median_cut`)
Procreate's "New palette from photo":

1. Use only opaque pixels (`a ≥ 128`).
2. Repeatedly split the box with the largest `count × widest-range`, on its
   widest channel.
3. Split at the value boundary nearest the median.
4. Stop at `max` boxes, or when every box is a single colour.
5. Sort by size, largest first. Average each box, rounded, and drop
   duplicate hex values.

In the app, the source is a ≤100×100 downscale of the flattened canvas or of
an image file.

## Acceptance
- `.gpl`, `.aco` (versions 1 and 2, RGB/HSB/CMYK/grey, Lab skipped) and
  `.ase` (RGB/CMYK/Gray, groups flattened) parse from hand-built bytes.
- A `.swatches` export→import round trip keeps the colours (8-bit HSV
  quantisation, within 1/255), and null slots are skipped.
- A 400 red + 100 blue image gives `[red, blue]`. Transparent input gives
  `[]`. `max` is respected, and the order is by size.
- The palette store persists and reloads. Delete-last makes a default. Add
  skips duplicates.
