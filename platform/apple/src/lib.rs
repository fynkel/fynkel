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

use fynkel_core::components::{Camera as SceneCamera, ExposureMode, Projection};
use fynkel_core::{Extract, ExtractCamera};
use fynkel_native::FynkelWorld;
use fynkel_render::{Assets, Camera, ExtractRenderer, FrameStats};
use glam::Vec3;

pub use ring::{IoSurfaceRing, RingStats, SyncMode, TEXTURE_FORMAT};

/// Ring + renderer. Owns the GPU device.
pub struct Viewport {
    ring: IoSurfaceRing,
    renderer: ExtractRenderer,
    /// This viewport's own camera, drawn instead of the world's (the
    /// editor's Scene view on a running game, ADR-0021 P7 L5).
    view: Option<(glam::Mat4, SceneCamera)>,
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
        Ok(Self {
            ring,
            renderer,
            view: None,
        })
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
    }

    pub fn view(&self) -> Option<(glam::Mat4, SceneCamera)> {
        self.view
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
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
        let Viewport { ring, renderer, .. } = self;
        let timed_before = renderer.stats().gpu_timed;
        let t0 = std::time::Instant::now();
        renderer.prepare(ring.device(), ring.queue(), extract);
        let prepare_us = t0.elapsed().as_micros() as u32;
        let slot = ring.render_with(|device, _queue, view| {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fynkel viewport frame"),
            });
            renderer.draw(&mut encoder, view);
            encoder.finish()
        });
        let s = renderer.stats();
        let gpu = (s.gpu_timed != timed_before).then_some((
            s.gpu_shadow_us,
            s.gpu_ao_us,
            s.gpu_scene_us,
            s.gpu_post_us,
            s.gpu_frame_us,
        ));
        ring.record_stage_times(extract_us, prepare_us, s.encode_us, gpu);
        slot
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

/// Viewport ABI version; bumped with `include/fynkel_apple.h` (3: the
/// M3-16 instrumentation fields of `FynkelAppleViewportStats`; 4:
/// `fynkel_apple_viewport_debug_lines` and `fynkel_apple_viewport_read_frame`,
/// the editor's overlays and its Save Frame — M4-06; 5: `gpuAo` in the
/// stats — R2; 6: `fynkel_apple_viewport_overlay`, the gizmo overlay —
/// ADR-0019, item 9f; 7: `fynkel_apple_viewport_set_view`, a viewport's
/// own camera over any world — ADR-0021 P7 L5; 8:
/// `fynkel_apple_viewport_outline`, the selection outline).
pub const APPLE_ABI_VERSION: u32 = 8;

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
    let t0 = std::time::Instant::now();
    let mut extract = w.core().extract();
    if let Some((world, camera)) = v.view {
        extract.camera = Some(ExtractCamera {
            record: u32::MAX,
            world,
            camera,
        });
    }
    let extract_us = t0.elapsed().as_micros() as u32;
    v.render_timed(&extract, extract_us)
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
    let Some(v) = vp.as_mut() else { return -1 };
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
    let Some(v) = vp.as_mut() else { return -1 };
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
    let Some(v) = vp.as_mut() else { return -1 };
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
    selected: *const u64,
    selected_count: u32,
    children: *const u64,
    children_count: u32,
    style: *const f32,
) -> i32 {
    let Some(v) = vp.as_mut() else { return -1 };
    if (selected_count > 0 && selected.is_null()) || (children_count > 0 && children.is_null()) {
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
    let entities = |bits: *const u64, count: u32| -> Vec<fynkel_contracts::Entity> {
        if count == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(bits, count as usize)
            .iter()
            .map(|b| fynkel_contracts::Entity::from_bits(*b))
            .collect()
    };
    let (selected, children) = (
        entities(selected, selected_count),
        entities(children, children_count),
    );
    v.renderer.overlay().outline(&selected, &children, style);
    (selected.len() + children.len()) as i32
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
    let Some(v) = vp.as_ref() else { return -1 };
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
