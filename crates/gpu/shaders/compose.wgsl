// GPU compositor: one invocation per pixel interprets its tile's program
// (crates/compose/src/program.rs) with a register stack. The blend maths are
// crates/compose/src/blend.rs line for line; the CPU compositor is the
// oracle in the tests. Pixels are premultiplied linear light.
//
// Op layout (four vec4<u32>):
//   a: kind, blend mode, opacity (f32 bits), flags
//   b: base slot, stroke slot, stroke code, stroke opacity
//   c: mask slot, mask stroke slot, mask stroke code, mask stroke opacity
//   d: ghost tint rgb (f32 bits); for content with F_RELIEF, the height
//      slots (centre | left << 16, right | up << 16, down), 0xffff for none
// A stroke code is `kind | mode << 8` (kind 0 paint, 1 erase, 2 alpha lock).
//
// Spec 42: an adjust op (a colour adjustment layer) has its params at
// `adj[b.x..]` (vec4s), its LUT at `adj[b.y..]` (256 entries, or NONE), its
// kind code in b.z, and its mask slot in c.x when F_MASK is set.
//
// Spec 38: a job may start from a cached segment image (`init`), lay a
// cached image over its result (`OP_OVER_CACHE`, slot in a.y), and write to
// a cache slot instead of the canvas (`dest`). NONE means "not used".

struct Params {
    // The target's size at this level.
    doc_size: vec2<u32>,
    per_atlas: u32,
    general_base: u32,
    // Level of detail (spec 39): tiles are 256 >> level pixels, read from mip `level`.
    level: u32,
    // Spec 40: the canvas is split into sheets (array layers), `cols`
    // across, of size `sheet` at this level.
    cols: u32,
    sheet: vec2<u32>,
    // Spec 50: 1 when pixel values are sRGB (8-bit documents) and blend as they are.
    gamma: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
}

struct Job {
    origin: vec2<u32>,
    op_start: u32,
    op_count: u32,
    init: u32,
    dest: u32,
}

struct Op {
    a: vec4<u32>,
    b: vec4<u32>,
    c: vec4<u32>,
    d: vec4<u32>,
}

struct Km {
    b: array<vec4<f32>, 9>,
    t: array<vec4<f32>, 9>,
}

@group(0) @binding(0) var<uniform> prm: Params;
@group(0) @binding(1) var<uniform> km: Km;
@group(0) @binding(2) var<storage, read> jobs: array<Job>;
@group(0) @binding(3) var<storage, read> ops: array<Op>;
@group(0) @binding(4) var out_tex: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(5) var atlas0: texture_2d_array<f32>;
@group(0) @binding(6) var atlas1: texture_2d_array<f32>;
@group(0) @binding(7) var atlas2: texture_2d_array<f32>;
@group(0) @binding(8) var atlas3: texture_2d_array<f32>;
@group(0) @binding(9) var atlas4: texture_2d_array<f32>;
@group(0) @binding(10) var atlas5: texture_2d_array<f32>;
@group(0) @binding(11) var atlas6: texture_2d_array<f32>;
@group(0) @binding(12) var atlas7: texture_2d_array<f32>;
// Cached segment images: read in the composite pass, written in the fill
// pass (each pass binds a stand-in for the other).
@group(0) @binding(13) var cache_tex: texture_2d_array<f32>;
@group(0) @binding(14) var cache_out: texture_storage_2d_array<rgba16float, write>;
// Colour adjustment params and LUTs (spec 42).
@group(0) @binding(15) var<storage, read> adj: array<vec4<f32>>;
// The document paper's grain (spec 43), R32Float, repeat-sampled by hand.
@group(0) @binding(16) var paper_tex: texture_2d<f32>;

const NONE: u32 = 0xffffffffu;

const OP_LAYER: u32 = 0u;
const OP_PUSH_ZERO: u32 = 1u;
const OP_PUSH_CONTENT: u32 = 2u;
const OP_CLIP: u32 = 3u;
const OP_POP: u32 = 4u;
const OP_OVER_CACHE: u32 = 5u;
const OP_ADJUST: u32 = 6u;
const OP_PAPER: u32 = 7u;
const OP_COVERAGE: u32 = 8u;
const OP_PUSH_COPY: u32 = 9u;
const OP_MIX: u32 = 10u;
const OP_KNOCKOUT: u32 = 11u;
const OP_SNAPSHOT: u32 = 12u;

