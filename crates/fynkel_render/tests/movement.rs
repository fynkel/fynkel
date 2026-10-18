//! Stage 10 phase 2: shaders that move their vertices. A mesh a shader's
//! `vertex` moves draws as the same mesh placed there would, with its
//! shadow and ambient occlusion, skinned or not, cut out or not; its
//! `reach` keeps it from being culled while it's in view; and copies of a
//! moving mesh are still one draw.

mod common;

use std::sync::Arc;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 160;
const LEVELS: [QualityLevel; 3] = [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High];
const BAR: MeshKey = MeshKey(10);
const PLAIN_BAR: MeshKey = MeshKey(11);

fn level_name(quality: QualityLevel) -> &'static str {
    match quality {
        QualityLevel::Low => "low",
        QualityLevel::Medium => "medium",
        QualityLevel::High => "high",
    }
}

/// How many pixels of `a` and `b` differ by more than `by` in a channel.
fn differing(a: &Image, b: &Image, by: u8) -> usize {
    a.rgba.chunks(4).zip(b.rgba.chunks(4)).filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > by)).count()
}

/// A post 0.4 m across and 2 m tall, as two boxes, each following a joint
/// of its own (skin.rs's).
fn bar(skinned: bool) -> MeshData {
    let cube = MeshData::cube();
    let (mut positions, mut normals, mut tangents, mut uvs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut indices, mut joints, mut weights) = (Vec::new(), Vec::new(), Vec::new());
    for (joint, lift) in [(0u16, -0.5), (1, 0.5)] {
        let first = positions.len() as u32;
        for v in cube.vertices() {
            positions.push([v.position[0] * 0.4, v.position[1] + lift, v.position[2] * 0.4]);
            normals.push(v.normal);
            tangents.push(v.tangent);
            uvs.push(v.uv);
            joints.push([joint, 0, 0, 0]);
            weights.push([1.0, 0.0, 0.0, 0.0]);
        }
        indices.extend(cube.indices().iter().map(|i| i + first));
    }
    MeshData::new(MeshParts {
        positions: &positions,
        normals: Some(&normals),
        tangents: Some(&tangents),
        uvs: Some(&uvs),
        indices: &indices,
        skin: skinned.then_some((&joints[..], &weights[..])),
    })
    .expect("a bar")
}

fn movement_renderer(gpu: &Gpu) -> Renderer {
    let mut r = renderer(gpu, SRGB);
    r.upload_mesh(BAR, &bar(true));
    r.upload_mesh(PLAIN_BAR, &bar(false));
    r
}

fn material(argb: u32, roughness: f32) -> PbrMaterial {
    PbrMaterial { base_color: linear_rgb(argb), metallic: 0.0, roughness, ..PbrMaterial::DEFAULT }
}

/// A floor under a sun that casts shadows, seen from in front and above,
/// with ambient occlusion where the level has it.
fn stage(quality: QualityLevel) -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.8, 2.6, 4.4), Vec3::new(0.0, 0.7, 0.0));
    camera.ambient_occlusion = Some(AmbientOcclusion::DEFAULT);
    RenderScene {
        camera: Some(camera),
        lights: vec![Light::sun(Vec3::new(1.0, -1.2, -0.4), Vec3::ONE).with_shadows(true)],
        quality: Some(quality.quality()),
        ambient: Vec3::splat(0.2),
        draws: vec![Draw::new(PLANE, Mat4::from_scale(Vec3::splat(12.0)), material(0xFFB0B0B0, 0.8))],
        ..RenderScene::default()
    }
}

fn shader(r: &mut Renderer, key: ShaderKey, name: &str, source: &str) -> Arc<CustomShader> {
    let shader = Arc::new(CustomShader::new(name, source).unwrap_or_else(|e| panic!("{e}")));
    r.upload_shader(key, shader.clone());
    shader
}

fn used(shader: ShaderKey, reach: f32) -> Option<ShaderUse> {
    Some(ShaderUse { shader, values: 0, textures: [None; MAX_TEXTURES], reach })
}

