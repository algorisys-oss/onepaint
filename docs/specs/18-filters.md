# 18: The remaining filters, generators and fill layers

This spec extends [10](10-adjustments.md) to all 48 happypaint kinds. The
source is `engine/adjustments/adjustment-renderer.ts` (AR), plus
`selective-color.ts` (SC), `lightning.ts` (LG), `utils/patterns.ts` (PAT) and
`engine/paint-engine.ts` (PE). The CPU f32 code in `crates/filters` is the
reference. The future wgpu passes are tested against it.

happypaint has no pixel goldens for its shaders. Its CPU-side goldens are
exported with bun into `crates/filters/tests/golden/` and pinned by the tests:

- `selective_color.json`: `selectiveColorRgb`, 3 stacks × 36 pixels.
- `patterns.json`: `patternPixels` bytes for all 8 built-ins.
- `lightning.json`: the polylines `renderLightning` strokes, for 2 seeds.

Everything else is hand-computed from the formulas below.

## Conventions (every kind)

- **Coordinates.** `v_uv` is y-down, like the document. Pixel `(x, y)` has
  its centre at doc px `p = (x+.5, y+.5)`, and `uv = p / size`, where `size`
  is the doc size. `gl_FragCoord.xy = p` too.
- **Sampling.** `texture(src, uv)` is bilinear at texel centres with
  clamp-to-edge: `bilinear(uv·size − .5)`. In the CPU code this is `Tex::uv`.
  A texture of a different size (the half-resolution levels) is sampled the
  same way with its own size.
- **Work space.** onepaint documents are float/linear (spec 10). So
  `toWork = lin2srgb` and `fromWork = srgb2lin`. A shader that skips
  `toWork` (Bloom's bright-pass, Line Art's luma, the Bokeh highlight weight)
  is ported as written: it reads linear values.
- **Selection.** `out = mix(src, effect, keep)`. Generators multiply by
  `keep` instead, because their source is empty.
- **Hashes.** These are ported bit for bit in f32: the `sin`-based ones
  (`rand`, `laneHash`, tone's sand), the `fract`-based ones (`hash`, `hash2`)
  and `ign` (interleaved gradient noise). GPU `sin` precision differs from
  CPU `sin` for large arguments. That is within the expected GPU/CPU
  tolerance for the hash-driven kinds (glitch, wind, sand), so their tests
  check structure, not exact values.
- **`Frame`.** A pass needs to know where its buffer sits in the document:
  its origin, the doc size and the DPI. `apply_spatial(kind, p, buf, w, h)`
  is the whole-document case. `apply_spatial_in(.., &Frame)` covers a region.
  `prepare_in(kind, p, &Frame)` gives per-pixel kinds the doc size they need.
- **Reach.** This is how far a spatial kind reads, so the engine can run it
  over the content plus a margin. `GLOBAL_REACH` means the result depends on
  the whole canvas (a centre, the doc size, wrap-around). The engine then runs
  it over the whole document.

## Classes

| Class | Kinds | How the engine runs them |
|---|---|---|
| colour (per pixel) | spec 10's colour kinds, **color-to-alpha, replace-color, selective-color, tone** | per tile; also live adjustment layers |
| fill (per pixel; ignores the source) | **gradient-fill, solid-fill, pattern-fill** | adjustment/fill layers only, as in happypaint (not in the destructive menu, AR:93) |
| spatial | blur, distort, stylize, light-rays | over content + `reach` |
| generator (new layer) | **clouds, fog, smoke, nebula, starfield, lens-flare, lightning, line-art** | `NEW_LAYER_KINDS` (PE:8125): rendered into a new layer above the active one; lens-flare's layer is Screen |

`Kind::is_color()` covers colour + fill. `Kind::is_generator()` covers the
new-layer set. `Kind::is_pixelwise()` is true when `Prepared::color_px` can
produce the pixel. That covers colour + fill + clouds, fog, smoke, nebula,
starfield and lens-flare, so those generators render per tile too.
`Kind::section()` gives happypaint's menu section (`state/adjustments-store.ts`
`ADJUSTMENT_SECTIONS`).

## Colour kinds

**color-to-alpha** (AR:784). Defaults: `{target [1,1,1], threshold 0}`.
Never the identity.

1. `rgb = toWork(unpremul(s))`. This is not clamped.
2. For each channel:
   `d_c = rgb_c > t_c ? (rgb_c−t_c)/max(1−t_c, 1e-4) : (t_c−rgb_c)/max(t_c, 1e-4)`.
