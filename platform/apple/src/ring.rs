//! IOSurface ring: the macOS/Metal synchronization contract (blueprint
//! §8.4, ADR-0002 Path A), moved from the M0 experiment with its tests.
//!
//! Invariants (verified by `tests/ring.rs`):
//! * every imported texture is created on the **same `MTLDevice`** wgpu
//!   owns (asserted at import; a mismatch is a hard failure);
//! * a ring of N IOSurfaces with an explicit per-slot state machine
//!   `Free → Rendering → Ready → Presented → Retiring → Free`;
//! * a slot is handed to the presenter only after the GPU work that renders
//!   into it has **completed** (Metal completion handler, wgpu's
//!   submitted-work-done callback, or a blocking wait);
//! * the slot the presenter shows, and the one it showed before that
//!   (possibly still sampled by an in-flight raster), are never rendered
//!   into; when the ring is full the frame is **dropped, never overwritten**;
//! * a resize rebuilds the ring under a generation counter so completions
//!   of the old ring cannot promote slots of the new one;
//! * the presenter thread never touches the device; it only calls
//!   [`IoSurfaceRing::acquire_presented`], which may wait a bounded time for
//!   the newest in-flight frame (lag 0);
//! * no CPU readback on any per-frame path.
//!
//! Colour: `BGRA8Unorm`, opaque (alpha 1), no colour-space tagging; DPI is
//! the host's business (it sizes the ring in physical pixels).

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_io_surface::{
    kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat, kIOSurfaceWidth,
    IOSurfaceRef,
};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandQueue, MTLDevice, MTLPixelFormat, MTLResource, MTLStorageMode,
    MTLTexture, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};

/// `kCVPixelFormatType_32BGRA` — what Flutter's macOS embedder expects.
const PIXEL_FORMAT_BGRA: i32 = 0x4247_5241; // 'BGRA'
pub const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncMode {
    /// Mark the slot ready from wgpu's submitted-work-done callback.
    Async = 0,
    /// Block on the queue after submit; the slot is ready when `render` returns.
    Blocking = 1,
    /// Commit an empty `MTLCommandBuffer` behind the frame on the same
    /// `MTLCommandQueue` and mark ready from its `addCompletedHandler`, which
    /// Metal fires immediately on its completion thread (no polling). The
    /// production mode (M0 measurement).
    MetalCompletion = 2,
}

