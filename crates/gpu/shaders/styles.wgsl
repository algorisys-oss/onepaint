// Layer styles (spec 56): compose styles.rs's pass on the GPU. The node's
// own content over its region (tile ± margin) comes from cache slots
// (`assemble`), its exact distance field from two passes of Felzenszwalb's
// transform (`edt_cols`, `edt_rows`), and `style` runs the effects in
// happypaint's order against what's under the node (`under`), writing the
// filtered tile. Coordinates are level pixels; sizes arrive scaled.
// Built with blend.wgsl in front.

struct Prm {
    // The region (origin, size), the output tile's origin, the tile edge and level.
    region: vec4<i32>,
    tile: vec2<i32>,
    edge: i32,
    level: i32,
    // The content grid: first tile column and row, columns.
    grid: vec3<i32>,
    out_slot: u32,
    // The document's size; the field's cap; Fill opacity.
    doc: vec2<f32>,
    cap: f32,
    fill: f32,
    // Flags: 1 linear document, 2 a backdrop, 4 a flat field (no reach).
    flags: u32,
    steps: u32,
    under_slot: u32,
    bevel_on: u32,
    // The bevel: master opacity, style, contour, size; slope, step, texture tile, depth.
    bv0: vec4<f32>,
    bv1: vec4<f32>,
    // Towards the light (y down), and the texture: on, invert sign, pattern.
    light: vec4<f32>,
    tex: vec4<f32>,
}

// One step of the effect stack (compose styles.rs, in its order).
struct Step {
    // What: 0 body over the effects under it, 1 drop shadow, 2 outer glow,
    // 3 outside stroke, 4 pattern, 5 gradient, 6 colour overlay, 7 satin,
    // 8 inner glow, 9 inner shadow, 10 inside stroke, 11 bevel shadow,
    // 12 bevel highlight, 13 centre stroke.
    kind: u32,
    mode: u32,
    extra: u32,
    _p: u32,
    // rgb and opacity.
    color: vec4<f32>,
    a: vec4<f32>,
    b: vec4<f32>,
}

@group(0) @binding(0) var<uniform> prm: Prm;
@group(0) @binding(1) var<storage, read> slots: array<u32>;
@group(0) @binding(2) var cache_tex: texture_2d_array<f32>;
@group(0) @binding(3) var src: texture_2d<f32>;
@group(0) @binding(4) var dst: texture_storage_2d<rgba32float, write>;
@group(0) @binding(5) var out: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(6) var<storage, read_write> sq: array<f32>;
@group(0) @binding(7) var<storage, read_write> field: array<f32>;
@group(0) @binding(8) var<storage, read> steps: array<Step>;
// Gradient ramps (1024 entries a row) and pattern tiles (32 × 32 grey each).
@group(0) @binding(9) var<storage, read> ramps: array<vec4<f32>>;
@group(0) @binding(10) var<storage, read> patterns: array<f32>;

override PIGMENT: bool = false;

// Effects never blend as Pigment; blend.wgsl only needs the name.
fn pigment_mix(b: vec3<f32>, s: vec3<f32>, t: f32) -> vec3<f32> {
    return s;
}

const NONE: u32 = 0xffffffffu;
const MAXN: i32 = 768;

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

// One line of Felzenszwalb's transform: squared distance to the nearest
// seed (f = 0) from `f`, in place, `n` cells read through `at`.
var<private> fv: array<f32, 768>;
var<private> vv: array<i32, 768>;
var<private> zv: array<f32, 769>;

fn envelope(n: i32) {
    var k = 0;
    vv[0] = 0;
    zv[0] = -1e30;
    zv[1] = 1e30;
    for (var q = 1; q < n; q++) {
        var s: f32;
        loop {
            let p = vv[k];
            s = ((fv[q] + f32(q * q)) - (fv[p] + f32(p * p))) / (2.0 * f32(q - p));
            if (s <= zv[k] && k > 0) {
                k -= 1;
            } else {
                break;
            }
        }
        if (s <= zv[k]) {
            vv[0] = q;
            zv[0] = -1e30;
            zv[1] = 1e30;
            continue;
        }
        k += 1;
        vv[k] = q;
        zv[k] = s;
        zv[k + 1] = 1e30;
    }
}

fn envelope_at(q: i32, k0: ptr<function, i32>) -> f32 {
    while (zv[*k0 + 1] < f32(q)) {
        *k0 += 1;
    }
    let d = f32(q - vv[*k0]);
    return d * d + fv[vv[*k0]];
}

fn far() -> f32 {
    let s = f32(prm.region.z + prm.region.w);
    return s * s * 4.0;
}

