//! Colour adjustment layers packed for the GPU compositor (specs 42, 46).
//! Each kind gets a code, a block of floats and, for curves and gradient
//! maps, the same 256-entry LUT the CPU samples (gradient fills: their
//! 1024-entry ramp). `compose.wgsl` implements the codes after
//! `Prepared::color_px`.

use crate::colour::{grad_shape_code, sc_samples};
use crate::gradient::Ramp;
use crate::tex::{from_work, to_work};
use crate::{Frame, Kind, Lut, MAX_COLOR_SAMPLES, Params, build_curves_lut, build_gradient_lut, fill_stops};

/// Floats per adjustment: selective colour's flag, count and eight samples
/// of target, delta and range, rounded up to whole `vec4`s.
pub const GPU_PARAMS: usize = 68;

/// The largest Color Lookup table side the GPU takes.
pub const GPU_LUT_MAX: usize = 65;

#[derive(Clone, Debug, PartialEq)]
pub struct GpuAdjust {
    /// The kind's code in `compose.wgsl` (`ADJ_*`).
    pub code: u32,
    pub params: [f32; GPU_PARAMS],
    pub lut: Option<Box<Lut>>,
    /// A gradient fill's ramp (linear RGBA, spec 46) or a pattern fill's
    /// tile (straight sRGB 0..1, row-major).
    pub table: Option<Vec<[f32; 4]>>,
}