impl SyncMode {
    pub fn from_i32(v: i32) -> SyncMode {
        match v {
            1 => SyncMode::Blocking,
            2 => SyncMode::MetalCompletion,
            _ => SyncMode::Async,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SlotState {
    Free,
    /// Submitted; GPU may still be writing.
    Rendering,
    /// GPU work complete; may be handed to the presenter.
    Ready,
    /// The presenter's current frame.
    Presented,
    /// The presenter's previous frame; may still be sampled by an in-flight raster.
    Retiring,
}

struct Slot {
    state: SlotState,
    /// Monotonic render sequence number, to pick the newest Ready slot.
    seq: u64,
    /// Owns the imported texture; `view` borrows from it logically.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct RingShared {
    ring: Mutex<Ring>,
    /// Signalled when a slot becomes Ready.
    ready: Condvar,
}

struct Ring {
    slots: Vec<Slot>,
    next_seq: u64,
    /// Bumped on every resize; completion callbacks from an older generation
    /// must not touch the new slots.
    generation: u64,
}

#[derive(Default, Debug)]
pub struct RingStats {
    pub frames_rendered: u64,
    pub frames_dropped_no_free_buffer: u64,
    pub frames_presented: u64,
    pub presents_without_new_frame: u64,
    pub resizes: u64,
    pub cpu_us: Vec<u32>,
    pub gpu_complete_us: Vec<u32>,
    pub resize_us: Vec<u32>,
    /// Per present: how many frames newer than the presented one had already
    /// been submitted (0 = the presenter got the frame rendered this tick).
    pub present_lag: Vec<u32>,
    /// Per present: time spent waiting for an in-flight frame in `acquire_presented`.
    pub acquire_wait_us: Vec<u32>,
    /// Per rendered frame (M3-16): the world's extract, the renderer's
    /// prepare and encode.
    pub extract_us: Vec<u32>,
    pub prepare_us: Vec<u32>,
    pub encode_us: Vec<u32>,
    /// Per frame read back from the GPU's timestamps: shadow, ambient
    /// occlusion, scene, post (+ present), the frame's span.
    pub gpu_shadow_us: Vec<u32>,
    pub gpu_ao_us: Vec<u32>,
    pub gpu_scene_us: Vec<u32>,
    pub gpu_post_us: Vec<u32>,
    pub gpu_frame_us: Vec<u32>,
}

/// Locks `m`, and still does after a panic poisoned it (foundation 4.3):
/// the ring's state is plain data a panic cannot leave half-written in a
/// way the next frame cannot recover from, and an `unwrap` here would turn
/// one caught panic into a second one on every later call — the presenter's
/// on Flutter's raster thread included.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub type FrameReadyFn = unsafe extern "C" fn(*mut c_void);

struct FrameReadyCallback {
    func: Mutex<Option<(FrameReadyFn, usize)>>,
    /// Incremented for each ready frame; lets a host detect missed signals.
    signalled: AtomicU32,
}

impl FrameReadyCallback {
    fn fire(&self) {
        self.signalled.fetch_add(1, Ordering::Relaxed);
        if let Some((f, ud)) = *lock(&self.func) {
            // SAFETY: the host registered this pointer pair and guarantees its
            // validity until it clears it or destroys the ring.
            unsafe { f(ud as *mut c_void) };
        }
    }
}

pub struct IoSurfaceRing {
    gpu: Arc<Gpu>,
    width: u32,
    height: u32,
    sync_mode: SyncMode,
    ring: Arc<RingShared>,
    /// How long `acquire_presented` may wait for an in-flight frame to
    /// complete (0 = never wait). Waiting happens on the presenter's thread.
    acquire_wait: Mutex<Duration>,
    stats: Arc<Mutex<RingStats>>,
    frame_ready_cb: Arc<FrameReadyCallback>,
    /// Kept alive for the lifetime of the textures; indexed like `ring.slots`.
    surfaces: Vec<CFRetained<IOSurfaceRef>>,
}

/// Where a viewport's GPU comes from (ADR-0030 §4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceChoice {
    /// The one device every viewport in the process shares: made by the
    /// first viewport, freed with the last. The default.
    Shared,
    /// A device of its own, as every viewport had before ABI 10 (the
    /// measurement's baseline, and `FYNKEL_VIEWPORT_DEVICE=own`).
    Own,
}

impl DeviceChoice {
    /// `FYNKEL_VIEWPORT_DEVICE=own` gives every viewport its own device
    /// (a way back if sharing ever misbehaves); anything else shares.
    pub fn from_env() -> DeviceChoice {
        match std::env::var("FYNKEL_VIEWPORT_DEVICE").as_deref() {
            Ok("own") => DeviceChoice::Own,
            _ => DeviceChoice::Shared,
        }
    }
}

/// A wgpu device on Metal with its queue, and the raw Metal objects the
/// ring imports surfaces on and fences with. wgpu's device and queue may be
/// used from any thread; the viewports use them from the host loop.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    mtl_device: Retained<ProtocolObject<dyn MTLDevice>>,
    /// The MTLCommandQueue wgpu submits on; used by `SyncMode::MetalCompletion`.
    mtl_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    lost: Arc<std::sync::atomic::AtomicBool>,
}

// SAFETY: the Metal device and command queue are thread-safe objects
// (Apple documents MTLDevice and MTLCommandQueue as usable from any
// thread); wgpu's device and queue are Send + Sync.
unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

/// The shared device, while any viewport holds it.
static SHARED_GPU: Mutex<std::sync::Weak<Gpu>> = Mutex::new(std::sync::Weak::new());

impl Gpu {
    /// The GPU for a new viewport: the shared one (made now if no viewport
    /// holds it, or if it was lost), or one of its own.
    pub fn get(choice: DeviceChoice) -> Result<Arc<Gpu>, String> {
        if choice == DeviceChoice::Own {
            return Gpu::create().map(Arc::new);
        }
        let mut shared = lock(&SHARED_GPU);
        if let Some(gpu) = shared.upgrade() {
            if !gpu.is_lost() {
                return Ok(gpu);
            }
        }
        let gpu = Arc::new(Gpu::create()?);
        *shared = Arc::downgrade(&gpu);
        Ok(gpu)
    }

