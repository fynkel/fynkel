//! `fynkel_platform_apple` — the macOS + iOS viewport (blueprint §8.4,
//! ADR-0002 Path A, ADR-0009: one Metal interop for both Apple targets): `fynkel_render` drawing the core's extract into an
//! [`IoSurfaceRing`] that a Flutter `Texture` presents without a copy.
//!
//! This dylib also links `fynkel_native`, so a Flutter host loads **one**
//! library that exports both the engine ABI (`fynkel_*`, `include/fynkel.h`
//! in `crates/fynkel_native`) and the viewport ABI (`fynkel_apple_*`,
//! `include/fynkel_apple.h`). The viewport reads the world's extract
//! through the same `FynkelWorld*` the SDK holds; the host never sees a
//! renderer object.
//!
//! Threading: `create`/`resize`/`render`/`destroy` are host-loop calls
//! (Flutter's platform thread); `acquire_presented` is the presenter's
//! (raster thread) only entry point; the frame-ready callback fires on a
//! Metal completion thread and must only signal.

pub mod ring;
mod trace;

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use fynkel_core::components::{Camera as SceneCamera, ExposureMode, Projection};
use fynkel_core::{Entity, Extract, ExtractCamera};
use fynkel_native::FynkelWorld;
use fynkel_render::{Assets, Camera, ExtractRenderer, FrameStats};
use glam::Vec3;

pub use ring::{DeviceChoice, Gpu, IoSurfaceRing, RingStats, SyncMode, TEXTURE_FORMAT};

/// The render worker of a pipelined viewport (`fynkel_apple_viewport_set_pipelined`):
/// one thread that runs the frames' `prepare`, encode and submit while the
/// host thread goes on to the next frame's simulation. At most one job is in
/// flight; every call that touches the renderer first waits for it
/// ([`Viewport::quiesce`]), so the renderer is never used by two threads.
struct Pipeline {
    jobs: std::sync::mpsc::Sender<Job>,
    state: Arc<PipeState>,
    thread: Option<std::thread::JoinHandle<()>>,
}

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct PipeState {
    /// A job is queued or running.
    busy: Mutex<bool>,
    idle: Condvar,
    /// What the last finished job answered (`render`'s slot, -1 dropped, ...).
    last: AtomicI32,
    /// The last frame left assets to load.
    assets_pending: AtomicBool,
    /// The extract the last job used, handed back so its buffers are reused.
    spare: Mutex<Option<Extract>>,
    /// A panic inside a job; it breaks the viewport at the next call.
    panic: Mutex<Option<String>>,
}

/// Raw pointers a job carries to the viewport's ring and renderer. Sound
/// because the host thread never touches either while a job is in flight
/// (`quiesce`) and the viewport never moves (it lives in a `Box`).
struct JobPtrs {
    ring: *const IoSurfaceRing,
    renderer: *mut ExtractRenderer,
}
unsafe impl Send for JobPtrs {}