3. `α = clamp(max d)`, then `α = clamp((α−thr)/max(1−thr, 1e-4))`.
4. `out = α>1e-4 ? clamp(t + (rgb−t)/α) : rgb`.
5. The output alpha is `a·α`.

Tests:
- The target colour becomes alpha 0.
- 50 % grey over white target becomes black at α .5.
- Pure black is untouched.

**replace-color** (AR:808). Defaults: `{target [1,1,1], replace [.2,.45,1],
threshold .3}`. Never the identity.

1. `rgb = clamp(toWork(...))`.
2. `dist = |rgb − clamp(t)| / 1.732`.
3. `w = 1 − smoothstep(thr, thr+.12, dist)`.
4. `recol = hsv2rgb(h_rep, s_rep, v_px)`, using the GLSL `rgb2hsv`.
5. `o = mix(rgb, recol, w)`.

Tests:
- An exact target takes the new hue and saturation at its own value.
- A far colour is unchanged.

**selective-color** (SC, AR:840). Defaults: one sample
`{target [1,0,0], reach .25, falloff .15, dh 0, ds 0, dv 0}`,
`showRange false`.

- The identity is `!showRange && no enabled sample with a non-zero delta`.
- Disabled samples are dropped, and only the first 8 are kept.
- Targets go in as HSV (hue in turns), from the CPU `rgb01ToHsv`. Pixels use
  the GLSL `rgb2hsv`.
- Metric: `dh = 1·min(s,s_t)·hueDist`, `ds = .7·Δs`, `dv = .45·Δv`.
  `dist = |(dh,ds,dv)| / √(1+.49+.2025)`.
- Weight: `w = 1` when `dist ≤ reach`, `0` when `dist ≥ reach+falloff`, and
  `1−smoothstep` in between.
- The deltas accumulate weighted, scaled by `1/Σw` when `Σw > 1`. Then
  `h = fract(h+dh)`, and `s` and `v` are multiplied by `1+d`, clamped.
- `showRange`:
  `grey = luma·.55`, `o = mix(grey·.5, mix(grey, (1,.15,.15), .75), clamp(Σw))`.

Tests: the bun goldens (`selective_color.json`) within 1e-4.

**tone** (AR:1061, uniforms AR:2079). Defaults: `{lpi 60, angle 15,
toneShape dot, toneDensity flat, toneLevel .5, hardDots false}`. The ink is
`target ?? black`.

- Cell: `cell = max(1, dpi / max(5, lpi))`, where `dpi` is the document's.
  Rotate into tone space with `rp = R(angle)·p`, then `d = rp − centre(cell)`.
- Level:
  - flat: `toneLevel`.
  - gradient: `mix(level, levelEnd, clamp(dot(p,(sin g, cos g)) / dot(|dir|, size)))`.
  - luminance: `(1 − luma(clamp(toWork))) · a`.
- Signed distance:
  - dot: `|d| − L·cell·.72`
  - line: `|d.y| − L·cell·.5`
  - cross: `min(|d.x|,|d.y|) − L·cell·.35`
  - sand: `rand(floor(rp / max(cell/4, 1))) − L`
- Coverage: hard dots give `dist ≤ 0`. Otherwise `1 − smoothstep(−w, w, dist)`.
  On the CPU, `w = fwidth(dist)` is estimated as
  `|dist(p+x̂) − dist(p)| + |dist(p+ŷ) − dist(p)|`, with the level held
  fixed. Then `cov *= step(.0005, L)`.
- Output: `(fromWork(ink)·cov, cov)`. It replaces the source. There is no
  view flattening, because the filters crate is always "real output"
  (`viewScale 0`).
- Identity: flat with `level ≤ 0`; gradient with both ends `≤ 0`.
  Luminance is never the identity.

Tests:
- Level 0 lays nothing.
- Hard dots are binary.
- The mean coverage over a cell tracks `π(.72·L)²` for small `L`.
- The ink colour is honoured.

## Fill kinds (layers only)

**solid-fill** (AR:1670). Default `{fillColor [0,0,0]}`.
Output: `(srgb2lin(c), 1)`.

**gradient-fill** (AR:1650). Defaults:

- `gradShape linear`;
- `fillStops [{0, black, 1}, {1, white, 1}]`;
- `A (.5,0)`, `B (.5,1)` as doc fractions;
- `gradReverse false`.

`t = shape_t(shape, p, A·size, B·size)` (spec 12). Then
`c = Ramp(stops).sample(t)` (linearised stops, straight alpha), and the output
is `(c.rgb·c.a, c.a)`. Reverse uses `reverse_stops`.

