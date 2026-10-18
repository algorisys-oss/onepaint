// Whole-canvas filter layers (spec 53): one pass over the document at the
// current level, reading the backdrop image (the backdrop tiles copied
// from their segment cache slots) and writing the filtered
// image the composite reads. Ports filters `distort.rs` and `stylize.rs`
// with `tex.rs`'s sampling: bilinear by hand, clamp-to-edge.

struct Prm {
    // The image in level px, the tile edge and the grid's columns.
    size: vec2<i32>,
    edge: i32,
    cols: i32,
    level: i32,
    kind: u32,
    gamma: u32,
    _p0: u32,
    // The document's size in level px (fractional above level 0).
    doc: vec2<f32>,
    _p1: vec2<f32>,
    k0: vec4<f32>,
    k1: vec4<f32>,
}

@group(0) @binding(0) var<uniform> prm: Prm;
@group(0) @binding(2) var back: texture_2d<f32>;
@group(0) @binding(3) var dst: texture_storage_2d<rgba16float, write>;
// The Blur Gallery's half-resolution images: a pass's input and output, and
// the three blur levels the final pass blends.
@group(0) @binding(4) var g_in: texture_2d<f32>;
@group(0) @binding(5) var g_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(6) var g_l1: texture_2d<f32>;
@group(0) @binding(7) var g_l2: texture_2d<f32>;
@group(0) @binding(8) var g_l3: texture_2d<f32>;
@group(0) @binding(9) var<uniform> gal: Gallery;

// A Blur Gallery pass: `gal.kind` is 0 for tilt-shift, 1 iris, 2 field.
struct Gallery {
    // The half-resolution size, and the blur pass's direction and radius.
    half: vec2<i32>,
    kind: u32,
    pins: u32,
    dir: vec2<f32>,
    radius: f32,
    r_max: f32,
    // Source texels per output px for the blur pass.
    per: vec2<f32>,
    bokeh: f32,
    range: f32,
    // Centre (doc px), the band's normal and the ellipse's axis.
    c: vec2<f32>,
    focus: f32,
    falloff: f32,
    u: vec2<f32>,
    n: vec2<f32>,
    rx: f32,
    ry: f32,
    inner: f32,
    _p: f32,
    // Field blur pins: x, y (doc px), blur.
    pin: array<vec4<f32>, 8>,
}

const NONE: u32 = 0xffffffffu;
const PI: f32 = 3.14159265358979;
const TAU: f32 = 6.28318530717959;

// `Tex::get`: one backdrop texel.
fn texel_at(x: i32, y: i32) -> vec4<f32> {
    return textureLoad(back, vec2<i32>(x, y), 0);
}

// `Tex::texel`: bilinear at texel coordinates (centres at integers), clamped.
fn texel(xi: f32, yi: f32) -> vec4<f32> {
    let x = clamp(xi, 0.0, f32(prm.size.x - 1));
    let y = clamp(yi, 0.0, f32(prm.size.y - 1));
    let x0 = i32(floor(x));
    let y0 = i32(floor(y));
    let x1 = min(x0 + 1, prm.size.x - 1);
    let y1 = min(y0 + 1, prm.size.y - 1);
    let fx = x - f32(x0);
    let fy = y - f32(y0);
    let top = texel_at(x0, y0) + (texel_at(x1, y0) - texel_at(x0, y0)) * fx;
    let bot = texel_at(x0, y1) + (texel_at(x1, y1) - texel_at(x0, y1)) * fx;
    return top + (bot - top) * fy;
}

// `Src::px` and `Src::uv`.
fn px(q: vec2<f32>) -> vec4<f32> {
    return texel(q.x - 0.5, q.y - 0.5);
}

fn uvs(uv: vec2<f32>) -> vec4<f32> {
    return px(uv * prm.doc);
}

fn lin2srgb(x: f32) -> f32 {
    let c = max(x, 0.0);
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

fn srgb2lin(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

fn unpremul(s: vec4<f32>) -> vec3<f32> {
    return select(s.xyz, s.xyz / s.w, s.w > 0.0);
}

// `straight_work`: straight values in the work space (sRGB).
fn work(t: vec4<f32>) -> vec3<f32> {
    let c = unpremul(t);
    if (prm.gamma != 0u) {
        return c;
    }
    return vec3<f32>(lin2srgb(c.x), lin2srgb(c.y), lin2srgb(c.z));
}

fn repremul(o: vec3<f32>, a: f32) -> vec4<f32> {
    var l = o;
    if (prm.gamma == 0u) {
        l = vec3<f32>(srgb2lin(o.x), srgb2lin(o.y), srgb2lin(o.z));
    }
    return vec4<f32>(l * a, a);
}

// Twirl and pinch: the centre (doc px) and the radius they reach.
fn centre() -> vec2<f32> {
    return prm.k0.xy * prm.doc;
}

fn reach() -> f32 {
    return max(prm.k0.w * 0.5 * min(prm.doc.x, prm.doc.y), 1.0);
}

// `ign`: interleaved gradient noise, the per-pixel tap jitter.
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.982918 * fract(p.x * 0.06711056 + p.y * 0.00583715));
}

