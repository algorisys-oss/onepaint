//! One fixed simulation step (`wet-renderer.ts` SETTLE_FRAG + UPDATE_FRAG):
//! settle the old suspended pigment into the layer, then advance water,
//! pigment and velocity. Both passes read the same old state.

use super::dab::{EVAP, WATER_MAX};
use super::{BLOCK, Field, FluidSim, V_MAX, blocks, smoothstep};
use onepaint_color::{fast_linear_to_srgb as l2s, fast_srgb_to_linear as s2l, pigment::pigment_mix};
use onepaint_raster::{Rect, Surface, TILE};
use rayon::prelude::*;

const DIFF_W: f32 = 0.5;
const DIFF_P: f32 = 0.55;
const SETTLE_DRY: f32 = 0.06;
const SETTLE_WET: f32 = 0.0015;
const EDGE_SETTLE: f32 = 1.0;
const RIM_GATE_R: f32 = 8.0;
const FORCE_RIM: f32 = 0.6;
const G_TILT: f32 = 0.08;
const G_SLOPE: f32 = 0.16;
const FINGER_AMP: f32 = 0.65;
const FINGER_CELL: f32 = 9.0;
const DAMP: f32 = 0.93;
const VORT_EPS: f32 = 0.5;

/// Settle rate and wet-front strength at (u, v) (sim px). The settle and
/// update passes must agree, or mass would not be conserved.
fn settle_rate(sim: &Field, w: f32, u: f32, v: f32, oil: f32) -> (f32, f32) {
    let wl = sim.sample(u - 1.0, v)[0];
    let wr = sim.sample(u + 1.0, v)[0];
    let wu = sim.sample(u, v - 1.0)[0];
    let wd = sim.sample(u, v + 1.0)[0];
    let front = ((wl.max(wr).max(wu).max(wd) - wl.min(wr).min(wu).min(wd)) * 3.0).clamp(0.0, 1.0);
    // Gate by the wettest water nearby, so a stroke's edge band next to a
    // wet body stays suspended instead of baking a rim. The gate saturates
    // at 0.35, so the rings only matter below it.
    let mut near = w.max(wl).max(wr).max(wu).max(wd);
    for r in [RIM_GATE_R / 2.0, RIM_GATE_R] {
        if near >= 0.35 {
            break;
        }
        let d = r * std::f32::consts::FRAC_1_SQRT_2;
        for (ox, oy) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r), (d, d), (-d, -d), (d, -d), (-d, d)] {
            near = near.max(sim.sample(u + ox, v + oy)[0]);
        }
    }
    let wet = smoothstep(0.0, 0.35, near);
    let mut dc = SETTLE_DRY + (SETTLE_WET - SETTLE_DRY) * wet;
    dc *= 1.0 - oil;
    dc *= 1.0 + EDGE_SETTLE * front * (1.0 - wet);
    (dc.clamp(0.0, 1.0), front)
}

fn hash21(x: f32, y: f32) -> f32 {
    let s = (x * 127.1 + y * 311.7).sin() * 43_758.547;
    s - s.floor()
}

/// Static value noise over doc px (fingering): a pure function of position.
fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (fx, fy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash21(ix, iy) + (hash21(ix + 1.0, iy) - hash21(ix, iy)) * fx;
    let b = hash21(ix, iy + 1.0) + (hash21(ix + 1.0, iy + 1.0) - hash21(ix, iy + 1.0)) * fx;
    a + (b - a) * fy
}

/// The fingering factor `1 + amp·(2·noise(doc px / cell) − 1)` over an allocation.
pub(crate) fn finger_field(r: Rect) -> Vec<f32> {
    let mut out = vec![0.0; (r.w * r.h) as usize];
    out.par_chunks_mut(r.w as usize).enumerate().for_each(|(y, row)| {
        for (x, f) in row.iter_mut().enumerate() {
            let (dx, dy) = ((r.x + x as i32) as f32 + 0.5, (r.y + y as i32) as f32 + 0.5);
            *f = 1.0 + FINGER_AMP * (2.0 * value_noise(dx / FINGER_CELL, dy / FINGER_CELL) - 1.0);
        }
    });
    out
}