impl Pipeline {
    fn new() -> Self {
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("fynkel viewport render".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
            .expect("render worker thread");
        Self {
            jobs,
            state: Arc::new(PipeState::default()),
            thread: Some(thread),
        }
    }

    fn wait_idle(&self) {
        let mut busy = self.state.busy.lock().unwrap();
        while *busy {
            busy = self.state.idle.wait(busy).unwrap();
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.wait_idle();
        // Dropping the sender ends the worker's loop.
        let (dead, _) = std::sync::mpsc::channel();
        drop(std::mem::replace(&mut self.jobs, dead));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Ring + renderer, on the GPU the process's viewports share (ADR-0030 §4).
pub struct Viewport {
    ring: IoSurfaceRing,
    renderer: ExtractRenderer,
    extract: Extract,
    /// This viewport's own camera, drawn instead of the world's (the
    /// editor's Scene view on a running game, ADR-0021 P7 L5).
    view: Option<(glam::Mat4, SceneCamera)>,
    /// One of the world's cameras this viewport looks through instead
    /// (`fynkel_apple_viewport_set_view_camera`); exclusive with `view`.
    view_camera: Option<Entity>,
    /// The render worker, once `set_pipelined` turned it on.
    pipeline: Option<Pipeline>,
    /// `renderer.placeholders`, readable while the worker has the renderer.
    placeholders: bool,
    /// Set by the first panic inside a viewport call (foundation 4.3):
    /// every call fails from then on, `destroy` still frees it.
    broken: OnceLock<String>,
}

impl Drop for Viewport {
    fn drop(&mut self) {
        // The worker holds pointers into the ring and renderer, which drop
        // before the `pipeline` field would.
        self.pipeline = None;
    }
}

impl Viewport {
    pub fn new(
        width: u32,
        height: u32,
        buffer_count: u32,
        sync_mode: SyncMode,
    ) -> Result<Self, String> {
        let ring = IoSurfaceRing::new(width, height, buffer_count, sync_mode)?;
        Ok(Self::around(ring, width, height))
    }

    /// A viewport on `gpu` (the measurement's own-device baseline; the
    /// host's viewports come from [`Viewport::new`]).
    pub fn with_gpu(
        gpu: std::sync::Arc<Gpu>,
        width: u32,
        height: u32,
        buffer_count: u32,
        sync_mode: SyncMode,
    ) -> Result<Self, String> {
        let ring = IoSurfaceRing::with_gpu(gpu, width, height, buffer_count, sync_mode)?;
        Ok(Self::around(ring, width, height))
    }

    fn around(ring: IoSurfaceRing, width: u32, height: u32) -> Self {
        let renderer = ExtractRenderer::new(ring.device(), TEXTURE_FORMAT, width, height);
        Self {
            ring,
            placeholders: renderer.placeholders,
            renderer,
            extract: Extract::default(),
            view: None,
            view_camera: None,
            pipeline: None,
            broken: OnceLock::new(),
        }
    }

    /// Waits for the render worker's job in flight, if any; the renderer is
    /// the host thread's again afterwards. A panic the job hit breaks the
    /// viewport here.
    fn quiesce(&self) {
        if let Some(p) = &self.pipeline {
            p.wait_idle();
            if let Some(text) = p.state.panic.lock().unwrap().take() {
                self.break_with(Box::new(text));
            }
        }
    }

    /// Turns the render worker on or off. Off waits for it and joins it.
    pub fn set_pipelined(&mut self, enabled: bool) {
        self.quiesce();
        match (enabled, self.pipeline.is_some()) {
            (true, false) => self.pipeline = Some(Pipeline::new()),
            (false, true) => self.pipeline = None,
            _ => {}
        }
    }

    /// Why the viewport is broken (a panic inside one of its calls,
    /// foundation 4.3), or `None` while it works.
    pub fn error(&self) -> Option<&str> {
        self.broken.get().map(String::as_str)
    }

    fn break_with(&self, payload: Box<dyn std::any::Any + Send>) {
        let msg = panic_text(payload);
        log::error!(
            "fynkel_platform_apple: panic caught at the viewport ABI, viewport broken: {msg}"
        );
        let _ = self.broken.set(msg);
    }

    pub fn ring(&self) -> &IoSurfaceRing {
        &self.ring
    }

    pub fn camera(&self) -> Camera {
        self.renderer.camera
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.renderer.camera = camera;
    }

    /// Draws every frame through `view` (a camera at `world`) instead of
    /// the world's active camera; `None` hands the picture back to it.
    pub fn set_view(&mut self, view: Option<(glam::Mat4, SceneCamera)>) {
        self.view = view;
        self.view_camera = None;
    }

    /// Draws every frame through the world's camera on `entity` (its pose
    /// in the frame's extract) instead of the one the frame picks; `None`
    /// hands the picture back. Replaces a [`Viewport::set_view`].
    pub fn set_view_camera(&mut self, entity: Option<Entity>) {
        self.view_camera = entity;
        self.view = None;
    }

    pub fn view_camera(&self) -> Option<Entity> {
        self.view_camera
    }

    /// The viewport's own camera into `extract`: a view matrix, or one of
    /// the extract's cameras by entity. False when that camera is not in
    /// it (nothing is drawn).
    fn aim(
        extract: &mut Extract,
        view: Option<(glam::Mat4, SceneCamera)>,
        view_camera: Option<Entity>,
    ) -> bool {
        if let Some((world, camera)) = view {
            extract.camera = Some(ExtractCamera {
                record: u32::MAX,
                world,
                camera,
            });
        }
        if let Some(entity) = view_camera {
            let found = extract
                .cameras
                .iter()
                .find(|c| extract.records.get(c.record as usize).map(|r| r.entity) == Some(entity))
                .copied();
            match found {
                Some(c) => extract.camera = Some(c),
                None => return false,
            }
        }
        true
    }

    pub fn view(&self) -> Option<(glam::Mat4, SceneCamera)> {
        self.view
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.ring.gpu().is_lost() {
            return Err("GPU device lost; recreate the viewport".into());
        }
        self.ring.resize(width, height)?;
        self.renderer.resize(self.ring.device(), width, height);
        Ok(())
    }

    /// Renders `extract` into a free slot; see [`IoSurfaceRing::render_with`].
    /// `extract_us` is how long the caller spent producing the extract
    /// (recorded with the frame's other stage times, M3-16).
    pub fn render(&mut self, extract: &Extract) -> i32 {
        self.render_timed(extract, 0)
    }

    pub fn render_timed(&mut self, extract: &Extract, extract_us: u32) -> i32 {
        if self.ring.gpu().is_lost() {
            return -5;
        }
        let Viewport { ring, renderer, .. } = self;
        Self::render_frame(ring, renderer, |renderer| {
            let start = std::time::Instant::now();
            renderer.prepare(ring.device(), ring.queue(), extract);
            Ok((extract_us, start.elapsed().as_micros() as u32))
        })
    }

    /// One frame through the render worker: the extract is taken here, on the
    /// host thread (it reads the world), and everything after it — admission,
    /// `prepare`, encode, submit — runs on the worker while the host goes on.
    /// Answers what the *previous* frame's job answered (a slot, -1 dropped,
    /// -4 failed); the first call answers 0.
    fn render_pipelined(&mut self, w: &FynkelWorld) -> i32 {
        let pipeline = self.pipeline.as_ref().expect("pipelined");
        let state = pipeline.state.clone();
        if let Some(text) = state.panic.lock().unwrap().take() {
            self.break_with(Box::new(text));
            return FYNKEL_APPLE_RENDER_BROKEN;
        }
        // Hand the job its extract: the buffers of an earlier one when back.
        let mut extract = state.spare.lock().unwrap().take().unwrap_or_default();
        let start = std::time::Instant::now();
        w.core().extract_into(&mut extract, self.placeholders);
        if !Self::aim(&mut extract, self.view, self.view_camera) {
            *state.spare.lock().unwrap() = Some(extract);
            return FYNKEL_APPLE_RENDER_NO_CAMERA;
        }
        let extract_us = start.elapsed().as_micros() as u32;
        let assets = w.assets().cloned();
        // Normally the previous job finished while the host simulated; when
        // the host is faster than the worker this is where it waits.
        pipeline.wait_idle();
        let answered = state.last.load(Ordering::Acquire);
        let ptrs = JobPtrs {
            ring: &self.ring,
            renderer: &mut self.renderer,
        };
        *state.busy.lock().unwrap() = true;
        let job_state = state.clone();
        let job: Job = Box::new(move || {
            let ptrs = ptrs;
            let result = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: see `JobPtrs`.
                let (ring, renderer) = unsafe { (&*ptrs.ring, &mut *ptrs.renderer) };
                let slot = Viewport::render_frame(ring, renderer, |renderer| {
                    if let Some(assets) = assets {
                        renderer.set_assets(Some(assets));
                    }
                    let start = std::time::Instant::now();
                    renderer.prepare(ring.device(), ring.queue(), &extract);
                    Ok((extract_us, start.elapsed().as_micros() as u32))
                });
                job_state
                    .assets_pending
                    .store(renderer.assets_pending(), Ordering::Release);
                slot
            }));
            *job_state.spare.lock().unwrap() = Some(extract);
            match result {
                Ok(slot) => job_state.last.store(slot, Ordering::Release),
                Err(payload) => {
                    *job_state.panic.lock().unwrap() = Some(panic_text(payload));
                    job_state
                        .last
                        .store(FYNKEL_APPLE_RENDER_BROKEN, Ordering::Release);
                }
            }
            *job_state.busy.lock().unwrap() = false;
            job_state.idle.notify_all();
        });
        if pipeline.jobs.send(job).is_err() {
            *state.busy.lock().unwrap() = false;
            return -4;
        }
        answered
    }

    fn render_frame(
        ring: &IoSurfaceRing,
        renderer: &mut ExtractRenderer,
        prepare: impl FnOnce(&mut ExtractRenderer) -> Result<(u32, u32), String>,
    ) -> i32 {
        let timed_before = renderer.stats().gpu_timed;
        let mut stages = None;
        let result = ring.try_render_with(|device, _queue, view| {
            stages = Some(prepare(renderer)?);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fynkel viewport frame"),
            });
            renderer.draw(&mut encoder, view);
            Ok::<_, String>(encoder.finish())
        });
        if let Some((extract_us, prepare_us)) = stages {
            let s = renderer.stats();
            let gpu = (s.gpu_timed != timed_before).then_some((
                s.gpu_shadow_us,
                s.gpu_ao_us,
                s.gpu_scene_us,
                s.gpu_post_us,
                s.gpu_frame_us,
            ));
            ring.record_stage_times(extract_us, prepare_us, s.encode_us, gpu);
            ring.record_prepare_phases(
                s.prepare_phases_us,
                s.visibility_phases_us,
                s.visibility_update_kind,
            );
        }
        if matches!(result, Ok(-1)) {
            // One-attempt inputs must not accumulate or reappear on a later
            // admitted frame just because this attempt skipped preparation.
            renderer.debug().clear();
            renderer.overlay().clear();
            renderer.clear_extra_worlds();
        }
        match result {
            Ok(slot) => slot,
            Err(error) => {
                log::warn!("viewport capture unavailable: {error}");
                -4
            }
        }
    }

    pub fn instance_count(&self) -> u32 {
        self.renderer.instance_count()
    }

    pub fn frame_stats(&self) -> FrameStats {
        self.renderer.stats()
    }

    /// Points the renderer at a cooked cache (`fynkel cook`), or at nothing.
    /// A world that loaded its scene from a cache brings its own store
    /// along (see [`fynkel_apple_viewport_render`]); this is for viewports
    /// that draw without one.
    pub fn set_asset_cache(&mut self, dir: Option<&std::path::Path>) -> Result<(), String> {
        let assets = match dir {
            Some(d) => Some(Assets::open(d).map_err(|e| e.message)?),
            None => None,
        };
        self.renderer.set_assets(assets);
        Ok(())
    }

    pub fn set_placeholders(&mut self, enabled: bool) {
        self.placeholders = enabled;
        self.renderer.placeholders = enabled;
    }

    /// Selects the project's quality profile by name (`None`: the project
    /// default); see [`ExtractRenderer::set_quality_profile`].
    pub fn set_quality(&mut self, profile: Option<&str>) -> bool {
        self.renderer.set_quality_profile(profile)
    }

    pub fn quality(&self) -> fynkel_render::QualityStatus {
        self.renderer.quality()
    }

    pub fn asset_errors(&self) -> &[String] {
        self.renderer.asset_errors()
    }
}

// ---------------------------------------------------------------------------
// C ABI (`include/fynkel_apple.h`)
// ---------------------------------------------------------------------------