const F_INHERIT: u32 = 1u;     // Layer: inherit alpha; Pop: clip instead of blend
const F_TINT: u32 = 2u;
const F_BASE: u32 = 4u;
const F_STROKE: u32 = 8u;
const F_MASK: u32 = 16u;
const F_MASK_ZERO: u32 = 32u;
const F_MASK_STROKE: u32 = 64u;
const F_RELIEF: u32 = 128u;

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

const STACK: u32 = 8u;

// Off unless the batch has a Pigment op: the Kubelka–Munk code is large, and
// leaving it out keeps every other program fast.
override PIGMENT: bool = true;

// Off for the lean pipeline, whose jobs are all plain normal layers (base
// tile, no stroke, mask or inherit alpha). Leaving the interpreter out keeps
// register use low, which nearly halves the time of a typical recomposite.
// The general jobs follow the lean ones in `jobs`, from `general_base`.
override GENERAL: bool = true;

fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn load(slot: u32, p: vec2<i32>) -> vec4<f32> {
    let l = i32(slot % prm.per_atlas);
    let m = i32(prm.level);
    switch slot / prm.per_atlas {
        case 0u: { return textureLoad(atlas0, p, l, m); }
        case 1u: { return textureLoad(atlas1, p, l, m); }
        case 2u: { return textureLoad(atlas2, p, l, m); }
        case 3u: { return textureLoad(atlas3, p, l, m); }
        case 4u: { return textureLoad(atlas4, p, l, m); }
        case 5u: { return textureLoad(atlas5, p, l, m); }
        case 6u: { return textureLoad(atlas6, p, l, m); }
        default: { return textureLoad(atlas7, p, l, m); }
    }
}

fn load_cache(slot: u32, p: vec2<i32>) -> vec4<f32> {
    return textureLoad(cache_tex, p, i32(slot), i32(prm.level));
}

// ── sRGB transfer (crates/color/src/lib.rs) ──

