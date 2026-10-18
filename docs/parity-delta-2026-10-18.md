# Parity delta: happypaint v0.201.7 → v0.204.7

What happypaint added after `parity-delta-2026-10-10.md`, and the order
onepaint ports it in. Fold each item into `feature-parity.md` as it lands.

## Decisions taken without the owner (2026-10-18)

- **Licence.** happypaint moved from AGPL-3.0-only to PolyForm
  Noncommercial 1.0.0 in v0.204.6. onepaint stays AGPL until its owner
  decides; nothing here depends on it.
- **Git.** Commit per milestone, do not push.
- **Build.** The machine has little free memory and disk: `cargo -j 2`,
  one crate's tests at a time.
- **Blend modes first.** The v0.200.1 modes were half done in the tree, so
  they landed before the v0.202 work (aaed348).

## Compositing and layers

- [x] The Photoshop blend modes and Pass Through groups (v0.200.1).
- [x] Knockout (v0.202.2): None / Shallow / Deep. A knocked-out layer punches
      through to the group's base (Shallow) or the document's (Deep); Deep
      inside a nested group stops at that group. PSD `knko` both ways,
      Properties select. Done: the CPU oracle (`crates/compose/tests/knockout.rs`),
      `Op::Knockout`/`Op::Snapshot` on the GPU (the oracle suites draw random
      knockouts), PSD `knko`, `LayerProps::knockout`, the Properties select.
- [ ] Mask density and feather, and Blend If (v0.202.0).
- [ ] Vector masks (v0.202.1): PSD `vmsk`/`vsms` both ways, live in the
      compositor, the Pen's Create Vector Mask, transforms map the knots.

## PSD

- [ ] `phfl` Photo Filter colour decode (v0.204.7): v3 holds D50 XYZ
      (1.0 = 32768); v2 Lab binary colours rescaled; also fixes Gradient Map
      stops stored as Lab or grey.

## Input

- [ ] A pointer pressed outside the canvas is remembered until it lifts and
      never "healed" into a stroke (v0.204.7, `pointer-router.ts`).

## UI

- [ ] Mix colour mode (v0.203.0, v0.204.7): 2–6 wells, rows × cols grid,
      OKLab or RGB mixing, save as palette.
- [ ] Side sliders anywhere (v0.204.0): edges, floating, with the tools; a
      typed value entry from the end icon (v0.204.7).
- [ ] Align acts on the active layer when nothing is multi-selected
      (v0.204.1).
- [ ] Tooltips on tool icons (v0.202.3), Save / Save As (v0.204.2), the
      header as a title bar (v0.204.3): check against onepaint's chrome.
