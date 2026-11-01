//! Which faces are drawn, and which way they face (stage 2 plan §3.3):
//! mirrored objects (a negative scale, as glTF files use for the other
//! half of a symmetric model), double-sided materials, and where a
//! material's texture sits on a mesh.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const ALBEDO: TextureKey = TextureKey(1);
const BUMPS: TextureKey = TextureKey(2);
const SIZE: u32 = 96;

/// `width × height` RGBA8 pixels from `f(x, y)`, rows from the top.
fn pixels(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).flat_map(|(x, y)| f(x, y)).collect()
}

/// Seen from +Z, lit from behind the camera: a scene that's its own
/// mirror image across the YZ plane, so mirroring an object in it must
/// mirror the picture of it and change nothing else.
fn scene(draws: Vec<Draw>) -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO);
    camera.clear_color = [0.0, 0.0, 0.2];
    RenderScene {
        joints: Vec::new(),
        camera: Some(camera),
        lights: vec![Light::sun(Vec3::new(0.0, -0.5, -1.0), Vec3::ONE)],
        quality: None,
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        ambient: Vec3::splat(0.1),
        lines: Vec::new(),
        particles: Vec::new(),
        particle_draws: Vec::new(),
        glyphs: Vec::new(),
        sky: None,
        fog: None,
        draws,
    }
}

/// Pixels of `a` that differ from the pixel across the middle column of
/// `b`, and of those, how many lie inside a surface. MSAA's sample
/// pattern isn't symmetric, so pixels on an edge (where a neighbour in `a`
/// differs by more than 16) may differ; inside a surface they must match.
fn mirror_mismatches(a: &Image, b: &Image) -> (usize, usize) {
    let differ = |p: [u8; 4], q: [u8; 4], by: u8| p.iter().zip(q).any(|(&p, q)| p.abs_diff(q) > by);
    let on_edge = |x: u32, y: u32| {
        let around = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)];
        around.iter().any(|&(dx, dy)| {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            (0..SIZE as i32).contains(&nx)
                && (0..SIZE as i32).contains(&ny)
                && differ(a.pixel(x, y), a.pixel(nx as u32, ny as u32), 16)
        })
    };
    let (mut all, mut inside) = (0, 0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            if differ(a.pixel(x, y), b.pixel(SIZE - 1 - x, y), 3) {
                all += 1;
                inside += usize::from(!on_edge(x, y));
            }
        }
    }
    (all, inside)
}

/// A cube turned to show three faces, with a texture and a normal map
/// that both tell left from right. Its mirror image (scale -1 on X) must
/// draw as exactly the mirrored picture: the same faces (not culled, not
/// inside out), lit the same way (normals and tangents mirrored too).
#[test]
fn a_mirrored_object_draws_as_the_mirror_image() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Red and blue halves with a white stripe near the left edge.
    let albedo = pixels(16, 16, |x, _| match x {
        1..=2 => [255, 255, 255, 255],
        0..8 => [220, 60, 40, 255],
        _ => [40, 80, 220, 255],
    });
    r.upload_texture(ALBEDO, ColorSpace::Srgb, 16, 16, &albedo);
    // The normal leans along +u on the left half and up the image on the
    // right, so a wrong tangent or bitangent shows.
    let bumps = pixels(16, 16, |x, _| if x < 8 { [210, 128, 200, 255] } else { [128, 210, 200, 255] });
    r.upload_texture(BUMPS, ColorSpace::Linear, 16, 16, &bumps);
    let material = PbrMaterial {
        roughness: 0.4,
        textures: MaterialTextures { base_color: Some(ALBEDO), normal: Some(BUMPS), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    };
    let turned =
        Mat4::from_quat(Quat::from_euler(glam::EulerRot::YXZ, 0.6, 0.4, 0.0)) * Mat4::from_scale(Vec3::splat(1.4));
    let mirror = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    let draw = |model| Draw {
        skin: None,
        mesh: CUBE,
        model,
        material,
        cast_shadows: true,
        receive_shadows: true,
        own: [0.0; 4],
        submesh: 0,
    };

    let (plain, _) = render(&gpu, &mut r, &scene(vec![draw(turned)]), SIZE, SIZE);
    let (mirrored, _) = render(&gpu, &mut r, &scene(vec![draw(mirror * turned)]), SIZE, SIZE);
    // The picture of the plain cube isn't symmetric itself, or the test
    // would prove nothing.
    let (_, asymmetric) = mirror_mismatches(&plain, &plain);
    assert!(asymmetric > 300, "the plain cube must look different mirrored ({asymmetric} pixels inside surfaces)");
    let (all, inside) = mirror_mismatches(&plain, &mirrored);
    assert!(inside == 0, "{inside} pixels inside surfaces differ from the mirror image ({all} in all)");
}