**pattern-fill** (AR:1685, PAT). Defaults: `{patternId checker, A black,
B white, scale 1, angle 0, offset (.5,.5)}`.

- The tile is `patternPixels(id, A, B)`: 32², with 4×4 supersampled coverage,
  as bytes. The lookup uses `d = p − off·size`,
  `r = (cos·dx − sin·dy, sin·dx + cos·dy)` and `uv = r / (32·max(.05, scale))`.
- The sample is bilinear with **repeat** wrap. Then `srgb2lin`, and alpha is 1.
- Imported `patternImage` tiles are not decoded (see divergences). The fill
  falls back to the built-in.

Tests:
- The tile bytes match `patterns.json`.
- Solid gives the linearised colour.
- The gradient's ends hit the stop colours.
- The pattern repeats with period `32·scale`.

## Blur

These are all spatial. They take premultiplied RGBA, with no work-space
conversion except where noted. `blur(src, dir, R, out)` is spec 10's 65-tap
pass, generalised to any output size.

**radial-blur** (AR:1175). Defaults: `{radialMode zoom, amount .3,
centre (.5,.5)}`. The identity is `amount == 0`. Reach: global.

- 96 taps at `t = (i + ign(p)) / 96`.
- Zoom: `c + d·(1 − t·A·.5)`.
- Spin: rotate `d` by `(t−.5)·A·π/3`.
- Average the taps.

Test: a pixel at the centre is unchanged in zoom mode.

**tilt-shift, iris-blur, field-blur** (AR:1448, AR:2411).

- Levels `L1..3` are blurs at `R/4, R/2, R` (separable, **half resolution**
  `ceil(size/2)`). `R` is `radius`, or `max pin blur` for field.
- With `bokeh > 0`, each level is instead a pre-blur at `r/8`, then a 64-tap
  golden-angle disc with weight `1 + bokeh·12·smoothstep(range, 1, luma)`
  (AR:1503).
- Amount `r`:
  - tilt: `smoothstep(f, f + max(fo, .5), |d·n|)`, where `n = (sin a, cos a)`.
  - iris: `smoothstep(inner, 1, |(d·u/rx, d·n/ry)|)`, where `u = (cos a, −sin a)`.
  - field: `Σ w·blur / Σ w / R`, with `w = 1/(|q|²+1)`.
- Output: `r < .25 ? mix(s, L1, 4r) : r < .5 ? mix(L1, L2, 4(r−.25)) : mix(L2, L3, 2(r−.5))`.
- Defaults (AR:416-424):
  - tilt `{c (.5,.5), angle 0, focusSize .1, falloffSize .2, radius 20}`;
  - iris `{c, extent .6, aspect .7, angle 0, focus .5, radius 20}`;
  - field `{pins [(.5,.75,0), (.5,.2,20)]}`.
- Identity: `radius ≤ 0`, or `max pin ≤ 0`. Reach: global.

Tests:
- Tilt-shift leaves the focus band sharp (exact) and blurs far from it.
- Field blur is sharp at a 0 pin.

**spin-blur** (AR:1535). Defaults: `{c, extent .4, aspect 1, angle 0,
focus .8, spin 30}`. The identity is `spin ≤ 0`. Reach: global.

- `q = (d·u/rx, d·n/ry)` and `w = 1 − smoothstep(inner, 1, |q|)`.
- 64 taps rotate `q` by `((i+ign)/64 − .5)·spin·w`, mapped back through the
  ellipse.

Test: a pixel outside the ellipse is exact.

## Distort

These all have global reach, except ripple, whose reach is `ceil(1.35·amp)+2`.

- **twirl** (AR:1205). Defaults `{angle 90, extent .8, c (.5,.5)}`. The
  radius is `max(1, extent·.5·min(w,h))`. Inside it, rotate `d` by
  `θ(1−r/R)²` and resample. The identity is `angle == 0`.
- **pinch** (AR:1229). Defaults `{amount .5, extent .8, c}`. Inside
  `0 < r < R`, read `c + d·sin(π/2·r/R)^(−amount)` with
  `amount ∈ [−1, 1]`. The identity is `amount == 0`.
- **ripple** (AR:1249). Defaults `{amplitude 8, wavelength 60, angle 0}`.
  Here `dir = (cos a, −sin a)`, `n = (−dir.y, dir.x)`,
  `k = 2π/max(len, 1)` with `len ≥ 2`, and
  `off = n·A·sin(p·dir·k) + dir·.35A·sin(p·n·k·1.37)`.
  The identity is `amplitude ≤ 0`.
