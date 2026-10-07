# 12: Gradient tool, shape tool and quick-shape (slice 5)

Sources are relative to `../happypaint/frontend/src/`.

## Gradient (`utils/gradient.ts`, `engine/renderer/gradient-ramp.ts`, compositor.ts:271-288)
- **`t(p)`:** with `d = p−a`, `ab = b−a`, `len2 = max(|ab|², 1e-4)`,
  `along = d·ab/len2` and `across = (d.x·ab.y − d.y·ab.x)/len2`:
  - linear: `along`
  - radial: `|d|/√len2`
  - reflected: `|along|`
  - diamond: `|along| + |across|`
  - angle: `fract((atan2(ab) − atan2(d))/2π)`, or 0 at `a`
  - The result is clamped to 0..1.
- **Stops:** `{pos, color (sRGB), opacity, mid?}`. Midpoints are clamped to
  0.05..0.95; `bend(f, mid) = f ≤ m ? .5f/m : .5 + .5(f−m)/(1−m)`.
  Sampling holds the end stops and lerps straight RGB and opacity by the bent
  fraction.
- **Reverse:** `pos → 1−pos`. A midpoint moves to the stop that now starts
  its segment, as `1−mid`.
- **Presets:** FG→transparent (the default) and FG→BG.
- **Ramp:** 1024 entries, with entry i at `t = i/1023`. Stops are
  **linearised per stop** (float document), then interpolated, then a
  manual lerp between entries.
- **Pixel:** `a = c.a · selection`, then `(c.rgb·a, a)`, premultiplied, drawn
  into the stroke buffer. It is flattened with the brush opacity, normal
  blending and alpha lock respected, as one undo step.
- **Gesture:** drag from a to b. There is no snapping.

## Shape tool (`engine/stroke/shape-tool.ts`)
- **Kinds:** line, ellipse, rect, polygon (3..12 sides, default 6). A drag
  shorter than 2 px gives nothing.
- **line:** with Alt, `from = a − d`. Shift snaps to 45°.
- **polygon:** centred on `a`. `r = |d|` and `rot = atan2(d)`; Shift snaps
  `rot` to 15°. Vertex i is at angle `rot + i·2π/n`.
- **rect / ellipse:**
  - Shift: `s = max(|dx|, |dy|)`; `hw = hh = Alt ? s : s/2`; the centre is
    `a` (Alt) or `a + sign·hw`.
  - Alt only: `hw = |dx|`, `hh = |dy|`, centred on `a`.
  - Otherwise: half the box, centred on `a + d/2`.
  - Both half-sizes are at least 1.
- **Stroking:** the outline is stroked with the current brush along
  `samplePath(shape, 2 px)` at pressure 1. With fill on, the shape is filled
  with the colour at brush opacity under the outline, in one undo step.
- **samplePath:**
  - The ellipse uses the Ramanujan perimeter with `n = max(16, ceil(P/2))`.
  - Rects and polygons are closed by repeating vertex 0.

## Quick-shape (`engine/stroke/quick-shape.ts`)
- **Trigger:** hold still for 600 ms during a brush or eraser stroke
  (movement under 3 px doesn't count). The shape replaces the stroke.
- **recognize:** at least 8 points and a bbox diagonal of 24 or more.
  - `closed = |start−end| < .22·diag`.
  - A stroke that isn't closed with `maxPerp < .1·len` is a line (snapped to
    45° with Shift). A stroke that isn't closed otherwise gives no shape.
  - Ellipse error: `mean(| |(p−c)/r| − 1 |·min(rx, ry))`.
  - Polygon corners come from RDP with tolerance `.04·diag`; the last corner
    is dropped if it lies within `.06·diag` of the first. The polygon error
    is the mean distance to the nearest edge.
  - If `polyErr < ellipseErr` and there are 3..8 corners: 4 corners within
    `.14·diag` of the bbox corners make a rect (square with Shift);
    otherwise it is a polygon.
  - Otherwise it is an ellipse (a circle with Shift, `r = (rx+ry)/2`).

## Golden tests
- **shapeT** with `a=(0,0)` and `b=(100,0)`:

  | Shape | Input → t |
  |---|---|
  | linear | (25,40) → .25; (−10,0) → 0; (150,0) → 1 |
  | radial | (0,50) → .5; (−30,−40) → .5; (300,0) → 1 |
  | reflected | (±25,0) → .25 |
  | diamond | (100,0) → 1; (50,50) → 1; (25,25) → .5 |
  | angle | (50,0) → 0; (0,−50) → .25; (−50,0) → .5; (0,50) → .75; at a → 0 |

- **Stops** [(0,red,1), (.5,green,.5), (1,blue,0)]:
  - t=0 → [1,0,0,1]; t=.5 → [0,1,0,.5]; t=1 → [0,0,1,0]
  - t=.25 → [.5,.5,0,.75]
- **Midpoints** on a black→white segment:
  - mid .25 gives .5 at t=.25; mid .8 gives .5 at t=.8
  - a missing mid or .5 is the identity
- **Reverse** of [0 mid .2, .6 mid .9, 1] gives positions [0, .4, 1] with
  mids [.1, .8, none].
- **Shapes**, with `A=(100,100)`:
  - A drag to (100.5,100.5) gives nothing.
  - rect to (300,200) gives `{200,150,100,50}`; Shift gives hw=hh=100 at
    c=(200,200).
  - Shift to (−100,40) gives c=(0,0) with half-size 100.
  - Alt to (160,130) gives `{100,100,60,30}`.
  - polygon to (100,20) with 7 sides gives r=80; to (100,180), `pts[0]=(100,180)`.
  - line with Shift to (200,110) gives `b=(100+√(100²+10²), 100)`.
  - line with Alt to (160,100) gives `a=(40,100)`.
  - clampSides: 6→6, 2→3, 99→12, 5.7→6, NaN→3.