fn lin2srgb(x: f32) -> f32 {
    let c = max(x, 0.0);
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

fn srgb2lin(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

fn prem_lin_to_srgb(p: vec4<f32>) -> vec4<f32> {
    let a = p.w;
    let s = select(p.xyz, p.xyz / a, a > 0.0);
    return vec4<f32>(vec3<f32>(lin2srgb(s.x), lin2srgb(s.y), lin2srgb(s.z)) * a, a);
}

fn prem_srgb_to_lin(p: vec4<f32>) -> vec4<f32> {
    let a = p.w;
    let s = select(p.xyz, p.xyz / a, a > 0.0);
    return vec4<f32>(vec3<f32>(srgb2lin(s.x), srgb2lin(s.y), srgb2lin(s.z)) * a, a);
}

fn straight(p: vec4<f32>) -> vec3<f32> {
    return select(vec3<f32>(0.0), p.xyz / p.w, p.w > 0.0);
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

// Gamma space (spec 50): every mode on the stored sRGB values; multiply,
// screen and add are happypaint's fixed-function GL blends, clamped like an
// 8-bit target (compose blend.rs `blend_px_in`).
fn blend_gamma(m: u32, bp: vec4<f32>, sp: vec4<f32>) -> vec4<f32> {
    switch m {
        case M_NORMAL: { return sp + bp * (1.0 - sp.w); }
        case M_ADD: { return vec4<f32>(sp.xyz + bp.xyz, sp.w + bp.w * (1.0 - sp.w)); }
        case M_MULTIPLY: { return sp * bp + bp * (1.0 - sp.w); }
        case M_SCREEN: { return sp + bp * (1.0 - sp); }
        default: {
            let cs = straight(sp);
            let cb = straight(bp);
            let bl = blend_color(m, cb, cs);
            let o = sp.w * (1.0 - bp.w) * cs + sp.w * bp.w * bl + (1.0 - sp.w) * bp.w * cb;
            return vec4<f32>(o, sp.w + bp.w * (1.0 - sp.w));
        }
    }
}

// This invocation's pixel in level-0 document px (Dissolve's dither).
var<private> doc_px: vec2<i32>;

// Dissolve's threshold (compose blend.rs `dissolve_noise`; `pcg` is below).
fn dissolve_hit(a: f32) -> bool {
    let r = f32(pcg(bitcast<u32>(doc_px.x) ^ pcg(bitcast<u32>(doc_px.y))) >> 8u) / 16777216.0;
    return a > 0.0 && r < a;
}

// A reduced level previews Dissolve as Normal: the dither's average.
fn previewed(m: u32) -> u32 {
    return select(m, M_NORMAL, m == M_DISSOLVE && prm.level != 0u);
}

fn blend_px(mode: u32, bp: vec4<f32>, src: vec4<f32>, opacity: f32) -> vec4<f32> {
    let m = previewed(mode);
    if (m == M_DISSOLVE) {
        // Wholly the source colour or wholly the backdrop (blend.rs `blend_px_at`).
        if (dissolve_hit(src.w * opacity)) {
            let o = vec4<f32>(straight(src), 1.0);
            if (prm.gamma != 0u) {
                return clamp(o, vec4<f32>(0.0), vec4<f32>(1.0));
            }
            return o;
        }
        return bp;
    }
    let sp = src * opacity;
    if (sp.w <= 0.0 && sp.x == 0.0 && sp.y == 0.0 && sp.z == 0.0) {
        return bp;
    }
    if (prm.gamma != 0u) {
        return clamp(blend_gamma(m, bp, sp), vec4<f32>(0.0), vec4<f32>(1.0));
    }
    if (m == M_NORMAL) {
        return sp + bp * (1.0 - sp.w);
    }
    if (m == M_ADD) {
        return vec4<f32>(sp.xyz + bp.xyz, sp.w + bp.w * (1.0 - sp.w));
    }
    let s = prem_lin_to_srgb(sp);
    let b = prem_lin_to_srgb(bp);
    let a_s = s.w;
    let ab = b.w;
    let cs = straight(s);
    let cb = straight(b);
    let bl = blend_color(m, cb, cs);
    let ao = a_s + ab * (1.0 - a_s);
    let o = a_s * (1.0 - ab) * cs + a_s * ab * bl + (1.0 - a_s) * ab * cb;
    return prem_srgb_to_lin(vec4<f32>(o, ao));
}

fn blend_px_alpha_lock(mode: u32, bp: vec4<f32>, src: vec4<f32>, opacity: f32) -> vec4<f32> {
    let m = previewed(mode);
    if (m == M_DISSOLVE) {
        if (dissolve_hit(src.w * opacity)) {
            let o = vec4<f32>(straight(src) * bp.w, bp.w);
            if (prm.gamma != 0u) {
                return clamp(o, vec4<f32>(0.0), vec4<f32>(1.0));
            }
            return o;
        }
        return bp;
    }
    let sp = src * opacity;
    if (sp.w <= 0.0) {
        return bp;
    }
    if (m == M_NORMAL) {
        return vec4<f32>(sp.xyz * bp.w + bp.xyz * (1.0 - sp.w), bp.w);
    }
    if (prm.gamma != 0u) {
        let bl = blend_color(m, straight(bp), straight(sp));
        return clamp(vec4<f32>((sp.w * bl + (1.0 - sp.w) * straight(bp)) * bp.w, bp.w), vec4<f32>(0.0), vec4<f32>(1.0));
    }
    let s = prem_lin_to_srgb(sp);
    let b = prem_lin_to_srgb(bp);
    let a_s = s.w;
    let ab = b.w;
    let cs = straight(s);
    let cb = straight(b);
    let bl = blend_color(m, cb, cs);
    return prem_srgb_to_lin(vec4<f32>((a_s * bl + (1.0 - a_s) * cb) * ab, ab));
}

fn clip_over(d: vec4<f32>, src: vec4<f32>, opacity: f32) -> vec4<f32> {
    return src * opacity * d.w + d * (1.0 - src.w * opacity);
}

fn apply_stroke(dst: vec4<f32>, src: vec4<f32>, opacity: f32, code: u32) -> vec4<f32> {
    let m = code >> 8u;
    switch code & 0xffu {
        case 0u: { return blend_px(m, dst, src, opacity); }
        case 1u: { return dst * (1.0 - src.w * opacity); }
        default: { return blend_px_alpha_lock(m, dst, src, opacity); }
    }
}

// ── Colour adjustment layers (spec 42; crates/filters `Prepared::color_px`) ──

const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

fn luma(c: vec3<f32>) -> f32 {
    return dot(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), LUMA);
}

fn c01(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

// GLSL rgb2hsv / hsv2rgb, hue in turns (filters colour.rs).
fn rgb2hsv(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.b, c.g, k.w, k.z), vec4<f32>(c.g, c.b, k.x, k.y), step(c.b, c.g));
    let q = mix(vec4<f32>(p.x, p.y, p.w, c.r), vec4<f32>(c.r, p.y, p.z, p.x), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + 1e-10)), d / (q.x + 1e-10), q.x);
}