- **polar-coordinates** (AR:1270). Defaults `{to-polar, c}`.
  - To-polar: `q = (p−c)/(size/2)`,
    `uv = (fract(atan(q.x, −q.y)/2π + 1), |q|)`.
  - To-rect: `a = uv.x·2π`, `uv' = (c + (sin a, −cos a)·uv.y·size/2)/size`.
  - The sample uv is clamped to [0, 1]. It is never the identity.

Tests:
- Every distort keeps pixels outside its radius exact.
- At the centre, twirl and pinch read the centre.
- Ripple with amplitude 0 is the identity.
- To-polar maps the top row to the centre.

## Stylize and pixelate

- **bloom** (AR:1710, AR:1726, AR:2344). Defaults `{threshold .6, amount .5,
  radius 15}`. The identity is `amount ≤ 0`. Reach: `ceil(R)+2`.
  1. Bright-pass on linear unpremultiplied colour:
     `k = smoothstep(t, min(t+.25, 1), luma(clamp rgb))·step(1e-4, a)`, which
     gives `(clamp(rgb)·k, 1)`.
  2. A separable blur at radius `R` (full resolution).
  3. Screen the result in the work space: `glow = toWork(clamp(bloom.rgb))·amount`
     and `o = 1 − (1−base)(1−glow)`. Alpha is unchanged.
- **chromatic-aberration** (AR:997). Defaults `{amount .3}`. The identity is
  `amount ≤ 0`. Reach: global.
  `shift = (uv−.5)·amount·.04`. Red is read at `uv+shift`, green at `uv` and
  blue at `uv−shift`, each unpremultiplied to the work space. Alpha comes from
  the green tap.
- **halftone** (AR:1016). Defaults `{cellSize 6, angle 15, mono true}`. It is
  never the identity. Reach: `ceil(cell)+2`.
  - `cell = max(2, cellSize)`.
  - The cell centre is computed in rotated space. The cell colour `L` drives
    the radius `(1−L)·cell·.72`.
  - `ink = 1 − smoothstep(r−1, r+1, dist)`.
  - `o = mix(white, mono ? black : cellRgb, ink)`. Alpha is kept.
- **glitch** (AR:1145). Defaults `{amount .5}`, seed 23. The identity is
  `amount ≤ 0`. Reach: global.
  - `band = floor(uv.y·24)`.
  - `shift = (rand(band) > .7 ? rand(band+1) − .5 : 0)·amount·.12`.
  - `uv.x` wraps with `fract`.
  - The RGB split is `amount·.012·rand(band+5)`.
- **mosaic** (AR:1292). Defaults `{cellSize 16}`. The identity is
  `cell ≤ 1`, and the cell is `max(2, ·)`. The cell is the mean of an 8×8
  lattice of taps at `origin + (i+.5)·cell/8`, clamped to `size−.5`.
  Reach: `cell+2`.
- **crystallize** (AR:1311). Defaults `{cellSize 20}`, seed 1. The identity
  is `cell ≤ 1`. Find the nearest jittered site
  (`hash2(c)`, with a 3×3 search). The facet colour is
  `.4·at(site) + .15·(4 taps at ±cell·.2)`. Reach: `3·cell + 2`.
- **emboss** (AR:1348). Defaults `{angle 135, distance 3, amount .5,
  mono true}`. The identity is `!mono && amount ≤ 0`.
  - `dir = (cos a, −sin a)·max(.5, distance)`.
  - The height is the luma of `toWork(rgb + 1 − a)`.
  - `rel = (h(p−dir) − h(p+dir))·amount·3`.
  - `o = clamp(mono ? .5+rel : c+rel)`. Alpha is kept.
  - Reach: `ceil(h)+2`.
- **oil-paint** (AR:1373). Defaults `{radius 8, amount .5}`. The identity is
  `radius ≤ 0`. Reach: `ceil(R+st)+2`.
  - It is a weighted Kuwahara over 4 quadrants:
    `st = max(1, R/6)`, `n = min(ceil(R/st), 6)`,
    `jit = st>1 ? ign·(st−1) : 0`.
  - Each quadrant's weight is `(Σvar_rgba + 1e-4)^−q`, where
    `q = 1 + 5·clamp(amount)`.
