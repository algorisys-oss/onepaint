# 48: View modes in the present pass

## Status
Built. Grayscale, print preview (CMYK soft-proof) and the gamut warning are
applied per screen pixel in `canvas.wgsl`. While any of them was on, every
tile used to composite on the CPU and was post-processed there. Now the
composite stays on the GPU path, and turning a mode on or off recomposites
nothing.

This is also how happypaint does it: its present shader applies them
(`cmyk::apply_view_with` cites it).

## Model
- `CanvasGpu::set_view_modes(device, queue, &ViewModes, &Press)` stores:
  - the flags;
  - the gamut threshold in LUT units (ΔE × 8);
  - the proof's black and white points in linear light (media-relative
    unless paper white or black ink is simulated).
- It uploads the press's 33³ proof LUT as an `Rgba8Unorm` 3D texture,
  rebuilt when the press changes (keyed on the LUT's address).
- `view_modes()` in the present shader follows `apply_view_with` step by
  step:
  - encode to premultiplied sRGB;
  - grayscale by Rec. 709 luma;
  - composite on white and look up the LUT with `cmyk::trilinear`'s own
    cell clamp;
  - normalise the print between the black and white points;
  - flag colours past the ΔE threshold in the warning red;
  - decode.
- The app passes its view modes and press in the paint callback.
  `set_view_modes` and `set_press` no longer mark tiles dirty, and warm-up
  runs as usual.
- `ONEPAINT_VIEW=gray|proof|warn` turns a mode on in a scripted run.

The modes now apply after the present pass's bilinear filter, not before
it. They match the CPU wherever a screen pixel is one document pixel (1:1)
and differ only in the blend between pixels when zoomed. The view isn't
exported, so this is safe.

## Acceptance criteria
1. At 1:1, for grayscale, proof (with and without paper white and black
   ink), the gamut warning, and all three together: the present pass on
   raw tiles matches CPU `apply_view` tiles presented plainly within 2/255
   on at least 99.9% of channels. Swapping two grayscale weights fails on
   165,000 channels.
2. The app tests, including Ctrl+Y and print preview, pass.