fn smoothstep_cpu(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// RADIAL_BLUR_SRC: spin around the centre or zoom toward it, 96 jittered taps.
fn radial(q: vec2<f32>) -> vec4<f32> {
    let c = centre();
    let d = q - c;
    let jit = ign(q);
    let amount = prm.k0.z;
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < 96; k++) {
        let t = (f32(k) + jit) / 96.0;
        var pt: vec2<f32>;
        if (prm.k0.w != 0.0) {
            pt = c + d * (1.0 - t * amount * 0.5);
        } else {
            let a = (t - 0.5) * amount * (PI / 3.0);
            let co = cos(a);
            let sn = sin(a);
            pt = vec2<f32>(c.x + co * d.x - sn * d.y, c.y + sn * d.x + co * d.y);
        }
        sum += px(pt);
    }
    return sum / 96.0;
}

// SPIN_SRC: rotate inside the ellipse's own normalised space, 64 taps.
fn spin(q: vec2<f32>, src: vec4<f32>) -> vec4<f32> {
    let c = centre();
    let a = prm.k0.z;
    let rx = max(prm.k0.w * min(prm.doc.x, prm.doc.y) * 0.5, 1.0);
    let ry = max(rx * prm.k1.x, 1.0);
    let u = vec2<f32>(cos(a), -sin(a));
    let n = vec2<f32>(sin(a), cos(a));
    let d = q - c;
    let e = vec2<f32>(dot(d, u) / rx, dot(d, n) / ry);
    let w = 1.0 - smoothstep_cpu(prm.k1.y, 1.0, length(e));
    if (w <= 0.0) {
        return src;
    }
    let arc = prm.k1.z * w;
    let jit = ign(q);
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < 64; k++) {
        let ang = ((f32(k) + jit) / 64.0 - 0.5) * arc;
        let co = cos(ang);
        let sn = sin(ang);
        let r = vec2<f32>(co * e.x - sn * e.y, sn * e.x + co * e.y);
        sum += px(c + u * r.x * rx + n * r.y * ry);
    }
    return sum / 64.0;
}

@compute @workgroup_size(8, 8, 1)
fn whole_filter(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= prm.size)) {
        return;
    }
    let q = vec2<f32>(l) + 0.5;
    var o = texel_at(l.x, l.y);
    switch prm.kind {
        // TWIRL_SRC: rotate by θ·(1 − r/R)² inside the circle.
        case 1u: {
            let c = centre();
            let rad = reach();
            let d = q - c;
            let r = length(d);
            if (r < rad) {
                let f = 1.0 - r / rad;
                let a = prm.k0.z * f * f;
                let co = cos(a);
                let sn = sin(a);
                o = px(vec2<f32>(c.x + co * d.x - sn * d.y, c.y + sn * d.x + co * d.y));
            }
        }
        // PINCH_SRC: read at c + d·sin(π/2·r/R)^−amount.
        case 2u: {
            let c = centre();
            let rad = reach();
            let d = q - c;
            let r = length(d);
            if (r < rad && r > 0.0) {
                let k = pow(sin(PI * 0.5 * r / rad), -prm.k0.z);
                o = px(c + d * k);
            }
        }
        // POLAR_SRC: rectangular to polar, or back.
        case 3u: {
            let c = centre();
            let hs = prm.doc * 0.5;
            let v = q / prm.doc;
            var uv: vec2<f32>;
            if (prm.k0.z == 0.0) {
                let p = (q - c) / hs;
                let a = atan2(p.x, -p.y);
                uv = vec2<f32>(fract(a / TAU + 1.0), length(p));
            } else {
                let a = v.x * TAU;
                uv = vec2<f32>((c.x + sin(a) * v.y * hs.x) / prm.doc.x, (c.y - cos(a) * v.y * hs.y) / prm.doc.y);
            }
            o = uvs(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
        }
        // CHROMATIC_SRC: red and blue read along the ray from the centre.
        case 4u: {
            let uv = q / prm.doc;
            let sh = (uv - 0.5) * prm.k0.x * 0.04;
            let sr = uvs(uv + sh);
            let sg = o;
            let sb = uvs(uv - sh);
            o = repremul(vec3<f32>(work(sr).x, work(sg).y, work(sb).z), sg.w);
        }
        case 5u: {
            o = radial(q);
        }
        case 6u: {
            o = spin(q, o);
        }
        default: {}
    }
    textureStore(dst, l, o);
}

// ── The Blur Gallery (filters blur.rs `gallery`) ──

const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

// `Tex::texel` over a half-resolution image.
fn g_texel(t: texture_2d<f32>, xi: f32, yi: f32) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(t));
    let x = clamp(xi, 0.0, f32(d.x - 1));
    let y = clamp(yi, 0.0, f32(d.y - 1));
    let x0 = i32(floor(x));
    let y0 = i32(floor(y));
    let x1 = min(x0 + 1, d.x - 1);
    let y1 = min(y0 + 1, d.y - 1);
    let fx = x - f32(x0);
    let fy = y - f32(y0);
    let a = textureLoad(t, vec2<i32>(x0, y0), 0);
    let b = textureLoad(t, vec2<i32>(x1, y0), 0);
    let c = textureLoad(t, vec2<i32>(x0, y1), 0);
    let e = textureLoad(t, vec2<i32>(x1, y1), 0);
    let top = a + (b - a) * fx;
    let bot = c + (e - c) * fx;
    return top + (bot - top) * fy;
}