- **wind** (AR:1416). Defaults `{angle 0, distance 40, amount .6}`. The
  identity is `distance ≤ 0 || amount ≤ 0`. Reach: `ceil(len)+2`.
  - `dir = (cos a, −sin a)`.
  - The lane length is `len·(.35 + .65·laneHash(floor(p·(−dir.y, dir.x))))`.
  - 48 taps upwind are faded by `1−t`. Keep the tap with the brightest
    premultiplied luma, then `mix(s, best, amount)`.
- **line-art** (AR:1741, AR:1797, AR:2223). Defaults `{threshold .5,
  amount .5, radius 3, mono false, lineMode xdog}`. It is a generator: a new
  layer, not mask-aware.
  - Luma is computed on linear unpremultiplied colour, and transparent pixels
    count as 1.
  - Sobel: offset `max(radius, 1)` texels; `mag = |∇|·(1+4·amount)`;
    `α = smoothstep(thr, thr+.15, mag)`.
  - XDoG: `σ = max(.8, radius)`, with blurs at `σ` and `1.6σ`;
    `D = g1 − .985·g2`; `eps = (thr−.5)·.2`; `φ = 5 + 60·amount`.
    Then `e = D ≥ eps ? 1 : 1 + tanh(φ(D−eps))` and `ink = 1 − clamp(e)`.
  - Output: `(line·ink, ink)`, where the line is white when `mono` is set and
    black otherwise.

Tests:
- Mosaic of a flat field is flat.
- Mosaic cells are constant.
- Emboss of a flat field in grey mode is .5.
- Oil paint and wind keep a flat field flat.
- Line art of a flat field is empty, and an edge gives ink.
- Bloom with threshold 1 is the identity on dark art.
- Halftone of white is white.

## Render

- **light-rays** (AR:1572, AR:2523). Defaults `{c (.5,.3), threshold .6,
  amount .6, extent .5, tint (1,.96,.88)}`. The identity is `amount ≤ 0`.
  Reach: global.
  1. Bright-pass.
  2. Half-resolution rays: 96 taps toward the centre at `t·extent`, with
     weight `1−t` and jitter `ign` on half-resolution frag coords. Then
     `·tint·3`.
  3. Bloom's screen combine with intensity `amount`.
- **lens-flare** (AR:1604). Defaults `{c (.3,.3), amount .8, extent .5,
  tint (1,.9,.75), flareStreak true}`. A generator, never the identity, and
  pixelwise.
  - `scale = max(8, extent·min(w,h))`.
  - Core, star, ring, streak and 6 ghosts, exactly as written in the shader.
  - `col = clamp(col·amount)` and `cov = max(col)`.
  - Output: `(col, cov)·keep`. The layer's blend mode is Screen.
- **clouds, fog, smoke, nebula, starfield** (AR:623, AR:2202). Defaults
  (AR:455-479): these are `genScale/Detail/Density/Softness/Warp/ColorA/
  ColorB/Seed`. They are generators, never the identity, and pixelwise.
  - `hash(p)`: `p += seed·37`, `p3 = fract(p.xyx·.1031)`,
    `p3 += dot(p3, p3.yzx+33.33)`, `fract((p3.x+p3.y)·p3.z)`.
  - `vnoise` uses the smoothstep blend. `fbm` takes `clamp(oct, 1, 8)`
    octaves, `×2` frequency and `×.5` amplitude per octave, normalised.
  - Clouds:
    1. `q = p/max(scale, 1)`.
    2. Warp (when warp > .001): `q += (fbm(q+11.3), fbm(q+47.7)) − .5) · warp·4`.
    3. `n = fbm(q)`, `edge = mix(.02, .5, soft)`.
    4. `cov = smoothstep(1−dens−edge, 1−dens+edge, n)`.
    5. Output: `(mix(A, B, n)·cov, cov)`. Colours are used as given (no
       linearisation in the shader).
  - Starfield:
    1. `g = p/scale` and `present = hash(cell)`.
    2. A star exists when `present > 1 − dens·.6`. It sits at
       `jit = (hash(cell+3.1), hash(cell+7.7))` with brightness
       `.35 + .65·hash(cell+5.5)`.
    3. `cov = smoothstep(mix(.06, .42, soft), 0, |f−jit|)·bright`.
    4. Output: `(B·cov, cov)`.
