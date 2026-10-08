// Live wet media on the GPU: one simulation step as two compute passes.
// A port of `crates/brush/src/fluid/step.rs` (itself happypaint's
// SETTLE_FRAG + UPDATE_FRAG); the CPU step is the oracle in the tests.
//
// settle: old suspended paint → `acc`, composited source-over in step order.
// update: old state → new state (ping-pong).

struct Params {
    size: vec2<u32>,      // allocation w, h
    origin: vec2<f32>,    // allocation origin, doc px
    tilt: vec2<f32>,
    oil: f32,
    paper_strength: f32,  // 0 = smooth
    paper_tile_px: f32,   // doc px per paper tile
    paper_size: u32,      // paper tile texels per side
    blocks_w: u32,        // active-block grid width (16-px blocks)
    _pad: f32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> src_sim: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> src_col: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> dst_sim: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> dst_col: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> acc: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> finger: array<f32>;
@group(0) @binding(7) var<storage, read> paper: array<f32>;
// 1 where a 16-px block may hold state during this batch (the CPU's
// occupancy, widened by the batch's spread); elsewhere everything is zero.
@group(0) @binding(9) var<storage, read> live_blocks: array<u32>;

fn is_active(x: u32, y: u32) -> bool {
    return live_blocks[(y / 16u) * p.blocks_w + x / 16u] != 0u;
}
// The KM basis: band k → (r, g, b) weights in b[k].xyz, and the
// pseudo-inverse's column k (r, g, b ← band k) in t[k].xyz.
struct Km {
    b: array<vec4<f32>, 9>,
    t: array<vec4<f32>, 9>,
}
@group(0) @binding(8) var<uniform> km: Km;

const DIFF_W: f32 = 0.5;
const DIFF_P: f32 = 0.55;
const SETTLE_DRY: f32 = 0.06;
const SETTLE_WET: f32 = 0.0015;
const EDGE_SETTLE: f32 = 1.0;
const RIM_GATE_R: f32 = 8.0;
const G_TILT: f32 = 0.08;
const G_SLOPE: f32 = 0.16;
const DAMP: f32 = 0.93;
const VORT_EPS: f32 = 0.5;
const PAPER_ABSORB: f32 = 1.6;
const PAPER_RESIST: f32 = 0.5;
const GRAN_SWING: f32 = 1.2;
const EVAP: f32 = 0.003;
const WATER_MAX: f32 = 2.5;
const V_MAX: f32 = 2.0;

fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn idx(x: i32, y: i32) -> u32 {
    let w = i32(p.size.x);
    let h = i32(p.size.y);
    return u32(clamp(y, 0, h - 1) * w + clamp(x, 0, w - 1));
}

// Bilinear at (u, v) in pixel units (centres at i + 0.5), clamp-to-edge.
fn sample_sim(u: f32, v: f32) -> vec4<f32> {
    let x = u - 0.5;
    let y = v - 0.5;
    let x0 = floor(x);
    let y0 = floor(y);
    let fx = x - x0;
    let fy = y - y0;
    let ix = i32(x0);
    let iy = i32(y0);
    if (fx == 0.0 && fy == 0.0) {
        return src_sim[idx(ix, iy)];
    }
    let top = mix(src_sim[idx(ix, iy)], src_sim[idx(ix + 1, iy)], fx);
    let bot = mix(src_sim[idx(ix, iy + 1)], src_sim[idx(ix + 1, iy + 1)], fx);
    return mix(top, bot, fy);
}

fn sample_col(u: f32, v: f32) -> vec4<f32> {
    let x = u - 0.5;
    let y = v - 0.5;
    let x0 = floor(x);
    let y0 = floor(y);
    let fx = x - x0;
    let fy = y - y0;
    let ix = i32(x0);
    let iy = i32(y0);
    if (fx == 0.0 && fy == 0.0) {
        return src_col[idx(ix, iy)];
    }
    let top = mix(src_col[idx(ix, iy)], src_col[idx(ix + 1, iy)], fx);
    let bot = mix(src_col[idx(ix, iy + 1)], src_col[idx(ix + 1, iy + 1)], fx);
    return mix(top, bot, fy);
}

// Paper height at sim-local (u, v): the grain tile repeat-sampled at doc px.
fn paper_at(u: f32, v: f32) -> f32 {
    let n = i32(p.paper_size);
    let nf = f32(n);
    let x = (u + p.origin.x) / p.paper_tile_px * nf - 0.5;
    let y = (v + p.origin.y) / p.paper_tile_px * nf - 0.5;
    let xm = x - floor(x / nf) * nf;
    let ym = y - floor(y / nf) * nf;
    let x0 = i32(xm) % n;
    let y0 = i32(ym) % n;
    let x1 = (x0 + 1) % n;
    let y1 = (y0 + 1) % n;
    let fx = xm - floor(xm);
    let fy = ym - floor(ym);
    let top = mix(paper[y0 * n + x0], paper[y0 * n + x1], fx);
    let bot = mix(paper[y1 * n + x0], paper[y1 * n + x1], fx);
    return mix(top, bot, fy);
}

// (settle rate, wet-front strength); the settle and update passes agree.
fn settle_rate(w: f32, u: f32, v: f32) -> vec2<f32> {
    let wl = sample_sim(u - 1.0, v).x;
    let wr = sample_sim(u + 1.0, v).x;
    let wu = sample_sim(u, v - 1.0).x;
    let wd = sample_sim(u, v + 1.0).x;
    let front = clamp((max(max(wl, wr), max(wu, wd)) - min(min(wl, wr), min(wu, wd))) * 3.0, 0.0, 1.0);
    var near = max(w, max(max(wl, wr), max(wu, wd)));
    for (var ring = 0; ring < 2; ring++) {
        if (near >= 0.35) {
            break;
        }
        let r = select(RIM_GATE_R, RIM_GATE_R / 2.0, ring == 0);
        let d = r * 0.70710677;
        near = max(near, sample_sim(u + r, v).x);
        near = max(near, sample_sim(u - r, v).x);
        near = max(near, sample_sim(u, v + r).x);
        near = max(near, sample_sim(u, v - r).x);
        near = max(near, sample_sim(u + d, v + d).x);
        near = max(near, sample_sim(u - d, v - d).x);
        near = max(near, sample_sim(u + d, v - d).x);
        near = max(near, sample_sim(u - d, v + d).x);
    }
    let wet = sstep(0.0, 0.35, near);
    var dc = SETTLE_DRY + (SETTLE_WET - SETTLE_DRY) * wet;
    dc *= 1.0 - p.oil;
    dc *= 1.0 + EDGE_SETTLE * front * (1.0 - wet);
    if (p.paper_strength > 0.0) {
        dc *= 1.0 + p.paper_strength * GRAN_SWING * (paper_at(u, v) * 2.0 - 1.0);
    }
    return vec2<f32>(clamp(dc, 0.0, 1.0), front);
}

// ── Kubelka–Munk (crates/color/src/pigment.rs) ──

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

fn same_color(a: vec3<f32>, b: vec3<f32>) -> bool {
    let d = abs(a - b);
    return d.x < 1.0 / 512.0 && d.y < 1.0 / 512.0 && d.z < 1.0 / 512.0;
}

@compute @workgroup_size(8, 8)
fn settle(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.size.x || gid.y >= p.size.y) {
        return;
    }
    let i = gid.y * p.size.x + gid.x;
    if (!is_active(gid.x, gid.y)) {
        return;
    }
    let col = src_col[i];
    if (col.w <= 0.0002) {
        return;
    }
    let u = f32(gid.x) + 0.5;
    let v = f32(gid.y) + 0.5;
    let k = settle_rate(src_sim[i].x, u, v).x;
    var s = clamp(col * k, vec4<f32>(0.0), vec4<f32>(1.0));
    s = vec4<f32>(min(s.xyz, vec3<f32>(s.w)), s.w);
    acc[i] = s + acc[i] * (1.0 - s.w);
}

