//! Sparse, unbounded, tiled pixel surfaces.
//!
//! Pixels are premultiplied RGBA in 256² tiles behind `Arc`, so cloning a
//! [`Surface`] is a cheap snapshot and writes are copy-on-write. Tiles hold
//! linear `f16`, or 8-bit bytes in 8-bit documents (spec 49).

use std::collections::HashMap;
use std::sync::Arc;

pub const TILE: i32 = 256;
pub const TILE_PX: usize = (TILE * TILE) as usize;

pub type Px = [f32; 4];

mod tile;
pub use tile::{Store, Tile};

/// File key for a tile: tx in the high 16 bits (happypaint `tile-grid.ts:22-25`).
pub fn pack_key(tx: i32, ty: i32) -> u32 {
    (((tx as u32) & 0xffff) << 16) | ((ty as u32) & 0xffff)
}

pub fn unpack_key(key: u32) -> (i32, i32) {
    ((key >> 16) as i32, (key & 0xffff) as i32)
}

/// Tile containing pixel (x, y).
pub fn tile_of(x: i32, y: i32) -> (i32, i32) {
    (x.div_euclid(TILE), y.div_euclid(TILE))
}

/// Integer rectangle in document pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_tile(tx: i32, ty: i32) -> Self {
        Self::new(tx * TILE, ty * TILE, TILE, TILE)
    }

    /// Smallest integer rect covering a float box.
    pub fn covering(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        let (x0, y0) = (x0.floor() as i32, y0.floor() as i32);
        let (x1, y1) = (x1.ceil() as i32, y1.ceil() as i32);
        Self::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Grown by `n` px on every side (an empty rect stays empty).
    pub fn inflate(&self, n: i32) -> Rect {
        if self.is_empty() { *self } else { Rect::new(self.x - n, self.y - n, self.w + 2 * n, self.h + 2 * n) }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        Rect::new(x0, y0, self.right().max(o.right()) - x0, self.bottom().max(o.bottom()) - y0)
    }

    /// Tiles this rect touches, row-major.
    pub fn tiles(&self) -> impl Iterator<Item = (i32, i32)> + use<> {
        let (tx0, ty0) = tile_of(self.x, self.y);
        let (tx1, ty1) = if self.is_empty() { (tx0 - 1, ty0 - 1) } else { tile_of(self.right() - 1, self.bottom() - 1) };
        (ty0..=ty1).flat_map(move |ty| (tx0..=tx1).map(move |tx| (tx, ty)))
    }
}

/// Sparse unbounded surface. A missing tile is transparent.
#[derive(Clone, Default, Debug)]
pub struct Surface {
    tiles: HashMap<(i32, i32), Arc<Tile>>,
}

impl Surface {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn tile(&self, tx: i32, ty: i32) -> Option<&Arc<Tile>> {
        self.tiles.get(&(tx, ty))
    }

    /// Mutable tile, created transparent if missing; copy-on-write if shared.
    pub fn tile_mut(&mut self, tx: i32, ty: i32) -> &mut Tile {
        Arc::make_mut(self.tiles.entry((tx, ty)).or_default())
    }

    /// Mutable tiles at `keys` (created transparent if missing; copy-on-write
    /// if shared), disjoint, for working them in parallel. In no set order.
    pub fn tiles_mut(&mut self, keys: &[(i32, i32)]) -> Vec<((i32, i32), &mut Tile)> {
        for &k in keys {
            self.tiles.entry(k).or_default();
        }
        let want: std::collections::HashSet<(i32, i32)> = keys.iter().copied().collect();
        self.tiles.iter_mut().filter(|(k, _)| want.contains(*k)).map(|(k, t)| (*k, Arc::make_mut(t))).collect()
    }

