use std::time::{Duration, Instant};

use fynkel_present::{Headless, HeadlessFrames};
use fynkel_render::wgpu;

use super::*;

fn gpu() -> Gpu {
    Gpu::headless().expect("these tests need a GPU")
}

fn headless(gpu: &Gpu) -> (Headless, HeadlessFrames) {
    Headless::new(&gpu.device, &gpu.queue, 16, 16)
}

fn start<T: Target + 'static>(gpu: &Gpu, target: T) -> Worker {
    let renderer = Renderer::new(gpu, target.format());
    Worker::start(gpu.clone(), renderer, target).unwrap()
}

/// Waits until `done` holds for the worker's stats.
fn wait_for(worker: &Worker, done: impl Fn(WorkerStats) -> bool) -> WorkerStats {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let stats = worker.stats();
        if done(stats) {
            return stats;
        }
        assert!(Instant::now() < deadline, "timed out at {stats:?}");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_newest_scene_wins_and_every_scene_is_accounted_for() {
    let gpu = gpu();
    let (target, frames) = headless(&gpu);
    let worker = start(&gpu, target);
    for number in 1..=200 {
        let scene = worker.scene();
        worker.post(number, scene, &mut Vec::new()).unwrap();
    }
    let mut rgba = vec![0; 16 * 16 * 4];
    assert_eq!(frames.read(200, Duration::from_secs(10), &mut rgba), Ok(200), "the last scene is always drawn");
    let stats = wait_for(&worker, |s| s.rendered + s.coalesced == 200);
    assert_eq!(stats.posted, 200);
    assert!(stats.coalesced > 0, "posting is far faster than drawing: {stats:?}");
}

#[test]
fn scene_buffers_are_reused() {
    let gpu = gpu();
    let (target, frames) = headless(&gpu);
    let worker = start(&gpu, target);
    let mut scene = worker.scene();
    scene.draws.reserve(1000);
    worker.post(1, scene, &mut Vec::new()).unwrap();
    frames.read(1, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]).unwrap();
    wait_for(&worker, |s| s.rendered == 1);
    let again = worker.scene();
    assert!(again.draws.capacity() >= 1000, "the buffer came back");
}

#[test]
fn no_more_than_three_scene_buffers_ever_exist() {
    let gpu = gpu();
    let (target, frames) = headless(&gpu);
    let worker = start(&gpu, target);
    let mut made = 0;
    for number in 1..=500 {
        let spare = worker.spare_scenes();
        let scene = worker.scene();
        if spare == 0 {
            made += 1;
        }
        worker.post(number, scene, &mut Vec::new()).unwrap();
    }
    frames.read(500, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]).unwrap();
    assert!(made <= 3, "{made} scene buffers made");
    wait_for(&worker, |s| s.rendered + s.coalesced == 500);
    assert_eq!(worker.spare_scenes(), made, "every buffer came back");
}

/// A target that never has anywhere to draw.
struct Nowhere(Headless);

impl Target for Nowhere {
    type Frame = ();
    fn format(&self) -> wgpu::TextureFormat {
        self.0.format()
    }
    fn size(&self) -> (u32, u32) {
        self.0.size()
    }
    fn begin(&mut self) -> Option<()> {
        None
    }
    fn view<'a>(&'a self, frame: &'a ()) -> &'a wgpu::TextureView {
        self.0.view(frame)
    }
    fn finish(&mut self, _: (), _: wgpu::CommandEncoder, _: u64) {
        unreachable!("never begun")
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.0.resize(width, height)
    }
}

#[test]
fn a_scene_with_nowhere_to_draw_is_skipped() {
    let gpu = gpu();
    let worker = start(&gpu, Nowhere(headless(&gpu).0));
    for number in 1..=3 {
        let scene = worker.scene();
        worker.post(number, scene, &mut Vec::new()).unwrap();
        wait_for(&worker, |s| s.skipped + s.coalesced == number);
    }
    assert_eq!(worker.stats().rendered, 0);
}

/// A target whose frames panic.
struct Boom(Headless);

impl Target for Boom {
    type Frame = ();
    fn format(&self) -> wgpu::TextureFormat {
        self.0.format()
    }
    fn size(&self) -> (u32, u32) {
        self.0.size()
    }
    fn begin(&mut self) -> Option<()> {
        Some(())
    }
    fn view<'a>(&'a self, frame: &'a ()) -> &'a wgpu::TextureView {
        self.0.view(frame)
    }
    fn finish(&mut self, _: (), _: wgpu::CommandEncoder, number: u64) {
        panic!("boom in frame {number}");
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        Err(format!("no room for {width} × {height}"))
    }
}

#[test]
fn a_panic_on_the_worker_is_reported_to_the_next_post() {
    let gpu = gpu();
    let worker = start(&gpu, Boom(headless(&gpu).0));
    worker.post(7, RenderScene::default(), &mut Vec::new()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while worker.failure().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let error = worker.post(8, RenderScene::default(), &mut Vec::new()).unwrap_err();
    assert_eq!(error, "the render worker panicked: boom in frame 7");
    drop(worker); // joins a thread that already ended
}

#[test]
fn dropping_the_worker_stops_it_even_with_a_scene_waiting() {
    let gpu = gpu();
    for _ in 0..20 {
        let worker = start(&gpu, headless(&gpu).0);
        worker.post(1, RenderScene::default(), &mut Vec::new()).unwrap();
        worker.post(2, RenderScene::default(), &mut Vec::new()).unwrap();
        drop(worker);
    }
}

#[test]
fn a_resize_the_target_refuses_stops_the_worker_with_a_clear_message() {
    let gpu = gpu();
    let worker = start(&gpu, Boom(headless(&gpu).0));
    worker.resize(9, 9).unwrap();
    worker.post(1, worker.scene(), &mut Vec::new()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while worker.failure().is_none() && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(worker.failure().as_deref(), Some("couldn't resize the view to 9 × 9: no room for 9 × 9"));
    assert_eq!(worker.resize(10, 10), Err(worker.failure().unwrap()), "later calls report it");
}
