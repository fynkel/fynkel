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

/// None of `allocations` in a run of frames, unless the GPU is GitHub's
/// Macs' virtual one: so slow that the frames waiting for it grow their
/// queues past what was warmed (FYNKEL_VIRTUAL_GPU, set by
/// .github/actions/hosted).
fn no_frame_allocations(allocations: u64, what: &str) {
    if std::env::var_os("FYNKEL_VIRTUAL_GPU").is_none() {
        assert_eq!(allocations, 0, "{what}");
    }
}

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

/// Particle systems stepping and drawn (stage 12): each a fixed pool,
/// stepped in place, extracted into the scene's kept buffers. Ten full
/// systems, half in world space, half moving, bursting each round.
#[test]
fn a_steady_frame_with_particle_systems_allocates_nothing_on_the_calling_thread() {
    let world = World::new();
    stage1(&world);
    let mut material = 0;
    // SAFETY: a live world and pointers to locals.
    ok(unsafe {
        fy_material_add(world.0, &FyMaterial { blend: FY_BLEND_ADDITIVE, ..FyMaterial::default() }, &mut material)
    });
    let mut bursts = [FyParticleBurst::default(); FY_PARTICLE_MAX_BURSTS as usize];
    bursts[0] = FyParticleBurst {
        time: 0.0,
        count: FyRange { min: 50.0, max: 80.0 },
        cycles: 0,
        interval: 0.5,
        probability: 1.0,
    };
    let systems: Vec<u64> = (0..10)
        .map(|i| {
            let id = world.spawn();
            let settings = FyParticleSettings {
                max_particles: 300,
                rate_over_time: 2000.0,
                start_lifetime: FyRange { min: 0.5, max: 1.0 },
                gravity_modifier: 1.0,
                space: if i % 2 == 0 { FY_PARTICLE_SPACE_WORLD } else { FY_PARTICLE_SPACE_LOCAL },
                burst_count: 1,
                bursts,
                has_size_table: true,
                has_color_table: true,
                material,
                ..FyParticleSettings::default()
            };
            // SAFETY: a live world and pointers to locals. Full from the
            // start, so every scene buffer the warm-up uses grows to the
            // size the measured frames need, however the render worker
            // keeps up (the spare buffers go last in, first out).
            ok(unsafe { fy_particles_set(world.0, id, &settings) });
            ok(unsafe { fy_particles_simulate(world.0, id, &1.0) });
            id
        })
        .collect();
    let view = View::headless(&world, 64, 64);
    let frame = |t: f32| {
        for (i, &id) in systems.iter().enumerate().filter(|(i, _)| i % 2 == 0) {
            let value = transform(Transform::from_position(Vec3::new(i as f32, 0.0, t.sin())));
            // SAFETY: a live world and a pointer to a local.
            ok(unsafe { fy_transform_set(world.0, id, &value) });
        }
        let (dt, mut events) = (1.0f32 / 60.0, 0);
        // SAFETY: a live world and pointers to locals.
        ok(unsafe { fy_world_particles_step(world.0, &dt, &dt, &mut events) });
        view.frame()
    };
    for i in 0..120 {
        frame(i as f32 / 60.0);
    }
    // A frame filled while the worker draws one and another waits takes a
    // third scene buffer, new and empty, which grows as it's filled. Left
    // to chance, the worker may first fall that far behind in the measured
    // frames (it did, once in a while, on a busy machine). So frames back
    // to back till some were replaced while waiting: by then all three
    // buffers have been filled.
    let mut warmed = 120;
    for _ in 0..1000 {
        let mut stats = FyViewStats::default();
        // SAFETY: a live view and a pointer to a local.
        ok(unsafe { fy_view_stats(view.0, &mut stats) });
        if stats.coalesced >= 3 {
            break;
        }
        for _ in 0..3 {
            warmed += 1;
            frame(warmed as f32 / 60.0);
        }
    }
    let mut stats = FyViewStats::default();
    // SAFETY: a live view and a pointer to a local.
    ok(unsafe { fy_view_stats(view.0, &mut stats) });
    assert!(stats.coalesced >= 3, "the worker never fell behind: {stats:?}");
    view.read(warmed, 64, 64);
    let mut numbers = Vec::with_capacity(100);
    let allocations = count(|| {
        for i in 0..100 {
            numbers.push(frame(i as f32 / 60.0));
        }
    });
    view.read(*numbers.last().unwrap(), 64, 64);
    no_frame_allocations(allocations, "allocations in 100 frames of ten full particle systems, stepped and drawn");
}

