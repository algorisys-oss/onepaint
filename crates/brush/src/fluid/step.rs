//! One fixed simulation step (`wet-renderer.ts` SETTLE_FRAG + UPDATE_FRAG):
//! settle the old suspended pigment into the layer, then advance water,
//! pigment and velocity. Both passes read the same old state.

use super::dab::{EVAP, WATER_MAX};
use super::{BLOCK, Field, FluidSim, V_MAX, blocks, smoothstep};
use onepaint_color::pigment::pigment_mix;
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
const PAPER_ABSORB: f32 = 1.6;
const PAPER_RESIST: f32 = 0.5;
const GRAN_SWING: f32 = 1.2;
// Paper absorbs (happypaint v0.199.1, `wet-renderer.ts:181-184`): water
// moves only between wet pixels, dry paper barely takes advected paint
// unless it runs downhill, and wet pixels bloom and flow harder.
const WET_RAMP: (f32, f32) = (0.3, 1.0);
const DRY_ACCEPT: f32 = 0.05;
const WET_BLOOM: f32 = 0.8;
/// The slope push on open paper (inside a wash's walls it is 3, spec 58).
const OPEN_FLOW: f32 = 4.0;

/// Settle rate and wet-front strength at (u, v) (sim px). The settle and
/// update passes must agree, or mass would not be conserved.
/// The paper under a sample point: (height, strength); strength 0 = smooth.
#[derive(Clone, Copy)]
pub(crate) struct PaperTap<'a> {
    sim: &'a FluidSim,
}

impl PaperTap<'_> {
    #[inline]
    fn at(&self, u: f32, v: f32) -> f32 {
        self.sim.paper_at(u, v)
    }
}

fn settle_rate(sim: &Field, w: f32, u: f32, v: f32, oil: f32, paper: (PaperTap, f32)) -> (f32, f32) {
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
    // Granulation: valleys collect pigment, ridges shed it.
    if paper.1 > 0.0 {
        dc *= 1.0 + paper.1 * GRAN_SWING * (1.0 - paper.0.at(u, v) * 2.0);
    }
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
        self.generation = super::next_generation();
        let Some(rect) = rect.intersect(&self.rect) else { return Rect::default() };
        let oil = if self.oil { 1.0 } else { 0.0 };
        let space = self.space;
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
            let paper = (PaperTap { sim: self }, self.paper_strength());
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
                                let (u, v) = (lx as f32 + 0.5, ly as f32 + 0.5);
                                let (dc, front) = settle_rate(&fs, fs.texel(lx, ly)[0], u, v, oil, paper);
                                let mut k = (1.0 + force_f * FORCE_RIM * front) * dc.max(force_f);
                                // Force-dry recreates the granulation it bypasses, once.
                                if paper.1 > 0.0 {
                                    k *= 1.0 + force_f * paper.1 * GRAN_SWING * (1.0 - paper.0.at(u, v) * 2.0);
                                }
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
        let paper = (PaperTap { sim: self }, self.paper_strength());
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
                            let (sv, cv) = update_px(&fs, &fc, x, y, oil, tilt, f, paper, space);
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
#[allow(clippy::too_many_arguments)]
fn update_px(
    fs: &Field,
    fc: &Field,
    x: i32,
    y: i32,
    oil: f32,
    tilt: [f32; 2],
    finger: f32,
    paper: (PaperTap, f32),
    sp: onepaint_color::Space,
) -> ([f32; 4], [f32; 4]) {
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
    // Dry paper refuses advected water, unless it runs downhill.
    let tl = tilt[0].hypot(tilt[1]).max(1e-4);
    let downhill = (trace[0] * tilt[0] + trace[1] * tilt[1]) / tl;
    let accept = smoothstep(WET_RAMP.0, WET_RAMP.1, here[0]).max(smoothstep(0.15, 0.6, downhill));
    let k = DRY_ACCEPT + (1.0 - DRY_ACCEPT) * accept;
    trace = [trace[0] * k, trace[1] * k];
    let (ou, ov) = (u - trace[0], v - trace[1]);
    let sim = fs.sample(ou, ov);
    let col = fc.sample(ou, ov);
    let w = sim[0];

    // 8 taps (1 px ring + 2 px ring): water relaxes, pigment blooms through
    // water, its colour folding in neighbour by neighbour through KM.
    const OFFS: [(f32, f32); 8] = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)];
    let mut w_flux = 0.0;
    let mut a_new = col[3];
    let mut n1 = [[0.0f32; 4]; 4];
    let mut c = if col[3] > 0.0005 { [col[0] / col[3], col[1] / col[3], col[2] / col[3]] } else { [1.0; 3] }.map(|v| sp.to_work(v));
    let mut mass = col[3];
    for (i, (ox, oy)) in OFFS.iter().enumerate() {
        let snb = fs.sample(ou + ox, ov + oy);
        let wn = snb[0];
        let cn = fc.sample(ou + ox, ov + oy);
        w_flux += smoothstep(WET_RAMP.0, WET_RAMP.1, w.min(wn)) * (wn - w);
        if i < 4 {
            n1[i] = snb;
        }
        let mut lam = DIFF_P * 0.125 * smoothstep(0.0, 0.04, w.min(wn)) * (1.0 - oil);
        lam *= 1.0 + WET_BLOOM * smoothstep(0.2, 1.5, w.min(wn));
        a_new += lam * (cn[3] - col[3]);
        if cn[3] > 0.0005 {
            let inc = lam * cn[3];
            let nc = [cn[0] / cn[3], cn[1] / cn[3], cn[2] / cn[3]].map(|v| sp.to_work(v));
            if mass <= 0.0005 {
                c = nc;
            } else if inc > 0.0 && !super::same_color(nc, c) {
                c = pigment_mix(c, nc, (inc / (mass + inc)).clamp(0.0, 1.0));
            }
            mass += inc;
        }
    }

    // Textured paper drinks water (uneven drying) and resists its flow.
    let ph = if paper.1 > 0.0 { paper.0.at(u, v) } else { 0.5 };
    let soak = 1.0 + paper.1 * PAPER_ABSORB * ph;
    let resist = 1.0 - paper.1 * PAPER_RESIST * ph;
    let w_new =
        (w + DIFF_W * resist * (1.0 + (0.06 - 1.0) * oil) * w_flux * 0.125 - EVAP as f32 * soak * (1.0 - oil)).clamp(0.0, WATER_MAX as f32);
    let mut col_new = if a_new <= 0.0005 {
        [0.0; 4]
    } else {
        let lin = c.map(|v| sp.from_work(v));
        [lin[0] * a_new, lin[1] * a_new, lin[2] * a_new, a_new]
    };
    let (dc, _) = settle_rate(fs, w, ou, ov, oil, paper);
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
        vel[k] += (tilt[k] * G_TILT * thick - grad[k] * G_SLOPE * OPEN_FLOW * (1.0 - oil)) * (wet * finger_eff) + f_vort[k] * wet;
    }
    let damp = (DAMP + (0.84 - DAMP) * oil) * smoothstep(0.005, 0.04, w_new);
    let vel = vel.map(|v| (v * damp).clamp(-V_MAX, V_MAX));
    ([w_new, curl, vel[0], vel[1]], col_new)
}

