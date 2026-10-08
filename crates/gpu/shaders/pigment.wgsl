// Kubelka–Munk pigment mixing, shared by the wet-media and compositor
// shaders (port of crates/color/src/pigment.rs). The including module
// declares `km: Km` (the 9-band basis and its pseudo-inverse) and `sstep`.

fn s2l(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

fn l2s(c: f32) -> f32 {
    let x = clamp(c, 0.0, 1.0);
    return select(1.055 * pow(x, 1.0 / 2.4) - 0.055, x * 12.92, x <= 0.0031308);
}

fn s2l3(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(s2l(c.x), s2l(c.y), s2l(c.z));
}

fn l2s3(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(l2s(c.x), l2s(c.y), l2s(c.z));
}

fn clamp_r(r: f32) -> f32 {
    return clamp(r, 1e-3, 1.0 - 1e-3);
}

fn sat3(c: vec3<f32>) -> f32 {
    let mx = max(c.x, max(c.y, c.z));
    let mn = min(c.x, min(c.y, c.z));
    return select(0.0, (mx - mn) / mx, mx > 1e-4);
}

fn hue_vec(c: vec3<f32>) -> vec2<f32> {
    let x = c.x - 0.5 * (c.y + c.z);
    let y = 0.8660254 * (c.y - c.z);
    let m = length(vec2<f32>(x, y));
    return select(vec2<f32>(0.0), vec2<f32>(x, y) / m, m > 1e-5);
}

fn hsv_vec_to_rgb(hx: f32, hy: f32, s: f32, v: f32) -> vec3<f32> {
    var h = atan2(hy, hx) * 3.0 / 3.14159265;
    if (h < 0.0) {
        h += 6.0;
    }
    var out = vec3<f32>(0.0);
    let ns = vec3<f32>(5.0, 3.0, 1.0);
    for (var i = 0; i < 3; i++) {
        let k = (ns[i] + h) % 6.0;
        out[i] = v - v * s * clamp(min(k, 4.0 - k), 0.0, 1.0);
    }
    return out;
}

fn sgn(x: f32) -> f32 {
    return select(select(0.0, -1.0, x < 0.0), 1.0, x > 0.0);
}

fn chroma_restore(a: vec3<f32>, b: vec3<f32>, t: f32, o: vec3<f32>) -> vec3<f32> {
    let target_sat = (1.0 - t) * sat3(a) + t * sat3(b);
    let so = sat3(o);
    if (target_sat <= so) {
        return o;
    }
    let ua = hue_vec(a);
    let ub = hue_vec(b);
    let uo = hue_vec(o);
    let mx0 = (1.0 - t) * sat3(a) * ua.x + t * sat3(b) * ub.x;
    let my0 = (1.0 - t) * sat3(a) * ua.y + t * sat3(b) * ub.y;
    let mm = length(vec2<f32>(mx0, my0));
    let cab = ua.x * ub.y - ua.y * ub.x;
    let side_a = (ua.x * uo.y - ua.y * uo.x) * sgn(cab);
    let side_b = (uo.x * ub.y - uo.y * ub.x) * sgn(cab);
    let arc_side = min(side_a, side_b);
    let sat_new = min(so + (target_sat - so) * 0.55, 1.0);
    let vo = max(o.x, max(o.y, o.z));
    if (mm < 1e-4) {
        return o;
    }
    let chroma_trust = sstep(0.0, 0.22, so);
    let arc_trust = select(sstep(-0.12, 0.0, arc_side), 1.0, abs(cab) < 1e-4);
    let trust = chroma_trust * arc_trust;
    let mux = mx0 / mm;
    let muy = my0 / mm;
    var hx = uo.x * trust + mux * (1.0 - trust);
    var hy = uo.y * trust + muy * (1.0 - trust);
    let hm = length(vec2<f32>(hx, hy));
    if (hm < 1e-5) {
        hx = mux;
        hy = muy;
    } else {
        hx /= hm;
        hy /= hm;
    }
    let f = select(1.0, sat_new / so, so > 1e-6);
    let scaled = clamp(vec3<f32>(vo) - (vec3<f32>(vo) - o) * f, vec3<f32>(0.0), vec3<f32>(1.0));
    if (trust >= 1.0) {
        return scaled;
    }
    let rebuilt = hsv_vec_to_rgb(hx, hy, sat_new, vo);
    return scaled * trust + rebuilt * (1.0 - trust);
}

// Subtractive mix of two straight sRGB colours; `t` is `b`'s weight.
fn pigment_mix(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    let wa = 1.0 - t;
    let la = s2l3(a);
    let lb = s2l3(b);
    var sa: array<f32, 9>;
    var sb: array<f32, 9>;
    var mina = 1.0;
    var minb = 1.0;
    for (var k = 0; k < 9; k++) {
        sa[k] = dot(km.b[k].xyz, la);
        sb[k] = dot(km.b[k].xyz, lb);
        mina = min(mina, clamp_r(sa[k]));
        minb = min(minb, clamp_r(sb[k]));
    }
    let sca = max(mina * mina, 1e-3);
    let scb = max(minb * minb, 1e-3);
    var out = vec3<f32>(0.0);
    for (var k = 0; k < 9; k++) {
        let ra = clamp_r(sa[k]);
        let rb = clamp_r(sb[k]);
        let ka = sca * (1.0 - ra) * (1.0 - ra) / (2.0 * ra);
        let kb = scb * (1.0 - rb) * (1.0 - rb) / (2.0 * rb);
        let v = (wa * ka + t * kb) / (wa * sca + t * scb);
        let m = 1.0 + v - sqrt(v * v + 2.0 * v);
        out += km.t[k].xyz * m;
    }
    return chroma_restore(a, b, t, l2s3(out));
}
