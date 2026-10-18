// What a material's shader can use (stage 10), after forward.wgsl: what
// it's given for a point of its surface, and a few helpers. Names that
// start with `fy_` are the engine's own.

// A point of the surface, as a shader's `surface` or `unlit` gets it.
struct SurfaceInput {
    // The texture coordinates, after the material's UV scale and offset.
    uv: vec2<f32>,
    // Where it is in the world.
    position: vec3<f32>,
    // The way it faces, in the world, out of the side that's seen: the
    // mesh's, without a normal map.
    normal: vec3<f32>,
    // The way to the camera, unit length.
    view: vec3<f32>,
    // Where it is on the screen: 0 to 1 across and down, from the top left.
    screen: vec2<f32>,
    // The material's colour (or the object's own), linear, with its alpha.
    color: vec4<f32>,
};

// A vertex, as a shader's `vertex` gets it and gives it back: moved, it's
// drawn where it says in the scene, in shadows and in ambient occlusion.
// The material's `reach` must say how far it can go, in metres, or it may
// be culled while it's still in view.
struct VertexInput {
    // Where it is in the world, after its skin if it has one, and the way
    // it faces, unit length. Move and turn these.
    position: vec3<f32>,
    normal: vec3<f32>,
    // Its texture coordinates, after the material's UV scale and offset.
    uv: vec2<f32>,
    // Where it is in the mesh, before the object's place, scale and skin.
    // Read it (a blade's height, say), or move it to move the vertex in
    // the mesh's own space: turned, scaled and bent with the object, as a
    // cape blown out behind whichever way its wearer faces.
    local: vec3<f32>,
    // Where the object is in the world: each copy of a mesh has its own,
    // so each can sway on its own.
    origin: vec3<f32>,
};

// One light reaching a point of the surface, as a shader's `light` gets
// it: called for each sun and lamp that reaches the point, and what it
// returns is added up, with the ambient and sky light and the glow, which
// stay the engine's. Fog and tone mapping come after, as ever.
struct LightInput {
    // The way to the light, unit length.
    direction: vec3<f32>,
    // The light as it arrives: its colour times its intensity, faded with
    // distance and by a spot's cone; not by its shadow.
    color: vec3<f32>,
    // 1 lit, 0 in the light's shadow, between at a shadow's soft edge.
    shadow: f32,
    // How far the light is, in metres: 0 for a sun.
    distance: f32,
    // The point, in the world; the way it faces (the surface's normal,
    // unit length); and the way to the camera.
    position: vec3<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    // What the shader's `surface` returned.
    surface: Surface,
};

// How the engine lights it: physically based, with a highlight. A
// shader's `light` can start from it.
fn standard_light(in: LightInput) -> vec3<f32> {
    return standard_lit(brdf(in.surface, in.normal, in.view), in.direction, in.color, in.shadow, fy_highlights);
}

// What the shader's `surface` is lit as, and its `standard` reads.
var<private> fy_in: VertexOut;
// Whether the light `standard_light` lights with gives a highlight: a
// lamp at low doesn't.
var<private> fy_highlights: bool;
var<private> fy_front: bool;

// The material's own surface, as the built-in shader has it: its colour,
// metal, roughness and glow, times its textures, and its normal map. A
// shader can start from it and change one thing.
fn standard(in: SurfaceInput) -> Surface {
    return standard_surface(fy_in, fy_front);
}

fn fy_surface_input(in: VertexOut, front_facing: bool) -> SurfaceInput {
    var n = normalize(in.normal);
    if seen_from_behind(in, front_facing) {
        n = -n;
    }
    return SurfaceInput(in.uv, in.world_position, n, to_viewer(in.world_position), in.clip.xy * frame.view_size.xy,
        in.base_color);
}

// The game's time in seconds, as its shaders see it: it stops while the
// game is paused, and goes slower with its time scale.
fn frame_time() -> f32 {
    return frame.time.x;
}

