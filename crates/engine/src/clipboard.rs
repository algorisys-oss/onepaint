//! Copy / paste / place image / layer via copy (happypaint PE:4503-4543, 5447-5577).

use super::*;
use onepaint_formats::DecodedImage;

/// Copied pixels: premultiplied linear RGBA over `rect` (doc px).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub rect: Rect,
    pub data: Vec<f32>,
    /// The space of the document it came from (spec 50).
    pub space: onepaint_color::Space,
}

impl Region {
    /// Straight sRGB RGBA8, e.g. for the system clipboard.
    pub fn to_srgb8(&self) -> Vec<u8> {
        onepaint_formats::flatten_to_srgb8_in(&self.data, self.space)
    }
}

impl Session {
    /// The active layer's pixels inside the selection (× coverage), or the
    /// whole layer; `None` when nothing visible would be copied.
    pub fn copy_region(&self) -> Option<Region> {
        let l = self.doc.layer(&self.active)?;
        // The first lifted layer previews through the overlay; the others
        // already show their result in their pixels.
        let mut floating = self.transform.as_ref().and_then(|f| f.lifts.first()).filter(|f| f.layer == l.id).map(|f| f.preview.clone());
        let pixels = match floating.take() {
            // While transforming, copy what is shown.
            Some(p) => {
                let mut s = l.pixels.clone();
                for (k, t) in p.tiles() {
                    s.set_tile(k.0, k.1, Some(t.clone()));
                }
                s
            }
            None => l.pixels.clone(),
        };
        let rect = match &self.clip {
            Some(m) => {
                let (x, y, w, h) = m.bounds()?;
                Rect::new(x as i32, y as i32, w as i32, h as i32)
            }
            None => pixels.content_bounds()?.intersect(&self.doc.bounds())?,
        };
        let mut data = pixels.read_rect(rect);
        if let Some(m) = &self.clip {
            for y in 0..rect.h {
                for x in 0..rect.w {
                    let cov = m.coverage(rect.x + x, rect.y + y);
                    let i = ((y * rect.w + x) * 4) as usize;
                    data[i..i + 4].iter_mut().for_each(|v| *v *= cov);
                }
            }
        }
        data.chunks_exact(4).any(|p| p[3] > 0.0).then_some(Region { rect, data, space: self.doc.space() })
    }

    fn insert_layer_with(&mut self, name: &str, pixels: Surface) {
        let mut l = Layer::new(name);
        l.pixels = pixels;
        let _ = self.insert_above_active(Node::Layer(l));
    }

    /// Paste in place on a new "Pasted" layer, fitted into the canvas (PE:5537-5577).
    pub fn paste_region(&mut self, r: &Region) {
        self.commit_transform();
        let (dw, dh) = (self.doc.width as i32, self.doc.height as i32);
        let (w, h) = (r.rect.w.min(dw), r.rect.h.min(dh));
        if w <= 0 || h <= 0 {
            return;
        }
        let (x, y) = (r.rect.x.clamp(0, dw - w), r.rect.y.clamp(0, dh - h));
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let s = (row * r.rect.w * 4) as usize;
            data.extend_from_slice(&r.data[s..s + (w * 4) as usize]);
        }
        // From a document of another depth: its values mean other colours.
        r.space.convert(&mut data, self.doc.space());
        let mut s = Surface::new();
        s.write_rect(Rect::new(x, y, w, h), &data);
        self.insert_layer_with("Pasted", s);
    }

    /// Place an image as a new "Image" layer, scaled down to fit and centred.
    pub fn place_image(&mut self, img: &DecodedImage) {
        self.commit_transform();
        let (dw, dh) = (self.doc.width as f64, self.doc.height as f64);
        let (iw, ih) = (img.width as f64, img.height as f64);
        let s = 1f64.min(dw / iw).min(dh / ih);
        let (ox, oy) = (((dw - iw * s) / 2.0).round(), ((dh - ih * s) / 2.0).round());
        let surface = if s >= 1.0 {
            onepaint_formats::image_to_surface_in(img, ox as i32, oy as i32, self.doc.space())
        } else {
            let src: Vec<f32> = img.rgba.chunks_exact(4).flat_map(|p| self.doc.space().from_srgb8([p[0], p[1], p[2], p[3]])).collect();
            let bounds = Rect::new(0, 0, img.width as i32, img.height as i32);
            let m = transform::Affine::scale(s, s).then(&transform::Affine::translate(ox, oy));
            transform::resample(src, bounds, m, self.transform_clip(), false)
        };
        self.insert_layer_with("Image", surface);
    }

    /// Ctrl+J / Ctrl+Shift+J: the selected region to a new layer (one undo step).
    pub(crate) fn layer_via_copy(&mut self, cut: bool) -> Result<(), EngineError> {
        if self.clip.is_none() {
            if !cut {
                let id = self.active.clone();
                return self.execute(Command::DuplicateLayer { id });
            }
            return Ok(());
        }
        let Some(region) = self.copy_region() else {
            return Ok(());
        };
        self.history.begin_transaction();
        if cut {
            let _ = self.execute_selection(Command::ClearSelected);
        }
        let mut s = Surface::new();
        s.write_rect(region.rect, &region.data);
        self.insert_layer_with(if cut { "Layer via Cut" } else { "Layer via Copy" }, s);
        self.history.end_transaction();
        self.history_changed();
        Ok(())
    }
}
