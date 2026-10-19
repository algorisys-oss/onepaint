//! Wash fills in the sim (happypaint `wet-renderer.ts` `addWall`,
//! `depositWash`, WASH_FRAG; spec 58 C): each wash's region gets its own
//! wall id, and nothing crosses between pixels of different ids; the front
//! lays water and pigment band by band along the wash field.

use super::dab::WATER_MAX;
use super::wash::WashField;
use super::{FluidSim, next_generation, same_color, smoothstep};
use onepaint_color::pigment::pigment_mix;
use onepaint_raster::Rect;

/// Px either side of a wall boundary that take the careful path.
const BAND: i32 = 6;
/// Px over which the front ramps up.
const WIDTH: f32 = 24.0;
/// Px the paper grain roughens the front.
const JITTER: f32 = 6.0;
/// How far the water runs ahead of the pigment (px).
const LEAD: f32 = 10.0;
/// The wall id bits of a [`FluidSim::walls`] entry; bit 8 marks the guard band.
pub(crate) const WALL_ID: u16 = 0xff;
pub(crate) const WALL_BAND: u16 = 0x100;

fn ramp(x: f32) -> f32 {
    smoothstep(0.0, 1.0, (x / WIDTH).clamp(0.0, 1.0))
}

/// One band of a spreading wash (`WashBand`): water level, pigment load,
/// edge softness (px), how much the pigment thins with distance, the
/// field's farthest distance, the colour (straight, the document's space),
/// and the band's distances from the tap.
#[derive(Clone, Copy, Debug)]
pub struct WashBand {
    pub level: f32,
    pub pigment: f32,
    pub soft: f32,
    pub grade: f32,
    pub max_dist: f32,
    pub color: [f32; 3],
    pub from: f32,
    pub to: f32,
}

impl FluidSim {
    /// Wall the wash `field` in: a fresh id over its region (the last wash
    /// wins where they overlap), and the guard band around every boundary.
    pub fn add_wall(&mut self, field: &WashField) {
        let fr = Rect::new(field.x0, field.y0, field.width(), field.y1 - field.y0);
        self.ensure_rect(fr.inflate(BAND + 1));
        let (r, doc) = (self.rect, self.doc);
        let walls = self.walls.get_or_insert_with(|| vec![0; (r.w * r.h) as usize]);
        let id = self.wall_next;
        self.wall_next = if self.wall_next >= 255 { 1 } else { self.wall_next + 1 };
        let li = |x: i32, y: i32| ((y - r.y) * r.w + x - r.x) as usize;
        for y in fr.y..fr.bottom() {
            for x in fr.x..fr.right() {
                if field.at(x, y).is_some_and(|f| f[0] >= 0.0) && r.contains(x, y) {
                    let w = &mut walls[li(x, y)];
                    *w = (*w & WALL_BAND) | u16::from(id);
                }
            }
        }
        // The guard band: within BAND px (Chebyshev) of a boundary between
        // ids, over the stamped rect and a margin. Outside the allocation
        // reads as no wash.
        let Some(m) = fr.inflate(BAND + 1).intersect(&doc) else { return };
        let id_at = |walls: &[u16], x: i32, y: i32| if r.contains(x, y) { walls[li(x, y)] & WALL_ID } else { 0 };
        let (mw, mh) = (m.w as usize, m.h as usize);
        let mut edge = vec![false; mw * mh];
        for y in m.y..m.bottom() {
            for x in m.x..m.right() {
                let v = id_at(walls, x, y);
                let differs = |nx: i32, ny: i32| doc.contains(nx, ny) && id_at(walls, nx, ny) != v;
                edge[((y - m.y) * m.w + x - m.x) as usize] =
                    differs(x - 1, y) || differs(x + 1, y) || differs(x, y - 1) || differs(x, y + 1);
            }
        }
        let b = BAND as usize;
        let mut rows = vec![false; mw * mh];
        for y in 0..mh {
            for x in 0..mw {
                rows[y * mw + x] = (x.saturating_sub(b)..=(x + b).min(mw - 1)).any(|k| edge[y * mw + k]);
            }
        }
        for y in 0..mh {
            for x in 0..mw {
                let (dx, dy) = (m.x + x as i32, m.y + y as i32);
                if r.contains(dx, dy) && (y.saturating_sub(b)..=(y + b).min(mh - 1)).any(|k| rows[k * mw + x]) {
                    walls[li(dx, dy)] |= WALL_BAND;
                }
            }
        }
        self.generation = next_generation();
    }

    /// Lay down one band of a wash: water up to `to + LEAD` (MAX), then,
    /// unless the wash is water only, the pigment between `from` and `to`
    /// (source-over, KM pre-mixed with the wet paint under it).
    pub fn deposit_wash(&mut self, field: &WashField, b: &WashBand) {
        let fr = Rect::new(field.x0, field.y0, field.width(), field.y1 - field.y0);
        let Some(rr) = fr.intersect(&self.rect) else { return };
        self.generation = next_generation();
        let (r, ps, sp) = (self.rect, self.paper_strength(), self.space);
        let level = b.level.min(WATER_MAX as f32);
        let bw = b.color.map(|v| sp.to_work(v));
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in rr.y..rr.bottom() {
            for x in rr.x..rr.right() {
                let Some(f) = field.at(x, y) else { continue };
                if f[0] < 0.0 || f[2] <= 0.0 {
                    continue;
                }
                let (lx, ly) = (x - r.x, y - r.y);
                let h = if ps > 0.0 { self.paper_at(lx as f32 + 0.5, ly as f32 + 0.5) } else { 0.5 };
                let dist = (f[0] + (h - 0.5) * JITTER * ps).max(0.0);
                let edge = if b.soft > 0.0 { smoothstep(0.0, b.soft, f[1] + 0.5) } else { 1.0 };
                let k = edge * f[2];
                let i = (ly * r.w + lx) as usize;
                let w = ramp(b.to + LEAD - dist) * k;
                if w > 0.0 {
                    let s = &mut self.sim[i];
                    s[0] = s[0].max(level * w);
                    (x0, y0, x1, y1) = (x0.min(lx), y0.min(ly), x1.max(lx + 1), y1.max(ly + 1));
                }
                if b.pigment <= 0.0 {
                    continue;
                }
                let p = b.pigment * k * (1.0 - b.grade * (dist / b.max_dist.max(1.0)).clamp(0.0, 1.0));
                let (c_new, c_prev) = (p * ramp(b.to - dist), p * ramp(b.from - dist));
                if c_new <= c_prev {
                    continue;
                }
                let a = ((c_new - c_prev) / (1.0 - c_prev).max(1e-4)).clamp(0.0, 1.0);
                let un = self.col[i];
                let mut brush = b.color;
                if un[3] > 0.001 {
                    let uc = [un[0] / un[3], un[1] / un[3], un[2] / un[3]].map(|v| sp.to_work(v));
                    let m = (un[3] / (un[3] + p * 2.0)).clamp(0.0, 0.9);
                    if !same_color(uc, bw) {
                        brush = pigment_mix(bw, uc, m).map(|v| sp.from_work(v));
                    }
                }
                (x0, y0, x1, y1) = (x0.min(lx), y0.min(ly), x1.max(lx + 1), y1.max(ly + 1));
                let c = &mut self.col[i];
                *c = [
                    brush[0] * a + c[0] * (1.0 - a),
                    brush[1] * a + c[1] * (1.0 - a),
                    brush[2] * a + c[2] * (1.0 - a),
                    a + c[3] * (1.0 - a),
                ];
            }
        }
        if x1 > x0 {
            self.occupy(x0, y0, x1, y1);
        }
    }
}
