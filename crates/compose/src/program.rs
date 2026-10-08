//! The layer tree flattened into one instruction list per output tile, for
//! the GPU compositor (spec 13). Sources are the `Arc<Tile>`s themselves, so
//! the GPU side can key its atlas on pointer identity.
//!
//! [`plan_tile`] mirrors `composite_nodes` step for step. A tile whose tree
//! uses something the GPU path does not handle yet returns `None` and stays
//! on the CPU. [`run`] interprets a program on the CPU, which tests the
//! flattening against [`crate::composite_tile`].

use crate::blend::{blend_px, clip_over};
use crate::{GHOST_PEG_KEY, GHOST_TINT_KEY, StrokeMode, StrokeOverlay, apply_stroke_px, panel_clip, riders};
use onepaint_doc::{BlendMode, Document, Layer, Node};
use onepaint_panel::coverage::TileCoverage;
use onepaint_raster::{Px, TILE_PX, Tile};
use std::sync::Arc;

/// Registers per pixel: the tile itself plus nested groups and clip families.
pub const MAX_STACK: usize = 8;

/// Most distinct source tiles one program may read. The GPU atlas always holds
/// more than this, so a program never has to be split.
pub const MAX_SOURCES: usize = 128;

/// A stroke buffer tile applied to a layer (or its mask) before it blends.
#[derive(Clone, Debug)]
pub struct StrokeSrc {
    pub tile: Arc<Tile>,
    pub opacity: f32,
    pub mode: StrokeMode,
}

/// The layer mask on this tile.
#[derive(Clone, Debug)]
pub enum MaskSrc {
    None,
    Tile(Arc<Tile>),
    /// The mask has no tile here but a mask stroke does: start from zero.
    Zero,
}

/// What `layer_content` produces: pixels, then stroke, then mask.
#[derive(Clone, Debug)]
pub struct Content {
    pub base: Option<Arc<Tile>>,
    pub stroke: Option<StrokeSrc>,
    pub mask: MaskSrc,
    pub mask_stroke: Option<StrokeSrc>,
}

#[derive(Clone, Debug)]
pub enum Op {
    /// Blend content onto the top register (`draw_layer`).
    Layer { content: Content, mode: BlendMode, opacity: f32, inherit_alpha: bool },
    /// Push a transparent register (a group starts).
    PushZero,
    /// Push content as a new register (a clip family's base).
    PushContent(Content),
    /// Clip content onto the top register (a clipped layer).
    Clip { content: Content, opacity: f32 },
    /// Pop the top register, tint it, and blend (or clip) it onto the next.
    Pop { mode: BlendMode, opacity: f32, clip: bool, tint: Option<[f32; 3]> },
}

#[derive(Clone, Debug)]
pub struct TileProgram {
    pub tx: i32,
    pub ty: i32,
    pub ops: Vec<Op>,
}

impl TileProgram {
    /// Every source tile the program reads, in op order (with repeats).
    pub fn sources(&self) -> impl Iterator<Item = &Arc<Tile>> {
        self.ops.iter().flat_map(|op| {
            let c = match op {
                Op::Layer { content, .. } | Op::PushContent(content) | Op::Clip { content, .. } => Some(content),
                _ => None,
            };
            c.into_iter().flat_map(|c| {
                let mask = match &c.mask {
                    MaskSrc::Tile(t) => Some(t),
                    _ => None,
                };
                c.base.iter().chain(c.stroke.iter().map(|s| &s.tile)).chain(mask).chain(c.mask_stroke.iter().map(|s| &s.tile))
            })
        })
    }
}

/// Why a tile cannot run on the GPU; any of these keeps it on the CPU.
struct Unsupported;

struct Planner<'a> {
    tx: i32,
    ty: i32,
    ov: Option<&'a StrokeOverlay<'a>>,
    ops: Vec<Op>,
    depth: usize,
}

