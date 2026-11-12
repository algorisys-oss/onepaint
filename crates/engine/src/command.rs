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
    /// Put a layer or group in a new group in its place.
    GroupLayer {
        id: NodeId,
    },
    /// Lift a group's children into its place.
    Ungroup {
        id: NodeId,
    },
    /// The visible image on a new layer above the active one.
    MergeVisible,
    /// Every layer replaced by one of what shows.
    FlattenImage,
    InvertMask {
        id: NodeId,
    },
    /// A mask that shows the whole layer, whatever is selected.
    AddRevealMask,
    /// An opaque colour over the whole layer, relief and off-canvas pixels
    /// gone (happypaint `fillLayerSolid`; Apply paper colour, spec 57).
    FillLayerSolid {
        id: NodeId,
        rgba: Rgba,
    },
    /// The layer's styles become its pixels (spec 56).
    BakeStyles {
        id: NodeId,
    },
    /// Every linked shadow and bevel takes this light (degrees; spec 56).
    ApplyGlobalLight {
        angle: f32,
        altitude: f32,
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
    /// The document paper (`None` = smooth).
    SetPaper {
        paper: Option<onepaint_doc::Paper>,
    },
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
    /// Exposure sheet (spec 24): flipbook → one track; retime and add sheets; tracks.
    ConvertToTracks,
    MoveSheet {
        id: NodeId,
        column: i64,
    },
    AddSheet {
        track: NodeId,
        column: usize,
    },
    AddTrack,
    DeleteTrack {
        id: NodeId,
    },
    MoveTrack {
        id: NodeId,
        delta: i32,
    },
    SetFrameHold {
        id: NodeId,
        hold: u32,
    },
    /// Batch retiming over a frame selection (spec 24); each is one step.
    SetFramesDuration {
        ids: Vec<NodeId>,
        hold: u32,
    },
    SplitFrames {
        ids: Vec<NodeId>,
        duration: u32,
    },
    AddInbetweens {
        ids: Vec<NodeId>,
    },
    DuplicateFrames {
        ids: Vec<NodeId>,
    },
    DeleteFrames {
        ids: Vec<NodeId>,
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
    /// Mixer Alt-click: load the brush from the canvas, the square `size`
    /// wide around (x, y) (document px) of the composite (`loadMixerPatch`).
    LoadMixerPatch {
        x: f64,
        y: f64,
        size: f64,
    },
    /// Drop a loaded patch: the next stroke loads the flat colour.
    ClearMixerPatch,
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
    /// The current tool's brush, edited in place (the angle dial): unlike
    /// `SetBrushPreset` it never switches between Brush and Eraser.
    SetCurrentBrush {
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
        /// Soft coverage along the edge (Photoshop's Anti-alias, happypaint
        /// v0.209.0) instead of a 0/255 staircase. Absent in older recordings,
        /// which replay hard-edged as they were drawn.
        #[serde(default)]
        antialias: bool,
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
    /// A watercolour wash over the region a tap picks, in the current colour
    /// (spec 58 C): it spreads out from the tap and dries like wet paint.
    WashFill {
        x: i32,
        y: i32,
        options: WashFillOptions,
    },
    /// Fill the selection with the current colour.
    FillSelection,
    /// Clear the active layer inside the selection (or entirely, with none).
    ClearSelected,
    /// Lift the active layer (or its selected part) for moving/transforming;
    /// with `ids`, those layers move as one (a group means its children).
    BeginTransform {
        #[serde(default)]
        ids: Vec<NodeId>,
    },
    /// Set the transform matrix (doc px → doc px) of the lifted content.
    SetTransform {
        matrix: transform::Affine,
    },
    CommitTransform,
    CancelTransform,
    /// Move layers by whole pixels as one undo step (align and distribute;
    /// happypaint `alignLayers`). Text and path layers stay vector.
    AlignLayers {
        moves: Vec<crate::align::LayerMove>,
    },
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

/// A wash fill's settings (happypaint `WashFillOptions`): the region
/// (tolerance, grow, sample all layers), water and pigment 0..1 (pigment
/// 0 = water only), how much the pigment thins with distance 0..1, the edge
/// softness in px and the front's speed in px per step.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WashFillOptions {
    pub tolerance: f32,
    pub grow: u32,
    pub sample_all: bool,
    pub water: f32,
    pub pigment: f32,
    pub grade: f32,
    pub soft: f32,
    pub speed: f32,
}