- **lightning** (LG). Defaults `{lgtStartX .5, lgtEndX .5, lgtJag .5,
  lgtBranch .5, lgtGlow .5, lgtWidth 3, genColorB (.7,.85,1), genSeed 1}`.
  It is a generator and never the identity.
  - The PRNG is mulberry32, seeded with `round(seed·2654435761) mod 2³²`.
  - The main bolt is a midpoint displacement from `(sx·w, 0)` to `(ex·w, h)`
    with 7 iterations and offset `(rnd−.5)·len·jag·.5` along the normal.
  - Branches:
    - There are `round(branch·8)` of them. Each starts at
      `main[1 + floor(rnd·(n−2))]`.
    - Each has `dir = rnd·2π` and `len = h·(.12 + rnd·.22)`.
    - The tip is `at + (cos·len, |sin|·len)`. The branch path uses 5
      iterations at `jag·1.2`.
  - The path is ported exactly and checked against `lightning.json`.
  - Raster (Canvas2D replacement): round-capped polylines with 1-px
    antialiasing. Each pass draws a shadow first: the coverage Gaussian
    blurred with `σ = shadowBlur/2` in the shadow colour. Then the stroke goes
    on top, source-over.
    - Pass 1: alpha .5, colour C, `blur = W(4+20g)`, main width `W(1.5+2g)`,
      branch width `W(1+g)`.
    - Pass 2: white, `blur = W(1+3g)`. Main width `W` at alpha 1; branch
      width `max(1, .6W)` at alpha .85.
  - Colours are sRGB, linearised on the way into the layer.

Tests:
- The lens-flare core is bright at the light, alpha is the max channel, and
  it is dimmer far away.
- Clouds are deterministic per seed, in [0, 1], and a different seed gives
  a different field.
- Starfield density 0 is empty.
- The lightning path matches the golden. The bolt touches the top and
  bottom rows, and the corners stay clear.

## Engine (PE:8102-8255)

- **BeginFilter** for a generator:
  - It is blocked like other kinds when the active layer is hidden, locked or
    an adjustment layer.
  - It inserts a new layer above the active one, named after the kind
    ("Line art" for line-art). For lens-flare, the layer's blend mode is
    Screen. The new layer becomes active, and the source layer is remembered.
  - Insertion is not a history entry yet.
- **UpdateFilter**:
  - Pixelwise generators render per doc tile, times `keep`.
  - Line art renders from the source layer's pixels over the whole document.
  - Lightning renders over the whole document. Neither of these two uses
    `keep`, as in happypaint.
- **CommitFilter** records one `Tree` history entry (before: without the
  layer; after: with it). Undo removes the layer.
- **CancelFilter** restores the tree and re-selects the source.
- Spatial kinds with `GLOBAL_REACH` run over the whole canvas. Others run
  over content + reach.
- Fill kinds and tone are added as layers (`AddAdjustmentLayer`) from the
  menu. They composite live per tile through `prepare_in` with the doc's size
  and DPI.

Tests (`crates/engine/tests/filters.rs`):
- Clouds commit makes one new layer and one undo step, and undo removes it.
- Cancel restores the layer count and the active layer.
- Lens flare lands on a Screen layer.
- Line art leaves the source untouched.
- A solid-fill layer covers the backdrop.
- Twirl on a full-canvas layer moves pixels.

## UI

`filters::ui::controls(kind)` describes the generic controls for every new
kind:

- slider: key, label, range, display scale, unit;
- toggle;
- choice;
- colour ("use the current colour").

The app renders these through the params' JSON form, so it needs no per-kind
code. Selective colour edits its first sample. Field blur edits its pins. The
menu is grouped by `Kind::section()`: Color, Blur, Sharpen & Noise, Distort,
Pixelate, Stylize, Render, Fill. Fill and tone items create layers.
On-canvas handles (`filter-handles.ts`) are not ported. Centres and ends are
sliders.

## Ambiguities resolved

- Lightning: pass 2 opens with `ctx.restore()` and `ctx.save()`, which reset
  `globalAlpha`, so the main core is drawn at alpha 1. (The golden export
  stubs the canvas and does not model that reset, so the golden pins only the
  geometry, widths and blurs.)
- Tone ink: the shader runs `fromWork(clamp(toWork(u_ink)))` on the `target`
  param, so on a float document the ink value is stored as given (it is
  treated as linear). Black and white inks are unaffected. This is ported as
  written.
- Pattern colours and lightning params pass through f64 maths in happypaint.
  onepaint widens them through their shortest decimal form, so rounding ties
  (`0.1·255 = 25.5`) land the same way.
- Tone `fwidth` is estimated from neighbouring doc pixels with the level held
  fixed. In luminance mode the GPU's derivative also sees the level change.
- The gallery levels and the rays use happypaint's half-resolution buffers.
  The bokeh pre-blur also runs at half resolution.