// Columns: squared distance down each column, to inside (first half of
// `sq`) and to outside (second half).
@compute @workgroup_size(64, 1, 1)
fn edt_cols(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let w = prm.region.z;
    let h = prm.region.w;
    if (x >= w) {
        return;
    }
    for (var pass_ = 0; pass_ < 2; pass_++) {
        for (var y = 0; y < h; y++) {
            let inside = textureLoad(src, vec2<i32>(x, y), 0).w >= 0.5;
            let seed = select(!inside, inside, pass_ == 0);
            fv[y] = select(far(), 0.0, seed);
        }
        envelope(h);
        var k = 0;
        for (var y = 0; y < h; y++) {
            sq[pass_ * w * h + y * w + x] = envelope_at(y, &k);
        }
    }
}

// Rows: the full squared distances, then the signed field (+ outside,
// − inside, ±½ at the edge), clamped and stored at f16 precision.
@compute @workgroup_size(64, 1, 1)
fn edt_rows(@builtin(global_invocation_id) gid: vec3<u32>) {
    let y = i32(gid.x);
    let w = prm.region.z;
    let h = prm.region.w;
    if (y >= h) {
        return;
    }
    let big = far();
    for (var pass_ = 0; pass_ < 2; pass_++) {
        for (var x = 0; x < w; x++) {
            fv[x] = sq[pass_ * w * h + y * w + x];
        }
        envelope(w);
        var k = 0;
        for (var x = 0; x < w; x++) {
            let d2 = envelope_at(x, &k);
            let inside = textureLoad(src, vec2<i32>(x, y), 0).w >= 0.5;
            // Inside reads the distance to outside (pass 1), outside to inside (pass 0).
            if (inside == (pass_ == 1)) {
                var s: f32;
                if (d2 >= big) {
                    s = select(prm.cap, -prm.cap, inside);
                } else if (inside) {
                    s = -(sqrt(d2) - 0.5);
                } else {
                    s = sqrt(d2) - 0.5;
                }
                s = clamp(s, -prm.cap, prm.cap);
                field[y * w + x] = unpack2x16float(pack2x16float(vec2<f32>(s, 0.0))).x;
            }
        }
    }
}

// Bilinear over the region at level px `q` (texel centres at +½), clamped to the document.
fn region_at(which: u32, q: vec2<f32>) -> f32 {
    let dmax = vec2<f32>(floor(prm.doc)) - 1.0;
    let t = clamp(q - 0.5, vec2<f32>(0.0), max(dmax, vec2<f32>(0.0)));
    let x0 = i32(floor(t.x));
    let y0 = i32(floor(t.y));
    let x1 = min(x0 + 1, i32(dmax.x));
    let y1 = min(y0 + 1, i32(dmax.y));
    let f = t - floor(t);
    let a = fetch(which, x0, y0);
    let b = fetch(which, x1, y0);
    let c = fetch(which, x0, y1);
    let d = fetch(which, x1, y1);
    let top = a + (b - a) * f.x;
    let bot = c + (d - c) * f.x;
    return top + (bot - top) * f.y;
}

// The field (0) or the alpha (1) at a level pixel, 0 outside the region.
fn fetch(which: u32, x: i32, y: i32) -> f32 {
    let l = vec2<i32>(x, y) - prm.region.xy;
    if (any(l < vec2<i32>(0)) || any(l >= prm.region.zw)) {
        return 0.0;
    }
    if (which == 0u) {
        return field[l.y * prm.region.z + l.x];
    }
    return textureLoad(src, l, 0).w;
}

// `sdf()`: off the canvas is outside; within a pixel of the edge the alpha
// says where it runs.
fn sdf(q: vec2<f32>) -> f32 {
    if (q.x < 0.0 || q.y < 0.0 || q.x > prm.doc.x || q.y > prm.doc.y) {
        return prm.cap;
    }
    var f = 0.0;
    if ((prm.flags & 4u) == 0u) {
        f = region_at(0u, q);
    }
    if (abs(f) < 1.0) {
        f = clamp(0.5 - region_at(1u, q), -0.5, 0.5);
    }
    return f;
}

fn fall(x: f32, f: vec2<f32>) -> f32 {
    if (f.y < 0.5) {
        return clamp(f.x + 0.5 - x, 0.0, 1.0);
    }
    return 1.0 - sstep(f.x, f.x + f.y, x);
}

fn contour(id: u32, x0: f32) -> f32 {
    let x = clamp(x0, 0.0, 1.0);
    switch id {
        case 1u: { return sqrt(max(1.0 - (1.0 - x) * (1.0 - x), 0.0)); }
        case 2u: { return x * x * (3.0 - 2.0 * x); }
        case 3u: { return 1.0 - abs(2.0 * x - 1.0); }
        case 4u: { return abs(2.0 * x - 1.0); }
        case 5u: { return 0.5 - 0.5 * cos(12.566370614359172 * x); }
        case 6u: { return clamp(x + 0.12 * sin(6.283185307179586 * x), 0.0, 1.0); }
        default: { return x; }
    }
}

