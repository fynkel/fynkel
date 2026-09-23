// The overlay (ADR-0019, docs/renderer.md § The overlay): gizmos and
// handles drawn after the post pass, into the display-referred target at
// its own resolution. Nothing here is exposed, tone mapped, bloomed or
// multisampled: lines are screen-space quads with an analytic edge,
// triangles carry their edges' distances, and the ground grid is computed
// per pixel from the view ray. Each reads the scene's depth to know what
// is behind geometry — a line there keeps `camera.w` of its alpha, the
// grid is hidden — compared as distances from the camera with a small
// tolerance, so a line lying on a surface never z-fights with it.

struct Overlay {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz: camera position; w: the alpha kept by what is behind the scene.
    camera: vec4<f32>,
    // xy: the target's size; zw: the scene depth's size (pixels).
    sizes: vec4<f32>,
    // x: 1 when the target is Unorm (this pass encodes sRGB itself);
    // y: the grid's cell (0: adaptive); z: the grid's fade distance
    // (0: adaptive); w: unused.
    params: vec4<f32>,
    grid_minor: vec4<f32>,
    grid_major: vec4<f32>,
    axis_x: vec4<f32>,
    axis_z: vec4<f32>,
};

@group(0) @binding(0) var<uniform> ov: Overlay;
// `DEPTH_TEXTURE` is replaced with the multisampled type when the scene
// passes are, and `DEPTH_SAMPLES` with their count; `textureLoad`'s last
// argument is then the sample (and at one sample the level, 0).
@group(0) @binding(1) var scene_depth: DEPTH_TEXTURE;

// `OverlayLine::flags` / `OverlayTriangle::flags`: drawn over everything.
const ON_TOP: u32 = 1u;
// The anti-aliased fringe around every primitive (pixels).
const AA: f32 = 1.0;

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn output(c: vec4<f32>) -> vec4<f32> {
    var rgb = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if ov.params.x > 0.5 {
        rgb = srgb_encode(rgb);
    }
    return vec4<f32>(rgb, clamp(c.a, 0.0, 1.0));
}

// Clip space → target pixels (origin top-left, y down) and back.
fn to_screen(c: vec4<f32>) -> vec2<f32> {
    let n = c.xy / c.w;
    return vec2<f32>((n.x * 0.5 + 0.5) * ov.sizes.x, (0.5 - n.y * 0.5) * ov.sizes.y);
}

fn from_screen(s: vec2<f32>, c: vec4<f32>) -> vec4<f32> {
    let n = vec2<f32>(s.x / ov.sizes.x * 2.0 - 1.0, 1.0 - s.y / ov.sizes.y * 2.0);
    return vec4<f32>(n * c.w, c.z, c.w);
}

fn unproject(ndc: vec2<f32>, depth: f32) -> vec3<f32> {
    let h = ov.inv_view_proj * vec4<f32>(ndc, depth, 1.0);
    return h.xyz / h.w;
}

// Whether `world`, seen at target pixel `px`, lies behind the scene's
// nearest surface there: distances from the camera, with a tolerance of
// 0.1 % + 2 mm — the reconstruction's own error grows with distance, and
// a line drawn on a surface must stay in front of it. Of a multisampled
// pixel's samples the farthest counts, so a sloped surface never hides
// what lies on it.
fn behind_scene(px: vec2<f32>, world: vec3<f32>) -> bool {
    let size = vec2<i32>(ov.sizes.zw);
    let q = clamp(vec2<i32>(px * ov.sizes.zw / ov.sizes.xy), vec2<i32>(0), size - vec2<i32>(1));
    var d = 0.0;
    for (var i = 0; i < DEPTH_SAMPLES; i++) {
        d = max(d, textureLoad(scene_depth, q, i));
    }
    if d >= 1.0 {
        return false;
    }
    let uv = (vec2<f32>(q) + 0.5) / ov.sizes.zw;
    let surface = unproject(vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0), d);
    let cam = ov.camera.xyz;
    return length(world - cam) > length(surface - cam) * 1.001 + 0.002;
}

// A vertex no triangle can cover (every corner of a dropped primitive).
const NOWHERE: vec4<f32> = vec4<f32>(2.0, 2.0, 2.0, 1.0);

// ------------------------------------------------------------------ lines

struct LineIn {
    @location(0) a: vec3<f32>,
    @location(1) width: f32,
    @location(2) b: vec3<f32>,
    @location(3) flags: u32,
    @location(4) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Signed distance from the line's centre, across it (pixels).
    @location(1) across: f32,
    @location(2) half_width: f32,
    @location(3) world: vec3<f32>,
    @location(4) @interpolate(flat) flags: u32,
};

