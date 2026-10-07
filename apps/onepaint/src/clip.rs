//! Clipboard and file-drop glue: internal regions plus the system clipboard.

use crate::App;
use onepaint_engine::{Command, Region};
use onepaint_formats::DecodedImage;

#[derive(Default)]
pub struct Clipboard {
    region: Option<Region>,
    /// Fingerprint of the image we last put on the system clipboard, so our
    /// own copy pastes in place instead of as a foreign image.
    ours: Option<(usize, usize, u64)>,
    #[cfg(not(test))]
    system: Option<arboard::Clipboard>,
}

fn fingerprint(w: usize, h: usize, bytes: &[u8]) -> (usize, usize, u64) {
    let mut x: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes.iter().step_by(97) {
        x = (x ^ *b as u64).wrapping_mul(0x100_0000_01b3);
    }
    (w, h, x)
}

impl Clipboard {
    #[cfg(not(test))]
    fn system(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.system.is_none() {
            self.system = arboard::Clipboard::new()
                .map_err(|e| log::warn!("system clipboard unavailable: {e}"))
                .ok();
        }
        self.system.as_mut()
    }

    fn set_system_image(&mut self, w: usize, h: usize, rgba: Vec<u8>) {
        self.ours = Some(fingerprint(w, h, &rgba));
        #[cfg(not(test))]
        if let Some(c) = self.system() {
            let _ = c.set_image(arboard::ImageData {
                width: w,
                height: h,
                bytes: rgba.into(),
            });
        }
    }

    /// A system clipboard image that we did not put there ourselves.
    fn foreign_image(&mut self) -> Option<DecodedImage> {
        #[cfg(not(test))]
        {
            let img = self.system()?.get_image().ok()?;
            if Some(fingerprint(img.width, img.height, &img.bytes)) == self.ours {
                return None;
            }
            Some(DecodedImage {
                width: img.width as u32,
                height: img.height as u32,
                rgba: img.bytes.into_owned(),
            })
        }
        #[cfg(test)]
        None
    }
}

pub fn copy(app: &mut App) -> bool {
    let Some(r) = app.session.copy_region() else {
        return false;
    };
    let rgba = r.to_srgb8();
    app.clipboard
        .set_system_image(r.rect.w as usize, r.rect.h as usize, rgba);
    app.clipboard.region = Some(r);
    true
}

pub fn cut(app: &mut App) {
    if copy(app) {
        let _ = app.session.execute(Command::ClearSelected);
    }
}

pub fn paste(app: &mut App) {
    if let Some(img) = app.clipboard.foreign_image() {
        app.session.place_image(&img);
    } else if let Some(r) = app.clipboard.region.clone() {
        app.session.paste_region(&r);
    }
}

/// Place an image file as a layer, or open a `.happypaint` (asking about unsaved work).
pub fn drop_file(
    app: &mut App,
    ctx: &egui::Context,
    path: Option<std::path::PathBuf>,
    bytes: Option<&[u8]>,
) {
    let native = path
        .as_ref()
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("happypaint"));
    if native {
        if let Some(p) = path {
            app.open_later = Some(p);
            app.request(crate::Pending::Open, ctx);
        }
        return;
    }
    let data = match (bytes, &path) {
        (Some(b), _) => Some(b.to_vec()),
        (None, Some(p)) => std::fs::read(p).ok(),
        _ => None,
    };
    match data.map(|d| onepaint_formats::decode_image(&d)) {
        Some(Ok(img)) => app.session.place_image(&img),
        Some(Err(e)) => app.notify(format!("Can't place that file: {e}"), ctx.input(|i| i.time)),
        None => {}
    }
}
