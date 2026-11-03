//! Stage 8 phase 3: shadows. The sun's (cascaded) and spot lights' fall
//! where something stands between them and a surface; a draw can choose
//! not to cast them, or not to receive them; and each level draws its own
//! number of maps. That a still shadow doesn't shimmer as the camera moves
//! is checked on the maps' maths, in `shadows.rs`, and here on the GPU.
//!
//! The pictures (hardening R5) hold what the counts can't: acne, a shadow
//! that floats off its caster, a cut-out's holes, the seam between two
//! cascades and a spot's cone. A shadow's edge is a few pixels wide, so
//! they allow a few pixels, not 0.1 % of the image.

mod common;

use common::*;
use fynkel_render::glam::{Mat4, Quat, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 128;

fn material(grey: f32) -> PbrMaterial {
    let g = linear(grey);
    PbrMaterial { base_color: [g, g, g, 1.0], metallic: 0.0, roughness: 1.0, ..PbrMaterial::DEFAULT }
}

/// A 12 m floor, and a 1 m cube whose bottom is 1 m above it, at the
/// origin; seen from above and in front.
fn stage(lights: Vec<Light>, quality: QualityLevel) -> RenderScene {
    RenderScene {
        joints: Vec::new(),
        camera: Some(CameraView::looking_at(Vec3::new(0.0, 7.0, 5.0), Vec3::ZERO)),
        lights,
        quality: Some(quality.quality()),
        time: [0.0; 2],
        shader_values: Vec::new(),
        effects: Vec::new(),
        particles: Vec::new(),
        particle_draws: Vec::new(),
        glyphs: Vec::new(),
        sprites: Vec::new(),
        ambient: Vec3::splat(0.1),
        lines: Vec::new(),
        sky: None,
        fog: None,
        draws: vec![
            Draw::new(PLANE, Mat4::from_scale(Vec3::splat(12.0)), material(0.7)),
            Draw::new(CUBE, Mat4::from_translation(Vec3::new(0.0, 1.5, 0.0)), material(0.5)),
        ],
    }
}

/// The sun, from up and to the left (-x): the cube's shadow falls about
/// 0.75 m to its right.
fn sun() -> Light {
    Light::sun(Vec3::new(1.0, -2.0, 0.0), Vec3::ONE).with_shadows(true)
}

/// Where `p` shows in a `SIZE`² frame of `scene`'s camera.
fn pixel_of(scene: &RenderScene, p: Vec3) -> (u32, u32) {
    let camera = scene.camera.expect("a camera");
    let projection =
        fynkel_render::glam::camera::rh::proj::directx::perspective(camera.fov_y, 1.0, camera.far, camera.near);
    let clip = (projection * camera.view).project_point3(p);
    (((clip.x * 0.5 + 0.5) * SIZE as f32) as u32, ((0.5 - clip.y * 0.5) * SIZE as f32) as u32)
}

fn brightness(image: &Image, (x, y): (u32, u32)) -> u32 {
    let [r, g, b, _] = image.pixel(x, y);
    u32::from(r) + u32::from(g) + u32::from(b)
}

/// How much darker `p` is in `scene` than with no light casting shadows:
/// 1 for no change, less where a shadow falls.
fn shade_at(gpu: &Gpu, scene: &RenderScene, p: Vec3) -> (f32, FrameStats) {
    let mut r = renderer(gpu, SRGB);
    let (shadowed, stats) = render(gpu, &mut r, scene, SIZE, SIZE);
    let mut plain = scene.clone();
    plain.lights.iter_mut().for_each(|l| l.shadows = false);
    let (lit, _) = render(gpu, &mut r, &plain, SIZE, SIZE);
    let at = pixel_of(scene, p);
    (brightness(&shadowed, at) as f32 / brightness(&lit, at).max(1) as f32, stats)
}

const IN_THE_SUNS_SHADOW: Vec3 = Vec3::new(0.75, 0.0, 0.0);
const IN_THE_OPEN: Vec3 = Vec3::new(-2.0, 0.0, 1.5);

#[test]
fn the_sun_casts_shadows_at_every_level() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let scene = stage(vec![sun()], quality);
        let (shade, stats) = shade_at(&gpu, &scene, IN_THE_SUNS_SHADOW);
        assert!(shade < 0.5, "{quality:?}: under the cube, the floor is in shadow ({shade})");
        assert_eq!(
            stats.shadow_maps as usize,
            quality.quality().shadows.unwrap().cascades,
            "{quality:?}: a map per cascade"
        );
        assert!(stats.shadow_casters >= 2, "{quality:?}: the floor and the cube cast ({stats:?})");
        let (open, _) = shade_at(&gpu, &scene, IN_THE_OPEN);
        assert!(open > 0.97, "{quality:?}: out in the open, it's lit as without shadows ({open})");
    }
}

