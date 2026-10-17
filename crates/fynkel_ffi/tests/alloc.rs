//! The frame clock's side of a frame allocates nothing (stage 1 plan
//! §3.5): `fy_view_frame` extracts into recycled scene buffers, world
//! matrices go into a kept buffer (stage 2 plan §3.1), texture slots are
//! only looked up once uploaded (§3.2), and setting
//! components goes straight into the world. Only the calling
//! thread is counted. The render worker's own allocations (wgpu's) are
//! measured in fynkel_render's tests.
//!
//! Its own test binary, because it replaces the global allocator. A global
//! allocator and C calls are both unsafe.
#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::*;
use fynkel_ffi::*;
use fynkel_world::{Transform, Vec3};

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

fn count(f: impl FnOnce()) -> u64 {
    COUNT.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    COUNT.with(Cell::get)
}

/// Five copies of the skinned KayKit Knight, each walking: their parts
/// are bent by joints the extract works out every frame. The model's
/// textures are left out; its materials are plain. Three go by a blend
/// of idle, walk and run, and all cheer with the right arm on a layer
/// above ([`knights_change_their_minds`]).
fn walking_knights(world: &World) -> Knights {
    let bytes = include_bytes!("../../../tests/assets/gltf/Knight.glb");
    let w = world.0;
    // SAFETY: a slice, a live world, and pointers to locals.
    unsafe {
        let mut model = std::ptr::null_mut();
        ok(fy_model_load(bytes.as_ptr(), bytes.len(), &mut model));
        let mut counts = FyModelCounts::default();
        ok(fy_model_counts(model, &mut counts));
        let meshes: Vec<u64> = (0..counts.meshes)
            .map(|i| {
                let (mut mesh, mut id) = (std::ptr::null_mut(), 0);
                ok(fy_model_mesh(model, i, &mut mesh));
                ok(fy_mesh_add(w, mesh, &mut id));
                fy_mesh_free(mesh);
                id
            })
            .collect();
        let materials: Vec<u64> = (0..counts.materials)
            .map(|_| {
                let mut id = 0;
                ok(fy_material_add(w, &FyMaterial::default(), &mut id));
                id
            })
            .collect();
        let clip = |name: &[u8]| {
            (0..counts.clips)
                .find(|&i| {
                    let mut buf = [0u8; 64];
                    let (mut len, mut length) = (0, 0.0);
                    ok(fy_model_clip(model, i, buf.as_mut_ptr(), buf.len(), &mut len, &mut length));
                    &buf[..len] == name
                })
                .expect("a clip") as u32
        };
        let (idle, walk, run, cheer) = (clip(b"Idle"), clip(b"Walking_A"), clip(b"Running_A"), clip(b"Cheer"));
        let roots = (0..5)
            .map(|i| {
                let mut root = 0;
                let at = Transform::from_position(Vec3::new(i as f32 * 1.5, 0.0, -3.0));
                ok(fy_model_spawn(
                    w,
                    model,
                    meshes.as_ptr(),
                    meshes.len(),
                    materials.as_ptr(),
                    materials.len(),
                    &transform(at),
                    &mut root,
                ));
                ok(fy_animator_play(w, root, 0, walk, &0.0, true));
                let mask = b"upperarm.r";
                ok(fy_animator_layer_set(w, root, 1, mask.as_ptr(), mask.len(), false, 0.8));
                let blend = (i >= 2).then(|| {
                    let mut blend = 0;
                    let (clips, thresholds) = ([idle, walk, run], [0.0, 1.1, 3.0]);
                    ok(fy_animator_blend_define(w, root, clips.as_ptr(), thresholds.as_ptr(), 3, &mut blend));
                    blend
                });
                (root, blend)
            })
            .collect();
        fy_model_free(model);
        Knights { roots, cheer }
    }
}

struct Knights {
    /// Each knight, and its blend, if it goes by one.
    roots: Vec<(u64, Option<u32>)>,
    cheer: u32,
}

/// A frame of a game that changes its mind: the blended knights' speed
/// swings, and every knight starts or stops its cheer, fading.
fn knights_change_their_minds(world: &World, knights: &Knights, frame: u32) {
    for &(root, blend) in &knights.roots {
        // SAFETY: a live world.
        unsafe {
            if let Some(blend) = blend {
                let at = [(frame as f32 * 0.1).sin() * 1.5 + 1.5, 0.2];
                ok(fy_animator_blend(world.0, root, 0, blend, at.as_ptr()));
            }
            if frame.is_multiple_of(3) {
                ok(fy_animator_play(world.0, root, 1, knights.cheer, &0.2, true));
            } else if frame % 3 == 1 {
                ok(fy_animator_stop(world.0, root, 1, &0.2));
            }
        }
    }
}