/// Plan one output tile, or `None` when it needs the CPU compositor.
pub fn plan_tile(doc: &Document, tx: i32, ty: i32, ov: Option<&StrokeOverlay>) -> Option<TileProgram> {
    // The paper shades the finished composite on the CPU.
    if doc.paper().is_some() {
        return None;
    }
    let mut p = Planner { tx, ty, ov, ops: Vec::new(), depth: 1 };
    p.nodes(&doc.root).ok()?;
    let prog = TileProgram { tx, ty, ops: p.ops };
    let mut seen: Vec<*const Tile> = prog.sources().map(Arc::as_ptr).collect();
    seen.sort_unstable();
    seen.dedup();
    (seen.len() <= MAX_SOURCES).then_some(prog)
}

impl Planner<'_> {
    fn content(&self, l: &Layer) -> Result<Option<Content>, Unsupported> {
        if l.height.is_some() {
            return Err(Unsupported); // relief lighting
        }
        let (tx, ty) = (self.tx, self.ty);
        let on_layer = self.ov.filter(|o| o.layer_id == l.id);
        let src = |o: &StrokeOverlay| o.stroke.tile(tx, ty).map(|t| StrokeSrc { tile: t.clone(), opacity: o.opacity, mode: o.mode });
        let stroke = on_layer.filter(|o| !o.mask).and_then(src);
        let mask_stroke = on_layer.filter(|o| o.mask).and_then(src);
        let base = l.pixels.tile(tx, ty).cloned();
        if base.is_none() && stroke.is_none() {
            return Ok(None);
        }
        let mut c = Content { base, stroke, mask: MaskSrc::None, mask_stroke: None };
        if l.mask_enabled
            && let Some(mask) = &l.mask
        {
            c.mask = match (mask.tile(tx, ty), &mask_stroke) {
                (Some(t), _) => MaskSrc::Tile(t.clone()),
                (None, Some(_)) => MaskSrc::Zero,
                (None, None) => return Ok(None),
            };
            c.mask_stroke = mask_stroke;
        }
        Ok(Some(c))
    }

    fn push(&mut self, op: Op) -> Result<(), Unsupported> {
        self.depth += 1;
        if self.depth > MAX_STACK {
            return Err(Unsupported);
        }
        self.ops.push(op);
        Ok(())
    }

    fn pop(&mut self, mode: BlendMode, opacity: f32, clip: bool, tint: Option<[f32; 3]>) {
        self.depth -= 1;
        self.ops.push(Op::Pop { mode, opacity, clip, tint });
    }

    fn clipped(&mut self, riders: &[Node]) -> Result<(), Unsupported> {
        for r in riders {
            let Node::Layer(c) = r else { continue };
            if !c.visible || c.opacity <= 0.0 {
                continue;
            }
            if let Some(content) = self.content(c)? {
                self.ops.push(Op::Clip { content, opacity: c.opacity });
            }
        }
        Ok(())
    }

    fn nodes(&mut self, nodes: &[Node]) -> Result<(), Unsupported> {
        let mut i = 0;
        while i < nodes.len() {
            if let Node::Layer(l) = &nodes[i]
                && l.adjustment.is_some()
            {
                if l.visible && l.opacity > 0.0 {
                    return Err(Unsupported); // adjustment layers run on the CPU for now
                }
                i += 1;
                continue;
            }
            let j = riders(nodes, i);
            match &nodes[i] {
                Node::Group(g) => {
                    let clip = panel_clip(g, self.tx, self.ty);
                    if g.visible && g.opacity > 0.0 && !g.children.is_empty() && clip != Some(TileCoverage::Empty) {
                        if matches!(clip, Some(TileCoverage::Partial(_))) || g.extra.contains_key(GHOST_PEG_KEY) {
                            return Err(Unsupported);
                        }
                        self.push(Op::PushZero)?;
                        self.nodes(&g.children)?;
                        self.clipped(&nodes[i + 1..j])?;
                        let tint = g.extra.get(GHOST_TINT_KEY).and_then(|v| serde_json::from_value::<[f32; 3]>(v.clone()).ok());
                        self.pop(g.blend_mode, g.opacity, false, tint);
                    }
                }
                Node::Layer(l) => {
                    if l.visible && l.opacity > 0.0 {
                        let family = &nodes[i + 1..j];
                        let content = self.content(l)?;
                        if family.is_empty() {
                            if let Some(content) = content {
                                self.ops.push(Op::Layer {
                                    content,
                                    mode: l.blend_mode,
                                    opacity: l.opacity,
                                    inherit_alpha: l.inherit_alpha,
                                });
                            }
                        } else {
                            match content {
                                Some(c) => self.push(Op::PushContent(c))?,
                                None => self.push(Op::PushZero)?,
                            }
                            self.clipped(family)?;
                            self.pop(l.blend_mode, l.opacity, l.inherit_alpha, None);
                        }
                    }
                }
            }
            i = j;
        }
        Ok(())
    }
}