/// Camera description crossing the ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FynkelAppleCamera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_y_radians: f32,
    pub near: f32,
    pub far: f32,
}

impl From<FynkelAppleCamera> for Camera {
    fn from(c: FynkelAppleCamera) -> Self {
        Camera {
            eye: Vec3::from(c.eye),
            target: Vec3::from(c.target),
            up: Vec3::from(c.up),
            fov_y_radians: c.fov_y_radians,
            near: c.near,
            far: c.far,
        }
    }
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct FynkelAppleViewportStats {
    pub frames_rendered: u64,
    pub frames_dropped_no_free_buffer: u64,
    pub frames_presented: u64,
    pub presents_without_new_frame: u64,
    pub cpu_p50_us: u32,
    pub cpu_p95_us: u32,
    pub cpu_p99_us: u32,
    pub gpu_complete_p50_us: u32,
    pub gpu_complete_p95_us: u32,
    pub gpu_complete_p99_us: u32,
    pub resizes: u64,
    pub resize_p50_us: u32,
    pub resize_max_us: u32,
    pub present_lag_p50: u32,
    pub present_lag_p95: u32,
    pub present_lag_max: u32,
    pub acquire_wait_p50_us: u32,
    pub acquire_wait_p95_us: u32,
    pub acquire_wait_max_us: u32,
    pub instances_last_frame: u32,
    pub frame_ready_signals: u32,
    pub meshes_last_frame: u32,
    pub placeholders_last_frame: u32,
    pub missing_last_frame: u32,
    pub asset_errors: u32,
    pub scene_camera: u32,
    /// Last frame (M3-05): blend instances, lights kept / dropped, sun
    /// shadow casters and the map side (0 when none), samples per pixel,
    /// the internal render size.
    pub transparent_last_frame: u32,
    pub lights: u32,
    pub lights_dropped: u32,
    pub shadow_casters: u32,
    pub shadow_map: u32,
    pub msaa: u32,
    pub render_width: u32,
    pub render_height: u32,
    /// Last frame (M3-06): instances outside the camera frustum (not
    /// drawn), mesh draw calls in the scene pass, draw calls in the shadow
    /// pass.
    pub culled_last_frame: u32,
    pub draws_last_frame: u32,
    pub shadow_draws_last_frame: u32,
    /// Instrumentation (M3-16, blueprint §15.4), over the window: the
    /// world's extract, the renderer's prepare (upload + cull) and encode
    /// (pass recording) on the render call — `cpu_*` above is encode +
    /// submit —; the GPU's pass times from Metal timestamps when the
    /// device samples them (`gpu_timestamps`; the values trail the draw by
    /// three or four frames and count only the frames read back,
    /// `gpu_frames_timed`): shadow, scene, post (+ present), and the
    /// frame's span from the first pass's begin to the last pass's end
    /// (the passes overlap on a tile-based GPU, so the sum exceeds it);
    /// the renderer's GPU memory estimate.
    pub extract_p50_us: u32,
    pub extract_p95_us: u32,
    pub extract_p99_us: u32,
    pub prepare_p50_us: u32,
    pub prepare_p95_us: u32,
    pub prepare_p99_us: u32,
    pub encode_p50_us: u32,
    pub encode_p95_us: u32,
    pub encode_p99_us: u32,
    pub gpu_timestamps: u32,
    pub gpu_frames_timed: u32,
    pub gpu_frames_untimed: u32,
    pub gpu_shadow_p50_us: u32,
    pub gpu_shadow_p95_us: u32,
    pub gpu_shadow_p99_us: u32,
    /// The ambient-occlusion stage (R2): the depth prepass, SSAO and its
    /// blur, timed as one span.
    pub gpu_ao_p50_us: u32,
    pub gpu_ao_p95_us: u32,
    pub gpu_ao_p99_us: u32,
    pub gpu_scene_p50_us: u32,
    pub gpu_scene_p95_us: u32,
    pub gpu_scene_p99_us: u32,
    pub gpu_post_p50_us: u32,
    pub gpu_post_p95_us: u32,
    pub gpu_post_p99_us: u32,
    pub gpu_frame_p50_us: u32,
    pub gpu_frame_p95_us: u32,
    pub gpu_frame_p99_us: u32,
    pub gpu_bytes: u64,
}

/// Returned by `fynkel_apple_viewport_render` when the world pointer is
/// null or the world is poisoned (distinct from -1 = frame dropped).
pub const FYNKEL_APPLE_RENDER_NO_WORLD: i32 = -2;

/// Returned by `fynkel_apple_viewport_render` when the viewport is broken:
/// a panic inside one of its calls (foundation 4.3). Every call fails from
/// then on; `fynkel_apple_viewport_error` says why. Destroy it (and make a
/// new one to try again).
pub const FYNKEL_APPLE_RENDER_BROKEN: i32 = -3;

/// Returned by `fynkel_apple_viewport_render` when the viewport's view
/// camera (`fynkel_apple_viewport_set_view_camera`) is not an active camera
/// of the world rendered: nothing is drawn and the last frame stays.
pub const FYNKEL_APPLE_RENDER_NO_CAMERA: i32 = -6;

/// Runs a viewport call so a panic never crosses the C boundary (Rust
/// aborts the process when one does): `None` for a NULL or broken
/// viewport; a panic breaks the viewport and answers `None`.
unsafe fn guard<R>(vp: *const Viewport, f: impl FnOnce(&Viewport) -> R) -> Option<R> {
    let v = vp.as_ref()?;
    if v.broken.get().is_some() {
        return None;
    }
    match catch_unwind(AssertUnwindSafe(|| f(v))) {
        Ok(r) => Some(r),
        Err(payload) => {
            v.break_with(payload);
            None
        }
    }
}

/// [`guard`] for the calls that change the viewport.
unsafe fn guard_mut<R>(vp: *mut Viewport, f: impl FnOnce(&mut Viewport) -> R) -> Option<R> {
    let v = vp.as_mut()?;
    v.quiesce();
    if v.broken.get().is_some() {
        return None;
    }
    match catch_unwind(AssertUnwindSafe(|| f(v))) {
        Ok(r) => Some(r),
        Err(payload) => {
            (*vp).break_with(payload);
            None
        }
    }
}

/// `FYNKEL_APPLE_RENDER_BROKEN` for a broken viewport, else `otherwise`.
unsafe fn broken_or(vp: *const Viewport, otherwise: i32) -> i32 {
    match vp.as_ref() {
        Some(v) if v.broken.get().is_some() => FYNKEL_APPLE_RENDER_BROKEN,
        _ => otherwise,
    }
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic".into())
}

/// Viewport ABI version; bumped with `include/fynkel_apple.h` (3: the
/// M3-16 instrumentation fields of `FynkelAppleViewportStats`; 4:
/// `fynkel_apple_viewport_debug_lines` and `fynkel_apple_viewport_read_frame`,
/// the editor's overlays and its Save Frame — M4-06; 5: `gpuAo` in the
/// stats — R2; 6: `fynkel_apple_viewport_overlay`, the gizmo overlay —
/// ADR-0019, item 9f; 7: `fynkel_apple_viewport_set_view`, a viewport's
/// own camera over any world — ADR-0021 P7 L5; 8:
/// `fynkel_apple_viewport_outline`, the selection outline; 9:
/// `fynkel_apple_viewport_extra_worlds`, other worlds drawn into the frame
/// — Prefab Mode in context, multi-scene editing; 10: every viewport
/// shares one GPU device, ADR-0030 §4 — no call changed; 11: a panic
/// inside a call breaks the viewport instead of aborting the process —
/// `FYNKEL_APPLE_RENDER_BROKEN`, `fynkel_apple_viewport_error`,
/// `fynkel_apple_viewport_debug_panic`, foundation 4.3; …; 16:
/// `fynkel_apple_viewport_set_view_camera`, a viewport looking through one
/// of the world's cameras by entity — ADR-0062 amendment 1).
pub const APPLE_ABI_VERSION: u32 = 16;

/// Set in `fynkel_apple_viewport_acquire_presented`'s answer when another finished
/// frame is queued behind the slot it names (the slot is the low byte).
pub const ACQUIRE_MORE_QUEUED: i32 = 0x100;

/// Bit 0 of an extra world's flags (`fynkel_apple_viewport_extra_worlds`):
/// drawn greyed.
pub const FYNKEL_APPLE_EXTRA_WORLD_GRAY: u32 = 1;

#[no_mangle]
pub extern "C" fn fynkel_apple_abi_version() -> u32 {
    APPLE_ABI_VERSION
}

/// Creates a viewport with an IOSurface ring on the GPU every viewport in
/// the process shares (ABI 10, ADR-0030 §4: made by the first, freed with
/// the last; `FYNKEL_VIEWPORT_DEVICE=own` gives each its own instead).
/// `buffer_count` is clamped to 2..=8; `sync_mode` see `SyncMode`.
/// Returns NULL on failure (details in the log).
#[no_mangle]
pub extern "C" fn fynkel_apple_viewport_create(
    width: u32,
    height: u32,
    buffer_count: u32,
    sync_mode: i32,
) -> *mut Viewport {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
    let made = catch_unwind(|| {
        Viewport::new(
            width.max(1),
            height.max(1),
            buffer_count.clamp(2, 8),
            SyncMode::from_i32(sync_mode),
        )
    })
    .unwrap_or_else(|payload| Err(panic_text(payload)));
    match made {
        Ok(vp) => Box::into_raw(Box::new(vp)),
        Err(e) => {
            log::error!("fynkel_apple_viewport_create failed: {e}");
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `vp` must come from `fynkel_apple_viewport_create` and not be used
/// afterwards; the presenter must have been detached (callback cleared).
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_destroy(vp: *mut Viewport) {
    if !vp.is_null() {
        let vp = Box::from_raw(vp);
        if let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(vp))) {
            log::error!(
                "fynkel_apple_viewport_destroy: panic while freeing the viewport: {}",
                panic_text(payload)
            );
        }
    }
}

/// Turns the viewport's render worker on (`enabled` ≠ 0) or off. With it on,
/// `fynkel_apple_viewport_render` extracts on the calling thread and hands
/// the rest of the frame — admission, `prepare`, encode, submit — to a worker
/// that runs while the host simulates the next one; the call answers what the
/// *previous* frame's job answered (a slot, -1 dropped, -4 failed; the first
/// call 0). Every other call that touches the renderer waits for the job in
/// flight first. `render_ready` stays synchronous. Off by default. Returns 0,
/// or -1 for a NULL or broken viewport.
///
/// # Safety
/// `vp` must be a live viewport; host loop thread only.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_pipelined(
    vp: *mut Viewport,
    enabled: i32,
) -> i32 {
    guard_mut(vp, |v| {
        v.set_pipelined(enabled != 0);
        0
    })
    .unwrap_or(-1)
}

/// How many finished frames the presenter shows in order before it skips
/// ahead to the newest: 1 (the default) is a mailbox, 2 shows two frames that
/// finished between rasters one after the other instead of dropping the older
/// (a host whose rasters bunch, such as a long tick frame followed by a short
/// one, then still shows every frame it makes). Returns 0, or -1 for a NULL or
/// broken viewport.
///
/// # Safety
/// `vp` must be a live viewport; host loop thread only.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_frame_queue(
    vp: *mut Viewport,
    depth: u32,
) -> i32 {
    guard_mut(vp, |v| {
        v.ring.set_frame_queue(depth);
        0
    })
    .unwrap_or(-1)
}

