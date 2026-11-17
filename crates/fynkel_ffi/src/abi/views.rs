//! Views: what a world is drawn into, headless, on Apple and on Android.

use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use fynkel_present::{Headless, ReadError};
use fynkel_render::{Gpu, Renderer};
use fynkel_world::ObjectId;

use crate::device::{Device, WorldGpu};
use crate::error::{Error, FyStatus, guard};
use crate::worker::Worker;

use super::*;

// --- Views -------------------------------------------------------------------

/// Starts a view of the world behind `world`: the target `make` builds on
/// the process's GPU device, a renderer sharing the world's meshes and
/// textures, and the render worker.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
unsafe fn new_view<T: fynkel_present::Target + 'static>(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
    make: impl FnOnce(&Gpu, u32, u32) -> Result<(T, Output), Error>,
) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null("out"));
    }
    if world.is_null() {
        return Err(Error::null("world"));
    }
    // SAFETY: not null, and made by fy_world_new (Arc::into_raw); this adds
    // the view's own reference.
    let world = unsafe {
        Arc::increment_strong_count(world);
        Arc::from_raw(world)
    };
    let device = Device::current().map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
    check_view_size(&device, width, height)?;
    let gpu = {
        let mut held = world.gpu.lock().map_err(|_| Error::new(FyStatus::Poisoned, "a view of this world panicked"))?;
        match held.upgrade() {
            Some(gpu) if Arc::ptr_eq(&gpu.device, &device) => gpu,
            // No view now, or the views were on a device that was lost:
            // what's uploaded starts over.
            _ => {
                let gpu = WorldGpu::new(&device);
                *held = Arc::downgrade(&gpu);
                gpu
            }
        }
    };
    let (target, output) = make(&device.gpu, width, height)?;
    let mut renderer = Renderer::sharing(gpu.assets.clone(), fynkel_present::Target::format(&target));
    // On screen, a shader the driver hasn't seen is compiled beside the
    // frames, its objects left out till it's ready, rather than holding a
    // frame up for a third of a second. Headless views wait, so what they
    // read back is the whole scene.
    renderer.set_wait_for_shaders(matches!(output, Output::Headless(_)));
    let worker = Worker::start_for(gpu.clone(), renderer, target)
        .map_err(|e| Error::new(FyStatus::WorkerFailed, format!("couldn't start the render worker: {e}")))?;
    let view = FyView { world, gpu, clock: Mutex::default(), worker, output };
    // SAFETY: checked non-null above; the caller promises it's writable.
    unsafe { out.write(Box::into_raw(Box::new(view))) };
    Ok(())
}

/// Whether `device` can draw a view of `width` × `height`: past its largest
/// texture, making the targets would lose the device for every view.
fn check_view_size(device: &Device, width: u32, height: u32) -> Result<(), Error> {
    let most = device.gpu.device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > most || height > most {
        return Err(Error::new(
            FyStatus::InvalidValue,
            format!("a view can't be {width} × {height}: each side is from 1 to {most} on this device"),
        ));
    }
    Ok(())
}

/// A view of `world` that draws `width` × `height` frames offscreen and
/// reads them back (`fy_view_read`). Starts its render worker. Free it with
/// `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_headless(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, frames) = Headless::new(&gpu.device, &gpu.queue, width, height);
                Ok((target, Output::Headless(frames)))
            })
        }
    })
}

/// A view of `world` that draws into a ring of IOSurfaces for Flutter to
/// show (macOS and iOS; `FY_STATUS_UNSUPPORTED` elsewhere). The Flutter
/// plugin shows them with `fy_apple_acquire` and learns of new frames
/// through `fy_apple_set_frame_ready`. Free it with `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_apple(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        #[cfg(target_vendor = "apple")]
        // SAFETY: per this function's contract.
        // Dart calls in on a thread with no autorelease pool, so what
        // Metal autoreleases while the device and surfaces are made would
        // outlive the view: drain it here.
        return fynkel_present::autorelease_pool(|| unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, presenter) = fynkel_present::Apple::new(&gpu.device, &gpu.queue, width, height)
                    .map_err(|e| Error::new(FyStatus::NoGpu, e.to_string()))?;
                Ok((target, Output::Apple(presenter)))
            })
        });
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (world, width, height, out);
            Err(Error::new(FyStatus::Unsupported, "Apple views exist only on macOS and iOS"))
        }
    })
}

