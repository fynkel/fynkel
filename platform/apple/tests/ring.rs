//! Regression tests for the IOSurface ring contract (moved from the M0
//! experiment) and the viewport over a core world.

use std::time::Duration;

use fynkel_core::{Transform, World, WorldConfig};
use fynkel_platform_apple::{IoSurfaceRing, SyncMode, Viewport};
use fynkel_render::{clear_rgba8, ExtractRenderer};

fn clear_bgra() -> [u8; 4] {
    let [r, g, b, a] = clear_rgba8();
    [b, g, r, a]
}

fn world_with_cube() -> World {
    let mut world = World::new(WorldConfig::default());
    world
        .edit_gameplay(|edit| edit.spawn("", Transform::IDENTITY, None))
        .unwrap();
    world.begin_frame(1.0 / 60.0);
    world.step();
    world
}

fn settle(ring: &IoSurfaceRing, mode: SyncMode) {
    if mode != SyncMode::Blocking {
        ring.wait_idle();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Renders the world's extract with a renderer owned by the test (the
/// `Viewport` does the same; this keeps the ring test independent of it).
fn render(ring: &IoSurfaceRing, renderer: &mut ExtractRenderer, world: &World) -> i32 {
    let extract = world.extract();
    renderer.prepare(ring.device(), ring.queue(), &extract);
    ring.render_with(|device, _q, view| {
        let mut enc = device.create_command_encoder(&Default::default());
        renderer.draw(&mut enc, view);
        enc.finish()
    })
}

#[test]
fn ring_rotates_slot_states_and_never_overwrites() {
    let world = world_with_cube();
    for mode in [
        SyncMode::Blocking,
        SyncMode::Async,
        SyncMode::MetalCompletion,
    ] {
        let ring = IoSurfaceRing::new(256, 256, 4, mode).expect("ring");
        let mut renderer = ExtractRenderer::new(
            ring.device(),
            fynkel_platform_apple::TEXTURE_FORMAT,
            256,
            256,
        );
        assert!(!ring.iosurface_ptr(0).is_null());
        assert!(ring.iosurface_ptr(4).is_null());
        assert_eq!(ring.acquire_presented(), -1, "nothing presented yet");

        assert_eq!(render(&ring, &mut renderer, &world), 0);
        settle(&ring, mode);
        assert_eq!(ring.acquire_presented(), 0, "first frame is presented");

        // Corner pixel is background; centre pixel is the lit cube.
        let corner = ring.read_pixel(0, 2, 2).unwrap();
        let centre = ring.read_pixel(0, 128, 128).unwrap();
        assert_eq!(
            corner,
            clear_bgra(),
            "corner is the clear colour ({mode:?})"
        );
        assert_ne!(centre, clear_bgra(), "centre shows the cube ({mode:?})");
        assert_eq!(centre[3], 255, "opaque");

        // Present with nothing new returns the same slot.
        assert_eq!(ring.acquire_presented(), 0);

        // Render three more; with 4 slots and one Presented, three are free.
        assert_eq!(render(&ring, &mut renderer, &world), 1);
        assert_eq!(render(&ring, &mut renderer, &world), 2);
        assert_eq!(render(&ring, &mut renderer, &world), 3);
        // Ring full. A frame still on the GPU cannot be taken (dropped); one
        // that finished unseen is stale and is recycled for the new frame
        // (slot 1, the oldest). Blocking mode completes inside `render`, so
        // there it is always the recycle; the others race the GPU.
        let fourth = render(&ring, &mut renderer, &world);
        if mode == SyncMode::Blocking {
            assert_eq!(fourth, 1, "oldest unshown frame recycled, never presented");
        } else {
            assert!(fourth == -1 || fourth == 1, "dropped or recycled, got {fourth}");
        }
        let dropped = fourth == -1;
        settle(&ring, mode);
        // Newest ready wins; older ready slots are recycled; 0 retires.
        assert_eq!(ring.acquire_presented(), if dropped { 3 } else { 1 });
        assert_eq!(
            render(&ring, &mut renderer, &world),
            if dropped { 1 } else { 2 },
            "0 is Retiring; the first Free is next"
        );
        let st = ring.take_stats();
        let rendered = if dropped { 5 } else { 6 };
        assert_eq!(st.frames_rendered, rendered);
        assert_eq!(st.frames_dropped_no_free_buffer, u64::from(dropped));
        assert_eq!(st.frames_presented, 2);
        assert_eq!(st.cpu_us.len(), rendered as usize);
        if mode != SyncMode::Blocking {
            assert!(ring.frame_ready_signals() >= 4);
        }
    }
}

#[test]
fn resize_rebuilds_ring_and_ignores_stale_completions() {
    let world = world_with_cube();
    let mut vp = Viewport::new(128, 128, 3, SyncMode::Async).expect("viewport");
    let old_surface = vp.ring().iosurface_ptr(0);
    assert_eq!(vp.render(&world.extract()), 0);
    assert_eq!(vp.render(&world.extract()), 1);
    // Resize while frames 0 and 1 may still be in flight.
    vp.resize(256, 192).expect("resize");
    assert_eq!(vp.ring().size(), (256, 192));
    assert_ne!(vp.ring().iosurface_ptr(0), old_surface, "new surfaces");
    // Stale completions must not have promoted anything.
    settle(vp.ring(), SyncMode::Async);
    assert_eq!(
        vp.ring().acquire_presented(),
        -1,
        "nothing presented after resize"
    );
    // Rendering into the new ring works and produces the right-sized frame.
    assert_eq!(vp.render(&world.extract()), 0);
    settle(vp.ring(), SyncMode::Async);
    assert_eq!(vp.ring().acquire_presented(), 0);
    assert_eq!(vp.ring().read_pixel(0, 2, 2).unwrap(), clear_bgra());
    assert_ne!(vp.ring().read_pixel(0, 128, 96).unwrap(), clear_bgra());
    assert!(vp.ring().read_pixel(0, 255, 191).is_some());
    // Same size is a no-op.
    vp.resize(256, 192).expect("noop resize");
    let st = vp.ring().take_stats();
    assert_eq!(st.resizes, 1);
    assert_eq!(st.frames_rendered, 3);
}

#[test]
fn presenter_wait_yields_lag_zero() {
    let world = world_with_cube();
    let mut vp = Viewport::new(1600, 1200, 4, SyncMode::MetalCompletion).expect("viewport");
    vp.ring().set_acquire_wait(Duration::from_millis(50)); // generous: first frame warms up
    for _ in 0..30 {
        assert!(vp.render(&world.extract()) >= 0);
        // Present immediately: the frame is still on the GPU; the wait
        // should absorb it.
        assert!(vp.ring().acquire_presented() >= 0);
        std::thread::sleep(Duration::from_micros(8333));
    }
    let mut st = vp.ring().take_stats();
    st.present_lag.sort_unstable();
    assert_eq!(
        *st.present_lag.last().unwrap(),
        0,
        "every present shows the newest frame"
    );
    st.acquire_wait_us.sort_unstable();
    let w95 = st.acquire_wait_us[(st.acquire_wait_us.len() - 1) * 95 / 100];
    assert!(
        w95 > 100 && w95 < 4000,
        "waited a bounded ~GPU time: {w95} us"
    );
}

#[test]
fn viewport_renders_the_world_the_host_steps() {
    // The host loop as the Flutter host runs it: step, then render the
    // extract; the extract is one tick behind and interpolated by alpha.
    let mut world = World::new(WorldConfig::default());
    let mut vp = Viewport::new(200, 200, 3, SyncMode::Blocking).expect("viewport");
    // Empty world renders (clear only).
    assert_eq!(vp.render(&world.extract()), 0);
    assert_eq!(vp.instance_count(), 0);
    assert_eq!(vp.ring().acquire_presented(), 0);
    assert_eq!(vp.ring().read_pixel(0, 100, 100).unwrap(), clear_bgra());

    world
        .edit_gameplay(|edit| edit.spawn("", Transform::IDENTITY, None))
        .unwrap();
    world.begin_frame(1.0 / 60.0);
    world.step();
    let slot = vp.render(&world.extract());
    assert!(slot >= 0);
    assert_eq!(vp.instance_count(), 1);
    assert_eq!(vp.ring().acquire_presented(), slot);
    assert_ne!(
        vp.ring().read_pixel(slot as u32, 100, 100).unwrap(),
        clear_bgra()
    );
}

#[test]
fn device_loss_is_reported_before_preparation_and_a_new_device_can_draw_the_snapshot() {
    use fynkel_platform_apple::{DeviceChoice, Gpu};
    let gpu = Gpu::get(DeviceChoice::Own).unwrap();
    let mut viewport = Viewport::with_gpu(gpu.clone(), 64, 64, 2, SyncMode::Blocking).unwrap();
    let snapshot = world_with_cube().extract();
    assert!(viewport.render(&snapshot) >= 0);
    gpu.device().destroy();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !gpu.is_lost() {
        let _ = gpu.device().poll(wgpu::PollType::Poll);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(viewport.render(&snapshot), -5);
    assert!(viewport.resize(100, 100).is_err());
    assert!(
        viewport.error().is_none(),
        "device loss is not a poisoned renderer panic"
    );
    let replacement = Gpu::get(DeviceChoice::Own).unwrap();
    let mut fresh = Viewport::with_gpu(replacement, 64, 64, 2, SyncMode::Blocking).unwrap();
    assert!(fresh.render(&snapshot) >= 0);
}

#[test]
fn admission_releases_errors_and_panics_without_blocking_the_presenter() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let mut ring = IoSurfaceRing::new(32, 32, 3, SyncMode::Blocking).unwrap();
    let empty = |device: &wgpu::Device, _: &wgpu::Queue, _: &wgpu::TextureView| {
        device.create_command_encoder(&Default::default()).finish()
    };
    assert_eq!(ring.render_with(empty), 0);
    ring.take_stats();
    let result = ring.try_render_with(|_, _, _| {
        // Reentrant acquisition would deadlock if preparation held the mutex.
        // Reserved CPU work is not a submitted frame to wait for or present.
        assert_eq!(ring.acquire_presented(), 0);
        Err::<wgpu::CommandBuffer, _>("asset preparation failed")
    });
    assert_eq!(result, Err("asset preparation failed"));
    let st = ring.take_stats();
    assert_eq!(st.acquire_wait_us, [0]);
    assert_eq!(st.present_lag, [0]);
    assert_eq!(st.frames_rendered, 0);
    assert!(catch_unwind(AssertUnwindSafe(|| {
        ring.render_with(|_, _, _| panic!("recording failed"));
    }))
    .is_err());
    assert_eq!(
        ring.render_with(empty),
        1,
        "both cancellations released slot 1"
    );
    assert_eq!(ring.acquire_presented(), 1);
    assert_eq!(
        ring.render_with(empty),
        2,
        "Presented and Retiring remain protected"
    );
    // Presented (1), Retiring (0) and a finished frame nobody has shown (2):
    // the stale frame is recycled for the new one, the protected two are not.
    assert_eq!(ring.render_with(empty), 2, "an unshown frame is recycled");
    assert_eq!(ring.acquire_presented(), 2);
    assert_eq!(ring.render_with(empty), 0, "only retired slot is recycled");
    ring.resize(64, 64).unwrap();
    assert_eq!(ring.acquire_presented(), -1);
    assert_eq!(ring.render_with(empty), 0);
    assert_eq!(ring.acquire_presented(), 0);
}

#[test]
fn full_async_ring_can_complete_without_renderer_preparation() {
    let ring = IoSurfaceRing::new(32, 32, 3, SyncMode::Async).unwrap();
    let empty = |device: &wgpu::Device, _: &wgpu::Queue, _: &wgpu::TextureView| {
        device.create_command_encoder(&Default::default()).finish()
    };
    for slot in 0..3 {
        assert_eq!(ring.render_with(empty), slot);
    }
    // No wait_idle and no renderer/GPU timer polling: admission itself keeps
    // asynchronous completion alive even when every texture is occupied. While
    // all three are on the GPU a frame is dropped without preparation; once
    // admission's own poll sees one complete, the stale frame is recycled.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let slot = ring.render_with(|device, _, _| {
            device.create_command_encoder(&Default::default()).finish()
        });
        if slot >= 0 {
            break;
        }
        assert_eq!(slot, -1);
        assert!(std::time::Instant::now() < deadline, "completion stalled");
        std::thread::yield_now();
    }
    ring.wait_idle();
}