/// Returns 0 on success, -1 on failure (the old ring is kept on failure).
/// # Safety
/// `vp` must be a live viewport; call from the host loop thread only.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_resize(
    vp: *mut Viewport,
    width: u32,
    height: u32,
) -> i32 {
    guard_mut(vp, |v| match v.resize(width.max(1), height.max(1)) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("fynkel_apple_viewport_resize failed: {e}");
            -1
        }
    })
    .unwrap_or(-1)
}

/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_buffer_count(vp: *const Viewport) -> u32 {
    guard(vp, |v| v.ring.buffer_count()).unwrap_or(0)
}

/// The `IOSurfaceRef` of slot `index` (unretained; valid until the next
/// resize or destroy). NULL for an out-of-range index.
/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_iosurface(
    vp: *const Viewport,
    index: u32,
) -> *const c_void {
    guard(vp, |v| v.ring.iosurface_ptr(index)).unwrap_or(std::ptr::null())
}

/// Extracts `world` (interpolated by its current alpha, presentation
/// overrides applied) and renders it into a free slot. Returns the slot
/// index, -1 if the frame was dropped (ring full), or
/// `FYNKEL_APPLE_RENDER_NO_WORLD`.
/// # Safety
/// `vp` must be a live viewport and `world` a live `FynkelWorld` (or NULL);
/// call from the host loop thread only, never concurrently with a mutating
/// `fynkel_world_*` call on the same world.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_render(
    vp: *mut Viewport,
    world: *const FynkelWorld,
) -> i32 {
    render_world(vp, world, None)
}

/// Export/loading boundary: waits for GPU assets before occupying a ring slot.
/// Returns -4 when required assets fail or the timeout expires.
/// # Safety
/// Same ownership and thread requirements as `fynkel_apple_viewport_render`.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_render_ready(
    vp: *mut Viewport,
    world: *const FynkelWorld,
    timeout_ms: u32,
) -> i32 {
    render_world(vp, world, Some(Duration::from_millis(timeout_ms as u64)))
}

/// Whether the last frame has pending asset work; errors are terminal.
/// # Safety
/// `vp` is live or NULL; host thread only.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_assets_pending(vp: *const Viewport) -> u32 {
    guard(vp, |v| match &v.pipeline {
        Some(p) => u32::from(p.state.assets_pending.load(Ordering::Acquire)),
        None => u32::from(v.renderer.assets_pending()),
    })
    .unwrap_or(0)
}

unsafe fn render_world(
    vp: *mut Viewport,
    world: *const FynkelWorld,
    wait: Option<Duration>,
) -> i32 {
    // A pipelined viewport answers without waiting for the worker (below);
    // every other path starts from a quiet renderer, like any mutating call.
    if let Some(v) = vp.as_mut() {
        if v.pipeline.is_none() || wait.is_some() {
            v.quiesce();
        }
    }
    guard_mut_nowait(vp, |v| {
        if v.ring.gpu().is_lost() {
            return -5;
        }
        let Some(w) = world.as_ref() else {
            return FYNKEL_APPLE_RENDER_NO_WORLD;
        };
        if w.core().is_poisoned() {
            return FYNKEL_APPLE_RENDER_NO_WORLD;
        }
        if v.pipeline.is_some() && wait.is_none() {
            return v.render_pipelined(w);
        }
        let Viewport {
            ring,
            renderer,
            extract,
            view,
            view_camera,
            ..
        } = v;
        // Through one of the world's cameras: extracted before admission,
        // so a panel whose camera is gone takes no slot and keeps its last
        // picture.
        let mut extracted = None;
        if view_camera.is_some() {
            let start = std::time::Instant::now();
            w.core().extract_into(extract, renderer.placeholders);
            if !Viewport::aim(extract, None, *view_camera) {
                return FYNKEL_APPLE_RENDER_NO_CAMERA;
            }
            extracted = Some(start.elapsed().as_micros() as u32);
        }
        Viewport::render_frame(ring, renderer, |renderer| {
            // Asset selection and extraction happen only after admission. The
            // retained extract is overwritten from the latest world on retry.
            if let Some(assets) = w.assets() {
                renderer.set_assets(Some(assets.clone()));
            }
            let extract_us = match extracted {
                Some(us) => us,
                None => {
                    let start = std::time::Instant::now();
                    w.core().extract_into(extract, renderer.placeholders);
                    Viewport::aim(extract, *view, None);
                    start.elapsed().as_micros() as u32
                }
            };
            let start = std::time::Instant::now();
            if let Some(timeout) = wait {
                renderer.prepare_ready(ring.device(), ring.queue(), extract, timeout)?;
            } else {
                renderer.prepare(ring.device(), ring.queue(), extract);
            }
            Ok((extract_us, start.elapsed().as_micros() as u32))
        })
    })
    .unwrap_or_else(|| broken_or(vp, -1))
}

