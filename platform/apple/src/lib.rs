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

use std::ffi::c_void;
use std::time::Duration;

use fynkel_core::Extract;
use fynkel_native::FynkelWorld;
use fynkel_render::{Assets, Camera, ExtractRenderer, FrameStats};
use glam::Vec3;

pub use ring::{IoSurfaceRing, RingStats, SyncMode, TEXTURE_FORMAT};

/// Ring + renderer. Owns the GPU device.
pub struct Viewport {
    ring: IoSurfaceRing,
    renderer: ExtractRenderer,
}

impl Viewport {
    pub fn new(
        width: u32,
        height: u32,
        buffer_count: u32,
        sync_mode: SyncMode,
    ) -> Result<Self, String> {
        let ring = IoSurfaceRing::new(width, height, buffer_count, sync_mode)?;
        let renderer = ExtractRenderer::new(ring.device(), TEXTURE_FORMAT, width, height);
        Ok(Self { ring, renderer })
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

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.ring.resize(width, height)?;
        self.renderer.resize(self.ring.device(), width, height);
        Ok(())
    }

    /// Renders `extract` into a free slot; see [`IoSurfaceRing::render_with`].
    pub fn render(&mut self, extract: &Extract) -> i32 {
        let Viewport { ring, renderer } = self;
        renderer.prepare(ring.device(), ring.queue(), extract);
        ring.render_with(|device, _queue, view| {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fynkel viewport frame"),
            });
            renderer.draw(&mut encoder, view);
            encoder.finish()
        })
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
}

/// Returned by `fynkel_apple_viewport_render` when the world pointer is
/// null or the world is poisoned (distinct from -1 = frame dropped).
pub const FYNKEL_APPLE_RENDER_NO_WORLD: i32 = -2;

/// Viewport ABI version; bumped with `include/fynkel_apple.h`.
pub const APPLE_ABI_VERSION: u32 = 2;

#[no_mangle]
pub extern "C" fn fynkel_apple_abi_version() -> u32 {
    APPLE_ABI_VERSION
}

/// Creates a viewport with its own Metal device and an IOSurface ring.
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
    match Viewport::new(
        width.max(1),
        height.max(1),
        buffer_count.clamp(2, 8),
        SyncMode::from_i32(sync_mode),
    ) {
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
        drop(Box::from_raw(vp));
    }
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
    let Some(v) = vp.as_mut() else { return -1 };
    match v.resize(width.max(1), height.max(1)) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("fynkel_apple_viewport_resize failed: {e}");
            -1
        }
    }
}

/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_buffer_count(vp: *const Viewport) -> u32 {
    vp.as_ref().map(|v| v.ring.buffer_count()).unwrap_or(0)
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
    vp.as_ref()
        .map(|v| v.ring.iosurface_ptr(index))
        .unwrap_or(std::ptr::null())
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
    let Some(v) = vp.as_mut() else { return -1 };
    let Some(w) = world.as_ref() else {
        return FYNKEL_APPLE_RENDER_NO_WORLD;
    };
    if w.core().is_poisoned() {
        return FYNKEL_APPLE_RENDER_NO_WORLD;
    }
    // The world's scene came from a cooked cache: draw from the same
    // store, so residency (what the scene retained) and the renderer's
    // GPU copies agree. Re-pointing at the store it already has is free.
    if let Some(assets) = w.assets() {
        v.renderer.set_assets(Some(assets.clone()));
    }
    let extract = w.core().extract();
    v.render(&extract)
}

/// # Safety
/// `vp` must be a live viewport; `camera` a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_camera(
    vp: *mut Viewport,
    camera: *const FynkelAppleCamera,
) {
    if let (Some(v), Some(c)) = (vp.as_mut(), camera.as_ref()) {
        let cam: Camera = (*c).into();
        if cam.eye.is_finite() && cam.target.is_finite() && cam.fov_y_radians > 0.0 {
            v.set_camera(cam);
        }
    }
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
    let Some(v) = vp.as_mut() else {
        return -1;
    };
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
}

/// Whether entities without a `MeshRenderer` draw as placeholder cubes
/// (on by default — the M1 look).
///
/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_placeholders(vp: *mut Viewport, enabled: i32) {
    if let Some(v) = vp.as_mut() {
        v.set_placeholders(enabled != 0);
    }
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
    let Some(v) = vp.as_mut() else {
        return -1;
    };
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
}

/// Sets the bounded wait `fynkel_apple_viewport_acquire_presented` may spend
/// on the newest in-flight frame (microseconds; 0 disables).
/// # Safety
/// `vp` must be a live viewport.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_set_acquire_wait_us(vp: *const Viewport, us: u32) {
    if let Some(v) = vp.as_ref() {
        v.ring.set_acquire_wait(Duration::from_micros(us as u64));
    }
}

/// Presenter entry point (Flutter raster thread). See
/// [`IoSurfaceRing::acquire_presented`].
/// # Safety
/// `vp` must be a live viewport. Safe to call from any thread.
#[no_mangle]
pub unsafe extern "C" fn fynkel_apple_viewport_acquire_presented(vp: *const Viewport) -> i32 {
    vp.as_ref()
        .map(|v| v.ring.acquire_presented())
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
    if let Some(v) = vp.as_ref() {
        v.ring.set_frame_ready_callback(func, userdata);
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
    let (Some(v), Some(out)) = (vp.as_ref(), out.as_mut()) else {
        return;
    };
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
    };
}
