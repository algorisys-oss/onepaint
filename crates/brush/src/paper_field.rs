//! Procedural paper height fields in physical units (happypaint
//! `engine/brush/paper-field.ts`, v0.197.0 / v0.198.2; spec 57).
//!
//! happypaint ported these from the Watercolor Playground's `paperFieldFS`
//! (MIT, (c) 2026 Maksim Surguy, <https://github.com/msurguy/watercolor-playground>).
//! Ten recipes: cold press, hot press, rough, khadi, washi, laid, canvas,
//! toned (Mi-Teintes honeycomb), kraft, student.
//!
//! Every tile is seamless: lattices wrap on the tile period and feature
//! sizes round to a whole number of cells (`period`). One texel is one unit
//! of [`PAPER_UNIT_MM`]; the engine scales the tile to doc px through the
//! document's DPI. Integer hashing on the CPU, so a tile is the same on
//! every device. Lattice tables are f32 and everything else f64, as
//! happypaint's `Float32Array`s and JS numbers.

use rayon::prelude::*;
use std::collections::HashMap;
use std::f64::consts::PI;

/// Tile side in units: a whole number of Laid's 5.6-unit lines and 140-unit chains.
pub const TILE_UNITS: usize = 560;
/// Physical size of one unit (mm).
pub const PAPER_UNIT_MM: f64 = 0.18;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaperKind {
    Cold,
    Hot,
    Rough,
    Khadi,
    Washi,
    Laid,
    Canvas,
    Toned,
    Kraft,
    Student,
}

impl PaperKind {
    pub const ALL: [PaperKind; 10] = [
        PaperKind::Cold,
        PaperKind::Hot,
        PaperKind::Rough,
        PaperKind::Khadi,
        PaperKind::Washi,
        PaperKind::Laid,
        PaperKind::Canvas,
        PaperKind::Toned,
        PaperKind::Kraft,
        PaperKind::Student,
    ];

    pub fn id(self) -> &'static str {
        match self {
            PaperKind::Cold => "cold",
            PaperKind::Hot => "hot",
            PaperKind::Rough => "rough",
            PaperKind::Khadi => "khadi",
            PaperKind::Washi => "washi",
            PaperKind::Laid => "laid",
            PaperKind::Canvas => "canvas",
            PaperKind::Toned => "toned",
            PaperKind::Kraft => "kraft",
            PaperKind::Student => "student",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == s)
    }
}

// ── Hashing ──

/// PCG-style integer hash (the original's `pcg`).
fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747796405).wrapping_add(2891336453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277803737);
    (w >> 22) ^ w
}

/// hash(cell, salt) in [0, 1).
fn hash(ix: u32, iy: u32, salt: u32) -> f64 {
    f64::from(pcg(ix.wrapping_add(pcg(iy.wrapping_add(pcg(salt)))))) / 4294967296.0
}

fn modf(a: f64, n: f64) -> f64 {
    ((a % n) + n) % n
}

