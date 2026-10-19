//! Replaying happypaint recordings (`.happypaint-rec`, spec 62): each
//! recorded command (happypaint `paint-engine.ts` `applyCommand`) mapped
//! onto onepaint's commands and strokes, with happypaint's layer ids mapped
//! to the ones onepaint creates. Commands onepaint has no counterpart for
//! are skipped and counted.

use crate::recording::{RecEvent, Replay};
use crate::{Command, EngineError, LayerProps, Session, Tool, WashFillOptions};
use onepaint_brush::BrushPreset;
use onepaint_color::Rgba;
use onepaint_filters::gradient::{GradStop, Shape};
use onepaint_stroke::StrokePoint;
use serde_json::Value;

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

/// A happypaint `StampPoint` (tilt −1..1, twist in degrees, t in ms).
fn point(v: &Value) -> Option<StrokePoint> {
    Some(StrokePoint {
        x: num(v, "x")?,
        y: num(v, "y")?,
        pressure: num(v, "pressure").unwrap_or(1.0),
        tilt_x: num(v, "tiltX").unwrap_or(0.0),
        tilt_y: num(v, "tiltY").unwrap_or(0.0),
        twist: num(v, "twist").unwrap_or(0.0),
        t: num(v, "t"),
    })
}

fn points(v: &Value) -> Vec<StrokePoint> {
    v.as_array().into_iter().flatten().filter_map(point).collect()
}

fn xy(v: &Value) -> Option<[f64; 2]> {
    Some([num(v, "x")?, num(v, "y")?])
}

/// A happypaint `Rgb` (sRGB 0..1).
fn rgb(v: &Value) -> Option<Rgba> {
    let c = v.as_array()?;
    let at = |i: usize| c.get(i).and_then(Value::as_f64).map(|x| x as f32);
    Some(Rgba::new(at(0)?, at(1)?, at(2)?, 1.0))
}

fn sel_mode(v: &Value) -> onepaint_selection::Mode {
    use onepaint_selection::Mode;
    match v.as_str() {
        Some("add") => Mode::Add,
        Some("subtract") => Mode::Subtract,
        Some("intersect") => Mode::Intersect,
        _ => Mode::Replace,
    }
}

fn shape(v: Option<&str>) -> Shape {
    v.and_then(Shape::parse).unwrap_or_default()
}

/// A recorded brush (happypaint's effective `BrushSettings`) as a preset.
fn brush(v: &Value) -> Option<BrushPreset> {
    let mut b = v.clone();
    let o = b.as_object_mut()?;
    o.entry("id").or_insert_with(|| Value::String("replay".into()));
    o.entry("name").or_insert_with(|| Value::String("Replay".into()));
    o.entry("category").or_insert_with(|| Value::String("Replay".into()));
    serde_json::from_value(b).ok()
}

impl Session {
    /// The onepaint id of a happypaint layer id (the start layers keep theirs).
    fn hid(&self, id: &str) -> String {
        self.happy_ids.get(id).cloned().unwrap_or_else(|| id.to_string())
    }

    fn hid_of(&self, args: &Value, k: &str) -> Option<String> {
        str_of(args, k).map(|i| self.hid(i))
    }

    /// Note a layer the last command created under happypaint's id.
    fn adopt(&mut self, args: &Value, k: &str) {
        if let Some(h) = str_of(args, k) {
            self.happy_ids.insert(h.to_string(), self.active.clone());
        }
    }

    fn run(&mut self, cmd: Command) {
        let _ = self.execute(cmd);
    }

    /// The current tool's brush, the colour, then the stroke's first point.
    fn begin_with(&mut self, tool: Tool, preset: Option<BrushPreset>, color: Option<Rgba>, first: Option<StrokePoint>) {
        self.run(Command::SetTool { tool });
        if let Some(b) = preset {
            self.run(Command::SetCurrentBrush { preset: Box::new(b) });
        }
        if let Some(rgba) = color {
            self.run(Command::SetColor { rgba });
        }
        if let Some(p) = first {
            self.stroke_begin(p);
        }
    }

    fn props(&mut self, id: Option<String>, props: LayerProps) {
        if let Some(id) = id {
            self.run(Command::SetLayerProps { id, props });
        }
    }

    /// Flip one of a layer's flags.
    fn toggle(&mut self, args: &Value, f: impl Fn(&onepaint_doc::Layer) -> LayerProps) {
        let Some(id) = self.hid_of(args, "id") else { return };
        let Some(p) = self.doc.layer(&id).map(f) else { return };
        self.run(Command::SetLayerProps { id, props: p });
    }