/// A view of `world` that draws straight to a `CAMetalLayer` the Flutter
/// plugin puts under Flutter's view (iOS and macOS;
/// `FY_STATUS_UNSUPPORTED` elsewhere). It draws nothing until the plugin
/// gives it a layer (`fy_apple_layer_attach`), and each layer brings its
/// own size: `fy_view_resize` changes nothing. Free it with
/// `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_apple_layer(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        #[cfg(target_vendor = "apple")]
        // SAFETY: per this function's contract.
        return fynkel_present::autorelease_pool(|| unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, presenter) =
                    fynkel_present::Layer::new(&gpu.instance, &gpu.adapter, &gpu.device, &gpu.queue, width, height);
                Ok((target, Output::AppleLayer(presenter)))
            })
        });
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (world, width, height, out);
            Err(Error::new(FyStatus::Unsupported, "Apple views exist only on macOS and iOS"))
        }
    })
}

/// A view of `world` that draws into an Android `Surface` for Flutter to
/// show (Android only; `FY_STATUS_UNSUPPORTED` elsewhere). It draws nothing
/// until the Flutter plugin gives it a surface, and each new surface brings
/// its own size: the plugin's JNI calls (`FynkelNative.attach` and
/// `detach`) do both, and `fy_view_resize` changes nothing. `width` ×
/// `height` is the size until the first surface. Free it with
/// `fy_view_free`.
///
/// # Safety
///
/// `world` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_new_android(
    world: *const FyWorld,
    width: u32,
    height: u32,
    out: *mut *mut FyView,
) -> FyStatus {
    guard(|| {
        #[cfg(target_os = "android")]
        // SAFETY: per this function's contract.
        return unsafe {
            new_view(world, width, height, out, |gpu, width, height| {
                let (target, presenter) =
                    fynkel_present::Android::new(&gpu.instance, &gpu.adapter, &gpu.device, &gpu.queue, width, height);
                Ok((target, Output::Android(presenter)))
            })
        };
        #[cfg(not(target_os = "android"))]
        {
            let _ = (world, width, height, out);
            Err(Error::new(FyStatus::Unsupported, "Android views exist only on Android"))
        }
    })
}

/// Stops a view's render worker (after the frame it's drawing) and frees
/// the view. Null is ignored.
///
/// # Safety
///
/// `view` is null or a live view, not used again after this, and no other
/// thread is inside a call on it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_free(view: *mut FyView) {
    if !view.is_null() {
        // A panic here leaks the view rather than abort (fy_world_free).
        let _ = guard(|| {
            // SAFETY: made by Box::into_raw in new_view; the caller gives
            // it up. In a pool, like creation, for what Metal autoreleases.
            fynkel_present::autorelease_pool(|| drop(unsafe { Box::from_raw(view) }));
            Ok(())
        });
    }
}