fn modi(a: i64, n: i64) -> usize {
    a.rem_euclid(n) as usize
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Whole cells of size ~`cell` units that fit the tile (≥ 1).
pub fn period(cell: f64, tile: usize) -> usize {
    ((tile as f64 / cell).round() as usize).max(1)
}

struct Octaves {
    nx: Vec<usize>,
    ny: Vec<usize>,
    amp: Vec<f64>,
    salts: Vec<u32>,
}

struct FibreGrid {
    n: usize,
    c: f64,
    ax: Vec<f32>,
    ay: Vec<f32>,
    bx: Vec<f32>,
    by: Vec<f32>,
    shade: Vec<f32>,
}

/// One generation run's caches (lattice tables, octave sets, fibre grids).
struct Field {
    t: usize,
    tables: HashMap<(u32, usize, usize), Vec<f32>>,
    octaves: HashMap<(u32, u64, u64, usize), Octaves>,
    fibres: HashMap<(u32, u64, u64, u64, u64), FibreGrid>,
}

impl Field {
    fn new(t: usize) -> Self {
        Field { t, tables: HashMap::new(), octaves: HashMap::new(), fibres: HashMap::new() }
    }

    fn tf(&self) -> f64 {
        self.t as f64
    }

    /// Hash values of an nx × ny wrapped lattice.
    fn lattice(&mut self, nx: usize, ny: usize, salt: u32) -> &[f32] {
        self.tables.entry((salt, nx, ny)).or_insert_with(|| {
            let mut t = Vec::with_capacity(nx * ny);
            for y in 0..ny {
                for x in 0..nx {
                    t.push(hash(x as u32, y as u32, salt) as f32);
                }
            }
            t
        })
    }

    /// Value noise over a lattice table, at lattice coords (u, v).
    fn vnoise_t(t: &[f32], u: f64, v: f64, nx: usize, ny: usize) -> f64 {
        let (i, j) = (u.floor(), v.floor());
        let (fx, fy) = (smooth(u - i), smooth(v - j));
        let x0 = modi(i as i64, nx as i64);
        let x1 = if x0 + 1 == nx { 0 } else { x0 + 1 };
        let jy = modi(j as i64, ny as i64);
        let y0 = jy * nx;
        let y1 = (if jy + 1 == ny { 0 } else { jy + 1 }) * nx;
        let g = |k: usize| f64::from(t[k]);
        let a = g(y0 + x0) + (g(y0 + x1) - g(y0 + x0)) * fx;
        let b = g(y1 + x0) + (g(y1 + x1) - g(y1 + x0)) * fx;
        a + (b - a) * fy
    }

    fn vnoise_l(&mut self, u: f64, v: f64, nx: usize, ny: usize, salt: u32) -> f64 {
        let t = self.lattice(nx, ny, salt);
        Self::vnoise_t(t, u, v, nx, ny)
    }

    /// Value noise in units with a cell of ~`cx` × `cy` units.
    fn vnoise(&mut self, x: f64, y: f64, cx: f64, cy: f64, salt: u32) -> f64 {
        let (nx, ny) = (period(cx, self.t), period(cy, self.t));
        let tf = self.tf();
        self.vnoise_l(x * nx as f64 / tf, y * ny as f64 / tf, nx, ny, salt)
    }

    /// fBm, normalised 0..1; first octave cell `cx` × `cy` units, then halving.
    fn fbm(&mut self, x: f64, y: f64, cx: f64, cy: f64, oct: usize, salt: u32) -> f64 {
        let key = (salt, cx.to_bits(), cy.to_bits(), oct);
        if !self.octaves.contains_key(&key) {
            let (mut nx, mut ny) = (period(cx, self.t), period(cy, self.t));
            let mut o = Octaves { nx: vec![], ny: vec![], amp: vec![], salts: vec![] };
            let (mut a, mut sum) = (0.5, 0.0);
            for k in 0..oct {
                o.nx.push(nx);
                o.ny.push(ny);
                o.amp.push(a);
                o.salts.push(salt + k as u32 * 13);
                self.lattice(nx, ny, salt + k as u32 * 13);
                sum += a;
                a *= 0.5;
                nx *= 2;
                ny *= 2;
            }
            for v in &mut o.amp {
                *v /= sum;
            }
            self.octaves.insert(key, o);
        }
        let tf = self.tf();
        let o = &self.octaves[&key];
        let mut v = 0.0;
        for k in 0..oct {
            let (nx, ny) = (o.nx[k], o.ny[k]);
            // A fractional per-octave shift stands in for the original's
            // rotation: it decorrelates octaves and keeps the period.
            let off = 17.1 * k as f64;
            let t = &self.tables[&(o.salts[k], nx, ny)];
            v += o.amp[k] * Self::vnoise_t(t, x * nx as f64 / tf + off, y * ny as f64 / tf + off * 0.61, nx, ny);
        }
        v
    }

    /// p + (fbm − 0.5)·amount on each axis, frequency in 1/units.
    fn warp(&mut self, x: f64, y: f64, freq: f64, amount: f64, salt: u32) -> (f64, f64) {
        let c = 1.0 / freq;
        (x + (self.fbm(x, y, c, c, 3, salt) - 0.5) * amount, y + (self.fbm(x, y, c, c, 3, salt + 50) - 0.5) * amount)
    }

    /// Nearest distance (cell units) to a jittered point lattice.
    fn cells(&mut self, x: f64, y: f64, cell: f64, jitter: f64, salt: u32) -> f64 {
        let n = period(cell, self.t);
        self.lattice(n, n, salt);
        self.lattice(n, n, salt + 1);
        let (hx, hy) = (&self.tables[&(salt, n, n)], &self.tables[&(salt + 1, n, n)]);
        let tf = self.t as f64;
        let (u, v) = (x * n as f64 / tf, y * n as f64 / tf);
        let (i, j) = (u.floor(), v.floor());
        let (fx, fy) = (u - i, v - j);
        let mut d1 = 9.0f64;
        for oy in -1..=1i64 {
            for ox in -1..=1i64 {
                let k = modi(j as i64 + oy, n as i64) * n + modi(i as i64 + ox, n as i64);
                let dx = ox as f64 + 0.5 + (f64::from(hx[k]) - 0.5) * jitter - fx;
                let dy = oy as f64 + 0.5 + (f64::from(hy[k]) - 0.5) * jitter - fy;
                d1 = d1.min((dx * dx + dy * dy).sqrt());
            }
        }
        d1
    }

    /// Rounded hills of random size and height on a jittered grid (~0..1).
    fn blobs(&mut self, x: f64, y: f64, cell: f64, salt: u32) -> f64 {
        let n = period(cell, self.t);
        for s in 0..4 {
            self.lattice(n, n, salt + s);
        }
        let l = |s: u32| &self.tables[&(salt + s, n, n)];
        let (rx, ry, rz, rw) = (l(0), l(1), l(2), l(3));
        let tf = self.t as f64;
        let (u, v) = (x * n as f64 / tf, y * n as f64 / tf);
        let (i, j) = (u.floor(), v.floor());
        let (fx, fy) = (u - i, v - j);
        let mut h = 0.0;
        for oy in -1..=1i64 {
            for ox in -1..=1i64 {
                let k = modi(j as i64 + oy, n as i64) * n + modi(i as i64 + ox, n as i64);
                let dx = ox as f64 + f64::from(rx[k]) - fx;
                let dy = oy as f64 + f64::from(ry[k]) - fy;
                let rad = 0.35 + 0.4 * f64::from(rz[k]);
                h += (0.5 + 0.5 * f64::from(rw[k])) * (-(dx * dx + dy * dy) / (rad * rad)).exp();
            }
        }
        h * 0.75
    }

    fn fibre_grid(&mut self, cell: f64, len: f64, density: f64, along: f64, salt: u32) -> (u32, u64, u64, u64, u64) {
        let key = (salt, cell.to_bits(), len.to_bits(), density.to_bits(), along.to_bits());
        if self.fibres.contains_key(&key) {
            return key;
        }
        let n = period(cell, self.t);
        let m = n * n * 2;
        let mut g = FibreGrid {
            n,
            c: self.t as f64 / n as f64,
            ax: vec![0.0; m],
            ay: vec![0.0; m],
            bx: vec![0.0; m],
            by: vec![0.0; m],
            shade: vec![0.0; m],
        };
        for k in 0..2 {
            let s = salt + k as u32 * 31;
            for d in 0..6 {
                self.lattice(n, n, s + d);
            }
            let l = |d: u32| &self.tables[&(s + d, n, n)];
            let (hx, hy, hz, hw, hl, hs) = (l(0), l(1), l(2), l(3), l(4), l(5));
            for q in 0..n * n {
                let o = q * 2 + k;
                if f64::from(hw[q]) >= density {
                    continue; // an empty slot: shade stays 0
                }
                let a = (f64::from(hz[q]) - 0.5) * PI * (1.0 - along);
                let l = len * (0.35 + 0.65 * f64::from(hl[q])) * 0.5;
                let (dx, dy) = (a.cos() * l, a.sin() * l);
                g.ax[o] = (f64::from(hx[q]) - dx) as f32;
                g.ay[o] = (f64::from(hy[q]) - dy) as f32;
                g.bx[o] = (2.0 * dx) as f32;
                g.by[o] = (2.0 * dy) as f32;
                g.shade[o] = (0.55 + 0.45 * f64::from(hs[q])) as f32;
            }
        }
        self.fibres.insert(key, g);
        key
    }

    /// Thin strands on a grid of ~`cell` units, up to `len` cells long and
    /// `width` units wide; coverage of the nearest. `along` lines them up
    /// with x; `density` is the share of slots holding one.
    #[allow(clippy::too_many_arguments)]
    fn fibres(&mut self, x: f64, y: f64, cell: f64, len: f64, width: f64, density: f64, along: f64, salt: u32) -> f64 {
        let key = self.fibre_grid(cell, len, density, along, salt);
        let g = &self.fibres[&key];
        let (u, v) = (x / g.c, y / g.c);
        let (i, j) = (u.floor(), v.floor());
        let reach = width * 0.5 + 0.9;
        let mut cov = 0.0f64;
        for oy in -2..=2i64 {
            let row = modi(j as i64 + oy, g.n as i64) * g.n;
            for ox in -2..=2i64 {
                let q = (row + modi(i as i64 + ox, g.n as i64)) * 2;
                for k in q..q + 2 {
                    let w0 = f64::from(g.shade[k]);
                    if w0 == 0.0 {
                        continue;
                    }
                    // Segment ends are stored relative to their own cell.
                    let sx = f64::from(g.ax[k]) + i + ox as f64;
                    let sy = f64::from(g.ay[k]) + j + oy as f64;
                    let (pax, pay) = (u - sx, v - sy);
                    let (bx, by) = (f64::from(g.bx[k]), f64::from(g.by[k]));
                    let bb = bx * bx + by * by;
                    let t = if bb > 0.0 { ((pax * bx + pay * by) / bb).clamp(0.0, 1.0) } else { 0.0 };
                    let (ex, ey) = (pax - bx * t, pay - by * t);
                    let dist = (ex * ex + ey * ey).sqrt() * g.c;
                    if dist > reach {
                        continue;
                    }
                    cov = cov.max((1.0 - smoothstep(width * 0.5, reach, dist)) * w0);
                }
            }
        }
        cov
    }

    /// Small round specks; `density` of the slots hold one.
    fn specks(&mut self, x: f64, y: f64, cell: f64, radius: f64, density: f64, salt: u32) -> f64 {
        let n = period(cell, self.t);
        let c = self.t as f64 / n as f64;
        for s in 0..4 {
            self.lattice(n, n, salt + s);
        }
        let l = |s: u32| &self.tables[&(salt + s, n, n)];
        let (hx, hy, hd, hr) = (l(0), l(1), l(2), l(3));
        let (u, v) = (x / c, y / c);
        let (i, j) = (u.floor(), v.floor());
        let (fx, fy) = (u - i, v - j);
        let mut cov = 0.0f64;
        for oy in -1..=1i64 {
            for ox in -1..=1i64 {
                let q = modi(j as i64 + oy, n as i64) * n + modi(i as i64 + ox, n as i64);
                if f64::from(hd[q]) >= density {
                    continue;
                }
                let r = radius * (0.4 + 0.6 * f64::from(hr[q]));
                let dx = ox as f64 + f64::from(hx[q]) - fx;
                let dy = oy as f64 + f64::from(hy[q]) - fy;
                let d = (dx * dx + dy * dy).sqrt() * c;
                cov = cov.max(1.0 - smoothstep(r * 0.6, r + 0.6, d));
            }
        }
        cov
    }
}

/// Distance to the nearest centre of a hexagonal lattice with spacing 1.
fn hex_dist(px: f64, py: f64) -> f64 {
    let (sx, sy) = (1.0, 1.7320508);
    let ax = modf(px, sx) - sx * 0.5;
    let ay = modf(py, sy) - sy * 0.5;
    let bx = modf(px - sx * 0.5, sx) - sx * 0.5;
    let by = modf(py - sy * 0.5, sy) - sy * 0.5;
    ax.hypot(ay).min(bx.hypot(by))
}

/// Laid's lines and the distance to its nearest chain line (shared by its
/// height and look).
fn laid_lines(f: &mut Field, x: f64, y: f64) -> (f64, f64) {
    let wob = (f.fbm(x, y, 250.0, 50.0, 3, 600) - 0.5) * 6.0;
    let line_p = f.tf() / period(5.6, f.t) as f64;
    let lines = (0.5 + 0.5 * ((2.0 * PI * (y + wob)) / line_p).cos()).powi(2);
    let cx = x + (f.fbm(x, y, 50.0, 250.0, 3, 610) - 0.5) * 8.0;
    let chain_p = f.tf() / period(140.0, f.t) as f64;
    let dc = (modf(cx / chain_p, 1.0) - 0.5).abs() * chain_p;
    (lines, dc)
}

fn washi_fibres(f: &mut Field, x: f64, y: f64) -> (f64, f64) {
    let (qx, qy) = f.warp(x, y, 0.025, 28.0, 500);
    let long = f.fibres(qx, qy, 42.0, 3.4, 1.3, 0.4, 0.0, 510) * (0.6 + 0.4 * f.vnoise(x, y, 20.0, 20.0, 512));
    let (wx, wy) = f.warp(x, y, 0.05, 10.0, 515);
    let fine = f.fibres(wx, wy, 14.0, 3.0, 0.6, 0.45, 0.0, 520);
    (long, fine)
}

fn khadi_fibre(f: &mut Field, x: f64, y: f64) -> f64 {
    let (wx, wy) = f.warp(x, y, 0.05, 8.0, 430);
    f.fibres(wx, wy, 9.0, 3.0, 1.1, 0.6, 0.0, 440)
}

/// The height at (x, y) units: 0..1, mean ~0.5, high = ridge.
fn height(kind: PaperKind, f: &mut Field, x: f64, y: f64) -> f64 {
    match kind {
        // Rounded, irregular bumps of mixed size over a fine felt.
        PaperKind::Cold => {
            let (qx, qy) = f.warp(x, y, 0.02, 10.0, 100);
            let hills = f.blobs(qx, qy, 11.0, 110) * 0.6 + f.blobs(qx, qy, 5.5, 115) * 0.4;
            let h = hills * 0.5 + f.fbm(qx, qy, 1.0 / 0.09, 1.0 / 0.09, 5, 120) * 0.38 + f.vnoise(x, y, 2.0, 2.0, 130) * 0.12;
            0.5 + (h - 0.5) * 1.3
        }
        // Pressed nearly flat: a faint felt and the fibres.
        PaperKind::Hot => {
            let h = f.fbm(x, y, 1.0 / 0.06, 1.0 / 0.06, 4, 200) * 0.6
                + f.vnoise(x, y, 2.5, 2.5, 210) * 0.25
                + f.fibres(x, y, 16.0, 2.5, 0.8, 0.3, 0.0, 220) * 0.05;
            0.5 + (h - 0.4) * 0.45
        }
        // Big knobbly hills with deep valleys between them.
        PaperKind::Rough => {
            let (qx, qy) = f.warp(x, y, 0.01, 40.0, 300);
            let big = smoothstep(0.3, 0.7, f.fbm(x, y, 1.0 / 0.006, 1.0 / 0.006, 3, 305));
            let hills = f.blobs(qx, qy, 26.0, 310) * (0.35 + 0.4 * big) + f.blobs(qx, qy, 12.0, 315) * (0.45 - 0.25 * big);
            let h = hills * 0.65 + f.fbm(qx, qy, 1.0 / 0.06, 1.0 / 0.06, 5, 320) * 0.25 + f.vnoise(x, y, 1.0 / 0.45, 1.0 / 0.45, 325) * 0.1;
            0.5 + (h - 0.52) * 1.5
        }
        // Handmade, lumpy at every scale, with raised fibres.
        PaperKind::Khadi => {
            let (qx, qy) = f.warp(x, y, 0.008, 60.0, 400);
            let lumps = f.fbm(qx, qy, 1.0 / 0.035, 1.0 / 0.035, 5, 410);
            let knobs = 1.0 - smoothstep(0.0, 0.9, f.cells(qx, qy, 13.0, 1.0, 420));
            let fib = khadi_fibre(f, x, y);
            let h = lumps * 0.5 + knobs * 0.3 + f.fbm(x, y, 5.0, 5.0, 3, 450) * 0.16 + fib * 0.03;
            0.5 + (h - 0.5) * 1.6
        }
        // Smooth and cloudy, long kozo fibres lying on top.
        PaperKind::Washi => {
            let (long, fine) = washi_fibres(f, x, y);
            let h = f.fbm(x, y, 20.0, 20.0, 4, 530) * 0.8 + long * 0.08 + fine * 0.03;
            0.5 + (h - 0.42) * 0.7
        }
        // Close laid lines crossed by chain lines, over a soft felt.
        PaperKind::Laid => {
            let (lines, dc) = laid_lines(f, x, y);
            let chain = 1.0 - smoothstep(1.0, 3.5, dc);
            let h = lines * 0.1 + chain * 0.05 + f.fbm(x, y, 12.5, 12.5, 4, 620) * 0.6;
            0.5 + (h - 0.36) * 1.1
        }
        // A plain weave of slightly uneven threads under gesso.
        PaperKind::Canvas => {
            let n = period(8.0, f.t);
            let tc = f.tf() / n as f64;
            let px = x + (f.vnoise(x, y, 50.0, 50.0, 700) - 0.5) * tc * 1.2;
            let py = y + (f.vnoise(x, y, 50.0, 50.0, 701) - 0.5) * tc * 1.2;
            let (cx, cy) = (px / tc, py / tc);
            let ix = modi(cx.floor() as i64, n as i64);
            let iy = modi(cy.floor() as i64, n as i64);
            let (fx, fy) = (cx - cx.floor(), cy - cy.floor());
            let k = ((n as f64 * 0.35).round() as usize).max(1);
            let tx = 0.8
                + 0.2 * f64::from(f.lattice(n, 1, 702)[ix])
                + 0.3 * (f.vnoise_l(ix as f64 * 5.0, cy * k as f64 / n as f64, n * 5, k, 703) - 0.5);
            let ty = 0.8
                + 0.2 * f64::from(f.lattice(1, n, 704)[iy])
                + 0.3 * (f.vnoise_l(cx * k as f64 / n as f64, iy as f64 * 5.0, k, n * 5, 705) - 0.5);
            // Over and under alternate: the crossings make a checkerboard.
            let warp_up = (ix + iy) % 2 == 1;
            let dx = (fx - 0.5) * if warp_up { 2.3 } else { 1.15 };
            let dy = (fy - 0.5) * if warp_up { 1.15 } else { 2.3 };
            let grain = (1.0 - (dx * dx + dy * dy)).max(0.0) * if warp_up { tx } else { ty };
            let h = grain.sqrt() * 0.75 + f.fbm(px, py, 1.0 / 0.15, 1.0 / 0.15, 3, 706) * 0.25;
            0.5 + (h - 0.55) * 1.1
        }
        // A regular honeycomb of shallow pits.
        PaperKind::Toned => {
            let (qx, qy) = f.warp(x, y, 0.05, 7.0, 800);
            let sx = f.tf() / period(9.0, f.t) as f64;
            let sy = f.tf() / (period(9.0 * 1.7320508, f.t) as f64 * 1.7320508);
            let d = hex_dist(qx / sx, qy / sy) * (0.8 + 0.4 * f.vnoise(qx, qy, 4.5, 4.5, 805));
            let rims = smoothstep(0.1, 0.55, d);
            let h = rims * 0.28 + f.fbm(x, y, 1.0 / 0.09, 1.0 / 0.09, 4, 810) * 0.6 + f.fibres(x, y, 12.0, 2.0, 0.9, 0.35, 0.0, 820) * 0.04;
            0.5 + (h - 0.42) * 1.2
        }
        // Smooth, streaky along the machine direction.
        PaperKind::Kraft => {
            let h = f.fbm(x, y, 1.0 / 0.07, 1.0 / 0.07, 4, 900) * 0.6
                + f.vnoise(x, y, 1.0 / 0.35, 1.0 / 0.35, 910) * 0.32
                + f.fibres(x, y, 10.0, 2.5, 0.9, 0.6, 0.7, 920) * 0.04;
            0.5 + (h - 0.45) * 0.7
        }
        // Felt-pressed small bumps all of one size.
        PaperKind::Student => {
            let (qx, qy) = f.warp(x, y, 0.03, 3.0, 995);
            let bumps = 1.0 - smoothstep(0.0, 0.7, f.cells(qx, qy, 7.0, 0.7, 1000));
            let h = bumps * 0.6 + f.fbm(x, y, 1.0 / 0.12, 1.0 / 0.12, 3, 1010) * 0.4;
            0.5 + (h - 0.48) * 1.1
        }
    }
}

/// The sheet's look at (x, y) units: mottle (0.5 neutral) and fleck (0.5
/// none; below = dark speck, above = light fibre).
fn look(kind: PaperKind, f: &mut Field, x: f64, y: f64) -> [f64; 2] {
    match kind {
        PaperKind::Cold => [f.fbm(x, y, 1.0 / 0.012, 1.0 / 0.012, 4, 140), 0.5],
        PaperKind::Hot => [f.fbm(x, y, 100.0, 100.0, 4, 230), 0.5],
        PaperKind::Rough => [f.fbm(x, y, 100.0, 100.0, 4, 330), 0.5],
        PaperKind::Khadi => {
            let fib = khadi_fibre(f, x, y);
            let (vx, vy) = f.warp(x, y, 0.06, 10.0, 465);
            let dark = f.specks(x, y, 60.0, 2.0, 0.3, 460).max(f.fibres(vx, vy, 110.0, 0.4, 0.9, 0.3, 0.0, 470) * 0.6);
            [f.fbm(x, y, 1.0 / 0.006, 1.0 / 0.006, 4, 480) * 0.7 + fib * 0.25, 0.5 - dark * 0.35 + fib * 0.06]
        }
        PaperKind::Washi => {
            let (long, fine) = washi_fibres(f, x, y);
            let cloud = f.fbm(x, y, 200.0, 200.0, 5, 540);
            let bark = f.specks(x, y, 70.0, 1.6, 0.18, 550);
            [cloud * 0.85 + long * 0.15, 0.5 + long * 0.16 + fine * 0.05 - bark * 0.4]
        }
        PaperKind::Laid => {
            let (lines, dc) = laid_lines(f, x, y);
            // A touch thicker beside each chain line; the laid lines show faintly.
            [0.5 - 0.12 * (-dc / 12.0).exp() - 0.06 * lines + (f.fbm(x, y, 100.0, 100.0, 3, 630) - 0.5) * 0.6, 0.5]
        }
        PaperKind::Canvas => [f.fbm(x, y, 100.0, 100.0, 3, 707), 0.5],
        PaperKind::Toned => [f.fbm(x, y, 1.0 / 0.012, 1.0 / 0.012, 4, 830), 0.5 - f.specks(x, y, 60.0, 1.2, 0.2, 840) * 0.3],
        PaperKind::Kraft => {
            let streak = f.fbm(x, y, 250.0, 1.0 / 0.03, 5, 930);
            let (ax, ay) = f.warp(x, y, 0.08, 10.0, 935);
            let dark = (f.fibres(ax, ay, 60.0, 0.35, 0.7, 0.2, 0.6, 940) * 0.6).max(f.specks(x, y, 40.0, 1.3, 0.35, 950));
            let (bx, by) = f.warp(x, y, 0.08, 10.0, 955);
            let light = f.fibres(bx, by, 70.0, 0.4, 1.0, 0.25, 0.5, 960);
            [streak * 0.8 + f.fbm(x, y, 100.0, 100.0, 3, 970) * 0.2, 0.5 - dark * 0.28 + light * 0.12]
        }
        PaperKind::Student => [f.fbm(x, y, 1.0 / 0.015, 1.0 / 0.015, 3, 1020), 0.5],
    }
}

/// Rows per parallel band (each band builds its own lattice tables).
const BAND: usize = 16;

/// `f` at texel centres over a `w` × `h` window of a `tile` starting at
/// (x0, y0) units, `n` values per texel, clamped 0..1.
fn sample<const N: usize>(
    tile: usize,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    at: impl Fn(&mut Field, f64, f64) -> [f64; N] + Sync,
) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h * N];
    out.par_chunks_mut(w * N * BAND).enumerate().for_each(|(b, rows)| {
        let mut f = Field::new(tile);
        for (r, row) in rows.chunks_mut(w * N).enumerate() {
            let y = (y0 + b * BAND + r) as f64 + 0.5;
            for x in 0..w {
                let v = at(&mut f, (x0 + x) as f64 + 0.5, y);
                for k in 0..N {
                    row[x * N + k] = v[k].clamp(0.0, 1.0) as f32;
                }
            }
        }
    });
    out
}

