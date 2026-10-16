//! The Android target: frames presented straight to the `Surface` of a
//! Flutter `SurfaceProducer`, through a wgpu (Vulkan) surface. The OS
//! queues the frames, so there is no ring here (docs/stage-1.md §3.5).
//!
//! The surface comes and goes with the platform thread: every resize makes
//! a new one (the producer makes a new `ImageReader`), and it is taken away
//! while the app is in the background. [`AndroidPresenter`] is the platform
//! side; [`Android`] is the render worker's.
//!
//! A surface can also die while the engine still holds it: its
//! `ImageReader` closed by Flutter without telling the plugin (seen once on
//! a Galaxy Note10+, docs/stage-1.md §3.5). Configuring it again is then a
//! wgpu validation error, which would panic the render worker; instead the
//! surface counts as dead and its frames are skipped until the next one.
//!
//! Frames are presented on a thread of their own, the presenter, which
//! waits for each frame's GPU work first (see [`Android::finish`]). The
//! render thread meanwhile draws the next frame, so the GPU always has one
//! queued: drawn one after another on the render thread, the CPU's part
//! and the GPU's took turns, and the Note10+'s GPU, never busy enough,
//! stayed at its lowest clock (docs/android-pacing.md).
//!
//! The presenter joins the JVM once, for its whole life. The surface's
//! `ImageReader` lives in this process, and its frame-available callback
//! runs on the thread that queued the frame and calls into Java: on a
//! thread the JVM doesn't know, it would attach the thread for that one
//! call, with a new `java.lang.Thread` each time, every frame.

use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;

use jni_sys::{JNI_OK, JNI_VERSION_1_6, JNIEnv, JavaVM, JavaVMAttachArgs};
use wgpu::rwh;

use crate::handoff::Handoff;
use crate::target::Target;

/// What frames are drawn in. The surface may hold plain RGBA8 instead and
/// is then drawn through an sRGB view of itself (§3.6).
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(env: *mut c_void, surface: *mut c_void) -> *mut c_void;
    fn ANativeWindow_acquire(window: *mut c_void);
    fn ANativeWindow_release(window: *mut c_void);
}

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

const ANDROID_LOG_WARN: c_int = 5;

/// Why a surface couldn't be used.
#[derive(Debug)]
pub struct AndroidError(pub String);

impl std::fmt::Display for AndroidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AndroidError {}

/// An `ANativeWindow`, holding one reference to it.
struct Window(NonNull<c_void>);

// SAFETY: ANativeWindow is reference counted and safe to use from any
// thread.
unsafe impl Send for Window {}
// SAFETY: as above.
unsafe impl Sync for Window {}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: we hold one reference, from ANativeWindow_fromSurface.
        unsafe { ANativeWindow_release(self.0.as_ptr()) };
    }
}

/// A surface the worker can draw to. The fields drop in order: the wgpu
/// surface before the window under it.
struct Live {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// It couldn't be configured again: frames to it are skipped.
    dead: AtomicBool,
    _window: Window,
}

/// The JVM: one per process, and it outlives everything here.
#[derive(Clone, Copy)]
struct Jvm(NonNull<JavaVM>);

// SAFETY: a JavaVM pointer is valid on any thread.
unsafe impl Send for Jvm {}
// SAFETY: as above.
unsafe impl Sync for Jvm {}

struct Shared {
    /// Known from the first [`AndroidPresenter::attach`].
    jvm: OnceLock<Jvm>,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    surface: Arc<Handoff<Live>>,
    /// Frames given to the surface.
    presented: AtomicU64,
    /// Which of the target's slots are free.
    free: Free,
}

/// The render worker's side: draws into whatever surface the platform
/// side gave it last, and skips frames while there is none.
pub struct Android {
    queue: wgpu::Queue,
    shared: Arc<Shared>,
    /// The size of the frame being drawn: the surface's, when it began.
    size: (u32, u32),
    /// The images frames are drawn into, taken in turn ([`SLOTS`]).
    slots: Vec<Option<Slot>>,
    /// Frames to present, and the thread presenting them. Taken when the
    /// target is dropped, which ends the thread.
    presenter: Option<(SyncSender<Submitted>, JoinHandle<()>)>,
}

/// How many images frames are drawn into: one being drawn while the
/// presenter copies the one before to the surface.
const SLOTS: usize = 2;

/// An image a frame is drawn into, before the presenter copies it to the
/// surface.
struct Slot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Which slots are free, shared with the presenter, which frees each once
/// its copy to the surface is submitted (later GPU work, drawing into it
/// again, runs after the copy).
#[derive(Default)]
struct Free {
    state: Mutex<FreeState>,
    changed: Condvar,
}

#[derive(Default)]
struct FreeState {
    taken: [bool; SLOTS],
}

