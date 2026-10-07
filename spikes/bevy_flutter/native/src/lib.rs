//! Spike: Bevy rendering inside a Flutter `Texture`, driven from Dart
//! (spikes/bevy_flutter/README.md).
//!
//! A worker thread owns the Bevy `App` (no window, no winit, no pipelined
//! rendering). Dart calls `bf_frame` once per vsync with every cube's
//! transform; the call copies them and wakes the worker, never waits. The
//! worker reserves a ring slot, points the camera's `ManualTextureView` at
//! it and runs `app.update()`, which renders and submits. A command buffer
//! committed behind the frame marks the slot Ready when the GPU is done and
//! signals Flutter.

mod ring;

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use bevy::app::PluginsState;
use bevy::camera::{ManualTextureViewHandle, RenderTarget};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::render::renderer::{PendingCommandBuffers, RenderDevice, RenderGraph, RenderGraphSystems};
use bevy::render::texture::{ManualTextureView, ManualTextureViews};
use bevy::render::RenderApp;
use bevy::world_serialization::WorldInstanceReady;
use objc2_metal::MTLCommandBuffer;

use ring::{FrameReadyFn, Reservation, Ring};

const VIEW: ManualTextureViewHandle = ManualTextureViewHandle(0);

// ------------------------------------------------------------- shared state

#[derive(Default)]
struct Input {
    /// x, y, z, yaw per cube.
    transforms: Vec<f32>,
    tick: bool,
    resize: Option<(u32, u32)>,
    quit: bool,
}

#[derive(Default)]
struct Stats {
    frames: u64,
    dropped: u64,
    coalesced: u64,
    update_us: Vec<u32>,
    gpu_us: Vec<u32>,
}

struct Shared {
    input: Mutex<Input>,
    wake: Condvar,
    /// The worker's answers to create/resize: Ok(()) or an error.
    reply: Mutex<Option<Result<(), String>>>,
    replied: Condvar,
    ring: Mutex<Option<Arc<Ring>>>,
    stats: Arc<Mutex<Stats>>,
}

pub struct Ctx {
    shared: Arc<Shared>,
    ring: Arc<Ring>,
    worker: Option<JoinHandle<()>>,
}

// ------------------------------------------------------------- bevy side

#[derive(Resource, Default)]
struct CubeInput(Vec<f32>);

#[derive(Component)]
struct Cube(usize);

#[derive(Resource)]
struct CubeCount(usize);

#[derive(Resource, Default)]
struct Character {
    gltf: Handle<Gltf>,
}

/// The slot the frame being rendered goes to; taken by the render world's
/// completion hook.
#[derive(Resource, Clone)]
struct FrameSlot(Arc<Mutex<Option<(usize, u64, u64)>>>);

#[derive(Resource, Clone)]
struct RingHandle(Arc<Mutex<Option<Arc<Ring>>>>);

#[derive(Resource, Clone)]
struct GpuEdges {
    starts: Arc<Mutex<HashMap<u64, f64>>>,
    stats: Arc<Mutex<Stats>>,
}

fn build_app(assets: String, cubes: usize, shared: &Shared) -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(AssetPlugin {
                file_path: assets,
                ..default()
            })
            .set(bevy::log::LogPlugin {
                level: bevy::log::Level::WARN,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    )
    .insert_resource(CubeInput(vec![0.0; cubes * 4]))
    .insert_resource(CubeCount(cubes))
    .init_resource::<Character>()
    .insert_resource(ClearColor(Color::linear_rgb(0.45, 0.6, 0.85)))
    .insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    })
    .add_systems(Startup, setup)
    .add_systems(Update, apply_cubes);

    let slot = FrameSlot(Arc::new(Mutex::new(None)));
    let edges = GpuEdges {
        starts: Default::default(),
        stats: shared.stats.clone(),
    };
    app.insert_resource(slot.clone());
    let render = app.sub_app_mut(RenderApp);
    render
        .insert_resource(slot)
        .insert_resource(RingHandle(Arc::new(Mutex::new(None))))
        .insert_resource(edges)
        .add_systems(
            RenderGraph,
            (
                gpu_begin.in_set(RenderGraphSystems::Begin),
                gpu_end
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            ),
        );
    app
}