/// [`guard_mut`] without the wait for the render worker: only the render
/// call itself, which hands the worker its next job.
unsafe fn guard_mut_nowait<R>(vp: *mut Viewport, f: impl FnOnce(&mut Viewport) -> R) -> Option<R> {
    let v = vp.as_mut()?;
    if v.broken.get().is_some() {
        return None;
    }
    match catch_unwind(AssertUnwindSafe(|| f(v))) {
        Ok(r) => Some(r),
        Err(payload) => {
            (*vp).break_with(payload);
            None
        }
    }
}

/// # Safety
/// `vp` must be a live viewport; `camera` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_camera(
    vp: *mut Viewport,
    camera: *const FynkelAppleCamera,
) {
    let Some(c) = camera.as_ref() else { return };
    guard_mut(vp, |v| {
        let cam: Camera = (*c).into();
        if cam.eye.is_finite() && cam.target.is_finite() && cam.fov_y_radians > 0.0 {
            v.set_camera(cam);
        }
    });
}

/// Draws this viewport through the world's camera on the entity (`world`,
/// `slot`, `generation`) — its `Camera` and the frame's interpolated pose —
/// instead of the one the frame picks by priority: a game's second camera
/// in a panel. While the entity is not an active camera, render answers
/// [`FYNKEL_APPLE_RENDER_NO_CAMERA`]. The null entity clears it; it and
/// `fynkel_apple_viewport_set_view` replace each other. Returns 0, or -1 for
/// a NULL viewport.
///
/// # Safety
/// `vp` must be NULL or a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_view_camera(
    vp: *mut Viewport,
    world: u64,
    slot: u32,
    generation: u32,
) -> i32 {
    guard_mut(vp, |v| {
        let e =
            Entity::from_bits(world as u128 | (slot as u128) << 64 | (generation as u128) << 96);
        v.set_view_camera((!e.is_null()).then_some(e));
        0
    })
    .unwrap_or(-1)
}

/// A viewport's own camera (`fynkel_apple_viewport_set_view`): the scene
/// `Camera` component's projection fields.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FynkelAppleViewCamera {
    /// 0 perspective, 1 orthographic.
    pub projection: u32,
    pub fov_y: f32,
    pub ortho_height: f32,
    pub near: f32,
    pub far: f32,
    /// 0 auto, 1 manual.
    pub exposure_mode: u32,
    pub exposure: f32,
    pub culling_mask: u32,
}

/// Draws this viewport through its own camera — at `world` (sixteen
/// floats, column-major) with `camera`'s projection — instead of the
/// world's active one, for every world it renders (the editor's Scene view
/// on a running game, which must not touch the game's camera). `world`
/// NULL clears it. Returns 0, or -1 for a NULL viewport, a NULL camera
/// with a world, or values that cannot draw (non-finite, `near <= 0`,
/// `far <= near`, a projection or exposure mode out of range).
///
/// # Safety
/// `vp` must be a live viewport; `world` NULL or readable for 16 floats;
/// `camera` NULL or valid.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_view(
    vp: *mut Viewport,
    world: *const f32,
    camera: *const FynkelAppleViewCamera,
) -> i32 {
    guard_mut(vp, |v| {
        if world.is_null() {
            v.set_view(None);
            return 0;
        }
        let Some(c) = camera.as_ref() else { return -1 };
        let m = glam::Mat4::from_cols_slice(std::slice::from_raw_parts(world, 16));
        let projection = match c.projection {
            0 => Projection::Perspective,
            1 => Projection::Orthographic,
            _ => return -1,
        };
        let exposure_mode = match c.exposure_mode {
            0 => ExposureMode::Auto,
            1 => ExposureMode::Manual,
            _ => return -1,
        };
        let finite = m.is_finite()
            && [c.fov_y, c.ortho_height, c.near, c.far, c.exposure]
                .iter()
                .all(|f| f.is_finite());
        if !finite || c.near <= 0.0 || c.far <= c.near {
            return -1;
        }
        v.set_view(Some((
            m,
            SceneCamera {
                projection,
                fov_y: c.fov_y,
                ortho_height: c.ortho_height,
                near: c.near,
                far: c.far,
                priority: 0,
                exposure_mode,
                exposure: c.exposure,
                culling_mask: c.culling_mask,
            },
        )));
        0
    })
    .unwrap_or(-1)
}