#[cfg(test)]
mod tests {
    use super::super::WetDab;
    use super::*;

    #[test]
    fn granulation_settles_pigment_in_the_paper_valleys() {
        // happypaint 0.195.3: dc ·= 1 + s·1.2·(1 − 2p), so low paper collects.
        let doc = Rect::new(0, 0, 200, 120);
        // The classic tooth: its valleys fit the 120×40 sim (the procedural Rough's hills are ~5 mm).
        let rough = onepaint_doc::PAPER_PRESETS.iter().find(|p| p.id == "rough-classic").and_then(|p| p.paper()).unwrap();
        let tex = crate::paper::PaperTexture::new(&rough, 300.0).unwrap();
        let mut sim = FluidSim::new(doc, Rect::new(40, 40, 120, 40), false);
        sim.set_paper(Some(&rough), 300.0);
        let dabs: Vec<_> = (0..40)
            .map(|i| WetDab {
                x: 60.0 + i as f64 * 2.0,
                y: 60.0,
                radius: 16.0,
                water: 0.09,
                pigment: 0.4,
                color: [0.1, 0.2, 0.8],
                vx: 0.0,
                vy: 0.0,
                rewet: 0.0,
                dry: 0.0,
                grain_thr: 0.0,
                halo: 1.0,
                rot: 0.0,
            })
            .collect();
        sim.deposit(&dabs, None, None);
        let mut layer = Surface::new();
        for _ in 0..30 {
            let r = sim.rect();
            sim.step(&mut layer, r, false);
        }
        let (mut lo, mut hi) = ((0.0, 0), (0.0, 0));
        for y in 52..68 {
            for x in 70..130 {
                let h = tex.height(x as f64 + 0.5, y as f64 + 0.5);
                let a = layer.get(x, y)[3] as f64;
                if h < 0.35 {
                    lo = (lo.0 + a, lo.1 + 1);
                } else if h > 0.65 {
                    hi = (hi.0 + a, hi.1 + 1);
                }
            }
        }
        assert!(lo.1 > 20 && hi.1 > 20, "both kinds of paper sampled: {lo:?} {hi:?}");
        let (lo, hi) = (lo.0 / lo.1 as f64, hi.0 / hi.1 as f64);
        assert!(lo > hi * 1.1, "valleys {lo} vs ridges {hi}");
    }
}
