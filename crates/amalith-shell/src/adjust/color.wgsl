// The color pass for adjustment layers. One invocation per pixel:
//   out = mix(beneath, blend(beneath, lut(beneath)), opacity), alpha kept.
// Colors are sRGB-encoded with straight alpha, exactly what vello writes
// and reads. Mirrors amalith-adjust's `cpu::apply`, `lut::tetrahedral` and
// `blend::blend`; the parity test checks them against each other.

struct Params {
    size: vec2<u32>,
    // 0: `lut` holds 256 entries, (r, g, b) tables in x/y/z.
    // 1: `lut` holds an n³ cube, r fastest, then g, then b.
    mode: u32,
    n: u32,
    blend: u32,
    opacity: f32,
    _pad: vec2<u32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var<storage, read> lut: array<vec4<f32>>;

fn cube_at(i: vec3<u32>) -> vec3<f32> {
    let n = params.n;
    return lut[(i.z * n + i.y) * n + i.x].xyz;
}

// Tetrahedral interpolation; same case order as `lut::tetrahedral`.
fn sample_cube(c: vec3<f32>) -> vec3<f32> {
    let m = f32(params.n - 1u);
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * m;
    let i0 = min(vec3<u32>(floor(x)), vec3<u32>(params.n - 2u));
    let f = x - vec3<f32>(i0);
    let c000 = cube_at(i0);
    let c111 = cube_at(i0 + vec3<u32>(1u, 1u, 1u));
    var a: vec3<f32>;
    var b: vec3<f32>;
    var w: vec3<f32>;
    if (f.x > f.y) {
        if (f.y > f.z) {
            a = cube_at(i0 + vec3<u32>(1u, 0u, 0u)); b = cube_at(i0 + vec3<u32>(1u, 1u, 0u)); w = vec3<f32>(f.x, f.y, f.z);
        } else if (f.x > f.z) {
            a = cube_at(i0 + vec3<u32>(1u, 0u, 0u)); b = cube_at(i0 + vec3<u32>(1u, 0u, 1u)); w = vec3<f32>(f.x, f.z, f.y);
        } else {
            a = cube_at(i0 + vec3<u32>(0u, 0u, 1u)); b = cube_at(i0 + vec3<u32>(1u, 0u, 1u)); w = vec3<f32>(f.z, f.x, f.y);
        }
    } else if (f.z > f.y) {
        a = cube_at(i0 + vec3<u32>(0u, 0u, 1u)); b = cube_at(i0 + vec3<u32>(0u, 1u, 1u)); w = vec3<f32>(f.z, f.y, f.x);
    } else if (f.z > f.x) {
        a = cube_at(i0 + vec3<u32>(0u, 1u, 0u)); b = cube_at(i0 + vec3<u32>(0u, 1u, 1u)); w = vec3<f32>(f.y, f.z, f.x);
    } else {
        a = cube_at(i0 + vec3<u32>(0u, 1u, 0u)); b = cube_at(i0 + vec3<u32>(1u, 1u, 0u)); w = vec3<f32>(f.y, f.x, f.z);
    }
    return c000 + w.x * (a - c000) + w.y * (b - a) + w.z * (c111 - b);
}

fn lookup(c: vec3<f32>) -> vec3<f32> {
    if (params.mode == 0u) {
        let i = vec3<u32>(round(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0));
        return vec3<f32>(lut[i.x].x, lut[i.y].y, lut[i.z].z);
    }
    return sample_cube(c);
}

// --- Blend modes (W3C Compositing and Blending; same as vello) ----------
// Numbering matches `engine::blend_index`.

fn hard_light(cb: f32, cs: f32) -> f32 {
    if (cs <= 0.5) { return cb * 2.0 * cs; }
    let s = 2.0 * cs - 1.0;
    return cb + s - cb * s;
}

fn separable(mode: u32, cb: f32, cs: f32) -> f32 {
    switch mode {
        case 1u: { return cb * cs; }
        case 2u: { return cb + cs - cb * cs; }
        case 3u: { return hard_light(cs, cb); }
        case 4u: { return min(cb, cs); }
        case 5u: { return max(cb, cs); }
        case 6u: {
            if (cb == 0.0) { return 0.0; }
            if (cs >= 1.0) { return 1.0; }
            return min(1.0, cb / (1.0 - cs));
        }
        case 7u: {
            if (cb >= 1.0) { return 1.0; }
            if (cs <= 0.0) { return 0.0; }
            return 1.0 - min(1.0, (1.0 - cb) / cs);
        }
        case 8u: { return hard_light(cb, cs); }
        case 9u: {
            if (cs <= 0.5) { return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb); }
            var d: f32;
            if (cb <= 0.25) { d = ((16.0 * cb - 12.0) * cb + 4.0) * cb; } else { d = sqrt(cb); }
            return cb + (2.0 * cs - 1.0) * (d - cb);
        }
        case 10u: { return abs(cb - cs); }
        case 11u: { return cb + cs - 2.0 * cb * cs; }
        default: { return cs; }
    }
}

fn lum(c: vec3<f32>) -> f32 {
    return 0.3 * c.x + 0.59 * c.y + 0.11 * c.z;
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(c.x, min(c.y, c.z));
    let x = max(c.x, max(c.y, c.z));
    var out = c;
    if (n < 0.0) { out = l + (out - l) * l / max(l - n, 1e-7); }
    if (x > 1.0) { out = l + (out - l) * (1.0 - l) / max(x - l, 1e-7); }
    return out;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_color(c + (l - lum(c)));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(c.x, max(c.y, c.z)) - min(c.x, min(c.y, c.z));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mx = max(c.x, max(c.y, c.z));
    let mn = min(c.x, min(c.y, c.z));
    if (mx > mn) { return (c - mn) * s / (mx - mn); }
    return vec3<f32>(0.0);
}

fn blend(mode: u32, b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 0u: { return s; }
        case 12u: { return set_lum(set_sat(s, sat(b)), lum(b)); }
        case 13u: { return set_lum(set_sat(b, sat(s)), lum(b)); }
        case 14u: { return set_lum(s, lum(b)); }
        case 15u: { return set_lum(b, lum(s)); }
        default: {
            return vec3<f32>(separable(mode, b.x, s.x), separable(mode, b.y, s.y), separable(mode, b.z, s.z));
        }
    }
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.size.x || id.y >= params.size.y) {
        return;
    }
    let p = vec2<i32>(id.xy);
    let c = textureLoad(src, p, 0);
    if (c.a <= 0.0) {
        textureStore(dst, p, c);
        return;
    }
    let blended = blend(params.blend, c.rgb, lookup(c.rgb));
    let t = clamp(params.opacity, 0.0, 1.0);
    textureStore(dst, p, vec4<f32>(clamp(mix(c.rgb, blended, t), vec3<f32>(0.0), vec3<f32>(1.0)), c.a));
}