/// The GPU form of a colour adjustment in the document `frame`, or `None`
/// when the kind runs only on the CPU (screentone, pattern fills, spatial
/// kinds).
pub fn gpu_adjust(kind: Kind, p: &Params, frame: Frame) -> Option<GpuAdjust> {
    let mut f = [0.0f32; GPU_PARAMS];
    let mut lut = None;
    let mut table = None;
    let mut put = |at: usize, v: &[f32]| f[at..at + v.len()].copy_from_slice(v);
    let code = match kind {
        Kind::BrightnessContrast => {
            put(0, &[p.brightness.unwrap_or(0.0), p.contrast.unwrap_or(0.0)]);
            1
        }
        Kind::HueSaturation => {
            put(0, &[p.hue.unwrap_or(0.0), p.saturation.unwrap_or(0.0), p.brightness.unwrap_or(0.0)]);
            2
        }
        Kind::BlackWhite => {
            let (prim, sec) = crate::colour::bw_weights(p);
            put(0, &[p.amount.unwrap_or(1.0), prim[0], prim[1], prim[2]]);
            put(4, &[sec[0], sec[1], sec[2], if p.bw_tint.unwrap_or(false) { 1.0 } else { 0.0 }]);
            put(8, &p.bw_tint_color.unwrap_or(crate::BW_DEFAULT_TINT));
            3
        }
        Kind::Invert => 4,
        Kind::Threshold => {
            put(0, &[p.threshold.unwrap_or(0.5)]);
            5
        }
        Kind::Posterize => {
            put(0, &[p.levels.unwrap_or(4.0)]);
            6
        }
        Kind::Levels => {
            let v = [p.in_black.unwrap_or(0.0), p.in_white.unwrap_or(1.0), p.gamma.unwrap_or(1.0)];
            put(0, &v);
            put(3, &[p.out_black.unwrap_or(0.0), p.out_white.unwrap_or(1.0)]);
            7
        }
        Kind::Curves => {
            lut = Some(Box::new(build_curves_lut(p)));
            8
        }
        Kind::ColorLookup => {
            // The table goes in the adjustment table, read trilinear by hand;
            // past 65³ (4 MiB a batch) it stays on the CPU.
            let l = crate::lut3d::lut_of(p.lut.as_ref()).filter(|l| l.size <= GPU_LUT_MAX)?;
            put(0, &[l.size as f32]);
            put(4, &l.domain_min);
            put(8, &[0, 1, 2].map(|i| l.domain_max[i] - l.domain_min[i]));
            table = Some(l.data.iter().map(|c| [c[0], c[1], c[2], 1.0]).collect());
            24
        }
        Kind::CmykSelectiveColor => {
            let (t, abs) = crate::colour::sc_table(p);
            for (i, r) in t.iter().enumerate() {
                put(i * 4, r);
            }
            put(36, &[if abs { 1.0 } else { 0.0 }]);
            23
        }
        Kind::ColorBalance => {
            let v = |x: Option<f32>| x.unwrap_or(0.0);
            let keep = if p.preserve_lum == Some(false) { 0.0 } else { 1.0 };
            put(0, &[v(p.cb_shadow_cr), v(p.cb_shadow_mg), v(p.cb_shadow_yb), keep]);
            put(4, &[v(p.cyan_red), v(p.magenta_green), v(p.yellow_blue)]);
            put(8, &[v(p.cb_high_cr), v(p.cb_high_mg), v(p.cb_high_yb)]);
            9
        }
        Kind::GradientMap => {
            lut = Some(Box::new(build_gradient_lut(p)));
            10
        }
        Kind::Exposure => {
            let gain = 2f32.powf(p.exposure.unwrap_or(0.0));
            put(0, &[gain, p.exp_offset.unwrap_or(0.0), 1.0 / p.exp_gamma.unwrap_or(1.0).max(0.01)]);
            19
        }
        Kind::Vibrance => {
            put(0, &[p.vibrance.unwrap_or(0.0), p.vib_saturation.unwrap_or(0.0)]);
            20
        }
        Kind::PhotoFilter => {
            put(0, &p.filter_color.unwrap_or([236.0 / 255.0, 138.0 / 255.0, 0.0]));
            put(3, &[p.filter_density.unwrap_or(0.25)]);
            put(4, &[if p.preserve_lum.unwrap_or(true) { 1.0 } else { 0.0 }]);
            21
        }
        Kind::ChannelMixer => {
            // Rows r, g, b as vec4s: the three weights and the constant.
            for (i, row) in crate::channel_mixer_rows(p)[..3].iter().enumerate() {
                put(i * 4, row);
            }
            22
        }
        Kind::ColorToAlpha => {
            put(0, &p.target.unwrap_or([1.0; 3]));
            put(3, &[p.threshold.unwrap_or(0.0)]);
            11
        }
        Kind::ReplaceColor => {
            put(0, &p.target.unwrap_or([1.0; 3]));
            put(3, &[p.threshold.unwrap_or(0.3)]);
            put(4, &p.replace.unwrap_or([0.2, 0.45, 1.0]));
            12
        }
        Kind::SelectiveColor => {
            let samples = sc_samples(p);
            put(0, &[if p.show_range.unwrap_or(false) { 1.0 } else { 0.0 }, samples.len() as f32]);
            for (i, s) in samples.iter().take(MAX_COLOR_SAMPLES).enumerate() {
                let (t, d, r) = s.parts();
                put(4 + i * 8, &[t[0], t[1], t[2], r[0], d[0], d[1], d[2], r[1]]);
            }
            13
        }
        Kind::Noise => {
            put(0, &[p.amount.unwrap_or(0.0), if p.mono.unwrap_or(true) { 1.0 } else { 0.0 }]);
            14
        }
        Kind::SolidFill => {
            put(0, &from_work(p.fill_color.unwrap_or([0.0; 3]), frame.space));
            15
        }
        Kind::GradientFill => {
            let [w, h] = frame.size();
            let a = [p.grad_ax.unwrap_or(0.5) * w, p.grad_ay.unwrap_or(0.0) * h];
            let b = [p.grad_bx.unwrap_or(0.5) * w, p.grad_by.unwrap_or(1.0) * h];
            put(0, &[grad_shape_code(p.grad_shape.as_deref()) as f32, a[0], a[1], b[0], b[1]]);
            table = Some(Ramp::new_in(&fill_stops(p), frame.space).entries().to_vec());
            16
        }
        Kind::Tone => {
            // The sand shape hashes with `sin`, whose low bits differ on the GPU.
            let shape = match p.tone_shape.as_deref().unwrap_or("dot") {
                "sand" => return None,
                "line" => 1.0,
                "cross" => 2.0,
                _ => 0.0,
            };
            let density = match p.tone_density.as_deref().unwrap_or("flat") {
                "luminance" => 1.0,
                "gradient" => 2.0,
                _ => 0.0,
            };
            let size = frame.size();
            let g = p.tone_gradient_angle.unwrap_or(0.0).to_radians();
            let dir = [g.sin(), g.cos()];
            let denom = (dir[0].abs() * size[0] + dir[1].abs() * size[1]).max(1.0);
            let dpi = if frame.dpi > 0.0 { frame.dpi } else { 300.0 };
            let cell = (dpi / p.lpi.unwrap_or(60.0).max(5.0)).max(1.0);
            let ang = p.angle.unwrap_or(15.0).to_radians();
            let lvl = p.tone_level.unwrap_or(0.5).clamp(0.0, 1.0);
            let end = p.tone_level_end.unwrap_or(0.0).clamp(0.0, 1.0);
            put(0, &[density, lvl, end, shape, dir[0], dir[1], denom, cell, ang.cos(), ang.sin()]);
            put(10, &[if p.hard_dots.unwrap_or(false) { 1.0 } else { 0.0 }]);
            put(12, &from_work(to_work(p.target.unwrap_or([0.0; 3]), frame.space).map(|v| v.clamp(0.0, 1.0)), frame.space));
            17
        }
        Kind::PatternFill => {
            // An imported image (up to 512², 4 MiB of table) goes in the table as
            // the built-ins do; one that doesn't decode stays on the CPU (empty).
            let tile = match p.pattern_image.as_deref().filter(|u| !u.is_empty()) {
                Some(url) => crate::pattern::imported_tile(url)?,
                None => {
                    let id = p.pattern_id.as_deref().unwrap_or("checker");
                    std::sync::Arc::new(crate::pattern::pattern_pixels(
                        id,
                        p.pattern_color_a.unwrap_or([0.0; 3]),
                        p.pattern_color_b.unwrap_or([1.0; 3]),
                    ))
                }
            };
            let [w, h] = frame.size();
            let scale = p.pattern_scale.unwrap_or(1.0).max(0.05);
            let a = p.pattern_angle.unwrap_or(0.0).to_radians();
            let off = [p.pattern_off_x.unwrap_or(0.0) * w, p.pattern_off_y.unwrap_or(0.0) * h];
            put(0, &[tile.w as f32, tile.h as f32, scale, 0.0, off[0], off[1], a.cos(), a.sin()]);
            table = Some(tile.data.chunks_exact(4).map(|px| [0, 1, 2, 3].map(|k| px[k] as f32 / 255.0)).collect());
            18
        }
        _ => return None,
    };
    Some(GpuAdjust { code, params: f, lut, table })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_kinds_pack_and_the_rest_stay_on_the_cpu() {
        let f = Frame::default();
        let gpu: Vec<Kind> = Kind::ALL.into_iter().filter(|&k| gpu_adjust(k, &crate::default_params(k), f).is_some()).collect();
        // Color Lookup has no table by default, so it stays on the CPU (as a no-op).
        assert_eq!(gpu.len(), 23);
        assert!(gpu.iter().all(|k| k.is_color()));
        for k in [Kind::GaussianBlur, Kind::Clouds] {
            assert!(gpu_adjust(k, &crate::default_params(k), f).is_none(), "{k:?}");
        }
        let sand = Params { tone_shape: Some("sand".into()), ..crate::default_params(Kind::Tone) };
        assert!(gpu_adjust(Kind::Tone, &sand, f).is_none());
        assert_eq!(gpu_adjust(Kind::GradientFill, &crate::default_params(Kind::GradientFill), f).unwrap().table.unwrap().len(), 1024);
        let broken = Params { pattern_image: Some("data:image/png;base64,AAAA".into()), ..crate::default_params(Kind::PatternFill) };
        assert!(gpu_adjust(Kind::PatternFill, &broken, f).is_none(), "an image that doesn't decode stays on the CPU");
        let url = crate::pattern::png_data_url(&[9u8; 3 * 2 * 4], 3, 2);
        let imported = Params { pattern_image: url, ..crate::default_params(Kind::PatternFill) };
        assert_eq!(gpu_adjust(Kind::PatternFill, &imported, f).unwrap().table.unwrap().len(), 6, "an imported image packs");
        let curves = gpu_adjust(Kind::Curves, &crate::default_params(Kind::Curves), f).unwrap();
        assert_eq!(curves.lut.unwrap()[255][0], 255);
    }
}
