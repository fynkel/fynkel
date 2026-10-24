//! Stage 9 phase 2: skinned meshes. A mesh a skeleton bends draws where
//! its joints put it, lit and shadowed as if it had been made there; the
//! shadow and ambient-occlusion passes bend it the same way; it's culled
//! where it's bent, not where it was bound; and a draw whose joints the
//! scene doesn't have is drawn as bound.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 128;
const BAR: MeshKey = MeshKey(10);

/// A post 0.4 m across and 2 m tall (y from -1 to 1), as two boxes: the
/// lower follows joint 0, the upper joint 1, so turning joint 1 bends it
/// at its middle.
fn bar() -> MeshData {
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
        skin: Some((&joints, &weights)),
        ..MeshParts::default()
    })
    .expect("a bar")
}

/// The same bar, unskinned.
fn plain_bar() -> MeshData {
    let skinned = bar();
    let v = skinned.vertices();
    let positions: Vec<[f32; 3]> = v.iter().map(|v| v.position).collect();
    let normals: Vec<[f32; 3]> = v.iter().map(|v| v.normal).collect();
    let tangents: Vec<[f32; 4]> = v.iter().map(|v| v.tangent).collect();
    let uvs: Vec<[f32; 2]> = v.iter().map(|v| v.uv).collect();
    MeshData::new(MeshParts {
        positions: &positions,
        normals: Some(&normals),
        tangents: Some(&tangents),
        uvs: Some(&uvs),
        indices: skinned.indices(),
        skin: None,
        ..MeshParts::default()
    })
    .expect("a bar")
}

const PLAIN_BAR: MeshKey = MeshKey(11);

fn skin_renderer(gpu: &Gpu) -> Renderer {
    let mut r = renderer(gpu, SRGB);
    r.upload_mesh(BAR, &bar());
    r.upload_mesh(PLAIN_BAR, &plain_bar());
    r
}

fn material(grey: f32) -> PbrMaterial {
    let g = linear(grey);
    PbrMaterial { base_color: [g, g, g, 1.0], metallic: 0.0, roughness: 0.6, ..PbrMaterial::DEFAULT }
}

/// A floor, under a sun that casts shadows, seen from in front and above.
fn stage(quality: QualityLevel) -> RenderScene {
    RenderScene {
        joints: Vec::new(),
        camera: Some(CameraView::looking_at(Vec3::new(1.5, 3.0, 6.0), Vec3::new(0.0, 1.0, 0.0))),
        lights: vec![Light::sun(Vec3::new(1.0, -1.5, -0.5), Vec3::ONE).with_shadows(true)],
        quality: Some(quality.quality()),
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        ambient: Vec3::splat(0.15),
        lines: Vec::new(),
        particles: Vec::new(),
        particle_draws: Vec::new(),
        sky: None,
        fog: None,
        draws: vec![Draw::new(PLANE, Mat4::from_scale(Vec3::splat(12.0)), material(0.7))],
    }
}

const LEVELS: [QualityLevel; 3] = [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High];

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

/// The skinned bar with both joints at `joint` draws as the plain bar
/// moved there: lit, shadowed and occluded the same at every level.
#[test]
fn a_skinned_mesh_draws_where_its_joints_put_it() {
    let gpu = gpu();
    let mut r = skin_renderer(&gpu);
    let joint = Mat4::from_rotation_translation(Quat::from_rotation_y(0.6), Vec3::new(0.3, 1.0, -0.2));
    for quality in LEVELS {
        let mut plain = stage(quality);
        plain.draws.push(Draw::new(PLAIN_BAR, joint, material(0.5)));
        let (expected, _) = render(&gpu, &mut r, &plain, SIZE, SIZE);

        let mut skinned = stage(quality);
        skinned.joints = vec![joint, joint];
        // The draw's own matrix is the bind pose's place: the joints alone
        // move a skinned mesh.
        skinned.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, material(0.5)) });
        let (image, stats) = render(&gpu, &mut r, &skinned, SIZE, SIZE);
        assert_eq!(stats.joints, 2);
        assert_eq!(stats.culled, 0);
        let off = differing(&image, &expected, 2);
        assert!(off == 0, "{off} pixels differ at {}", level_name(quality));
    }
}