    /// Apply one happypaint command; `false` when onepaint can't.
    pub(crate) fn apply_happy(&mut self, op: &str, a: &Value) -> bool {
        match op {
            // ── Strokes ──
            "beginStroke" => {
                let erase = a.get("erase").and_then(Value::as_bool).unwrap_or(false);
                let tool = if erase { Tool::Eraser } else { Tool::Brush };
                self.begin_with(tool, a.get("brush").and_then(brush), a.get("color").and_then(rgb), a.get("first").and_then(point));
            }
            "continueStroke" | "continueWet" | "continueSmudge" | "continueLiquify" => {
                let pts = points(a.get("points").unwrap_or(&Value::Null));
                self.stroke_extend(&pts);
            }
            "endStroke" | "endWet" | "endSmudge" | "endLiquify" => self.stroke_end(),
            // Live wet media: a live brush of the recorded load, size and tip.
            "beginWet" => {
                let fluid = a.get("fluid").cloned().unwrap_or(Value::Null);
                let oil = fluid.get("oil").and_then(Value::as_bool).unwrap_or(false);
                let mut b = onepaint_brush::preset(if oil { "oil-paint-live" } else { "watercolor-live" })
                    .or_else(|| onepaint_brush::preset("watercolor-live"));
                if let Some(b) = b.as_mut() {
                    b.size = num(a, "size").unwrap_or(b.size);
                    b.extra.insert("fluid".into(), fluid);
                    if let Some(i) = num(a, "impasto").filter(|v| *v > 0.0) {
                        b.extra.insert("impasto".into(), serde_json::json!({ "depth": i }));
                    }
                    if let Some(t) = a.get("tip") {
                        b.tip_id = str_of(t, "tipId").unwrap_or(&b.tip_id).to_string();
                        b.tip_roundness = num(t, "roundness");
                        b.tip_angle_deg = num(t, "angleDeg");
                    }
                }
                self.begin_with(Tool::Brush, b, a.get("color").and_then(rgb), a.get("first").and_then(point));
            }
            "wetTick" => self.wet_steps(num(a, "steps").unwrap_or(0.0) as usize),
            "dryWet" => self.run(Command::DryWet),
            "wetTilt" => self.run(Command::SetWetTilt { x: num(a, "x").unwrap_or(0.0) as f32, y: num(a, "y").unwrap_or(0.0) as f32 }),
            "setPaper" => {
                let paper = a.get("paper").and_then(|p| serde_json::from_value(p.clone()).ok());
                self.run(Command::SetPaper { paper });
            }
            "beginWashFill" => {
                let (Some(p), Some(o)) = (a.get("point").and_then(xy), a.get("opts")) else { return false };
                if let Some(rgba) = a.get("color").and_then(rgb) {
                    self.run(Command::SetColor { rgba });
                }
                let f = |k: &str, d: f64| num(o, k).unwrap_or(d) as f32;
                let options = WashFillOptions {
                    tolerance: f("tolerance", 0.1),
                    grow: f("grow", 0.0).round() as u32,
                    sample_all: o.get("sampleAll").and_then(Value::as_bool).unwrap_or(false),
                    water: f("water", 0.6),
                    pigment: f("pigment", 0.7),
                    grade: f("grade", 0.0),
                    soft: f("soft", 0.0),
                    speed: f("speed", 10.0),
                };
                self.run(Command::WashFill { x: p[0].floor() as i32, y: p[1].floor() as i32, options });
            }
            "beginSmudge" => {
                self.run(Command::SetTool { tool: Tool::Smudge });
                if let Some(s) = num(a, "strength") {
                    self.run(Command::SetSmudgeStrength { strength: s });
                }
                if let Some(s) = num(a, "size") {
                    self.run(Command::SetBrushSize { size: s });
                }
                if let Some(p) = a.get("first").and_then(point) {
                    self.stroke_begin(p);
                }
            }
            "beginLiquify" => {
                self.run(Command::SetTool { tool: Tool::Liquify });
                if let Some(mode) = a.get("mode").and_then(|m| serde_json::from_value(m.clone()).ok()) {
                    self.run(Command::SetLiquify { mode, strength: num(a, "strength").unwrap_or(0.5) });
                }
                if let Some(s) = num(a, "size") {
                    self.run(Command::SetBrushSize { size: s });
                }
                if let Some(p) = a.get("point").and_then(point) {
                    self.stroke_begin(p);
                }
            }
            "cleanMixer" => self.run(Command::CleanMixer),
            // ── Fills ──
            "floodFill" => {
                let Some(p) = a.get("point").and_then(xy) else { return false };
                if let Some(rgba) = a.get("color").and_then(rgb) {
                    self.run(Command::SetColor { rgba });
                }
                let (tolerance, grow) = (num(a, "tolerance").unwrap_or(0.1) as f32, num(a, "grow").unwrap_or(0.0) as u32);
                self.run(Command::Fill { x: p[0].floor() as i32, y: p[1].floor() as i32, tolerance, grow });
            }
            "fillShape" => {
                if let Some(rgba) = a.get("color").and_then(rgb) {
                    self.run(Command::SetColor { rgba });
                }
                let pts: Vec<[f64; 2]> = a.get("points").and_then(Value::as_array).into_iter().flatten().filter_map(xy).collect();
                self.run(Command::FillShape { points: pts, opacity: num(a, "opacity").unwrap_or(1.0) as f32 });
            }
            "gradient" => {
                let (Some(pa), Some(pb)) = (a.get("a").and_then(xy), a.get("b").and_then(xy)) else { return false };
                let stops: Vec<GradStop> = match a.get("stops").and_then(Value::as_array) {
                    Some(s) => s
                        .iter()
                        .filter_map(|s| {
                            let c = rgb(s.get("color")?)?;
                            Some(GradStop {
                                pos: num(s, "pos")?,
                                color: [c.r, c.g, c.b].map(f64::from),
                                opacity: num(s, "opacity").unwrap_or(1.0),
                                mid: num(s, "mid"),
                            })
                        })
                        .collect(),
                    // Before v0.188.0: {color, color2}, no color2 = fade out.
                    None => {
                        let c = a.get("color").and_then(rgb).unwrap_or(Rgba::BLACK);
                        let c2 = a.get("color2").and_then(rgb);
                        let at = |c: Rgba| [c.r, c.g, c.b].map(f64::from);
                        vec![
                            GradStop { pos: 0.0, color: at(c), opacity: 1.0, mid: None },
                            GradStop { pos: 1.0, color: at(c2.unwrap_or(c)), opacity: if c2.is_some() { 1.0 } else { 0.0 }, mid: None },
                        ]
                    }
                };
                let mut stops = stops;
                stops.sort_by(|x, y| x.pos.total_cmp(&y.pos));
                if let Some(o) = num(a, "opacity") {
                    self.run(Command::SetBrushOpacity { opacity: o });
                }
                self.gradient_begin();
                self.gradient_update(pa, pb, shape(str_of(a, "shape")), &stops);
                self.gradient_end();
            }
            "fillLayerSolid" => {
                let (Some(id), Some(rgba)) = (self.hid_of(a, "id"), a.get("color").and_then(rgb)) else { return false };
                self.run(Command::FillLayerSolid { id, rgba });
            }
            // ── Layers ──
            "addLayer" => {
                self.run(Command::AddLayer);
                self.adopt(a, "id");
            }
            "deleteLayer" => {
                if let Some(id) = self.hid_of(a, "id") {
                    self.run(Command::DeleteLayer { id });
                }
            }
            "clearLayer" => {
                if let Some(id) = self.hid_of(a, "id") {
                    self.run(Command::ClearLayer { id });
                }
            }
            "setActiveLayer" => {
                if let Some(id) = self.hid_of(a, "id") {
                    self.run(Command::SetActiveLayer { id });
                }
            }
            "duplicateLayer" => {
                let Some(id) = self.hid_of(a, "id") else { return false };
                self.run(Command::DuplicateLayer { id });
                if a.get("newId").is_some() {
                    self.adopt(a, "newId");
                } else if let Some(first) = a.get("newIds").and_then(Value::as_array).and_then(|l| l.first()).and_then(Value::as_str) {
                    self.happy_ids.insert(first.to_string(), self.active.clone());
                }
            }
            "moveNodeTo" => {
                let Some(id) = self.hid_of(a, "id") else { return false };
                let parent = self.hid_of(a, "parentId");
                self.run(Command::MoveLayer { id, parent, index: num(a, "index").unwrap_or(0.0) as usize });
            }
            "moveLayer" => {
                // One step up or down among its siblings.
                let Some(id) = self.hid_of(a, "id") else { return false };
                let delta = num(a, "delta").unwrap_or(0.0) as i64;
                let (Some(parent), Some(index)) = (self.doc.parent_of(&id), self.doc.path_of(&id).and_then(|p| p.last().copied())) else {
                    return false;
                };
                let to = (index as i64 + delta).max(0) as usize;
                self.run(Command::MoveLayer { id, parent, index: to });
            }
            "mergeDown" => {
                if let Some(id) = self.hid_of(a, "id") {
                    self.run(Command::MergeDown { id });
                }
            }
            "setLayerProps" => {
                let id = self.hid_of(a, "id");
                let p = a.get("props").cloned().unwrap_or(Value::Null);
                if let Some(l) = p.get("colorLabel")
                    && let Some(i) = id.clone()
                {
                    self.run(Command::SetColorLabel { id: i, label: l.as_str().map(String::from) });
                }
                let props = LayerProps {
                    name: str_of(&p, "name").map(String::from),
                    visible: p.get("visible").and_then(Value::as_bool),
                    opacity: num(&p, "opacity").map(|v| v as f32),
                    blend_mode: str_of(&p, "blendMode").map(onepaint_doc::BlendMode::parse),
                    locked: p.get("locked").and_then(Value::as_bool),
                    ..Default::default()
                };
                self.props(id, props);
            }
            "toggleClipped" => self.toggle(a, |l| LayerProps { clipped: Some(!l.clipped), ..Default::default() }),
            "toggleAlphaLock" => self.toggle(a, |l| LayerProps { alpha_lock: Some(!l.alpha_lock), ..Default::default() }),
            "addMask" => {
                if let Some(id) = self.hid_of(a, "id") {
                    self.run(Command::SetActiveLayer { id });
                }
                self.run(Command::AddMask);
            }
            "deleteMask" => {
                if let Some(id) = self.hid_of(a, "id") {
                    self.run(Command::DeleteMask { id });
                }
            }
            "toggleMaskEnabled" => {
                let Some(id) = self.hid_of(a, "id") else { return false };
                let enabled = !self.doc.layer(&id).is_some_and(|l| l.mask_enabled);
                self.run(Command::SetMaskEnabled { id, enabled });
            }
            "toggleMaskActive" => {
                let on = !self.mask_target();
                self.run(Command::SetMaskTarget { on });
            }
            "createAdjustmentLayer" => {
                let (Some(kind), Some(params)) = (str_of(a, "kind"), a.get("params").and_then(|p| serde_json::from_value(p.clone()).ok()))
                else {
                    return false;
                };
                self.run(Command::AddAdjustmentLayer { kind: kind.into(), params });
                self.adopt(a, "id");
            }
            "updateAdjustmentParams" => {
                let (Some(id), Some(params)) = (self.hid_of(a, "id"), a.get("params").and_then(|p| serde_json::from_value(p.clone()).ok()))
                else {
                    return false;
                };
                self.run(Command::SetAdjustment { id, params });
                self.run(Command::SealHistory);
            }
            // A destructive filter with its final params.
            "adjustment" => {
                let (Some(kind), Some(params)) = (str_of(a, "kind"), a.get("params").and_then(|p| serde_json::from_value(p.clone()).ok()))
                else {
                    return false;
                };
                self.run(Command::BeginFilter { kind: kind.into() });
                self.run(Command::UpdateFilter { params });
                self.run(Command::CommitFilter);
            }
            // ── Selection ──
            "selectShape" => {
                let pts: Vec<[f64; 2]> = a.get("points").and_then(Value::as_array).into_iter().flatten().filter_map(xy).collect();
                self.run(Command::SelectShape { contours: vec![pts], mode: sel_mode(a.get("mode").unwrap_or(&Value::Null)) });
            }
            "selectAll" => self.run(Command::SelectAll),
            "deselect" => self.run(Command::Deselect),
            "invertSelection" => self.run(Command::InvertSelection),
            "selectByColor" => {
                let Some(p) = a.get("point").and_then(xy) else { return false };
                self.run(Command::SelectByColor {
                    x: p[0].floor() as i32,
                    y: p[1].floor() as i32,
                    tolerance: num(a, "tolerance").unwrap_or(0.1) as f32,
                    mode: sel_mode(a.get("mode").unwrap_or(&Value::Null)),
                    contiguous: a.get("contiguous").and_then(Value::as_bool).unwrap_or(true),
                    sample_all: a.get("sampleAll").and_then(Value::as_bool).unwrap_or(false),
                });
            }
            "setSelectionFeather" => self.run(Command::SetFeather { radius: num(a, "radius").unwrap_or(0.0) as f32 }),
            "growSelection" => self.run(Command::GrowSelection { delta: num(a, "deltaPx").unwrap_or(0.0) as i32 }),
            "smoothSelection" => self.run(Command::SmoothSelection { px: num(a, "px").unwrap_or(0.0) as f32 }),
            // ── Canvas ──
            "transform" => {
                let Some(t) = a.get("t") else { return false };
                let sx = num(t, "scale").unwrap_or(1.0);
                let p = crate::transform::TransformParams {
                    dx: num(t, "dx").unwrap_or(0.0),
                    dy: num(t, "dy").unwrap_or(0.0),
                    sx,
                    sy: num(t, "scaleY").unwrap_or(sx),
                    rotation: num(t, "rotation").unwrap_or(0.0),
                };
                let doc = [f64::from(self.doc.width), f64::from(self.doc.height)];
                self.run(Command::BeginTransform);
                self.run(Command::SetTransform { matrix: p.to_affine(doc) });
                self.run(Command::CommitTransform);
            }
            "resizeCanvas" => {
                let (Some(w), Some(h)) = (num(a, "newWidth"), num(a, "newHeight")) else { return false };
                self.run(Command::ResizeCanvas {
                    width: w as u32,
                    height: h as u32,
                    dx: num(a, "dx").unwrap_or(0.0) as i32,
                    dy: num(a, "dy").unwrap_or(0.0) as i32,
                });
            }
            "scaleImage" => {
                let (Some(w), Some(h)) = (num(a, "newWidth"), num(a, "newHeight")) else { return false };
                self.run(Command::ScaleImage { width: w as u32, height: h as u32 });
            }
            // ── History ──
            "undo" => self.run(Command::Undo),
            "redo" => self.run(Command::Redo),
            // A no-op since v0.199.1.
            "wetModel" => {}
            _ => return false,
        }
        true
    }
}

