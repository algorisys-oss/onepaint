// Blend modes and the sRGB transfer, shared by the compositor and the
// layer-style pass (spec 56). A shader including this declares
// `override PIGMENT: bool` and includes the pigment code.

const M_NORMAL: u32 = 0u;
const M_MULTIPLY: u32 = 1u;
const M_SCREEN: u32 = 2u;
const M_OVERLAY: u32 = 3u;
const M_ADD: u32 = 4u;
const M_DARKEN: u32 = 5u;
const M_LIGHTEN: u32 = 6u;
const M_DODGE: u32 = 7u;
const M_BURN: u32 = 8u;
const M_HARD_LIGHT: u32 = 9u;
const M_SOFT_LIGHT: u32 = 10u;
const M_DIFFERENCE: u32 = 11u;
const M_EXCLUSION: u32 = 12u;
const M_LINEAR_BURN: u32 = 13u;
const M_DIVIDE: u32 = 14u;
const M_HUE: u32 = 15u;
const M_SATURATION: u32 = 16u;
const M_COLOR: u32 = 17u;
const M_LUMINOSITY: u32 = 18u;
const M_PIGMENT: u32 = 19u;
const M_VIVID_LIGHT: u32 = 20u;
const M_LINEAR_LIGHT: u32 = 21u;
const M_PIN_LIGHT: u32 = 22u;
const M_HARD_MIX: u32 = 23u;
const M_SUBTRACT: u32 = 24u;
const M_DARKER_COLOR: u32 = 25u;
const M_LIGHTER_COLOR: u32 = 26u;
const M_DISSOLVE: u32 = 27u;


fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// ── sRGB transfer (crates/color/src/lib.rs) ──

fn lin2srgb(x: f32) -> f32 {
    let c = max(x, 0.0);
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

fn srgb2lin(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

// ── Blend modes (crates/compose/src/blend.rs) ──

fn bc(m: u32, b: f32, s: f32) -> f32 {
    switch m {
        case M_MULTIPLY: { return b * s; }
        case M_SCREEN: { return b + s - b * s; }
        case M_OVERLAY: {
            if (b <= 0.5) { return 2.0 * b * s; }
            return 1.0 - 2.0 * (1.0 - b) * (1.0 - s);
        }
        case M_DARKEN: { return min(b, s); }
        case M_LIGHTEN: { return max(b, s); }
        case M_DODGE: {
            if (s >= 1.0) { return 1.0; }
            return min(b / (1.0 - s), 1.0);
        }
        case M_BURN: {
            if (s <= 0.0) { return 0.0; }
            return 1.0 - min((1.0 - b) / s, 1.0);
        }
        case M_HARD_LIGHT: {
            if (s <= 0.5) { return 2.0 * b * s; }
            return 1.0 - 2.0 * (1.0 - b) * (1.0 - s);
        }
        case M_SOFT_LIGHT: {
            if (s <= 0.5) { return b - (1.0 - 2.0 * s) * b * (1.0 - b); }
            let d = select(sqrt(b), ((16.0 * b - 12.0) * b + 4.0) * b, b <= 0.25);
            return b + (2.0 * s - 1.0) * (d - b);
        }
        case M_DIFFERENCE: { return abs(b - s); }
        case M_EXCLUSION: { return b + s - 2.0 * b * s; }
        case M_LINEAR_BURN: { return max(b + s - 1.0, 0.0); }
        case M_DIVIDE: {
            if (s <= 0.0) { return 1.0; }
            return min(b / s, 1.0);
        }
        case M_ADD: { return b + s; }
        case M_VIVID_LIGHT: {
            if (s <= 0.5) {
                let x = 2.0 * s;
                if (x <= 0.0) { return 0.0; }
                return 1.0 - min((1.0 - b) / x, 1.0);
            }
            let x = 2.0 * (s - 0.5);
            if (x >= 1.0) { return 1.0; }
            return min(b / (1.0 - x), 1.0);
        }
        case M_LINEAR_LIGHT: { return clamp(b + 2.0 * s - 1.0, 0.0, 1.0); }
        case M_PIN_LIGHT: {
            if (s <= 0.5) { return min(b, 2.0 * s); }
            return max(b, 2.0 * s - 1.0);
        }
        case M_HARD_MIX: { return select(0.0, 1.0, b + s >= 1.0); }
        case M_SUBTRACT: { return max(b - s, 0.0); }
        default: { return s; }
    }
}

fn lum(c: vec3<f32>) -> f32 {
    return 0.3 * c.x + 0.59 * c.y + 0.11 * c.z;
}

fn clip_color(c0: vec3<f32>) -> vec3<f32> {
    var c = c0;
    let l = lum(c);
    let n = min(c.x, min(c.y, c.z));
    let x = max(c.x, max(c.y, c.z));
    if (n < 0.0) {
        c = vec3<f32>(l) + (c - vec3<f32>(l)) * l / max(l - n, 1e-6);
    }
    if (x > 1.0) {
        c = vec3<f32>(l) + (c - vec3<f32>(l)) * (1.0 - l) / max(x - l, 1e-6);
    }
    return c;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_color(c + vec3<f32>(l - lum(c)));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(c.x, max(c.y, c.z)) - min(c.x, min(c.y, c.z));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mn = min(c.x, min(c.y, c.z));
    let mx = max(c.x, max(c.y, c.z));
    if (mx > mn) {
        return (c - vec3<f32>(mn)) * s / (mx - mn);
    }
    return vec3<f32>(0.0);
}

fn blend_color(m: u32, b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    switch m {
        case M_HUE: { return set_lum(set_sat(s, sat(b)), lum(b)); }
        case M_SATURATION: { return set_lum(set_sat(b, sat(s)), lum(b)); }
        case M_COLOR: { return set_lum(s, lum(b)); }
        case M_LUMINOSITY: { return set_lum(b, lum(s)); }
        case M_PIGMENT: {
            if (PIGMENT) {
                return pigment_mix(b, s, 0.5);
            }
            return s;
        }
        case M_DARKER_COLOR: { return select(b, s, lum(s) < lum(b)); }
        case M_LIGHTER_COLOR: { return select(b, s, lum(s) > lum(b)); }
        default: { return vec3<f32>(bc(m, b.x, s.x), bc(m, b.y, s.y), bc(m, b.z, s.z)); }
    }
}

// Where a point falls along a gradient (filters gradient.rs `shape_t`): `d`
// from the start, `ab` the drag; 0 linear, 1 radial, 2 angle, 3 reflected, 4 diamond.
fn shape_t(shape: u32, d: vec2<f32>, ab: vec2<f32>) -> f32 {
    let len2 = max(dot(ab, ab), 1e-4);
    let along = dot(d, ab) / len2;
    let across = (d.x * ab.y - d.y * ab.x) / len2;
    switch shape {
        case 1u: { return clamp(length(d) / sqrt(len2), 0.0, 1.0); }
        case 2u: {
            if (dot(d, d) < 1e-8) {
                return 0.0;
            }
            let turn = (atan2(ab.y, ab.x) - atan2(d.y, d.x)) / 6.283185307179586;
            return turn - floor(turn);
        }
        case 3u: { return clamp(abs(along), 0.0, 1.0); }
        case 4u: { return clamp(abs(along) + abs(across), 0.0, 1.0); }
        default: { return clamp(along, 0.0, 1.0); }
    }
}