fn hsv2rgb(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * (k.xxx + (clamp(p - k.xxx, vec3<f32>(0.0), vec3<f32>(1.0)) - k.xxx) * c.y);
}

fn to_work(c: vec3<f32>) -> vec3<f32> {
    if (prm.gamma != 0u) {
        return c;
    }
    return vec3<f32>(lin2srgb(c.x), lin2srgb(c.y), lin2srgb(c.z));
}

fn from_work(c: vec3<f32>) -> vec3<f32> {
    if (prm.gamma != 0u) {
        return c;
    }
    return vec3<f32>(srgb2lin(c.x), srgb2lin(c.y), srgb2lin(c.z));
}

fn repremul(o: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(from_work(o) * a, a);
}

// Bilinear 256 × 1 LUT lookup with texel centres at (i + 0.5) / 256.
fn lut_at(lut: u32, v: f32, ch: u32) -> f32 {
    let t = clamp(v * 256.0 - 0.5, 0.0, 255.0);
    let i0 = u32(floor(t));
    let i1 = min(i0 + 1u, 255u);
    return mix(adj[lut + i0][ch], adj[lut + i1][ch], t - floor(t));
}

fn lut3(lut: u32, v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(lut_at(lut, v.x, 0u), lut_at(lut, v.y, 1u), lut_at(lut, v.z, 2u));
}

fn sc_weight(px: vec3<f32>, t: vec3<f32>, reach: f32, falloff: f32) -> f32 {
    let chroma = min(px.y, t.y);
    let d = abs(px.x - t.x);
    let dh = chroma * min(d, 1.0 - d) * 2.0;
    let ds = 0.7 * (px.y - t.y);
    let dv = 0.45 * (px.z - t.z);
    let dist = sqrt(dh * dh + ds * ds + dv * dv) / sqrt(1.0 + 0.49 + 0.2025);
    let edge = reach + falloff;
    if (dist <= reach) {
        return 1.0;
    }
    if (dist >= edge) {
        return 0.0;
    }
    return 1.0 - sstep(reach, edge, dist);
}

// Selective Colour+: the weighted H/S/V shift, or the range preview.
fn selective(rgb: vec3<f32>, base: u32) -> vec3<f32> {
    let head = adj[base];
    let px = rgb2hsv(rgb);
    var w_sum = 0.0;
    var delta = vec3<f32>(0.0);
    for (var i = 0u; i < u32(head.y); i++) {
        let t = adj[base + 1u + 2u * i];
        let d = adj[base + 2u + 2u * i];
        let w = sc_weight(px, t.xyz, t.w, d.w);
        if (w > 0.0) {
            w_sum += w;
            delta += w * d.xyz;
        }
    }
    var o = rgb;
    if (w_sum > 0.0) {
        let dd = delta * select(1.0, 1.0 / w_sum, w_sum > 1.0);
        o = hsv2rgb(vec3<f32>(fract(px.x + dd.x), clamp(px.y * (1.0 + dd.y), 0.0, 1.0), clamp(px.z * (1.0 + dd.z), 0.0, 1.0)));
    }
    if (head.x > 0.5) {
        let cov = clamp(w_sum, 0.0, 1.0);
        let grey = dot(rgb, LUMA) * 0.55;
        o = mix(vec3<f32>(grey * 0.5), mix(vec3<f32>(grey), vec3<f32>(1.0, 0.15, 0.15), 0.75), cov);
    }
    return o;
}

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rnd(x: u32, y: u32, k: u32) -> f32 {
    return f32(pcg(x ^ pcg(y ^ pcg(k)))) * (1.0 / 4294967296.0);
}

// Gradient fill position along the shape (filters gradient.rs `shape_t`).
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

// Screentone's signed distance to the ink at `q` (filters colour.rs `tone`).
fn tone_dist(q: vec2<f32>, cell: f32, cs: vec2<f32>, shape: u32, level: f32) -> f32 {
    let rp = vec2<f32>(cs.x * q.x - cs.y * q.y, cs.y * q.x + cs.x * q.y);
    let d = rp - (floor(rp / cell) + 0.5) * cell;
    switch shape {
        case 1u: { return abs(d.y) - level * cell * 0.5; }
        case 2u: { return min(abs(d.x), abs(d.y)) - level * cell * 0.35; }
        default: { return length(d) - level * cell * 0.72; }
    }
}

