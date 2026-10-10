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
    // x: has selection, y: time (s), z: level of detail to sample.
    sel: vec4<f32>,
    // Spec 40: sheet width and height at level 0, sheets across and down.
    sheets: vec4<f32>,
    // Spec 48: grayscale, proof, gamut-warning threshold (LUT units, 0 off),
    // any; then the proof's linear black and white points.
    view: vec4<f32>,
    lo: vec4<f32>,
    hi: vec4<f32>,
    // Spec 50: x = 1 when the composite holds premultiplied sRGB values.
    space: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var tex: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var sel_tex: texture_2d_array<f32>;
// The press's proof LUT (gamma sRGB on white → print, ΔE in alpha), 33³.
@group(0) @binding(4) var proof_lut: texture_3d<f32>;

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

// Texel `p` (document pixels at `level`, clamped to the document) of the
// composite sheets (spec 40).
fn load_doc(p: vec2<i32>, level: u32, size: vec2<i32>) -> vec4<f32> {
    let c = clamp(p, vec2<i32>(0), size - vec2<i32>(1));
    let sheet = max(vec2<i32>(u.sheets.xy) >> vec2<u32>(level), vec2<i32>(1));
    let s = c / sheet;
    return textureLoad(tex, c - s * sheet, s.y * i32(u.sheets.z) + s.x, i32(level));
}

// Composite sample at the display level (spec 39): the composite keeps a
// box-filtered pyramid, so zoomed-out views read one level, bilinearly. Across
// several sheets the bilinear filter is done by hand, so seams don't show.
fn sample_doc(d: vec2<f32>) -> vec4<f32> {
    if (u.sheets.z * u.sheets.w < 1.5) {
        return textureSampleLevel(tex, samp, d / u.doc_size, 0, u.sel.z);
    }
    let level = u32(u.sel.z);
    let size = max(vec2<i32>(u.doc_size) >> vec2<u32>(level), vec2<i32>(1));
    let t = d / u.doc_size * vec2<f32>(size) - 0.5;
    let i = vec2<i32>(floor(t));
    let f = t - floor(t);
    let top = mix(load_doc(i, level, size), load_doc(i + vec2<i32>(1, 0), level, size), f.x);
    let bottom = mix(load_doc(i + vec2<i32>(0, 1), level, size), load_doc(i + vec2<i32>(1, 1), level, size), f.x);
    return mix(top, bottom, f.y);
}

fn sel_at(p: vec2<i32>) -> f32 {
    let c = clamp(p, vec2<i32>(0), vec2<i32>(u.doc_size) - vec2<i32>(1));
    let sheet = vec2<i32>(u.sheets.xy);
    let s = c / sheet;
    return textureLoad(sel_tex, c - s * sheet, s.y * i32(u.sheets.z) + s.x, 0).r;
}

// Marching ants on the selection's inner edge (happypaint compositor.ts:290-308):
// a band of max(1, 2 / screen-scale) doc px, diagonal 6px black/white dashes moving at 24px/s.
fn ants(d: vec2<f32>, pos: vec2<f32>) -> vec4<f32> {
    let p = vec2<i32>(floor(d));
    let b = i32(max(1.0, 2.0 / u.scale));
    let m = sel_at(p);
    let n = min(min(sel_at(p + vec2<i32>(b, 0)), sel_at(p - vec2<i32>(b, 0))), min(sel_at(p + vec2<i32>(0, b)), sel_at(p - vec2<i32>(0, b))));
    if (m >= 0.5 && n <= 0.5) {
        let k = (pos.x + pos.y + u.sel.y * 24.0) % 12.0;
        return vec4<f32>(select(vec3<f32>(1.0), vec3<f32>(0.0), k < 6.0), 1.0);
    }
    return vec4<f32>(0.0);
}

// `cmyk::trilinear` on the proof LUT (0..1 per channel, ΔE / 255 in w).
fn proof_at(c: vec3<f32>) -> vec4<f32> {
    let n = i32(textureDimensions(proof_lut).x);
    let t = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(n - 1);
    let i = min(vec3<i32>(floor(t)), vec3<i32>(n - 2));
    let f = t - vec3<f32>(i);
    var out = vec4<f32>(0.0);
    for (var k = 0; k < 8; k++) {
        let d = vec3<i32>(k & 1, (k >> 1) & 1, (k >> 2) & 1);
        let w = select(1.0 - f.x, f.x, d.x == 1) * select(1.0 - f.y, f.y, d.y == 1) * select(1.0 - f.z, f.z, d.z == 1);
        out += w * textureLoad(proof_lut, i + d, 0);
    }
    return out;
}

// Present-only view modes on premultiplied linear `p` (`cmyk::apply_view_with`):
// grayscale on the encoded colour, then the proof on white and the warning.
fn view_modes(p: vec4<f32>) -> vec4<f32> {
    let a = p.a;
    var c = vec3<f32>(0.0);
    if (a > 0.0) {
        c = lin2srgb(p.rgb / a) * a;
    }
    if (u.view.x > 0.5) {
        c = vec3<f32>(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)));
    }
    var out_a = a;
    if (u.view.y > 0.5 || u.view.z > 0.0) {
        let on_white = clamp(c + 1.0 - a, vec3<f32>(0.0), vec3<f32>(1.0));
        let s = proof_at(on_white);
        if (u.view.y > 0.5) {
            c = lin2srgb(clamp((srgb2lin(s.rgb) - u.lo.rgb) / max(u.hi.rgb - u.lo.rgb, vec3<f32>(1e-4)), vec3<f32>(0.0), vec3<f32>(1.0)));
            out_a = 1.0;
        }
        if (u.view.z > 0.0 && s.w * 255.0 >= u.view.z) {
            c = vec3<f32>(0.78, 0.06, 0.10);
            out_a = 1.0;
        }
    }
    if (out_a <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(srgb2lin(c / out_a) * out_a, out_a);
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
        var p = sample_doc(d);
        if (u.space.x > 0.5 && p.a > 0.0) {
            p = vec4<f32>(srgb2lin(p.rgb / p.a) * p.a, p.a);
        }
        if (u.view.w > 0.5) {
            p = view_modes(p);
        }
        var s = vec3<f32>(0.0);
        if (p.a > 0.0) {
            s = lin2srgb(p.rgb / p.a);
        }
        // Over the checker in display (gamma) space, like happypaint's present.
        display = s * p.a + chk * (1.0 - p.a);
        if (u.sel.x > 0.5) {
            let a = ants(d, pos.xy);
            display = mix(display, a.rgb, a.a);
        }
    }
    if (u.srgb_target > 0.5) {
        return vec4<f32>(srgb2lin(display), 1.0);
    }
    return vec4<f32>(display, 1.0);
}