/// High's filter draws the same shadow read either way: four gathers on
/// Apple's GPUs, nine filtered compares elsewhere (stage 16).
#[test]
fn the_softer_filter_draws_the_same_shadow_gathered_or_not() {
    let gpu = gpu();
    let scene = stage(vec![sun()], QualityLevel::High);
    let draw = |kit: std::sync::Arc<Kit>| {
        let mut r = Renderer::sharing(Assets::shared(&kit), SRGB);
        r.upload_mesh(CUBE, &MeshData::cube());
        r.upload_mesh(PLANE, &MeshData::plane());
        render(&gpu, &mut r, &scene, SIZE, SIZE).0
    };
    let gathered = Kit::new(&gpu);
    gathered.set_gather_shadows(true);
    let compared = Kit::new(&gpu);
    compared.set_gather_shadows(false);
    let (a, b) = (draw(gathered), draw(compared));
    let mut most = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            for (p, q) in a.pixel(x, y).into_iter().zip(b.pixel(x, y)) {
                most = most.max(p.abs_diff(q));
            }
        }
    }
    assert!(most <= 2, "the two differ by {most} of 255");
}

/// Hardening R10: a caster far towards the sun, past how far the map's
/// depth range may stretch, is flattened onto its near end: it still
/// casts.
#[test]
fn a_caster_far_towards_the_sun_still_casts() {
    let gpu = gpu();
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let mut scene = stage(vec![sun()], quality);
        let towards_sun = -Vec3::new(1.0, -2.0, 0.0).normalize();
        let roof =
            Mat4::from_scale_rotation_translation(Vec3::splat(6.0), Quat::IDENTITY, IN_THE_OPEN + towards_sun * 200.0);
        scene.draws.push(Draw { receive_shadows: false, ..Draw::new(PLANE, roof, material(0.5)) });
        let (shade, _) = shade_at(&gpu, &scene, IN_THE_OPEN);
        assert!(shade < 0.5, "{quality:?}: under a roof 200 m up, the floor is in shadow ({shade})");
    }
}

#[test]
fn a_draw_can_choose_not_to_cast_or_receive_shadows() {
    let gpu = gpu();
    let mut scene = stage(vec![sun()], QualityLevel::Medium);
    scene.draws[1].cast_shadows = false;
    let (shade, stats) = shade_at(&gpu, &scene, IN_THE_SUNS_SHADOW);
    assert!(shade > 0.97, "the cube casts none ({shade})");
    assert_eq!(stats.shadow_casters, 2, "only the floor casts, into each cascade: {stats:?}");

    let mut scene = stage(vec![sun()], QualityLevel::Medium);
    scene.draws[0].receive_shadows = false;
    let (shade, _) = shade_at(&gpu, &scene, IN_THE_SUNS_SHADOW);
    assert!(shade > 0.97, "the floor takes none ({shade})");
}

#[test]
fn a_spot_light_casts_shadows() {
    let gpu = gpu();
    // Up to the left, aimed past the cube: its shadow falls about 0.9 m to
    // the cube's right.
    let from = Vec3::new(-1.5, 4.0, 0.0);
    let torch = Light::spot(from, Vec3::new(0.5, 0.0, 0.0) - from, 12.0, 1.4, 0.2, Vec3::splat(4.0));
    let scene = stage(vec![torch.with_shadows(true)], QualityLevel::Medium);
    let (shade, stats) = shade_at(&gpu, &scene, Vec3::new(0.9, 0.0, 0.0));
    assert!(shade < 0.5, "the cube's shadow ({shade})");
    assert_eq!(stats.shadow_maps, 1);
    let (open, _) = shade_at(&gpu, &scene, Vec3::new(-1.0, 0.0, 1.0));
    assert!(open > 0.97, "lit where nothing blocks it ({open})");
}

