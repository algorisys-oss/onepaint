// Whole-canvas filter layers (spec 53): one pass over the document at the
// current level, reading the backdrop tiles from their segment cache slots
// (`grid`, row by row over the document's tiles) and writing the filtered
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
@group(0) @binding(1) var<storage, read> grid: array<u32>;
@group(0) @binding(2) var cache: texture_2d_array<f32>;
@group(0) @binding(3) var dst: texture_storage_2d<rgba16float, write>;

const NONE: u32 = 0xffffffffu;
const PI: f32 = 3.14159265358979;
const TAU: f32 = 6.28318530717959;

// `Tex::get`: one backdrop texel (0 where a tile has no backdrop).
fn texel_at(x: i32, y: i32) -> vec4<f32> {
    let slot = grid[(y / prm.edge) * prm.cols + x / prm.edge];
    if (slot == NONE) {
        return vec4<f32>(0.0);
    }
    return textureLoad(cache, vec2<i32>(x % prm.edge, y % prm.edge), i32(slot), prm.level);
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
        default: {}
    }
    textureStore(dst, l, o);
}