    /// Whether the device was lost (a driver reset, the GPU removed): no
    /// new viewport is given it any more.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The `MTLCommandQueue` wgpu submits on: one per wgpu device, so two
    /// viewports share a device exactly when they share it. (The
    /// `MTLDevice` is the system's one per GPU either way.)
    pub fn mtl_queue_ptr(&self) -> *const c_void {
        Retained::as_ptr(&self.mtl_queue) as *const c_void
    }

    /// Runs in its own autorelease pool: Metal autoreleases on the calling
    /// thread, which need not have a pool.
    fn create() -> Result<Gpu, String> {
        objc2::rc::autoreleasepool(|_| Gpu::create_in_pool())
    }

    fn create_in_pool() -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            fynkel_render::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .map_err(|e| format!("no adapter: {e}"))?;
        let info = adapter.get_info();
        log::info!(
            "fynkel apple viewport adapter: {} ({:?})",
            info.name,
            info.backend
        );

        let (device, queue) =
            fynkel_render::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("fynkel apple viewport device"),
                required_features: fynkel_render::device_features(&adapter),
                required_limits: fynkel_render::device_limits(&adapter),
                ..Default::default()
            }))
            .map_err(|e| format!("request_device: {e}"))?;
        let lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            log::error!("fynkel apple viewport device lost ({reason:?}): {message}");
            flag.store(true, Ordering::Release);
        });

        // The MTLDevice wgpu owns. Every imported texture must come from it.
        let mtl_device: Retained<ProtocolObject<dyn MTLDevice>> = unsafe {
            let hal = device
                .as_hal::<wgpu_hal::api::Metal>()
                .ok_or("device is not Metal")?;
            hal.raw_device().clone()
        };
        let mtl_queue: Retained<ProtocolObject<dyn MTLCommandQueue>> = unsafe {
            let hal = queue
                .as_hal::<wgpu_hal::api::Metal>()
                .ok_or("queue is not Metal")?;
            Retained::from(hal.as_raw())
        };
        Ok(Gpu {
            device,
            queue,
            mtl_device,
            mtl_queue,
            lost,
        })
    }
}

impl IoSurfaceRing {
    /// A ring of `buffer_count` surfaces on the shared GPU (or on its own,
    /// by `FYNKEL_VIEWPORT_DEVICE`).
    pub fn new(
        width: u32,
        height: u32,
        buffer_count: u32,
        sync_mode: SyncMode,
    ) -> Result<Self, String> {
        let gpu = Gpu::get(DeviceChoice::from_env())?;
        Self::with_gpu(gpu, width, height, buffer_count, sync_mode)
    }

    /// A ring of `buffer_count` surfaces on `gpu`.
    ///
    /// Runs in its own autorelease pool, like [`Self::resize`] and the
    /// completion path of [`Self::render_with`]: Metal and IOSurface
    /// autorelease on the calling thread, which need not have a pool.
    pub fn with_gpu(
        gpu: Arc<Gpu>,
        width: u32,
        height: u32,
        buffer_count: u32,
        sync_mode: SyncMode,
    ) -> Result<Self, String> {
        objc2::rc::autoreleasepool(|_| {
            let (slots, surfaces) =
                build_ring(&gpu.device, &gpu.mtl_device, width, height, buffer_count)?;
            Ok(Self {
                gpu,
                width,
                height,
                sync_mode,
                ring: Arc::new(RingShared {
                    ring: Mutex::new(Ring {
                        slots,
                        next_seq: 1,
                        generation: 0,
                    }),
                    ready: Condvar::new(),
                }),
                acquire_wait: Mutex::new(Duration::ZERO),
                stats: Arc::new(Mutex::new(RingStats::default())),
                frame_ready_cb: Arc::new(FrameReadyCallback {
                    func: Mutex::new(None),
                    signalled: AtomicU32::new(0),
                }),
                surfaces,
            })
        })
    }