// A pattern tile's texel, wrapped (`PatternTile::sample_repeat`).
fn pattern_at(lut: u32, i: vec2<i32>, sz: vec2<i32>) -> vec4<f32> {
    let w = ((i % sz) + sz) % sz;
    return adj[lut + u32(w.y * sz.x + w.x)];
}

// `pos`: the pixel in doc px (noise and fills depend on it, spec 46).
fn adjust(op: Op, s: vec4<f32>, pos: vec2<i32>) -> vec4<f32> {
    let base = op.b.x;
    let lut = op.b.y;
    let a = s.w;
    let w = to_work(select(s.xyz, s.xyz / a, a > 0.0));
    let p0 = adj[base];
    let p1 = adj[base + 1u];
    var o = w;
    switch op.b.z {
        case 1u: {
            let k = select(1.0 / max(1.0 - p0.y, 1e-3), 1.0 + p0.y, p0.y <= 0.0);
            o = c01((w + p0.x - 0.5) * k + 0.5);
        }
        case 2u: {
            var h = rgb2hsv(c01(w));
            h.x = fract(h.x + p0.x / 360.0);
            h.y = clamp(h.y * (1.0 + p0.y), 0.0, 1.0);
            h.z = clamp(h.z * (1.0 + p0.z), 0.0, 1.0);
            o = hsv2rgb(h);
        }
        case 3u: {
            o = w + (luma(w) - w) * p0.x;
        }
        case 4u: {
            o = c01(1.0 - w);
        }
        case 5u: {
            o = vec3<f32>(select(0.0, 1.0, luma(w) >= p0.x));
        }
        case 6u: {
            let n = max(p0.x, 2.0) - 1.0;
            o = floor(c01(w) * n + 0.5) / n;
        }
        case 7u: {
            let n = pow(c01((c01(w) - p0.x) / max(p0.y - p0.x, 1e-4)), vec3<f32>(1.0 / max(p0.z, 1e-3)));
            o = c01(p0.w + n * (p1.x - p0.w));
        }
        case 8u: {
            o = lut3(lut, c01(w));
        }
        case 9u: {
            let l = luma(w);
            o = c01(w + p0.xyz * 0.5 * (1.0 - abs(2.0 * l - 1.0)));
        }
        case 10u: {
            o = lut3(lut, vec3<f32>(luma(w)));
        }
        case 11u: {
            let t = p0.xyz;
            let up = (w - t) / max(1.0 - t, vec3<f32>(1e-4));
            let down = (t - w) / max(t, vec3<f32>(1e-4));
            let d = select(down, up, w > t);
            var alpha = clamp(max(d.x, max(d.y, d.z)), 0.0, 1.0);
            alpha = clamp((alpha - p0.w) / max(1.0 - p0.w, 1e-4), 0.0, 1.0);
            var c = w;
            if (alpha > 1e-4) {
                c = c01(t + (w - t) / alpha);
            }
            return repremul(c, a * alpha);
        }
        case 12u: {
            let rgb = c01(w);
            let t = c01(p0.xyz);
            let dist = length(rgb - t) / 1.732;
            let k = 1.0 - sstep(p0.w, p0.w + 0.12, dist);
            let rh = rgb2hsv(c01(p1.xyz));
            let recol = hsv2rgb(vec3<f32>(rh.x, rh.y, rgb2hsv(rgb).z));
            return repremul(mix(rgb, recol, k), a);
        }
        case 13u: {
            return repremul(selective(c01(w), base), a);
        }
        case 14u: {
            let q = vec2<u32>(max(pos, vec2<i32>(0)));
            var n = vec3<f32>(rnd(q.x, q.y, 51u) - 0.5);
            if (p0.y < 0.5) {
                n = vec3<f32>(n.x, rnd(q.x, q.y, 52u) - 0.5, rnd(q.x, q.y, 53u) - 0.5);
            }
            o = c01(w + n * p0.x);
        }
        case 15u: {
            return vec4<f32>(p0.xyz, 1.0);
        }
        case 17u: {
            // Screentone: p0 = density, level, level end, shape; p1 = gradient
            // direction, its extent, cell; p2 = angle cos, sin, hard dots; p3 = ink.
            let p2 = adj[base + 2u];
            let px = vec2<f32>(pos) + 0.5;
            var level = p0.y;
            if (p0.x == 1.0) {
                level = (1.0 - clamp(dot(c01(w), LUMA), 0.0, 1.0)) * a;
            } else if (p0.x == 2.0) {
                level = mix(p0.y, p0.z, clamp(dot(px, p1.xy) / p1.z, 0.0, 1.0));
            }
            let shape = u32(p0.w);
            let dist = tone_dist(px, p1.w, p2.xy, shape, level);
            var cov: f32;
            if (p2.z > 0.5) {
                cov = select(0.0, 1.0, dist <= 0.0);
            } else {
                let fw = abs(tone_dist(px + vec2<f32>(1.0, 0.0), p1.w, p2.xy, shape, level) - dist)
                    + abs(tone_dist(px + vec2<f32>(0.0, 1.0), p1.w, p2.xy, shape, level) - dist);
                let k = max(fw, 1e-5);
                cov = 1.0 - sstep(-k, k, dist);
            }
            if (level < 0.0005) {
                cov = 0.0;
            }
            return vec4<f32>(adj[base + 3u].xyz * cov, cov);
        }
        case 18u: {
            // Pattern fill: p0 = tile size, scale; p1 = offset (doc px), angle cos, sin.
            let d = vec2<f32>(pos) + 0.5 - p1.xy;
            let r = vec2<f32>(p1.z * d.x - p1.w * d.y, p1.w * d.x + p1.z * d.y);
            let sz = vec2<i32>(p0.xy);
            let q = r / (p0.xy * p0.z) * p0.xy - 0.5;
            let i = vec2<i32>(floor(q));
            let f = q - floor(q);
            let t = mix(
                mix(pattern_at(lut, i, sz), pattern_at(lut, i + vec2<i32>(1, 0), sz), f.x),
                mix(pattern_at(lut, i + vec2<i32>(0, 1), sz), pattern_at(lut, i + vec2<i32>(1, 1), sz), f.x),
                f.y,
            );
            return vec4<f32>(from_work(t.xyz) * t.w, t.w);
        }
        case 16u: {
            let t = shape_t(u32(p0.x), vec2<f32>(pos) + 0.5 - p0.yz, vec2<f32>(p0.w, p1.x) - p0.yz);
            let x = clamp(t, 0.0, 1.0) * 1023.0;
            let i0 = u32(floor(x));
            let c = mix(adj[lut + i0], adj[lut + min(i0 + 1u, 1023u)], x - floor(x));
            return vec4<f32>(c.xyz * c.w, c.w);
        }
        default: {}
    }
    return repremul(o, a);
}