/// `values` laid out as `shader`'s `Values`, in the scene's blocks.
fn values(shader: &CustomShader, numbers: &[(&str, &[f32])]) -> Vec<[[f32; 4]; 3]> {
    let mut words = vec![0f32; (shader.values_size as usize).div_ceil(48) * 12];
    for (name, value) in numbers {
        let v = shader.values.iter().find(|v| v.name == *name).unwrap_or_else(|| panic!("no value {name}"));
        let at = v.offset as usize / 4;
        words[at..at + value.len()].copy_from_slice(value);
    }
    words.chunks(12).map(|c| [[c[0], c[1], c[2], c[3]], [c[4], c[5], c[6], c[7]], [c[8], c[9], c[10], c[11]]]).collect()
}

/// Moves every vertex by `values.by`, in the world.
const SHIFT: &str = "struct Values { by: vec3<f32> }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    v.position += values.by;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    return standard(in);
}
";

const SAME: &str = "fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n";

/// A cube and a sphere a shader lifts and pushes back draw as the same
/// shapes placed there: lit, shadowed and occluded the same at every level.
#[test]
fn a_mesh_its_shader_moves_draws_as_if_it_were_placed_there() {
    let gpu = gpu();
    let mut r = movement_renderer(&gpu);
    let shift = shader(&mut r, ShaderKey(1), "shift.wgsl", SHIFT);
    shader(&mut r, ShaderKey(2), "same.wgsl", SAME);
    let by = Vec3::new(0.3, 0.5, -0.4);
    let shapes = |lift: Vec3| {
        [
            (SPHERE, Mat4::from_translation(Vec3::new(-0.9, 0.5, 0.0) + lift), 0xFFD8C8A8),
            (
                CUBE,
                Mat4::from_rotation_translation(Quat::from_rotation_y(0.5), Vec3::new(0.9, 0.5, 0.3) + lift),
                0xFF5C7A3A,
            ),
        ]
    };
    for quality in LEVELS {
        let mut placed = stage(quality);
        for (mesh, model, color) in shapes(by) {
            placed.draws.push(Draw::new(
                mesh,
                model,
                PbrMaterial { shader: used(ShaderKey(2), 0.0), ..material(color, 0.5) },
            ));
        }
        let (expected, _) = render(&gpu, &mut r, &placed, SIZE, SIZE);

        let mut moved = stage(quality);
        moved.shader_values = values(&shift, &[("by", &by.to_array())]);
        for (mesh, model, color) in shapes(Vec3::ZERO) {
            moved.draws.push(Draw::new(
                mesh,
                model,
                PbrMaterial { shader: used(ShaderKey(1), 1.0), ..material(color, 0.5) },
            ));
        }
        let (image, stats) = render(&gpu, &mut r, &moved, SIZE, SIZE);
        assert_eq!((stats.draws, stats.culled), (3, 0));
        let off = differing(&image, &expected, 2);
        assert!(off <= 2, "{off} pixels differ at {}", level_name(quality));

        // Not moved, they'd be elsewhere, and so would their shadows.
        let mut still = moved.clone();
        still.shader_values = values(&shift, &[("by", &[0.0, 0.0, 0.0])]);
        let (image, _) = render(&gpu, &mut r, &still, SIZE, SIZE);
        assert!(differing(&image, &expected, 2) > 500, "at {}", level_name(quality));
    }
}

/// Cut out, a moved mesh's shadow is cut and moved.
#[test]
fn a_cut_out_mesh_its_shader_moves_casts_its_shadow_where_it_is() {
    let gpu = gpu();
    let mut r = movement_renderer(&gpu);
    let source = "struct Values { by: vec3<f32> }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    v.position += values.by;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.alpha = step(0.5, fract(in.uv.x * 4.0));
    return s;
}
";
    let shift = shader(&mut r, ShaderKey(1), "shift_holes.wgsl", source);
    let by = Vec3::new(0.0, 0.6, 0.0);
    let cube = |at: Vec3, by: Vec3| {
        let mut scene = stage(QualityLevel::Medium);
        scene.shader_values = values(&shift, &[("by", &by.to_array())]);
        let m = PbrMaterial { shader: used(ShaderKey(1), 1.0), alpha_cutoff: 0.5, ..material(0xFFD08050, 0.5) };
        scene.draws.push(Draw::new(CUBE, Mat4::from_translation(at), m));
        scene
    };
    let (expected, _) = render(&gpu, &mut r, &cube(Vec3::new(0.0, 0.5, 0.0) + by, Vec3::ZERO), SIZE, SIZE);
    let (image, _) = render(&gpu, &mut r, &cube(Vec3::new(0.0, 0.5, 0.0), by), SIZE, SIZE);
    let off = differing(&image, &expected, 2);
    assert!(off <= 2, "{off} pixels differ");
}