fn setup(
    mut commands: Commands,
    count: Res<CubeCount>,
    assets: Res<AssetServer>,
    mut character: ResMut<Character>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Off,
        RenderTarget::TextureView(VIEW),
        Transform::from_xyz(0.0, 9.0, 22.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::IDENTITY.looking_to(Vec3::new(-0.3, -1.0, -0.25).normalize(), Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(80.0, 80.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.55, 0.5),
            perceptual_roughness: 0.9,
            ..default()
        })),
    ));
    let cube = meshes.add(Cuboid::from_length(0.4));
    let palette: Vec<_> = (0..8)
        .map(|i| {
            materials.add(StandardMaterial {
                base_color: Color::hsl(i as f32 * 45.0, 0.6, 0.55),
                perceptual_roughness: 0.6,
                ..default()
            })
        })
        .collect();
    for i in 0..count.0 {
        commands.spawn((
            Cube(i),
            Mesh3d(cube.clone()),
            MeshMaterial3d(palette[i % palette.len()].clone()),
            Transform::from_xyz(0.0, -10.0, 0.0),
        ));
    }
    character.gltf = assets.load("character.glb");
    commands
        .spawn((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset("character.glb"))),
            Transform::from_xyz(0.0, 0.0, 8.0).with_scale(Vec3::splat(1.5)),
        ))
        .observe(start_walk);
}

fn start_walk(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    character: Res<Character>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Some(gltf) = gltfs.get(&character.gltf) else {
        return;
    };
    let Some(walk) = gltf.named_animations.get("walk").cloned() else {
        return;
    };
    let (graph, index) = AnimationGraph::from_clip(walk);
    let graph = graphs.add(graph);
    for child in children.iter_descendants(ready.entity) {
        if let Ok(mut player) = players.get_mut(child) {
            player.play(index).repeat();
            commands.entity(child).insert(AnimationGraphHandle(graph.clone()));
        }
    }
}

fn apply_cubes(input: Res<CubeInput>, mut cubes: Query<(&Cube, &mut Transform)>) {
    if !input.is_changed() {
        return;
    }
    for (cube, mut t) in &mut cubes {
        let o = cube.0 * 4;
        if let Some(v) = input.0.get(o..o + 4) {
            t.translation = Vec3::new(v[0], v[1], v[2]);
            t.rotation = Quat::from_rotation_y(v[3]);
        }
    }
}

// ------------------------------------------------------------- GPU hooks

fn gpu_begin(
    slot: Res<FrameSlot>,
    edges: Res<GpuEdges>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let Some((_, _, seq)) = *slot.0.lock().unwrap() else {
        return;
    };
    let starts = edges.starts.clone();
    let mut encoder = device.create_command_encoder(&default());
    hook(&mut encoder, move |cb| {
        starts.lock().unwrap().insert(seq, cb.GPUStartTime());
    });
    pending.push([encoder.finish()]);
}

fn gpu_end(
    slot: Res<FrameSlot>,
    ring: Res<RingHandle>,
    edges: Res<GpuEdges>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let Some((index, generation, seq)) = slot.0.lock().unwrap().take() else {
        return;
    };
    let Some(ring) = ring.0.lock().unwrap().clone() else {
        return;
    };
    let edges = edges.clone();
    let mut encoder = device.create_command_encoder(&default());
    hook(&mut encoder, move |cb| {
        if let Some(start) = edges.starts.lock().unwrap().remove(&seq) {
            let end = cb.GPUEndTime();
            if start > 0.0 && end >= start {
                edges
                    .stats
                    .lock()
                    .unwrap()
                    .gpu_us
                    .push(((end - start) * 1e6) as u32);
            }
        }
        ring.complete(index, generation, seq);
    });
    pending.push([encoder.finish()]);
}

/// Attaches `done` as the Metal completion handler of the encoder's command
/// buffer; wgpu still ends and commits it.
fn hook(
    encoder: &mut wgpu::CommandEncoder,
    done: impl Fn(&objc2::runtime::ProtocolObject<dyn objc2_metal::MTLCommandBuffer>) + 'static,
) {
    use block2::RcBlock;
    use objc2::runtime::ProtocolObject;

    unsafe {
        encoder.as_hal_mut::<wgpu::hal::api::Metal, _, _>(|hal| {
            let Some(cb) = hal.and_then(|h| h.raw_command_buffer()) else {
                return;
            };
            let block = RcBlock::new(move |cb: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                done(cb.as_ref());
            });
            cb.addCompletedHandler(RcBlock::as_ptr(&block));
        });
    }
}

// ------------------------------------------------------------- worker

