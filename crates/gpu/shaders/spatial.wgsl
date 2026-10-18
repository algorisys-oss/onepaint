// Spatial adjustment layers (spec 52): a filter over the backdrop around one
// tile. The backdrop tiles are segment cache slots; `assemble` copies the
// region (tile ± reach, inside the document) into a scratch image, the blur
// passes run happypaint's 65-tap kernel exactly as filters `blur_pass` does
// (bilinear taps, clamped to the region), and `blur_out` writes the centre
// tile to the output cache slot. Coordinates are in level pixels.

struct Prm {
    // The region's origin and size.
    region: vec4<i32>,
    // The output tile's origin.
    tile: vec2<i32>,
    // The tile edge at this level; the cache level.
    edge: i32,
    level: i32,
    // The backdrop grid: first tile column and row, columns.
    grid: vec3<i32>,
    // 0 blur, 1 sharpen (combine with `orig`), 2 copy, 3 a per-pixel kind,
    // 4 bloom (screen the blurred glow over `orig`), 5 XDoG line art.
    mode: u32,
    dir: vec2<f32>,
    radius: f32,
    amount: f32,
    out_slot: u32,
    // The per-pixel kind (`kind_px`): 1 emboss, 2 mosaic, 3 ripple,
    // 4 halftone, 5 crystallize, 7 line art (Sobel).
    kind: u32,
    // 1 when values are sRGB (8-bit documents, spec 50).
    gamma: u32,
    _p0: u32,
    // The document's size in doc px; doc px per level px.
    doc: vec2<f32>,
    scale: f32,
    _p1: f32,
    // The kind's params (spatial.rs `kind_params`).
    k0: vec4<f32>,
    k1: vec4<f32>,
}

const NONE: u32 = 0xffffffffu;
const HALF: i32 = 32;

@group(0) @binding(0) var<uniform> prm: Prm;
@group(0) @binding(1) var<storage, read> slots: array<u32>;
@group(0) @binding(2) var cache_tex: texture_2d_array<f32>;
@group(0) @binding(3) var src: texture_2d<f32>;
@group(0) @binding(4) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(5) var out: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(6) var orig: texture_2d<f32>;

@compute @workgroup_size(8, 8, 1)
fn assemble(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= prm.region.zw)) {
        return;
    }
    let g = prm.region.xy + l;
    let t = g / prm.edge - prm.grid.xy;
    let slot = slots[t.y * prm.grid.z + t.x];
    var v = vec4<f32>(0.0);
    if (slot != NONE) {
        v = textureLoad(cache_tex, g - (t + prm.grid.xy) * prm.edge, i32(slot), prm.level);
    }
    textureStore(dst, l, v);
}

// filters `sample`: bilinear, clamped to the region.
fn sample_in(t: texture_2d<f32>, x0: f32, y0: f32) -> vec4<f32> {
    let w = prm.region.z;
    let h = prm.region.w;
    let x = clamp(x0, 0.0, f32(w - 1));
    let y = clamp(y0, 0.0, f32(h - 1));
    let xi = i32(floor(x));
    let yi = i32(floor(y));
    let x1 = min(xi + 1, w - 1);
    let y1 = min(yi + 1, h - 1);
    let fx = x - f32(xi);
    let fy = y - f32(yi);
    let a = textureLoad(t, vec2<i32>(xi, yi), 0);
    let b = textureLoad(t, vec2<i32>(x1, yi), 0);
    let c = textureLoad(t, vec2<i32>(xi, y1), 0);
    let d = textureLoad(t, vec2<i32>(x1, y1), 0);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    return top + (bot - top) * fy;
}

fn sample(x0: f32, y0: f32) -> vec4<f32> {
    return sample_in(src, x0, y0);
}

// filters `blur_pass` at one pixel of `t`.
fn blur_in(t: texture_2d<f32>, x: f32, y: f32, radius: f32, dir: vec2<f32>) -> vec4<f32> {
    let spread = max(radius, 1.0) / f32(HALF);
    let sigma = f32(HALF) * 0.5;
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = 0; k <= 2 * HALF; k++) {
        let i = f32(k - HALF);
        let wt = exp(-0.5 * i * i / (sigma * sigma));
        let o = i * spread;
        acc += sample_in(t, x + dir.x * o, y + dir.y * o) * wt;
        wsum += wt;
    }
    return acc / max(wsum, 1e-4);
}

fn blur(x: f32, y: f32) -> vec4<f32> {
    return blur_in(src, x, y, prm.radius, prm.dir);
}


// Line art's luminance map over the region (XDoG's source).
@compute @workgroup_size(8, 8, 1)
fn lum(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= prm.region.zw)) {
        return;
    }
    let v = line_lum(textureLoad(src, l, 0));
    textureStore(dst, l, vec4<f32>(v, v, v, 1.0));
}

