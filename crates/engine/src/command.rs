//! The engine's command set: everything the UI can ask for, serialisable
//! so it doubles as the replay and scripting format.

use super::*;

/// Everything the UI can ask the engine to do (serialisable for replay).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Command {
    AddLayer,
    AddGroup,
    DeleteLayer {
        id: NodeId,
    },
    DuplicateLayer {
        id: NodeId,
    },
    MoveLayer {
        id: NodeId,
        parent: Option<NodeId>,
        index: usize,
    },
    SetActiveLayer {
        id: NodeId,
    },
    SetLayerProps {
        id: NodeId,
        props: LayerProps,
    },
    MergeDown {
        id: NodeId,
    },
    ClearLayer {
        id: NodeId,
    },
    /// Bevel & Emboss: relief derived from the layer's alpha (spec 35).
    BevelFromAlpha {
        id: NodeId,
        depth: f64,
        softness: f64,
    },
    /// Drop the layer's relief (`clearBevel`).
    ClearBevel {
        id: NodeId,
    },
    /// Settle all wet paint now and commit it (the Dry button).
    DryWet,
    /// Canvas tilt for wet paint (direction × strength, each -1..1).
    SetWetTilt {
        x: f32,
        y: f32,
    },
    SetTool {
        tool: Tool,
    },
    SetSymmetry {
        symmetry: Symmetry,
    },
    /// Replace the drawing guides (grid, symmetry, perspective…).
    SetGuides {
        guides: Box<Guides>,
    },
    /// Change the guide lines live; `CommitGuideLines` makes it one undo step.
    EditGuideLines {
        lines: Vec<guides::GuideLine>,
    },
    CommitGuideLines,
    SaveSelection {
        name: String,
    },
    DeleteSavedSelection {
        id: String,
    },
    LoadSelection {
        id: String,
        #[serde(default)]
        mode: onepaint_selection::Mode,
    },
    CreatePathLayer {
        spec: Box<onepaint_stroke::bezier::PathSpec>,
    },
    AddPathObject {
        id: NodeId,
        spec: Box<onepaint_stroke::bezier::PathSpec>,
    },
    /// Replace one path object (`None` removes it); live edits coalesce.
    UpdatePathObject {
        id: NodeId,
        index: usize,
        spec: Option<Box<onepaint_stroke::bezier::PathSpec>>,
    },
    RasterizePath {
        id: NodeId,
    },
    SelectFromPath {
        #[serde(default)]
        mode: onepaint_selection::Mode,
    },
    FillPath,
    /// End live-edit coalescing: the next edit is a new undo step.
    SealHistory,
    /// Brush fill mode: fill a freehand silhouette (doc px) at `opacity`.
    FillShape {
        points: Vec<[f64; 2]>,
        opacity: f32,
    },
    /// Switch the transform in progress to 4-corner distort.
    BeginDistort,
    /// Switch the transform in progress to mesh warp.
    BeginWarp,
    SetWarp {
        warp: transform::WarpMode,
    },
    /// Replace a layer's smart-filter stack (live edits coalesce; seal to end).
    SetSmartFilters {
        id: NodeId,
        filters: Vec<smart_filter_ops::SmartFilter>,
    },
    AddSmartFilter {
        id: NodeId,
        kind: String,
        params: onepaint_filters::Params,
    },
    BakeSmartFilters {
        id: NodeId,
    },
    SetAnimation {
        settings: Box<anim::AnimSettings>,
    },
    SetAnimSlot {
        slot: usize,
    },
    SetAnimPlaying {
        playing: bool,
    },
    AddFrame,
    SetFrameHold {
        id: NodeId,
        hold: u32,
    },
    /// `background`, `foreground`, or `None` for a normal frame.
    SetFrameRole {
        id: NodeId,
        role: Option<String>,
    },
    StrokePath {
        tool: path_ops::StrokePathTool,
        simulate_pressure: bool,
    },
    MaskFromPath,
    SetSmudgeStrength {
        strength: f64,
    },
    SetMixer {
        mixer: MixerSettings,
    },
    CleanMixer,
    SetLiquify {
        mode: onepaint_filters::liquify::LiquifyMode,
        strength: f64,
    },
    SetBrush {
        preset_id: String,
    },
    /// Replace the whole current preset (the brush studio's live edit);
    /// an eraser preset goes to the eraser tool, like `SetBrush`.
    SetBrushPreset {
        preset: Box<BrushPreset>,
    },
    SetBrushSize {
        size: f64,
    },
    SetBrushOpacity {
        opacity: f64,
    },
    SetBrushFlow {
        flow: f64,
    },
    SetColor {
        rgba: Rgba,
    },
    Undo,
    Redo,
    /// Select the union of closed contours (doc px), combined by `mode`.
    SelectShape {
        contours: Vec<Vec<[f64; 2]>>,
        mode: onepaint_selection::Mode,
    },
    /// Magic wand.
    SelectByColor {
        x: i32,
        y: i32,
        tolerance: f32,
        mode: onepaint_selection::Mode,
        contiguous: bool,
        sample_all: bool,
    },
    SelectAll,
    Deselect,
    InvertSelection,
    /// Grow (positive) or shrink (negative) by whole pixels.
    GrowSelection {
        delta: i32,
    },
    SmoothSelection {
        px: f32,
    },
    /// Live, non-destructive feather of the clip mask (not undoable).
    SetFeather {
        radius: f32,
    },
    /// Paint bucket on the active layer.
    Fill {
        x: i32,
        y: i32,
        tolerance: f32,
        grow: u32,
    },
    /// Fill the selection with the current colour.
    FillSelection,
    /// Clear the active layer inside the selection (or entirely, with none).
    ClearSelected,
    /// Lift the active layer (or its selected part) for moving/transforming.
    BeginTransform,
    /// Set the transform matrix (doc px → doc px) of the lifted content.
    SetTransform {
        matrix: transform::Affine,
    },
    CommitTransform,
    CancelTransform,
    /// Selection to a new layer (Ctrl+J), or cut to a new layer (Ctrl+Shift+J).
    LayerViaCopy {
        cut: bool,
    },
    /// Crop/extend the canvas; content moves by (dx, dy).
    ResizeCanvas {
        width: u32,
        height: u32,
        dx: i32,
        dy: i32,
    },
    /// Resample the whole image to a new size.
    ScaleImage {
        width: u32,
        height: u32,
    },
    /// Start a destructive filter on the active layer (live preview).
    BeginFilter {
        kind: String,
    },
    UpdateFilter {
        params: onepaint_filters::Params,
    },
    CommitFilter,
    CancelFilter,
    AddAdjustmentLayer {
        kind: String,
        params: onepaint_filters::Params,
    },
    SetAdjustment {
        id: NodeId,
        params: onepaint_filters::Params,
    },
    /// Add a mask to the active layer: white, or the selection's coverage.
    AddMask,
    DeleteMask {
        id: NodeId,
    },
    /// Bake the mask into the pixels, then remove it.
    ApplyMask {
        id: NodeId,
    },
    SetMaskEnabled {
        id: NodeId,
        enabled: bool,
    },
    /// Paint into the active layer's mask (true) or its pixels (false).
    SetMaskTarget {
        on: bool,
    },
    /// Organisational colour label (red, orange, yellow, green, blue, violet, gray) or none.
    SetColorLabel {
        id: NodeId,
        label: Option<String>,
    },
    /// Add a text object to the active text layer (or a new "Text" layer).
    AddText {
        spec: onepaint_text::TextSpec,
    },
    UpdateText {
        id: NodeId,
        index: usize,
        spec: onepaint_text::TextSpec,
    },
    DeleteText {
        id: NodeId,
        index: usize,
    },
    /// Turn a text layer into plain pixels.
    RasterizeText {
        id: NodeId,
    },
    /// Lay out a page of `rows × cols` panels (spec 30).
    CreatePanelGrid {
        rows: u32,
        cols: u32,
        gutter: f64,
        margin: f64,
        border: f64,
    },
    /// One panel from a drag rectangle (doc px).
    CreatePanel {
        a: [f64; 2],
        b: [f64; 2],
        gutter: f64,
        border: f64,
    },
    /// Cut panel `id` along the line a→b.
    SplitPanel {
        id: NodeId,
        a: [f64; 2],
        b: [f64; 2],
    },
    /// Replace a panel's frame; live edits coalesce until `SealHistory`.
    SetPanelOutline {
        id: NodeId,
        outline: Vec<[f64; 2]>,
    },
    /// Gutter and/or border width of one panel, or every panel (`id: None`);
    /// live edits coalesce until `SealHistory`.
    SetPanelStyle {
        #[serde(default)]
        id: Option<NodeId>,
        #[serde(default)]
        gutter: Option<f64>,
        #[serde(default)]
        border: Option<f64>,
    },
}