#[test]
fn each_level_draws_its_own_number_of_maps() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // Six spot lights with shadows, and a lamp, whose shadows are for a
    // later stage.
    let mut lights: Vec<Light> = (0..6)
        .map(|i| {
            Light::spot(Vec3::new(i as f32 - 3.0, 4.0, 0.0), Vec3::NEG_Y, 8.0, 1.0, 0.2, Vec3::ONE).with_shadows(true)
        })
        .collect();
    lights.push(Light::point(Vec3::new(0.0, 3.0, 2.0), 5.0, Vec3::ONE).with_shadows(true));
    lights.push(sun());
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let level = quality.quality().shadows.unwrap();
        let (_, stats) = render(&gpu, &mut r, &stage(lights.clone(), quality), SIZE, SIZE);
        assert_eq!(stats.shadow_maps as usize, level.cascades + level.spot_maps, "{quality:?}");
    }
    // No light asks for them: no maps.
    lights.iter_mut().for_each(|l| l.shadows = false);
    let (_, stats) = render(&gpu, &mut r, &stage(lights, QualityLevel::High), SIZE, SIZE);
    assert_eq!((stats.shadow_maps, stats.shadow_casters), (0, 0));
}

// --- Pictures ------------------------------------------------------------------

const PICTURE: u32 = 256;

/// At most this many of a picture's pixels may differ from its golden.
const PICTURE_PIXELS: usize = 8;

fn picture(gpu: &Gpu, r: &mut Renderer, name: &str, scene: &RenderScene) {
    let (image, _) = render(gpu, r, scene, PICTURE, PICTURE);
    assert_golden_within(name, &image, PICTURE_PIXELS);
}

fn level_name(quality: QualityLevel) -> &'static str {
    match quality {
        QualityLevel::Low => "low",
        QualityLevel::Medium => "medium",
        QualityLevel::High => "high",
    }
}

/// A cube standing on the floor, close up, under a low sun: acne shows as
/// stripes on the lit floor and the cube's lit faces, and a shadow that
/// floats shows as a lit gap where the cube meets the floor.
#[test]
fn picture_a_cube_on_the_floor() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [QualityLevel::Low, QualityLevel::Medium, QualityLevel::High] {
        let mut scene = stage(vec![Light::sun(Vec3::new(1.0, -0.6, 0.4), Vec3::ONE).with_shadows(true)], quality);
        scene.camera = Some(CameraView::looking_at(Vec3::new(-1.5, 2.0, 3.0), Vec3::new(0.5, 0.3, 0.0)));
        scene.draws[1].model = Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0));
        picture(&gpu, &mut r, &format!("shadow_contact_{}", level_name(quality)), &scene);
    }
}

/// A standing square, cut out in a 4 × 4 checker, between the sun and the
/// floor: its shadow has the checker's holes.
#[test]
fn picture_a_cutout_casts_its_holes() {
    const CHECKER: TextureKey = TextureKey(1);
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let checker: Vec<u8> = (0..4u32)
        .flat_map(|y| (0..4u32).map(move |x| (x, y)))
        .flat_map(|(x, y)| [255, 255, 255, if (x + y) % 2 == 0 { 255 } else { 0 }])
        .collect();
    r.upload_texture(CHECKER, ColorSpace::Srgb, 4, 4, &checker);
    let mut scene =
        stage(vec![Light::sun(Vec3::new(0.3, -1.0, 0.9), Vec3::ONE).with_shadows(true)], QualityLevel::Medium);
    scene.camera = Some(CameraView::looking_at(Vec3::new(0.0, 5.0, 5.0), Vec3::new(0.0, 0.0, 1.0)));
    scene.draws[1] = Draw::new(
        PLANE,
        Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::from_rotation_x(90f32.to_radians()),
            Vec3::new(0.0, 1.5, 0.0),
        ),
        PbrMaterial {
            alpha_cutoff: 0.5,
            double_sided: true,
            textures: MaterialTextures { base_color: Some(CHECKER), ..MaterialTextures::NONE },
            ..material(0.5)
        },
    );
    picture(&gpu, &mut r, "shadow_cutout", &scene);
}

