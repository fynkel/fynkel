//! M0 Flutter viewport experiment (Path A, blueprint §8.2 / ADR-0002).
//!
//! Renders the M0 scene with wgpu/Metal into a ring of IOSurface-backed
//! textures that the Flutter macOS embedder can present through its
//! `FlutterTexture` protocol without a CPU copy.
//!
//! This is a throwaway C ABI for the experiment, not `fynkel_native`.
//!
//! Synchronization contract under test (ADR-0002):
//! * the imported textures are created on the **same `MTLDevice`** that wgpu
//!   owns (asserted at creation);
//! * a ring of N IOSurfaces with an explicit per-buffer state machine;
//! * a buffer is handed to Flutter only after the GPU work that renders into
//!   it has **completed** (`on_submitted_work_done`, or a blocking wait in
//!   `SyncMode::Blocking`);
//! * the buffer Flutter is presenting, and the one it presented before that
//!   (possibly still sampled by an in-flight Flutter frame), are never
//!   rendered into.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_io_surface::{
    kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat, kIOSurfaceWidth,
    IOSurfaceRef,
};
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLResource, MTLStorageMode, MTLTexture, MTLTextureDescriptor,
    MTLTextureType, MTLTextureUsage,
};

use fynkel_m0_scene::CubeRenderer;

