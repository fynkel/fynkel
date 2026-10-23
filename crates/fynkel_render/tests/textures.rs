//! Textured materials (stage 2 plan §3.2), checked on the real GPU against
//! the CPU copy of the shading maths.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const A: TextureKey = TextureKey(1);
const B: TextureKey = TextureKey(2);

/// `width × height` RGBA8 pixels from `f(x, y)`, rows from the top.
fn pixels(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).flat_map(|(x, y)| f(x, y)).collect()
}

fn solid(rgba: [u8; 4]) -> Vec<u8> {
    rgba.to_vec()
}

/// A 2 × 2 plane facing a camera 3 units away, whose UV (0, 0) (the
/// image's top-left) is at the top-left of the screen.
fn facing_plane(material: PbrMaterial) -> RenderScene {
    RenderScene {
        joints: Vec::new(),
        camera: Some(CameraView::looking_at(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO)),
        lights: Vec::new(),
        quality: None,
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        ambient: Vec3::ZERO,
        lines: Vec::new(),
        particles: Vec::new(),
        particle_draws: Vec::new(),
        sky: None,
        fog: None,
        draws: vec![Draw {
            skin: None,
            own: [0.0; 4],
            mesh: PLANE,
            model: Mat4::from_rotation_x(90f32.to_radians()) * Mat4::from_scale(Vec3::splat(2.0)),
            material,
            cast_shadows: true,
            receive_shadows: true,
            submesh: 0,
        }],
    }
}

/// Where the plane's point `(x, y)` (each -1 to 1, +y up) is on a 64 × 64
/// image of [`facing_plane`].
fn on_screen(x: f32, y: f32) -> (u32, u32) {
    let half = 3.0 * 30f32.to_radians().tan();
    let px = |v: f32| (32.0 + v / half * 32.0).floor() as u32;
    (px(x), 63 - px(y))
}

fn rgb(p: [u8; 4]) -> [u8; 3] {
    [p[0], p[1], p[2]]
}

fn close(got: [u8; 3], want: [u8; 3], tolerance: u8) -> bool {
    got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= tolerance)
}

/// Linear radiance → what the image shows.
fn shown(radiance: Vec3) -> [u8; 3] {
    to_srgb8(tonemap(radiance))
}

#[test]
fn a_base_colour_texture_shows_its_pixels_the_right_way_up() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Four quadrants of 2 × 2 texels, so the centre of each samples one
    // colour exactly.
    let quadrants = [[200, 40, 30, 255], [40, 180, 60, 255], [30, 60, 200, 255], [230, 230, 230, 255]];
    let image = pixels(4, 4, |x, y| quadrants[(y / 2 * 2 + x / 2) as usize]);
    r.upload_texture(A, ColorSpace::Srgb, 4, 4, &image);
    let material = PbrMaterial {
        roughness: 1.0,
        textures: MaterialTextures { base_color: Some(A), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    };
    let mut scene = facing_plane(material);
    scene.ambient = Vec3::ONE;
    let (image, _) = render(&gpu, &mut r, &scene, 64, 64);

    // (where, texel): top-left of the image at the top-left of the screen.
    let cases = [((-0.5, 0.5), 0), ((0.5, 0.5), 1), ((-0.5, -0.5), 2), ((0.5, -0.5), 3)];
    for ((x, y), texel) in cases {
        let [r8, g8, b8, _] = quadrants[texel];
        let albedo = Vec3::new(linear(r8 as f32 / 255.0), linear(g8 as f32 / 255.0), linear(b8 as f32 / 255.0));
        let point = Vec3::new(x, y, 0.0);
        let v = (Vec3::new(0.0, 0.0, 3.0) - point).normalize();
        let surface = Surface { albedo, metallic: 0.0, roughness: 1.0 };
        let want = shown(shade(&surface, Vec3::Z, v, Vec3::Z, Vec3::ZERO, Vec3::ONE));
        let (px, py) = on_screen(x, y);
        let got = rgb(image.pixel(px, py));
        assert!(close(got, want, 2), "texel {texel} at ({px}, {py}): GPU {got:?}, maths {want:?}");
    }
}