impl Free {
    fn lock(&self) -> std::sync::MutexGuard<'_, FreeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn release(&self, slot: usize) {
        self.lock().taken[slot] = false;
        self.changed.notify_all();
    }
}

/// A frame drawn and submitted, for the presenter.
struct Submitted {
    slot: usize,
    texture: wgpu::Texture,
}

/// The platform side: gives the worker its surface and takes it away. Any
/// thread may call it; each call waits for a frame in progress.
#[derive(Clone)]
pub struct AndroidPresenter {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for AndroidPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidPresenter").field("presented", &self.shared.presented).finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Android {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Android").field("size", &self.size).finish_non_exhaustive()
    }
}

impl Android {
    /// A target with no surface yet, and its presenter. `instance` made
    /// `adapter`, which made `device` and `queue`.
    pub fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> (Self, AndroidPresenter) {
        let shared = Arc::new(Shared {
            jvm: OnceLock::new(),
            instance: instance.clone(),
            adapter: adapter.clone(),
            device: device.clone(),
            surface: Arc::new(Handoff::new()),
            presented: AtomicU64::new(0),
            free: Free::default(),
        });
        // One frame waits for the presenter while it presents another.
        let (frames, submitted) = sync_channel::<Submitted>(SLOTS - 1);
        let presenter = std::thread::Builder::new()
            .name("fynkel present".into())
            .spawn({
                let (shared, queue) = (shared.clone(), queue.clone());
                move || present(&shared, &queue, &submitted)
            })
            .ok()
            .map(|thread| (frames, thread));
        if presenter.is_none() {
            warn("couldn't start the presenter thread: frames are presented on the render thread");
        }
        let target = Self {
            queue: queue.clone(),
            shared: shared.clone(),
            size: (width.max(1), height.max(1)),
            slots: (0..SLOTS).map(|_| None).collect(),
            presenter,
        };
        (target, AndroidPresenter { shared })
    }
}

/// The presenter thread: copies each frame to the surface and presents it
/// once the GPU has finished it.
fn present(shared: &Shared, queue: &wgpu::Queue, frames: &std::sync::mpsc::Receiver<Submitted>) {
    let mut joined = None;
    let mut tried = false;
    for frame in frames {
        // The JVM is known once there has been a surface.
        if !tried && let Some(&jvm) = shared.jvm.get() {
            tried = true;
            joined = join_jvm(jvm);
        }
        present_one(shared, queue, frame);
    }
    if let Some(jvm) = joined {
        let vm = jvm.0.as_ptr();
        // SAFETY: this thread attached itself in join_jvm, and a thread must
        // leave the JVM before it ends.
        unsafe {
            if let Some(detach) = (**vm).DetachCurrentThread {
                detach(vm);
            }
        }
    }
}

/// Copies `frame` to the surface's next image, then waits for the GPU and
/// presents it.
///
/// Flutter takes each frame on the main thread, where Dart runs, and waits
/// there for the GPU to finish it (Android 12's ImageReader in acquiring
/// it, Flutter itself from 13). Waiting here first keeps the game's next
/// update from waiting behind the GPU: on the Note10+, 10,000 objects went
/// from 12.7 to 27–36 fps (docs/stage-6-scale.md). A GPU that takes over a
/// second is presented all the same, and that wait falls on Flutter as
/// before. If the platform thread already waits for the surface (a resize,
/// the app going to the background), the frame is presented at once and
/// the wait falls on Flutter.
///
/// One blocking wait: a short timeout lost the Note10+'s device (Mali's
/// driver answers a timed-out wait with an error wgpu takes as a lost
/// device), and polling every 0.5 ms instead cost the shadows scene 9 fps
/// (56.9 → 47.9 at medium).
fn present_one(shared: &Shared, queue: &wgpu::Queue, frame: Submitted) {
    let Submitted { slot, texture } = frame;
    let Some(live) = shared.surface.lease() else {
        shared.free.release(slot);
        return;
    };
    if live.dead.load(Ordering::Relaxed) {
        shared.free.release(slot);
        return;
    }
    let image = match live.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(image) | wgpu::CurrentSurfaceTexture::Suboptimal(image) => image,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            // The window changed under the swapchain, or went away: make the
            // swapchain again and present the next frame. A surface whose
            // window is gone can't be, and stays dead until the platform side
            // gives a new one.
            if let Err(error) = configure(&shared.device, &live.surface, &live.config) {
                live.dead.store(true, Ordering::Relaxed);
                warn(&format!("the surface is gone ({error}); frames are skipped until a new one comes"));
            }
            shared.free.release(slot);
            return;
        }
        // Busy or hidden: skip this frame.
        _ => {
            shared.free.release(slot);
            return;
        }
    };
    // Drawn at the surface's size when it began; a surface of another size
    // came since, and the next frame is drawn at it.
    if texture.size() != image.texture.size() {
        shared.free.release(slot);
        return;
    }
    let mut encoder =
        shared.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("to the surface") });
    encoder.copy_texture_to_texture(texture.as_image_copy(), image.texture.as_image_copy(), texture.size());
    let index = queue.submit([encoder.finish()]);
    shared.free.release(slot);
    if !live.wanted() {
        let _ = shared.device.poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: Some(std::time::Duration::from_secs(1)),
        });
    }
    queue.present(image);
    shared.presented.fetch_add(1, Ordering::Relaxed);
    // The lease ends last, after the present.
    drop(live);
}