impl FluidSim {
    /// Advance one step over `rect` (doc px): settle into `layer`, then
    /// update. `force` settles everything (the Dry action). Returns the doc
    /// rect whose layer pixels may have changed.
    pub fn step(&mut self, layer: &mut Surface, rect: Rect, force: bool) -> Rect {
        self.ensure_rect(rect);
        let Some(rect) = rect.intersect(&self.rect) else { return Rect::default() };
        let oil = if self.oil { 1.0 } else { 0.0 };
        let origin = (self.rect.x, self.rect.y);
        let w = self.rect.w;
        let (bw, bh) = blocks(self.rect);

        // 1. Settle: old suspended paint → layer (premultiplied source-over),
        // where there is any. Tiles are independent, so they run in parallel.
        let force_f = if force { 1.0 } else { 0.0 };
        let mut dirty = Rect::default();
        let mut per_tile: std::collections::BTreeMap<(i32, i32), Vec<Rect>> = Default::default();
        for (b, _) in self.occ.iter().enumerate().filter(|(_, o)| **o) {
            let block = Rect::new((b % bw) as i32 * BLOCK + origin.0, (b / bw) as i32 * BLOCK + origin.1, BLOCK, BLOCK);
            let Some(ix) = block.intersect(&rect) else { continue };
            dirty = dirty.union(&ix);
            for (tx, ty) in ix.tiles() {
                if let Some(part) = ix.intersect(&Rect::from_tile(tx, ty)) {
                    per_tile.entry((tx, ty)).or_default().push(part);
                }
            }
        }
        {
            let (fs, fc) = self.field();
            let src = &*layer;
            let out: Vec<_> = per_tile
                .into_par_iter()
                .filter_map(|((tx, ty), parts)| {
                    let mut t = src.tile(tx, ty).map(|t| (**t).clone()).unwrap_or_default();
                    let mut touched = false;
                    for r in parts {
                        for y in r.y..r.bottom() {
                            for x in r.x..r.right() {
                                let (lx, ly) = (x - origin.0, y - origin.1);
                                let col = fc.texel(lx, ly);
                                if col[3] <= 0.0002 {
                                    continue;
                                }
                                let (dc, front) = settle_rate(&fs, fs.texel(lx, ly)[0], lx as f32 + 0.5, ly as f32 + 0.5, oil);
                                let k = (1.0 + force_f * FORCE_RIM * front) * dc.max(force_f);
                                let mut s = col.map(|c| (c * k).clamp(0.0, 1.0));
                                for c in 0..3 {
                                    s[c] = s[c].min(s[3]);
                                }
                                let (px, py) = ((x - tx * TILE) as usize, (y - ty * TILE) as usize);
                                let d = t.get(px, py);
                                t.set(px, py, std::array::from_fn(|c| s[c] + d[c] * (1.0 - s[3])));
                                touched = true;
                            }
                        }
                    }
                    touched.then_some(((tx, ty), t))
                })
                .collect();
            for ((tx, ty), t) in out {
                layer.set_tile(tx, ty, (!t.is_empty()).then(|| std::sync::Arc::new(t)));
            }
        }

        // 2. Update into the other buffers, block by block: blocks holding
        // state and their neighbours; stale blocks there are cleared.
        let mut active = vec![false; bw * bh];
        for (b, _) in self.occ.iter().enumerate().filter(|(_, o)| **o) {
            let (bx, by) = ((b % bw) as i64, (b / bw) as i64);
            for ny in (by - 1).max(0)..=(by + 1).min(bh as i64 - 1) {
                for nx in (bx - 1).max(0)..=(bx + 1).min(bw as i64 - 1) {
                    active[ny as usize * bw + nx as usize] = true;
                }
            }
        }
        let tilt = self.tilt();
        let mut sim_next = std::mem::take(&mut self.sim_next);
        let mut col_next = std::mem::take(&mut self.col_next);
        let stale = std::mem::take(&mut self.occ_next);
        let (fs, fc) = self.field();
        let finger = &self.finger;
        // One task per pixel row (a wash may span only a few block rows).
        let row_flags: Vec<Vec<bool>> = sim_next
            .par_chunks_mut(w as usize)
            .zip(col_next.par_chunks_mut(w as usize))
            .enumerate()
            .map(|(y, (srow, crow))| {
                let y = y as i32;
                let by = (y / BLOCK) as usize;
                let mut flags = vec![false; bw];
                for (bx, flag) in flags.iter_mut().enumerate() {
                    let b = by * bw + bx;
                    let (x0, x1) = (bx as i32 * BLOCK, ((bx as i32 + 1) * BLOCK).min(w));
                    if active[b] {
                        for x in x0..x1 {
                            let f = finger[(y * w + x) as usize];
                            let (sv, cv) = update_px(&fs, &fc, x, y, oil, tilt, f);
                            *flag |= sv[0] != 0.0 || sv[2] != 0.0 || sv[3] != 0.0 || cv[3] != 0.0;
                            srow[x as usize] = sv;
                            crow[x as usize] = cv;
                        }
                    } else if stale[b] {
                        srow[x0 as usize..x1 as usize].fill([0.0; 4]);
                        crow[x0 as usize..x1 as usize].fill([0.0; 4]);
                    }
                }
                flags
            })
            .collect();
        let mut occ = vec![false; bw * bh];
        for (y, flags) in row_flags.iter().enumerate() {
            let by = y / BLOCK as usize;
            for (bx, f) in flags.iter().enumerate() {
                occ[by * bw + bx] |= *f;
            }
        }
        self.sim_next = std::mem::replace(&mut self.sim, sim_next);
        self.col_next = std::mem::replace(&mut self.col, col_next);
        self.occ_next = std::mem::replace(&mut self.occ, occ);
        dirty
    }

    /// Settle everything now and clear the state.
    pub fn dry_now(&mut self, layer: &mut Surface, rect: Rect) -> Rect {
        let r = self.step(layer, rect, true);
        for buf in [&mut self.sim, &mut self.col, &mut self.sim_next, &mut self.col_next] {
            buf.fill([0.0; 4]);
        }
        self.occ.fill(false);
        self.occ_next.fill(false);
        r
    }
}

