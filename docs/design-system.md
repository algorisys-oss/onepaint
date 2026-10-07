# onepaint design system

The goal is a modern, beautiful and calm UI. The artwork is the hero, and the
chrome floats around it and stays out of the way. The feel should be closer
to Procreate than to Photoshop's dense panels. Stock egui looks like a
debugging tool, so **no default egui widget or visual ever reaches the
user**. Everything goes through `crates/ui_kit`.

## Starting point: photocraft's `ui-egui`

Bootstrap `ui_kit` from photocraft's design layer
(`github.com/storytold/photocraft`, `crates/ui-egui/src/`, MIT or
Apache-2.0). It already solves "egui that doesn't look like egui". Copy or
adapt its code with an attribution header (`// Adapted from photocraft
(MIT/Apache-2.0): <path>`), strip any photocraft or ArtCraft branding, and
decouple it from `PhotocraftApp` and the photocraft crates.

| photocraft file | What to take | onepaint target |
|---|---|---|
| `theme.rs` | A `Tokens` struct that widgets read through `Tokens::get` instead of hard-coding colors. Includes `ThemeKind`, font setup (Inter and JetBrains Mono) and the mapping from tokens to egui `Visuals`. | `ui_kit::theme`. Keep the **Studio** and **Studio Light** themes as references and add a happypaint palette. Drop Pro/ProMedium/Classic unless asked. |
| `widgets.rs` | Cards with pill tabs, thin sliders with round knobs (with optional gradient tracks), monospace value fields with dimmed units, toggles, primary and secondary buttons, dropdowns, swatch popups, a checkerboard, hairlines | `ui_kit::widgets` |
| `icons.rs` and `icon_data.rs` | Embedded SVGs, rasterized by `egui_extras` at the exact on-screen pixel size and tinted per use; icon buttons and rail buttons | `ui_kit::icons`, with the SVG set swapped for **Lucide** |
| `tab_strip.rs` | Tab strips that fit any width: labels shrink and elide, then overflow into a » menu | `ui_kit::tabs` |
| `dock.rs` | A custom dock with panel groups and fixed heights, used instead of `egui_dock` | `ui_kit::dock` (the desktop docked mode) |
| `gpu_canvas.rs` | An `egui_wgpu` `CallbackTrait` canvas with damaged-rect texture updates | pattern for `apps/onepaint` canvas plus our `gpu` crate |
| `egui_kittest` snapshot tests | How their visual tests are set up | our snapshot tests |

Where onepaint goes beyond photocraft:

- photocraft follows Photoshop's grammar: dense docked panels and a status
  bar. onepaint's **default** is the canvas-first, floating, Procreate-like
  layout below. photocraft's docked style is the optional power-user mode.
- photocraft has no frosted-glass panels, motion system, SDF shadows or
  touch-gesture chrome. Those are onepaint additions (see below).
- photocraft's Studio accent is violet `#8b7cf6`. Our default accent is
  happypaint's blue `#4c8dff`, and the accent is a token, so a theme can
  change it.

## Principles

1. **Canvas first.** The canvas fills the window edge to edge, and the chrome
   is floating, rounded and translucent. One gesture or key (four-finger tap,
   or Tab) hides all of it.
2. **Quiet by default, rich on demand.** Show few controls. Reveal detail
   only on focus (the brush studio, filter panels).
3. **Motion explains state.** Animate every state change: panel open and
   close, hover, selection, tool switch. Most motion uses 120–200 ms ease-out
   springs. Respect the OS reduced-motion setting.
4. **One visual language.** Spacing, radius, type, icons and color all come
   from tokens. Never use a magic number in widget code.
5. **Fast is beautiful.** The UI never drops a frame. Repaint only on change
   or animation, and keep the canvas latency budget sacred.

## Tokens (`ui_kit::theme`)

The starting palette is happypaint's palette
(`../happypaint/frontend/src/styles/global.css`), so the two apps feel like
siblings. Both dark and light themes are required, and dark is the default.