/// One frame: copies the world into a scene and hands it to the render
/// worker, without waiting for the GPU. Writes the frame's number (1, 2, …)
/// to `out_number`.
///
/// # Safety
///
/// `view` is null or live; `out_number` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_frame(view: *const FyView, out_number: *mut u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        if out_number.is_null() {
            return Err(Error::null("out_number"));
        }
        let mut clock = view.clock.lock().map_err(|_| Error::new(FyStatus::Poisoned, "the view's clock panicked"))?;
        let clock = &mut *clock;
        let mut scene = view.worker.scene();
        clock.extract.shake = *view.world.shake.lock().unwrap_or_else(PoisonError::into_inner);
        clock.extract.quality = *view.world.quality.lock().unwrap_or_else(PoisonError::into_inner);
        clock.extract.sky.clone_from(&view.world.sky.lock().unwrap_or_else(PoisonError::into_inner));
        clock.extract.fog = *view.world.fog.lock().unwrap_or_else(PoisonError::into_inner);
        let world = match view.world.lock() {
            Ok(world) => world,
            Err(error) => {
                view.worker.recycle(scene);
                return Err(error);
            }
        };
        // The extract only reads the world: a panic in it (a bug) is caught
        // while the world's lock is held, so the lock isn't poisoned and the
        // game and its other views go on. It goes on unwinding after, past
        // this view's clock alone, which it poisons: the view fails.
        let panic_next = std::mem::take(&mut clock.panic_next);
        let extracted = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut ledger = view.gpu.ledger();
            assert!(!panic_next, "a test made the extract panic");
            clock.extract.run_shared(&world, &mut scene, &mut ledger)
        }));
        drop(world);
        match extracted {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                view.worker.recycle(scene);
                return Err(Error::new(FyStatus::InvalidValue, message));
            }
            Err(payload) => panic::resume_unwind(payload),
        }
        let number = clock.last_number + 1;
        view.worker.post(number, scene, &mut Vec::new())?;
        clock.last_number = number;
        // SAFETY: checked non-null above; the caller promises it's writable.
        unsafe { out_number.write(number) };
        Ok(())
    })
}

/// Headless views: waits up to `timeout_ms` for frame `number` (or a later
/// one) to finish, copies its pixels into `rgba` (width × height × 4 bytes,
/// RGBA8 sRGB, rows top to bottom) and writes the number of the frame read
/// to `out_number`. Blocks; never call it from a UI thread.
///
/// # Safety
///
/// `view` is null or live; `rgba` is null or valid for `len` bytes of
/// writes; `out_number` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_read(
    view: *const FyView,
    number: u64,
    timeout_ms: u32,
    rgba: *mut u8,
    len: usize,
    out_number: *mut u64,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        if rgba.is_null() {
            return Err(Error::null("rgba"));
        }
        if out_number.is_null() {
            return Err(Error::null("out_number"));
        }
        #[allow(irrefutable_let_patterns)] // only headless views on desktop Linux and Windows
        let Output::Headless(frames) = &view.output else {
            return Err(Error::new(FyStatus::InvalidValue, "not a headless view"));
        };
        // SAFETY: not null, and the caller promises len writable bytes.
        let rgba = unsafe { std::slice::from_raw_parts_mut(rgba, len) };
        let read = frames.read(number, Duration::from_millis(timeout_ms.into()), rgba).map_err(|e| {
            // A frame that never came may be the worker's fault.
            if let Some(failure) = view.worker.failure() {
                return failure;
            }
            let status = match e {
                ReadError::Timeout { .. } => FyStatus::Timeout,
                ReadError::WrongSize { .. } => FyStatus::WrongSize,
            };
            Error::new(status, e.to_string())
        })?;
        // SAFETY: checked non-null above; the caller promises it's writable.
        unsafe { out_number.write(read) };
        Ok(())
    })
}

/// Draws the view's frames from the next `fy_view_frame` on from the camera
/// on object `camera`, or from the world's main camera (`fy_camera_active`)
/// when it's 0, as a new view does. A view with a camera of its own draws
/// black while that object is hidden, gone, or has no camera or transform:
/// a minimap whose camera goes stays a minimap, not a second main view.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_camera_set(view: *const FyView, camera: u64) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        let mut clock = view.clock.lock().map_err(|_| Error::new(FyStatus::Poisoned, "the view's clock panicked"))?;
        clock.extract.camera = ObjectId::from_raw(camera);
        Ok(())
    })
}

/// Draws the frames from the next `fy_view_frame` on at `width` × `height`
/// pixels. Frames already on their way finish at the old size and, on
/// Apple, are never shown.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_resize(view: *const FyView, width: u32, height: u32) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        check_view_size(&view.gpu.device, width, height)?;
        view.worker.resize(width, height)
    })
}