/// A row of posts down a long floor, seen low along it: the posts' shadows
/// cross from one cascade into the next.
#[test]
fn picture_the_cascades_meet() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [QualityLevel::Medium, QualityLevel::High] {
        let mut draws = vec![Draw::new(PLANE, Mat4::from_scale(Vec3::new(12.0, 1.0, 120.0)), material(0.7))];
        for i in 0..24 {
            let z = -2.0 - i as f32 * 2.5;
            draws.push(Draw::new(
                CUBE,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(0.3, 3.0, 0.3),
                    Quat::IDENTITY,
                    Vec3::new(-1.0, 1.5, z),
                ),
                material(0.5),
            ));
        }
        let scene = RenderScene {
            joints: Vec::new(),
            camera: Some(CameraView::looking_at(Vec3::new(1.5, 2.5, 4.0), Vec3::new(-1.0, 0.0, -20.0))),
            lights: vec![Light::sun(Vec3::new(1.0, -0.8, -0.2), Vec3::ONE).with_shadows(true)],
            quality: Some(quality.quality()),
            time: [0.0; 2],
            shader_values: Vec::new(),
            effects: Vec::new(),
            particles: Vec::new(),
            particle_draws: Vec::new(),
            glyphs: Vec::new(),
            sprites: Vec::new(),
            ambient: Vec3::splat(0.1),
            lines: Vec::new(),
            sky: None,
            fog: None,
            draws,
        };
        picture(&gpu, &mut r, &format!("shadow_cascades_{}", level_name(quality)), &scene);
    }
}

/// A spot light with a hard edge over the cube: the cube's shadow, and the
/// cone's rim on the floor.
#[test]
fn picture_a_spot_lights_cone() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let from = Vec3::new(-1.0, 4.0, 0.5);
    let spot = Light::spot(from, Vec3::new(0.5, 0.0, 0.0) - from, 12.0, 1.0, 0.0, Vec3::splat(6.0));
    let scene = stage(vec![spot.with_shadows(true)], QualityLevel::Medium);
    picture(&gpu, &mut r, "shadow_spot", &scene);
}

// --- Shimmer ---------------------------------------------------------------------

/// Half the shimmer camera's view, in metres, and its image's side: a
/// pixel is 6/256 m, exact in binary, so a camera slid by whole pixels
/// sees the floor's points at the same places in its pixels.
const SHIMMER_HALF: f32 = 3.0;
const SHIMMER_SIDE: u32 = 256;

/// The floor's pixels in a still shadow, seen from straight above by a
/// camera slid `shift` whole pixels to the right: each as its column and
/// row on the floor, so a still shadow gives the same set however far the
/// camera has slid.
fn shadow_pixels(
    gpu: &Gpu,
    r: &mut Renderer,
    quality: QualityLevel,
    shift: i32,
) -> std::collections::BTreeSet<(i32, u32)> {
    let metres = 2.0 * SHIMMER_HALF / SHIMMER_SIDE as f32;
    let x = shift as f32 * metres;
    let mut camera = CameraView::looking_at(Vec3::new(x, 10.0, 0.0), Vec3::new(x, 0.0, 0.0));
    camera.view =
        fynkel_render::glam::camera::rh::view::look_at_mat4(camera.position, Vec3::new(x, 0.0, 0.0), Vec3::NEG_Z);
    camera.orthographic = Some(SHIMMER_HALF);
    camera.far = 20.0;
    let mut scene = stage(vec![Light::sun(Vec3::new(0.7, -1.0, 0.3), Vec3::ONE).with_shadows(true)], quality);
    scene.camera = Some(camera);
    let (image, _) = render(gpu, r, &scene, SHIMMER_SIDE, SHIMMER_SIDE);
    // The lit floor in a corner; its shadow is below half that.
    let lit = brightness(&image, (2, 2));
    let mut dark = std::collections::BTreeSet::new();
    for y in 0..SHIMMER_SIDE {
        for column in 0..SHIMMER_SIDE {
            if brightness(&image, (column, y)) * 2 < lit {
                dark.insert((column as i32 + shift, y));
            }
        }
    }
    assert!(dark.len() > 100, "{quality:?}: a shadow under the cube ({} pixels)", dark.len());
    dark
}