// A pattern tile's grey, bilinear and repeating, at level px `q`, tile `tile` px wide.
fn pattern(id: u32, q: vec2<f32>, tile: f32) -> f32 {
    let u = q / tile * 32.0 - 0.5;
    let f0 = floor(u);
    let f = u - f0;
    let base = id * 1024u;
    let x0 = u32(i32(f0.x) - 32 * i32(floor(f0.x / 32.0)));
    let y0 = u32(i32(f0.y) - 32 * i32(floor(f0.y / 32.0)));
    let x1 = (x0 + 1u) % 32u;
    let y1 = (y0 + 1u) % 32u;
    let a = patterns[base + y0 * 32u + x0];
    let b = patterns[base + y0 * 32u + x1];
    let c = patterns[base + y1 * 32u + x0];
    let d = patterns[base + y1 * 32u + x1];
    let top = a + (b - a) * f.x;
    let bot = c + (d - c) * f.x;
    return top + (bot - top) * f.y;
}

fn ramp(row: u32, t: f32) -> vec4<f32> {
    let x = clamp(t, 0.0, 1.0) * 1023.0;
    let i0 = u32(floor(x));
    let i1 = min(i0 + 1u, 1023u);
    let f = x - f32(i0);
    let a = ramps[row * 1024u + i0];
    let b = ramps[row * 1024u + i1];
    return a + (b - a) * f;
}

fn over(top: vec4<f32>, bottom: vec4<f32>) -> vec4<f32> {
    return top + bottom * (1.0 - top.w);
}

fn paint(rgb: vec3<f32>, k: f32) -> vec4<f32> {
    return vec4<f32>(rgb * k, k);
}

// An effect's colour after its blend mode against what's under it (`eff`).
fn eff(m: u32, c: vec3<f32>, stack: vec4<f32>, bd: vec4<f32>) -> vec3<f32> {
    if (m == M_NORMAL || (prm.flags & 2u) == 0u) {
        return c;
    }
    let u = over(stack, bd);
    if (u.w <= 0.0) {
        return c;
    }
    var b = u.xyz / u.w;
    var sc = c;
    let lin = (prm.flags & 1u) != 0u;
    if (lin) {
        b = vec3<f32>(lin2srgb(b.x), lin2srgb(b.y), lin2srgb(b.z));
        sc = vec3<f32>(lin2srgb(sc.x), lin2srgb(sc.y), lin2srgb(sc.z));
    }
    var big = blend_color(m, b, sc);
    if (lin) {
        big = vec3<f32>(srgb2lin(big.x), srgb2lin(big.y), srgb2lin(big.z));
    }
    return c + (big - c) * u.w;
}

fn bevel_height(q: vec2<f32>) -> f32 {
    let sd = sdf(q);
    let s = prm.bv0.w;
    let style = u32(prm.bv0.y);
    let ct = u32(prm.bv0.z);
    var h: f32;
    if (style == 1u) {
        h = contour(ct, 1.0 - clamp(sd / s, 0.0, 1.0));
    } else if (style == 2u) {
        h = contour(ct, clamp(0.5 - sd / s, 0.0, 1.0));
    } else if (style == 3u) {
        if (sd <= 0.0) {
            h = contour(ct, clamp(-sd / s, 0.0, 1.0));
        } else {
            h = -contour(ct, 1.0 - clamp(sd / s, 0.0, 1.0));
        }
    } else {
        h = contour(ct, clamp(-sd / s, 0.0, 1.0));
    }
    if (prm.tex.x > 0.5) {
        let t = pattern(u32(prm.tex.z), q, prm.bv1.z);
        h += prm.tex.y * (t - 0.5) * prm.bv1.w * clamp(0.5 - sd, 0.0, 1.0) / max(s, 1.0) * 4.0;
    }
    return h;
}