// ── Paper tooth (spec 43; crates/brush paper.rs `shade_at`) ──

fn paper_at(i: vec2<i32>, n: i32) -> f32 {
    return textureLoad(paper_tex, vec2<i32>(i.x % n, i.y % n), 0).x;
}

// `TipMask::sample_repeat` at doc position `d`.
fn paper_h(d: vec2<f32>, tile_px: f32) -> f32 {
    let n = i32(textureDimensions(paper_tex).x);
    let nf = f32(n);
    let t = d / tile_px * nf - 0.5;
    let w = t - nf * floor(t / nf);
    let i = vec2<i32>(w);
    let f = w - floor(w);
    let top = mix(paper_at(i, n), paper_at(i + vec2<i32>(1, 0), n), f.x);
    let bottom = mix(paper_at(i + vec2<i32>(0, 1), n), paper_at(i + vec2<i32>(1, 1), n), f.x);
    return mix(top, bottom, f.y);
}

fn paper_k(d: vec2<f32>, tile_px: f32, amt: f32) -> f32 {
    var l = vec3<f32>(-0.45, -0.6, 0.66);
    l /= sqrt(dot(l, l));
    let nx = (paper_h(d - vec2<f32>(1.0, 0.0), tile_px) - paper_h(d + vec2<f32>(1.0, 0.0), tile_px)) * 2.0;
    let ny = (paper_h(d - vec2<f32>(0.0, 1.0), tile_px) - paper_h(d + vec2<f32>(0.0, 1.0), tile_px)) * 2.0;
    let lambert = clamp((nx * l.x + ny * l.y + l.z) / sqrt(nx * nx + ny * ny + 1.0), 0.0, 1.0);
    let h = paper_h(d, tile_px);
    let shade = 1.0 + (0.55 + 0.45 * (0.35 + 0.65 * lambert) * (0.4 + 0.6 * h) - 1.0) * amt;
    // 8-bit documents use the shade as it is (`u_paperLinear`, spec 50).
    return select(pow(shade, 2.2), shade, prm.gamma != 0u);
}