/// Points the viewport's renderer at a cooked asset cache directory (the
/// `index.json` written by `fynkel cook`); `dir` NULL or empty clears it.
/// Returns 0, or -1 when the directory is not a readable cache (the
/// previous source is dropped either way). Rendering a world that loaded
/// from a cache adopts that world's store instead, so hosts on the SDK's
/// `loadProject`/`loadScene` never need this.
///
/// # Safety
/// `vp` must be a live viewport; `dir_utf8` points at `dir_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_asset_cache(
    vp: *mut Viewport,
    dir_utf8: *const u8,
    dir_len: u32,
) -> i32 {
    guard_mut(vp, |v| {
        let dir = if dir_utf8.is_null() || dir_len == 0 {
            None
        } else {
            let bytes = std::slice::from_raw_parts(dir_utf8, dir_len as usize);
            match std::str::from_utf8(bytes) {
                Ok(s) => Some(std::path::PathBuf::from(s)),
                Err(_) => return -1,
            }
        };
        match v.set_asset_cache(dir.as_deref()) {
            Ok(()) => 0,
            Err(e) => {
                log::error!("fynkel_apple_viewport_set_asset_cache: {e}");
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Whether entities without a `MeshRenderer` draw as placeholder cubes
/// (on by default — the M1 look).
///
/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_placeholders(vp: *mut Viewport, enabled: i32) {
    guard_mut(vp, |v| v.set_placeholders(enabled != 0));
}

/// Selects the quality profile the renderer runs with, by name from the
/// project the world's cache was cooked from (`quality.profiles` in
/// `fynkel.project.json`; `name` NULL or empty: the project's default,
/// or the engine defaults without a project). Applied by the next render.
/// Returns 0, or -1 when the store the viewport currently draws from has
/// profiles and none is called `name` (the default applies meanwhile; a
/// viewport that has not rendered a world yet accepts any name and
/// resolves it on the first frame).
///
/// # Safety
/// `vp` must be a live viewport; `name_utf8` points at `name_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_quality(
    vp: *mut Viewport,
    name_utf8: *const u8,
    name_len: u32,
) -> i32 {
    guard_mut(vp, |v| {
        let name = if name_utf8.is_null() || name_len == 0 {
            None
        } else {
            let bytes = std::slice::from_raw_parts(name_utf8, name_len as usize);
            match std::str::from_utf8(bytes) {
                Ok(s) => Some(s.to_string()),
                Err(_) => return -1,
            }
        };
        if v.set_quality(name.as_deref()) {
            0
        } else {
            log::warn!(
                "fynkel_apple_viewport_set_quality: no profile {:?} (profiles: {})",
                name.unwrap_or_default(),
                v.renderer.quality_profiles().join(", ")
            );
            -1
        }
    })
    .unwrap_or(-1)
}

/// Queues `count` world-space line segments for the NEXT render of this
/// viewport — ten `f32` each: `ax ay az bx by bz r g b a` (metres; linear,
/// display-referred RGBA) — through the renderer's debug primitives
/// (blueprint §19.2d item 5): depth-tested against the opaque scene, never
/// written, unlit. The queue empties with every render, so a host draws its
/// grid, gizmos and outlines by queuing them before each frame (the
/// editor's Scene view, M4-06). Returns the segments queued, -1 for a NULL
/// viewport or a NULL `lines` with a non-zero `count`; a non-finite value
/// drops that segment.
///
/// # Safety
/// `vp` must be a live viewport; `lines` readable for `count * 10` floats,
/// or NULL with `count` 0.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_debug_lines(
    vp: *mut Viewport,
    lines: *const f32,
    count: u32,
) -> i32 {
    guard_mut(vp, |v| {
        if count == 0 {
            return 0;
        }
        if lines.is_null() {
            return -1;
        }
        let floats = std::slice::from_raw_parts(lines, count as usize * 10);
        let debug = v.renderer.debug();
        let mut queued = 0;
        for l in floats.chunks_exact(10) {
            if l.iter().any(|f| !f.is_finite()) {
                continue;
            }
            debug.line(
                glam::Vec3::new(l[0], l[1], l[2]),
                glam::Vec3::new(l[3], l[4], l[5]),
                [l[6], l[7], l[8], l[9]],
            );
            queued += 1;
        }
        queued
    })
    .unwrap_or(-1)
}

/// Queues the NEXT render's overlay (ADR-0019): gizmos and handles drawn
/// after the post pass at the target's resolution, never exposed or tone
/// mapped. `lines`: twelve `f32` each — `ax ay az bx by bz r g b a width
/// on_top` (metres; linear, display-referred RGBA; width in target pixels;
/// `on_top` non-zero to draw over the scene instead of fading behind it).
/// `triangles`: fourteen each — `ax ay az bx by bz cx cy cz r g b a
/// on_top`. `grid`: NULL, or eighteen floats — minor, major, X-axis and
/// Z-axis RGBA, then the cell (0: adaptive) and the fade distance (0:
/// adaptive) — to draw the ground grid. Everything is cleared by the
/// render. Returns the lines plus triangles queued (a primitive with a
/// non-finite value is dropped), -1 for a NULL viewport or a NULL array
/// with a non-zero count.
///
/// # Safety
/// `vp` must be a live viewport; `lines` readable for `line_count * 12`
/// floats and `triangles` for `triangle_count * 14` (or NULL with a zero
/// count); `grid` NULL or readable for 18 floats.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_overlay(
    vp: *mut Viewport,
    lines: *const f32,
    line_count: u32,
    triangles: *const f32,
    triangle_count: u32,
    grid: *const f32,
) -> i32 {
    guard_mut(vp, |v| {
        if (line_count > 0 && lines.is_null()) || (triangle_count > 0 && triangles.is_null()) {
            return -1;
        }
        let overlay = v.renderer.overlay();
        let v3 = |f: &[f32]| glam::Vec3::new(f[0], f[1], f[2]);
        let flags = |f: f32| {
            if f != 0.0 {
                fynkel_render::overlay::ON_TOP
            } else {
                0
            }
        };
        let mut queued = 0;
        if line_count > 0 {
            for l in std::slice::from_raw_parts(lines, line_count as usize * 12).chunks_exact(12) {
                if l.iter().any(|f| !f.is_finite()) {
                    continue;
                }
                overlay.push_line(
                    v3(&l[0..3]),
                    v3(&l[3..6]),
                    [l[6], l[7], l[8], l[9]],
                    l[10],
                    flags(l[11]),
                );
                queued += 1;
            }
        }
        if triangle_count > 0 {
            for t in
                std::slice::from_raw_parts(triangles, triangle_count as usize * 14).chunks_exact(14)
            {
                if t.iter().any(|f| !f.is_finite()) {
                    continue;
                }
                overlay.push_triangle(
                    v3(&t[0..3]),
                    v3(&t[3..6]),
                    v3(&t[6..9]),
                    [t[9], t[10], t[11], t[12]],
                    flags(t[13]),
                );
                queued += 1;
            }
        }
        if !grid.is_null() {
            let g = std::slice::from_raw_parts(grid, 18);
            if g.iter().all(|f| f.is_finite()) {
                let rgba = |i: usize| [g[i], g[i + 1], g[i + 2], g[i + 3]];
                overlay.grid(fynkel_render::Grid {
                    minor: rgba(0),
                    major: rgba(4),
                    x_axis: rgba(8),
                    z_axis: rgba(12),
                    cell: g[16],
                    fade: g[17],
                });
            }
        }
        queued
    })
    .unwrap_or(-1)
}

/// Queues the NEXT render's selection outline (Unity's Scene view): the
/// silhouettes of what the `selected` entities draw ringed in one colour,
/// their `children`'s in another, `width` target pixels past the edge and
/// over everything — hidden parts included, handles on top
/// (`fynkel_render::Overlay::outline`). Entities are handle bits
/// (`Entity::to_bits`; the authoring preview's `entities` call, the live
/// world's `handles`); one in both lists is selected, one the frame does
/// not draw has no outline. `style`: NULL for the default, or nine floats —
/// selected RGBA, children RGBA (linear, display-referred), width. Cleared
/// by the render like the rest of the overlay; a later call replaces an
/// earlier one. Returns the entities queued (both lists), -1 for a NULL
/// viewport, a NULL list with a non-zero count, or a non-finite style
/// (nothing is queued).
///
/// # Safety
/// `vp` must be a live viewport; `selected` readable for `selected_count`
/// `u64`s and `children` for `children_count` (or NULL with a zero
/// count); `style` NULL or readable for 9 floats.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_outline(
    vp: *mut Viewport,
    selected: *const fynkel_contracts::Entity,
    selected_count: u32,
    children: *const fynkel_contracts::Entity,
    children_count: u32,
    style: *const f32,
) -> i32 {
    guard_mut(vp, |v| {
        if (selected_count > 0 && selected.is_null()) || (children_count > 0 && children.is_null())
        {
            return -1;
        }
        let style = if style.is_null() {
            fynkel_render::OutlineStyle::default()
        } else {
            let f = std::slice::from_raw_parts(style, 9);
            if f.iter().any(|x| !x.is_finite()) {
                return -1;
            }
            fynkel_render::OutlineStyle {
                selected: [f[0], f[1], f[2], f[3]],
                children: [f[4], f[5], f[6], f[7]],
                width: f[8],
            }
        };
        let entities =
            |bits: *const fynkel_contracts::Entity, count: u32| -> Vec<fynkel_contracts::Entity> {
                if count == 0 {
                    return Vec::new();
                }
                std::slice::from_raw_parts(bits, count as usize).to_vec()
            };
        let (selected, children) = (
            entities(selected, selected_count),
            entities(children, children_count),
        );
        v.renderer.overlay().outline(&selected, &children, style);
        (selected.len() + children.len()) as i32
    })
    .unwrap_or(-1)
}

/// Queues `count` other worlds for the NEXT render, drawn into the frame
/// of the world `fynkel_apple_viewport_render` is given
/// ([`fynkel_render::ExtraWorld`]): the editor's Prefab Mode in context
/// (the scene's preview around the prefab's, transformed by the inverse of
/// the instance's world matrix, greyed or not) and multi-scene editing (the
/// other loaded scenes' previews, identity). Each world is extracted here
/// — interpolated by its current alpha, presentation overrides applied —
/// so the host calls this right before the render and may destroy a world
/// afterwards. `transforms`: sixteen floats per world, column-major, its
/// frame → the main world's. `flags`: one per world,
/// [`FYNKEL_APPLE_EXTRA_WORLD_GRAY`] draws it greyed; other bits are
/// ignored. The extras draw from the viewport's asset store (the main
/// world's cache, which every preview of one authoring session shares);
/// they are never outlined. Cleared by the render; a later call replaces an
/// earlier one (`count` 0 clears). A NULL or poisoned world is skipped.
/// Returns the worlds queued, -1 for a NULL viewport, a NULL array with a
/// non-zero count, or a non-finite transform (nothing is queued).
///
/// # Safety
/// `vp` must be a live viewport; `worlds` readable for `count` pointers,
/// each a live `FynkelWorld` or NULL, `transforms` for `count * 16` floats
/// and `flags` for `count` `u32`s (or NULL with a zero count); call from
/// the host loop thread only, never concurrently with a mutating
/// `fynkel_world_*` call on one of the worlds.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_extra_worlds(
    vp: *mut Viewport,
    worlds: *const *const FynkelWorld,
    transforms: *const f32,
    flags: *const u32,
    count: u32,
) -> i32 {
    guard_mut(vp, |v| {
        if count > 0 && (worlds.is_null() || transforms.is_null() || flags.is_null()) {
            return -1;
        }
        let n = count as usize;
        let (worlds, transforms, flags): (&[*const FynkelWorld], &[f32], &[u32]) = if n == 0 {
            (&[], &[], &[])
        } else {
            (
                std::slice::from_raw_parts(worlds, n),
                std::slice::from_raw_parts(transforms, n * 16),
                std::slice::from_raw_parts(flags, n),
            )
        };
        if transforms.iter().any(|f| !f.is_finite()) {
            return -1;
        }
        v.renderer.clear_extra_worlds();
        let mut queued = 0;
        for (i, w) in worlds.iter().enumerate() {
            let Some(w) = w.as_ref() else { continue };
            if w.core().is_poisoned() {
                continue;
            }
            v.renderer.push_extra_world(fynkel_render::ExtraWorld {
                extract: w.core().extract(),
                transform: glam::Mat4::from_cols_slice(&transforms[i * 16..i * 16 + 16]),
                gray: flags[i] & FYNKEL_APPLE_EXTRA_WORLD_GRAY != 0,
            });
            queued += 1;
        }
        queued
    })
    .unwrap_or(-1)
}