/// `kCVPixelFormatType_32BGRA` — what Flutter's macOS embedder expects.
const PIXEL_FORMAT_BGRA: i32 = 0x4247_5241; // 'BGRA'
const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncMode {
    /// Mark the buffer ready from wgpu's submitted-work-done callback.
    Async = 0,
    /// Block on the queue after submit; the buffer is ready when `render` returns.
    Blocking = 1,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BufferState {
    Free,
    /// Submitted; GPU may still be writing.
    Rendering,
    /// GPU work complete; may be handed to Flutter.
    Ready,
    /// Flutter's current frame.
    Presented,
    /// Flutter's previous frame; may still be sampled by an in-flight raster.
    Retiring,
}

struct Slot {
    state: BufferState,
    /// Monotonic render sequence number, to pick the newest Ready buffer.
    seq: u64,
    /// Owns the imported texture; `view` borrows from it logically.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct Ring {
    slots: Vec<Slot>,
    next_seq: u64,
}

#[derive(Default)]
pub struct Stats {
    pub frames_rendered: u64,
    pub frames_dropped_no_free_buffer: u64,
    pub frames_presented: u64,
    pub presents_without_new_frame: u64,
    pub cpu_us: Vec<u32>,
    pub gpu_complete_us: Vec<u32>,
}

pub struct Context {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: CubeRenderer,
    width: u32,
    height: u32,
    sync_mode: SyncMode,
    ring: Arc<Mutex<Ring>>,
    stats: Arc<Mutex<Stats>>,
    frame_ready_cb: Arc<FrameReadyCallback>,
    /// Kept alive for the lifetime of the textures; indexed like `ring.slots`.
    surfaces: Vec<CFRetained<IOSurfaceRef>>,
    _mtl_device: Retained<ProtocolObject<dyn MTLDevice>>,
}

type FrameReadyFn = unsafe extern "C" fn(*mut c_void);

struct FrameReadyCallback {
    func: Mutex<Option<(FrameReadyFn, usize)>>,
    /// Incremented for each ready frame; lets a host detect missed signals.
    signalled: AtomicU32,
}

impl FrameReadyCallback {
    fn fire(&self) {
        self.signalled.fetch_add(1, Ordering::Relaxed);
        if let Some((f, ud)) = *self.func.lock().unwrap() {
            // SAFETY: the host registered this pointer pair and guarantees its
            // validity until it clears it or destroys the context.
            unsafe { f(ud as *mut c_void) };
        }
    }
}

impl Context {
    pub fn new(
        width: u32,
        height: u32,
        buffer_count: u32,
        sync_mode: SyncMode,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no adapter: {e}"))?;
        let info = adapter.get_info();
        log::info!("fynkel_m0 adapter: {} ({:?})", info.name, info.backend);

        let (device, queue) = pollster_block(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fynkel_m0 device"),
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;

        // The MTLDevice wgpu owns. Every imported texture must come from it.
        let mtl_device: Retained<ProtocolObject<dyn MTLDevice>> = unsafe {
            let hal = device
                .as_hal::<wgpu_hal::api::Metal>()
                .ok_or("device is not Metal")?;
            hal.raw_device().clone()
        };

        let renderer = CubeRenderer::new(&device, TEXTURE_FORMAT, width, height);

        let mut slots = Vec::with_capacity(buffer_count as usize);
        let mut surfaces = Vec::with_capacity(buffer_count as usize);
        for i in 0..buffer_count {
            let surface = create_iosurface(width, height)?;
            let texture = import_iosurface(&device, &mtl_device, &surface, width, height, i)?;
            let view = texture.create_view(&Default::default());
            slots.push(Slot {
                state: BufferState::Free,
                seq: 0,
                _texture: texture,
                view,
            });
            surfaces.push(surface);
        }

        Ok(Self {
            device,
            queue,
            renderer,
            width,
            height,
            sync_mode,
            ring: Arc::new(Mutex::new(Ring { slots, next_seq: 1 })),
            stats: Arc::new(Mutex::new(Stats::default())),
            frame_ready_cb: Arc::new(FrameReadyCallback {
                func: Mutex::new(None),
                signalled: AtomicU32::new(0),
            }),
            surfaces,
            _mtl_device: mtl_device,
        })
    }

    pub fn iosurface_ptr(&self, index: u32) -> *const c_void {
        match self.surfaces.get(index as usize) {
            Some(s) => CFRetained::as_ptr(s).as_ptr() as *const c_void,
            None => std::ptr::null(),
        }
    }

    /// Render one frame into a free buffer. Returns the buffer index, or -1
    /// if every buffer is in use (the frame is dropped).
    pub fn render(&self, time_secs: f32) -> i32 {
        let t0 = Instant::now();
        let (index, seq) = {
            let mut ring = self.ring.lock().unwrap();
            let Some(i) = ring.slots.iter().position(|s| s.state == BufferState::Free) else {
                self.stats.lock().unwrap().frames_dropped_no_free_buffer += 1;
                return -1;
            };
            let seq = ring.next_seq;
            ring.next_seq += 1;
            ring.slots[i].state = BufferState::Rendering;
            ring.slots[i].seq = seq;
            (i, seq)
        };

        self.renderer
            .update(&self.queue, time_secs, self.width, self.height);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("m0 frame"),
            });
        {
            let ring = self.ring.lock().unwrap();
            self.renderer.draw(&mut encoder, &ring.slots[index].view);
        }
        self.queue.submit(Some(encoder.finish()));

        let ring = self.ring.clone();
        let stats = self.stats.clone();
        let cb = self.frame_ready_cb.clone();
        let submitted = Instant::now();
        let mark_ready = move || {
            {
                let mut ring = ring.lock().unwrap();
                let slot = &mut ring.slots[index];
                // A resize may have recycled the slot; only promote our own frame.
                if slot.state == BufferState::Rendering && slot.seq == seq {
                    slot.state = BufferState::Ready;
                }
            }
            stats
                .lock()
                .unwrap()
                .gpu_complete_us
                .push(submitted.elapsed().as_micros() as u32);
            cb.fire();
        };

        match self.sync_mode {
            SyncMode::Async => self.queue.on_submitted_work_done(mark_ready),
            SyncMode::Blocking => {
                let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
                mark_ready();
            }
        }

        let mut st = self.stats.lock().unwrap();
        st.frames_rendered += 1;
        st.cpu_us.push(t0.elapsed().as_micros() as u32);
        index as i32
    }

    /// Called by the presenter (Flutter's raster thread) when it wants a frame.
    /// Returns the newest GPU-complete buffer and rotates Presented/Retiring.
    /// If nothing new is ready, returns the currently presented buffer again
    /// (or -1 if nothing has ever been presented).
    pub fn acquire_presented(&self) -> i32 {
        let mut ring = self.ring.lock().unwrap();
        let newest_ready = ring
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.state == BufferState::Ready)
            .max_by_key(|(_, s)| s.seq)
            .map(|(i, _)| i);

        let mut stats = self.stats.lock().unwrap();
        match newest_ready {
            Some(new) => {
                for (i, s) in ring.slots.iter_mut().enumerate() {
                    s.state = match s.state {
                        BufferState::Retiring => BufferState::Free,
                        BufferState::Presented => BufferState::Retiring,
                        // Older ready frames were never shown; recycle them.
                        BufferState::Ready if i != new => BufferState::Free,
                        other => other,
                    };
                }
                ring.slots[new].state = BufferState::Presented;
                stats.frames_presented += 1;
                new as i32
            }
            None => {
                stats.presents_without_new_frame += 1;
                ring.slots
                    .iter()
                    .position(|s| s.state == BufferState::Presented)
                    .map(|i| i as i32)
                    .unwrap_or(-1)
            }
        }
    }

    pub fn set_frame_ready_callback(&self, func: Option<FrameReadyFn>, userdata: *mut c_void) {
        *self.frame_ready_cb.func.lock().unwrap() = func.map(|f| (f, userdata as usize));
    }

    pub fn take_stats(&self) -> Stats {
        std::mem::take(&mut *self.stats.lock().unwrap())
    }
}

fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    // Tiny inline executor; avoids pulling pollster into the cdylib.
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::sleep(Duration::from_micros(50));
    }
}

fn create_iosurface(width: u32, height: u32) -> Result<CFRetained<IOSurfaceRef>, String> {
    let keys: [&CFString; 4] = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfacePixelFormat,
        ]
    };
    let w = CFNumber::new_i32(width as i32);
    let h = CFNumber::new_i32(height as i32);
    let bpe = CFNumber::new_i32(4);
    let fmt = CFNumber::new_i32(PIXEL_FORMAT_BGRA);
    let values: [&CFNumber; 4] = [&w, &h, &bpe, &fmt];
    let props: CFRetained<CFDictionary<CFString, CFNumber>> =
        CFDictionary::from_slices(&keys, &values);
    let props: &CFDictionary = props.as_opaque();
    unsafe { IOSurfaceRef::new(props) }.ok_or_else(|| "IOSurfaceCreate failed".to_string())
}

fn import_iosurface(
    device: &wgpu::Device,
    mtl_device: &ProtocolObject<dyn MTLDevice>,
    surface: &IOSurfaceRef,
    width: u32,
    height: u32,
    index: u32,
) -> Result<wgpu::Texture, String> {
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm,
            width as usize,
            height as usize,
            false,
        )
    };
    desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    desc.setStorageMode(MTLStorageMode::Shared);
    let raw: Retained<ProtocolObject<dyn MTLTexture>> = mtl_device
        .newTextureWithDescriptor_iosurface_plane(&desc, surface, 0)
        .ok_or_else(|| format!("newTextureWithDescriptor:iosurface: failed for buffer {index}"))?;

    // Same-device assertion: the texture's device must be wgpu's device.
    let tex_device = raw.device();
    if !std::ptr::eq(
        Retained::as_ptr(&tex_device) as *const c_void,
        mtl_device as *const _ as *const c_void,
    ) {
        return Err("imported texture is on a different MTLDevice than wgpu".into());
    }

    let hal_texture = unsafe {
        wgpu_hal::metal::Device::texture_from_raw(
            raw,
            TEXTURE_FORMAT,
            MTLTextureType::Type2D,
            1,
            1,
            wgpu_hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
            None,
        )
    };
    let texture = unsafe {
        device.create_texture_from_hal::<wgpu_hal::api::Metal>(
            hal_texture,
            &wgpu::TextureDescriptor {
                label: Some("m0 iosurface"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TEXTURE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::TextureUses::UNINITIALIZED,
        )
    };
    Ok(texture)
}

// ---------------------------------------------------------------------------
// Throwaway C ABI for the M0 experiment.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn fynkel_m0_create(
    width: u32,
    height: u32,
    buffer_count: u32,
    sync_mode: i32,
) -> *mut Context {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
    let mode = if sync_mode == 1 {
        SyncMode::Blocking
    } else {
        SyncMode::Async
    };
    match Context::new(width.max(1), height.max(1), buffer_count.clamp(2, 8), mode) {
        Ok(ctx) => Box::into_raw(Box::new(ctx)),
        Err(e) => {
            log::error!("fynkel_m0_create failed: {e}");
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `ctx` must come from `fynkel_m0_create` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_destroy(ctx: *mut Context) {
    if !ctx.is_null() {
        drop(Box::from_raw(ctx));
    }
}

/// # Safety
/// `ctx` must be a live context.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_iosurface(ctx: *const Context, index: u32) -> *const c_void {
    ctx.as_ref()
        .map(|c| c.iosurface_ptr(index))
        .unwrap_or(std::ptr::null())
}

/// # Safety
/// `ctx` must be a live context.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_render(ctx: *const Context, time_secs: f32) -> i32 {
    ctx.as_ref().map(|c| c.render(time_secs)).unwrap_or(-1)
}

/// # Safety
/// `ctx` must be a live context. Safe to call from any thread.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_acquire_presented(ctx: *const Context) -> i32 {
    ctx.as_ref().map(|c| c.acquire_presented()).unwrap_or(-1)
}

