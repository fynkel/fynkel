//! The `surface@1` contract (ADR-0031 § 3): the WGSL a shader's body is
//! compiled against.
//!
//! The prelude declares what the body may use:
//!
//! - `SurfaceInput` and `Surface`, and `default_surface()`;
//! - `VertexInput`, for the optional `vertex()`;
//! - `globals` (`time`, `delta_time`, `camera_pos`, `screen_size`);
//! - `material`, a struct of the value properties by name;
//! - per texture property `t_<name>`, `s_<name>` and `sample_<name>(uv)`.
//!
//! `globals` and `material` are private copies the engine's entry points
//! fill before calling `surface` or `vertex` (`fynkel_globals`/
//! `fynkel_load_material`), so the body never depends on how the frame's
//! uniform is laid out, and a `bool` property can be a real `bool`.
//! Names starting with `fynkel_` or `__` are the engine's.
//!
//! Bindings, group 1 (the material): `@binding(0)` the property block,
//! then texture property `i` at `1 + 2i` and its sampler at `2 + 2i`.

use std::fmt::Write;

use crate::layout::Layout;
use crate::{Interface, ValueType};

/// The contract's name and version. A change that could break a body
/// written against it is a new version.
pub const CONTRACT: &str = "surface@1";

/// Top-level names of the engine's own WGSL that a shader's module is
/// compiled with (`fynkel_render`: the frame, the mesh stage, the light
/// and the entry points). A shader's code may not declare them, nor any
/// name starting with `fynkel`/`Fynkel`; `fynkel_render`'s tests hold
/// this list to its WGSL.
pub const ENGINE_NAMES: &[&str] = &[
    "CASCADES",
    "Cascade",
    "CascadeHit",
    "Frame",
    "GRAY_DIM",
    "Grade",
    "LIGHT_POINT",
    "LIGHT_SPOT",
    "Light",
    "LitSurface",
    "MAX_LIGHTS",
    "PI",
    "VsIn",
    "VsOut",
    "VsSkin",
    "brdf_lut",
    "cascade_project",
    "cube_sampler",
    "d_ggx",
    "f_schlick",
    "frame",
    "fullscreen",
    "grayed",
    "instance_lights",
    "local_lights",
    "irradiance_cube",
    "joints",
    "light_surface",
    "punctual",
    "rotate_sky",
    "shade",
    "shadow_map",
    "shadow_pcf",
    "shadow_sampler",
    "specular_cube",
    "srgb_encode",
    "ssao_tex",
    "sun_visibility",
    "v_smith",
    "vs_main",
    "vs_skinned",
];

/// Whether a shader's code may not declare `name` at the top level.
pub fn is_engine_name(name: &str) -> bool {
    ENGINE_NAMES.contains(&name) || name.to_ascii_lowercase().starts_with("fynkel")
}

/// The fixed part of the prelude: the types every body sees.
pub const PRELUDE_TYPES: &str = r#"// surface@1 (ADR-0031): written by the engine above the shader's body.

// What the surface sees of the pixel.
struct SurfaceInput {
    world_pos: vec3<f32>,
    // Unit length, facing the camera on a double-sided back face.
    world_normal: vec3<f32>,
    // Unit vector from the surface toward the camera.
    view_dir: vec3<f32>,
    uv0: vec2<f32>,
    uv1: vec2<f32>,
    // The vertex colour.
    color: vec4<f32>,
    // 0..1 across the screen, (0, 0) at the top left.
    screen_uv: vec2<f32>,
    front_facing: bool,
}

// What the surface gives the engine's light (Unity's fragment stack).
struct Surface {
    // Linear RGB.
    base_color: vec3<f32>,
    alpha: f32,
    metallic: f32,
    roughness: f32,
    // Tangent space: (0, 0, 1) is the mesh's own normal.
    normal: vec3<f32>,
    // Linear RGB relative to white; above 1 blooms.
    emission: vec3<f32>,
    occlusion: f32,
}

fn default_surface() -> Surface {
    return Surface(vec3<f32>(1.0), 1.0, 0.0, 0.5, vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0), 1.0);
}