/// A skinned bar a shader moves draws as the bar its joints put there.
#[test]
fn a_skinned_mesh_its_shader_moves_draws_where_it_moves_it() {
    let gpu = gpu();
    let mut r = movement_renderer(&gpu);
    let shift = shader(&mut r, ShaderKey(1), "shift.wgsl", SHIFT);
    shader(&mut r, ShaderKey(2), "same.wgsl", SAME);
    let joint = Mat4::from_rotation_translation(Quat::from_rotation_y(0.6), Vec3::new(0.3, 1.0, -0.2));
    let by = Vec3::new(-0.4, 0.2, 0.3);
    for quality in LEVELS {
        let mut placed = stage(quality);
        placed.joints = vec![Mat4::from_translation(by) * joint; 2];
        let m = PbrMaterial { shader: used(ShaderKey(2), 0.0), ..material(0xFF8080C0, 0.6) };
        placed.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, m) });
        let (expected, _) = render(&gpu, &mut r, &placed, SIZE, SIZE);

        let mut moved = stage(quality);
        moved.joints = vec![joint; 2];
        moved.shader_values = values(&shift, &[("by", &by.to_array())]);
        let m = PbrMaterial { shader: used(ShaderKey(1), 1.0), ..material(0xFF8080C0, 0.6) };
        moved.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, m) });
        let (image, stats) = render(&gpu, &mut r, &moved, SIZE, SIZE);
        assert_eq!(stats.culled, 0);
        let off = differing(&image, &expected, 2);
        assert!(off <= 2, "{off} pixels differ at {}", level_name(quality));
    }
}

/// A cube placed out of view that its shader brings into it is culled
/// without a reach, and drawn with one.
#[test]
fn the_reach_keeps_a_moved_mesh_from_being_culled() {
    let gpu = gpu();
    let mut r = movement_renderer(&gpu);
    let shift = shader(&mut r, ShaderKey(1), "shift.wgsl", SHIFT);
    shader(&mut r, ShaderKey(2), "same.wgsl", SAME);
    let by = Vec3::new(8.0, 0.0, 0.0);
    let away = Vec3::new(-8.0, 0.5, 0.0);
    let scene = |reach: f32| {
        let mut scene = stage(QualityLevel::Medium);
        scene.shader_values = values(&shift, &[("by", &by.to_array())]);
        let m = PbrMaterial { shader: used(ShaderKey(1), reach), ..material(0xFFD08050, 0.5) };
        scene.draws.push(Draw::new(CUBE, Mat4::from_translation(away), m));
        scene
    };
    let mut placed = stage(QualityLevel::Medium);
    let m = PbrMaterial { shader: used(ShaderKey(2), 0.0), ..material(0xFFD08050, 0.5) };
    placed.draws.push(Draw::new(CUBE, Mat4::from_translation(away + by), m));
    let (expected, _) = render(&gpu, &mut r, &placed, SIZE, SIZE);

    let (image, stats) = render(&gpu, &mut r, &scene(0.0), SIZE, SIZE);
    assert_eq!(stats.culled, 1, "without a reach, it's culled where the mesh is");
    assert!(differing(&image, &expected, 2) > 500);

    let (image, stats) = render(&gpu, &mut r, &scene(8.5), SIZE, SIZE);
    assert_eq!(stats.culled, 0);
    let off = differing(&image, &expected, 2);
    assert!(off <= 2, "{off} pixels differ");
}

/// Grass in the wind: each blade bends from its foot by a sway of its
/// own, from where it stands.
const GRASS: &str = "struct Values { strength: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    let up = saturate(in.local.y + 0.5);
    let sway = sin(frame_time() * 2.0 + in.origin.x * 1.7 + in.origin.z * 1.3) * values.strength * up * up;
    v.position.x += sway;
    v.position.z += sway * 0.4;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.color = mix(vec3<f32>(0.08, 0.2, 0.04), s.color, saturate(in.position.y * 1.5));
    return s;
}
";

/// A jelly that wobbles: its surface pushed out and in along the way it
/// faces, as a wave runs up it.
const JELLY: &str = "fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    v.position += in.normal * sin(in.local.y * 9.0 - frame_time() * 5.0) * 0.07;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.roughness = 0.2;
    return s;
}
";