#[test]
fn a_steady_frame_with_1000_moving_cubes_in_trees_allocates_nothing_on_the_calling_thread() {
    let world = World::new();
    let cube = stage1(&world);
    let mut renderer = FyMeshRenderer::default();
    // SAFETY: a live world and a local out-value.
    ok(unsafe { fy_mesh_renderer_get(world.0, cube, &mut renderer) });
    // Textured, in both colour spaces, so the extract looks at texture
    // slots every frame too.
    // SAFETY: a live world, a slice, and pointers to locals.
    unsafe {
        let (mut texture, mut id) = (std::ptr::null_mut(), 0);
        ok(fy_texture_new_rgba(1, 1, [255u8; 4].as_ptr(), 4, &mut texture));
        ok(fy_texture_add(world.0, texture, &mut id));
        fy_texture_free(texture);
        let textured =
            FyMaterial { base_color_texture: id, normal_texture: id, emissive_texture: id, ..FyMaterial::default() };
        ok(fy_material_set(world.0, renderer.material, &textured));
        // A sky that lights and fog, which every frame takes from the world.
        let c = FyColor { r: 0.4, g: 0.6, b: 0.9, a: 1.0 };
        let sky = FySky {
            kind: FY_SKY_GRADIENT,
            zenith: c,
            horizon: c,
            ground: c,
            image: std::ptr::null(),
            intensity: 1.0,
            rotation: 0.0,
            lighting: 1.0,
        };
        ok(fy_world_sky_set(world.0, &sky));
        ok(fy_world_fog_set(world.0, &FyFog { start: 10.0, end: 50.0, ..FyFog::default() }));
    }
    let cubes: Vec<u64> = (0..1000)
        .map(|_| {
            let id = world.spawn();
            // SAFETY: a live world and a pointer to a local.
            ok(unsafe { fy_mesh_renderer_set(world.0, id, &renderer) });
            id
        })
        .collect();
    // Chains of ten, so the extract computes world matrices through
    // parents (stage 2 plan §3.1). The order is built once, in warm-up.
    for (i, &id) in cubes.iter().enumerate().filter(|(i, _)| !i.is_multiple_of(10)) {
        // SAFETY: a live world.
        ok(unsafe { fy_parent_set(world.0, id, cubes[i - 1]) });
    }
    let knights = walking_knights(&world);
    let view = View::headless(&world, 64, 64);
    let mut ticks = 0;

    // Half the cubes are moved one call each, half in one call for all
    // (`fy_transform_set_many`, as package:fynkel sends a frame's moves),
    // from a buffer kept between frames.
    let half = cubes.len() / 2;
    let mut batch = vec![FyTransform::default(); cubes.len() - half];
    let mut frame = |t: f32, hide: bool| {
        for (i, &id) in cubes.iter().enumerate() {
            let position = FyVec3 { x: i as f32 * 0.01 + t, y: 0.0, z: 0.0 };
            let value = FyTransform {
                position,
                rotation: FyQuat { w: 1.0, ..Default::default() },
                scale: FyVec3 { x: 1.0, y: 1.0, z: 1.0 },
            };
            if i < half {
                // SAFETY: a live world and a pointer to a local.
                ok(unsafe { fy_transform_set(world.0, id, &value) });
            } else {
                batch[i - half] = value;
            }
        }
        // SAFETY: a live world, and slices of the length passed.
        ok(unsafe { fy_transform_set_many(world.0, cubes[half..].as_ptr(), batch.as_ptr(), batch.len()) });
        // Chains and loose cubes hidden and shown again (stage 3 plan
        // §3.3): the flags are kept in a set that has grown to size.
        for &id in cubes.iter().step_by(7) {
            // SAFETY: a live world.
            ok(unsafe { fy_active_set(world.0, id, !hide) });
        }
        knights_change_their_minds(&world, &knights, ticks);
        ticks += 1;
        let mut events = 0;
        // SAFETY: a live world and pointers to locals.
        ok(unsafe { fy_world_animate(world.0, &(1.0 / 60.0), &mut events) });
        view.frame()
    };
    // Warm up the way the frame clock runs: without waiting for the GPU,
    // so all three scene buffers come into use and grow to size, with
    // everything shown. Then hide and show once, for the flags' set.
    for i in 0..50 {
        frame(i as f32, false);
    }
    frame(0.0, true);
    frame(0.0, false);
    view.read(52, 64, 64);
    let mut numbers = Vec::with_capacity(100);
    let allocations = count(|| {
        for i in 0..100 {
            numbers.push(frame(i as f32, i % 2 == 0));
        }
    });
    view.read(*numbers.last().unwrap(), 64, 64);
    assert_eq!(
        allocations, 0,
        "allocations in 100 frames of 1,000 transform writes and 143 flags each, under a sky in fog, with five \
         walking skinned knights, blending and cheering on a layer"
    );
}

/// Flutter's raster thread calls `fy_apple_acquire` once per frame.
#[cfg(target_vendor = "apple")]
#[test]
fn acquiring_the_surface_to_show_allocates_nothing() {
    unsafe extern "C" {
        fn CFRelease(object: *mut std::ffi::c_void);
    }
    let world = World::new();
    stage1(&world);
    let mut view = std::ptr::null_mut();
    // SAFETY: a live world and a local out-value.
    ok(unsafe { fy_view_new_apple(world.0, 32, 32, &mut view) });
    let mut number = 0;
    let acquire = || {
        let mut surface = std::ptr::null_mut();
        // SAFETY: a live view and a local out-value.
        let flags = unsafe { fy_apple_acquire(view, &mut surface) };
        if flags & FY_APPLE_SHOWN != 0 {
            // SAFETY: retained for us by fy_apple_acquire.
            unsafe { CFRelease(surface) };
        }
        flags
    };
    // Warm up: frames finish on a Metal thread, so acquire until one is there.
    for _ in 0..50 {
        // SAFETY: a live view and a local out-value.
        ok(unsafe { fy_view_frame(view, &mut number) });
        while acquire() == 0 {
            std::thread::yield_now();
        }
    }
    let allocations = count(|| {
        for _ in 0..1000 {
            acquire();
        }
    });
    // SAFETY: a live view, not used again.
    unsafe { fy_view_free(view) };
    assert_eq!(allocations, 0, "allocations in 1,000 acquires");
}