/// Heights over a window of the tile, one per unit (wraps), 0..1.
pub fn paper_heights(kind: PaperKind, x0: usize, y0: usize, w: usize, h: usize, tile: usize) -> Vec<f32> {
    sample::<1>(tile, x0, y0, w, h, |f, x, y| [height(kind, f, x, y)])
}

/// Looks over a window of the tile: (mottle, fleck) interleaved, 0..1.
pub fn paper_looks(kind: PaperKind, x0: usize, y0: usize, w: usize, h: usize, tile: usize) -> Vec<f32> {
    sample::<2>(tile, x0, y0, w, h, |f, x, y| look(kind, f, x, y))
}

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().map(|x| (x * 255.0).round() as u8).collect()
}

/// The whole seamless height tile as R8 bytes.
pub fn paper_tile(kind: PaperKind, tile: usize) -> Vec<u8> {
    bytes(&paper_heights(kind, 0, 0, tile, tile, tile))
}

/// The whole seamless look tile as RG8 bytes (mottle, fleck).
pub fn paper_look_tile(kind: PaperKind, tile: usize) -> Vec<u8> {
    bytes(&paper_looks(kind, 0, 0, tile, tile, tile))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: usize = 140;

    fn fnv(d: &[u8]) -> u64 {
        d.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
    }

    /// The tiles match happypaint's byte for byte (FNV-1a of its
    /// `paperTile` / `paperLookTile` at tile 140, from paper-field.ts run in
    /// node), but for one texel noted below.
    #[test]
    fn tiles_match_happypaint_byte_for_byte() {
        let want = [
            ("cold", 0xf45e8ab8e3687901u64, 0x6eb157c4ce32fedau64),
            ("hot", 0x244b1fa16e54c7f5, 0xdf70b31c86d9129d),
            ("rough", 0xaa7e8d48238be6e7, 0xaadd21a3f03bbacf),
            ("khadi", 0xb6c40bcf89ad3ddf, 0xfdbcbc628471a69b),
            // One texel of washi's look (131, 56) rounds to 137 here and 136 in
            // happypaint: libm and V8 differ in the last bit of a cos or exp.
            ("washi", 0x4fddb385dd17dda2, 0xe5fb8d5bc3017eab),
            ("laid", 0x65c809b5c515e3e6, 0x775152c146eda7aa),
            ("canvas", 0xf553afdd6393b171, 0x60ea4ed374bdf970),
            ("toned", 0x60e776b8163f8ad5, 0x1861b3ef5db59f8f),
            ("kraft", 0x77c9d2d01477669b, 0x7522b358bf638c4a),
            ("student", 0x25b197b3cd2bfae8, 0x07a4b4b65c26e97f),
        ];
        for (id, h, l) in want {
            let k = PaperKind::parse(id).unwrap();
            assert_eq!(fnv(&paper_tile(k, T)), h, "{id} height");
            assert_eq!(fnv(&paper_look_tile(k, T)), l, "{id} look");
        }
    }

    #[test]
    fn periods_fit_the_tile() {
        assert_eq!(period(5.6, 560), 100);
        assert_eq!(period(140.0, 560), 4);
        assert_eq!(period(11.0, 560), 51);
        assert_eq!(period(5000.0, 560), 1);
    }

    #[test]
    fn every_kind_is_seamless() {
        for k in PaperKind::ALL {
            // Columns T..T+4 are columns 0..4 again.
            let a = paper_heights(k, 0, 0, 4, T, T);
            let b = paper_heights(k, T, 0, 4, T, T);
            for (p, q) in a.iter().zip(&b) {
                assert!((p - q).abs() < 1e-5, "{k:?}");
            }
            // Across the seam it changes no more than inside the tile.
            let h = paper_heights(k, 0, 0, T, T, T);
            let (mut seam, mut inner) = (0.0, 0.0);
            for y in 0..T {
                seam += (h[y * T + T - 1] - h[y * T]).abs();
                inner += (h[y * T + 1] - h[y * T]).abs();
            }
            assert!(seam < inner * 2.5 + 0.5, "{k:?}: {seam} vs {inner}");
        }
    }

    #[test]
    fn a_tile_is_the_same_every_time() {
        assert_eq!(paper_tile(PaperKind::Khadi, T), paper_tile(PaperKind::Khadi, T));
    }

    #[test]
    fn heights_are_in_range_with_a_middling_mean() {
        let std = |v: &[f32]| {
            let m = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32).sqrt()
        };
        let mut stds = HashMap::new();
        for k in PaperKind::ALL {
            let h = paper_heights(k, 0, 0, T, T, T);
            assert!(h.iter().all(|v| (0.0..=1.0).contains(v)));
            let mean = h.iter().sum::<f32>() / h.len() as f32;
            assert!(mean > 0.35 && mean < 0.65, "{k:?}: {mean}");
            stds.insert(k, std(&h));
        }
        assert!(stds[&PaperKind::Hot] < 0.5 * stds[&PaperKind::Rough]);
    }

    #[test]
    fn every_kind_looks_different() {
        let all: Vec<Vec<f32>> = PaperKind::ALL.iter().map(|k| paper_heights(*k, 0, 0, 32, 32, T)).collect();
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                let d = all[i].iter().zip(&all[j]).map(|(a, b)| (a - b).abs()).sum::<f32>() / all[i].len() as f32;
                assert!(d > 0.01, "{:?} vs {:?}: {d}", PaperKind::ALL[i], PaperKind::ALL[j]);
            }
        }
        assert_eq!(paper_tile(PaperKind::Laid, TILE_UNITS).len(), 560 * 560);
    }

    #[test]
    fn looks_are_seamless_with_dark_flecks_where_expected() {
        for k in PaperKind::ALL {
            let l = paper_looks(k, 0, 0, T, T, T);
            assert!(l.iter().all(|v| (0.0..=1.0).contains(v)), "{k:?}");
            let again = paper_looks(k, T, 0, 2, T, T);
            for y in 0..T {
                for x in 0..2 {
                    for c in 0..2 {
                        assert!((again[(y * 2 + x) * 2 + c] - l[(y * T + x) * 2 + c]).abs() < 1e-5, "{k:?}");
                    }
                }
            }
        }
        let min_fleck = |k| paper_looks(k, 0, 0, T, T, T).chunks_exact(2).map(|p| p[1]).fold(1.0f32, f32::min);
        assert!(min_fleck(PaperKind::Khadi) < 0.4);
        assert!(min_fleck(PaperKind::Kraft) < 0.4);
        assert_eq!(min_fleck(PaperKind::Cold), 0.5);
    }
}