// ── Impasto relief (spec 45; crates/compose relief.rs `light_px`) ──

// The job's tile origin and this pixel, in document pixels at the level.
var<private> tile_origin: vec2<i32>;

// Height at document pixel `q` (clamped to the document), from the tile's
// own height slot or the one across the edge it lies past.
fn relief_h(o: Op, q: vec2<i32>) -> f32 {
    let e = i32(256u >> prm.level);
    let c = clamp(q, vec2<i32>(0), vec2<i32>(prm.doc_size) - vec2<i32>(1)) - tile_origin;
    var slot = o.d.x & 0xffffu;
    var local = c;
    if (c.x < 0) {
        slot = o.d.x >> 16u;
        local.x += e;
    } else if (c.x >= e) {
        slot = o.d.y & 0xffffu;
        local.x -= e;
    } else if (c.y < 0) {
        slot = o.d.y >> 16u;
        local.y += e;
    } else if (c.y >= e) {
        slot = o.d.z & 0xffffu;
        local.y -= e;
    }
    if (slot == 0xffffu) {
        return 0.0;
    }
    return load(slot, local).x;
}

fn relief(o: Op, px: vec4<f32>, p: vec2<i32>) -> vec4<f32> {
    let g = tile_origin + p;
    if (px.w <= 0.0 || any(g >= vec2<i32>(prm.doc_size))) {
        return px;
    }
    let hl = relief_h(o, g - vec2<i32>(1, 0));
    let hr = relief_h(o, g + vec2<i32>(1, 0));
    let hu = relief_h(o, g - vec2<i32>(0, 1));
    let hd = relief_h(o, g + vec2<i32>(0, 1));
    let amt = sstep(0.02, 0.25, max(max(hl, hr), max(hu, hd)));
    if (amt <= 0.0) {
        return px;
    }
    var l = vec3<f32>(-0.45, -0.6, 0.66);
    l /= sqrt(dot(l, l));
    let nx = (hl - hr) * 9.0;
    let ny = (hu - hd) * 9.0;
    let len = sqrt(nx * nx + ny * ny + 1.0);
    let n = vec3<f32>(nx, ny, 1.0) / len;
    let ndl = dot(n, l);
    let spec = pow(clamp(-l.z + 2.0 * ndl * n.z, 0.0, 1.0), 22.0);
    let k = 1.0 + (0.62 + 0.62 * clamp(ndl, 0.0, 1.0) - 1.0) * amt;
    return vec4<f32>(px.xyz * k + spec * 0.3 * amt * px.w, px.w);
}

// `layer_content`: pixels, then the live stroke, then relief, then the mask.
fn content(o: Op, p: vec2<i32>) -> vec4<f32> {
    let flags = o.a.w;
    var c = vec4<f32>(0.0);
    if ((flags & F_BASE) != 0u) {
        c = load(o.b.x, p);
    }
    if ((flags & F_STROKE) != 0u) {
        c = apply_stroke(c, load(o.b.y, p), bitcast<f32>(o.b.w), o.b.z);
    }
    if ((flags & F_RELIEF) != 0u) {
        c = relief(o, c, p);
    }
    if ((flags & F_MASK) != 0u) {
        var m = vec4<f32>(0.0);
        if ((flags & F_MASK_ZERO) == 0u) {
            m = load(o.c.x, p);
        }
        if ((flags & F_MASK_STROKE) != 0u) {
            m = apply_stroke(m, load(o.c.y, p), bitcast<f32>(o.c.w), o.c.z);
        }
        c *= m.x;
    }
    return c;
}

