//! Flat text (stage 15 phase 2), checked on the real GPU: letters drawn
//! from their font's atlas of distances, crisp close up and still read a
//! few pixels high, an outline round them if asked, hidden behind what's
//! solid, or drawn over everything.

mod common;

use common::*;
use fynkel_mesh::{SdfFont, TextAlign, TextStyle};
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const ATLAS: TextureKey = TextureKey(77);

const RED: [f32; 4] = [0.8, 0.05, 0.05, 1.0];
const WHITE: [f32; 4] = [1.0; 4];

fn roboto() -> SdfFont {
    let data =
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/assets/fonts/Roboto-Regular.ttf")).unwrap();
    SdfFont::new(data.into()).unwrap()
}

/// How a text is drawn.
#[derive(Clone, Copy)]
struct Look {
    color: [f32; 4],
    outline: f32,
    outline_color: [f32; 4],
    on_top: bool,
}

const PLAIN: Look = Look { color: WHITE, outline: 0.0, outline_color: [0.0, 0.0, 0.0, 1.0], on_top: false };

/// `text` at `size`, centred on `at` facing +Z, into `scene`; its atlas
/// uploaded to `r`.
fn add_text(
    r: &mut Renderer,
    scene: &mut RenderScene,
    font: &mut SdfFont,
    text: &str,
    size: f32,
    at: Vec3,
    look: Look,
) {
    let mut quads = Vec::new();
    font.quads(text, &TextStyle { size, align: TextAlign::Center, line_spacing: 1.0 }, &mut quads);
    let (w, h) = font.atlas().size();
    r.upload_distances(ATLAS, w, h, font.atlas().pixels());
    let first = scene.glyphs.len() as u32;
    // Centred up and down too, on its capitals' middle.
    let lift = Vec3::new(0.0, -size * 0.35, 0.0);
    for q in &quads {
        let [l, b, rt, t] = q.rect;
        let [x, y, pw, ph] = q.pixels.map(f32::from);
        scene.glyphs.push(Glyph {
            center: at + lift + Vec3::new((l + rt) * 0.5, (b + t) * 0.5, 0.0),
            right: Vec3::new((rt - l) * 0.5, 0.0, 0.0),
            up: Vec3::new(0.0, (t - b) * 0.5, 0.0),
            uv: [pw / w as f32, ph / h as f32, x / w as f32, y / h as f32],
            color: look.color,
            outline_color: look.outline_color,
            outline: look.outline,
            softness: 0.0,
        });
    }
    let material = PbrMaterial {
        blend: Blend::Transparent,
        textures: MaterialTextures { base_color: Some(ATLAS), ..MaterialTextures::NONE },
        ..PbrMaterial::DEFAULT
    };
    scene.particle_draws.push(ParticleDraw {
        material,
        mode: ParticleMode::Glyphs { on_top: look.on_top },
        sheet: [1, 1],
        lit: false,
        softness: 0.0,
        largest: 0.0,
        first,
        count: quads.len() as u32,
        center: at,
        radius: size * text.len() as f32,
    });
}

/// Seen from 5 m along +Z, on a grey that's neither the text nor black.
fn scene() -> RenderScene {
    let mut camera = CameraView::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    camera.clear_color = [0.1, 0.1, 0.3];
    RenderScene { camera: Some(camera), quality: Some(QualityLevel::Medium.quality()), ..Default::default() }
}

fn is_background(p: [u8; 4]) -> bool {
    p[2] > p[0] + 40
}

fn is_white(p: [u8; 4]) -> bool {
    p[0] > 230 && p[1] > 230 && p[2] > 230
}

#[test]
fn a_letter_is_its_colour_inside_and_its_hole_shows_through() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut font = roboto();
    let mut s = scene();
    add_text(&mut r, &mut s, &mut font, "O", 2.0, Vec3::ZERO, PLAIN);
    let (image, stats) = render(&gpu, &mut r, &s, 128, 128);
    assert_eq!(stats.draw_calls, 1);
    assert!(is_background(image.pixel(64, 64)), "the O's hole: {:?}", image.pixel(64, 64));
    // Along the middle row: background, the O's left stroke, its hole, its
    // right stroke, background.
    let row: Vec<bool> = (0..128).map(|x| is_white(image.pixel(x, 64))).collect();
    let strokes = row.windows(2).filter(|w| !w[0] && w[1]).count();
    assert_eq!(strokes, 2, "two strokes across an O: {row:?}");
    assert!(is_background(image.pixel(2, 64)) && is_background(image.pixel(125, 64)));
}

#[test]
fn an_outline_goes_round_the_letters_in_its_colour() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut font = roboto();
    let mut count = |outline: f32| {
        let mut s = scene();
        let look = Look { color: WHITE, outline, outline_color: [0.0, 0.0, 0.0, 1.0], on_top: false };
        add_text(&mut r, &mut s, &mut font, "l", 2.0, Vec3::ZERO, look);
        let image = render(&gpu, &mut r, &s, 128, 128).0;
        let black = image.rgba.chunks(4).filter(|p| p[0] < 20 && p[1] < 20 && p[2] < 20).count();
        let white = image.rgba.chunks(4).filter(|p| is_white([p[0], p[1], p[2], p[3]])).count();
        (black, white)
    };
    let (none, white) = count(0.0);
    assert_eq!(none, 0, "no dark fringe without an outline");
    let (thin, thin_white) = count(0.4);
    let (thick, _) = count(0.9);
    assert!(thin > 50 && thick > thin * 3 / 2, "a wider outline, more of it: {thin} then {thick}");
    assert!(thin_white.abs_diff(white) < white / 10, "the letter stays as it was: {white} then {thin_white}");
}

#[test]
fn text_is_hidden_behind_what_is_solid_unless_it_is_on_top() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut font = roboto();
    // A grey wall between the camera and the text.
    let wall = Draw::new(
        CUBE,
        Mat4::from_translation(Vec3::new(0.0, 0.0, 2.0)) * Mat4::from_scale(Vec3::new(4.0, 4.0, 0.1)),
        PbrMaterial { base_color: [0.0, 0.0, 0.0, 1.0], emissive: [0.3, 0.3, 0.3], ..PbrMaterial::DEFAULT },
    );
    let mut whites = |on_top: bool| {
        let mut s = scene();
        s.draws.push(wall);
        add_text(&mut r, &mut s, &mut font, "HI", 2.5, Vec3::ZERO, Look { on_top, ..PLAIN });
        let image = render(&gpu, &mut r, &s, 128, 128).0;
        image.rgba.chunks(4).filter(|p| is_white([p[0], p[1], p[2], p[3]])).count()
    };
    assert_eq!(whites(false), 0, "behind the wall");
    let over = whites(true);
    assert!(over > 100, "over it: {over}");
}

#[test]
fn signs_close_up_and_far_off() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    let mut font = roboto();
    let mut s = scene();
    let red = Look { color: RED, outline: 0.5, outline_color: [1.0, 0.9, 0.2, 1.0], on_top: false };
    add_text(&mut r, &mut s, &mut font, "EXIT", 1.2, Vec3::new(0.0, 0.7, 0.0), red);
    // Far off: capitals about 6 and 5 pixels high.
    add_text(&mut r, &mut s, &mut font, "Score: 12,345", 0.4, Vec3::new(0.0, -0.5, 0.0), PLAIN);
    add_text(&mut r, &mut s, &mut font, "-12 +305 x4", 0.3, Vec3::new(0.0, -1.0, 0.0), PLAIN);
    let image = render(&gpu, &mut r, &s, 192, 128).0;
    assert_golden("text_signs", &image);
}