/// Each draw of a skinned mesh reads its own joints, though they're drawn
/// together.
#[test]
fn copies_of_a_skinned_mesh_each_follow_their_own_joints() {
    let gpu = gpu();
    let mut r = skin_renderer(&gpu);
    let (left, right) =
        (Mat4::from_translation(Vec3::new(-1.5, 1.0, 0.0)), Mat4::from_translation(Vec3::new(1.5, 1.0, 0.0)));
    let mut plain = stage(QualityLevel::High);
    plain.draws.push(Draw::new(PLAIN_BAR, left, material(0.5)));
    plain.draws.push(Draw::new(PLAIN_BAR, right, material(0.5)));
    let (expected, _) = render(&gpu, &mut r, &plain, SIZE, SIZE);

    let mut skinned = stage(QualityLevel::High);
    skinned.joints = vec![left, left, right, right];
    skinned.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, material(0.5)) });
    skinned.draws.push(Draw { skin: Some(2), ..Draw::new(BAR, Mat4::IDENTITY, material(0.5)) });
    let (image, stats) = render(&gpu, &mut r, &skinned, SIZE, SIZE);
    // The floor, then both bars in one call.
    assert_eq!(stats.draw_calls, 2);
    assert_eq!(differing(&image, &expected, 2), 0);
}

/// A bar its joints move out of view is culled; one whose draw is out of
/// view but whose joints bring it in isn't.
#[test]
fn a_skinned_mesh_is_culled_where_its_joints_put_it() {
    let gpu = gpu();
    let mut r = skin_renderer(&gpu);
    let away = Mat4::from_translation(Vec3::new(0.0, 0.0, 100.0));
    let mut scene = stage(QualityLevel::Medium);
    scene.joints = vec![away, away];
    scene.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, material(0.5)) });
    let (_, stats) = render(&gpu, &mut r, &scene, SIZE, SIZE);
    assert_eq!(stats.culled, 1);

    let at = Mat4::from_translation(Vec3::Y);
    let mut plain = stage(QualityLevel::Medium);
    plain.draws.push(Draw::new(PLAIN_BAR, at, material(0.5)));
    let (expected, _) = render(&gpu, &mut r, &plain, SIZE, SIZE);
    let mut back = stage(QualityLevel::Medium);
    back.joints = vec![at, at];
    back.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, away, material(0.5)) });
    let (image, stats) = render(&gpu, &mut r, &back, SIZE, SIZE);
    assert_eq!(stats.culled, 0);
    assert_eq!(differing(&image, &expected, 2), 0);
}

/// A draw that names joints past the scene's draws as bound, by its own
/// matrix.
#[test]
fn a_skin_the_scene_doesnt_have_is_drawn_as_bound() {
    let gpu = gpu();
    let mut r = skin_renderer(&gpu);
    let at = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
    let mut plain = stage(QualityLevel::Medium);
    plain.draws.push(Draw::new(PLAIN_BAR, at, material(0.5)));
    let (expected, _) = render(&gpu, &mut r, &plain, SIZE, SIZE);

    let mut skinned = stage(QualityLevel::Medium);
    skinned.joints = vec![Mat4::IDENTITY];
    skinned.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, at, material(0.5)) });
    let (image, stats) = render(&gpu, &mut r, &skinned, SIZE, SIZE);
    assert_eq!(stats.draws, 2);
    assert_eq!(differing(&image, &expected, 2), 0);
}

/// The bar bent at its middle: the upper joint turned 50° about the
/// middle, so the top leans over and its shadow on the floor bends too.
#[test]
fn picture_a_bent_bar() {
    let gpu = gpu();
    let mut r = skin_renderer(&gpu);
    let base = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
    let bend = base * Mat4::from_rotation_z(-50f32.to_radians());
    for quality in LEVELS {
        let mut scene = stage(quality);
        scene.camera = Some(CameraView::looking_at(Vec3::new(1.0, 2.5, 4.0), Vec3::new(0.5, 1.0, 0.0)));
        // From behind, so the shadow falls towards the camera, bent too.
        scene.lights = vec![Light::sun(Vec3::new(-0.2, -1.2, 1.0), Vec3::ONE).with_shadows(true)];
        scene.joints = vec![base, bend];
        scene.draws.push(Draw { skin: Some(0), ..Draw::new(BAR, Mat4::IDENTITY, material(0.5)) });
        let (image, _) = render(&gpu, &mut r, &scene, 256, 256);
        assert_golden_within(&format!("skin_bent_{}", level_name(quality)), &image, 8);
    }
}
