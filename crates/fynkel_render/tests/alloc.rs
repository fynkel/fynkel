//! A steady frame allocates nothing in the renderer's own code (stage 1
//! plan, step 2). This is its own test binary because it replaces the
//! global allocator.
//!
//! Unsafe code is allowed here and only here: a global allocator is unsafe
//! by definition. It just counts, then forwards to the system allocator.
#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::*;
use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::*;

/// Counts allocations made on this thread while counting is switched on.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn count<T>(f: impl FnOnce() -> T) -> (T, u64) {
    COUNT.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    let out = f();
    COUNTING.with(|c| c.set(false));
    (out, COUNT.with(Cell::get))
}

/// 1,000 cubes moving every frame, some of them see-through.
fn busy_scene(t: f32) -> RenderScene {
    let mut scene = stage1_scene();
    // High above the 40 × 25 grid, so every cube is seen.
    scene.camera = Some(CameraView::looking_at(Vec3::new(20.0, 60.0, 30.0), Vec3::new(20.0, 0.0, 12.0)));
    let material = scene.draws[0].material;
    // Shadows from the sun and a spot light: their maps and casters too.
    scene.lights = vec![
        Light::sun(Vec3::new(-0.4, -1.0, -0.3), Vec3::ONE).with_shadows(true),
        Light::spot(Vec3::new(20.0, 8.0, 12.0), Vec3::NEG_Y, 20.0, 1.2, 0.2, Vec3::splat(5.0)).with_shadows(true),
    ];
    scene.quality = Some(Quality::High);
    // A sky that lights, and fog.
    let c = Vec3::new(0.3, 0.5, 0.9);
    scene.sky = Some(Sky { kind: SkyKind::Gradient { zenith: c, horizon: Vec3::ONE, ground: c * 0.2 }, lighting: 1.0 });
    scene.fog = Some(Fog { color: None, start: 20.0, end: 80.0, height: Some(10.0) });
    scene.draws = (0..1000)
        .map(|i| Draw {
            mesh: CUBE,
            model: Mat4::from_translation(Vec3::new((i % 40) as f32, (t + i as f32).sin(), (i / 40) as f32)),
            // Every fourth see-through, every tenth glowing: sorted back
            // to front each frame.
            material: PbrMaterial {
                blend: match i {
                    _ if i % 10 == 0 => Blend::Additive,
                    _ if i % 4 == 0 => Blend::Transparent,
                    _ => Blend::Opaque,
                },
                base_color: [0.8, 0.3, 0.1, if i % 4 == 0 { 0.5 } else { 1.0 }],
                ..material
            },
            cast_shadows: true,
            receive_shadows: true,
        })
        .collect();
    scene
}

#[test]
fn a_steady_frame_allocates_nothing_in_prepare() {
    let gpu = gpu();
    let mut r = renderer(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 256, height: 256, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

    // Warm up: buffers grow to the scene's size.
    let scenes: Vec<RenderScene> = (0..8).map(|i| busy_scene(i as f32 * 0.1)).collect();
    for scene in &scenes {
        r.render(scene, &view, 256, 256);
    }

    let mut wgpu_allocs = Vec::new();
    for scene in &scenes {
        let (stats, prepare_allocs) = count(|| r.prepare(scene, 256, 256));
        assert_eq!(stats.draws, 1000);
        assert_eq!(stats.shadow_maps, 4, "three cascades and a spot map");
        assert_eq!(prepare_allocs, 0, "prepare allocated {prepare_allocs} times in a steady frame");

        let (_, n) = count(|| {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            r.encode(&mut encoder, &view);
            gpu.queue.submit([encoder.finish()]);
        });
        wgpu_allocs.push(n);
    }
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    // wgpu allocates inside encode and submit (command buffers, tracking),
    // which our code can't avoid. Recorded, not asserted.
    eprintln!("encode + submit, 1000 draws: wgpu allocations per frame {wgpu_allocs:?}");
}