/// One image of 128s (0.502), read by each slot. Colour slots decode it
/// as sRGB (0.216); data slots take it as it is. Each case also checks
/// that reading it the other way would show clearly differently.
#[test]
fn each_slot_reads_its_texture_in_its_colour_space() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let grey = solid([128, 128, 128, 255]);
    r.upload_texture(A, ColorSpace::Srgb, 1, 1, &grey);
    r.upload_texture(A, ColorSpace::Linear, 1, 1, &grey);
    let (as_data, as_colour) = (128.0 / 255.0, linear(128.0 / 255.0));
    let centre = |scene: &RenderScene, r: &mut Renderer| rgb(render(&gpu, r, scene, 64, 64).0.pixel(32, 32));
    let textured = |textures: MaterialTextures, f: &dyn Fn(&mut PbrMaterial)| {
        let mut m = PbrMaterial { textures, ..PbrMaterial::DEFAULT };
        f(&mut m);
        m
    };
    let none = MaterialTextures::NONE;

    // Base colour: sRGB.
    let mut scene = facing_plane(textured(MaterialTextures { base_color: Some(A), ..none }, &|m| m.roughness = 1.0));
    scene.ambient = Vec3::ONE;
    let lit = |albedo: f32, metallic: f32, roughness: f32, sun: f32, ambient: f32| {
        let surface = Surface { albedo: Vec3::splat(albedo), metallic, roughness };
        shown(shade(&surface, Vec3::Z, Vec3::Z, Vec3::Z, Vec3::splat(sun), Vec3::splat(ambient)))
    };
    let (want, wrong) = (lit(as_colour, 0.0, 1.0, 0.0, 1.0), lit(as_data, 0.0, 1.0, 0.0, 1.0));
    check("base colour", centre(&scene, &mut r), want, wrong);

    // Emissive: sRGB. Black, unlit, glowing white through the texture.
    let scene = facing_plane(textured(MaterialTextures { emissive: Some(A), ..none }, &|m| {
        m.base_color = [0.0, 0.0, 0.0, 1.0];
        m.emissive = [1.0; 3];
    }));
    check("emissive", centre(&scene, &mut r), shown(Vec3::splat(as_colour)), shown(Vec3::splat(as_data)));

    // Occlusion: linear, and it darkens the ambient light.
    let mut scene = facing_plane(textured(MaterialTextures { occlusion: Some(A), ..none }, &|m| {
        m.base_color = [0.5, 0.5, 0.5, 1.0];
        m.roughness = 1.0;
    }));
    scene.ambient = Vec3::ONE;
    let ambient_only = |occlusion: f32| {
        let surface = Surface { albedo: Vec3::splat(0.5), metallic: 0.0, roughness: 1.0 };
        shown(shade(&surface, Vec3::Z, Vec3::Z, Vec3::Z, Vec3::ZERO, Vec3::ONE) * occlusion)
    };
    check("occlusion", centre(&scene, &mut r), ambient_only(as_data), ambient_only(as_colour));

    // Metallic-roughness: linear, roughness in G and metallic in B.
    let mut scene = facing_plane(textured(MaterialTextures { metallic_roughness: Some(A), ..none }, &|m| {
        m.base_color = [0.5, 0.5, 0.5, 1.0];
        m.metallic = 1.0;
        m.roughness = 1.0;
    }));
    scene.ambient = Vec3::splat(0.1);
    scene.lights = vec![Light::sun(Vec3::NEG_Z, Vec3::splat(0.3))];
    let (want, wrong) = (lit(0.5, as_data, as_data, 0.3, 0.1), lit(0.5, as_colour, as_colour, 0.3, 0.1));
    check("metallic-roughness", centre(&scene, &mut r), want, wrong);
}

