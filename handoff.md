# Handoff

Last updated 2026-10-18. Read this first when picking the work back up,
then `docs/parity-delta-2026-10-18.md` (this run's list and decisions),
`todo.md` and `AGENTS.md`. **Keep this file current after every step**, so a
dropped session can resume from it.

## Current run: happypaint v0.201.7 → v0.204.7 parity (2026-10-18)
Rules for the run: commit locally per milestone, **do not push**; test with
`RAYON_NUM_THREADS=2 cargo test -p X -j 2 -- --test-threads=2` (the user saw
10 GB / all cores otherwise); build with
`cargo -j 2` and test one crate at a time (RAM ~3 GB free, disk 95%).

Order (✓ done, ▶ in progress):
1. ✓ Photoshop blend modes + Pass Through groups — `aaed348`.
2. ✓ Knockout (v0.202.2) — committed.
3. ✓ Mask density/feather + Blend If (v0.202.0) — c1c2d56 and the feather commit.
4. ✓ Vector masks (v0.202.1) — committed.
5. ✓ PSD `phfl` blocked on the Photo Filter kind (todo); pointer fix not applicable (no heal).
6. ✓ UI: Mix colour grid, side sliders + typed values, Align on active layer.

### Step 2, Knockout — done
Semantics (happypaint `compositor.ts` `knockoutPunch`): before a knocked-out
layer draws, `target = mix(target, reveal, content.a · opacity)`. Reveal =
the Background snapshot at the top level, a Pass Through group's backdrop,
or transparency in an isolated group. Stored in `extra["knockout"]`.
Oracle `crates/compose/tests/knockout.rs`; GPU `OP_KNOCKOUT`/`OP_SNAPSHOT`;
PSD `knko`; `LayerProps::knockout`; Properties select.

### Step 3, mask density/feather + Blend If — state
Spec (happypaint `look-meta.ts`, `compositor.ts` MASKMUL :72, BLEND :375,
`lookMask` :1668, `storage/psd/mask-params.ts`; docs/graphics/01 :307-334):
- Layer-only fields `maskDensity` (0..1, absent=1), `maskFeather` (doc px,
  absent=0), `blendIf` ([gray,R,G,B] of {src,dst} [bl,bh,wl,wh] 0..255,
  absent=full [0,0,255,255]); omitted when default in `.happypaint`.
- Mask: feather first = Gaussian σ = feather px (clamp to edge), then
  `m' = 1 − d·(1 − m)`. Density doesn't touch the vector mask.
- Blend If: on sRGB straight colour (doc converted), values rounded to
  0..255, gray = .3R+.59G+.11B; weight = Π over gray,R,G,B of
  w(src,Cs)·w(dst,Cb); w = 0 outside [bl,wh], ramps (v−bl)/(bh−bl) and
  (wh−v)/(wh−wl). Multiplies src alpha (after opacity/mask, after
  dissolve) before blending. Transparent backdrop → Cb = 0.
  Ignored for clipped riders, clipped adjustments, inherit-alpha, groups;
  a clip family's base applies it to the whole family; unclipped
  adjustments honour it. Knockout punch ignores it. Merge Down honours all.
- PSD: mask body byte 17 flags 0x10 → byte 18 `which`, then bit0 density u8,
  bit1 feather f64 BE (bit2/3 vector ones). Blend ranges: 8 bytes/channel
  (src4,dst4), gray first; RGB only. Tests in `mask-params.test.ts`.
- UI (Properties, under Mask): Density 0–100 %, Feather 0–250 px step .5;
  Blend If: channel select, Split, This Layer / Underlying Layer bars, Reset.

Split into 3a (density + Blend If, per tile) and 3b (feather, spatial).
3a progress (uncommitted):
- ✓ `doc::look` (`BlendIf`, `ChannelRanges`, `mask_k`, Layer accessors, serde).
- ✓ compose oracle (`blend_if_weight`, `blend_px_if`; tests `tests/look.rs`).
- ✓ program: `Content::density`, `blend_if` on Layer/Pop/Adjust ops, Adjust
  mask is a `MaskSrc`; Blend If units don't fold or occlude. testdocs adds
  random looks (mutation check: 8 program tests fail without Blend If).
- ✓ GPU: mode word `mode | (bif+1) << 8`, ranges in the adj table, density
  in `d.w`. A Blend If unit at/above the hot one disables the split (its
  steps flipped on the f16 prefix: 27–36 px over). All GPU suites green;
  `lod.rs` drops Blend If like the other tone steps.
- ✓ PSD mask params + blend ranges (writer/reader + test).
- ✓ Engine LayerProps mask_density/mask_feather/blend_if (+ test).
- ✓ App test `blend_if_unfolds_and_reset_clears_it`.
- 3a committed (c1c2d56). 3b feather: `compose::feather` wired into the
  oracle and planner (PlanCache, Weak-keyed), unfeathered while a mask
  stroke runs, engine `mark_dirty` pads by the largest reach, Feather slider
  back. All suites green; committed.
- ✓ UI: Density slider, Blend If section (`ui_kit::range_bar`). Feather
  slider held back until 3b renders it (code in scratchpad feather_ui.rs:
  slider 0–250 px step .5, tooltip "Soften the mask's edges, without
  changing the mask (Photoshop's Feather)").
- TODO: range bar keyboard (arrows ±1, Shift ±10) — not done.

### Step 4 done; Blend If bar keys done (arrows ±1, Shift ±10).

### Run status (2026-10-18, evening)
All six steps are done and committed locally (not pushed): 4e2116f Knockout,
c1c2d56 density + Blend If, 9bad5ed feather, f19e6d5 vector masks,
382840d Align, a442802 Mix grid, 0945a10 side sliders. Open: PSD `phfl`
(needs the Photo Filter adjustment, `todo.md` new adjustments), the
title-bar workspace decision, the licence question (delta doc).

### Done: Exposure, Vibrance, Photo Filter, Channel Mixer (2026-10-18, committed)
PSD live adjustments done too (levl curv hue2 blnc blwh grdm in
`formats::psd_adjust`). Next: untouched PSD round trips need a plan for the
user (keep original records), then perf (8-bit atlases,
spatial adjustments on the GPU), then photocraft robustness items.
Pushed 14e05aa first. Spec (happypaint `adjustment-renderer.ts` :1048-1118,
:2370-2401, `psd-map.ts` :206-234/:357-381, `psd/adjust-blocks.ts`):
all run on sRGB-encoded 0..1 (toWork), input clamped.
- exposure {exposure 0, expOffset 0, expGamma 1}: lin = srgb2lin(rgb);
  lin = pow(max(lin·2^exp + off, 0), 1/max(.01,gamma)); o = lin2srgb(clamp01).
- vibrance {vibrance 0, vibSaturation 0}: k = vib·(1 − (max−min));
  rgb = mix(lum601, rgb, 1+k); lum601 of clamped; rgb = mix(lum, rgb, 1+sat).
- photo-filter {filterColor [236,138,0]/255, filterDensity .25, preserveLum
  true}: o = mix(rgb, rgb·f, d); keepLum → setLum(o, lum(.3,.59,.11) of rgb)
  with ClipColor. Identity when density ≤ 0.
- channel-mixer {mixRR/GG/BB 1, rest 0, mixMono false, mixKR .4 KG .4 KB .2
  KC 0}: o = clamp(M·rgb + C); mono uses the K row for all three.
- UI fields: Exposure −20..20 (.01), Offset −.5...5 (.001), Gamma .01..9.99;
  Vibrance/Saturation −100..100 %; Colour, Density 0..100 %, Preserve
  luminosity; Monochrome, "Red ← Red"… "Red constant" −200..200 %.
- PSD: expA, vibA, phfl (v3 XYZ/32768 via MI, v2 colour), mixr.

## Earlier session (2026-10-10, evening, unattended)
Items 1–5 from the earlier list; every step pushed as it landed.

- **Spec 40, composite sheets** (`gpu::sheets`): the canvas composite and
  the selection are texture arrays of sheets.
  - A document within the GPU texture limit is one sheet, and nothing
    changes for it.
  - Past the limit, each axis is cut into 256-aligned sheets.
  - The present pass filters across seams by hand.
  - The app now asks for the adapter's own texture limits. egui asks for
    8192, so documents from 8193 to 16384 px had been blank.
  - `ONEPAINT_MAX_SHEET=256` shows sheets on small documents.
- **Spec 41, opening in the background** (`apps/onepaint/src/loading.rs`):
  - `.happypaint`, PSD and images decode on a worker thread, with a
    progress card and Cancel after 150 ms;
  - `formats::Progress` carries the progress and the cancel flag;
  - `.happypaint` tiles decode in parallel: 10.7 s down to 3.6 s for a
    328 MB file;
  - books still open on the UI thread.
- **GPU fallbacks moved off the CPU**, each checked against the CPU oracle,
  with a mutation that the test catches:
  - spec 42: 13 colour adjustment kinds (`filters::gpu` packs them,
    `Op::Adjust`);
  - spec 43: paper shading (`Op::Paper`, the grain as an R32Float
    texture);
  - spec 44: partial panel clips (`Op::Coverage`, cached across frames in
    the session's `PlanCache`);
  - spec 45: impasto relief (the height tiles of the tile and its four edge
    neighbours);
  - spec 46: noise, screentone (not the "sand" shape), solid, gradient
    and built-in pattern fills. With the session's `PlanCache`, each
    adjustment is prepared once per layer and shared by every tile;
  - spec 48: grayscale, print preview and the gamut warning, applied in the
    present pass with the proof LUT as a 3D texture (as happypaint does).
    They no longer force the CPU path or recomposite when toggled.
  - Spec 46 also adds a rule: a tile with an adjustment layer at or above
    the hot layer is not split. The f16 cached image under it, amplified by
    hue near grey, threshold or levels, was visibly off.
- **Spec 47:** the planner skips layers under a fully opaque normal tile,
  exactly, using an opacity flag cached per tile.
- **Segment cache at 384 MiB** (it was 256): a 205-tile drag had been
  refilling 76 segments a frame. On the quiet machine the split drag frame
  went from 54 to 16.5 ms, and to 14.5 ms back to back (spec 38).
- **Still on the CPU:** imported pattern images, sand screentone, spatial
  adjustment layers and ghost pegs (`todo.md` §2.1). The last two need a
  multi-pass design: a padded backdrop pass, and intermediate images
  resampled through the peg transform.
- **A review of the evening's commits** found four bugs, all fixed with
  tests where possible:
  - warm-up filled relief segments with a 1 × 1 document size;
  - a background load could land on a document opened after it started;
  - the proof LUT and paper grain textures were cached by an address
    that could be reused.
- **8-bit documents (the user chose A, then B):**
  - spec 49 stores them as 8-bit tiles, half the CPU memory;
  - spec 50 makes them composite in gamma space like happypaint, with
    `color::Space` at every boundary;
  - the New Canvas dialog offers 8-bit, 16-bit float (the default) and
    32-bit float;
  - `ONEPAINT_NEW=WxH@8` starts an 8-bit page.
- **Debug hooks:** `ONEPAINT_NEW=WxH` (a blank document of that size;
  16384 × 12000 checked, one sheet and 4 × 3) and
  `ONEPAINT_VIEW=gray|proof|warn`.
- **Item 1, quiet measurements** (specs 38 and 39 have them):
  - the split drag frame was 54 ms against 144 ms unsplit. That exposed a
    segment-cache thrash; with the cache at 384 MiB it is **16.5 ms**,
    under the 60 fps budget even at the low clock;
  - level 3 was 6.2 ms;
  - the iGPU sat at 500–650 MHz, because the governor doesn't raise the
    clock while the bench waits after every frame. So they are relative,
    and the earlier 19.7 ms was taken at a higher clock. Absolute numbers
    need the clock pinned (root) or a bench that keeps the GPU busy.
- **Item 3, 8-bit storage:** waiting for your decision (below).

## Next up (suggested order)
1. **8-bit atlases on the GPU**, so 8-bit documents also halve VRAM.
2. **Spatial adjustment layers on the GPU** (a padded backdrop pass), then
   ghost pegs.
3. **The rest of the photocraft list** (`todo.md` §2.1):
   - working-set checks before a full refresh;
   - bounded submits and device-loss handling;
   - a perf-budget gate;
   - skipping layers under opaque tiles.
4. **Vector support** (`todo.md` §3.1): discuss its scope first, then write
   a spec.

## Open decisions for the user
- The GPU memory budget on integrated GPUs: 1 GiB of source tiles plus
  **384 MiB** of segment cache (raised from 256 MiB tonight), both counted
  with mips. 256 MiB thrashed on a 205-tile drag; 384 MiB took the split
  drag frame from 54 ms to 16.5 ms (spec 38).
- Whether to drop the CPU tile copy once tiles are resident on the GPU.
- Vector support: its scope, and when it comes relative to the rest.

## Gotchas
- The disk is about 95% full (26 GB free on 2026-10-18). Run `cargo clean` if
  free space drops below 8 GB. RAM is tight: build with `-j 2`.
- GPU tests and benches skip silently when no adapter is present. Check
  for the "skipping" line before trusting a green run.
- **GPU timings are noisy here.** Browsers share the iGPU and its clock
  drops. Compare variants in the same run (the bench trace alternates
  them) and quote medians.
- `PIGMENT_WGSL` (in `gpu::wet`) is shared by the wet and compose shaders.
- **Debug scenes** (`apps/onepaint/src/debug.rs`): `ONEPAINT_DOCK=1|float`
  (docked or floating panels), `ONEPAINT_PANELS=1` (a manga page), plus the
  older hooks. `ONEPAINT_CPU_COMPOSE=1` forces the CPU compositor, which is
  useful for diffing screenshots.
- **A locked screen stalls the app on Wayland:** it gets no frames, so
  screenshot runs hang. Run them with `env -u WAYLAND_DISPLAY` (X11).
- Commit messages carry no AI attribution.
