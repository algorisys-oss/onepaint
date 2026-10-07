// Canvas present pass: draws the premultiplied linear composite through the
// view transform over a screen-fixed checkerboard (happypaint compositor.ts
// FRAGMENT_SRC + CHECKER_FRAGMENT_SRC). Coordinates are physical pixels.

struct Uniforms {
    // Callback rect origin and size in framebuffer pixels.
    origin: vec2<f32>,
    size: vec2<f32>,
    // Pan offset in pixels.
    offset: vec2<f32>,
    doc_size: vec2<f32>,
    // Document pixels → framebuffer pixels.
    scale: f32,
    rotation: f32,
    checker: f32,
    // 1 when the target format is sRGB-encoding (shader must output linear).
    srgb_target: f32,
    stage: vec4<f32>,
    checker_a: vec4<f32>,
    checker_b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn lin2srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, x <= vec3<f32>(0.0031308));
}

fn srgb2lin(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn to_doc(px: vec2<f32>) -> vec2<f32> {
    let q = px - u.origin - u.size * 0.5 - u.offset;
    let c = cos(u.rotation);
    let s = sin(u.rotation);
    return vec2<f32>(q.x * c + q.y * s, -q.x * s + q.y * c) / u.scale + u.doc_size * 0.5;
}

// Composite sample; box-filters when zoomed out so thin lines don't shimmer.
fn sample_doc(d: vec2<f32>) -> vec4<f32> {
    let uv = d / u.doc_size;
    if (u.scale >= 0.75) {
        return textureSampleLevel(tex, samp, uv, 0.0);
    }
    let n = i32(clamp(ceil(1.0 / u.scale), 2.0, 6.0));
    let step = 1.0 / (u.scale * f32(n));
    var acc = vec4<f32>(0.0);
    for (var y = 0; y < n; y = y + 1) {
        for (var x = 0; x < n; x = x + 1) {
            let o = (vec2<f32>(f32(x), f32(y)) + 0.5) * step - 0.5 / u.scale;
            acc = acc + textureSampleLevel(tex, samp, (d + o) / u.doc_size, 0.0);
        }
    }
    return acc / f32(n * n);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = to_doc(pos.xy);
    var display: vec3<f32>;
    if (d.x < 0.0 || d.y < 0.0 || d.x >= u.doc_size.x || d.y >= u.doc_size.y) {
        display = u.stage.rgb;
    } else {
        let cell = floor(pos.xy / u.checker);
        let odd = (i32(cell.x) + i32(cell.y)) % 2 != 0;
        let chk = select(u.checker_a.rgb, u.checker_b.rgb, odd);
        let p = sample_doc(d);
        var s = vec3<f32>(0.0);
        if (p.a > 0.0) {
            s = lin2srgb(p.rgb / p.a);
        }
        // Over the checker in display (gamma) space, like happypaint's present.
        display = s * p.a + chk * (1.0 - p.a);
    }
    if (u.srgb_target > 0.5) {
        return vec4<f32>(srgb2lin(display), 1.0);
    }
    return vec4<f32>(display, 1.0);
}