| Token | Dark | Light |
|---|---|---|
| bg | `#1a1a1a` | `#f2f2f4` |
| bg_elevated | `#242424` | `#ffffff` |
| panel (floating) | `rgba(30,30,34,0.92)` | `rgba(255,255,255,0.95)` |
| panel_glass | `rgba(30,30,30,0.6)` + backdrop blur | `rgba(255,255,255,0.72)` + blur |
| fg / fg_muted | `#f2f2f2` / `#a0a0a0` | `#1d1d1f` / `#6e6e73` |
| accent / on_accent | `#4c8dff` / `#fff` | `#2f74e8` / `#fff` |
| border | `#333` | `#d9d9de` |
| danger / success / warning | `#e5484d` / `#3ec46d` / `#e0a23c` | `#d23b3b` / `#2ea85a` / `#c07d18` |
| shadow | `rgba(0,0,0,0.45)` | `rgba(0,0,0,0.18)` |
| layer labels | red, orange, yellow, green, blue, violet, gray, as in happypaint | darker variants |

The rest of the scale:

- **Radius:** 4 (small controls), 8 (default), 12 (panels), 999 (pills).
- **Spacing:** a 4 px grid: 4, 8, 12, 16, 24, 32.
- **Elevation:** three shadow levels (popover, panel, modal), all soft and
  wide with low opacity.
- **Type:** Inter (variable) for the UI and JetBrains Mono for numbers in
  inputs. Both ship inside the binary. The sizes are 11, 12, 13 (body),
  15 and 18. Numeric readouts use tabular figures.
- **Icons:** Lucide, the same set as happypaint, at 16 and 20 px with a 1.5 px
  stroke. They are rendered as SVG to texture at the DPI scale, and are never
  blurry bitmaps.

## Signature effects (all on wgpu)

- **Frosted-glass panels.** We own the canvas render, so the
  backdrop is a cheap downsampled dual-Kawase blur of the composited canvas,
  sampled under each panel rect in the `egui_wgpu` callback. It is applied
  only where panels overlap the canvas. Fall back to the opaque `panel` token
  on low-end GPUs.
- **Soft shadows** come from a single SDF rounded-rect shader, not from
  stacked egui shapes.
- **Live previews everywhere.** Brush cards show a real rendered stroke from
  the brush engine (cached). Filter thumbnails come from the actual GPU pass,
  and layer thumbnails update incrementally from the GPU tiles.
- **Color picker:** a disc or square, plus harmony, value and palette views,
  in the style of Procreate. It is GPU-drawn, smooth, and has no banding
  (16F).

## Layout

- **Top bar:** gallery and actions on the left, adjustments, selection and
  transform in the center, tools on the right, then brush, smudge, eraser,
  layers and color. This follows happypaint and Procreate.
- **Side rail:** a slim floating bar with size and opacity sliders, the
  modifier button, and undo and redo.
- **Popovers** anchor to their trigger, with an arrow and a spring-in. Use
  them for brushes, layers and color.
- **Optional docked mode** (`ui_kit::dock`, adapted from photocraft's
  `dock.rs`) for desktop power users. It uses the same widgets with denser
  spacing.
- **HiDPI:** pixel-perfect at 1×, 1.5×, 2× and 3×. Snap 1 px hairlines to the
  physical pixel grid.

## Components (`ui_kit`)

Every component must have:

- a hover, pressed, focused and disabled state
- a keyboard focus ring
- an accesskit label
- a tooltip that shows its shortcut

The components:

- Button, in these kinds: primary, secondary, ghost, icon, toggle and segmented.
- Slider (with fine-drag on modifier), number field (scrub on drag), switch,
  checkbox and radio.
- Popover, menu, context menu, modal, toast and tooltip.
- Panel and card, tabs, a list with drag-to-reorder (for layers), and a tree
  (for groups).
- Swatch, color wheel, gradient editor and curve editor.
- Brush card, layer row (thumbnail, blend mode, opacity, lock and visibility)
  and the filter-parameter panel.

## Quality bar

- **Visual regression:** use `egui_kittest` snapshot tests for every
  component in both themes. Every screen gets a screenshot at 1× and 2×.
- **Contrast:** text meets WCAG AA in both themes.
- **Sign-off:** before a UI feature counts as done in
  `docs/feature-parity.md`, compare it side by side with happypaint. It must
  look at least as good.
