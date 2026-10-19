//! Photoshop style libraries (`.asl`), read for their layer styles
//! (happypaint `storage/asl-file.ts`; spec 56 phase 5). The patterns block
//! is skipped, so pattern effects come in without their pattern.
//!
//! Layout, big-endian: u16 version 2, `8BSL`, u16 patterns version, u32
//! patterns length and the patterns, u32 style count, then per style a u32
//! length (padded to 4 bytes) holding two versioned descriptors: `null`
//! with the name (`Nm  `) and `Styl` with the effects (`Lefx`) and blend
//! options.

use crate::psd_desc::{DVal, Desc, read};
use crate::psd_effects::{DEFAULT_LIGHT, effects_in, styles_of};
use onepaint_doc::styles::LayerStyles;

/// One style in a library.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedStyle {
    pub name: String,
    pub styles: LayerStyles,
}

#[derive(Debug, thiserror::Error)]
#[error("Not a Photoshop style library (.asl)")]
pub struct NotAsl;

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// A u32 descriptor version (16) and the descriptor.
fn versioned(d: &[u8], at: usize) -> Option<(Desc, usize)> {
    (u32_at(d, at)? == 16).then_some(())?;
    read(d, at + 4)
}

/// Fill opacity from a style's blend options, 0–1.
fn fill_of(blend: Option<&Desc>) -> Option<f32> {
    let b = blend?;
    let v = b.f64("fillOpacity").or_else(|| b.f64("FlOp"))?;
    v.is_finite().then_some((v / 100.0) as f32)
}

/// The styles in a library, by name. Styles that can't be read, or that
/// change nothing, are left out; one bad style doesn't stop the rest.
pub fn read_asl(d: &[u8]) -> Result<Vec<NamedStyle>, NotAsl> {
    if u16_at(d, 0) != Some(2) || d.get(2..6) != Some(b"8BSL") {
        return Err(NotAsl);
    }
    let patterns = u32_at(d, 8).ok_or(NotAsl)?;
    let mut at = 12 + patterns;
    let count = u32_at(d, at).ok_or(NotAsl)?;
    at += 4;
    let mut out = Vec::new();
    for i in 0..count {
        let Some(len) = u32_at(d, at) else { break };
        let start = at + 4;
        at = start + len;
        let style = (|| {
            let (head, next) = versioned(d.get(..at)?, start)?;
            let (body, _) = versioned(d.get(..at)?, next)?;
            let fx = match body.obj("Lefx") {
                Some(fx) => Some(effects_in(fx, DEFAULT_LIGHT)?),
                None => None,
            };
            let styles = styles_of(fx, fill_of(body.obj("blendOptions")))?;
            let name = head.text("Nm  ").map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| format!("Style {}", i + 1), String::from);
            Some(NamedStyle { name, styles })
        })();
        out.extend(style);
    }
    Ok(out)
}