// `Tex::uv` over a half-resolution image.
fn g_uv(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    let d = vec2<f32>(textureDimensions(t));
    return g_texel(t, uv.x * d.x - 0.5, uv.y * d.y - 0.5);
}

// `blur_to`: 65 taps at k·R/32 along `dir`, σ = 16 taps. `from_src` reads
// the backdrop (the document at this level), else `g_in`.
fn blur_at(l: vec2<i32>, from_src: bool) -> vec4<f32> {
    var sw: vec2<f32>;
    if (from_src) {
        sw = vec2<f32>(prm.size);
    } else {
        sw = vec2<f32>(textureDimensions(g_in));
    }
    let o = vec2<f32>(gal.half);
    let b = (vec2<f32>(l) + 0.5) / o * sw - 0.5;
    let spread = max(gal.radius, 1.0) / 32.0;
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = -32; k <= 32; k++) {
        let wt = exp(-0.5 * f32(k * k) / 256.0);
        let off = f32(k) * spread;
        let q = b + gal.dir * off * gal.per;
        var s: vec4<f32>;
        if (from_src) {
            s = texel(q.x, q.y);
        } else {
            s = g_texel(g_in, q.x, q.y);
        }
        acc += s * wt;
        wsum += wt;
    }
    return acc / max(wsum, 1e-4);
}

@compute @workgroup_size(8, 8, 1)
fn gallery_blur_src(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= gal.half)) {
        return;
    }
    textureStore(g_out, l, blur_at(l, true));
}

@compute @workgroup_size(8, 8, 1)
fn gallery_blur(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= gal.half)) {
        return;
    }
    textureStore(g_out, l, blur_at(l, false));
}

// BOKEH_SRC: a 64-tap golden-angle disc weighted toward highlights.
@compute @workgroup_size(8, 8, 1)
fn gallery_bokeh(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= gal.half)) {
        return;
    }
    let size = vec2<f32>(prm.size);
    let o = vec2<f32>(gal.half);
    let uv = (vec2<f32>(l) + 0.5) / o;
    let p = uv * size;
    let rot = ign(vec2<f32>(l) + 0.5) * TAU;
    let radius = max(gal.radius, 1.0);
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = 0; k < 64; k++) {
        let r = sqrt((f32(k) + 0.5) / 64.0) * radius;
        let a = f32(k) * 2.3999632 + rot;
        let q = p + vec2<f32>(cos(a), sin(a)) * r;
        let c = g_uv(g_in, q / size);
        let lum = dot(unpremul(c), LUMA);
        let w = 1.0 + gal.bokeh * 12.0 * smoothstep_cpu(gal.range, 1.0, lum);
        sum += c * w;
        wsum += w;
    }
    textureStore(g_out, l, sum / wsum);
}

// The blur amount at a doc-px position: the band, the ellipse, or the pins.
fn gallery_amount(px: vec2<f32>) -> f32 {
    let d = px - gal.c;
    if (gal.kind == 0u) {
        return smoothstep_cpu(gal.focus, gal.focus + max(gal.falloff, 0.5), abs(dot(d, gal.n)));
    }
    if (gal.kind == 1u) {
        let e = length(vec2<f32>(dot(d, gal.u) / gal.rx, dot(d, gal.n) / gal.ry));
        return smoothstep_cpu(gal.inner, 1.0, e);
    }
    var num = 0.0;
    var den = 0.0;
    for (var i = 0u; i < gal.pins; i++) {
        let q = gal.pin[i];
        let dq = q.xy - px;
        let wt = 1.0 / (dot(dq, dq) + 1.0);
        num += wt * q.z;
        den += wt;
    }
    if (den > 0.0) {
        return num / den / max(gal.r_max, 1e-4);
    }
    return 0.0;
}

@compute @workgroup_size(8, 8, 1)
fn gallery_final(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= prm.size)) {
        return;
    }
    let src = texel_at(l.x, l.y);
    let q = vec2<f32>(l) + 0.5;
    let r = clamp(gallery_amount(q), 0.0, 1.0);
    let uv = q / vec2<f32>(prm.size);
    let a1 = g_uv(g_l1, uv);
    let a2 = g_uv(g_l2, uv);
    let a3 = g_uv(g_l3, uv);
    var o: vec4<f32>;
    if (r < 0.25) {
        o = src + (a1 - src) * (r * 4.0);
    } else if (r < 0.5) {
        o = a1 + (a2 - a1) * ((r - 0.25) * 4.0);
    } else {
        o = a2 + (a3 - a2) * ((r - 0.5) * 2.0);
    }
    textureStore(dst, l, o);
}