/// Copies the frame in ring slot `slot` (what `fynkel_apple_viewport_render`
/// returned) into `out` as tightly packed RGBA8, top row first — `width ×
/// height × 4` bytes at the viewport's current size. Blocks until every
/// submitted frame has completed; the editor's Save Frame and its pixel
/// gate against `fynkel render`, never a per-frame path. Returns the bytes
/// written, or -1 for a NULL viewport or `out`, a slot the ring lacks, or a
/// `cap` smaller than the frame.
///
/// # Safety
/// `vp` must be a live viewport; `out` writable for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_read_frame(
    vp: *const Viewport,
    slot: i32,
    out: *mut u8,
    cap: u32,
) -> i32 {
    guard(vp, |v| {
        if out.is_null() || slot < 0 {
            return -1;
        }
        let (w, h) = v.ring.size();
        let needed = w as usize * h as usize * 4;
        if (cap as usize) < needed {
            return -1;
        }
        let Some(pixels) = v.ring.read_frame(slot as u32) else {
            return -1;
        };
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), out, needed);
        needed as i32
    })
    .unwrap_or(-1)
}

/// Sets the bounded wait `fynkel_apple_viewport_acquire_presented` may spend
/// for a completion when no new image is ready (microseconds; 0 disables).
/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_acquire_wait_us(vp: *const Viewport, us: u32) {
    guard(vp, |v| {
        v.ring.set_acquire_wait(Duration::from_micros(us as u64))
    });
}

/// Presenter entry point (Flutter raster thread). See
/// [`IoSurfaceRing::acquire_presented`].
/// # Safety
/// `vp` must be a live viewport. Safe to call from any thread.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_acquire_presented(vp: *const Viewport) -> i32 {
    guard(vp, |v| {
        // A frame queue (`set_frame_queue`) can leave a finished frame behind
        // the one returned: bit 8 asks the presenter to signal for another raster.
        let (slot, more) = v.ring.acquire_presented_queued();
        if more && slot >= 0 {
            slot | ACQUIRE_MORE_QUEUED
        } else {
            slot
        }
    })
    .unwrap_or(-1)
}

/// # Safety
/// `vp` must be a live viewport; `func`/`userdata` must stay valid until
/// cleared (call with NULL before the target goes away; this call returns
/// only after any in-progress callback finished).
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_frame_ready_callback(
    vp: *const Viewport,
    func: Option<ring::FrameReadyFn>,
    userdata: *mut c_void,
) {
    // Also on a broken viewport: the host must be able to detach.
    let Some(v) = vp.as_ref() else { return };
    let set = catch_unwind(AssertUnwindSafe(|| {
        v.ring.set_frame_ready_callback(func, userdata)
    }));
    if let Err(payload) = set {
        v.break_with(payload);
    }
}

/// Drains and summarizes stats since the previous call.
/// # Safety
/// `vp` must be a live viewport; `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_take_stats(
    vp: *const Viewport,
    out: *mut FynkelAppleViewportStats,
) {
    let Some(out) = out.as_mut() else { return };
    guard(vp, |v| take_stats(v, out));
}

fn take_stats(v: &Viewport, out: &mut FynkelAppleViewportStats) {
    // The renderer's last-frame numbers: not while the worker is drawing.
    v.quiesce();
    let mut s = v.ring.take_stats();
    let pct = ring::pct;
    *out = FynkelAppleViewportStats {
        frames_rendered: s.frames_rendered,
        frames_dropped_no_free_buffer: s.frames_dropped_no_free_buffer,
        frames_presented: s.frames_presented,
        presents_without_new_frame: s.presents_without_new_frame,
        cpu_p50_us: pct(&mut s.cpu_us, 0.50),
        cpu_p95_us: pct(&mut s.cpu_us, 0.95),
        cpu_p99_us: pct(&mut s.cpu_us, 0.99),
        gpu_complete_p50_us: pct(&mut s.gpu_complete_us, 0.50),
        gpu_complete_p95_us: pct(&mut s.gpu_complete_us, 0.95),
        gpu_complete_p99_us: pct(&mut s.gpu_complete_us, 0.99),
        resizes: s.resizes,
        resize_p50_us: pct(&mut s.resize_us, 0.50),
        resize_max_us: pct(&mut s.resize_us, 1.0),
        present_lag_p50: pct(&mut s.present_lag, 0.50),
        present_lag_p95: pct(&mut s.present_lag, 0.95),
        present_lag_max: pct(&mut s.present_lag, 1.0),
        acquire_wait_p50_us: pct(&mut s.acquire_wait_us, 0.50),
        acquire_wait_p95_us: pct(&mut s.acquire_wait_us, 0.95),
        acquire_wait_max_us: pct(&mut s.acquire_wait_us, 1.0),
        instances_last_frame: v.instance_count(),
        frame_ready_signals: v.ring.frame_ready_signals(),
        meshes_last_frame: v.frame_stats().meshes,
        placeholders_last_frame: v.frame_stats().placeholders,
        missing_last_frame: v.frame_stats().missing,
        asset_errors: v.asset_errors().len() as u32,
        scene_camera: v.frame_stats().scene_camera as u32,
        transparent_last_frame: v.frame_stats().transparent,
        lights: v.frame_stats().lights,
        lights_dropped: v.frame_stats().lights_dropped,
        shadow_casters: v.frame_stats().shadow_casters,
        shadow_map: v.frame_stats().shadow_map,
        msaa: v.frame_stats().msaa,
        render_width: v.frame_stats().render_width,
        render_height: v.frame_stats().render_height,
        culled_last_frame: v.frame_stats().culled,
        draws_last_frame: v.frame_stats().draws,
        shadow_draws_last_frame: v.frame_stats().shadow_draws,
        extract_p50_us: pct(&mut s.extract_us, 0.50),
        extract_p95_us: pct(&mut s.extract_us, 0.95),
        extract_p99_us: pct(&mut s.extract_us, 0.99),
        prepare_p50_us: pct(&mut s.prepare_us, 0.50),
        prepare_p95_us: pct(&mut s.prepare_us, 0.95),
        prepare_p99_us: pct(&mut s.prepare_us, 0.99),
        encode_p50_us: pct(&mut s.encode_us, 0.50),
        encode_p95_us: pct(&mut s.encode_us, 0.95),
        encode_p99_us: pct(&mut s.encode_us, 0.99),
        gpu_timestamps: v.frame_stats().gpu_timestamps as u32,
        gpu_frames_timed: s.gpu_shadow_us.len() as u32,
        gpu_frames_untimed: v.frame_stats().gpu_untimed,
        gpu_shadow_p50_us: pct(&mut s.gpu_shadow_us, 0.50),
        gpu_shadow_p95_us: pct(&mut s.gpu_shadow_us, 0.95),
        gpu_shadow_p99_us: pct(&mut s.gpu_shadow_us, 0.99),
        gpu_ao_p50_us: pct(&mut s.gpu_ao_us, 0.50),
        gpu_ao_p95_us: pct(&mut s.gpu_ao_us, 0.95),
        gpu_ao_p99_us: pct(&mut s.gpu_ao_us, 0.99),
        gpu_scene_p50_us: pct(&mut s.gpu_scene_us, 0.50),
        gpu_scene_p95_us: pct(&mut s.gpu_scene_us, 0.95),
        gpu_scene_p99_us: pct(&mut s.gpu_scene_us, 0.99),
        gpu_post_p50_us: pct(&mut s.gpu_post_us, 0.50),
        gpu_post_p95_us: pct(&mut s.gpu_post_us, 0.95),
        gpu_post_p99_us: pct(&mut s.gpu_post_us, 0.99),
        gpu_frame_p50_us: pct(&mut s.gpu_frame_us, 0.50),
        gpu_frame_p95_us: pct(&mut s.gpu_frame_us, 0.95),
        gpu_frame_p99_us: pct(&mut s.gpu_frame_us, 0.99),
        gpu_bytes: v.frame_stats().gpu_bytes,
    };
}

