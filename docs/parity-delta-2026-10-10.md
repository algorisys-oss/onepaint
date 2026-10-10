# happypaint changes since onepaint's last parity pass (2026-10-10)

happypaint shipped v0.188 → v0.201.7 (about 90 commits, 2026-10-05 → 10-10)
while onepaint was porting. This lists what onepaint lacks or now gets
wrong. Evidence came from three read-only reviews of the commits against
onepaint's code. Where a feature is *missing*, see the cited happypaint
commit for the spec.

## Bugs and data loss in onepaint (fix first)

All five are fixed (2026-10-10). Item 1 keeps the mode by name until
the modes are implemented.
1. **New blend modes are lost on save.** `.happypaint` files with
   `pass-through`, `dissolve`, `darker-color`, `lighter-color`,
   `vivid-light`, `linear-light`, `pin-light`, `hard-mix` or `subtract`
   parse as Normal and save back as Normal (`BlendMode::parse`). PSD
   `pass`/`diss`/… do the same (`psd.rs` `mode_of`).
2. **An untouched transform fades a soft selection.** `commit_transform`
   has no identity short-circuit for raster layers (transform.rs:408), so
   partial coverage loses opacity. happypaint fixed this in 0.195.1.
3. **Fill layers export to PSD empty.** `layer_record` drops the pixels of
   any layer with an adjustment (psd.rs:164).
4. **The damp-paper veil is 0.24 in 8-bit documents.** happypaint uses
   0.12 there (`wet-renderer.ts:1441`). This follows from spec 50.
5. **Granulation's sign is reversed** since happypaint 0.195.3: pigment now
   settles in the valleys (`1 + s·1.2·(1 − 2p)`). onepaint has
   `(2p − 1)` in step.rs, wet.wgsl and spec 36.

## Missing features
### Compositing and layers
- 8 blend modes (Dissolve, Darker Color, Lighter Color, Vivid Light,
  Linear Light, Pin Light, Hard Mix, Subtract) and Pass Through groups
  (v0.200.1, fd8e45b).
- Layer styles (v0.196.0–0.196.8):
  - Blending Options: Stroke, Drop and Inner Shadow, Outer and Inner Glow,
    Colour and Gradient Overlay, Fill opacity;
  - Bevel & Emboss with Contour and Texture; Satin; Pattern Overlay;
  - a blend mode per effect, up to four of each effect, Global Light;
  - styles on groups and inherit-alpha layers;
  - a Styles page with built-ins, `.asl` import, and PSD `lfx2` both ways.
- Ctrl-click a layer or mask thumbnail to select by opacity (0.195.0).

### Adjustments (v0.201.2–0.201.6)
- Exposure, Vibrance, Photo Filter, Channel Mixer.
- CMYK Selective Color.
- Color Lookup (`.cube`/`.3dl`).
- Black & White hue weights and tint, and Color Balance tone ranges.
  happypaint re-renders existing layers of both kinds with the new
  formulas, so the same file now looks different in onepaint.

### PSD (happypaint's own codec, v0.201.0)
- Colour modes: CMYK, Lab, Indexed, Duotone. Depth: 32-bit.
- PSB export.
- Byte-identical round trips.
- Locks, colour labels, guides.
- Live adjustment layers for Levels, Curves, Hue/Sat, Gradient Map and
  the new kinds.
- Fill layers through `vectorFill`.

### Wet media and paper (v0.195.3–0.199.2)
- Paper absorbs: water stays on dry paper and flows on wet. This changes
  diffusion, advection, drift and slope push. The CPU sim, `wet.wgsl` and
  their oracle tests must change together.
- The water halo (`fluid.halo`; Wet-on-Wet Live uses 1.5).
- Wash fill, with water-only washes inside walls.
- Live dabs that take the brush tip (mask, roundness, angle).
- Ten procedural papers in mm with sheet colours, flecks and mottle, and
  "Apply paper colour to Background". The preset ids change: `rough` →
  `paper-rough`, and `rough-tooth` → `rough-classic`.

### Workspace (v0.190–0.201.7)
- A menu bar: File, Edit, View, Image, Layer, Select, Adjust, Help.
- The tool options row in the header, with Brushes, Brush settings,
  Guides and Layers at its right.
- The tool dock as a left rail by default.
- Side sliders: an eyedropper square, a draggable strip, on/off and
  left/right settings, and a size ceiling that follows the maximum brush
  size.
- Canvas scrollbars.
- A brush angle dial with Shift+[ / ].
- In-app dialogs: gallery rename and delete, a confirm before deleting a
  palette, and a name prompt when saving a selection.
- A Help ▸ Manual entry, and an "All brushes" library scope with search.
- The gradient stop editor, presets and on-canvas handles for gradient
  and pattern fills.

## onepaint docs (refreshed 2026-10-10)
- Divergences 1, 3 and 6 were stale (pigment, overflow and the Wayland
  tablet are implemented); 1 and 3 are gone, 6 and 15 are narrowed, and 41
  notes happypaint's new left rail. 42–44 still hold.
- Spec 15 no longer claims `hue2`/`levl`/`curv` export, the 16-bit and DPI
  notes are current, and pass-through is kept by name.
- `feature-parity.md` points here for everything after v0.188.