/// UPDATE_FRAG for one sim pixel (x, y).
fn update_px(fs: &Field, fc: &Field, x: i32, y: i32, oil: f32, tilt: [f32; 2], finger: f32) -> ([f32; 4], [f32; 4]) {
    let (u, v) = (x as f32 + 0.5, y as f32 + 0.5);
    // Advect along the velocity, adopting the fastest wet neighbour heading here.
    let here = fs.texel(x, y);
    let mut trace = [here[2], here[3]];
    let mut best = trace[0] * trace[0] + trace[1] * trace[1];
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let sn = fs.texel(x + dx, y + dy);
        let m = sn[2] * sn[2] + sn[3] * sn[3];
        if m > best && sn[0] > 0.05 && -(sn[2] * dx as f32 + sn[3] * dy as f32) > 0.2 {
            best = m;
            trace = [sn[2], sn[3]];
        }
    }
    let (ou, ov) = (u - trace[0], v - trace[1]);
    let sim = fs.sample(ou, ov);
    let col = fc.sample(ou, ov);
    let w = sim[0];

    // 8 taps (1 px ring + 2 px ring): water relaxes, pigment blooms through
    // water, its colour folding in neighbour by neighbour through KM.
    const OFFS: [(f32, f32); 8] = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)];
    let mut w_avg = 0.0;
    let mut a_new = col[3];
    let mut n1 = [[0.0f32; 4]; 4];
    let mut c = if col[3] > 0.0005 { [col[0] / col[3], col[1] / col[3], col[2] / col[3]] } else { [1.0; 3] }.map(l2s);
    let mut mass = col[3];
    for (i, (ox, oy)) in OFFS.iter().enumerate() {
        let snb = fs.sample(ou + ox, ov + oy);
        let wn = snb[0];
        let cn = fc.sample(ou + ox, ov + oy);
        w_avg += wn;
        if i < 4 {
            n1[i] = snb;
        }
        let lam = DIFF_P * 0.125 * smoothstep(0.0, 0.04, w.min(wn)) * (1.0 - oil);
        a_new += lam * (cn[3] - col[3]);
        if cn[3] > 0.0005 {
            let inc = lam * cn[3];
            let nc = [cn[0] / cn[3], cn[1] / cn[3], cn[2] / cn[3]].map(l2s);
            if mass <= 0.0005 {
                c = nc;
            } else if inc > 0.0 && !super::same_color(nc, c) {
                c = pigment_mix(c, nc, (inc / (mass + inc)).clamp(0.0, 1.0));
            }
            mass += inc;
        }
    }
    w_avg *= 0.125;

    let w_new = (w + DIFF_W * (1.0 + (0.06 - 1.0) * oil) * (w_avg - w) - EVAP as f32 * (1.0 - oil)).clamp(0.0, WATER_MAX as f32);
    let mut col_new = if a_new <= 0.0005 {
        [0.0; 4]
    } else {
        let lin = c.map(s2l);
        [lin[0] * a_new, lin[1] * a_new, lin[2] * a_new, a_new]
    };
    let (dc, _) = settle_rate(fs, w, ou, ov, oil);
    col_new = col_new.map(|v| (v * (1.0 - dc)).clamp(0.0, 1.0));
    for k in 0..3 {
        col_new[k] = col_new[k].min(col_new[3]);
    }

    // Velocity: advected momentum + tilt − slope (dead-zoned), fingered by
    // noise; vorticity confinement from last step's stored curl; damped.
    let mut grad = [0.5 * (n1[1][0] - n1[0][0]), 0.5 * (n1[3][0] - n1[2][0])];
    let gl = grad[0].hypot(grad[1]);
    let g = smoothstep(0.02, 0.06, gl);
    grad = [grad[0] * g, grad[1] * g];
    let curl = 0.5 * ((n1[1][3] - n1[0][3]) - (n1[3][2] - n1[2][2]));
    let gw = [0.5 * (n1[1][1].abs() - n1[0][1].abs()), 0.5 * (n1[3][1].abs() - n1[2][1].abs())];
    let gwl = gw[0].hypot(gw[1]) + 0.00001;
    let n_hat = [gw[0] / gwl, gw[1] / gwl];
    let f_vort = [VORT_EPS * n_hat[1] * sim[1], -VORT_EPS * n_hat[0] * sim[1]];
    let thick = 1.0 + (0.15 - 1.0) * oil;
    let finger_eff = finger + (1.0 - finger) * oil;
    let wet = smoothstep(0.1, 0.8, w);
    let mut vel = [sim[2], sim[3]];
    for k in 0..2 {
        vel[k] += (tilt[k] * G_TILT * thick - grad[k] * G_SLOPE * (1.0 - oil)) * (wet * finger_eff) + f_vort[k] * wet;
    }
    let damp = (DAMP + (0.84 - DAMP) * oil) * smoothstep(0.005, 0.04, w_new);
    let vel = vel.map(|v| (v * damp).clamp(-V_MAX, V_MAX));
    ([w_new, curl, vel[0], vel[1]], col_new)
}
