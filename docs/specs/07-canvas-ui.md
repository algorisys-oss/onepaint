# 07: Canvas, view and UI

## View transform (`utils/coords.ts`, `state/viewport-store.ts`)
s = scale, θ = rotation and off = offset, all in logical screen px with
y pointing down.

- **Forward:** `screen = view/2 + off + R(θ)·s·(doc − docSize/2)`
- **Inverse:** `doc = docSize/2 + R(−θ)·(screen − view/2 − off)/s`
- **Scale limits:** clamp to [0.05, 64].
- **Zoom about a point:** `zoomAt` keeps the point fixed:
  `off' = q − f·(q − off)` with `q = point − view/2`.
- **Rotation snapping:** snap to the cardinal angles within 5°.
- **Fit to view:** fit the whole document with a margin.

## GPU canvas (`gpu`)
- An `egui_wgpu` paint callback.
- **Composite storage:** an Rgba16Float texture (chunked if it exceeds the
  device's maximum texture size). Dirty tiles are updated with
  `queue.write_texture`.
- **Present shader:**
  - Sample the composite with linear filtering, using mipmaps when
    zoomed out.
  - Unpremultiply, apply `linear_to_srgb`, then re-premultiply.
  - Draw over the screen-fixed checkerboard (0.85 / 0.64, squares of
    `max(2,10·dpr)` px), clipped to the rotated document quad.
- The stage color comes from the theme.
- **Brush cursor:** an outline circle at the brush radius × scale.

## Input
- **Pointer sources:**
  - egui pointer events for the mouse.
  - egui touch events for pen and touch. `force` is used as the pressure
    when it is present.
- **No hardware pressure:** fall back to the velocity synth (spec 03).
- **Pan and zoom:**
  - Pan: space-drag, middle-drag or two-finger scroll.
  - Zoom: ctrl+wheel or pinch, toward the cursor.
  - Rotate: R-drag, or the shift+wheel equivalent.
- **Keys:**
  - B brush, E eraser, [ and ] size, ctrl+Z undo, ctrl+shift+Z / ctrl+Y redo
  - ctrl+0 fit, ctrl+1 100%, ctrl+S save, ctrl+O open, ctrl+shift+E export
    PNG
  - Tab toggles the UI

## UI (slice 1; follows `docs/design-system.md`)
- **Top bar** (floating, glass):
  - Left: app menu (New, Open, Save, Export PNG).
  - Right: tools (brush, eraser), brush picker, layers, color.
- **Side rail** (floating, left): size and opacity sliders, undo and redo.
- **Brush popover:** the preset list with live stroke previews.
- **Layers popover:** layer rows with a thumbnail, name, visibility,
  opacity, blend mode, clip and alpha lock. Also: add, delete, duplicate,
  merge down, and reordering by drag or up/down buttons.
- **Color popover:** an HSV square and hue strip, a hex field and recent
  colors.
- **Themes:** dark (default) and light.

## Acceptance tests
- Converting doc → screen → doc round-trips within 1e-9 at random s and θ.
- `zoom_at` keeps the anchor point fixed.
- `ui_kit` widgets build without panicking in an `egui_kittest` harness,
  and snapshot images are produced for both themes.
- Manual check: `./dev-start.sh` covers every goal in spec 00.

## Pen tablets (`input::tablet`, `apps/onepaint/src/tablet.rs`)
- **Wayland:** the tablet protocol (`zwp_tablet_v2`, via `octotablet`)
  gives pressure (0..1), tilt (`[sin x, sin y]` of the angle from
  perpendicular, rotated into canvas space), twist (roll, degrees) and the
  eraser end.
- **Pointer events:** once the protocol is bound, the compositor sends the
  pen there instead of as pointer events. So each sample becomes the
  touch-with-force plus pointer events egui-winit makes for pens elsewhere,
  and the UI and canvas need no second path.
- **Every sample is a stroke point:** high-rate samples are not collapsed to
  one per frame.
- **Per-sample data:** pressure, tilt and twist travel in a queue the canvas
  matches by position.
- **Eraser end:** turning the pen over switches to the Eraser while it is in
  range, then back to the previous tool.
- **Safety:** the tablet holds the window's raw handles. It is built from
  eframe's creation context and dropped in `on_exit`, the module's one
  `unsafe` (SAFETY-commented).
- **Not yet:**
  - Windows: winit's pen input gives pressure; tilt and twist need Windows
    Ink, which isn't hooked up.
  - X11: XInput2 valuators.
  - macOS: NSEvent pressure and tilt.
