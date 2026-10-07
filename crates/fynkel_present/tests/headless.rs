//! The headless target, driven the way the render worker drives it, with
//! a plain clear pass standing in for the renderer.

use std::thread;
use std::time::Duration;

use fynkel_present::{Headless, ReadError, Target, wgpu};

fn device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).expect("these tests need a GPU");
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
        ..Default::default()
    }))
    .expect("a device")
}

/// One frame that clears to `rgb`, driven through the trait the way the
/// render worker drives any target. The values are 8-bit sRGB, which an
/// sRGB target encodes back to the same bytes.
fn draw<T: Target>(device: &wgpu::Device, target: &mut T, rgb: [u8; 3], number: u64) {
    let frame = target.begin().expect("headless always has somewhere to draw");
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
    target.finish(frame, encoder, number);
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

#[test]
fn every_pixel_of_the_latest_frame_is_read_back() {
    let (device, queue) = device();
    // 70 wide: 280 bytes a row, padded to 512 on the GPU, so the unpadding
    // is tested.
    let (mut target, frames) = Headless::new(&device, &queue, 70, 3);
    assert_eq!(target.format(), wgpu::TextureFormat::Rgba8UnormSrgb);
    assert_eq!((target.size(), frames.size()), ((70, 3), (70, 3)));
    let mut rgba = vec![0; 70 * 3 * 4];

    draw(&device, &mut target, [200, 30, 90], 1);
    assert_eq!(frames.read(1, Duration::ZERO, &mut rgba), Ok(1));
    assert!(rgba.chunks(4).all(|p| p == [200, 30, 90, 255]), "{:?}", &rgba[..8]);

    draw(&device, &mut target, [10, 250, 0], 2);
    draw(&device, &mut target, [1, 2, 3], 3);
    assert_eq!(frames.read(2, Duration::ZERO, &mut rgba), Ok(3), "a later frame satisfies an earlier wish");
    assert!(rgba.chunks(4).all(|p| p == [1, 2, 3, 255]));
}

#[test]
fn a_reader_waits_for_its_frame_on_another_thread() {
    let (device, queue) = device();
    let (mut target, frames) = Headless::new(&device, &queue, 4, 4);
    let reader = thread::spawn(move || {
        let mut rgba = vec![0; 4 * 4 * 4];
        let number = frames.read(5, Duration::from_secs(10), &mut rgba);
        (number, rgba[..4].to_vec())
    });
    for n in 1..=5 {
        draw(&device, &mut target, [n as u8 * 40, 0, 0], n);
    }
    assert_eq!(reader.join().unwrap(), (Ok(5), vec![200, 0, 0, 255]));
}

#[test]
fn reading_a_frame_that_never_comes_times_out() {
    let (device, queue) = device();
    let (mut target, frames) = Headless::new(&device, &queue, 2, 2);
    draw(&device, &mut target, [0, 0, 0], 1);
    let mut rgba = vec![0; 16];
    assert_eq!(frames.read(2, Duration::from_millis(20), &mut rgba), Err(ReadError::Timeout { wanted: 2, latest: 1 }));
}

#[test]
fn a_buffer_of_the_wrong_size_is_refused() {
    let (device, queue) = device();
    let (_target, frames) = Headless::new(&device, &queue, 2, 2);
    assert_eq!(frames.read(0, Duration::ZERO, &mut [0; 15]), Err(ReadError::WrongSize { expected: 16, got: 15 }));
}