// BLOOM_BRIGHT_SRC over the region: the straight light map of what clears
// the threshold (k0.x), read as stored (no `to_work`).
@compute @workgroup_size(8, 8, 1)
fn bright(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= prm.region.zw)) {
        return;
    }
    let s = textureLoad(src, l, 0);
    let rgb = clamp(unpremul(s), vec3<f32>(0.0), vec3<f32>(1.0));
    let thr = prm.k0.x;
    let k = smoothstep(thr, min(thr + 0.25, 1.0), dot(rgb, LUMA)) * select(1.0, 0.0, s.w < 0.0001);
    textureStore(dst, l, vec4<f32>(rgb * k, 1.0));
}

@compute @workgroup_size(8, 8, 1)
fn blur_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= prm.region.zw)) {
        return;
    }
    textureStore(dst, l, blur(f32(l.x), f32(l.y)));
}

// ── Per-pixel kinds (filters stylize.rs and distort.rs) ──

fn lin2srgb(x: f32) -> f32 {
    let c = max(x, 0.0);
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

fn srgb2lin(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
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

// filters `unpremul`: `a > 0 ? rgb / a : rgb`.
fn unpremul(s: vec4<f32>) -> vec3<f32> {
    return select(s.xyz, s.xyz / s.w, s.w > 0.0);
}

fn repremul(o: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(from_work(o) * a, a);
}

const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

// `Src::px`: the source at a doc-px position (texel centres at +0.5).
fn px(q: vec2<f32>) -> vec4<f32> {
    let l = q / prm.scale - vec2<f32>(prm.region.xy) - 0.5;
    return sample(l.x, l.y);
}

fn emboss_height(q: vec2<f32>) -> f32 {
    let t = px(q);
    return dot(to_work(t.xyz + 1.0 - t.w), LUMA);
}

// Line art's `lum`: luma, transparent = white.
fn line_lum(t: vec4<f32>) -> f32 {
    let l = dot(clamp(unpremul(t), vec3<f32>(0.0), vec3<f32>(1.0)), LUMA);
    return select(l, 1.0, t.w < 0.0001);
}

fn hash2(p0: vec2<f32>) -> vec2<f32> {
    let p = p0 + 17.0;
    var p3 = fract(vec3<f32>(p.x * 0.1031, p.y * 0.1030, p.x * 0.0973));
    let d = p3.x * (p3.y + 33.33) + p3.y * (p3.z + 33.33) + p3.z * (p3.x + 33.33);
    p3 += d;
    return vec2<f32>(fract((p3.x + p3.y) * p3.z), fract((p3.x + p3.z) * p3.y));
}

// One output pixel of a per-pixel kind: `l` its region pixel, `q` its doc-px centre.
fn kind_px(l: vec2<i32>, q: vec2<f32>) -> vec4<f32> {
    let k0 = prm.k0;
    switch prm.kind {
        case 1u: {
            // EMBOSS_SRC: k0 = dir (h), amount, mono.
            let src = textureLoad(src, l, 0);
            let rel = (emboss_height(q - k0.xy) - emboss_height(q + k0.xy)) * k0.z * 3.0;
            let c = to_work(unpremul(src));
            var o = c + rel;
            if (k0.w > 0.5) {
                o = vec3<f32>(0.5 + rel);
            }
            return repremul(clamp(o, vec3<f32>(0.0), vec3<f32>(1.0)), src.w);
        }
        case 2u: {
            // MOSAIC_SRC: the mean of an 8 × 8 tap lattice; k0.x = cell.
            let cell = k0.x;
            let origin = floor(q / cell) * cell;
            var sum = vec4<f32>(0.0);
            for (var y = 0; y < 8; y++) {
                for (var x = 0; x < 8; x++) {
                    let t = origin + (vec2<f32>(f32(x), f32(y)) + 0.5) * (cell / 8.0);
                    sum += px(min(t, prm.doc - 0.5));
                }
            }
            return sum / 64.0;
        }
        case 3u: {
            // RIPPLE_SRC: k0 = dir, k (2π / wavelength), amplitude.
            let dir = k0.xy;
            let nrm = vec2<f32>(-dir.y, dir.x);
            let along = dot(q, dir) * k0.z;
            let across = dot(q, nrm) * k0.z * 1.37;
            let m = k0.w * sin(along);
            let n = k0.w * 0.35 * sin(across);
            return px(q + nrm * m + dir * n);
        }
        case 4u: {
            // HALFTONE_SRC: k0 = cos, sin, cell, keep colour.
            let a = textureLoad(src, l, 0).w;
            let c = k0.x;
            let sn = k0.y;
            let scale = k0.z;
            let rp = vec2<f32>(c * q.x - sn * q.y, sn * q.x + c * q.y);
            let ccr = (floor(rp / scale) + 0.5) * scale;
            let cell_px = vec2<f32>(c * ccr.x + sn * ccr.y, -sn * ccr.x + c * ccr.y);
            let cell_rgb = clamp(to_work(unpremul(px(cell_px))), vec3<f32>(0.0), vec3<f32>(1.0));
            let lum = dot(cell_rgb, LUMA);
            let radius = (1.0 - lum) * (scale * 0.72);
            let dist = length(rp - ccr);
            let ink = 1.0 - smoothstep(radius - 1.0, radius + 1.0, dist);
            var dot_c = vec3<f32>(0.0);
            if (k0.w > 0.5) {
                dot_c = cell_rgb;
            }
            return repremul(mix(vec3<f32>(1.0), dot_c, ink), a);
        }
        case 7u: {
            // Line art, Sobel: k0 = offset, threshold, strength, line.
            let o = k0.x;
            let tl = line_lum(px(q + vec2<f32>(-1.0, 1.0) * o));
            let t = line_lum(px(q + vec2<f32>(0.0, 1.0) * o));
            let tr = line_lum(px(q + vec2<f32>(1.0, 1.0) * o));
            let lf = line_lum(px(q + vec2<f32>(-1.0, 0.0) * o));
            let rt = line_lum(px(q + vec2<f32>(1.0, 0.0) * o));
            let bl = line_lum(px(q + vec2<f32>(-1.0, -1.0) * o));
            let b = line_lum(px(q + vec2<f32>(0.0, -1.0) * o));
            let br = line_lum(px(q + vec2<f32>(1.0, -1.0) * o));
            let gx = (tr + 2.0 * rt + br) - (tl + 2.0 * lf + bl);
            let gy = (tl + 2.0 * t + tr) - (bl + 2.0 * b + br);
            let mag = length(vec2<f32>(gx, gy)) * k0.z;
            let a = clamp(smoothstep(k0.y, k0.y + 0.15, mag), 0.0, 1.0);
            return vec4<f32>(vec3<f32>(k0.w * a), a);
        }
        default: {
            // CRYSTALLIZE_SRC: Voronoi facets, seed 1; k0.x = cell.
            let cell = k0.x;
            let g = q / cell;
            let c0 = floor(g);
            var best = 1e9;
            var site = g;
            for (var dj = -1; dj <= 1; dj++) {
                for (var di = -1; di <= 1; di++) {
                    let c = c0 + vec2<f32>(f32(di), f32(dj));
                    let pt = c + hash2(c);
                    let d = (pt.x - g.x) * (pt.x - g.x) + (pt.y - g.y) * (pt.y - g.y);
                    if (d < best) {
                        best = d;
                        site = pt;
                    }
                }
            }
            let sp = site * cell;
            let r = cell * 0.2;
            let m = px(sp);
            let o = px(sp + vec2<f32>(r, 0.0)) + px(sp + vec2<f32>(-r, 0.0)) + px(sp + vec2<f32>(0.0, r)) + px(sp + vec2<f32>(0.0, -r));
            return m * 0.4 + o * 0.15;
        }
    }
}

@compute @workgroup_size(8, 8, 1)
fn blur_out(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = vec2<i32>(gid.xy);
    if (any(p >= vec2<i32>(prm.edge))) {
        return;
    }
    let l = prm.tile - prm.region.xy + p;
    var v = vec4<f32>(0.0);
    if (all(l >= vec2<i32>(0)) && all(l < prm.region.zw)) {
        if (prm.mode == 2u) {
            v = textureLoad(src, l, 0);
        } else if (prm.mode == 3u) {
            let q = (vec2<f32>(prm.tile + p) + 0.5) * prm.scale;
            v = kind_px(l, q);
        } else {
            v = blur(f32(l.x), f32(l.y));
            if (prm.mode == 1u) {
                let a = textureLoad(orig, l, 0);
                v = clamp(a + (a - v) * prm.amount, vec4<f32>(0.0), vec4<f32>(1.0));
            } else if (prm.mode == 5u) {
                // Line art, XDoG: the difference of the two Gaussians (`src`
                // σ, `orig` 1.6σ, both blurred across already). k0 = 1.6σ,
                // eps, phi, line.
                let g1 = v.x;
                let g2 = blur_in(orig, f32(l.x), f32(l.y), prm.k0.x, prm.dir).x;
                let d = g1 - 0.985 * g2;
                var e = 1.0;
                if (d < prm.k0.y) {
                    e = 1.0 + tanh(prm.k0.z * (d - prm.k0.y));
                }
                let ink = 1.0 - clamp(e, 0.0, 1.0);
                v = vec4<f32>(vec3<f32>(prm.k0.w * ink), ink);
            } else if (prm.mode == 4u) {
                // BLOOM_COMBINE_SRC: screen the glow over the source; alpha kept.
                let s = textureLoad(orig, l, 0);
                let base = to_work(unpremul(s));
                let gl = to_work(clamp(v.xyz, vec3<f32>(0.0), vec3<f32>(1.0))) * prm.amount;
                v = repremul(1.0 - (1.0 - base) * (1.0 - gl), s.w);
            }
        }
    }
    textureStore(out, p, i32(prm.out_slot), v);
}