/// A still shadow keeps its place on the GPU while the camera slides:
/// by whole pixels, but fractions of a shadow texel, so a map that slid
/// with the camera would move the shadow's edge across the floor's pixels.
#[test]
fn a_still_shadow_stays_still_as_the_camera_moves() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    for quality in [QualityLevel::Low, QualityLevel::High] {
        let first = shadow_pixels(&gpu, &mut r, quality, 0);
        for step in 1..10 {
            let shift = step * 3;
            let now = shadow_pixels(&gpu, &mut r, quality, shift);
            let changed = first.symmetric_difference(&now).count();
            assert!(
                changed <= 4,
                "{quality:?}: slid {shift} pixels, {changed} of the shadow's {} changed",
                first.len()
            );
        }
    }
}

// --- Memory (hardening R3) ------------------------------------------------------

/// A level's sun maps' bytes: 16-bit depth.
fn cascade_bytes(quality: QualityLevel) -> u64 {
    let level = quality.quality().shadows.unwrap();
    u64::from(level.map_size).pow(2) * level.cascades as u64 * 2
}

/// `n` of a level's spot maps' bytes: 32-bit depth.
fn spot_bytes(quality: QualityLevel, n: u64) -> u64 {
    u64::from(quality.quality().shadows.unwrap().spot_map_size).pow(2) * n * 4
}

#[test]
fn one_shadowed_spot_holds_one_map() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let from = Vec3::new(-1.5, 4.0, 0.0);
    let torch = Light::spot(from, Vec3::NEG_Y, 12.0, 1.0, 0.2, Vec3::ONE).with_shadows(true);
    let (_, stats) = render(&gpu, &mut r, &stage(vec![torch], QualityLevel::High), 32, 32);
    assert_eq!(stats.shadow_maps, 1);
    assert_eq!(stats.shadow_bytes, spot_bytes(QualityLevel::High, 1), "not the level's 4");
    assert_eq!(r.kit().shadow_bytes(), stats.shadow_bytes);
}

#[test]
fn maps_go_a_while_after_shadows_stop() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let lit = stage(vec![sun()], QualityLevel::Medium);
    let (_, stats) = render(&gpu, &mut r, &lit, 32, 32);
    assert_eq!(stats.shadow_bytes, cascade_bytes(QualityLevel::Medium));
    // A frame or two without: still there, so shadows that flicker on and
    // off don't remake them.
    let dark = stage(vec![sun().with_shadows(false)], QualityLevel::Medium);
    let (_, stats) = render(&gpu, &mut r, &dark, 32, 32);
    assert_eq!(stats.shadow_bytes, cascade_bytes(QualityLevel::Medium));
    // A couple of seconds without: gone.
    for _ in 0..120 {
        render(&gpu, &mut r, &dark, 32, 32);
    }
    let (_, stats) = render(&gpu, &mut r, &dark, 32, 32);
    assert_eq!(stats.shadow_bytes, 0);
    assert_eq!(r.kit().shadow_bytes(), 0);
}

#[test]
fn views_of_one_kit_share_their_maps() {
    let gpu = gpu();
    let kit = Kit::new(&gpu);
    let mut views: Vec<Renderer> = (0..2).map(|_| Renderer::sharing(Assets::shared(&kit), SRGB)).collect();
    for r in &mut views {
        r.upload_mesh(CUBE, &MeshData::cube());
        r.upload_mesh(PLANE, &MeshData::plane());
        let (_, stats) = render(&gpu, r, &stage(vec![sun()], QualityLevel::Medium), 32, 32);
        assert_eq!(stats.shadow_bytes, cascade_bytes(QualityLevel::Medium), "each view sees its maps");
    }
    assert_eq!(kit.shadow_bytes(), cascade_bytes(QualityLevel::Medium), "one set between them");
    // At another level, a view takes maps of its own.
    render(&gpu, &mut views[1], &stage(vec![sun()], QualityLevel::High), 32, 32);
    assert_eq!(kit.shadow_bytes(), cascade_bytes(QualityLevel::Medium) + cascade_bytes(QualityLevel::High));
    drop(views);
    assert_eq!(kit.shadow_bytes(), 0, "gone with the views");
}

