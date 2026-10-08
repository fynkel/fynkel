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
//! The render thread joins the JVM once, for its whole life. The surface's
//! `ImageReader` lives in this process, and its frame-available callback
//! runs on the thread that queued the frame and calls into Java: on a
//! thread the JVM doesn't know, it would attach the thread for that one
//! call, with a new `java.lang.Thread` each time, every frame.

use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use jni_sys::{JNI_OK, JNI_VERSION_1_6, JNIEnv, JavaVM, JavaVMAttachArgs};
use wgpu::rwh;

use crate::handoff::{Handoff, Lease};
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
}

/// The render worker's side: draws into whatever surface the platform
/// side gave it last, and skips frames while there is none.
pub struct Android {
    queue: wgpu::Queue,
    shared: Arc<Shared>,
    /// The size of the last surface drawn to.
    size: (u32, u32),
    /// The render thread joined the JVM, and leaves it when this target is
    /// dropped (on the same thread: the worker owns it).
    joined: Option<Jvm>,
    /// Joining was tried: it happens once.
    tried: bool,
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
        f.debug_struct("Android")
            .field("size", &self.size)
            .field("joined", &self.joined.is_some())
            .finish_non_exhaustive()
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
        });
        let target = Self {
            queue: queue.clone(),
            shared: shared.clone(),
            size: (width.max(1), height.max(1)),
            joined: None,
            tried: false,
        };
        (target, AndroidPresenter { shared })
    }
}

/// One frame: the surface's next texture, and the lease that keeps the
/// platform side from taking the surface away meanwhile. The texture goes
/// first.
pub struct Frame {
    texture: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
    _lease: Lease<Live>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame").finish_non_exhaustive()
    }
}

impl Android {
    /// Attaches this (the render) thread to the JVM, as a daemon that keeps
    /// the worker's name. Failing only costs speed, so it is just logged.
    fn join_jvm(&mut self) {
        let Some(&jvm) = self.shared.jvm.get() else { return };
        self.tried = true;
        let mut env: *mut c_void = ptr::null_mut();
        let mut args = JavaVMAttachArgs {
            version: JNI_VERSION_1_6,
            name: c"fynkel render".as_ptr().cast_mut(),
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
            self.joined = Some(jvm);
        } else {
            warn(&format!("the render thread couldn't join the JVM ({status}): each frame will attach it again"));
        }
    }
}

impl Drop for Android {
    fn drop(&mut self) {
        if let Some(jvm) = self.joined.take() {
            let vm = jvm.0.as_ptr();
            // SAFETY: this thread attached itself in join_jvm, and a thread
            // must leave the JVM before it ends.
            unsafe {
                if let Some(detach) = (**vm).DetachCurrentThread {
                    detach(vm);
                }
            }
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

    fn begin(&mut self) -> Option<Frame> {
        let live = self.shared.surface.lease()?;
        // The JVM is known once there has been a surface.
        if !self.tried {
            self.join_jvm();
        }
        if live.dead.load(Ordering::Relaxed) {
            return None;
        }
        let shared = &self.shared;
        let texture = match live.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                // The window changed under the swapchain, or went away: make
                // the swapchain again and draw the next frame. A surface
                // whose window is gone can't be, and stays dead until the
                // platform side gives a new one.
                if let Err(error) = configure(&shared.device, &live.surface, &live.config) {
                    live.dead.store(true, Ordering::Relaxed);
                    warn(&format!("the surface is gone ({error}); frames are skipped until a new one comes"));
                }
                return None;
            }
            // Busy or hidden: skip this frame.
            _ => return None,
        };
        let view =
            texture.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(FORMAT), ..Default::default() });
        self.size = (live.config.width, live.config.height);
        Some(Frame { texture, view, _lease: live })
    }

    fn view<'a>(&'a self, frame: &'a Frame) -> &'a wgpu::TextureView {
        &frame.view
    }

    fn finish(&mut self, frame: Frame, encoder: wgpu::CommandEncoder, _number: u64) {
        self.queue.submit([encoder.finish()]);
        let Frame { texture, view, _lease } = frame;
        drop(view);
        self.queue.present(texture);
        self.shared.presented.fetch_add(1, Ordering::Relaxed);
        // The lease ends last, after the present.
        drop(_lease);
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
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            // Fifo: the OS paces the frames, and every device has it.
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            // sRGB for an RGBA8 format, and valid for every format offered.
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: if format == FORMAT { vec![] } else { vec![FORMAT] },
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
