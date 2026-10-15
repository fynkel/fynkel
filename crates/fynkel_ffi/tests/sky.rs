//! The world's sky and fog (stage 8 phase 4), set through the C interface.
#![allow(unsafe_code)]

mod common;

use common::*;
use fynkel_ffi::*;

/// sRGB 0–1 to the 8-bit value a view shows.
fn srgb8(c: f32) -> u8 {
    (c * 255.0).round() as u8
}

fn near(got: [u8; 4], want: [u8; 3], within: u8, what: &str) {
    for c in 0..3 {
        assert!(got[c].abs_diff(want[c]) <= within, "{what}: got {got:?}, want {want:?}");
    }
}

fn one_colour(kind: u32, c: FyColor) -> FySky {
    FySky {
        kind,
        zenith: c,
        horizon: c,
        ground: c,
        image: std::ptr::null(),
        intensity: 1.0,
        rotation: 0.0,
        lighting: 1.0,
    }
}

fn camera(world: &World) -> (u64, FyCamera) {
    let (mut id, mut lens) = (0, FyCamera::default());
    // SAFETY: a live world and local out-values.
    unsafe {
        ok(fy_camera_active(world.0, &mut id));
        ok(fy_camera_get(world.0, id, &mut lens));
    }
    (id, lens)
}

#[test]
fn a_worlds_sky_is_behind_everything_unless_its_camera_leaves_it_out() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 64, 64);
    let blue = srgb(0.2, 0.4, 0.8);
    // SAFETY: a live world and a local.
    ok(unsafe { fy_world_sky_set(world.0, &one_colour(FY_SKY_GRADIENT, blue)) });
    let shown = view.read(view.frame(), 64, 64);
    near(shown.pixel(0, 0), [51, 102, 204], 1, "a gradient's colours as picked");
    assert_ne!(shown.pixel(32, 32), shown.pixel(0, 0), "the cube in front");

    let (id, lens) = camera(&world);
    // SAFETY: a live world and a local.
    ok(unsafe { fy_camera_set(world.0, id, &FyCamera { show_sky: false, ..lens }) });
    let clear = [srgb8(0.1), srgb8(0.1), srgb8(0.12)];
    near(view.read(view.frame(), 64, 64).pixel(0, 0), clear, 1, "the clear colour without the sky");

    // SAFETY: a live world.
    unsafe {
        ok(fy_camera_set(world.0, id, &lens));
        ok(fy_world_sky_set(world.0, &one_colour(FY_SKY_COLOR, blue)));
    }
    near(view.read(view.frame(), 64, 64).pixel(0, 0), [51, 102, 204], 1, "a plain sky");
    // SAFETY: a live world; null is no sky.
    ok(unsafe { fy_world_sky_set(world.0, std::ptr::null()) });
    near(view.read(view.frame(), 64, 64).pixel(0, 0), clear, 1, "no sky: the clear colour again");

    // SAFETY: a live world and locals.
    unsafe {
        assert_eq!(fy_world_sky_set(world.0, &one_colour(7, blue)), FyStatus::InvalidValue);
        assert!(last_error().contains("7 isn't a kind of sky"), "{}", last_error());
        assert_eq!(fy_world_sky_set(world.0, &one_colour(FY_SKY_IMAGE, blue)), FyStatus::NullPointer);
        assert!(last_error().contains("image"), "{}", last_error());
    }
}

/// A Radiance .hdr, `width × height`, every pixel `rgb`.
fn hdr(width: usize, height: usize, rgb: [f32; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let pixels = vec![image::Rgb(rgb); width * height];
    image::codecs::hdr::HdrEncoder::new(&mut bytes).encode(&pixels, width, height).unwrap();
    bytes
}

#[test]
fn a_sky_image_is_decoded_from_an_hdr_and_lights_the_world() {
    let bytes = hdr(32, 16, [0.5, 0.5, 0.5]);
    let mut image = std::ptr::null_mut();
    let (mut width, mut height) = (0, 0);
    // SAFETY: bytes of the length passed and local out-values.
    unsafe {
        ok(fy_sky_image_decode(bytes.as_ptr(), bytes.len(), &mut image));
        ok(fy_sky_image_size(image, &mut width, &mut height));
    }
    assert_eq!((width, height), (32, 16));

    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 64, 64);
    let unlit = view.read(view.frame(), 64, 64).pixel(32, 32);
    let sky = FySky { image, ..one_colour(FY_SKY_IMAGE, srgb(0.0, 0.0, 0.0)) };
    // SAFETY: a live world and image.
    ok(unsafe { fy_world_sky_set(world.0, &sky) });
    // The world keeps its share: freeing the handle leaves the sky.
    // SAFETY: made above, not used again.
    unsafe { fy_sky_image_free(image) };
    let shown = view.read(view.frame(), 64, 64);
    let corner = shown.pixel(0, 0);
    assert!(corner[0] == corner[1] && corner[1] == corner[2] && corner[0] > 100, "grey light: {corner:?}");
    let lit = shown.pixel(32, 32);
    assert!(lit[0] > unlit[0] && lit[2] > unlit[2], "the sky lights the cube: {unlit:?} → {lit:?}");
    let mut stats = FyViewStats::default();
    // SAFETY: a live view and a local.
    ok(unsafe { fy_view_stats(view.0, &mut stats) });
    assert!(stats.sky_bytes > 0, "its light is counted");

    // Anything but an .hdr is refused, saying why.
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    let mut refused = std::ptr::null_mut();
    // SAFETY: bytes of the length passed and a local out-value.
    assert_eq!(unsafe { fy_sky_image_decode(png.as_ptr(), png.len(), &mut refused) }, FyStatus::InvalidValue);
    assert!(last_error().contains("a sky is a Radiance .hdr"), "{}", last_error());
    assert!(refused.is_null());
}

#[test]
fn fog_fades_the_world_into_its_colour() {
    let world = World::new();
    stage1(&world);
    let view = View::headless(&world, 64, 64);
    let red = srgb(0.8, 0.1, 0.1);
    let thick = FyFog { color: red, use_color: true, start: 0.0, end: 0.5, height: 0.0 };
    // SAFETY: a live world and a local.
    ok(unsafe { fy_world_fog_set(world.0, &thick) });
    near(view.read(view.frame(), 64, 64).pixel(32, 32), [204, 26, 26], 1, "the cube, 4 m away, all fog");

    // Without a colour of its own, the sky's horizon.
    let blue = srgb(0.2, 0.4, 0.8);
    // SAFETY: a live world and locals.
    unsafe {
        ok(fy_world_sky_set(world.0, &one_colour(FY_SKY_GRADIENT, blue)));
        ok(fy_world_fog_set(world.0, &FyFog { use_color: false, ..thick }));
    }
    near(view.read(view.frame(), 64, 64).pixel(32, 32), [51, 102, 204], 1, "the sky's horizon");

    // SAFETY: a live world; null is no fog.
    ok(unsafe { fy_world_fog_set(world.0, std::ptr::null()) });
    let clear = view.read(view.frame(), 64, 64).pixel(32, 32);
    assert!(clear[0] > clear[2], "the orange cube again: {clear:?}");

    // SAFETY: a live world and a local.
    let status = unsafe { fy_world_fog_set(world.0, &FyFog { start: 5.0, end: 5.0, ..thick }) };
    assert_eq!(status, FyStatus::InvalidValue);
    assert!(last_error().contains("end 5 must be beyond its start 5"), "{}", last_error());
}
