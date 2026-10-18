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
    // 0 blur, 1 sharpen (combine with `orig`), 2 copy.
    mode: u32,
    dir: vec2<f32>,
    radius: f32,
    amount: f32,
    out_slot: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
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
fn sample(x0: f32, y0: f32) -> vec4<f32> {
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
    let a = textureLoad(src, vec2<i32>(xi, yi), 0);
    let b = textureLoad(src, vec2<i32>(x1, yi), 0);
    let c = textureLoad(src, vec2<i32>(xi, y1), 0);
    let d = textureLoad(src, vec2<i32>(x1, y1), 0);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    return top + (bot - top) * fy;
}

// filters `blur_pass` at one pixel.
fn blur(x: f32, y: f32) -> vec4<f32> {
    let spread = max(prm.radius, 1.0) / f32(HALF);
    let sigma = f32(HALF) * 0.5;
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = 0; k <= 2 * HALF; k++) {
        let i = f32(k - HALF);
        let wt = exp(-0.5 * i * i / (sigma * sigma));
        let o = i * spread;
        acc += sample(x + prm.dir.x * o, y + prm.dir.y * o) * wt;
        wsum += wt;
    }
    return acc / max(wsum, 1e-4);
}

@compute @workgroup_size(8, 8, 1)
fn blur_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let l = vec2<i32>(gid.xy);
    if (any(l >= prm.region.zw)) {
        return;
    }
    textureStore(dst, l, blur(f32(l.x), f32(l.y)));
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
        } else {
            v = blur(f32(l.x), f32(l.y));
            if (prm.mode == 1u) {
                let a = textureLoad(orig, l, 0);
                v = clamp(a + (a - v) * prm.amount, vec4<f32>(0.0), vec4<f32>(1.0));
            }
        }
    }
    textureStore(out, p, i32(prm.out_slot), v);
}