// How much `frame_time()` moved since the last frame.
fn frame_delta() -> f32 {
    return frame.time.y;
}

// What's behind a see-through surface: the solid scene and the sky, as
// drawn before anything see-through. A shader that reads them is drawn
// see-through, and a frame it's in keeps them; other frames pay nothing.
@group(2) @binding(5) var fy_scene_depth: texture_depth_2d;
@group(2) @binding(6) var fy_scene_color: texture_2d<f32>;

// How far along the view `p` is from the camera, in metres: what
// `scene_depth` says of the scene, for a point of the shader's own.
fn view_depth(p: vec3<f32>) -> f32 {
    return dot(p - frame.cells_eye.xyz, frame.cells_forward.xyz);
}

// How far along the view the solid scene is at `screen` (0 to 1 across
// and down, as `SurfaceInput.screen`), in metres: the camera's far end
// where only the sky is.
fn scene_depth(screen: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(fy_scene_depth));
    let pixel = clamp(vec2<i32>(screen * vec2<f32>(size)), vec2<i32>(0), size - 1);
    let depth = textureLoad(fy_scene_depth, pixel, 0);
    let seen = frame.sky.inverse_view_proj * vec4<f32>(screen.x * 2.0 - 1.0, 1.0 - screen.y * 2.0, depth, 1.0);
    return view_depth(seen.xyz / seen.w);
}

// How far behind this point the solid scene is, along the view, in
// metres: 0 where the surface meets it, so shallow water and soft edges.
fn depth_behind(in: SurfaceInput) -> f32 {
    return max(scene_depth(in.screen) - view_depth(in.position), 0.0);
}

// The solid scene's colour at `screen`, smoothly, as light: what the
// surface returns is lit and tone mapped like anything else, so returned
// as it is, it shows the scene unchanged. Move `screen` a little to bend
// what's behind.
fn scene_color(screen: vec2<f32>) -> vec3<f32> {
    return fy_untonemap(textureSampleLevel(fy_scene_color, ao_sampler, screen, 0.0).rgb);
}

// neutral.wgsl's `untonemap`: the scene is kept tone mapped.
fn fy_untonemap(mapped: vec3<f32>) -> vec3<f32> {
    let start = 0.76;
    let desaturation = 0.15;
    var color = max(mapped, vec3<f32>(0.0));
    let top = max(color.r, max(color.g, color.b));
    if top >= start {
        let d = 1.0 - start;
        let new_peak = min(top, 0.9995);
        let peak = d * d / (1.0 - new_peak) - d + start;
        let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
        color = max(color - new_peak * g, vec3<f32>(0.0)) / (1.0 - g) * (peak / new_peak);
    }
    let y = min(color.r, min(color.g, color.b));
    var offset = 0.04;
    if y < 0.04 {
        offset = sqrt(y) / 2.5 - y;
    }
    return color + offset;
}

// `t` at `uv`, smoothly, repeating past 0 and 1.
fn sample(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t, material_sampler, uv);
}

// `t` seen from the three axes and mixed by how `normal` faces them: a
// texture on a surface with no UVs (rock, ground), `scale` repeats a
// metre.
fn triplanar(t: texture_2d<f32>, position: vec3<f32>, normal: vec3<f32>, scale: f32) -> vec4<f32> {
    var w = pow(abs(normal), vec3<f32>(4.0));
    w /= w.x + w.y + w.z;
    let x = textureSample(t, material_sampler, position.zy * scale);
    let y = textureSample(t, material_sampler, position.xz * scale);
    let z = textureSample(t, material_sampler, position.xy * scale);
    return x * w.x + y * w.y + z * w.z;
}

// 0 facing the camera, to 1 at the edges as seen: for rims and
// holograms. A higher `power` keeps it nearer the edges.
fn fresnel(in: SurfaceInput, power: f32) -> f32 {
    return pow(1.0 - saturate(dot(in.normal, in.view)), power);
}
