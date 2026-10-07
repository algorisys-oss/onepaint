# 01: Raster tiles and color

## raster
- `TILE = 256`. The tile grid is anchored at document (0,0). Tile coordinates
  are `(tx, ty) = (floor(x/256), floor(y/256))`, and they may be negative,
  because the surface is unbounded.
- `Tile`: 256·256·4 `f16` values, premultiplied RGBA, row-major, with row 0
  at the top. Fully transparent tiles are never stored: a missing tile
  means transparent.
- `Surface`: a `HashMap<(i32,i32), Arc<Tile>>`.
  - `get(x,y) -> [f32;4]`
  - `tile(tx,ty) -> Option<&Arc<Tile>>`
  - `tile_mut(tx,ty) -> &mut Tile`, which creates the tile if needed and is
    copy-on-write through `Arc::make_mut`
  - `remove_if_empty(tx,ty)`
  - `tiles()` iterates the tiles, and `snapshot()` returns a cheap clone
  - `read_rect(x,y,w,h) -> Vec<f32>` and `write_rect(...)` for import and
    export
- `Rect` (integer) and `RectF`, with `union`, `intersect` and `tiles_covering`.
- Key packing for files: `key = ((tx & 0xffff) << 16) | (ty & 0xffff)`
  (`engine/history/tile-grid.ts:22-25`). Decode with `tx = key >>> 16` and
  `ty = key & 0xffff`.

## color
- The exact sRGB curves (`engine/color/color.ts:14-21, 33-44`):
  - `srgb_to_linear(c) = c <= 0.04045 ? c/12.92 : ((c+0.055)/1.055)^2.4`
  - `linear_to_srgb(c) = max(c,0); c <= 0.0031308 ? 12.92c : 1.055 c^(1/2.4) - 0.055`
- `premultiply` / `unpremultiply`. Unpremultiplying when a = 0 gives rgb 0.
- `Rgba` (straight, 0..1 sRGB) is the UI color type. Conversion to a
  premultiplied linear pixel goes through `to_linear_premul(a)`.
- Hex parsing and formatting (`#rrggbb`).
- HSV ↔ RGB, for the color picker.

## Acceptance tests
- Tile key pack and unpack round-trip for tx and ty in 0..1000.
- Writing a pixel then reading it back round-trips within f16 precision
  (relative 1e-3).
- A snapshot taken before a write is unchanged after the write
  (copy-on-write).
- An empty tile is removed by `remove_if_empty`.
- `srgb_to_linear(linear_to_srgb(x))` round-trips within 1e-6 for
  x in [0,1]. At the known points: 0 → 0, 1 → 1, and
  `srgb_to_linear(0.5) = 0.21404114`.
- `unpremultiply(premultiply(c))` round-trips when a > 0.
