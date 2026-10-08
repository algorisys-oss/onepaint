// GPU compositor: one invocation per pixel interprets its tile's program
// (crates/compose/src/program.rs) with a register stack. The blend maths are
// crates/compose/src/blend.rs line for line; the CPU compositor is the
// oracle in the tests. Pixels are premultiplied linear light.
//
// Op layout (four vec4<u32>):
//   a: kind, blend mode, opacity (f32 bits), flags
//   b: base slot, stroke slot, stroke code, stroke opacity
//   c: mask slot, mask stroke slot, mask stroke code, mask stroke opacity
//   d: ghost tint rgb (f32 bits)
// A stroke code is `kind | mode << 8` (kind 0 paint, 1 erase, 2 alpha lock).

struct Params {
    doc_size: vec2<u32>,
    per_atlas: u32,
    _pad: u32,
}

struct Job {
    origin: vec2<u32>,
    op_start: u32,
    op_count: u32,
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
@group(0) @binding(4) var out_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var atlas0: texture_2d_array<f32>;
@group(0) @binding(6) var atlas1: texture_2d_array<f32>;
@group(0) @binding(7) var atlas2: texture_2d_array<f32>;
@group(0) @binding(8) var atlas3: texture_2d_array<f32>;
@group(0) @binding(9) var atlas4: texture_2d_array<f32>;
@group(0) @binding(10) var atlas5: texture_2d_array<f32>;
@group(0) @binding(11) var atlas6: texture_2d_array<f32>;
@group(0) @binding(12) var atlas7: texture_2d_array<f32>;

const OP_LAYER: u32 = 0u;
const OP_PUSH_ZERO: u32 = 1u;
const OP_PUSH_CONTENT: u32 = 2u;
const OP_CLIP: u32 = 3u;
const OP_POP: u32 = 4u;

const F_INHERIT: u32 = 1u;     // Layer: inherit alpha; Pop: clip instead of blend
const F_TINT: u32 = 2u;
const F_BASE: u32 = 4u;
const F_STROKE: u32 = 8u;
const F_MASK: u32 = 16u;
const F_MASK_ZERO: u32 = 32u;
const F_MASK_STROKE: u32 = 64u;

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

const STACK: u32 = 8u;

// Off unless the batch has a Pigment op: the Kubelka–Munk code is large, and
// leaving it out keeps every other program fast.
override PIGMENT: bool = true;

fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn load(slot: u32, p: vec2<i32>) -> vec4<f32> {
    let l = i32(slot % prm.per_atlas);
    switch slot / prm.per_atlas {
        case 0u: { return textureLoad(atlas0, p, l, 0); }
        case 1u: { return textureLoad(atlas1, p, l, 0); }
        case 2u: { return textureLoad(atlas2, p, l, 0); }
        case 3u: { return textureLoad(atlas3, p, l, 0); }
        case 4u: { return textureLoad(atlas4, p, l, 0); }
        case 5u: { return textureLoad(atlas5, p, l, 0); }
        case 6u: { return textureLoad(atlas6, p, l, 0); }
        default: { return textureLoad(atlas7, p, l, 0); }
    }
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
        default: { return vec3<f32>(bc(m, b.x, s.x), bc(m, b.y, s.y), bc(m, b.z, s.z)); }
    }
}

fn blend_px(m: u32, bp: vec4<f32>, src: vec4<f32>, opacity: f32) -> vec4<f32> {
    let sp = src * opacity;
    if (sp.w <= 0.0 && sp.x == 0.0 && sp.y == 0.0 && sp.z == 0.0) {
        return bp;
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

fn blend_px_alpha_lock(m: u32, bp: vec4<f32>, src: vec4<f32>, opacity: f32) -> vec4<f32> {
    let sp = src * opacity;
    if (sp.w <= 0.0) {
        return bp;
    }
    if (m == M_NORMAL) {
        return vec4<f32>(sp.xyz * bp.w + bp.xyz * (1.0 - sp.w), bp.w);
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

// `layer_content`: pixels, then the live stroke, then the mask.
fn content(o: Op, p: vec2<i32>) -> vec4<f32> {
    let flags = o.a.w;
    var c = vec4<f32>(0.0);
    if ((flags & F_BASE) != 0u) {
        c = load(o.b.x, p);
    }
    if ((flags & F_STROKE) != 0u) {
        c = apply_stroke(c, load(o.b.y, p), bitcast<f32>(o.b.w), o.b.z);
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

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let job = jobs[gid.z];
    let g = job.origin + gid.xy;
    if (g.x >= prm.doc_size.x || g.y >= prm.doc_size.y) {
        return;
    }
    let p = vec2<i32>(gid.xy);
    // The top of the stack lives in `top`; `stack` holds the registers under it.
    var stack: array<vec4<f32>, STACK>;
    var top = vec4<f32>(0.0);
    var sp = 0u;
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
    textureStore(out_tex, vec2<i32>(g), top);
}