/// Whether the test hooks may run: only in a test process, one with
/// `FLUTTER_TEST` (set by `flutter test`) or `FYNKEL_TEST_HOOKS` in its
/// environment. A game's hook calls fail with `FY_STATUS_UNSUPPORTED`.
fn test_hooks() -> Result<(), Error> {
    if std::env::var_os("FLUTTER_TEST").is_some() || std::env::var_os("FYNKEL_TEST_HOOKS").is_some() {
        return Ok(());
    }
    Err(Error::new(FyStatus::Unsupported, "test hooks run only in tests (FLUTTER_TEST or FYNKEL_TEST_HOOKS set)"))
}

/// For tests only (see `fy_view_fail_worker`): destroys the GPU device, as
/// a driver reset would. Every view shares it, so soon after,
/// `fy_view_frame` returns `FY_STATUS_DEVICE_LOST` for each; a view made
/// after has a new one.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_lose_device(view: *const FyView) -> FyStatus {
    guard(|| {
        test_hooks()?;
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        view.worker.lose_device();
        Ok(())
    })
}

/// For tests only: makes the view's render worker panic before its next
/// frame, as a bug in it would, so soon after `fy_view_frame` returns
/// `FY_STATUS_WORKER_FAILED`. Runs only in a test process (`FLUTTER_TEST`
/// or `FYNKEL_TEST_HOOKS` set); `FY_STATUS_UNSUPPORTED` otherwise.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_fail_worker(view: *const FyView) -> FyStatus {
    guard(|| {
        test_hooks()?;
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        view.worker.panic();
        Ok(())
    })
}

/// For tests only: makes the view's next `fy_view_frame` panic while it
/// copies the world, as a bug in the extract would; that frame returns
/// `FY_STATUS_PANIC`. Runs only in a test process (`FLUTTER_TEST` or
/// `FYNKEL_TEST_HOOKS` set); `FY_STATUS_UNSUPPORTED` otherwise.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_panic_extract(view: *const FyView) -> FyStatus {
    guard(|| {
        test_hooks()?;
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        view.clock.lock().map_err(|_| Error::new(FyStatus::Poisoned, "the view's clock panicked"))?.panic_next = true;
        Ok(())
    })
}

/// What a view did since it was made.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FyViewStats {
    /// Scenes handed to the render worker by `fy_view_frame`.
    pub posted: u64,
    /// Scenes drawn.
    pub rendered: u64,
    /// Scenes replaced by a newer one before they were drawn.
    pub coalesced: u64,
    /// Scenes not drawn because the target had nowhere to draw.
    pub skipped: u64,
    /// Apple: new frames taken by Flutter (`fy_apple_acquire`). Android:
    /// frames given to the surface. 0 for headless views.
    pub presented: u64,
    /// Apple: frames finished but never shown, because newer ones were
    /// waiting. 0 for other views (Android's queue is the OS's).
    pub dropped: u64,
    /// GPU memory the world's textures hold, in bytes: every mip level, as
    /// stored, once however many views the world has. Up to date as of the
    /// view's last frame drawn.
    pub texture_bytes: u64,
    /// GPU memory the world's meshes hold, in bytes.
    pub mesh_bytes: u64,
    /// GPU memory the sky's light holds for this view, in bytes: 0 for no
    /// sky or a plain one.
    pub sky_bytes: u64,
    /// GPU memory the view's shadow maps hold, in bytes.
    pub shadow_bytes: u64,
    /// GPU memory the image the scene is drawn into and ambient occlusion's
    /// images hold, in bytes.
    pub target_bytes: u64,
    /// GPU memory the frames on their way to the screen hold, in bytes: 0
    /// when the target doesn't own them.
    pub frame_bytes: u64,
    /// GPU memory the images hold for what's behind see-through surfaces
    /// and soft particles, in bytes.
    pub behind_bytes: u64,
    /// GPU memory bloom's images hold, in bytes.
    pub bloom_bytes: u64,
    /// GPU memory the screen effects' pictures hold, in bytes.
    pub effect_bytes: u64,
    /// Microseconds the view's render thread spent on scenes, from taking
    /// one to being done with it: busy, against waiting for the next. On
    /// Android it includes waiting for the GPU to finish each frame.
    pub busy_micros: u64,
    /// Frames timed on the GPU while `fy_gpu_timing_set` had timing on,
    /// and the GPU's time over them for each part of a frame, in
    /// microseconds. Each frame's come back a frame or two after it's
    /// drawn. Each part counts only the time it adds past the parts
    /// before it, so they never overlap.
    pub gpu_frames: u64,
    /// The shadow maps.
    pub gpu_shadows_micros: u64,
    /// A new sky's light, worked out when the sky changes.
    pub gpu_sky_light_micros: u64,
    /// Ambient occlusion.
    pub gpu_ambient_occlusion_micros: u64,
    /// The scene itself.
    pub gpu_scene_micros: u64,
    /// Bloom.
    pub gpu_bloom_micros: u64,
    /// The scene image onto the view.
    pub gpu_finish_micros: u64,
    /// The screen effects.
    pub gpu_effects_micros: u64,
    /// Passes in the frames timed whose clocks the GPU didn't give, left
    /// out of the times: many of them say the times fall short.
    pub gpu_untimed_passes: u64,
}