// A vertex in object space, before the engine places it; `vertex()` may
// move `position` and turn `normal`.
struct VertexInput {
    position: vec3<f32>,
    normal: vec3<f32>,
    tangent: vec4<f32>,
    uv0: vec2<f32>,
    uv1: vec2<f32>,
    color: vec4<f32>,
}

struct Globals {
    // Seconds of the world's clock; stops while Play is paused.
    time: f32,
    delta_time: f32,
    camera_pos: vec3<f32>,
    // Pixels.
    screen_size: vec2<f32>,
}

var<private> globals: Globals;
"#;

/// The whole prelude for one shader: [`PRELUDE_TYPES`], its `material`
/// struct and loader, and its textures.
pub fn prelude(interface: &Interface, layout: &Layout) -> String {
    let mut s = String::from(PRELUDE_TYPES);
    s.push_str("\n// The shader's value properties.\nstruct Material {\n");
    if layout.fields.is_empty() {
        s.push_str("    fynkel_none: u32,\n");
    }
    for f in &layout.fields {
        let _ = writeln!(s, "    {}: {},", f.name, f.ty.wgsl());
    }
    s.push_str("}\n\nvar<private> material: Material;\n\n");
    let _ = writeln!(
        s,
        "@group(1) @binding(0) var<uniform> fynkel_material_block: array<vec4<f32>, {}>;\n",
        layout.size / 16
    );
    s.push_str("fn fynkel_load_material() -> Material {\n    var m: Material;\n");
    if layout.fields.is_empty() {
        s.push_str("    m.fynkel_none = 0u;\n");
    }
    for f in &layout.fields {
        let slot = f.offset / 16;
        let lane = ["x", "y", "z", "w"][((f.offset % 16) / 4) as usize];
        let v = format!("fynkel_material_block[{slot}]");
        let expr = match f.ty {
            ValueType::Float => format!("{v}.{lane}"),
            ValueType::Int => format!("bitcast<i32>({v}.{lane})"),
            ValueType::Bool => format!("bitcast<u32>({v}.{lane}) != 0u"),
            ValueType::Vec2 => format!("{v}.{}", if lane == "x" { "xy" } else { "zw" }),
            ValueType::Vec3 | ValueType::Color3 => format!("{v}.xyz"),
            ValueType::Vec4 | ValueType::Color => v,
        };
        let _ = writeln!(s, "    m.{} = {expr};", f.name);
    }
    s.push_str("    return m;\n}\n");
    for (i, p) in interface.textures().enumerate() {
        let (t, sm) = (1 + 2 * i, 2 + 2 * i);
        let n = &p.name;
        let _ = write!(
            s,
            "\n@group(1) @binding({t}) var t_{n}: texture_2d<f32>;\n@group(1) @binding({sm}) var s_{n}: sampler;\n\
             fn sample_{n}(uv: vec2<f32>) -> vec4<f32> {{\n    return textureSample(t_{n}, s_{n}, uv);\n}}\n"
        );
    }
    s.push_str("\n// ---- the shader's body ----\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn the_prelude_unpacks_each_field_from_its_offset() {
        let s = parse(
            None,
            "shader \"A\" {\n properties {\n  a float\n  n int\n  on bool\n  d vec2\n  c color3\n  t texture\n }\n}\n",
        )
        .unwrap();
        let p = prelude(&s.interface, &s.layout);
        assert!(p.contains("m.a = fynkel_material_block[0].x;"), "{p}");
        assert!(p.contains("m.n = bitcast<i32>(fynkel_material_block[0].y);"));
        assert!(p.contains("m.on = bitcast<u32>(fynkel_material_block[0].z) != 0u;"));
        assert!(p.contains("m.d = fynkel_material_block[1].xy;"));
        assert!(p.contains("m.c = fynkel_material_block[2].xyz;"));
        assert!(p.contains("array<vec4<f32>, 3>"));
        assert!(p.contains("@group(1) @binding(1) var t_t: texture_2d<f32>;"));
        assert!(p.contains("@group(1) @binding(2) var s_t: sampler;"));
        assert!(p.contains("fn sample_t(uv: vec2<f32>)"));
    }
}
