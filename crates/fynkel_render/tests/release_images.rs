//! A view's images can be let go, as when the system is short of memory and
//! the app isn't drawing, and are made again by its next frame.

mod common;

use common::*;
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: u32 = 96;

#[test]
fn a_views_images_go_and_come_back_for_its_next_frame() {
    let gpu = gpu();
    let mut r = renderer(&gpu, SRGB);
    assert!(!r.holds_images(), "none before the first frame");
    let mut scene = stage1_scene();
    scene.quality = Some(QualityLevel::High.quality());
    scene.lights = vec![Light::sun(glam::Vec3::new(1.0, -1.2, -0.4), glam::Vec3::ONE).with_shadows(true)];
    let (before, stats) = render(&gpu, &mut r, &scene, SIZE, SIZE);
    assert!(r.holds_images());
    assert!(stats.shadow_bytes > 0, "the scene casts shadows");

    r.release_images();
    assert!(!r.holds_images());

    let (after, again) = render(&gpu, &mut r, &scene, SIZE, SIZE);
    assert!(r.holds_images());
    assert_eq!(again.shadow_bytes, stats.shadow_bytes);
    assert_eq!(before.rgba, after.rgba, "the same picture");
}