/// A grid of 64 × 64 squares, marked dynamic, its heights changed, its
/// normals worked out and the world given it every frame, and drawn: once
/// the mesh has copies from before to write into, nothing is allocated.
#[test]
fn a_mesh_changed_every_frame_allocates_nothing_on_the_calling_thread() {
    let world = World::new();
    let cube = stage1(&world);
    let n = 65_u32;
    let mut positions: Vec<f32> =
        (0..n * n).flat_map(|i| [(i % n) as f32 * 0.05 - 1.6, 0.0, (i / n) as f32 * 0.05 - 1.6]).collect();
    let mut indices = Vec::new();
    for z in 0..n - 1 {
        for x in 0..n - 1 {
            let a = z * n + x;
            indices.extend([a, a + n, a + 1, a + 1, a + n, a + n + 1]);
        }
    }
    let (mut mesh, mut id) = (std::ptr::null_mut(), 0);
    let mut renderer = FyMeshRenderer::default();
    // SAFETY: a live world, slices, and pointers to locals.
    unsafe {
        ok(fy_mesh_new_empty(&mut mesh));
        ok(fy_mesh_set_positions(mesh, positions.as_ptr(), (n * n) as usize));
        ok(fy_mesh_set_triangles(mesh, indices.as_ptr(), indices.len(), std::ptr::null(), 0));
        ok(fy_mesh_mark_dynamic(mesh));
        ok(fy_mesh_add(world.0, mesh, &mut id));
        ok(fy_mesh_renderer_get(world.0, cube, &mut renderer));
        renderer.mesh = id;
        ok(fy_mesh_renderer_set(world.0, cube, &renderer));
    }
    let view = View::headless(&world, 64, 64);
    let mut frame = |t: f32| {
        for (i, y) in positions.iter_mut().skip(1).step_by(3).enumerate() {
            *y = ((i % 65) as f32 * 0.3 + t).sin() * 0.1;
        }
        // SAFETY: a live mesh and world, and a slice.
        unsafe {
            ok(fy_mesh_set_positions(mesh, positions.as_ptr(), (n * n) as usize));
            ok(fy_mesh_recalculate(mesh, FY_MESH_NORMALS));
            ok(fy_mesh_update(world.0, id, mesh));
        }
        view.frame()
    };
    let mut warmed = 0;
    for i in 0..240 {
        warmed = frame(i as f32 / 60.0);
    }
    view.read(warmed, 64, 64);
    let mut numbers = Vec::with_capacity(100);
    let allocations = count(|| {
        for i in 0..100 {
            numbers.push(frame(i as f32 / 60.0));
        }
    });
    view.read(*numbers.last().unwrap(), 64, 64);
    // SAFETY: made above, freed once.
    unsafe { fy_mesh_free(mesh) };
    no_frame_allocations(allocations, "allocations in 100 frames changing a 4,225-vertex mesh");
}

