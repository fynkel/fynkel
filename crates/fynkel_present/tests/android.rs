//! The Android target with real surfaces: an NDK `AImageReader` stands in
//! for Flutter's `SurfaceProducer`, which is an `ImageReader` too. Runs on
//! a device or the emulator (tools/test-mobile.sh).
#![cfg(target_os = "android")]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use fynkel_present::{Android, AndroidPresenter, Target, wgpu};

const AIMAGE_FORMAT_RGBA_8888: i32 = 1;
/// What Flutter's producer asks for (FlutterRenderer.java), with 7 images.
const AIMAGE_FORMAT_PRIVATE: i32 = 0x22;
const AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE: u64 = 1 << 8;

#[link(name = "mediandk")]
unsafe extern "C" {
    fn AImageReader_newWithUsage(
        width: i32,
        height: i32,
        format: i32,
        usage: u64,
        max_images: i32,
        reader: *mut *mut c_void,
    ) -> i32;
    fn AImageReader_getWindow(reader: *mut c_void, window: *mut *mut c_void) -> i32;
    fn AImageReader_acquireLatestImage(reader: *mut c_void, image: *mut *mut c_void) -> i32;
    fn AImageReader_delete(reader: *mut c_void);
    fn AImage_delete(image: *mut c_void);
}

/// What Flutter's producer is: an image reader, whose window is the
/// surface. Dropping it closes the reader, which abandons the surface.
struct Reader(NonNull<c_void>);

impl Reader {
    fn new(width: i32, height: i32) -> Self {
        Self::with(width, height, AIMAGE_FORMAT_RGBA_8888, 4)
    }

    /// A reader made the way Flutter's producer makes one.
    fn like_flutter(width: i32, height: i32) -> Self {
        Self::with(width, height, AIMAGE_FORMAT_PRIVATE, 7)
    }

    fn with(width: i32, height: i32, format: i32, max_images: i32) -> Self {
        let mut reader = ptr::null_mut();
        // SAFETY: plain values and an out pointer.
        let status = unsafe {
            AImageReader_newWithUsage(
                width,
                height,
                format,
                AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE,
                max_images,
                &mut reader,
            )
        };
        assert_eq!(status, 0, "AImageReader_newWithUsage");
        Self(NonNull::new(reader).expect("a reader"))
    }

    /// The reader's window, which it owns.
    fn window(&self) -> NonNull<c_void> {
        let mut window = ptr::null_mut();
        // SAFETY: a live reader.
        assert_eq!(unsafe { AImageReader_getWindow(self.0.as_ptr(), &mut window) }, 0);
        NonNull::new(window).expect("a window")
    }

    /// Takes the newest frame and lets it go, as Flutter does, so the
    /// producer never runs out of buffers. Whether there was one.
    fn consume(&self) -> bool {
        let mut image = ptr::null_mut();
        // SAFETY: a live reader and an out pointer.
        let status = unsafe { AImageReader_acquireLatestImage(self.0.as_ptr(), &mut image) };
        if status == 0 && !image.is_null() {
            // SAFETY: an image this reader gave out.
            unsafe { AImage_delete(image) };
            return true;
        }
        false
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: the reader is ours; its window and images go with it.
        unsafe { AImageReader_delete(self.0.as_ptr()) };
    }
}

fn target() -> (Android, AndroidPresenter, wgpu::Device) {
    // The emulator's driver crashes on object names (fynkel_render's
    // instance_flags).
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::from_build_config() | wgpu::InstanceFlags::DISCARD_HAL_LABELS,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).expect("these tests need a GPU");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
        ..Default::default()
    }))
    .expect("a device");
    let (target, presenter) = Android::new(&instance, &adapter, &device, &queue, 64, 64);
    (target, presenter, device)
}

/// One frame the way the render worker draws it, cleared. Whether there
/// was somewhere to draw.
fn draw(device: &wgpu::Device, target: &mut Android) -> bool {
    let Some(frame) = target.begin() else { return false };
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.view(&frame),
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLUE), store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    });
    target.finish(frame, encoder, 0);
    true
}