fn finish(job: Job, g: vec2<u32>, p: vec2<i32>, top: vec4<f32>) {
    if (job.dest != NONE) {
        textureStore(cache_out, p, i32(job.dest), top);
    } else {
        let s = g / prm.sheet;
        textureStore(out_tex, vec2<i32>(g - s * prm.sheet), i32(s.y * prm.cols + s.x), top);
    }
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let job = jobs[gid.z + select(0u, prm.general_base, GENERAL)];
    if (gid.x >= (256u >> prm.level) || gid.y >= (256u >> prm.level)) {
        return;
    }
    let g = job.origin + gid.xy;
    tile_origin = vec2<i32>(job.origin);
    // A cache image covers its whole tile: it may be reused by any tile.
    if (job.dest == NONE && (g.x >= prm.doc_size.x || g.y >= prm.doc_size.y)) {
        return;
    }
    let p = vec2<i32>(gid.xy);
    doc_px = (tile_origin + p) * i32(1u << prm.level);
    // The top of the stack lives in `top`; `stack` holds the registers under it.
    var stack: array<vec4<f32>, STACK>;
    var top = vec4<f32>(0.0);
    // The Background top-level knockouts reveal (OP_SNAPSHOT).
    var background = vec4<f32>(0.0);
    if (job.init != NONE) {
        top = load_cache(job.init, p);
    }
    var sp = 0u;
    if (!GENERAL) {
        for (var i = job.op_start; i < job.op_start + job.op_count; i++) {
            let o = ops[i];
            var c: vec4<f32>;
            if (o.a.x == OP_OVER_CACHE) {
                c = load_cache(o.a.y, p);
            } else {
                c = load(o.b.x, p) * bitcast<f32>(o.a.z);
            }
            top = c + top * (1.0 - c.w);
        }
        finish(job, g, p, top);
        return;
    }
    for (var i = job.op_start; i < job.op_start + job.op_count; i++) {
        let o = ops[i];
        let opacity = bitcast<f32>(o.a.z);
        switch o.a.x {
            case OP_LAYER: {
                var c = content(o, p);
                if ((o.a.w & F_INHERIT) != 0u) {
                    c *= top.w;
                }
                top = blend_px(o.a.y, top, c, opacity);
            }
            case OP_PUSH_ZERO: {
                stack[sp] = top;
                sp += 1u;
                top = vec4<f32>(0.0);
            }
            case OP_PUSH_CONTENT: {
                stack[sp] = top;
                sp += 1u;
                top = content(o, p);
            }
            case OP_CLIP: {
                top = clip_over(top, content(o, p), opacity);
            }
            case OP_PUSH_COPY: {
                stack[sp] = top;
                sp += 1u;
            }
            case OP_MIX: {
                // A Pass Through group fades with its backdrop.
                let v = top;
                sp -= 1u;
                top = stack[sp] * (1.0 - opacity) + v * opacity;
            }
            case OP_KNOCKOUT: {
                // The hole shows transparency, the register below (a Pass
                // Through backdrop) or the Background (program.rs `Reveal`).
                let k = content(o, p).w * opacity;
                var bg = vec4<f32>(0.0);
                if (o.a.y == 1u && sp > 0u) {
                    bg = stack[sp - 1u];
                } else if (o.a.y == 2u) {
                    bg = background;
                }
                top = mix(top, bg, k);
            }
            case OP_SNAPSHOT: {
                background = top;
            }
            case OP_OVER_CACHE: {
                let c = load_cache(o.a.y, p);
                top = c + top * (1.0 - c.w);
            }
            case OP_ADJUST: {
                var v = adjust(o, top, (tile_origin + p) * i32(1u << prm.level));
                if ((o.a.w & F_MASK) != 0u) {
                    v *= load(o.c.x, p).x;
                }
                top = blend_px(o.a.y, top, v, opacity);
            }
            case OP_COVERAGE: {
                // A panel frame's partial clip (spec 44), coverage in the slot's channels.
                top *= load(o.c.x, p).x;
            }
            case OP_PAPER: {
                if (any(top.xyz != vec3<f32>(0.0))) {
                    // The level's pixel centre in doc px (an approximation above level 0).
                    let d = (vec2<f32>(g) + 0.5) * f32(1u << prm.level);
                    top = vec4<f32>(top.xyz * paper_k(d, bitcast<f32>(o.b.x), bitcast<f32>(o.b.y)), top.w);
                }
            }
            default: {
                var v = top;
                sp -= 1u;
                top = stack[sp];
                if ((o.a.w & F_TINT) != 0u) {
                    let t = vec3<f32>(bitcast<f32>(o.d.x), bitcast<f32>(o.d.y), bitcast<f32>(o.d.z));
                    v = vec4<f32>(v.xyz + (t * v.w - v.xyz) * 0.65, v.w);
                }
                if ((o.a.w & F_INHERIT) != 0u) {
                    top = clip_over(top, v, opacity);
                } else {
                    top = blend_px(o.a.y, top, v, opacity);
                }
            }
        }
    }
    finish(job, g, p, top);
}
