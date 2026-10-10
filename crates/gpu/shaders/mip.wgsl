// Spec 39: one level of the composite pyramid or of the tile atlases, as
// the 2 × 2 box average of the level below (premultiplied linear light, so
// the average is the display filter). One invocation per destination pixel;
// a job is one tile region of the composite sheets or one atlas slot.

struct Params {
    // Sheets: the document size at this level. Array: unused.
    dst_size: vec2<u32>,
    // The tile edge at this level.
    tile: u32,
    // Sheets across (spec 40).
    cols: u32,
    // Sheets: the sheet size at this level, and the document size at the level below.
    sheet: vec2<u32>,
    src_size: vec2<u32>,
}

@group(0) @binding(0) var<uniform> prm: Params;
@group(0) @binding(1) var<storage, read> jobs: array<vec2<u32>>;
@group(0) @binding(4) var src_arr: texture_2d_array<f32>;
@group(0) @binding(5) var dst_arr: texture_storage_2d_array<rgba16float, write>;

// Composite sheets: a job is a tile key; its region at this level starts at
// key * tile in document pixels. Sheets are even at every level, so the 2 × 2
// source block lies in the same sheet.
@compute @workgroup_size(8, 8, 1)
fn mip_sheets(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= prm.tile || gid.y >= prm.tile) {
        return;
    }
    let p = jobs[gid.z] * prm.tile + gid.xy;
    if (p.x >= prm.dst_size.x || p.y >= prm.dst_size.y) {
        return;
    }
    let s = p / prm.sheet;
    let layer = i32(s.y * prm.cols + s.x);
    let base = vec2<i32>(s * prm.sheet * 2u);
    let hi = vec2<i32>(prm.src_size) - vec2<i32>(1);
    let q = vec2<i32>(p * 2u);
    let sum = textureLoad(src_arr, min(q, hi) - base, layer, 0)
        + textureLoad(src_arr, min(q + vec2<i32>(1, 0), hi) - base, layer, 0)
        + textureLoad(src_arr, min(q + vec2<i32>(0, 1), hi) - base, layer, 0)
        + textureLoad(src_arr, min(q + vec2<i32>(1, 1), hi) - base, layer, 0);
    textureStore(dst_arr, vec2<i32>(p - s * prm.sheet), layer, sum * 0.25);
}

// Array: a job is a slot (x); the whole slot at this level.
@compute @workgroup_size(8, 8, 1)
fn mip_array(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= prm.tile || gid.y >= prm.tile) {
        return;
    }
    let layer = i32(jobs[gid.z].x);
    let q = vec2<i32>(gid.xy * 2u);
    let s = textureLoad(src_arr, q, layer, 0)
        + textureLoad(src_arr, q + vec2<i32>(1, 0), layer, 0)
        + textureLoad(src_arr, q + vec2<i32>(0, 1), layer, 0)
        + textureLoad(src_arr, q + vec2<i32>(1, 1), layer, 0);
    textureStore(dst_arr, vec2<i32>(gid.xy), layer, s * 0.25);
}