#[test]
fn a_full_ring_of_protected_slots_drops_without_preparation() {
    // Two slots: Presented plus the one being prepared (Reserved). Nothing is
    // Free or Ready, so a reentrant admission must drop and skip its closure.
    let ring = IoSurfaceRing::new(32, 32, 2, SyncMode::Blocking).unwrap();
    let empty = |device: &wgpu::Device, _: &wgpu::Queue, _: &wgpu::TextureView| {
        device.create_command_encoder(&Default::default()).finish()
    };
    assert_eq!(ring.render_with(empty), 0);
    assert_eq!(ring.acquire_presented(), 0);
    let outer = ring.try_render_with(|_, _, _| {
        assert_eq!(
            ring.render_with(|_, _, _| panic!("a protected ring must skip preparation")),
            -1
        );
        Ok::<_, ()>(
            ring_empty_commands(&ring),
        )
    });
    assert_eq!(outer, Ok(1));
}

fn ring_empty_commands(ring: &IoSurfaceRing) -> wgpu::CommandBuffer {
    ring.device().create_command_encoder(&Default::default()).finish()
}

#[test]
fn a_frame_queue_shows_finished_frames_in_order_and_catches_up_when_it_falls_behind() {
    let world = world_with_cube();
    let ring = IoSurfaceRing::new(64, 64, 5, SyncMode::Blocking).expect("ring");
    let mut renderer = ExtractRenderer::new(
        ring.device(),
        fynkel_platform_apple::TEXTURE_FORMAT,
        64,
        64,
    );
    // A mailbox (the default) shows the newest and recycles the older.
    assert_eq!(render(&ring, &mut renderer, &world), 0);
    assert_eq!(render(&ring, &mut renderer, &world), 1);
    assert_eq!(ring.acquire_presented_queued(), (1, false));
    assert_eq!(ring.acquire_presented_queued(), (1, false), "nothing newer: the same frame");

    // Depth 2: two finished frames are shown one raster each, and the first
    // answer says another is queued behind it.
    ring.set_frame_queue(2);
    assert_eq!(render(&ring, &mut renderer, &world), 0, "the retired slot");
    assert_eq!(render(&ring, &mut renderer, &world), 2);
    assert_eq!(ring.acquire_presented_queued(), (0, true), "the older first");
    assert_eq!(ring.acquire_presented_queued(), (2, false), "then the newer");
    assert_eq!(ring.acquire_presented_queued(), (2, false), "then nothing new");

    // Three waiting is more than the queue holds: jump to the newest.
    let a = render(&ring, &mut renderer, &world);
    let b = render(&ring, &mut renderer, &world);
    let c = render(&ring, &mut renderer, &world);
    assert!(a >= 0 && b >= 0 && c >= 0);
    assert_eq!(ring.acquire_presented_queued(), (c, false), "caught up to the newest");
}