// Six vertices a segment: two triangles over the quad from `a` to `b`,
// `half_width + AA` either side and `AA` past either end.
@vertex
fn vs_line(@builtin(vertex_index) vid: u32, in: LineIn) -> LineOut {
    var ts = array<f32, 6>(0.0, 1.0, 0.0, 0.0, 1.0, 1.0);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let t = ts[vid];
    let side = sides[vid];
    var out: LineOut;
    out.flags = in.flags;
    var wa = in.a;
    var wb = in.b;
    var ca = ov.view_proj * vec4<f32>(wa, 1.0);
    var cb = ov.view_proj * vec4<f32>(wb, 1.0);
    // Clipped to the near plane (z ≥ 0 in clip space); wholly behind it,
    // the segment is dropped.
    if ca.z < 0.0 && cb.z < 0.0 {
        out.clip = NOWHERE;
        return out;
    }
    if ca.z < 0.0 {
        let k = ca.z / (ca.z - cb.z);
        ca = mix(ca, cb, k);
        wa = mix(wa, wb, k);
    } else if cb.z < 0.0 {
        let k = cb.z / (cb.z - ca.z);
        cb = mix(cb, ca, k);
        wb = mix(wb, wa, k);
    }
    let sa = to_screen(ca);
    let sb = to_screen(cb);
    let delta = sb - sa;
    let len = length(delta);
    let dir = select(vec2<f32>(1.0, 0.0), delta / max(len, 1e-6), len > 1e-4);
    let normal = vec2<f32>(-dir.y, dir.x);
    // A line thinner than a pixel is drawn one pixel wide and fainter.
    let width = max(in.width, 1.0);
    let half = width * 0.5 + AA;
    let end = t > 0.5;
    let s = select(sa, sb, end) + normal * side * half + dir * (t * 2.0 - 1.0) * AA;
    out.clip = from_screen(s, select(ca, cb, end));
    out.across = side * half;
    out.half_width = width * 0.5;
    out.world = select(wa, wb, end);
    out.color = vec4<f32>(in.color.rgb, in.color.a * clamp(in.width, 0.0, 1.0));
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    let coverage = clamp(in.half_width + 0.5 - abs(in.across), 0.0, 1.0);
    var a = in.color.a * coverage;
    if (in.flags & ON_TOP) == 0u && behind_scene(in.clip.xy, in.world) {
        a *= ov.camera.w;
    }
    if a <= 0.0 {
        discard;
    }
    return output(vec4<f32>(in.color.rgb, a));
}

// -------------------------------------------------------------- triangles

struct TriangleIn {
    @location(0) a: vec3<f32>,
    @location(1) flags: u32,
    @location(2) b: vec3<f32>,
    @location(3) c: vec3<f32>,
    @location(4) color: vec4<f32>,
};

struct TriangleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(flat) flags: u32,
    // The triangle's corners on screen (pixels) and its winding (±1): the
    // fragment measures its distance to each edge.
    @location(3) @interpolate(flat) s01: vec4<f32>,
    @location(4) @interpolate(flat) s2: vec3<f32>,
};