fn check(slot: &str, got: [u8; 3], want: [u8; 3], wrong: [u8; 3]) {
    assert!(!close(want, wrong, 8), "{slot}: the test can't tell the spaces apart: {want:?} vs {wrong:?}");
    assert!(close(got, want, 2), "{slot}: GPU {got:?}, maths {want:?} (the other space would give {wrong:?})");
}

/// glTF's convention: a normal map's red bends the normal along +u (the
/// image's right), green bends it up the image.
#[test]
fn normal_maps_bend_the_normal_the_gltf_way() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let encode = |n: Vec3| {
        let n = n.normalize();
        let c = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        [c(n.x), c(n.y), c(n.z), 255]
    };
    let surface = Surface { albedo: Vec3::splat(0.5), metallic: 0.0, roughness: 1.0 };
    // (tangent-space normal, where the light comes from): the plane's +u is
    // world +X and up the image is world +Y.
    for (mapped, to_light) in [(Vec3::new(0.5, 0.0, 0.8), Vec3::X), (Vec3::new(0.0, 0.5, 0.8), Vec3::Y)] {
        r.upload_texture(A, ColorSpace::Linear, 1, 1, &encode(mapped));
        let mut scene = facing_plane(PbrMaterial {
            base_color: [0.5, 0.5, 0.5, 1.0],
            roughness: 1.0,
            textures: MaterialTextures { normal: Some(A), ..MaterialTextures::NONE },
            ..PbrMaterial::DEFAULT
        });
        let l = (to_light + Vec3::Z).normalize();
        scene.lights = vec![Light::sun(-l, Vec3::ONE)];
        let got = rgb(render(&gpu, &mut r, &scene, 64, 64).0.pixel(32, 32));
        // The texel stores the normal to 8 bits; shade with what it holds.
        let [x, y, z, _] = encode(mapped).map(|c| c as f32 / 255.0 * 2.0 - 1.0);
        let stored = Vec3::new(x, y, z).normalize();
        let want = shown(shade(&surface, stored, Vec3::Z, l, Vec3::ONE, Vec3::ZERO));
        let flat = shown(shade(&surface, Vec3::Z, Vec3::Z, l, Vec3::ONE, Vec3::ZERO));
        let away = shown(shade(&surface, stored * Vec3::new(-1.0, -1.0, 1.0), Vec3::Z, l, Vec3::ONE, Vec3::ZERO));
        assert!(want[0] > flat[0] + 10 && flat[0] > away[0] + 10, "{mapped}: {want:?} {flat:?} {away:?}");
        assert!(
            close(got, want, 2),
            "{mapped}, lit from {to_light}: GPU {got:?}, maths {want:?} (flat {flat:?}, bent away {away:?})"
        );
    }
}

