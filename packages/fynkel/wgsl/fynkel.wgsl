// What the engine gives a Fynkel shader: every name it can use, with what
// it does. Not loaded by anything: a reference to read beside your own
// `.wgsl` files (docs/writing-a-game.md, "Shaders"). The bodies here are
// stand-ins; the engine's are in crates/fynkel_render/src (library.wgsl,
// forward.wgsl, effect.wgsl), and a test keeps the two the same.
//
// A material's shader has one of:
//   fn surface(in: SurfaceInput) -> Surface      lit by the engine
//   fn unlit(in: SurfaceInput) -> vec4<f32>      its colour as it is
// and may have:
//   fn vertex(in: VertexInput) -> VertexInput    moves its vertices
//   fn light(in: LightInput) -> vec3<f32>        lights it, light by light (with `surface`)
// A screen effect's has:
//   fn effect(in: EffectInput) -> vec4<f32>      each pixel of the finished picture
//
// Besides its functions, a shader's file may have:
//   struct Values { … }       numbers and vectors (f32, i32, u32) the game sets by name
//                             (Material.set, ScreenEffect.set), read as `values.name`;
//                             512 bytes at most
//   struct PerObject { … }    up to four f32s each object sets on its own
//                             (MeshRenderer.set), read as `per_object.name`
//   var name: color_texture;  a texture of colours (sRGB), set with Material.set
//   var name: texture_2d<f32>;  a texture of data (linear); four textures at most
// Names starting with `fy_` are the engine's own; don't use them.

// ---------------------------------------------------------------------
// Materials' shaders

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

// What a `surface` returns: the engine lights it.
struct Surface {
    color: vec3<f32>,
    alpha: f32,
    // In the world, out of the side that's seen.
    normal: vec3<f32>,
    metallic: f32,
    roughness: f32,
    // The light it gives off.
    emission: vec3<f32>,
    // How much of the ambient light and the sky's reaches it, 0 to 1.
    occlusion: f32,
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
    // the mesh's own space: turned, scaled and bent with the object.
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

// The material's own surface, as the built-in shader has it: its colour,
// metal, roughness and glow, times its textures, and its normal map. A
// shader can start from it and change one thing.
fn standard(in: SurfaceInput) -> Surface {
    return Surface(in.color.rgb, in.color.a, in.normal, 0.0, 0.5, vec3<f32>(0.0), 1.0);
}

// How the engine lights it: physically based, with a highlight. A `light`
// can return it for some lights and its own for others.
fn standard_light(in: LightInput) -> vec3<f32> {
    return in.surface.color * in.color * max(dot(in.normal, in.direction), 0.0) * in.shadow;
}

// The game's time in seconds, as its shaders see it: it stops while the
// game is paused, and goes slower with its time scale.
fn frame_time() -> f32 {
    return 0.0;
}

// How much `frame_time()` moved since the last frame.
fn frame_delta() -> f32 {
    return 0.0;
}

// `t` at `uv`, smoothly, repeating past 0 and 1.
fn sample(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(0.0);
}

// `t` seen from the three axes and mixed by how `normal` faces them: a
// texture on a surface with no UVs (rock, ground), `scale` repeats a
// metre.
fn triplanar(t: texture_2d<f32>, position: vec3<f32>, normal: vec3<f32>, scale: f32) -> vec4<f32> {
    return vec4<f32>(0.0);
}

// 0 facing the camera, to 1 at the edges as seen: for rims and
// holograms. A higher `power` keeps it nearer the edges.
fn fresnel(in: SurfaceInput, power: f32) -> f32 {
    return 0.0;
}

// What's behind a see-through surface: the solid scene and the sky, as
// drawn before anything see-through. A shader that uses any of these four
// is drawn see-through, and a frame it's in keeps them; other frames pay
// nothing.

// How far along the view `p` is from the camera, in metres: what
// `scene_depth` says of the scene, for a point of the shader's own.
fn view_depth(p: vec3<f32>) -> f32 {
    return 0.0;
}

// How far along the view the solid scene is at `screen` (0 to 1 across
// and down, as `SurfaceInput.screen`), in metres: the camera's far end
// where only the sky is.
fn scene_depth(screen: vec2<f32>) -> f32 {
    return 0.0;
}

// How far behind this point the solid scene is, along the view, in
// metres: 0 where the surface meets it, so shallow water and soft edges.
fn depth_behind(in: SurfaceInput) -> f32 {
    return 0.0;
}

// The solid scene's colour at `screen`, smoothly, as light: returned as
// it is, it shows the scene unchanged. Move `screen` a little to bend
// what's behind.
fn scene_color(screen: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(0.0);
}

// ---------------------------------------------------------------------
// Screen effects' shaders

// A pixel of the picture, as an effect gets it.
struct EffectInput {
    // 0 to 1 across the view, from its top left.
    uv: vec2<f32>,
    // The pixel's centre, in the view's pixels from its top left.
    pixel: vec2<f32>,
    // The view's size, in pixels.
    size: vec2<f32>,
    // Seconds since the world began, as `frame_time()` in a material's.
    time: f32,
};

// The picture at `uv` (0 to 1 from the top left), blended between its
// pixels; outside it, its nearest edge. Tone mapped, linear, with the
// fog and the glow in.
fn picture(uv: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(0.0);
}

// The picture's pixel `at`, in the view's pixels from its top left, as it
// is; outside it, its nearest edge.
fn picture_at(at: vec2<i32>) -> vec4<f32> {
    return vec4<f32>(0.0);
}

// ---------------------------------------------------------------------
// For any shader

// A number from 0 to 1 that looks random, the same for the same `p`.
fn hash(p: vec2<f32>) -> f32 {
    return 0.0;
}

fn hash3(p: vec3<f32>) -> f32 {
    return 0.0;
}

// Smooth noise from 0 to 1: about one bump each 1 of `p`.
fn noise(p: vec2<f32>) -> f32 {
    return 0.0;
}

// `noise` in 3D: for a solid that's the same all through.
fn noise3(p: vec3<f32>) -> f32 {
    return 0.0;
}

// Noise with detail: four of `noise`, each twice as fine and half as
// strong as the last. 0 to 1.
fn fbm(p: vec2<f32>) -> f32 {
    return 0.0;
}

// `x` from the range `a` to `b` moved to `c` to `d`, not clamped.
fn remap(x: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    return 0.0;
}