impl Replay {
    /// A happypaint recording, replayed through onepaint's commands. The
    /// second value names the kinds of command onepaint skips.
    pub fn from_happypaint(bytes: &[u8]) -> Result<(Self, Vec<String>), EngineError> {
        let rec = onepaint_formats::happy_rec::read_happypaint_rec(bytes)?;
        let mut session = Session::from_document(rec.doc);
        if let Some(a) = rec.active.filter(|a| session.doc.find(a).is_some()) {
            session.active = a;
        }
        let mut skipped: Vec<String> = Vec::new();
        let events = rec
            .commands
            .into_iter()
            .map(|c| {
                if !HANDLED.contains(&c.op.as_str()) && !skipped.contains(&c.op) {
                    skipped.push(c.op.clone());
                }
                RecEvent::Happy { t: c.t, name: c.op, args: c.args }
            })
            .collect();
        Ok((Replay::from_parts(session, events), skipped))
    }
}

/// Every op [`Session::apply_happy`] maps.
const HANDLED: &[&str] = &[
    "beginStroke",
    "continueStroke",
    "endStroke",
    "beginWet",
    "continueWet",
    "endWet",
    "wetTick",
    "dryWet",
    "wetTilt",
    "setPaper",
    "beginWashFill",
    "beginSmudge",
    "continueSmudge",
    "endSmudge",
    "beginLiquify",
    "continueLiquify",
    "endLiquify",
    "cleanMixer",
    "floodFill",
    "fillShape",
    "gradient",
    "fillLayerSolid",
    "addLayer",
    "deleteLayer",
    "clearLayer",
    "setActiveLayer",
    "duplicateLayer",
    "moveNodeTo",
    "moveLayer",
    "mergeDown",
    "setLayerProps",
    "toggleClipped",
    "toggleAlphaLock",
    "addMask",
    "deleteMask",
    "toggleMaskEnabled",
    "toggleMaskActive",
    "createAdjustmentLayer",
    "updateAdjustmentParams",
    "adjustment",
    "selectShape",
    "selectAll",
    "deselect",
    "invertSelection",
    "selectByColor",
    "setSelectionFeather",
    "growSelection",
    "smoothSelection",
    "transform",
    "resizeCanvas",
    "scaleImage",
    "undo",
    "redo",
    "wetModel",
];