@compute @workgroup_size(8, 8, 1)
fn style(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pl = vec2<i32>(gid.xy);
    if (any(pl >= vec2<i32>(prm.edge))) {
        return;
    }
    let g = prm.tile + pl;
    let p = vec2<f32>(g) + 0.5;
    let rl = g - prm.region.xy;
    var c = vec4<f32>(0.0);
    if (all(rl >= vec2<i32>(0)) && all(rl < prm.region.zw)) {
        c = textureLoad(src, rl, 0);
    }
    let a = c.w;
    let s = sdf(p);
    var bd = vec4<f32>(0.0);
    if (prm.under_slot != NONE) {
        bd = textureLoad(cache_tex, pl, i32(prm.under_slot), prm.level);
    }
    let outside = clamp(s + 1.5, 0.0, 1.0);
    let known = clamp(1.5 - s, 0.0, 1.0);
    var body = c * prm.fill;
    var r = vec4<f32>(0.0);
    var lit = 0.0;
    var cover = 0.0;
    if (prm.bevel_on != 0u) {
        let d = prm.bv1.y;
        let h00 = bevel_height(p + vec2<f32>(-d, -d));
        let h10 = bevel_height(p + vec2<f32>(0.0, -d));
        let h20 = bevel_height(p + vec2<f32>(d, -d));
        let h01 = bevel_height(p + vec2<f32>(-d, 0.0));
        let h21 = bevel_height(p + vec2<f32>(d, 0.0));
        let h02 = bevel_height(p + vec2<f32>(-d, d));
        let h12 = bevel_height(p + vec2<f32>(0.0, d));
        let h22 = bevel_height(p + vec2<f32>(d, d));
        let gx = ((h20 + 2.0 * h21 + h22) - (h00 + 2.0 * h01 + h02)) / (8.0 * d);
        let gy = ((h02 + 2.0 * h12 + h22) - (h00 + 2.0 * h10 + h20)) / (8.0 * d);
        let slope = prm.bv1.x;
        let n = vec3<f32>(-gx * slope, -gy * slope, 1.0);
        let dt = dot(n, prm.light.xyz) / length(n);
        lit = clamp((dt - prm.light.z) * 2.5, -1.0, 1.0);
        let style = u32(prm.bv0.y);
        var inside = a * known;
        if (style == 1u) {
            inside = 0.0;
        }
        var ring = (1.0 - a) * clamp(prm.bv0.w + 0.5 - s, 0.0, 1.0) * outside;
        if (style == 0u) {
            ring = 0.0;
        }
        cover = (inside + ring) * prm.bv0.x;
    }
    for (var i = 0u; i < prm.steps; i++) {
        let st = steps[i];
        if (st.kind == 0u) {
            r = over(body, r);
        } else {
            var col = st.color.xyz;
            let op = st.color.w;
            var k = 0.0;
            // Inside the pixels (into the body) for kinds 4–10, else under or over them.
            let into_body = st.kind >= 4u && st.kind <= 10u;
            switch st.kind {
                case 1u: { k = fall(sdf(p - st.a.xy), st.a.zw) * op * (1.0 - a) * outside; }
                case 2u: { k = fall(s, st.a.xy) * op * (1.0 - a) * outside; }
                case 3u: { k = clamp(st.a.x + 0.5 - s, 0.0, 1.0) * clamp(s + 1.5, 0.0, 1.0) * op; }
                case 4u: {
                    let cov = pattern(st.extra, p, st.a.x);
                    col = st.b.xyz + (col - st.b.xyz) * cov;
                    k = a * op;
                }
                case 5u: {
                    var t = shape_t(st.extra & 255u, p - st.a.xy, st.a.zw - st.a.xy);
                    if (((st.extra >> 8u) & 1u) != 0u) {
                        t = 1.0 - t;
                    }
                    let gr = ramp(st.extra >> 16u, t);
                    col = gr.xyz;
                    k = gr.w * a * op;
                }
                case 6u: { k = a * op; }
                case 7u: {
                    let size = max(st.a.z, 0.5);
                    let m1 = 1.0 - sstep(-size, size, sdf(p + st.a.xy));
                    let m2 = 1.0 - sstep(-size, size, sdf(p - st.a.xy));
                    var v = contour(u32(st.a.w), abs(m1 - m2));
                    if (st.extra != 0u) {
                        v = 1.0 - v;
                    }
                    k = v * a * known * op;
                }
                case 8u: { k = fall(-s, st.a.xy) * a * known * op; }
                case 9u: { k = fall(-sdf(p - st.a.xy), st.a.zw) * a * known * op; }
                case 10u: { k = clamp(st.a.x + 0.5 + s, 0.0, 1.0) * a * known * op; }
                case 11u: { k = max(-lit, 0.0) * op * cover; }
                case 12u: { k = max(lit, 0.0) * op * cover; }
                default: { k = clamp(st.a.x * 0.5 + 0.5 - abs(s), 0.0, 1.0) * op; }
            }
            if (into_body) {
                let e = paint(eff(st.mode, col, over(body, r), bd), k);
                body = over(e, body);
            } else {
                let e = paint(eff(st.mode, col, r, bd), k);
                r = over(e, r);
            }
        }
    }
    textureStore(out, pl, i32(prm.out_slot), r);
}