/// One frame: the slot it's drawn into.
pub struct Frame {
    slot: usize,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame").field("slot", &self.slot).finish()
    }
}

/// Attaches this (the presenter) thread to the JVM, as a daemon that keeps
/// the thread's name. Failing only costs speed, so it is just logged.
fn join_jvm(jvm: Jvm) -> Option<Jvm> {
    let mut env: *mut c_void = ptr::null_mut();
    let mut args = JavaVMAttachArgs {
        version: JNI_VERSION_1_6,
        name: c"fynkel present".as_ptr().cast_mut(),
        group: ptr::null_mut(),
    };
    let vm = jvm.0.as_ptr();
    // SAFETY: a live JVM; every JVM has this function.
    let status = unsafe {
        match (**vm).AttachCurrentThreadAsDaemon {
            Some(attach) => attach(vm, &mut env, (&raw mut args).cast()),
            None => -1,
        }
    };
    if status == JNI_OK {
        Some(jvm)
    } else {
        warn(&format!("the presenter couldn't join the JVM ({status}): each frame will attach it again"));
        None
    }
}

impl Drop for Android {
    /// Presents the frames in flight, and ends the presenter.
    fn drop(&mut self) {
        if let Some((frames, thread)) = self.presenter.take() {
            drop(frames);
            let _ = thread.join();
        }
    }
}

impl Target for Android {
    type Frame = Frame;

