//! A first frame compiles what it draws with, not every pipeline the
//! renderer could use (hardening R2): compiling them is most of the time
//! to a first frame on a phone.

mod common;

use common::*;
use fynkel_render::glam::Vec3;
use fynkel_render::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A renderer on a kit of its own, so the count is this test's.
fn fresh(gpu: &Gpu) -> (std::sync::Arc<Kit>, Renderer) {
    let kit = Kit::new(gpu);
    let mut r = Renderer::sharing(Assets::shared(&kit), SRGB);
    r.upload_mesh(CUBE, &MeshData::cube());
    (kit, r)
}

#[test]
fn a_first_frame_compiles_only_what_it_draws_with() {
    let gpu = gpu();
    // Low draws straight to the view: the opaque pipeline for its format.
    // Above, into the scene image, then the last pass onto the view.
    for (quality, want) in [(Quality::Low, 1), (Quality::Medium, 2), (Quality::High, 2)] {
        let (kit, mut r) = fresh(&gpu);
        assert_eq!(kit.pipelines_made(), 0, "{quality:?}: none before a frame");
        let mut scene = stage1_scene();
        scene.quality = Some(quality);
        render(&gpu, &mut r, &scene, 32, 32);
        assert_eq!(kit.pipelines_made(), want, "{quality:?}: an opaque cube under a sun without shadows");
        render(&gpu, &mut r, &scene, 32, 32);
        assert_eq!(kit.pipelines_made(), want, "{quality:?}: and nothing more the next frame");
    }
}

#[test]
fn shadows_lines_and_cutouts_compile_theirs_when_first_drawn() {
    let gpu = gpu();
    let (kit, mut r) = fresh(&gpu);
    let mut scene = stage1_scene();
    scene.quality = Some(Quality::Medium);
    render(&gpu, &mut r, &scene, 32, 32);
    let plain = kit.pipelines_made();

    scene.lights[0].shadows = true;
    render(&gpu, &mut r, &scene, 32, 32);
    assert_eq!(kit.pipelines_made(), plain + 1, "the cascades' opaque caster pipeline");

    scene.lines.push(Line { a: Vec3::ZERO, b: Vec3::ONE, color: [1.0; 4] });
    render(&gpu, &mut r, &scene, 32, 32);
    assert_eq!(kit.pipelines_made(), plain + 2, "the lines'");

    scene.draws[0].material.double_sided = true;
    render(&gpu, &mut r, &scene, 32, 32);
    assert_eq!(kit.pipelines_made(), plain + 3, "the double-sided variant");

    render(&gpu, &mut r, &scene, 32, 32);
    assert_eq!(kit.pipelines_made(), plain + 3, "each once");
}

/// Where the GPU keeps a pipeline cache (Vulkan), what a frame compiled is
/// written to the app's directory, for the next run. Elsewhere (Metal) the
/// driver keeps its own, and nothing is written.
#[test]
fn compiled_pipelines_are_kept_for_the_next_run() {
    let gpu = gpu();
    let dir = std::env::temp_dir().join(format!("fynkel-pipelines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    set_pipeline_cache_dir(&dir);
    let (kit, mut r) = fresh(&gpu);
    let mut scene = stage1_scene();
    scene.quality = Some(Quality::Medium);
    render(&gpu, &mut r, &scene, 32, 32);
    let backend = gpu.adapter.get_info().backend;
    if !kit.caches_pipelines() {
        assert_ne!(backend, wgpu::Backend::Vulkan, "Vulkan keeps a cache");
        return;
    }
    // Written off the render thread.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let kept = loop {
        let kept: u64 =
            std::fs::read_dir(&dir).unwrap().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum();
        if kept > 0 || std::time::Instant::now() > deadline {
            break kept;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(kept > 0, "the cache was written to {}", dir.display());
}