/// A field of blades swaying and a wobbling jelly, at set times: the
/// blades are one draw, and the picture changes with the time.
#[test]
fn grass_and_a_jelly_move_with_the_time() {
    let gpu = gpu();
    let mut r = movement_renderer(&gpu);
    let grass = shader(&mut r, ShaderKey(1), "grass.wgsl", GRASS);
    shader(&mut r, ShaderKey(2), "jelly.wgsl", JELLY);
    let scene = |time: f32| {
        let mut scene = stage(QualityLevel::Medium);
        scene.time = [time, 1.0 / 60.0];
        scene.shader_values = values(&grass, &[("strength", &[0.25])]);
        let blade = PbrMaterial { shader: used(ShaderKey(1), 0.3), double_sided: true, ..material(0xFF6FA040, 0.7) };
        for i in 0..64 {
            let (x, z) = ((i % 8) as f32 * 0.32 - 1.9, (i / 8) as f32 * 0.32 - 1.2);
            let model = Mat4::from_scale_rotation_translation(
                Vec3::new(0.05, 0.7, 0.05),
                Quat::from_rotation_y(i as f32 * 0.7),
                Vec3::new(x, 0.35, z),
            );
            scene.draws.push(Draw::new(CUBE, model, blade));
        }
        let jelly = PbrMaterial { shader: used(ShaderKey(2), 0.1), ..material(0xFFE04070, 0.2) };
        scene.draws.push(Draw::new(
            SPHERE,
            Mat4::from_translation(Vec3::new(1.1, 0.6, 0.2)) * Mat4::from_scale(Vec3::splat(0.55)),
            jelly,
        ));
        scene
    };
    let (image, stats) = render(&gpu, &mut r, &scene(0.7), SIZE, SIZE);
    // The floor, the blades together, and the jelly.
    assert_eq!(stats.draw_calls, 3);
    assert_golden("movement_medium", &image);
    let (later, _) = render(&gpu, &mut r, &scene(1.3), SIZE, SIZE);
    assert!(differing(&image, &later, 4) > 200, "it moves with the time");
}

/// Moves every vertex by `values.by` in the mesh's own space.
const NUDGE: &str = "struct Values { by: vec3<f32> }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    v.local += values.by;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    return standard(in);
}
";

/// Moved in its own space, a turned and stretched cube goes where its
/// matrix takes the move, and a skinned bar where its joints take it.
#[test]
fn a_move_in_the_meshs_own_space_turns_and_scales_with_it() {
    let gpu = gpu();
    let mut r = movement_renderer(&gpu);
    let nudge = shader(&mut r, ShaderKey(1), "nudge.wgsl", NUDGE);
    shader(&mut r, ShaderKey(2), "same.wgsl", SAME);
    let by = Vec3::new(0.0, 0.0, 0.5);
    let model = Mat4::from_scale_rotation_translation(
        Vec3::new(1.4, 0.8, 0.6),
        Quat::from_rotation_y(1.1),
        Vec3::new(-0.6, 0.45, 0.0),
    );
    let joint = Mat4::from_rotation_translation(Quat::from_rotation_y(-0.8), Vec3::new(1.0, 1.0, -0.3));
    for quality in LEVELS {
        let mut placed = stage(quality);
        placed.joints = vec![joint * Mat4::from_translation(by); 2];
        let same = PbrMaterial { shader: used(ShaderKey(2), 0.0), ..material(0xFF80A0C0, 0.5) };
        placed.draws.push(Draw::new(CUBE, model * Mat4::from_translation(by), same));
        placed.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, same) });
        let (expected, _) = render(&gpu, &mut r, &placed, SIZE, SIZE);

        let mut moved = stage(quality);
        moved.joints = vec![joint; 2];
        moved.shader_values = values(&nudge, &[("by", &by.to_array())]);
        let nudged = PbrMaterial { shader: used(ShaderKey(1), 1.0), ..material(0xFF80A0C0, 0.5) };
        moved.draws.push(Draw::new(CUBE, model, nudged));
        moved.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, nudged) });
        let (image, _) = render(&gpu, &mut r, &moved, SIZE, SIZE);
        let off = differing(&image, &expected, 2);
        assert!(off <= 2, "{off} pixels differ at {}", level_name(quality));
    }
}