/// Lets go of the images `view` draws through, between frames and without
/// waiting for one, for when the system is short of memory and the view
/// isn't drawing. Its next frame makes them again, a little slower.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_release_images(view: *const FyView) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        view.worker.release_images();
        Ok(())
    })
}

/// Times every view's frames on the GPU, each part of a frame on its own,
/// or stops (the default): for finding what a frame's GPU time goes on.
/// The times are in [`FyViewStats`]. Timing costs a little GPU time, and
/// on Apple GPUs keeps some driver memory in use; where the GPU can't time
/// its work, nothing is timed.
#[unsafe(no_mangle)]
pub extern "C" fn fy_gpu_timing_set(on: bool) {
    crate::worker::GPU_TIMING.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// # Safety
///
/// `view` is null or live; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_view_stats(view: *const FyView, out: *mut FyViewStats) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        let w = view.worker.stats();
        let mut stats = FyViewStats {
            posted: w.posted,
            rendered: w.rendered,
            coalesced: w.coalesced,
            skipped: w.skipped,
            texture_bytes: w.texture_bytes,
            mesh_bytes: w.mesh_bytes,
            sky_bytes: w.sky_bytes,
            shadow_bytes: w.shadow_bytes,
            target_bytes: w.target_bytes,
            frame_bytes: w.frame_bytes,
            behind_bytes: w.behind_bytes,
            bloom_bytes: w.bloom_bytes,
            effect_bytes: w.effect_bytes,
            busy_micros: w.busy_micros,
            gpu_frames: w.gpu.frames,
            gpu_shadows_micros: w.gpu.shadows / 1000,
            gpu_sky_light_micros: w.gpu.sky_light / 1000,
            gpu_ambient_occlusion_micros: w.gpu.ambient_occlusion / 1000,
            gpu_scene_micros: w.gpu.scene / 1000,
            gpu_bloom_micros: w.gpu.bloom / 1000,
            gpu_finish_micros: w.gpu.finish / 1000,
            gpu_effects_micros: w.gpu.effects / 1000,
            gpu_untimed_passes: w.gpu.untimed,
            ..Default::default()
        };
        #[cfg(target_vendor = "apple")]
        if let Output::Apple(presenter) = &view.output {
            let ring = presenter.stats();
            (stats.presented, stats.dropped) = (ring.presented, ring.dropped);
        }
        #[cfg(target_vendor = "apple")]
        if let Output::AppleLayer(presenter) = &view.output {
            stats.presented = presenter.presented();
        }
        #[cfg(target_os = "android")]
        if let Some(presenter) = view.android() {
            stats.presented = presenter.presented();
        }
        // SAFETY: per this function's contract.
        unsafe { write(out, stats, "out") }
    })
}

/// `fy_apple_acquire`: a surface was written to `out_surface`.
pub const FY_APPLE_SHOWN: u32 = 1;

/// `fy_apple_acquire`: another finished frame is waiting; ask Flutter for
/// one more frame.
pub const FY_APPLE_MORE: u32 = 2;