/// A capped pipe 100 points long, swept again along a moving path, the
/// world given it and drawn every frame (stage 14's WO8): nothing is
/// allocated once warm.
#[test]
fn a_mesh_swept_every_frame_allocates_nothing_on_the_calling_thread() {
    let world = World::new();
    let cube = stage1(&world);
    let ring = |n: usize, r: f32| -> Vec<f32> {
        (0..n)
            .flat_map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                [a.cos() * r, a.sin() * r]
            })
            .collect()
    };
    let mut points = ring(8, 0.1);
    points.extend(ring(5, 0.05));
    let sizes = [8_u32, 5];
    let mut path = vec![0.0_f32; 100 * 10];
    let bend = |path: &mut [f32], t: f32| {
        for (i, p) in path.chunks_exact_mut(10).enumerate() {
            let d = i as f32 * 0.03;
            p.copy_from_slice(&[
                d - 1.5,
                (d * 4.0 + t).sin() * 0.2,
                0.0,
                1.0,
                0.8 * (d * 4.0 + t).cos(),
                0.0,
                0.0,
                1.0,
                0.0,
                d,
            ]);
        }
    };
    bend(&mut path, 0.0);
    let (mut mesh, mut id) = (std::ptr::null_mut(), 0);
    let mut renderer = FyMeshRenderer::default();
    // SAFETY: a live world, slices, and pointers to locals.
    unsafe {
        ok(fy_mesh_sweep(path.as_ptr(), 100, points.as_ptr(), 13, sizes.as_ptr(), 2, true, 1.0, &mut mesh));
        ok(fy_mesh_mark_dynamic(mesh));
        ok(fy_mesh_add(world.0, mesh, &mut id));
        ok(fy_mesh_renderer_get(world.0, cube, &mut renderer));
        renderer.mesh = id;
        ok(fy_mesh_renderer_set(world.0, cube, &renderer));
    }
    let view = View::headless(&world, 64, 64);
    let mut frame = |t: f32| {
        bend(&mut path, t);
        // SAFETY: a live mesh and world, and slices.
        unsafe {
            ok(fy_mesh_set_sweep(mesh, path.as_ptr(), 100, points.as_ptr(), 13, sizes.as_ptr(), 2, true, 1.0));
            ok(fy_mesh_update(world.0, id, mesh));
        }
        view.frame()
    };
    let mut warmed = 0;
    for i in 0..240 {
        warmed = frame(i as f32 / 60.0);
    }
    view.read(warmed, 64, 64);
    let mut numbers = Vec::with_capacity(100);
    let allocations = count(|| {
        for i in 0..100 {
            numbers.push(frame(i as f32 / 60.0));
        }
    });
    view.read(*numbers.last().unwrap(), 64, 64);
    // SAFETY: made above, freed once.
    unsafe { fy_mesh_free(mesh) };
    no_frame_allocations(allocations, "allocations in 100 frames sweeping a pipe again");
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

/// 200 numbers each changed every frame, as hit numbers are, facing the
/// camera, and drawn: once their letters are in the atlas, nothing is
/// allocated.
#[test]
fn a_steady_frame_with_200_numbers_changed_every_frame_allocates_nothing_on_the_calling_thread() {
    let world = World::new();
    stage1(&world);
    let bytes =
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/assets/fonts/Roboto-Regular.ttf")).unwrap();
    let mut font = 0;
    // SAFETY: a live world and pointers to locals.
    ok(unsafe { fy_font_add(world.0, bytes.as_ptr(), bytes.len(), &mut font) });
    let settings = FyTextSettings {
        font,
        size: 0.2,
        align: FY_TEXT_CENTER,
        line_spacing: 1.0,
        color: FyColor { r: 1.0, g: 0.9, b: 0.2, a: 1.0 },
        outline: 0.3,
        outline_color: FyColor { r: 0.0, g: 0.0, b: 0.0, a: 1.0 },
        softness: 0.0,
        shadow_color: FyColor { r: 0.0, g: 0.0, b: 0.0, a: 0.5 },
        shadow_offset: FyVec2 { x: 0.05, y: -0.05 },
        shadow_softness: 0.3,
        facing: FY_FACING_CAMERA,
        on_top: true,
        lit: false,
    };
    let numbers: Vec<u64> = (0..200)
        .map(|i| {
            let id = world.spawn();
            let at =
                transform(Transform::from_position(Vec3::new((i % 20) as f32 * 0.3 - 3.0, (i / 20) as f32 * 0.3, 0.0)));
            let text = b"-1234567890";
            // SAFETY: a live world and pointers to locals.
            ok(unsafe { fy_transform_set(world.0, id, &at) });
            ok(unsafe { fy_text_renderer_set(world.0, id, &settings, text.as_ptr(), text.len()) });
            id
        })
        .collect();
    let view = View::headless(&world, 64, 64);
    // Each number's text written into the same bytes each frame, as Dart
    // writes it into memory it keeps.
    let mut text = [0u8; 8];
    let mut frame = |n: usize| {
        for (i, &id) in numbers.iter().enumerate() {
            let value = (i * 37 + n * 13) % 1000;
            let mut len = 0;
            text[len] = b'-';
            len += 1;
            for d in [value / 100, value / 10 % 10, value % 10] {
                text[len] = b'0' + d as u8;
                len += 1;
            }
            // SAFETY: a live world and a pointer to a local.
            ok(unsafe { fy_text_renderer_set_text(world.0, id, text.as_ptr(), len) });
        }
        view.frame()
    };
    let (mut warmed, mut last) = (0, 0);
    for _ in 0..1000 {
        warmed += 1;
        last = frame(warmed);
        let mut stats = FyViewStats::default();
        // SAFETY: a live view and a pointer to a local.
        ok(unsafe { fy_view_stats(view.0, &mut stats) });
        if warmed >= 120 && stats.coalesced >= 3 {
            break;
        }
    }
    view.read(last, 64, 64);
    let mut drawn = Vec::with_capacity(100);
    let allocations = count(|| {
        for n in 0..100 {
            drawn.push(frame(warmed + 1 + n));
        }
    });
    view.read(*drawn.last().unwrap(), 64, 64);
    no_frame_allocations(allocations, "allocations in 100 frames of 200 numbers changed and drawn");
}

