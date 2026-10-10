//! Colour adjustment layers packed for the GPU compositor (spec 42). Each
//! kind that depends only on the pixel's own colour gets a code, a block of
//! floats and, for curves and gradient maps, the same 256-entry LUT the CPU
//! samples. `compose.wgsl` implements the codes after `Prepared::color_px`.

use crate::colour::sc_samples;
use crate::{Kind, Lut, MAX_COLOR_SAMPLES, Params, build_curves_lut, build_gradient_lut};

/// Floats per adjustment: selective colour's flag, count and eight samples
/// of target, delta and range, rounded up to whole `vec4`s.
pub const GPU_PARAMS: usize = 68;

#[derive(Clone, Debug, PartialEq)]
pub struct GpuAdjust {
    /// The kind's code in `compose.wgsl` (`ADJ_*`).
    pub code: u32,
    pub params: [f32; GPU_PARAMS],
    pub lut: Option<Box<Lut>>,
}

/// The GPU form of a colour adjustment, or `None` when the kind runs only on
/// the CPU (noise, tone, fills, spatial kinds).
pub fn gpu_adjust(kind: Kind, p: &Params) -> Option<GpuAdjust> {
    let mut f = [0.0f32; GPU_PARAMS];
    let mut lut = None;
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
            put(0, &[p.amount.unwrap_or(1.0)]);
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
        Kind::ColorBalance => {
            put(0, &[p.cyan_red.unwrap_or(0.0), p.magenta_green.unwrap_or(0.0), p.yellow_blue.unwrap_or(0.0)]);
            9
        }
        Kind::GradientMap => {
            lut = Some(Box::new(build_gradient_lut(p)));
            10
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
        _ => return None,
    };
    Some(GpuAdjust { code, params: f, lut })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_only_colour_kinds_pack_and_the_rest_stay_on_the_cpu() {
        let gpu: Vec<Kind> = Kind::ALL.into_iter().filter(|&k| gpu_adjust(k, &crate::default_params(k)).is_some()).collect();
        assert_eq!(gpu.len(), 13);
        assert!(gpu.iter().all(|k| k.is_color() && !k.is_fill()));
        for k in [Kind::Noise, Kind::Tone, Kind::SolidFill, Kind::GaussianBlur] {
            assert!(gpu_adjust(k, &crate::default_params(k)).is_none(), "{k:?}");
        }
        let curves = gpu_adjust(Kind::Curves, &crate::default_params(Kind::Curves)).unwrap();
        assert_eq!(curves.lut.unwrap()[255][0], 255);
    }
}