    /// The GPU this ring draws with (shared with the process's other
    /// viewports unless made with its own).
    pub fn gpu(&self) -> &Arc<Gpu> {
        &self.gpu
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.gpu.queue
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn buffer_count(&self) -> u32 {
        self.surfaces.len() as u32
    }

    pub fn sync_mode(&self) -> SyncMode {
        self.sync_mode
    }

    /// Rebuild the ring at a new size. Must be called from the same thread as
    /// `render` (the host loop). Blocks until in-flight GPU work is done, so
    /// no old surface is being written when it is released. Old IOSurfaces
    /// stay alive as long as the presenter still holds them (CF refcount);
    /// the host must re-wrap the new surfaces (`iosurface_ptr`) before it
    /// renders again. After this call nothing is Presented, so
    /// `acquire_presented` returns -1 until a new frame is ready.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if width == self.width && height == self.height {
            return Ok(());
        }
        objc2::rc::autoreleasepool(|_| self.resize_in_pool(width, height))
    }

    fn resize_in_pool(&mut self, width: u32, height: u32) -> Result<(), String> {
        let t0 = Instant::now();
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let count = self.surfaces.len() as u32;
        let (slots, surfaces) =
            build_ring(&self.gpu.device, &self.gpu.mtl_device, width, height, count)?;
        {
            let mut ring = lock(&self.ring.ring);
            ring.slots = slots;
            ring.generation += 1;
        }
        self.surfaces = surfaces;
        self.width = width;
        self.height = height;
        let mut st = lock(&self.stats);
        st.resizes += 1;
        st.resize_us.push(t0.elapsed().as_micros() as u32);
        Ok(())
    }

    pub fn iosurface_ptr(&self, index: u32) -> *const c_void {
        match self.surfaces.get(index as usize) {
            Some(s) => CFRetained::as_ptr(s).as_ptr() as *const c_void,
            None => std::ptr::null(),
        }
    }

    /// Render one frame into a free slot: `record` receives the slot's view
    /// and returns the command buffer to submit. Returns the slot index, or
    /// -1 if every slot is in use (the frame is dropped, `record` not called).
    pub fn render_with(
        &self,
        record: impl FnOnce(&wgpu::Device, &wgpu::Queue, &wgpu::TextureView) -> wgpu::CommandBuffer,
    ) -> i32 {
        let t0 = Instant::now();
        let (index, seq, generation) = {
            let mut ring = lock(&self.ring.ring);
            let Some(i) = ring.slots.iter().position(|s| s.state == SlotState::Free) else {
                lock(&self.stats).frames_dropped_no_free_buffer += 1;
                return -1;
            };
            let seq = ring.next_seq;
            ring.next_seq += 1;
            ring.slots[i].state = SlotState::Rendering;
            ring.slots[i].seq = seq;
            (i, seq, ring.generation)
        };

        let commands = {
            let ring = lock(&self.ring.ring);
            record(&self.gpu.device, &self.gpu.queue, &ring.slots[index].view)
        };
        self.gpu.queue.submit(Some(commands));

        let shared = self.ring.clone();
        let stats = self.stats.clone();
        let cb = self.frame_ready_cb.clone();
        let submitted = Instant::now();
        let mark_ready = move || {
            {
                let mut ring = lock(&shared.ring);
                // A resize may have replaced the ring; only promote our own frame.
                if ring.generation == generation {
                    let slot = &mut ring.slots[index];
                    if slot.state == SlotState::Rendering && slot.seq == seq {
                        slot.state = SlotState::Ready;
                    }
                }
                shared.ready.notify_all();
            }
            lock(&stats)
                .gpu_complete_us
                .push(submitted.elapsed().as_micros() as u32);
            cb.fire();
        };

        match self.sync_mode {
            SyncMode::Async => self.gpu.queue.on_submitted_work_done(mark_ready),
            SyncMode::Blocking => {
                let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
                mark_ready();
            }
            SyncMode::MetalCompletion => objc2::rc::autoreleasepool(|_| {
                // Command buffers on one MTLCommandQueue complete in commit
                // order, so this empty buffer completes after the frame. The
                // pool drains what Metal autoreleases here (the caller's
                // thread may have none: Rust hosts, tests), else every frame
                // retains a command buffer (M1 leak test).
                let fence = self
                    .gpu
                    .mtl_queue
                    .commandBuffer()
                    .expect("MTLCommandQueue.commandBuffer");
                let block = block2::RcBlock::new(
                    move |_cb: std::ptr::NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                        // Metal's thread: a panic here would abort the process.
                        let ready =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mark_ready));
                        if ready.is_err() {
                            log::error!("fynkel_platform_apple: panic in a frame's completion");
                        }
                    },
                );
                // SAFETY: the block is retained by Metal until it has run.
                unsafe { fence.addCompletedHandler(block2::RcBlock::as_ptr(&block)) };
                fence.commit();
            }),
        }

        let mut st = lock(&self.stats);
        st.frames_rendered += 1;
        st.cpu_us.push(t0.elapsed().as_micros() as u32);
        index as i32
    }

    /// Bounded wait `acquire_presented` may spend on the newest in-flight frame.
    pub fn set_acquire_wait(&self, wait: Duration) {
        *lock(&self.acquire_wait) = wait;
    }

    /// Called by the presenter (Flutter's raster thread) when it wants a frame.
    /// Returns the newest GPU-complete slot and rotates Presented/Retiring.
    /// If nothing new is ready, returns the currently presented slot again
    /// (or -1 if nothing has ever been presented). Safe from any thread.
    pub fn acquire_presented(&self) -> i32 {
        let mut ring = lock(&self.ring.ring);
        let mut waited = Duration::ZERO;
        let max_wait = *lock(&self.acquire_wait);
        if max_wait > Duration::ZERO {
            // If the newest submitted frame is still on the GPU, give it a
            // bounded chance to finish so this present shows it (lag 0).
            let t0 = Instant::now();
            loop {
                let newest_seq = ring.next_seq - 1;
                let newest_in_flight = ring
                    .slots
                    .iter()
                    .any(|s| s.state == SlotState::Rendering && s.seq == newest_seq);
                let remaining = max_wait.saturating_sub(t0.elapsed());
                if !newest_in_flight || remaining.is_zero() {
                    break;
                }
                ring = self
                    .ring
                    .ready
                    .wait_timeout(ring, remaining)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            waited = t0.elapsed();
        }
        let newest_ready = ring
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.state == SlotState::Ready)
            .max_by_key(|(_, s)| s.seq)
            .map(|(i, _)| i);

        let mut stats = lock(&self.stats);
        match newest_ready {
            Some(new) => {
                for (i, s) in ring.slots.iter_mut().enumerate() {
                    s.state = match s.state {
                        SlotState::Retiring => SlotState::Free,
                        SlotState::Presented => SlotState::Retiring,
                        // Older ready frames were never shown; recycle them.
                        SlotState::Ready if i != new => SlotState::Free,
                        other => other,
                    };
                }
                ring.slots[new].state = SlotState::Presented;
                stats.frames_presented += 1;
                let lag = (ring.next_seq - 1).saturating_sub(ring.slots[new].seq);
                stats.present_lag.push(lag as u32);
                stats.acquire_wait_us.push(waited.as_micros() as u32);
                new as i32
            }
            None => {
                stats.presents_without_new_frame += 1;
                ring.slots
                    .iter()
                    .position(|s| s.state == SlotState::Presented)
                    .map(|i| i as i32)
                    .unwrap_or(-1)
            }
        }
    }

    /// Registers (or clears, with `None`) the host callback fired when a
    /// frame becomes Ready. Takes the callback lock, so it returns only after
    /// any in-progress callback has finished.
    pub fn set_frame_ready_callback(&self, func: Option<FrameReadyFn>, userdata: *mut c_void) {
        *lock(&self.frame_ready_cb.func) = func.map(|f| (f, userdata as usize));
    }

    pub fn frame_ready_signals(&self) -> u32 {
        self.frame_ready_cb.signalled.load(Ordering::Relaxed)
    }

    pub fn take_stats(&self) -> RingStats {
        std::mem::take(&mut *lock(&self.stats))
    }

    /// Records a rendered frame's stage times (M3-16); `gpu` is the pass
    /// times of a frame whose timestamps came back on this call, if any.
    pub fn record_stage_times(
        &self,
        extract_us: u32,
        prepare_us: u32,
        encode_us: u32,
        gpu: Option<(u32, u32, u32, u32, u32)>,
    ) {
        let mut st = lock(&self.stats);
        st.extract_us.push(extract_us);
        st.prepare_us.push(prepare_us);
        st.encode_us.push(encode_us);
        if let Some((shadow, ao, scene, post, frame)) = gpu {
            st.gpu_shadow_us.push(shadow);
            st.gpu_ao_us.push(ao);
            st.gpu_scene_us.push(scene);
            st.gpu_post_us.push(post);
            st.gpu_frame_us.push(frame);
        }
    }

    /// Blocks until all submitted GPU work has completed (tests, teardown).
    pub fn wait_idle(&self) {
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Test/diagnostic helper: read one BGRA pixel back from a surface on
    /// the CPU. This is exactly the readback the production path never does.
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

    /// The whole frame in slot `index` as tightly packed RGBA8 rows, top
    /// row first — the editor's "Save Frame" and its pixel gate against
    /// `fynkel render` (viewport ABI v4). Waits for every submitted frame
    /// to complete first, so the slot's picture is the one its render
    /// drew; never on a per-frame path.
    pub fn read_frame(&self, index: u32) -> Option<Vec<u8>> {
        use objc2_io_surface::IOSurfaceLockOptions;
        let s = self.surfaces.get(index as usize)?;
        let (width, height) = self.size();
        self.wait_idle();
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        unsafe {
            if s.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) != 0 {
                return None;
            }
            let base = s.base_address().as_ptr() as *const u8;
            let stride = s.bytes_per_row();
            for y in 0..height as usize {
                let row = std::slice::from_raw_parts(base.add(y * stride), width as usize * 4);
                for px in row.chunks_exact(4) {
                    // The ring's surfaces are BGRA (`TEXTURE_FORMAT`).
                    out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            }
            s.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        }
        Some(out)
    }
}

type RingParts = (Vec<Slot>, Vec<CFRetained<IOSurfaceRef>>);

fn build_ring(
    device: &wgpu::Device,
    mtl_device: &ProtocolObject<dyn MTLDevice>,
    width: u32,
    height: u32,
    buffer_count: u32,
) -> Result<RingParts, String> {
    let mut slots = Vec::with_capacity(buffer_count as usize);
    let mut surfaces = Vec::with_capacity(buffer_count as usize);
    for i in 0..buffer_count {
        let surface = create_iosurface(width, height)?;
        let texture = import_iosurface(device, mtl_device, &surface, width, height, i)?;
        let view = texture.create_view(&Default::default());
        slots.push(Slot {
            state: SlotState::Free,
            seq: 0,
            _texture: texture,
            view,
        });
        surfaces.push(surface);
    }
    Ok((slots, surfaces))
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
        .ok_or_else(|| format!("newTextureWithDescriptor:iosurface: failed for slot {index}"))?;

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
                label: Some("fynkel iosurface"),
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

pub(crate) fn pct(v: &mut [u32], q: f64) -> u32 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[((v.len() as f64 - 1.0) * q).round() as usize]
}