    pub fn set_tile(&mut self, tx: i32, ty: i32, tile: Option<Arc<Tile>>) {
        match tile {
            Some(t) => {
                self.tiles.insert((tx, ty), t);
            }
            None => {
                self.tiles.remove(&(tx, ty));
            }
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = ((i32, i32), &Arc<Tile>)> {
        self.tiles.iter().map(|(k, v)| (*k, v))
    }

    pub fn tile_keys(&self) -> Vec<(i32, i32)> {
        let mut k: Vec<_> = self.tiles.keys().copied().collect();
        k.sort_by_key(|&(x, y)| (y, x));
        k
    }

    pub fn clear(&mut self) {
        self.tiles.clear();
    }

    pub fn get(&self, x: i32, y: i32) -> Px {
        let (tx, ty) = tile_of(x, y);
        match self.tiles.get(&(tx, ty)) {
            Some(t) => t.get((x - tx * TILE) as usize, (y - ty * TILE) as usize),
            None => [0.0; 4],
        }
    }

    pub fn set(&mut self, x: i32, y: i32, p: Px) {
        let (tx, ty) = tile_of(x, y);
        if p[3] <= 0.0 && p[0] == 0.0 && p[1] == 0.0 && p[2] == 0.0 && !self.tiles.contains_key(&(tx, ty)) {
            return;
        }
        self.tile_mut(tx, ty).set((x - tx * TILE) as usize, (y - ty * TILE) as usize, p);
    }

    /// Drop tiles whose every pixel has zero alpha.
    pub fn prune_empty(&mut self) {
        self.tiles.retain(|_, t| !t.is_empty());
    }

    pub fn prune_tile(&mut self, tx: i32, ty: i32) {
        if self.tiles.get(&(tx, ty)).is_some_and(|t| t.is_empty()) {
            self.tiles.remove(&(tx, ty));
        }
    }

    /// Read a rect as row-major premultiplied f32 RGBA.
    pub fn read_rect(&self, r: Rect) -> Vec<f32> {
        let mut out = vec![0.0; (r.w.max(0) * r.h.max(0) * 4) as usize];
        for (tx, ty) in r.tiles() {
            let Some(t) = self.tiles.get(&(tx, ty)) else {
                continue;
            };
            let Some(ix) = r.intersect(&Rect::from_tile(tx, ty)) else {
                continue;
            };
            for y in ix.y..ix.bottom() {
                for x in ix.x..ix.right() {
                    let p = t.get((x - tx * TILE) as usize, (y - ty * TILE) as usize);
                    let o = (((y - r.y) * r.w + (x - r.x)) * 4) as usize;
                    out[o..o + 4].copy_from_slice(&p);
                }
            }
        }
        out
    }

    /// Write row-major premultiplied f32 RGBA into a rect (fully transparent tiles are skipped).
    pub fn write_rect(&mut self, r: Rect, data: &[f32]) {
        assert_eq!(data.len(), (r.w * r.h * 4) as usize, "write_rect: data size");
        for (tx, ty) in r.tiles() {
            let Some(ix) = r.intersect(&Rect::from_tile(tx, ty)) else {
                continue;
            };
            let any = (ix.y..ix.bottom()).any(|y| {
                (ix.x..ix.right()).any(|x| {
                    let o = (((y - r.y) * r.w + (x - r.x)) * 4) as usize;
                    data[o..o + 4].iter().any(|&v| v != 0.0)
                })
            });
            if !any && !self.tiles.contains_key(&(tx, ty)) {
                continue;
            }
            let t = self.tile_mut(tx, ty);
            for y in ix.y..ix.bottom() {
                for x in ix.x..ix.right() {
                    let o = (((y - r.y) * r.w + (x - r.x)) * 4) as usize;
                    t.set((x - tx * TILE) as usize, (y - ty * TILE) as usize, [data[o], data[o + 1], data[o + 2], data[o + 3]]);
                }
            }
        }
    }

    /// A copy shifted by whole pixels (fast path for tile-aligned offsets).
    pub fn translated(&self, dx: i32, dy: i32) -> Surface {
        if dx % TILE == 0 && dy % TILE == 0 {
            let (tx, ty) = (dx / TILE, dy / TILE);
            return Surface { tiles: self.tiles.iter().map(|(&(x, y), t)| ((x + tx, y + ty), t.clone())).collect() };
        }
        let mut out = Surface::new();
        for &(tx, ty) in self.tiles.keys() {
            let r = Rect::from_tile(tx, ty);
            let data = self.read_rect(r);
            out.write_rect(Rect::new(r.x + dx, r.y + dy, TILE, TILE), &data);
        }
        out
    }

    /// Tight bounds of pixels with non-zero alpha.
    pub fn content_bounds(&self) -> Option<Rect> {
        let mut acc: Option<Rect> = None;
        for (&(tx, ty), t) in &self.tiles {
            let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
            for ly in 0..TILE as usize {
                for lx in 0..TILE as usize {
                    if t.get(lx, ly)[3] > 0.0 {
                        x0 = x0.min(lx as i32);
                        x1 = x1.max(lx as i32);
                        y0 = y0.min(ly as i32);
                        y1 = y1.max(ly as i32);
                    }
                }
            }
            if x0 <= x1 {
                let r = Rect::new(tx * TILE + x0, ty * TILE + y0, x1 - x0 + 1, y1 - y0 + 1);
                acc = Some(acc.map_or(r, |a| a.union(&r)));
            }
        }
        acc
    }
}