fn worker(shared: Arc<Shared>, assets: String, cubes: usize, width: u32, height: u32) {
    let mut app = build_app(assets, cubes, &shared);
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();

    let device = app.world().resource::<RenderDevice>().wgpu_device().clone();
    let ring = match Ring::new(&device, width, height) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            reply(&shared, Err(e));
            return;
        }
    };
    *shared.ring.lock().unwrap() = Some(ring.clone());
    *app
        .sub_app(RenderApp)
        .world()
        .resource::<RingHandle>()
        .0
        .lock()
        .unwrap() = Some(ring.clone());
    let slot = app.world().resource::<FrameSlot>().clone();
    reply(&shared, Ok(()));

    loop {
        let (transforms, resize) = {
            let mut input = shared.input.lock().unwrap();
            while !input.tick && input.resize.is_none() && !input.quit {
                input = shared.wake.wait(input).unwrap();
            }
            if input.quit {
                break;
            }
            let ticked = std::mem::take(&mut input.tick);
            (
                ticked.then(|| std::mem::take(&mut input.transforms)),
                input.resize.take(),
            )
        };
        if let Some((w, h)) = resize {
            reply(&shared, ring.resize(w, h));
        }
        let Some(transforms) = transforms else {
            continue;
        };
        {
            let mut input = app.world_mut().resource_mut::<CubeInput>();
            if transforms.len() == input.0.len() {
                input.0.copy_from_slice(&transforms);
            }
        }
        // Hand the buffer back for the next frame's fill.
        {
            let mut input = shared.input.lock().unwrap();
            if input.transforms.is_empty() {
                input.transforms = transforms;
            }
        }
        let Some(r) = ring.reserve() else {
            shared.stats.lock().unwrap().dropped += 1;
            continue;
        };
        set_target(&mut app, &r);
        *slot.0.lock().unwrap() = Some((r.index, r.generation, r.seq));
        let t0 = Instant::now();
        app.update();
        let us = t0.elapsed().as_micros() as u32;
        if slot.0.lock().unwrap().take().is_some() {
            // The render world never reached the hook (nothing rendered).
            ring.cancel(&r);
        }
        let mut stats = shared.stats.lock().unwrap();
        stats.frames += 1;
        stats.update_us.push(us);
    }
    drop(app);
}

fn set_target(app: &mut App, r: &Reservation) {
    let mut views = app.world_mut().resource_mut::<ManualTextureViews>();
    views.insert(
        VIEW,
        ManualTextureView {
            texture_view: r.view.clone().into(),
            size: UVec2::new(r.width, r.height),
            view_format: ring::FORMAT,
        },
    );
}

fn reply(shared: &Shared, result: Result<(), String>) {
    *shared.reply.lock().unwrap() = Some(result);
    shared.replied.notify_all();
}

fn wait_reply(shared: &Shared) -> Result<(), String> {
    let mut reply = shared.reply.lock().unwrap();
    loop {
        if let Some(r) = reply.take() {
            return r;
        }
        reply = shared.replied.wait(reply).unwrap();
    }
}

// ------------------------------------------------------------- C ABI

static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

fn set_error(e: String) {
    eprintln!("[bevy_flutter] {e}");
    *LAST_ERROR.lock().unwrap() = e;
}

