//! A device without a window, and one frame rendered offscreen: the tests'
//! and `fynkel render`'s way in.

use super::*;

/// Creates a headless Metal/any-backend device for tests and offscreen hosts.
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("fynkel headless device"),
        required_features: device_features(&adapter),
        required_limits: device_limits(&adapter),
        ..Default::default()
    }))
    .ok()
}

/// Features to request for [`ExtractRenderer`]: pass timestamps when the
/// adapter samples them (`FrameStats::gpu_*`, M3-16); nothing the renderer
/// cannot do without.
pub fn device_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    // ASTC (ADR-0061): cooked textures upload as blocks where the GPU
    // samples them; elsewhere they are decoded at upload.
    adapter.features()
        & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TEXTURE_COMPRESSION_ASTC)
}

/// Limits to request for [`ExtractRenderer`]: WebGPU defaults, lowered to
/// what the adapter offers (the iOS simulator's Metal device sits below
/// them, e.g. 15 inter-stage variables). The renderer needs nothing beyond
/// the downlevel set, so every Apple adapter satisfies the result.
pub fn device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    wgpu::Limits::default().or_worse_values_from(&adapter.limits())
}

/// Tiny inline executor so the renderer does not depend on an async runtime.
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: the vtable functions do nothing with the (null) data pointer.
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_micros(50));
    }
}

/// Test/diagnostic helper: renders the extract into an offscreen RGBA8
/// texture and returns the pixels (row-major, tightly packed). This is the
/// readback the production paths never do per frame.
pub fn render_offscreen(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut ExtractRenderer,
    extract: &Extract,
) -> Vec<[u8; 4]> {
    let (width, height) = renderer.size();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fynkel offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // Rows must be 256-byte aligned for the copy.
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fynkel readback"),
        size: bytes_per_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    if let Err(error) =
        renderer.prepare_ready(device, queue, extract, std::time::Duration::from_secs(30))
    {
        if renderer.asset_errors.is_empty() {
            renderer.asset_errors.push(error);
        }
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &view);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().expect("map callback").expect("map readback");
    let data = slice.get_mapped_range().expect("mapped range");
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        let row = &data[y * bytes_per_row as usize..][..width as usize * 4];
        for px in row.chunks_exact(4) {
            pixels.push([px[0], px[1], px[2], px[3]]);
        }
    }
    drop(data);
    readback.unmap();
    pixels
}

/// `CLEAR_COLOR` as RGBA8 (the bytes the post pass writes where nothing
/// was drawn, whatever the target format).
pub fn clear_rgba8() -> [u8; 4] {
    [
        (CLEAR_COLOR.r * 255.0).round() as u8,
        (CLEAR_COLOR.g * 255.0).round() as u8,
        (CLEAR_COLOR.b * 255.0).round() as u8,
        255,
    ]
}
