# 65: Workspace layout (menu bar, options row, left rail)

## Status
Agreed (2026-10-20). Phases 1 (the menu bar) and 2 (the options row) are
built (below); phases 3–6 follow.
Replaces onepaint's canvas-first chrome (a "…" menu, a floating tool bar
top right, the Layers popover) with happypaint's v0.204 workspace.
Retires divergences 41 and 42, and the View-menu half of 49.

## Source
happypaint `routes/editor.tsx` (821–1038, the header), `components/menu-bar/`
(`menu-model.ts` 209–433, `menu-bar.tsx`), `components/toolbar/toolbar.tsx`
(the rail 1897–2152, the options row 2156–3064, the header buttons
3065–3667), `components/window-controls/window-controls.tsx`,
`state/dock-store.ts` 119–139, `styles/global.css`. Tests:
`menu-reachability.test.ts`, `tools-dock-default.test.ts`,
`tool-reachability.test.ts`, `side-sliders-placement.test.ts`,
`dock-tabs.test.ts`.

## Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ◉ File Edit View Image Layer Select Adjust Help │  Untitled  │ ●REC Brushes Brush Guides Layers ─ □ × │  row 1 (34 px)
│ ■□ ⇄ │ [brush ▾] ∠ 0° ⊜ ──○── 20 │ Size|Opacity │ ‹tool options…›       100% ▾ │  row 2 (36 px)
├──┬───────────────────────────────────────────────────────────┬───────────┤
│▣▣│ tabs (when several drawings are open)                     │ Layers    │
│▣▣│┃                                                           │           │
│▣▣│┃ side sliders (size, pick, opacity, undo, redo)          │───────────│
│▣▣│┃                       canvas                              │Properties │
│▣▣│                                                           │ (closed)  │
├──┴───────────────────────────────────────────────────────────┴───────────┤
│ status bar                                                               │
└──────────────────────────────────────────────────────────────────────────┘
```

### Row 1: the title bar
A three-column grid: menus at the start, the drawing name centred in the
window (pushed right, never under the menus, when they need more than
half), and the end column.

- **Start:** the app mark (opens the gallery), then the menu bar.
- **Centre:** the drawing name, an inline field (click to rename; empty
  becomes "Untitled"; width 14 rem, 7 rem at ≤720 px, 5 rem at ≤600 px).
- **End:** the REC pill while recording (click stops it); the panel
  buttons Brushes, Brush settings, Guides, Layers (icon and label;
  labels hide below 1180 px); then the window controls.
- **Window:** on Linux and Windows the native title bar is off
  (`with_decorations(false)`): empty space in row 1 drags the window,
  double-click maximises, and minimise / maximise-restore / close sit in
  the corner (close turns red on hover). Edge resizing goes through
  `ViewportCommand::BeginResize` from the window border. macOS keeps its
  native title bar (as happypaint). Settings gets "Use the system title
  bar" for window managers that misbehave without decorations.

### Row 2: the options row
One line that scrolls sideways when it overflows (the wheel scrolls it);
nothing folds into a "More" popover. Its popovers anchor below their
button outside the row's clip. In order:

1. **Colour:** stacked foreground / background swatches (click FG for the
   Colour popover, BG to swap), swap, and reset to black and white.
2. **Brush strip**, for Brush, Eraser, Smudge, Mixer and Shape only: the
   brush (thumbnail and name, opens the library), the angle dial with its
   field (dimmed while the brush is wet), the stabiliser toggle and
   amount (0 = off), and Pressure as two toggles, Size | Opacity. Size and
   opacity stay on the side sliders.
3. **Tool options**, by tool (below).
4. **Zoom**, at the end: a menu showing the zoom %, with Zoom in, Zoom
   out, Fit to screen, Actual size.

| Tool | Options row |
|---|---|
| Brush, Eraser | the brush strip only |
| Smudge | Strength 1–100 |
| Mixer | preset (Custom; Dry, Moist, Wet, Very wet groups), Wet, Load, Mix, load from canvas (swatch), Keep colour, Clean |
| Liquify | mode (Push, Twirl left / right, Pinch, Expand, Reconstruct), Strength |
| Fill | Tolerance, Tone, Wash; with Wash: Water only, Water, Pigment, Graded, Soft, Speed, All layers |
| Gradient | output (Pixels / Fill layer), shape, Reverse, Options (the Colour panel's stops and presets) |
| Eyedropper, Pan | none |
| Move | Flip H / V, rotate ∓15°, scale ÷ / ×1.1, Uniform, From centre, Distort, Warp, Apply, Cancel (the last four only with a transform pending) |
| Select tools | New / Add / Subtract / Intersect; the wand adds Tolerance, Contiguous, Sample all layers |
| Select tools, or any tool with a selection | Select all, Invert, Expand, Contract, Smooth, Deselect, Save selection, Saved ▾; Copy / Cut to new layer, Copy, Paste; Amount (px); Feather |
| Text | none: the text panel opens, and clicking the tool again toggles it |
| Pen | output (Shape / Path / Pixels), Pen \| Curvature, Constrain 45°, Fill, Snap, edit mode, Path ▾ (Make selection, Fill, Stroke, Vector mask, Free transform, Delete) |
| Shape | Line \| Ellipse \| Rectangle \| Polygon, Sides (polygon), Fill |
| Panel | Grid 1×2, 2×1, 2×2, 3×1, 3×2, 4×2 |
| Any, while resizing the canvas | Canvas W × H, new area (Transparent, Background, Foreground, White, Black), Apply, Cancel |

These replace the per-tool option popovers and the Tool options panel's
contents today; the panel stays dockable and shows the same controls
stacked, for people who want them beside the canvas.

### Menu bar
File · Edit · View · Image · Layer · Select · Adjust · Help, as data (one
list for the bar and the compact sheet). Click opens; hovering another
title switches while one is open; ←/→ and ↑/↓ move; Esc or a click
outside closes. Toggles show ✓. Shortcuts show the live key binding.
Every action in today's "…" menu moves here; the "…" menu goes.

- **File:** New or open drawing… (gallery) · Save · Save as… · Save book
  (book pages only) · Export… · — · Place image… · Paste image · — ·
  Recording, replay and timelapse items (onepaint's "Capture" lives here
  until View gets it, below) · Paper… (onepaint)
- **Edit:** Undo · Redo · — · Cut · Copy · Paste · Copy to new layer ·
  Cut to new layer · — · Fill with foreground · Fill with background ·
  Clear · — · Settings…
- **View:** Zoom in · Zoom out · Fit to screen · Actual size · Rotate left
  · Rotate right · Reset rotation · — · Grayscale · Print preview (with
  Press, Paper white, Black ink) · Non-printable colours · Fill mode ·
  — · *Guides:* Show guides · Rulers · Edit guides · Measure · Symmetry
  vertical / horizontal · Move symmetry axis · Scrollbars · — · *Panels:*
  Layers · Properties · Brush settings · Reference image · Align &
  distribute · Animation · Palettes and Tool options (onepaint) · — ·
  *Capture:* Record drawing · Replays… · Export timelapse… · — ·
  *Workspace:* Zen mode · Size & opacity sliders · their six placements
  · Horizontal toolbar · Reset panel layout · Light / Dark theme
- **Image:** Canvas size… · Image size… (onepaint keeps both) · — · Merge
  visible to new layer · Flatten image
- **Layer:** New · Duplicate · Delete · — · Group / Ungroup · Merge down ·
  — · Clipping mask · Alpha lock · Lock · Layer style… · — · *Mask:* Add /
  From selection, or Invert / Delete · — · *New fill layer:* Solid colour,
  Gradient, Pattern · Convert to paint layer (text and path layers)
- **Select:** All · Deselect · Invert · — · Expand, Contract, Smooth by N px
- **Adjust:** Recent, then the categories (Color, Blur, Sharpen & Noise,
  Distort, Pixelate, Stylize, Render, Effect lines) as flyouts, as the
  Adjustments popover lists them today
- **Help:** Shortcuts & gestures · Guide (onepaint's help sections) ·
  Manual · About onepaint

Items happypaint has and onepaint doesn't (Google Drive, camera photo,
the tour, What's new) are left out until those features exist.

### The left rail
The tool bar defaults to a rail docked left (stored `None` reads as
left), two 34 px columns, from under the header to the status bar, with
the stage reserving its width. Tool order: Brush, Eraser, Fill,
Eyedropper, Move, Smudge, Mixer, Gradient, Liquify, Text, Pen, Shape,
Panel, Pan; then the five selection tools each with its own button;
then Undo / Redo (only when the side sliders are off); then Clear layer
and Zen mode. No flyouts, no colour swatch (it is in row 2). Dragging the
grip floats it; dropping at either edge docks it there; double-click
flips a floating bar horizontal. onepaint's Guides, Animation,
Adjustments, Brush library, Layers and Colour buttons leave the bar for
the header and the menus.

### Side sliders and panels
The side sliders sit just right of the rail. Layers docks right and is
open by default; Properties docks right under it, closed. Brush
settings, Colour and Guides open as popovers under their header buttons.
The dock width defaults to 280 px (190–400).

### Narrow windows
At ≤900 px the menus collapse into one ☰ button whose sheet lists them as
sections, panel labels go, zoom moves to the status bar, and the rail
becomes a scrolling strip above the status bar. At ≤600 px the brush
strip hides and popovers become bottom sheets.

## Phases
1. **Menu bar** as data, rendered in a new row 1 with the drawing name
   and the panel buttons; the "…" menu goes. A reachability test: every
   action the old menu had is in a menu.
2. **Options row**: the colour group, the brush strip, the per-tool
   controls (moved out of today's popovers into shared functions the Tool
   options panel also uses), and the zoom menu.
3. **Left rail by default**, the new tool order, the five selection
   buttons; Layers docked right and open.
4. **Title bar**: decorations off on Linux and Windows, drag, double-click,
   window controls, border resizing, and the Settings escape hatch.
5. **Narrow windows**: the ☰ sheet, the strip, the hidden labels.
6. **Help ▸ Manual**, written against this layout.

Each phase lands on its own, with kittest coverage and screenshots in both
themes; the old chrome stays usable until phase 3 switches the default.

## Phase 1 as built
- `ui_kit::menu_bar`: menus as data (items, toggles, separators, headers,
  one level of submenus), keyboard and pointer as above, `is_open` so the
  click that closes a menu doesn't paint and the shortcuts pause while
  one is open.
- `apps/onepaint/src/menus.rs`: the eight menus built from the app each
  frame (checks, enabled rows, the live key bindings) and `run`; the
  keyboard shortcuts share its helpers.
- The header row: the app mark (the gallery), the menus, the drawing's
  name centred (several drawings show as chips there until the tab strip
  lands), REC, the wet-paint controls, Brushes, Brush settings, Guides
  and Layers. The "…" menu and the tool bar's Guides, Brush library and
  Layers buttons are gone. The stage, docks and rails start below it.
- New engine commands for the Layer and Image menus: `GroupLayer`,
  `Ungroup`, `MergeVisible`, `FlattenImage`, `InvertMask`,
  `AddRevealMask`.
- View's Guides, Panels, Capture and Workspace sections are submenus
  (divergence 55): inline they made the menu taller than the window.
- Not yet: renaming the drawing in place (no document rename command yet;
  the name comes from the file), the window controls (phase 4). The
  transform bar sits at the bottom until phase 2 moves it into the row.
- Tests: `every_menu_command_is_reachable`,
  `menus_run_their_command_and_escape_closes`, the engine's
  `tests/structure.rs`, `menu_bar` focus stepping.

## Phase 2 as built
- `options_row.rs` in the header's second row (`chrome::ROW2_H`): the
  colours (foreground opens the Colour popover under it, background
  swaps, swap, black and white), the brush strip for Brush, Eraser,
  Smudge, Mixer and Shape (the brush ▾, angle dial, Stabilizer and its
  amount, Pressure Size / Opacity), each tool's options, and the zoom
  menu fixed at the end. It scrolls sideways; popovers it opens anchor
  under their control (`App::row_anchor`).
- `ui_kit::inline`: `inline_slider`, `chip`, `menu_chip`, `dropdown`.
- The transform bar is gone: the row shows the transform's controls
  whenever one is pending. The tool bar's colour swatch is gone, and
  clicking an active tool no longer opens its options popover (Brush and
  Eraser still open the brushes); the Panel tool no longer opens its
  layouts. The stacked controls stay in the Tool options panel (View ▸
  Panels).
- Kept as onepaint has them: Fill's Grow, the gradient's stops in its
  panel ("Stops ▾"), the pen's path menu in its panel ("Path ▾"), saved
  selections in the selection panel ("Saved ▾"). Not yet: the shape
  segment's Polygon sides as a field, the selection Amount field (refine
  steps are 4 px), Tone for Fill (onepaint has no tone fill).
- A view still fitted to the window refits when the canvas changes size
  (the header, a dock, the window); a panned or zoomed one stays.
- Tests: `the_options_row_drives_the_brush_the_tool_and_the_zoom`.

## Divergences
- 41, 42 retire. 49 narrows to the eyedropper square (side sliders).
- Kept onepaint additions: Image size…, Paper…, Palettes and Tool options
  panels, the Guide.
- The REC pill shows for onepaint's own recordings (happypaint shows it
  for recording and screen capture; onepaint has no screen capture yet).