@compute @workgroup_size(8, 8)
fn update(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.size.x || gid.y >= p.size.y) {
        return;
    }
    let x = i32(gid.x);
    let y = i32(gid.y);
    let i = gid.y * p.size.x + gid.x;
    if (!is_active(gid.x, gid.y)) {
        dst_sim[i] = vec4<f32>(0.0);
        dst_col[i] = vec4<f32>(0.0);
        return;
    }
    let u = f32(x) + 0.5;
    let v = f32(y) + 0.5;
    // Advect along the velocity, adopting the fastest wet neighbour heading here.
    let here = src_sim[i];
    var trace = here.zw;
    var best = dot(trace, trace);
    let dirs = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    for (var d = 0; d < 4; d++) {
        let sn = src_sim[idx(x + dirs[d].x, y + dirs[d].y)];
        let m = dot(sn.zw, sn.zw);
        if (m > best && sn.x > 0.05 && -(sn.z * f32(dirs[d].x) + sn.w * f32(dirs[d].y)) > 0.2) {
            best = m;
            trace = sn.zw;
        }
    }
    let ou = u - trace.x;
    let ov = v - trace.y;
    let sim = sample_sim(ou, ov);
    let col = sample_col(ou, ov);
    let w = sim.x;

    let offs = array<vec2<f32>, 8>(
        vec2<f32>(-1.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, -1.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(-2.0, 0.0), vec2<f32>(2.0, 0.0), vec2<f32>(0.0, -2.0), vec2<f32>(0.0, 2.0),
    );
    var w_avg = 0.0;
    var a_new = col.w;
    var n1: array<vec4<f32>, 4>;
    var c = l2s3(select(vec3<f32>(1.0), col.xyz / col.w, col.w > 0.0005));
    var mass = col.w;
    for (var k = 0; k < 8; k++) {
        let snb = sample_sim(ou + offs[k].x, ov + offs[k].y);
        let wn = snb.x;
        let cn = sample_col(ou + offs[k].x, ov + offs[k].y);
        w_avg += wn;
        if (k < 4) {
            n1[k] = snb;
        }
        let lam = DIFF_P * 0.125 * sstep(0.0, 0.04, min(w, wn)) * (1.0 - p.oil);
        a_new += lam * (cn.w - col.w);
        if (cn.w > 0.0005) {
            let inc = lam * cn.w;
            let nc = l2s3(cn.xyz / cn.w);
            if (mass <= 0.0005) {
                c = nc;
            } else if (inc > 0.0 && !same_color(nc, c)) {
                c = pigment_mix(c, nc, clamp(inc / (mass + inc), 0.0, 1.0));
            }
            mass += inc;
        }
    }
    w_avg *= 0.125;

    let ph = select(0.5, paper_at(u, v), p.paper_strength > 0.0);
    let soak = 1.0 + p.paper_strength * PAPER_ABSORB * ph;
    let resist = 1.0 - p.paper_strength * PAPER_RESIST * ph;
    let w_new = clamp(w + DIFF_W * resist * (1.0 + (0.06 - 1.0) * p.oil) * (w_avg - w) - EVAP * soak * (1.0 - p.oil), 0.0, WATER_MAX);
    var col_new = vec4<f32>(0.0);
    if (a_new > 0.0005) {
        col_new = vec4<f32>(s2l3(c) * a_new, a_new);
    }
    let dc = settle_rate(w, ou, ov).x;
    col_new = clamp(col_new * (1.0 - dc), vec4<f32>(0.0), vec4<f32>(1.0));
    col_new = vec4<f32>(min(col_new.xyz, vec3<f32>(col_new.w)), col_new.w);

    var grad = vec2<f32>(0.5 * (n1[1].x - n1[0].x), 0.5 * (n1[3].x - n1[2].x));
    grad *= sstep(0.02, 0.06, length(grad));
    let curl = 0.5 * ((n1[1].w - n1[0].w) - (n1[3].z - n1[2].z));
    let gw = vec2<f32>(0.5 * (abs(n1[1].y) - abs(n1[0].y)), 0.5 * (abs(n1[3].y) - abs(n1[2].y)));
    let n_hat = gw / (length(gw) + 0.00001);
    let f_vort = vec2<f32>(VORT_EPS * n_hat.y * sim.y, -VORT_EPS * n_hat.x * sim.y);
    let thick = 1.0 + (0.15 - 1.0) * p.oil;
    let fg = finger[i];
    let finger_eff = fg + (1.0 - fg) * p.oil;
    let wet = sstep(0.1, 0.8, w);
    var vel = sim.zw + (p.tilt * G_TILT * thick - grad * G_SLOPE * (1.0 - p.oil)) * (wet * finger_eff) + f_vort * wet;
    let damp = (DAMP + (0.84 - DAMP) * p.oil) * sstep(0.005, 0.04, w_new);
    vel = clamp(vel * damp, vec2<f32>(-V_MAX), vec2<f32>(V_MAX));
    dst_sim[i] = vec4<f32>(w_new, curl, vel);
    dst_col[i] = col_new;
}