fn cross2(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

// Three vertices a triangle, each pushed `2 AA` away from the centroid so
// the fringe has room; a triangle reaching behind the near plane is
// dropped (handles are in front of the camera).
@vertex
fn vs_triangle(@builtin(vertex_index) vid: u32, in: TriangleIn) -> TriangleOut {
    var out: TriangleOut;
    out.flags = in.flags;
    out.color = in.color;
    let c0 = ov.view_proj * vec4<f32>(in.a, 1.0);
    let c1 = ov.view_proj * vec4<f32>(in.b, 1.0);
    let c2 = ov.view_proj * vec4<f32>(in.c, 1.0);
    if c0.z < 0.0 || c1.z < 0.0 || c2.z < 0.0 {
        out.clip = NOWHERE;
        return out;
    }
    let s0 = to_screen(c0);
    let s1 = to_screen(c1);
    let s2 = to_screen(c2);
    let area = cross2(s1 - s0, s2 - s0);
    if abs(area) < 1e-3 {
        out.clip = NOWHERE;
        return out;
    }
    out.s01 = vec4<f32>(s0, s1);
    out.s2 = vec3<f32>(s2, sign(area));
    let g = (s0 + s1 + s2) / 3.0;
    var s = s0;
    var c = c0;
    var w = in.a;
    if vid == 1u {
        s = s1;
        c = c1;
        w = in.b;
    } else if vid == 2u {
        s = s2;
        c = c2;
        w = in.c;
    }
    let away = s - g;
    let push = select(vec2<f32>(0.0), away / max(length(away), 1e-6), length(away) > 1e-4);
    out.clip = from_screen(s + push * 2.0 * AA, c);
    out.world = w;
    return out;
}

// Signed distance from `x` to the edge `p → q`, positive inside.
fn edge(p: vec2<f32>, q: vec2<f32>, x: vec2<f32>, winding: f32) -> f32 {
    let e = q - p;
    return winding * cross2(e, x - p) / max(length(e), 1e-6);
}

@fragment
fn fs_triangle(in: TriangleOut) -> @location(0) vec4<f32> {
    let s0 = in.s01.xy;
    let s1 = in.s01.zw;
    let s2 = in.s2.xy;
    let x = in.clip.xy;
    let inside = min(min(edge(s0, s1, x, in.s2.z), edge(s1, s2, x, in.s2.z)), edge(s2, s0, x, in.s2.z));
    var a = in.color.a * clamp(inside + 0.5, 0.0, 1.0);
    if (in.flags & ON_TOP) == 0u && behind_scene(in.clip.xy, in.world) {
        a *= ov.camera.w;
    }
    if a <= 0.0 {
        discard;
    }
    return output(vec4<f32>(in.color.rgb, a));
}

// ------------------------------------------------------------ ground grid

struct GridOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_grid(@builtin(vertex_index) id: u32) -> GridOut {
    let x = f32(i32(id & 1u) * 4 - 1);
    let y = f32(i32(id >> 1u) * 4 - 1);
    var out: GridOut;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

// How much of the pixel a one-pixel line of the grid at `cell` covers,
// given the plane's footprint `fw` (metres per pixel along x and z); a
// cell narrower than a few pixels fades away rather than turning to moiré.
fn grid_level(p: vec2<f32>, fw: vec2<f32>, cell: f32) -> f32 {
    let w = max(fw / cell, vec2<f32>(1e-6));
    let g = abs(fract(p / cell - 0.5) - 0.5) / w;
    let line = clamp(1.0 - min(g.x, g.y), 0.0, 1.0);
    return line * (1.0 - smoothstep(0.12, 0.3, max(w.x, w.y)));
}

// `dst` with `color` (straight alpha `a`) laid over it, premultiplied.
fn over(dst: vec4<f32>, color: vec3<f32>, a: f32) -> vec4<f32> {
    return vec4<f32>(color * a + dst.rgb * (1.0 - a), a + dst.a * (1.0 - a));
}

fn log10(x: f32) -> f32 {
    return log2(x) * 0.30102999566;
}

// The plane y = 0 under each pixel. Minor lines at the cell, major ones
// every ten; the cell steps by ten as the camera rises (perspective) or
// the view widens (orthographic, and far away in perspective), the finer
// level fading out and the coarser one taking the major colour on the
// way, so no step is ever visible. The X axis (z = 0) and the Z axis
// (x = 0) in their colours. Derivatives are taken before any branch.
@fragment
fn fs_grid(in: GridOut) -> @location(0) vec4<f32> {
    let px = in.clip.xy;
    let ndc = vec2<f32>(px.x / ov.sizes.x * 2.0 - 1.0, 1.0 - px.y / ov.sizes.y * 2.0);
    let near = unproject(ndc, 0.0);
    let far = unproject(ndc, 1.0);
    let dy = far.y - near.y;
    let t = select(-1.0, -near.y / dy, abs(dy) > 1e-9);
    let p = near + (far - near) * clamp(t, 0.0, 1.0);
    let fw = max(fwidth(p.xz), vec2<f32>(1e-6));

    let cam = ov.camera.xyz;
    let orthographic = abs(ov.view_proj[0][3]) + abs(ov.view_proj[1][3]) + abs(ov.view_proj[2][3]) < 1e-6;
    // Cells at least ten pixels wide where the pixel lands; in perspective
    // also a tenth of the camera's height above the plane.
    var lod = log10(max(fw.x, fw.y) * 10.0);
    if !orthographic {
        lod = max(lod, log10(max(abs(cam.y), 0.05) * 0.1));
    }
    var cell = pow(10.0, floor(lod));
    var f = fract(lod);
    if ov.params.y > 0.0 {
        cell = ov.params.y;
        f = 0.0;
    }

    var c = vec4<f32>(0.0);
    c = over(c, ov.grid_minor.rgb, ov.grid_minor.a * (1.0 - f) * grid_level(p.xz, fw, cell));
    let major = mix(ov.grid_major, ov.grid_minor, f);
    c = over(c, major.rgb, major.a * grid_level(p.xz, fw, cell * 10.0));
    c = over(c, ov.grid_major.rgb, ov.grid_major.a * f * grid_level(p.xz, fw, cell * 100.0));
    // The axes, a pixel and a half wide.
    let ax = clamp(1.25 - abs(p.z) / fw.y, 0.0, 1.0);
    let az = clamp(1.25 - abs(p.x) / fw.x, 0.0, 1.0);
    c = over(c, ov.axis_x.rgb, ov.axis_x.a * ax);
    c = over(c, ov.axis_z.rgb, ov.axis_z.a * az);

    // Far away the plane thins out: in perspective by distance from the
    // camera (forty times its height, at least forty metres), and at a
    // grazing angle whatever the projection.
    var fade = 1.0;
    if !orthographic {
        let reach = select(max(abs(cam.y) * 40.0, 40.0), ov.params.z, ov.params.z > 0.0);
        fade = 1.0 - smoothstep(reach * 0.5, reach, length(p.xz - cam.xz));
    }
    let ray = normalize(far - near);
    fade *= smoothstep(0.0, 0.08, abs(ray.y));
    let a = c.a * fade;

    if t < 0.0 || t > 1.0 || a <= 0.0 || behind_scene(px, p) {
        discard;
    }
    return output(vec4<f32>(c.rgb / max(c.a, 1e-6), a));
}
