// Spec 39: one level of the composite pyramid or of the tile atlases, as
// the 2 × 2 box average of the level below (premultiplied linear light, so
// the average is the display filter). One invocation per destination pixel;
// a job is one tile region (2D) or one atlas slot (array).

struct Params {
    // Destination level size (2D) and the tile edge at this level.
    dst_size: vec2<u32>,
    tile: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> prm: Params;
@group(0) @binding(1) var<storage, read> jobs: array<vec2<u32>>;
@group(0) @binding(2) var src2d: texture_2d<f32>;
@group(0) @binding(3) var dst2d: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var src_arr: texture_2d_array<f32>;
@group(0) @binding(5) var dst_arr: texture_storage_2d_array<rgba16float, write>;

// 2D: a job is a tile key; its region at this level starts at key * tile.
@compute @workgroup_size(8, 8, 1)
fn mip_2d(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= prm.tile || gid.y >= prm.tile) {
        return;
    }
    let p = jobs[gid.z] * prm.tile + gid.xy;
    if (p.x >= prm.dst_size.x || p.y >= prm.dst_size.y) {
        return;
    }
    let src = vec2<i32>(textureDimensions(src2d)) - vec2<i32>(1);
    let q = vec2<i32>(p * 2u);
    let s = textureLoad(src2d, min(q, src), 0)
        + textureLoad(src2d, min(q + vec2<i32>(1, 0), src), 0)
        + textureLoad(src2d, min(q + vec2<i32>(0, 1), src), 0)
        + textureLoad(src2d, min(q + vec2<i32>(1, 1), src), 0);
    textureStore(dst2d, vec2<i32>(p), s * 0.25);
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