/// A library holding `styles` (no patterns): the inverse of [`read_asl`],
/// for tests and round trips.
pub fn write_asl(styles: &[NamedStyle]) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend(2u16.to_be_bytes());
    o.extend(b"8BSL");
    o.extend(3u16.to_be_bytes());
    o.extend(0u32.to_be_bytes());
    o.extend((styles.len() as u32).to_be_bytes());
    for s in styles {
        let mut head = Desc::new("null");
        head.push("Nm  ", DVal::Text(s.name.clone())).push("Idnt", DVal::Text(String::new()));
        let mut body = Desc::new("Styl");
        if let Some((fx, _)) = crate::psd_effects::effects_desc(&s.styles) {
            body.push("Lefx", DVal::Obj(fx));
        }
        let mut blend = Desc::new("blendOptions");
        blend.push("fillOpacity", DVal::Unit(*b"#Prc", f64::from(s.styles.fill_opacity) * 100.0));
        body.push("blendOptions", DVal::Obj(blend));
        let mut sec = Vec::new();
        for d in [&head, &body] {
            sec.extend(16u32.to_be_bytes());
            crate::psd_desc::write_into(&mut sec, d);
        }
        while sec.len() % 4 != 0 {
            sec.push(0);
        }
        o.extend((sec.len() as u32).to_be_bytes());
        o.extend(sec);
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use onepaint_doc::styles::{EffectKind, default_effect};

    fn named(name: &str, styles: LayerStyles) -> NamedStyle {
        NamedStyle { name: name.into(), styles }
    }

    /// `write_asl` with `n` bytes of patterns in front of the styles.
    fn with_patterns(styles: &[NamedStyle], n: usize) -> Vec<u8> {
        let mut o = write_asl(styles);
        o[8..12].copy_from_slice(&(n as u32).to_be_bytes());
        o.splice(12..12, std::iter::repeat_n(0u8, n));
        o
    }

    #[test]
    fn every_style_comes_in_by_name_with_its_effects_and_settings() {
        let mut stroke = default_effect(EffectKind::Stroke);
        stroke.size = 5.0;
        stroke.color = [0x12 as f32 / 255.0, 0x34 as f32 / 255.0, 0x56 as f32 / 255.0];
        let outline = LayerStyles { fill_opacity: 1.0, effects: vec![stroke] };
        let fx = |kind, f: &dyn Fn(&mut onepaint_doc::styles::Effect)| {
            let mut e = default_effect(kind);
            e.use_global_light = false;
            f(&mut e);
            e
        };
        let fancy = LayerStyles {
            fill_opacity: 1.0,
            effects: vec![
                fx(EffectKind::Bevel, &|e| {
                    e.size = 7.0;
                    e.angle = 60.0;
                }),
                fx(EffectKind::DropShadow, &|e| {
                    e.distance = 9.0;
                    e.angle = 135.0;
                }),
                fx(EffectKind::DropShadow, &|e| {
                    e.distance = 20.0;
                    e.angle = 135.0;
                }),
            ],
        };
        // The 37 pattern bytes are skipped, not misread.
        let lib = read_asl(&with_patterns(&[named("Outline", outline), named("Fancy", fancy)], 37)).unwrap();
        assert_eq!(lib.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Outline", "Fancy"]);
        let e = &lib[0].styles.effects[0];
        assert_eq!((e.kind, e.size), (EffectKind::Stroke, 5.0));
        assert_eq!(e.color.map(|c| (c * 255.0).round() as u8), [0x12, 0x34, 0x56]);
        let kinds: Vec<_> = lib[1].styles.effects.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [EffectKind::Bevel, EffectKind::DropShadow, EffectKind::DropShadow]);
        assert_eq!(lib[1].styles.effects[2].distance, 20.0);
    }

    #[test]
    fn a_file_that_isnt_a_style_library_is_refused() {
        let e = read_asl(&[0u8; 16]).unwrap_err();
        assert!(e.to_string().contains("Not a Photoshop style library"));
    }

    #[test]
    fn a_bad_style_is_skipped_and_the_rest_come_in() {
        let mut stroke = default_effect(EffectKind::Stroke);
        stroke.size = 3.0;
        let one = LayerStyles { fill_opacity: 1.0, effects: vec![stroke] };
        let mut d = write_asl(&[named("A", one.clone()), named("B", one)]);
        // Break the first style's head descriptor version.
        d[20..24].copy_from_slice(&7u32.to_be_bytes());
        let lib = read_asl(&d).unwrap();
        assert_eq!(lib.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["B"]);
    }

    #[test]
    fn fill_opacity_comes_from_the_blend_options_and_names_default() {
        let s = LayerStyles { fill_opacity: 0.4, effects: vec![] };
        let lib = read_asl(&write_asl(&[named("  ", s)])).unwrap();
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].name, "Style 1");
        assert!((lib[0].styles.fill_opacity - 0.4).abs() < 1e-6);
        // Nothing to apply: left out.
        let none = LayerStyles { fill_opacity: 1.0, effects: vec![] };
        assert!(read_asl(&write_asl(&[named("Empty", none)])).unwrap().is_empty());
    }
}