/// A plane seen from behind: gone when single-sided, as glTF says, and
/// lit as if seen from the front when double-sided, also when mirrored.
#[test]
fn a_double_sided_surface_shows_its_back_lit_like_its_front() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let material = |double_sided| PbrMaterial {
        base_color: [0.6, 0.5, 0.4, 1.0],
        roughness: 0.6,
        double_sided,
        ..PbrMaterial::DEFAULT
    };
    // PLANE faces +Y; turned to face the camera (+Z), or away from it.
    let facing = Mat4::from_rotation_x(90f32.to_radians()) * Mat4::from_scale(Vec3::splat(2.0));
    let away = Mat4::from_rotation_y(180f32.to_radians()) * facing;
    let mirror = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    let centre = |r: &mut Renderer, model, double_sided| {
        let draws = vec![Draw {
            skin: None,
            own: [0.0; 4],
            mesh: PLANE,
            model,
            material: material(double_sided),
            cast_shadows: true,
            receive_shadows: true,
            submesh: 0,
        }];
        render(&gpu, r, &scene(draws), SIZE, SIZE).0.pixel(SIZE / 2, SIZE / 2)
    };

    let front = centre(&mut r, facing, false);
    let clear = centre(&mut r, away, false);
    assert_ne!(front, clear, "the front is drawn");
    assert_eq!(clear[..3], [0, 0, 124], "a single-sided back isn't drawn: the clear colour shows");
    let close = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= 1);
    for (what, model) in
        [("its back", away), ("its back, mirrored", mirror * away), ("its front, mirrored", mirror * facing)]
    {
        let got = centre(&mut r, model, true);
        assert!(close(got, front), "double-sided, {what}: {got:?}, the front shows {front:?}");
    }
    assert!(close(centre(&mut r, facing, true), front), "double-sided changes nothing at the front");
}

/// glTF's texture transform: UVs are scaled (per axis), then offset, so a
/// part of an atlas can fill a mesh.
#[test]
fn uv_scale_and_offset_pick_part_of_a_texture() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let quadrants = [[200, 40, 30, 255], [40, 180, 60, 255], [30, 60, 200, 255], [230, 230, 230, 255]];
    // Quadrants 32 texels wide, so filtering never mixes in a neighbour
    // where the test looks.
    let image = pixels(64, 64, |x, y| quadrants[(y / 32 * 2 + x / 32) as usize]);
    r.upload_texture(ALBEDO, ColorSpace::Srgb, 64, 64, &image);
    // Facing the camera, UV (0, 0) at the top-left of the screen.
    let model = Mat4::from_rotation_x(90f32.to_radians()) * Mat4::from_scale(Vec3::splat(2.0));
    let shows = |r: &mut Renderer, uv_scale, uv_offset| {
        let material = PbrMaterial {
            base_color: [0.0, 0.0, 0.0, 1.0],
            emissive: [1.0; 3],
            uv_scale,
            uv_offset,
            textures: MaterialTextures { emissive: Some(ALBEDO), ..MaterialTextures::NONE },
            ..PbrMaterial::DEFAULT
        };
        // Unlit, so only the emission shows.
        let mut unlit = scene(vec![Draw {
            skin: None,
            mesh: PLANE,
            model,
            material,
            cast_shadows: true,
            receive_shadows: true,
            own: [0.0; 4],
            submesh: 0,
        }]);
        (unlit.lights, unlit.ambient) = (Vec::new(), Vec3::ZERO);
        let (image, _) = render(&gpu, r, &unlit, SIZE, SIZE);
        // Two points across the plane's middle row, left and right.
        [SIZE / 2 - 12, SIZE / 2 + 12].map(|x| image.pixel(x, SIZE / 2 - 12))
    };
    let colour = |q: usize| {
        let [r, g, b, _] = quadrants[q].map(|c| linear(c as f32 / 255.0));
        to_srgb8(tonemap(Vec3::new(r, g, b)))
    };
    let close = |a: [u8; 4], b: [u8; 3]| a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= 2);

    // The top-right quadrant, stretched over the whole plane.
    let [left, right] = shows(&mut r, [0.5, 0.5], [0.5, 0.0]);
    assert!(close(left, colour(1)) && close(right, colour(1)), "{left:?} {right:?}: want {:?}", colour(1));
    // The bottom half, squeezed across: both halves across, the bottom row.
    let [left, right] = shows(&mut r, [1.0, 0.5], [0.0, 0.5]);
    assert!(close(left, colour(2)) && close(right, colour(3)), "{left:?} {right:?}");
}
