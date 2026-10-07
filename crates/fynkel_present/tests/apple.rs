//! The Apple target, driven the way the render worker drives it, with a
//! plain clear pass standing in for the renderer. The pixels are read from
//! the IOSurfaces themselves, which is what Flutter shows.

#![cfg(target_vendor = "apple")]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fynkel_present::{Apple, ApplePresenter, SLOTS, Target, wgpu};

fn device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).expect("these tests need a GPU");
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
        ..Default::default()
    }))
    .expect("a device")
}

/// Clears one frame to `rgb` (8-bit sRGB) if the ring has a slot for it.
fn draw(device: &wgpu::Device, target: &mut Apple, rgb: [u8; 3]) -> bool {
    let Some(frame) = target.begin() else { return false };
    let mut encoder = device.create_command_encoder(&Default::default());
    let [r, g, b] = rgb.map(|c| srgb_to_linear(f64::from(c) / 255.0));
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.view(&frame),
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    target.finish(frame, encoder, 0);
    true
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Counts frame-ready calls and wakes a waiting test.
struct Signals {
    count: AtomicU32,
    sender: std::sync::Mutex<mpsc::Sender<()>>,
}

unsafe extern "C" fn frame_ready(userdata: *mut c_void) {
    // SAFETY: the tests pass a `Signals` that outlives the callback.
    let signals = unsafe { &*userdata.cast::<Signals>() };
    signals.count.fetch_add(1, Ordering::SeqCst);
    let _ = signals.sender.lock().unwrap().send(());
}

fn listen(presenter: &ApplePresenter) -> (Box<Signals>, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    let signals = Box::new(Signals { count: AtomicU32::new(0), sender: std::sync::Mutex::new(sender) });
    // SAFETY: `frame_ready` is thread-safe and the box outlives the
    // callback: each test clears it before dropping the box.
    unsafe { presenter.set_frame_ready(Some(frame_ready), (&*signals as *const Signals).cast_mut().cast()) };
    (signals, receiver)
}

fn wait(receiver: &mpsc::Receiver<()>) {
    receiver.recv_timeout(Duration::from_secs(5)).expect("a frame-ready signal within 5 s");
}

#[test]
fn a_finished_frame_signals_and_its_surface_holds_the_pixels() {
    let (device, queue) = device();
    // 70 wide: IOSurface pads its rows, so the row stride is tested.
    let (mut target, presenter) = Apple::new(&device, &queue, 70, 3).unwrap();
    assert_eq!(target.format(), wgpu::TextureFormat::Bgra8UnormSrgb);
    assert!(presenter.acquire().is_none(), "nothing before the first frame");
    let (signals, ready) = listen(&presenter);

    assert!(draw(&device, &mut target, [200, 30, 90]));
    wait(&ready);
    let shown = presenter.acquire().expect("the finished frame");
    assert_eq!(shown.size(), (70, 3));
    let rgba = shown.read_rgba();
    assert_eq!(rgba.len(), 70 * 3 * 4);
    assert!(rgba.chunks(4).all(|p| p == [200, 30, 90, 255]), "sRGB encoded once, BGRA swizzled: {:?}", &rgba[..8]);
    assert!(!shown.more);

    // SAFETY: clearing the callback.
    unsafe { presenter.set_frame_ready(None, std::ptr::null_mut()) };
    assert_eq!(signals.count.load(Ordering::SeqCst), 1);
}

#[test]
fn two_finished_frames_are_both_shown_oldest_first() {
    let (device, queue) = device();
    let (mut target, presenter) = Apple::new(&device, &queue, 8, 8).unwrap();
    let (signals, ready) = listen(&presenter);

    assert!(draw(&device, &mut target, [10, 0, 0]));
    assert!(draw(&device, &mut target, [20, 0, 0]));
    wait(&ready);
    wait(&ready);
    let first = presenter.acquire().unwrap();
    assert_eq!(first.read_rgba()[0], 10, "the oldest finished frame first");
    assert!(first.more, "another is waiting");
    assert_eq!(presenter.acquire().unwrap().read_rgba()[0], 20);

    // SAFETY: clearing the callback.
    unsafe { presenter.set_frame_ready(None, std::ptr::null_mut()) };
    drop(signals);
}

#[test]
fn a_resize_makes_new_surfaces_and_old_frames_are_never_shown() {
    let (device, queue) = device();
    let (mut target, presenter) = Apple::new(&device, &queue, 16, 16).unwrap();
    let (signals, ready) = listen(&presenter);

    assert!(draw(&device, &mut target, [50, 50, 50]));
    target.resize(32, 8).unwrap();
    assert_eq!(target.size(), (32, 8));
    // The old frame finishes as stale: no signal, nothing to show.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline && presenter.stats().stale == 0 {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(presenter.stats().stale, 1, "the old frame came back stale");
    assert!(presenter.acquire().is_none(), "a frame of the old size must not be shown");

    assert!(draw(&device, &mut target, [90, 90, 90]));
    wait(&ready);
    let shown = presenter.acquire().unwrap();
    assert_eq!(shown.size(), (32, 8));
    assert_eq!(&shown.read_rgba()[..4], &[90, 90, 90, 255]);

    // SAFETY: clearing the callback.
    unsafe { presenter.set_frame_ready(None, std::ptr::null_mut()) };
    assert_eq!(signals.count.load(Ordering::SeqCst), 1);
}

#[test]
fn every_slot_takes_its_turn_and_the_ring_never_leaks_a_frame() {
    let (device, queue) = device();
    let (mut target, presenter) = Apple::new(&device, &queue, 4, 4).unwrap();
    let (signals, ready) = listen(&presenter);
    let mut surfaces = std::collections::HashSet::new();
    for i in 0..50u8 {
        assert!(draw(&device, &mut target, [i, 0, 0]));
        wait(&ready);
        let shown = presenter.acquire().unwrap();
        assert_eq!(shown.read_rgba()[0], i);
        surfaces.insert(shown.into_raw() as usize);
    }
    // `into_raw` retained each; give them back.
    for &surface in &surfaces {
        // SAFETY: each is an IOSurface retained once by `into_raw`.
        unsafe { objc2_core_foundation_release(surface as *mut c_void) };
    }
    assert_eq!(surfaces.len(), SLOTS, "all {SLOTS} slots were used");
    let stats = presenter.stats();
    assert_eq!((stats.reserved, stats.completed, stats.presented), (50, 50, 50));

    // SAFETY: clearing the callback.
    unsafe { presenter.set_frame_ready(None, std::ptr::null_mut()) };
    drop(signals);
}

unsafe extern "C" {
    #[link_name = "CFRelease"]
    fn objc2_core_foundation_release(object: *mut c_void);
}