    fn format(&self) -> wgpu::TextureFormat {
        FORMAT
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    /// A free slot at the surface's size, or `None` with no surface. Waits
    /// for the presenter to free a slot, up to a second.
    fn begin(&mut self) -> Option<Frame> {
        let size = {
            let live = self.shared.surface.lease()?;
            if live.dead.load(Ordering::Relaxed) {
                return None;
            }
            (live.config.width, live.config.height)
        };
        let free = &self.shared.free;
        let slot = {
            let (mut state, _) = free
                .changed
                .wait_timeout_while(free.lock(), std::time::Duration::from_secs(1), |s| s.taken.iter().all(|&t| t))
                .unwrap_or_else(PoisonError::into_inner);
            let slot = state.taken.iter().position(|&t| !t)?;
            state.taken[slot] = true;
            slot
        };
        let fits = self.slots[slot].as_ref().is_some_and(|s| {
            let made = s.texture.size();
            (made.width, made.height) == size
        });
        if !fits {
            let texture = self.shared.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frame"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.slots[slot] = Some(Slot { texture, view });
        }
        self.size = size;
        Some(Frame { slot })
    }

    fn view<'a>(&'a self, frame: &'a Frame) -> &'a wgpu::TextureView {
        &self.slots[frame.slot].as_ref().expect("begin made it").view
    }

    /// Submits the frame and hands it to the presenter ([`present_one`]).
    ///
    /// The presenter may still be waiting for the frame before, so the GPU
    /// has this one queued meanwhile. Submitting only after the one before
    /// is presented, so that nothing is queued in front of the fence
    /// Flutter waits on, took Sheepdog at low on the Note10+ from 51 back to
    /// 36 fps: what made the GPU idle was the time between frames on the
    /// CPU's side (docs/android-pacing.md).
    fn finish(&mut self, frame: Frame, encoder: wgpu::CommandEncoder, _number: u64) {
        let commands = encoder.finish();
        self.queue.submit([commands]);
        let texture = self.slots[frame.slot].as_ref().expect("begin made it").texture.clone();
        let submitted = Submitted { slot: frame.slot, texture };
        let submitted = match &self.presenter {
            Some((frames, _)) => match frames.send(submitted) {
                Ok(()) => return,
                Err(std::sync::mpsc::SendError(submitted)) => submitted,
            },
            None => submitted,
        };
        // No presenter: as it would, here.
        present_one(&self.shared, &self.queue, submitted);
    }

    /// The size comes with each surface ([`AndroidPresenter::attach`]), so
    /// there is nothing to do here.
    fn resize(&mut self, _width: u32, _height: u32) -> Result<(), String> {
        Ok(())
    }
}

impl AndroidPresenter {
    /// Draws the next frames into `surface` (an `android.view.Surface`) at
    /// `width` × `height`, the size its producer was given. Waits for a
    /// frame in progress on the old surface, which is then let go.
    ///
    /// # Safety
    ///
    /// `env` is the calling thread's `JNIEnv`, and `surface` a live local or
    /// global reference to an `android.view.Surface`.
    pub unsafe fn attach(
        &self,
        env: *mut c_void,
        surface: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<(), AndroidError> {
        // SAFETY: per this function's contract. The window holds its own
        // reference from here on.
        let window = NonNull::new(unsafe { ANativeWindow_fromSurface(env, surface) })
            .ok_or_else(|| AndroidError("ANativeWindow_fromSurface gave no window".into()))?;
        let window = Window(window);
        let shared = &self.shared;
        if shared.jvm.get().is_none() {
            let env = env.cast::<JNIEnv>();
            let mut vm: *mut JavaVM = ptr::null_mut();
            // SAFETY: `env` is this thread's, per this function's contract.
            let status = unsafe { (**env).GetJavaVM.map_or(-1, |get| get(env, &mut vm)) };
            if let (JNI_OK, Some(vm)) = (status, NonNull::new(vm)) {
                let _ = shared.jvm.set(Jvm(vm));
            }
        }
        self.attach_owned(window, width, height)
    }

    /// [`AndroidPresenter::attach`] for a caller that has the
    /// `ANativeWindow` already. The presenter takes a reference of its own.
    /// With no JVM known, the render thread doesn't join it.
    ///
    /// # Safety
    ///
    /// `window` is a live `ANativeWindow`.
    pub unsafe fn attach_window(&self, window: NonNull<c_void>, width: u32, height: u32) -> Result<(), AndroidError> {
        // SAFETY: a live window, per this function's contract.
        unsafe { ANativeWindow_acquire(window.as_ptr()) };
        self.attach_owned(Window(window), width, height)
    }

    fn attach_owned(&self, window: Window, width: u32, height: u32) -> Result<(), AndroidError> {
        let shared = &self.shared;
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(rwh::RawDisplayHandle::Android(rwh::AndroidDisplayHandle::new())),
            raw_window_handle: rwh::RawWindowHandle::AndroidNdk(rwh::AndroidNdkWindowHandle::new(window.0)),
        };
        // SAFETY: the window outlives the surface: `Live` drops the surface
        // first.
        let surface = unsafe { shared.instance.create_surface_unsafe(target) }
            .map_err(|e| AndroidError(format!("couldn't make a wgpu surface: {e}")))?;

        let caps = surface.get_capabilities(&shared.adapter);
        let format = [FORMAT, FORMAT.remove_srgb_suffix()]
            .into_iter()
            .find(|f| caps.formats.contains(f))
            .ok_or_else(|| AndroidError(format!("the surface offers no RGBA8 format, only {:?}", caps.formats)))?;
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)
        };
        if !caps.usages.contains(wgpu::TextureUsages::COPY_DST) {
            return Err(AndroidError("the surface can't be copied to".into()));
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::COPY_DST,
            format,
            width: width.max(1),
            height: height.max(1),
            // Fifo: the OS paces the frames, and every device has it.
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            // sRGB for an RGBA8 format, and valid for every format offered.
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        };
        configure(&shared.device, &surface, &config)
            .map_err(|e| AndroidError(format!("couldn't configure the surface: {e}")))?;

        // The old surface and its window go here, after the frame in
        // progress.
        drop(shared.surface.put(Live { surface, config, dead: AtomicBool::new(false), _window: window }));
        Ok(())
    }

    /// Stops drawing to the surface: waits for a frame in progress, then
    /// lets the surface and its window go. Frames are skipped until the
    /// next [`AndroidPresenter::attach`].
    pub fn detach(&self) {
        drop(self.shared.surface.take());
    }

    /// Frames given to the surface so far.
    pub fn presented(&self) -> u64 {
        self.shared.presented.load(Ordering::Relaxed)
    }
}

/// `surface.configure`, with its validation error returned instead of
/// panicking the thread.
fn configure(
    device: &wgpu::Device,
    surface: &wgpu::Surface<'_>,
    config: &wgpu::SurfaceConfiguration,
) -> Result<(), wgpu::Error> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(device, config);
    // On native backends the scope's result is ready at once.
    let mut pop = std::pin::pin!(scope.pop());
    match pop.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(Some(error)) => Err(error),
        _ => Ok(()),
    }
}

/// A warning in logcat, tagged `fynkel`.
fn warn(message: &str) {
    let Ok(text) = CString::new(message.replace('\0', "")) else { return };
    // SAFETY: two nul-terminated strings.
    unsafe { __android_log_write(ANDROID_LOG_WARN, c"fynkel".as_ptr(), text.as_ptr()) };
}
