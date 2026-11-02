//! The iOS layer target, on the Mac: a `CAMetalLayer` in no window, which
//! frees its drawables late, as an iPhone's does when the screen is busy.

#![cfg(target_vendor = "apple")]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use fynkel_present::{Layer, LayerPresenter, Target, wgpu};
use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};

fn gpu() -> (wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).expect("these tests need a GPU");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
        ..Default::default()
    }))
    .expect("a device");
    (instance, adapter, device, queue)
}

/// A new `CAMetalLayer`, in no window.
fn metal_layer() -> Retained<AnyObject> {
    let class = AnyClass::get(c"CAMetalLayer").expect("QuartzCore is linked by wgpu");
    // SAFETY: `new` on an NSObject subclass returns a +1 object.
    unsafe { msg_send![class, new] }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct CGSize {
    width: f64,
    height: f64,
}

// SAFETY: CGSize is two doubles, as declared.
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

/// Clears one frame if the layer gives a drawable.
fn draw(device: &wgpu::Device, target: &mut Layer) -> bool {
    let Some(frame) = target.begin() else { return false };
    let mut encoder = device.create_command_encoder(&Default::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.view(&frame),
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::RED), store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    }));
    target.finish(frame, encoder, 0);
    true
}

/// Stage 14's BR5: the render thread waiting up to a second for a drawable
/// held the layer, so the main thread taking it away (the app going to the
/// background) or giving it a new size waited as long.
#[test]
fn the_main_thread_never_waits_for_a_drawable() {
    let (instance, adapter, device, queue) = gpu();
    let layer = metal_layer();
    let (mut target, presenter): (Layer, LayerPresenter) = Layer::new(&instance, &adapter, &device, &queue, 64, 64);
    let pointer = NonNull::new(Retained::as_ptr(&layer) as *mut c_void).unwrap();
    // SAFETY: a live CAMetalLayer, kept alive for the whole test.
    unsafe { presenter.attach(pointer, 64, 64) }.unwrap();
    // The layer's three drawables, kept: the render thread waits for one
    // until Core Animation gives up, a second later.
    let held: Vec<Option<Retained<AnyObject>>> = (0..3)
        // SAFETY: a configured CAMetalLayer; `nextDrawable` returns an
        // autoreleased object, retained here.
        .map(|_| objc2::rc::autoreleasepool(|_| unsafe { msg_send![&*layer, nextDrawable] }))
        .collect();
    assert!(held.iter().all(Option::is_some), "the layer gave its three drawables");

    let stop = Arc::new(AtomicBool::new(false));
    let drawn = Arc::new(AtomicU32::new(0));
    let worker = thread::spawn({
        let (stop, drawn, device) = (stop.clone(), drawn.clone(), device.clone());
        move || {
            while !stop.load(Ordering::Relaxed) {
                fynkel_present::autorelease_pool(|| {
                    if draw(&device, &mut target) {
                        drawn.fetch_add(1, Ordering::Relaxed);
                    } else {
                        thread::sleep(Duration::from_millis(1));
                    }
                });
            }
        }
    });

    // Long enough for the worker to use up the layer's drawables and wait.
    thread::sleep(Duration::from_millis(300));
    let mut slowest = Duration::ZERO;
    for size in [80, 64, 96] {
        let started = Instant::now();
        // SAFETY: as above.
        unsafe { presenter.attach(pointer, size, size) }.unwrap();
        slowest = slowest.max(started.elapsed());
        thread::sleep(Duration::from_millis(50));
    }
    // The render thread sets the last size asked for before its next frame.
    // SAFETY: a live CAMetalLayer; `drawableSize` returns a CGSize.
    let size = || -> CGSize { unsafe { msg_send![&*layer, drawableSize] } };
    let waiting = Instant::now();
    while size() != (CGSize { width: 96.0, height: 96.0 }) && waiting.elapsed() < Duration::from_secs(3) {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(size(), CGSize { width: 96.0, height: 96.0 });

    let started = Instant::now();
    presenter.detach();
    slowest = slowest.max(started.elapsed());

    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    println!(
        "drawn {}, presented {}, the main thread waited at most {slowest:?}",
        drawn.load(Ordering::Relaxed),
        presenter.presented()
    );
    assert!(slowest < Duration::from_millis(50), "the main thread waited {slowest:?} for the render thread");
    drop(held);
    drop(layer);
}