/// For Flutter's raster thread: the IOSurface to show now. Returns
/// `FY_APPLE_SHOWN` (plus `FY_APPLE_MORE` when another frame waits) and
/// writes the surface, retained once for the caller, to `out_surface`; or 0
/// with nothing written, when there's no frame yet or `view` isn't an Apple
/// view. Never blocks on the GPU and allocates nothing.
///
/// # Safety
///
/// `view` is null or live; `out_surface` is null or writable. The caller
/// releases the surface (`CFRelease`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_acquire(view: *const FyView, out_surface: *mut *mut c_void) -> u32 {
    #[cfg(target_vendor = "apple")]
    {
        let acquire = || -> u32 {
            // SAFETY: per this function's contract.
            let Ok(view) = (unsafe { handle(view, "view") }) else { return 0 };
            let Output::Apple(presenter) = &view.output else { return 0 };
            if out_surface.is_null() {
                return 0;
            }
            let Some(shown) = presenter.acquire() else { return 0 };
            let more = if shown.more { FY_APPLE_MORE } else { 0 };
            // SAFETY: checked non-null above; the caller promises it's
            // writable.
            unsafe { out_surface.write(shown.into_raw()) };
            FY_APPLE_SHOWN | more
        };
        // The raster thread is Flutter's: nothing may unwind into it.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(acquire)).unwrap_or(0)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (view, out_surface);
        0
    }
}

/// Called on a GPU thread each time an Apple view finishes a frame.
pub type FyFrameReady = Option<unsafe extern "C" fn(userdata: *mut c_void)>;

/// Sets the function an Apple view calls, on a GPU thread, each time a
/// frame finishes (null for none). Waits for a call in progress, so once
/// this returns the old `userdata` is no longer used.
///
/// # Safety
///
/// `view` is null or live; `function` must be safe to call with `userdata`
/// from any thread until it is replaced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_set_frame_ready(
    view: *const FyView,
    function: FyFrameReady,
    userdata: *mut c_void,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        #[cfg(target_vendor = "apple")]
        if let Output::Apple(presenter) = &view.output {
            // SAFETY: promised by this function's caller.
            unsafe { presenter.set_frame_ready(function, userdata) };
            return Ok(());
        }
        let _ = (view, function, userdata);
        Err(Error::new(FyStatus::Unsupported, "not an Apple view"))
    })
}

/// Draws an Apple layer view's next frames to `layer` (a `CAMetalLayer`)
/// at `width` × `height` pixels. It never waits for the render thread: a
/// frame in progress ends on the old layer, and a new size is set before
/// the next frame. For the Flutter plugin, on the main thread.
///
/// # Safety
///
/// `view` is null or live; `layer` is null or a live `CAMetalLayer`, kept
/// alive until `fy_apple_layer_detach` or the next attach returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_layer_attach(
    view: *const FyView,
    layer: *mut c_void,
    width: u32,
    height: u32,
) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        #[cfg(target_vendor = "apple")]
        if let Output::AppleLayer(presenter) = &view.output {
            let layer = std::ptr::NonNull::new(layer).ok_or_else(|| Error::null("layer"))?;
            // SAFETY: promised by this function's caller.
            return fynkel_present::autorelease_pool(|| unsafe { presenter.attach(layer, width, height) })
                .map_err(|e| Error::new(FyStatus::NoGpu, e));
        }
        let _ = (view, layer, width, height);
        Err(Error::new(FyStatus::Unsupported, "not an Apple layer view"))
    })
}

/// Stops an Apple layer view drawing to its layer, without waiting for a
/// frame in progress, which ends on it: the layer may then go (the frame's
/// surface keeps it alive). Frames are skipped until the next
/// `fy_apple_layer_attach`.
///
/// # Safety
///
/// `view` is null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fy_apple_layer_detach(view: *const FyView) -> FyStatus {
    guard(|| {
        // SAFETY: per this function's contract.
        let view = unsafe { handle(view, "view") }?;
        #[cfg(target_vendor = "apple")]
        if let Output::AppleLayer(presenter) = &view.output {
            presenter.detach();
            return Ok(());
        }
        let _ = view;
        Err(Error::new(FyStatus::Unsupported, "not an Apple layer view"))
    })
}