/// A frame with 1,000 sprites of one texture, each moved every frame and
/// drawn: nothing is allocated on the calling thread.
#[test]
fn a_steady_frame_with_1000_moving_sprites_allocates_nothing_on_the_calling_thread() {
    let world = World::new();
    stage1(&world);
    let pixels = [255u8; 4 * 4 * 4];
    let (mut image, mut texture) = (std::ptr::null_mut(), 0);
    // SAFETY: a slice and pointers to locals; the image is freed once added.
    unsafe {
        ok(fy_texture_new_rgba(4, 4, pixels.as_ptr(), pixels.len(), &mut image));
        ok(fy_texture_add(world.0, image, &mut texture));
        fy_texture_free(image);
    }
    let settings = FySpriteSettings {
        texture,
        size: FyVec2 { x: 0.1, y: 0.1 },
        color: FyColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
        flip_x: false,
        flip_y: false,
        pivot: FyVec2 { x: 0.5, y: 0.5 },
        slice: [0.0; 4],
        pixels_per_metre: 100.0,
        alpha_cutoff: 0.0,
        facing: FY_FACING_CAMERA,
        sorting_layer: 0,
        order: 0,
        lit: false,
    };
    let sprites: Vec<u64> = (0..1000)
        .map(|_| {
            let id = world.spawn();
            // SAFETY: a live world and a pointer to a local.
            ok(unsafe { fy_sprite_renderer_set(world.0, id, &settings) });
            id
        })
        .collect();
    let view = View::headless(&world, 64, 64);
    let frame = |n: usize| {
        for (i, &id) in sprites.iter().enumerate() {
            let a = (i + n) as f32 * 0.01;
            let at =
                transform(Transform::from_position(Vec3::new(a.sin() * 3.0, a.cos() * 2.0, (i % 7) as f32 * -0.1)));
            // SAFETY: a live world and a pointer to a local.
            ok(unsafe { fy_transform_set(world.0, id, &at) });
        }
        view.frame()
    };
    let (mut warmed, mut last) = (0, 0);
    for _ in 0..1000 {
        warmed += 1;
        last = frame(warmed);
        let mut stats = FyViewStats::default();
        // SAFETY: a live view and a pointer to a local.
        ok(unsafe { fy_view_stats(view.0, &mut stats) });
        if warmed >= 120 && stats.coalesced >= 3 {
            break;
        }
    }
    view.read(last, 64, 64);
    let mut drawn = Vec::with_capacity(100);
    let allocations = count(|| {
        for n in 0..100 {
            drawn.push(frame(warmed + 1 + n));
        }
    });
    view.read(*drawn.last().unwrap(), 64, 64);
    no_frame_allocations(allocations, "allocations in 100 frames of 1,000 sprites moved and drawn");
}