#[test]
fn frames_reach_the_surface_and_stop_when_it_is_taken_away() {
    let (mut target, presenter, device) = target();
    assert!(!draw(&device, &mut target), "no surface yet");

    let reader = Reader::new(64, 32);
    // SAFETY: the reader's window is live until the reader goes.
    unsafe { presenter.attach_window(reader.window(), 64, 32) }.expect("attach");
    let mut consumed = 0;
    for _ in 0..10 {
        assert!(draw(&device, &mut target));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        consumed += usize::from(reader.consume());
    }
    assert_eq!(target.size(), (64, 32));
    // The presenter thread presents each frame after the GPU is done with
    // it, a frame or two behind the drawing.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while presenter.presented() < 10 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        consumed += usize::from(reader.consume());
    }
    assert_eq!(presenter.presented(), 10);
    assert!(consumed > 0, "the reader got frames");

    presenter.detach();
    assert!(!draw(&device, &mut target), "taken away");
    assert_eq!(presenter.presented(), 10);
}

// What a Galaxy Note10+ did once (docs/stage-1.md §3.5): Flutter closed
// the reader behind a surface the engine still held. Configuring that
// surface again is a validation error, which used to panic the render
// worker. Now the surface counts as dead until the next one comes.
#[test]
fn a_surface_whose_reader_is_closed_is_skipped_until_a_new_one_comes() {
    let (mut target, presenter, device) = target();
    let reader = Reader::new(64, 64);
    // SAFETY: the window is live until the reader goes, and the presenter
    // holds a reference of its own.
    unsafe { presenter.attach_window(reader.window(), 64, 64) }.expect("attach");
    assert!(draw(&device, &mut target));
    reader.consume();

    drop(reader);
    // The driver may still hand out a texture or two before it notices.
    let drawn = (0..10).filter(|_| draw(&device, &mut target)).count();
    assert!(drawn < 10, "frames still drawn into a closed reader's surface");
    assert!(!draw(&device, &mut target), "the dead surface stays skipped");

    let reader = Reader::new(48, 48);
    // SAFETY: as above.
    unsafe { presenter.attach_window(reader.window(), 48, 48) }.expect("a new surface after a dead one");
    assert!(draw(&device, &mut target), "drawing again");
    assert_eq!(target.size(), (48, 48));
}

/// This process's Graphics row in `dumpsys meminfo`, in MB (the test runs
/// as the shell user, which may ask). Drivers count GPU memory there that
/// PSS doesn't show.
fn graphics_mb() -> f64 {
    let out = std::process::Command::new("dumpsys")
        .args(["meminfo", &std::process::id().to_string()])
        .output()
        .expect("dumpsys");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.trim_start().starts_with("Graphics:")).unwrap_or("Graphics: 0");
    line.split_whitespace().nth(1).and_then(|kb| kb.parse::<f64>().ok()).unwrap_or(0.0) / 1024.0
}

/// Probe, run by hand on a new device (`--ignored --nocapture probe`): the
/// Graphics memory after each of 20 full-screen surfaces, attached, drawn
/// into, taken away and closed, as a trip to the background does. What the
/// engine keeps of a surface grows here; what Flutter keeps doesn't.
#[test]
#[ignore = "a measurement, run by hand"]
fn probe_graphics_memory_over_many_surfaces() {
    let (mut target, presenter, device) = target();
    let mut line = String::from("Graphics MB with each surface / after it:");
    let start = graphics_mb();
    for _ in 0..20 {
        let reader = Reader::like_flutter(1080, 2241);
        // SAFETY: the window is live until the reader goes.
        unsafe { presenter.attach_window(reader.window(), 1080, 2241) }.expect("attach");
        for _ in 0..10 {
            assert!(draw(&device, &mut target));
            device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
            reader.consume();
        }
        let attached = graphics_mb() - start;
        presenter.detach();
        drop(reader);
        line += &format!(" {attached:+.1}/{:+.1}", graphics_mb() - start);
    }
    println!("{line}");
}
