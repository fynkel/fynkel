//! Regression tests for the IOSurface ring contract (moved from the M0
//! experiment) and the viewport over a core world.

use std::time::Duration;

use fynkel_core::{Command, Lane, Transform, World, WorldConfig};
use fynkel_platform_apple::{IoSurfaceRing, SyncMode, Viewport};
use fynkel_render::{clear_rgba8, ExtractRenderer};

fn clear_bgra() -> [u8; 4] {
    let [r, g, b, a] = clear_rgba8();
    [b, g, r, a]
}

fn world_with_cube() -> World {
    let mut world = World::new(WorldConfig::default());
    world.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id: 1,
            transform: Transform::IDENTITY,
            parent: None,
            name: None,
        }],
    );
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
        assert_eq!(
            render(&ring, &mut renderer, &world),
            -1,
            "ring full: frame dropped, not overwritten"
        );
        settle(&ring, mode);
        // Newest ready wins; older ready slots are recycled; 0 retires.
        assert_eq!(ring.acquire_presented(), 3);
        assert_eq!(
            render(&ring, &mut renderer, &world),
            1,
            "0 is Retiring; 1 is the first Free"
        );
        let st = ring.take_stats();
        assert_eq!(st.frames_rendered, 5);
        assert_eq!(st.frames_dropped_no_free_buffer, 1);
        assert_eq!(st.frames_presented, 2);
        assert_eq!(st.cpu_us.len(), 5);
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

    world.submit(
        Lane::Simulation,
        vec![Command::Spawn {
            temp_id: 1,
            transform: Transform::IDENTITY,
            parent: None,
            name: None,
        }],
    );
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