/// A black and white checkerboard far away must look mid-grey, the grey
/// of linear 0.5 (sRGB 188), as the eye averages it. Averaging the stored
/// sRGB values, as mipmaps built naively do, would show 128: much darker.
#[test]
fn a_distant_checkerboard_is_mid_grey_not_darker() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let checker = pixels(64, 64, |x, y| if (x / 8 + y / 8) % 2 == 0 { [255; 4] } else { [0, 0, 0, 255] });
    r.upload_texture(A, ColorSpace::Srgb, 64, 64, &checker);
    r.upload_texture(B, ColorSpace::Srgb, 1, 1, &solid([188, 188, 188, 255]));
    // The ground, glowing its texture: no lighting to account for.
    let ground = |texture| {
        let mut camera = CameraView::looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -10.0));
        camera.clear_color = [0.0, 0.0, 0.3];
        RenderScene {
            joints: Vec::new(),
            camera: Some(camera),
            lights: Vec::new(),
            quality: None,
            time: [0.0; 2],
            shader_values: Vec::new(),
            effects: Vec::new(),
            ambient: Vec3::ZERO,
            lines: Vec::new(),
            particles: Vec::new(),
            particle_draws: Vec::new(),
            sky: None,
            fog: None,
            draws: vec![Draw {
                skin: None,
                own: [0.0; 4],
                mesh: PLANE,
                model: Mat4::from_scale(Vec3::new(400.0, 1.0, 400.0)),
                material: PbrMaterial {
                    base_color: [0.0, 0.0, 0.0, 1.0],
                    emissive: [1.0; 3],
                    uv_scale: [400.0; 2],
                    textures: MaterialTextures { emissive: Some(texture), ..MaterialTextures::NONE },
                    ..PbrMaterial::DEFAULT
                },
                cast_shadows: true,
                receive_shadows: true,
                submesh: 0,
            }],
        }
    };
    let (checkers, _) = render(&gpu, &mut r, &ground(A), 256, 128);
    let (grey, _) = render(&gpu, &mut r, &ground(B), 256, 128);

    // The far ground: the rows just below the horizon (row 52.5).
    let average = |image: &Image| {
        let rows = 55..62;
        let sum: u32 =
            rows.clone().flat_map(|y| (0..256).map(move |x| (x, y))).map(|(x, y)| image.pixel(x, y)[1] as u32).sum();
        sum as f32 / (rows.len() * 256) as f32
    };
    let (far, want) = (average(&checkers), average(&grey));
    assert!((far - want).abs() < 6.0, "the far checkerboard averages {far:.1}, the grey {want:.1}");
    assert_golden("distant_checkerboard", &checkers);
}

#[test]
fn a_cutout_drops_the_surface_below_its_cutoff() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Left half opaque, right half transparent.
    r.upload_texture(A, ColorSpace::Srgb, 4, 4, &pixels(4, 4, |x, _| [255, 255, 255, if x < 2 { 255 } else { 0 }]));
    let material = |alpha_cutoff| PbrMaterial {
        base_color: [0.0, 0.0, 0.0, 1.0],
        emissive: [0.5; 3],
        alpha_cutoff,
        textures: MaterialTextures { base_color: Some(A), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    };
    let (left, right) = (on_screen(-0.5, 0.0), on_screen(0.5, 0.0));

    let (cut, _) = render(&gpu, &mut r, &facing_plane(material(0.5)), 64, 64);
    let clear = cut.pixel(0, 0);
    assert_ne!(cut.pixel(left.0, left.1), clear, "the opaque half is drawn");
    assert_eq!(cut.pixel(right.0, right.1), clear, "the transparent half is cut away");
    assert!(cut.rgba.chunks(4).all(|p| p[3] == 255), "the target stays opaque");

    let (opaque, _) = render(&gpu, &mut r, &facing_plane(material(0.0)), 64, 64);
    assert_eq!(opaque.pixel(right.0, right.1), opaque.pixel(left.0, left.1), "without a cutoff alpha is ignored");
}

#[test]
fn uv_scale_repeats_the_texture() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Left half red, right half green.
    r.upload_texture(
        A,
        ColorSpace::Srgb,
        4,
        4,
        &pixels(4, 4, |x, _| if x < 2 { [255, 0, 0, 255] } else { [0, 255, 0, 255] }),
    );
    let scene = facing_plane(PbrMaterial {
        base_color: [0.0, 0.0, 0.0, 1.0],
        emissive: [1.0; 3],
        uv_scale: [2.0; 2],
        textures: MaterialTextures { emissive: Some(A), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    });
    let (image, _) = render(&gpu, &mut r, &scene, 64, 64);
    let red_or_green = |x: f32| {
        let (px, py) = on_screen(x, 0.0);
        let [r, g, ..] = image.pixel(px, py);
        if r > 150 && g < 50 {
            "red"
        } else if g > 150 && r < 50 {
            "green"
        } else {
            "neither"
        }
    };
    assert_eq!([-0.75, -0.25, 0.25, 0.75].map(red_or_green), ["red", "green", "red", "green"]);
}

