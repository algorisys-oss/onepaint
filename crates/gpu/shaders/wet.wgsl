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
    gamma: f32,           // 1: colours are sRGB values (8-bit documents, spec 50)
}

fn to_work3(c: vec3<f32>) -> vec3<f32> {
    return select(l2s3(c), c, p.gamma > 0.5);
}

fn from_work3(c: vec3<f32>) -> vec3<f32> {
    return select(s2l3(c), c, p.gamma > 0.5);
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
        dc *= 1.0 + p.paper_strength * GRAN_SWING * (1.0 - paper_at(u, v) * 2.0);
    }
    return vec2<f32>(clamp(dc, 0.0, 1.0), front);
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
    // Dry paper refuses advected water, unless it runs downhill (spec 58).
    let downhill = dot(trace, p.tilt) / max(length(p.tilt), 1e-4);
    let accept = max(sstep(0.3, 1.0, here.x), sstep(0.15, 0.6, downhill));
    trace *= mix(0.05, 1.0, accept);
    let ou = u - trace.x;
    let ov = v - trace.y;
    let sim = sample_sim(ou, ov);
    let col = sample_col(ou, ov);
    let w = sim.x;

    let offs = array<vec2<f32>, 8>(
        vec2<f32>(-1.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, -1.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(-2.0, 0.0), vec2<f32>(2.0, 0.0), vec2<f32>(0.0, -2.0), vec2<f32>(0.0, 2.0),
    );
    var w_flux = 0.0;
    var a_new = col.w;
    var n1: array<vec4<f32>, 4>;
    var c = to_work3(select(vec3<f32>(1.0), col.xyz / col.w, col.w > 0.0005));
    var mass = col.w;
    for (var k = 0; k < 8; k++) {
        let snb = sample_sim(ou + offs[k].x, ov + offs[k].y);
        let wn = snb.x;
        let cn = sample_col(ou + offs[k].x, ov + offs[k].y);
        w_flux += sstep(0.3, 1.0, min(w, wn)) * (wn - w);
        if (k < 4) {
            n1[k] = snb;
        }
        let lam = DIFF_P * 0.125 * sstep(0.0, 0.04, min(w, wn)) * (1.0 - p.oil) * (1.0 + 0.8 * sstep(0.2, 1.5, min(w, wn)));
        a_new += lam * (cn.w - col.w);
        if (cn.w > 0.0005) {
            let inc = lam * cn.w;
            let nc = to_work3(cn.xyz / cn.w);
            if (mass <= 0.0005) {
                c = nc;
            } else if (inc > 0.0 && !same_color(nc, c)) {
                c = pigment_mix(c, nc, clamp(inc / (mass + inc), 0.0, 1.0));
            }
            mass += inc;
        }
    }

    let ph = select(0.5, paper_at(u, v), p.paper_strength > 0.0);
    let soak = 1.0 + p.paper_strength * PAPER_ABSORB * ph;
    let resist = 1.0 - p.paper_strength * PAPER_RESIST * ph;
    let w_new = clamp(w + DIFF_W * resist * (1.0 + (0.06 - 1.0) * p.oil) * w_flux * 0.125 - EVAP * soak * (1.0 - p.oil), 0.0, WATER_MAX);
    var col_new = vec4<f32>(0.0);
    if (a_new > 0.0005) {
        col_new = vec4<f32>(from_work3(c) * a_new, a_new);
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
    var vel = sim.zw + (p.tilt * G_TILT * thick - grad * G_SLOPE * 4.0 * (1.0 - p.oil)) * (wet * finger_eff) + f_vort * wet;
    let damp = (DAMP + (0.84 - DAMP) * p.oil) * sstep(0.005, 0.04, w_new);
    vel = clamp(vel * damp, vec2<f32>(-V_MAX), vec2<f32>(V_MAX));
    dst_sim[i] = vec4<f32>(w_new, curl, vel);
    dst_col[i] = col_new;
}