// ── CPU interpreter (the encoding's own oracle) ──

fn load(t: &Tile, i: usize) -> Px {
    let d = &t.data[i * 4..i * 4 + 4];
    [d[0].to_f32(), d[1].to_f32(), d[2].to_f32(), d[3].to_f32()]
}

fn eval(c: &Content, i: usize) -> Px {
    let mut p = c.base.as_ref().map_or([0.0; 4], |t| load(t, i));
    if let Some(s) = &c.stroke {
        p = apply_stroke_px(p, load(&s.tile, i), s.opacity, s.mode);
    }
    let mut m = match &c.mask {
        MaskSrc::None => return p,
        MaskSrc::Tile(t) => load(t, i),
        MaskSrc::Zero => [0.0; 4],
    };
    if let Some(s) = &c.mask_stroke {
        m = apply_stroke_px(m, load(&s.tile, i), s.opacity, s.mode);
    }
    p.map(|v| v * m[0])
}

/// Run a program on the CPU: premultiplied linear f32, `TILE × TILE`.
pub fn run(prog: &TileProgram) -> Vec<f32> {
    let mut out = vec![0.0; TILE_PX * 4];
    let mut stack: Vec<Px> = Vec::with_capacity(MAX_STACK);
    for i in 0..TILE_PX {
        stack.clear();
        stack.push([0.0; 4]);
        for op in &prog.ops {
            // The stack is never empty: the planner pairs every push with a pop.
            let Some(top) = stack.last_mut() else { break };
            match op {
                Op::Layer { content, mode, opacity, inherit_alpha } => {
                    let mut c = eval(content, i);
                    if *inherit_alpha {
                        let a = top[3];
                        c = c.map(|v| v * a);
                    }
                    *top = blend_px(*mode, *top, c, *opacity);
                }
                Op::PushZero => stack.push([0.0; 4]),
                Op::PushContent(c) => {
                    let v = eval(c, i);
                    stack.push(v);
                }
                Op::Clip { content, opacity } => *top = clip_over(*top, eval(content, i), *opacity),
                Op::Pop { mode, opacity, clip, tint } => {
                    let Some(mut v) = stack.pop() else { break };
                    if let Some(t) = tint {
                        for c in 0..3 {
                            v[c] += (t[c] * v[3] - v[c]) * 0.65;
                        }
                    }
                    let Some(dst) = stack.last_mut() else { break };
                    *dst = if *clip { clip_over(*dst, v, *opacity) } else { blend_px(*mode, *dst, v, *opacity) };
                }
            }
        }
        out[i * 4..i * 4 + 4].copy_from_slice(&stack[0]);
    }
    out
}

/// Strokes keep their mode on the GPU as a small code.
pub fn stroke_code(m: StrokeMode) -> (u32, BlendMode) {
    match m {
        StrokeMode::Paint(b) => (0, b),
        StrokeMode::Erase => (1, BlendMode::Normal),
        StrokeMode::AlphaLock(b) => (2, b),
    }
}