#[test]
fn a_texture_can_be_replaced_and_removed() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Half intensity, below where tonemapping starts to compress (and mix
    // in white), so a pure colour stays pure.
    let scene = facing_plane(PbrMaterial {
        base_color: [0.0, 0.0, 0.0, 1.0],
        emissive: [0.5; 3],
        textures: MaterialTextures { emissive: Some(A), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    });
    let centre = |r: &mut Renderer| rgb(render(&gpu, r, &scene, 64, 64).0.pixel(32, 32));
    let white = centre(&mut r);
    assert!(white.iter().all(|&c| c > 150), "before it's uploaded, a slot changes nothing: {white:?}");

    r.upload_texture(A, ColorSpace::Srgb, 1, 1, &solid([255, 0, 0, 255]));
    assert!(matches!(centre(&mut r), [r, 0, 0] if r > 150), "red");
    r.upload_texture(A, ColorSpace::Srgb, 1, 1, &solid([0, 0, 255, 255]));
    assert!(matches!(centre(&mut r), [0, 0, b] if b > 150), "replaced by blue");
    assert!(r.remove_texture(A));
    assert!(!r.remove_texture(A));
    assert_eq!(centre(&mut r), white, "removed: back to nothing");
    assert_eq!(r.texture_bytes(), 0);
}

/// Every mip level, as stored: 4 bytes a pixel, each level half the one
/// above (rounded down, at least 1).
#[test]
fn texture_memory_counts_every_mip_level() {
    let gpu = gpu();
    let mut r = Renderer::new(&gpu, SRGB);
    let expected = |w: u64, h: u64| {
        let levels = 64 - w.max(h).leading_zeros() as u64;
        (0..levels).map(|i| (w >> i).max(1) * (h >> i).max(1) * 4).sum::<u64>()
    };
    r.upload_texture(A, ColorSpace::Srgb, 256, 256, &vec![0; 256 * 256 * 4]);
    assert_eq!(r.texture_bytes(), expected(256, 256));
    assert_eq!(expected(256, 256), 349_524, "4/3 of the image, give or take");
    // Not a power of two, and used both ways: two textures.
    r.upload_texture(B, ColorSpace::Srgb, 300, 100, &vec![0; 300 * 100 * 4]);
    r.upload_texture(B, ColorSpace::Linear, 300, 100, &vec![0; 300 * 100 * 4]);
    assert_eq!(r.texture_bytes(), expected(256, 256) + 2 * expected(300, 100));
    r.remove_texture(B);
    assert_eq!(r.texture_bytes(), expected(256, 256));
}

/// Draws are instanced together when they share a mesh, a set of textures
/// and opaqueness.
#[test]
fn draws_are_batched_by_textures_too() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    r.upload_texture(A, ColorSpace::Srgb, 1, 1, &solid([255, 0, 0, 255]));
    r.upload_texture(B, ColorSpace::Srgb, 1, 1, &solid([0, 255, 0, 255]));
    let with = |key: Option<TextureKey>, alpha_cutoff: f32| PbrMaterial {
        alpha_cutoff,
        textures: MaterialTextures { base_color: key, ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    };
    let mut scene = stage1_scene();
    let at = |x: f32| Mat4::from_translation(Vec3::new(x, 0.0, 0.0)) * Mat4::from_scale(Vec3::splat(0.4));
    scene.draws = [(Some(A), 0.0), (Some(B), 0.0), (Some(A), 0.0), (None, 0.0), (Some(A), 0.5)]
        .into_iter()
        .enumerate()
        .map(|(i, (key, cutoff))| Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: at(i as f32 - 2.0),
            material: with(key, cutoff),
            cast_shadows: true,
            receive_shadows: true,
            submesh: 0,
        })
        .collect();
    let (_, stats) = render(&gpu, &mut r, &scene, 64, 64);
    // A ×2, B, none, and A cut out.
    assert_eq!(
        stats,
        FrameStats { draws: 5, draw_calls: 4, missing_meshes: 0, culled: 0, lines: 0, ..Default::default() }
    );
}