/// Starts Bevy on its own thread and returns once its first ring exists;
/// NULL on failure (`bf_last_error`).
#[unsafe(no_mangle)]
pub extern "C" fn bf_create(width: u32, height: u32, assets: *const c_char, cubes: u32) -> *mut Ctx {
    let assets = unsafe { CStr::from_ptr(assets) }.to_string_lossy().into_owned();
    let shared = Arc::new(Shared {
        input: Mutex::new(Input::default()),
        wake: Condvar::new(),
        reply: Mutex::new(None),
        replied: Condvar::new(),
        ring: Mutex::new(None),
        stats: Default::default(),
    });
    let worker_shared = shared.clone();
    let cubes = cubes as usize;
    let worker = std::thread::Builder::new()
        .name("bevy".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                worker(worker_shared.clone(), assets, cubes, width.max(1), height.max(1))
            }));
            if let Err(p) = result {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
                reply(&worker_shared, Err(format!("bevy worker panicked: {msg}")));
            }
        })
        .expect("spawn bevy thread");
    if let Err(e) = wait_reply(&shared) {
        set_error(e);
        let _ = worker.join();
        return std::ptr::null_mut();
    }
    let ring = shared.ring.lock().unwrap().clone().unwrap();
    Box::into_raw(Box::new(Ctx {
        shared,
        ring,
        worker: Some(worker),
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn bf_last_error(out: *mut c_char, cap: u32) -> u32 {
    let e = LAST_ERROR.lock().unwrap();
    let n = e.len().min(cap.saturating_sub(1) as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(e.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    n as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn bf_surface_count(ctx: *mut Ctx) -> u32 {
    unsafe { &*ctx }.ring.surfaces().len() as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn bf_surface(ctx: *mut Ctx, index: u32) -> *mut c_void {
    unsafe { &*ctx }
        .ring
        .surfaces()
        .get(index as usize)
        .copied()
        .unwrap_or(std::ptr::null_mut())
}

/// Flutter's raster thread.
#[unsafe(no_mangle)]
pub extern "C" fn bf_acquire(ctx: *mut c_void) -> i32 {
    unsafe { &*(ctx as *mut Ctx) }.ring.acquire()
}

#[unsafe(no_mangle)]
pub extern "C" fn bf_set_frame_ready_callback(
    ctx: *mut c_void,
    func: Option<FrameReadyFn>,
    userdata: *mut c_void,
) {
    unsafe { &*(ctx as *mut Ctx) }.ring.set_callback(func, userdata);
}

/// 1: present finished frames in order (default); 0: always the newest.
#[unsafe(no_mangle)]
pub extern "C" fn bf_set_queue(ctx: *mut Ctx, on: i32) {
    unsafe { &*ctx }.ring.set_queue(on != 0);
}

/// One frame: copies `count` × (x, y, z, yaw) and wakes the worker. Never
/// waits; a frame still rendering makes this one coalesce into the next.
#[unsafe(no_mangle)]
pub extern "C" fn bf_frame(ctx: *mut Ctx, transforms: *const f32, count: u32) {
    let ctx = unsafe { &*ctx };
    let src = unsafe { std::slice::from_raw_parts(transforms, count as usize * 4) };
    let mut input = ctx.shared.input.lock().unwrap();
    if input.tick {
        ctx.shared.stats.lock().unwrap().coalesced += 1;
    }
    input.transforms.clear();
    input.transforms.extend_from_slice(src);
    input.tick = true;
    ctx.shared.wake.notify_one();
}

/// Rebuilds the ring at the new size; 0 on success. The caller re-wraps the
/// surfaces (`bf_surface`).
#[unsafe(no_mangle)]
pub extern "C" fn bf_resize(ctx: *mut Ctx, width: u32, height: u32) -> i32 {
    let ctx = unsafe { &*ctx };
    {
        let mut input = ctx.shared.input.lock().unwrap();
        input.resize = Some((width.max(1), height.max(1)));
        ctx.shared.wake.notify_one();
    }
    match wait_reply(&ctx.shared) {
        Ok(()) => 0,
        Err(e) => {
            set_error(e);
            -1
        }
    }
}

/// Writes [frames, dropped, coalesced, presented, update ms p50, p95,
/// gpu ms p50, p95] and resets the counters. Returns how many were written.
#[unsafe(no_mangle)]
pub extern "C" fn bf_stats(ctx: *mut Ctx, out: *mut f64, cap: u32) -> u32 {
    let ctx = unsafe { &*ctx };
    let mut s = std::mem::take(&mut *ctx.shared.stats.lock().unwrap());
    let pct = |v: &mut Vec<u32>, q: f64| -> f64 {
        if v.is_empty() {
            return 0.0;
        }
        v.sort_unstable();
        v[((v.len() - 1) as f64 * q).round() as usize] as f64 / 1000.0
    };
    let values = [
        s.frames as f64,
        s.dropped as f64,
        s.coalesced as f64,
        ctx.ring.take_presented() as f64,
        pct(&mut s.update_us, 0.5),
        pct(&mut s.update_us, 0.95),
        pct(&mut s.gpu_us, 0.5),
        pct(&mut s.gpu_us, 0.95),
    ];
    let n = values.len().min(cap as usize);
    unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), out, n) };
    n as u32
}

/// Stops the worker and drops Bevy. Unregister the Flutter texture first.
#[unsafe(no_mangle)]
pub extern "C" fn bf_destroy(ctx: *mut Ctx) {
    if ctx.is_null() {
        return;
    }
    let mut ctx = unsafe { Box::from_raw(ctx) };
    {
        let mut input = ctx.shared.input.lock().unwrap();
        input.quit = true;
        ctx.shared.wake.notify_one();
    }
    if let Some(w) = ctx.worker.take() {
        let _ = w.join();
    }
}
