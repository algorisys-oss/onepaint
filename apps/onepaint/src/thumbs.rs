//! Cached UI images: layer thumbnails and brush stroke previews.

use egui::{ColorImage, TextureHandle, TextureOptions};
use onepaint_brush::{BrushPreset, StrokeEngine, rasterize_dab};
use onepaint_doc::Layer;
use onepaint_raster::{Rect, Surface};
use onepaint_stroke::StrokePoint;
use std::collections::HashMap;

pub const THUMB: [usize; 2] = [44, 34];

#[derive(Default)]
pub struct Thumbs {
    layers: HashMap<String, TextureHandle>,
    brushes: HashMap<String, TextureHandle>,
    masks: HashMap<String, TextureHandle>,
}

fn to_image(s: &Surface, area: Rect, w: usize, h: usize) -> ColorImage {
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let sx = area.x + ((x as f32 + 0.5) / w as f32 * area.w as f32) as i32;
            let sy = area.y + ((y as f32 + 0.5) / h as f32 * area.h as f32) as i32;
            let [r, g, b, a] = onepaint_formats_free::to_srgb8(s.get(sx, sy));
            px.push(egui::Color32::from_rgba_unmultiplied(r, g, b, a));
        }
    }
    ColorImage::new([w, h], px)
}

/// Tiny local helper so the app doesn't need the formats crate for one conversion.
mod onepaint_formats_free {
    pub fn to_srgb8(p: [f32; 4]) -> [u8; 4] {
        let a = p[3].clamp(0.0, 1.0);
        if a <= 0.0 {
            return [0, 0, 0, 0];
        }
        let q = |v: f32| (onepaint_color::linear_to_srgb(v / a).clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(p[0]), q(p[1]), q(p[2]), (a * 255.0).round() as u8]
    }
}

impl Thumbs {
    pub fn clear(&mut self) {
        self.layers.clear();
        self.masks.clear();
    }

    pub fn invalidate_active(&mut self, id: &str) {
        self.layers.remove(id);
        self.masks.remove(id);
    }

    /// Grey thumbnail of a layer mask (coverage in R).
    pub fn mask(&mut self, ctx: &egui::Context, l: &Layer, doc: [u32; 2]) -> Option<TextureHandle> {
        let m = l.mask.as_ref()?;
        if let Some(t) = self.masks.get(&l.id) {
            return Some(t.clone());
        }
        let (dw, dh) = (doc[0] as f32, doc[1] as f32);
        let s = (THUMB[0] as f32 / dw).min(THUMB[1] as f32 / dh);
        let (w, h) = (((dw * s).round() as usize).max(1), ((dh * s).round() as usize).max(1));
        let mut px = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let sx = ((x as f32 + 0.5) / w as f32 * dw) as i32;
                let sy = ((y as f32 + 0.5) / h as f32 * dh) as i32;
                let v = (m.get(sx, sy)[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                px.push(egui::Color32::from_gray(v));
            }
        }
        let tex = ctx.load_texture(format!("mask-thumb-{}", l.id), ColorImage::new([w, h], px), TextureOptions::LINEAR);
        self.masks.insert(l.id.clone(), tex.clone());
        Some(tex)
    }

    pub fn layer(&mut self, ctx: &egui::Context, l: &Layer, doc: [u32; 2]) -> TextureHandle {
        if let Some(t) = self.layers.get(&l.id) {
            return t.clone();
        }
        let (dw, dh) = (doc[0] as f32, doc[1] as f32);
        let s = (THUMB[0] as f32 / dw).min(THUMB[1] as f32 / dh);
        let (w, h) = (((dw * s).round() as usize).max(1), ((dh * s).round() as usize).max(1));
        let img = to_image(&l.pixels, Rect::new(0, 0, doc[0] as i32, doc[1] as i32), w, h);
        let tex = ctx.load_texture(format!("layer-thumb-{}", l.id), img, TextureOptions::LINEAR);
        self.layers.insert(l.id.clone(), tex.clone());
        tex
    }

    /// A white sample stroke, tinted at draw time.
    pub fn brush(&mut self, ctx: &egui::Context, b: &BrushPreset) -> TextureHandle {
        if let Some(t) = self.brushes.get(&b.id) {
            return t.clone();
        }
        let (w, h) = (220usize, 48usize);
        let mut preset = b.clone();
        preset.size = preset.size.min(22.0);
        preset.stabilizer = None;
        let mut eng = StrokeEngine::new(preset.clone(), [1.0, 1.0, 1.0], 7);
        let mut s = Surface::new();
        let n = 60;
        let mut dabs = Vec::new();
        for i in 0..=n {
            let f = i as f64 / n as f64;
            let x = 16.0 + f * (w as f64 - 32.0);
            let y = h as f64 / 2.0 + (f * std::f64::consts::TAU).sin() * 9.0;
            let p = StrokePoint::at(x, y, (f * std::f64::consts::PI).sin().max(0.05), f * 400.0);
            dabs.extend(if i == 0 { eng.begin(p) } else { eng.add(p) });
        }
        dabs.extend(eng.end());
        for d in &dabs {
            rasterize_dab(&mut s, d, preset.wash, None);
        }
        let mut px = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let a = (s.get(x as i32, y as i32)[3] * preset.opacity as f32).clamp(0.0, 1.0);
                px.push(egui::Color32::from_white_alpha((a * 255.0) as u8));
            }
        }
        let tex = ctx.load_texture(format!("brush-{}", b.id), ColorImage::new([w, h], px), TextureOptions::LINEAR);
        self.brushes.insert(b.id.clone(), tex.clone());
        tex
    }
}
