//! Sprites (stage 15 phase 3), checked on the real GPU: a texture on
//! quads, its colour multiplied in, many of one texture one draw, drawn
//! by sorting layer and order before by distance, and cut out by alpha.

mod common;

use common::*;
use fynkel_render::glam::Vec3;
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const WHITE_TEXTURE: TextureKey = TextureKey(81);
const ARROW: TextureKey = TextureKey(82);

/// A 2 × 2 white texture; and an arrow, 8 × 8: its left half opaque
/// white, its right half white at alpha 0.3.
fn upload(r: &mut Renderer) {
    r.upload_texture(WHITE_TEXTURE, ColorSpace::Srgb, 2, 2, &[255; 16]);
    let arrow: Vec<u8> = (0..64).flat_map(|i| [255, 255, 255, if i % 8 < 4 { 255 } else { 77 }]).collect();
    r.upload_texture(ARROW, ColorSpace::Srgb, 8, 8, &arrow);
}

/// A square sprite `size` across at `at`, facing +Z, the whole texture.
fn sprite(at: Vec3, size: f32, color: [f32; 4]) -> Sprite {
    Sprite {
        center: at,
        right: Vec3::new(size * 0.5, 0.0, 0.0),
        up: Vec3::new(0.0, size * 0.5, 0.0),
        uv: [1.0, 1.0, 0.0, 0.0],
        color,
    }
}

/// Sprites of `texture` as one draw, in `layer` and `order`.
fn add(scene: &mut RenderScene, texture: TextureKey, sprites: &[Sprite], sorting: [i32; 2], cutoff: f32) {
    let first = scene.sprites.len() as u32;
    scene.sprites.extend_from_slice(sprites);
    let material = PbrMaterial {
        blend: Blend::Transparent,
        textures: MaterialTextures { base_color: Some(texture), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    };
    scene.particle_draws.push(ParticleDraw {
        material,
        mode: ParticleMode::Sprites { cutoff },
        sheet: [1, 1],
        lit: false,
        softness: 0.0,
        largest: 0.0,
        first,
        count: sprites.len() as u32,
        center: sprites[0].center,
        radius: 20.0,
        sorting,
    });
}

fn scene() -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    camera.clear_color = [0.0, 0.0, 0.0];
    RenderScene { camera: Some(camera), quality: Some(QualityLevel::Medium.quality()), ..Default::default() }
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

#[test]
fn a_thousand_sprites_of_one_texture_are_one_draw_in_their_colours() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    upload(&mut r);
    let mut s = scene();
    let many: Vec<Sprite> = (0..1000)
        .map(|i| sprite(Vec3::new((i % 40) as f32 * 0.1 - 2.0, (i / 40) as f32 * 0.1 - 1.2, 0.0), 0.05, RED))
        .collect();
    add(&mut s, WHITE_TEXTURE, &many, [0, 0], 0.0);
    let (image, stats) = render(&gpu, &mut r, &s, 128, 128);
    assert_eq!(stats.draw_calls, 1);
    let red = image.rgba.chunks(4).filter(|p| p[0] > 200 && p[1] < 40 && p[2] < 40).count();
    assert!(red > 100, "{red}");
}

#[test]
fn sprites_are_drawn_by_layer_then_order_before_by_distance() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    upload(&mut r);
    // Red in front, blue behind it: by distance red covers blue.
    let mut middle = |sorting_red: [i32; 2], sorting_blue: [i32; 2]| {
        let mut s = scene();
        add(&mut s, WHITE_TEXTURE, &[sprite(Vec3::new(0.0, 0.0, 1.0), 1.0, RED)], sorting_red, 0.0);
        add(&mut s, WHITE_TEXTURE, &[sprite(Vec3::ZERO, 1.0, BLUE)], sorting_blue, 0.0);
        render(&gpu, &mut r, &s, 64, 64).0.pixel(32, 32)
    };
    assert!(middle([0, 0], [0, 0])[0] > 200, "the nearer over the farther");
    assert!(middle([0, 0], [1, 0])[2] > 200, "a higher layer over a lower, however far");
    assert!(middle([0, 0], [0, 5])[2] > 200, "a higher order over a lower in one layer");
    assert!(middle([2, 0], [1, 9])[0] > 200, "the layer first, then the order");
}

#[test]
fn a_cutoff_cuts_out_what_is_below_it_and_without_one_it_is_see_through() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    upload(&mut r);
    let mut halves = |cutoff: f32| {
        let mut s = scene();
        add(&mut s, ARROW, &[sprite(Vec3::ZERO, 3.0, [1.0; 4])], [0, 0], cutoff);
        let image = render(&gpu, &mut r, &s, 64, 64).0;
        (image.pixel(24, 32), image.pixel(40, 32))
    };
    let (left, right) = halves(0.0);
    assert!(left[0] > 230, "opaque: {left:?}");
    assert!(right[0] > 60 && right[0] < 200, "see-through at 0.3: {right:?}");
    let (left, right) = halves(0.5);
    assert!(left[0] > 230);
    assert!(right[0] < 5, "cut out: {right:?}");
}