/// The look, for a person to check: a UV grid on a cube, a bumpy
/// normal-mapped sphere, a leaf-like cut-out and a glowing panel.
#[test]
fn textured_materials_match_their_golden() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    const GRID: TextureKey = TextureKey(10);
    const BUMPS: TextureKey = TextureKey(11);
    const LEAF: TextureKey = TextureKey(12);
    const GLOW: TextureKey = TextureKey(13);
    // UV grid: red grows with u, green with v, white lines every 8 texels.
    r.upload_texture(
        GRID,
        ColorSpace::Srgb,
        64,
        64,
        &pixels(64, 64, |x, y| {
            if x % 16 == 0 || y % 16 == 0 { [240, 240, 240, 255] } else { [(x * 4) as u8, (y * 4) as u8, 90, 255] }
        }),
    );
    // Bumps: a grid of domes in tangent space.
    r.upload_texture(
        BUMPS,
        ColorSpace::Linear,
        128,
        64,
        &pixels(128, 64, |x, y| {
            let (u, v) = ((x % 16) as f32 / 8.0 - 0.94, (y % 16) as f32 / 8.0 - 0.94);
            // Up the image is +y, so a dome's upper half leans up.
            let n = Vec3::new(u * 0.8, -v * 0.8, 1.0).normalize();
            let c = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
            [c(n.x), c(n.y), c(n.z), 255]
        }),
    );
    // A leaf: an ellipse, opaque inside.
    r.upload_texture(
        LEAF,
        ColorSpace::Srgb,
        64,
        64,
        &pixels(64, 64, |x, y| {
            let (u, v) = ((x as f32 - 31.5) / 30.0, (y as f32 - 31.5) / 18.0);
            let inside = u * u + v * v <= 1.0;
            [60, 150 + (y * 2) as u8 / 2, 50, if inside { 255 } else { 0 }]
        }),
    );
    // Glowing stripes.
    r.upload_texture(
        GLOW,
        ColorSpace::Srgb,
        32,
        32,
        &pixels(32, 32, |_, y| if (y / 4) % 2 == 0 { [255, 200, 80, 255] } else { [40, 20, 10, 255] }),
    );

    let mut scene = stage1_scene();
    scene.camera = Some(CameraView::looking_at(Vec3::new(0.0, 1.3, 3.6), Vec3::new(0.0, 0.1, 0.0)));
    scene.ambient = Vec3::splat(0.25);
    let place = |x: f32, turn: f32, scale: Vec3| {
        Mat4::from_scale_rotation_translation(scale, Quat::from_rotation_y(turn), Vec3::new(x, 0.0, 0.0))
    };
    let textures = |f: fn(&mut MaterialTextures)| {
        let mut t = MaterialTextures::NONE;
        f(&mut t);
        t
    };
    scene.draws = vec![
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: place(-1.9, 0.5, Vec3::ONE),
            material: PbrMaterial { textures: textures(|t| t.base_color = Some(GRID)), ..PbrMaterial::DEFAULT },
            cast_shadows: true,
            receive_shadows: true,
            submesh: 0,
        },
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: SPHERE,
            model: place(-0.4, 0.0, Vec3::splat(1.3)),
            material: PbrMaterial {
                base_color: [0.6, 0.6, 0.65, 1.0],
                roughness: 0.35,
                textures: textures(|t| t.normal = Some(BUMPS)),
                ..PbrMaterial::DEFAULT
            },
            cast_shadows: true,
            receive_shadows: true,
            submesh: 0,
        },
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: PLANE,
            model: Mat4::from_translation(Vec3::new(0.85, 0.1, 0.4))
                * Mat4::from_rotation_x(55f32.to_radians())
                * Mat4::from_scale(Vec3::splat(1.2)),
            material: PbrMaterial {
                alpha_cutoff: 0.5,
                roughness: 0.8,
                textures: textures(|t| t.base_color = Some(LEAF)),
                ..PbrMaterial::DEFAULT
            },
            cast_shadows: true,
            receive_shadows: true,
            submesh: 0,
        },
        Draw {
            skin: None,
            own: [0.0; 4],
            mesh: CUBE,
            model: place(2.1, -0.4, Vec3::new(0.9, 1.4, 0.2)),
            material: PbrMaterial {
                base_color: [0.05, 0.05, 0.05, 1.0],
                emissive: [1.5, 1.5, 1.5],
                textures: textures(|t| t.emissive = Some(GLOW)),
                ..PbrMaterial::DEFAULT
            },
            cast_shadows: true,
            receive_shadows: true,
            submesh: 0,
        },
    ];
    let (image, stats) = render(&gpu, &mut r, &scene, 320, 180);
    assert_eq!(stats.draw_calls, 4);
    assert_golden("textured_materials", &image);

    // The same at every level: drawn straight to the view at low, through
    // the scene image above it. The cut-out's edges, the glow and the
    // normal map are where the two could differ.
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        scene.quality = Some(quality.quality());
        let (image, _) = render(&gpu, &mut r, &scene, 320, 180);
        assert_eq!(r.quality(), quality.quality());
        assert_golden("textured_materials", &image);
    }
}