/// Hardening R12: a cut-out caster whose texture is small in the shadow map
/// (far, or tiled fine) still casts about its share of shadow; mips
/// averaged its alpha below the cutoff, and it cast none.
#[test]
fn a_fine_cutout_still_casts_its_share() {
    const STRIPES: TextureKey = TextureKey(2);
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    // 3 texels opaque in each 8: 37.5 % covered.
    let stripes: Vec<u8> =
        (0..64u32 * 64).flat_map(|i| [255, 255, 255, if i % 64 % 8 < 3 { 255 } else { 0 }]).collect();
    r.upload_texture(STRIPES, ColorSpace::Srgb, 64, 64, &stripes);
    // A standing screen at x = -3, facing +x, tiled 32 times: the sun,
    // from the left, throws it onto the floor at -2.5 < x < -0.5.
    let mut scene =
        stage(vec![Light::sun(Vec3::new(1.0, -1.0, 0.0), Vec3::ONE).with_shadows(true)], QualityLevel::Medium);
    scene.draws[1] = Draw::new(
        PLANE,
        Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::from_rotation_z(-90f32.to_radians()),
            Vec3::new(-3.0, 1.5, 0.0),
        ),
        PbrMaterial {
            alpha_cutoff: 0.5,
            double_sided: true,
            uv_scale: [32.0; 2],
            textures: MaterialTextures { base_color: Some(STRIPES), ..MaterialTextures::NONE },
            ..material(0.5)
        },
    );
    let light = |scene: &RenderScene, r: &mut Renderer| {
        let (image, _) = render(&gpu, r, scene, SIZE, SIZE);
        let mut sum = 0.0;
        for i in 0..8 {
            for j in 0..8 {
                let p = Vec3::new(-2.2 + 1.4 * i as f32 / 7.0, 0.0, -0.7 + 1.4 * j as f32 / 7.0);
                let (x, y) = pixel_of(scene, p);
                sum += linear(f32::from(image.pixel(x, y)[1]) / 255.0);
            }
        }
        sum
    };
    let shadowed = light(&scene, &mut r);
    scene.draws[1].cast_shadows = false;
    let open = light(&scene, &mut r);
    let share = shadowed / open;
    assert!(share < 0.85, "the screen casts some shadow: {share} of the light reaches the floor");
    assert!(share > 0.3, "not a whole one: {share}");
}

/// A quality without shadows draws no maps, whatever the lights ask: the
/// floor under the cube is lit as in the open.
#[test]
fn a_quality_without_shadows_draws_none() {
    let gpu = gpu();
    let from = Vec3::new(-1.5, 4.0, 0.0);
    let torch = Light::spot(from, Vec3::new(0.5, 0.0, 0.0) - from, 12.0, 1.4, 0.2, Vec3::splat(4.0));
    for (lights, at) in [(vec![sun()], IN_THE_SUNS_SHADOW), (vec![torch.with_shadows(true)], Vec3::new(0.9, 0.0, 0.0))]
    {
        let mut scene = stage(lights, QualityLevel::High);
        let (shade, _) = shade_at(&gpu, &scene, at);
        assert!(shade < 0.5, "with shadows, in shadow ({shade})");
        scene.quality = Some(Quality { shadows: None, ..Quality::HIGH });
        let (shade, stats) = shade_at(&gpu, &scene, at);
        assert_eq!((stats.shadow_maps, stats.shadow_casters), (0, 0), "{stats:?}");
        assert!(shade > 0.97, "without, lit ({shade})");
    }
}

/// Shadow settings out of range are said, not drawn.
#[test]
fn shadow_settings_out_of_range_are_invalid() {
    let base = Quality::MEDIUM.shadows.unwrap();
    assert_eq!(Quality::MEDIUM.invalid(), None);
    assert_eq!(Quality { shadows: None, ..Quality::LOW }.invalid(), None);
    for level in [
        ShadowLevel { cascades: 0, ..base },
        ShadowLevel { cascades: 4, ..base },
        ShadowLevel { map_size: 1000, ..base },
        ShadowLevel { spot_map_size: 8192, ..base },
        ShadowLevel { spot_maps: 5, ..base },
        ShadowLevel { distance: 0.0, ..base },
        ShadowLevel { distance: f32::NAN, ..base },
    ] {
        assert!(Quality { shadows: Some(level), ..Quality::MEDIUM }.invalid().is_some(), "{level:?}");
    }
}