/// # Safety
/// `ctx` must be a live context; `func`/`userdata` must stay valid until cleared.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_set_frame_ready_callback(
    ctx: *const Context,
    func: Option<FrameReadyFn>,
    userdata: *mut c_void,
) {
    if let Some(c) = ctx.as_ref() {
        c.set_frame_ready_callback(func, userdata);
    }
}

#[repr(C)]
pub struct M0Stats {
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
}

fn pct(v: &mut [u32], q: f64) -> u32 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[((v.len() as f64 - 1.0) * q).round() as usize]
}

/// Drains and summarizes stats since the previous call.
/// # Safety
/// `ctx` must be a live context; `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn fynkel_m0_take_stats(ctx: *const Context, out: *mut M0Stats) {
    let (Some(c), Some(out)) = (ctx.as_ref(), out.as_mut()) else {
        return;
    };
    let mut s = c.take_stats();
    *out = M0Stats {
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
    };
}

impl Context {
    /// Test/diagnostic helper: read one BGRA pixel back from an IOSurface on the CPU.
    /// This is exactly the readback the production path must never do per frame.
    pub fn read_pixel(&self, index: u32, x: u32, y: u32) -> Option<[u8; 4]> {
        use objc2_io_surface::IOSurfaceLockOptions;
        let s = self.surfaces.get(index as usize)?;
        unsafe {
            if s.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) != 0 {
                return None;
            }
            let base = s.base_address().as_ptr() as *const u8;
            let off = (y as usize) * s.bytes_per_row() + (x as usize) * 4;
            let p = std::ptr::read_unaligned(base.add(off) as *const [u8; 4]);
            s.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
            Some(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_bgra() -> [u8; 4] {
        // CubeRenderer clears to (0.08, 0.09, 0.11); BGRA order, sRGB-less Unorm.
        [
            (0.11f32 * 255.0).round() as u8,
            (0.09f32 * 255.0).round() as u8,
            (0.08f32 * 255.0).round() as u8,
            255,
        ]
    }

    #[test]
    fn renders_into_iosurface_ring_and_rotates_states() {
        for mode in [SyncMode::Blocking, SyncMode::Async] {
            let ctx = Context::new(256, 256, 4, mode).expect("context");
            assert!(!ctx.iosurface_ptr(0).is_null());
            assert!(ctx.iosurface_ptr(4).is_null());
            assert_eq!(ctx.acquire_presented(), -1, "nothing presented yet");

            let i = ctx.render(0.0);
            assert_eq!(i, 0);
            if mode == SyncMode::Async {
                // Wait for the GPU completion callback.
                let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(ctx.acquire_presented(), 0, "first frame is presented");

            // Corner pixel is background; centre pixel is the lit cube.
            let corner = ctx.read_pixel(0, 2, 2).unwrap();
            let centre = ctx.read_pixel(0, 128, 128).unwrap();
            assert_eq!(
                corner,
                clear_bgra(),
                "corner should be the clear colour ({mode:?})"
            );
            assert_ne!(
                centre,
                clear_bgra(),
                "centre should show the cube ({mode:?})"
            );
            assert_eq!(centre[3], 255);

            // Present with nothing new returns the same buffer.
            assert_eq!(ctx.acquire_presented(), 0);

            // Render three more; with 4 buffers and one Presented, three are free.
            assert_eq!(ctx.render(0.1), 1);
            assert_eq!(ctx.render(0.2), 2);
            assert_eq!(ctx.render(0.3), 3);
            assert_eq!(
                ctx.render(0.4),
                -1,
                "ring full: frame dropped, not overwritten"
            );
            if mode == SyncMode::Async {
                let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
                std::thread::sleep(Duration::from_millis(20));
            }
            // Newest ready wins; older ready buffers are recycled; 0 retires.
            assert_eq!(ctx.acquire_presented(), 3);
            assert_eq!(ctx.render(0.5), 1, "0 is Retiring; 1 is the first Free");
            let st = ctx.take_stats();
            assert_eq!(st.frames_rendered, 5);
            assert_eq!(st.frames_dropped_no_free_buffer, 1);
            assert_eq!(st.frames_presented, 2);
            assert!(st.cpu_us.len() == 5);
        }
    }
}