/// Hardening R8: a texture or mesh is made on the GPU (pixels written,
/// mips drawn) without the assets every view of the world draws from,
/// then put in under their lock, so a level streaming in doesn't stall
/// the views.
#[test]
fn textures_and_meshes_are_made_without_holding_the_assets() {
    let gpu = gpu();
    let r = renderer(&gpu, SRGB);
    let (assets, kit) = (r.assets().clone(), r.kit().clone());
    let held = lock_assets(&assets);
    let (texture, mesh) = std::thread::scope(|s| {
        s.spawn(|| (kit.make_texture(ColorSpace::Srgb, 64, 64, &[200; 64 * 64 * 4]), kit.make_mesh(&MeshData::cube())))
            .join()
            .expect("made while another thread holds the assets")
    });
    drop(held);
    let mut assets = lock_assets(&assets);
    let before = (assets.texture_bytes(), assets.mesh_bytes());
    assets.insert_texture(A, texture);
    assets.insert_mesh(MeshKey(9), mesh);
    assert!(assets.texture_bytes() > before.0 && assets.mesh_bytes() > before.1);
}

/// Hardening R12: mips average alpha, so where a cut-out's texture is far
/// (or tiled small) its alpha is below the cutoff everywhere and it
/// vanished. Covered as much far as near: about the share of the texture
/// that's opaque.
#[test]
fn a_cutout_covers_as_much_far_away_as_near() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Stripes, 3 texels opaque in each 8: 37.5 % covered.
    r.upload_texture(
        A,
        ColorSpace::Srgb,
        64,
        64,
        &pixels(64, 64, |x, _| [255, 255, 255, if x % 8 < 3 { 255 } else { 0 }]),
    );
    let material = |alpha_cutoff, tiles: f32| PbrMaterial {
        base_color: [0.0, 0.0, 0.0, 1.0],
        emissive: [1.0; 3],
        alpha_cutoff,
        uv_scale: [tiles; 2],
        textures: MaterialTextures { base_color: Some(A), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    };
    // The mean light of the middle of the plane (linear: a pixel half
    // covered is half as bright), against the plane drawn whole.
    let mut covered = |alpha_cutoff, tiles| {
        let (image, _) = render(&gpu, &mut r, &facing_plane(material(alpha_cutoff, tiles)), 256, 256);
        let mut sum = 0.0;
        for y in 96..160 {
            for x in 96..160 {
                sum += f64::from(linear(f32::from(image.pixel(x, y)[1]) / 255.0));
            }
        }
        sum
    };
    let whole = covered(0.0, 1.0);
    let near = covered(0.5, 1.0) / whole;
    // 16 tiles: about 7 texels a pixel, the texture's third mip.
    let far = covered(0.5, 16.0) / whole;
    assert!((0.25..0.5).contains(&near), "near: {near}");
    assert!((0.2..0.6).contains(&far), "far: {far}, near {near}");
}