/// Copies why the viewport is broken (the panic's message, foundation 4.3)
/// into `out`, cut at `cap` bytes; answers the bytes written — 0 while the
/// viewport works, or for a NULL viewport or `out`.
///
/// # Safety
/// `vp` must be a live viewport or NULL; `out` writable for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_error(
    vp: *const Viewport,
    out: *mut u8,
    cap: u32,
) -> i32 {
    let (Some(v), false) = (vp.as_ref(), out.is_null()) else {
        return 0;
    };
    let Some(msg) = v.error() else { return 0 };
    let n = msg.len().min(cap as usize);
    std::ptr::copy_nonoverlapping(msg.as_ptr(), out, n);
    n as i32
}

/// Panics inside a viewport call, on purpose: the test of the host's
/// broken-viewport path. Answers -1 (the viewport is broken afterwards).
///
/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_debug_panic(vp: *mut Viewport) -> i32 {
    guard_mut(vp, |_| -> i32 {
        panic!("fynkel_apple_viewport_debug_panic: a panic on purpose")
    })
    .unwrap_or(-1)
}

/// Sets an explicit supported local-light budget. Returns zero on success.
/// # Safety
/// `vp` is a live viewport or NULL; host thread only.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_light_budget(
    vp: *mut Viewport,
    pool: u32,
    per_object: u32,
) -> i32 {
    guard_mut(vp, |v| {
        match v.renderer.set_light_budget(fynkel_render::LightBudget {
            pool: pool as usize,
            per_object: per_object as usize,
        }) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
    .unwrap_or(-1)
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use fynkel_native::{
        fynkel_runtime_create, fynkel_runtime_destroy, fynkel_world_create, fynkel_world_destroy,
    };

    #[test]
    fn full_viewport_skips_world_extraction_and_preparation_then_refreshes() {
        let rt = fynkel_runtime_create();
        let world = unsafe { fynkel_world_create(rt, std::ptr::null(), std::ptr::null_mut()) };
        assert!(!world.is_null());
        let mut vp = Viewport::new(32, 32, 3, SyncMode::Blocking).unwrap();
        for slot in 0..3 {
            assert_eq!(
                unsafe { fynkel_apple_viewport_render(&mut vp, world) },
                slot
            );
        }
        vp.ring.take_stats();
        // Three finished frames nobody has shown: the mailbox recycles the
        // oldest for the new one, so the world is extracted and prepared and
        // nothing is dropped. (A ring of protected slots drops without
        // touching the extract; platform/apple/tests/ring.rs covers it.)
        vp.extract.camera = Some(ExtractCamera {
            record: 123,
            world: glam::Mat4::IDENTITY,
            camera: SceneCamera::default(),
        });
        assert_eq!(unsafe { fynkel_apple_viewport_render(&mut vp, world) }, 0);
        assert!(
            vp.extract.camera.is_none(),
            "the recycled frame extracts the current empty world"
        );
        let stats = vp.ring.take_stats();
        assert_eq!(stats.frames_dropped_no_free_buffer, 0);
        assert_eq!(stats.extract_us.len(), 1);
        assert_eq!(stats.prepare_us.len(), 1);
        assert_eq!(vp.ring.acquire_presented(), 0, "the newest frame is shown");
        let renderer_stats = vp.renderer.stats();
        assert_eq!(
            renderer_stats.prepare_phases_us.iter().sum::<u32>(),
            renderer_stats.prepare_us
        );
        assert!(renderer_stats.prepare_us <= stats.prepare_us[0]);
        assert!(
            renderer_stats.visibility_phases_us.iter().sum::<u32>()
                <= renderer_stats.prepare_phases_us[2]
        );
        unsafe {
            fynkel_world_destroy(world);
            fynkel_runtime_destroy(rt);
        }
    }

    #[test]
    fn a_pipelined_viewport_renders_on_its_worker_and_every_other_call_waits_for_it() {
        let rt = fynkel_runtime_create();
        let world = unsafe { fynkel_world_create(rt, std::ptr::null(), std::ptr::null_mut()) };
        assert!(!world.is_null());
        let mut vp = Viewport::new(32, 32, 3, SyncMode::Async).unwrap();
        assert_eq!(
            unsafe { fynkel_apple_viewport_set_pipelined(&mut vp, 1) },
            0
        );
        // The first call answers 0 (no earlier job); later ones answer what
        // the frame before them did.
        for _ in 0..12 {
            let answered = unsafe { fynkel_apple_viewport_render(&mut vp, world) };
            assert!(answered >= -1, "answered {answered}");
        }
        // Reading the renderer's numbers waits for the job in flight.
        let mut stats = unsafe { std::mem::zeroed::<FynkelAppleViewportStats>() };
        unsafe { fynkel_apple_viewport_take_stats(&vp, &mut stats) };
        assert_eq!(
            stats.frames_rendered + stats.frames_dropped_no_free_buffer,
            12
        );
        assert!(stats.frames_rendered >= 3, "{}", stats.frames_rendered);
        assert!(stats.prepare_p50_us > 0 || stats.encode_p50_us > 0 || stats.frames_rendered > 0);
        // The ring has a finished frame to show.
        vp.ring.wait_idle();
        assert!(vp.ring.acquire_presented() >= 0);
        // Off again: the worker is joined and the call is synchronous.
        assert_eq!(
            unsafe { fynkel_apple_viewport_set_pipelined(&mut vp, 0) },
            0
        );
        assert!(unsafe { fynkel_apple_viewport_render(&mut vp, world) } >= -1);
        unsafe {
            fynkel_world_destroy(world);
            fynkel_runtime_destroy(rt);
        }
    }
}
