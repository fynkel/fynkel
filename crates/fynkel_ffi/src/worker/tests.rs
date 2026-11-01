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

/// The time spent drawing is counted, and is never more than the time
/// that passed.
#[test]
fn the_time_spent_drawing_is_counted() {
    let gpu = gpu();
    let (target, frames) = headless(&gpu);
    let worker = start(&gpu, target);
    let clock = std::time::Instant::now();
    assert_eq!(worker.stats().busy_micros, 0);
    for number in 1..=20 {
        let scene = worker.scene();
        worker.post(number, scene, &mut Vec::new()).unwrap();
        frames.read(number, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]).unwrap();
    }
    let stats = wait_for(&worker, |s| s.rendered == 20);
    assert!(stats.busy_micros > 0);
    assert!(u128::from(stats.busy_micros) <= clock.elapsed().as_micros(), "{stats:?}");
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
    let spare = worker.spare_scenes();
    let error = worker.post(8, RenderScene::default(), &mut Vec::new()).unwrap_err();
    assert_eq!(error, Error::new(FyStatus::WorkerFailed, "the render worker panicked: boom in frame 7"));
    assert_eq!(worker.spare_scenes(), spare + 1, "the scene posted in vain is kept for reuse");
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
    assert_eq!(
        worker.failure(),
        Some(Error::new(FyStatus::WorkerFailed, "couldn't resize the view to 9 × 9: no room for 9 × 9"))
    );
    assert_eq!(worker.resize(10, 10), Err(worker.failure().unwrap()), "later calls report it");
}

/// Waits until the worker has stopped, and returns why.
fn failure(worker: &Worker) -> Error {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(failure) = worker.failure() {
            return failure;
        }
        assert!(Instant::now() < deadline, "the worker goes on");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_lost_device_stops_the_worker_with_its_own_status() {
    let gpu = gpu();
    let (target, frames) = headless(&gpu);
    let worker = start(&gpu, target);
    worker.post(1, worker.scene(), &mut Vec::new()).unwrap();
    frames.read(1, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]).unwrap();
    worker.lose_device();
    let failure = failure(&worker);
    assert_eq!(failure, Error::new(FyStatus::DeviceLost, "the GPU device was lost"));
    assert_eq!(worker.post(2, RenderScene::default(), &mut Vec::new()), Err(failure));
    drop(worker);

    // A new device draws again.
    let gpu = Gpu::headless().expect("a new device");
    let (target, frames) = headless(&gpu);
    let worker = start(&gpu, target);
    worker.post(1, worker.scene(), &mut Vec::new()).unwrap();
    assert_eq!(frames.read(1, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]), Ok(1));
}

#[test]
fn a_gpu_error_stops_the_worker_rather_than_panic() {
    let gpu = gpu();
    let worker = start(&gpu, headless(&gpu).0);
    // Mapped at creation, a buffer's size must be a multiple of 4.
    let _ = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 3,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    let failure = failure(&worker);
    assert_eq!(failure.status, FyStatus::DeviceLost);
    assert!(failure.message.starts_with("the GPU reported an error"), "{}", failure.message);
}

/// A target with no room for its first frame, which frees some from
/// another thread a little later, as Metal's completion does.
struct Full {
    inner: Headless,
    full: bool,
    wake: Arc<Mutex<Option<fynkel_present::Wake>>>,
}

impl Target for Full {
    type Frame = <Headless as Target>::Frame;
    fn format(&self) -> wgpu::TextureFormat {
        self.inner.format()
    }
    fn size(&self) -> (u32, u32) {
        self.inner.size()
    }
    fn begin(&mut self) -> Option<Self::Frame> {
        if std::mem::take(&mut self.full) {
            return None;
        }
        self.inner.begin()
    }
    fn view<'a>(&'a self, frame: &'a Self::Frame) -> &'a wgpu::TextureView {
        self.inner.view(frame)
    }
    fn finish(&mut self, frame: Self::Frame, encoder: wgpu::CommandEncoder, number: u64) {
        self.inner.finish(frame, encoder, number);
    }
    fn on_free(&mut self, wake: fynkel_present::Wake) -> bool {
        *self.wake.lock().unwrap() = Some(wake);
        true
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.inner.resize(width, height)
    }
}

fn full(gpu: &Gpu) -> (Worker, HeadlessFrames, Arc<Mutex<Option<fynkel_present::Wake>>>) {
    let (inner, frames) = headless(gpu);
    let wake = Arc::default();
    (start(gpu, Full { inner, full: true, wake: Arc::clone(&wake) }), frames, wake)
}

#[test]
fn a_scene_with_no_room_is_drawn_when_a_frame_frees_some() {
    let gpu = gpu();
    let (worker, frames, wake) = full(&gpu);
    worker.post(1, worker.scene(), &mut Vec::new()).unwrap();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(worker.stats().rendered, 0, "waits for room");
    let wake = wake.lock().unwrap().clone().expect("the worker asked to be woken");
    wake();
    assert_eq!(frames.read(1, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]), Ok(1), "without a new post");
    let stats = wait_for(&worker, |stats| stats.rendered == 1);
    assert_eq!((stats.posted, stats.skipped, stats.coalesced), (1, 0, 0));
}

#[test]
fn a_newer_scene_replaces_one_waiting_for_room_which_counts_as_skipped() {
    let gpu = gpu();
    let (worker, frames, wake) = full(&gpu);
    worker.post(1, worker.scene(), &mut Vec::new()).unwrap();
    thread::sleep(Duration::from_millis(50));
    worker.post(2, worker.scene(), &mut Vec::new()).unwrap();
    wake.lock().unwrap().clone().expect("the worker asked to be woken")();
    assert_eq!(frames.read(2, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]), Ok(2));
    let stats = wait_for(&worker, |stats| stats.rendered == 1);
    assert_eq!((stats.posted, stats.skipped, stats.coalesced), (2, 1, 0));
}

/// A target whose frames make a GPU error on the render thread.
struct Faulty(Headless, wgpu::Device);

impl Target for Faulty {
    type Frame = <Headless as Target>::Frame;
    fn format(&self) -> wgpu::TextureFormat {
        self.0.format()
    }
    fn size(&self) -> (u32, u32) {
        self.0.size()
    }
    fn begin(&mut self) -> Option<Self::Frame> {
        // Mapped at creation, a buffer's size must be a multiple of 4.
        let _ = self.1.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 3,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        self.0.begin()
    }
    fn view<'a>(&'a self, frame: &'a Self::Frame) -> &'a wgpu::TextureView {
        self.0.view(frame)
    }
    fn finish(&mut self, frame: Self::Frame, encoder: wgpu::CommandEncoder, number: u64) {
        self.0.finish(frame, encoder, number);
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.0.resize(width, height)
    }
}

#[test]
fn a_gpu_error_in_a_frame_stops_that_view_alone() {
    let gpu = gpu();
    let worker = start(&gpu, Faulty(headless(&gpu).0, gpu.device.clone()));
    worker.post(1, worker.scene(), &mut Vec::new()).unwrap();
    let failure = failure(&worker);
    assert_eq!(failure.status, FyStatus::WorkerFailed);
    assert!(failure.message.starts_with("the GPU reported an error"), "{}", failure.message);
    assert!(!worker.world.device.is_lost(), "the device every view shares goes on");
}

/// A target that notes the thread it draws on, and when it is dropped.
struct Noting {
    inner: Headless,
    thread: Arc<Mutex<Option<thread::ThreadId>>>,
    dropped: Arc<Mutex<bool>>,
}

impl Drop for Noting {
    fn drop(&mut self) {
        *self.dropped.lock().unwrap() = true;
    }
}

impl Target for Noting {
    type Frame = <Headless as Target>::Frame;
    fn format(&self) -> wgpu::TextureFormat {
        self.inner.format()
    }
    fn size(&self) -> (u32, u32) {
        self.inner.size()
    }
    fn begin(&mut self) -> Option<Self::Frame> {
        *self.thread.lock().unwrap() = Some(thread::current().id());
        self.inner.begin()
    }
    fn view<'a>(&'a self, frame: &'a Self::Frame) -> &'a wgpu::TextureView {
        self.inner.view(frame)
    }
    fn finish(&mut self, frame: Self::Frame, encoder: wgpu::CommandEncoder, number: u64) {
        self.inner.finish(frame, encoder, number);
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.inner.resize(width, height)
    }
}

/// Starts a worker on `threads` with a [`Noting`] target, draws one frame,
/// and returns it with the thread it drew on and its dropped flag.
fn noting(gpu: &Gpu, threads: &'static RenderThreads) -> (Worker, thread::ThreadId, Arc<Mutex<bool>>) {
    let (inner, frames) = headless(gpu);
    let (thread, dropped) = (Arc::default(), Arc::default());
    let target = Noting { inner, thread: Arc::clone(&thread), dropped: Arc::clone(&dropped) };
    let renderer = Renderer::new(gpu, target.format());
    let worker = Worker::start_on(threads, gpu.clone(), renderer, target).unwrap();
    worker.post(1, worker.scene(), &mut Vec::new()).unwrap();
    frames.read(1, Duration::from_secs(10), &mut vec![0; 16 * 16 * 4]).unwrap();
    let id = thread.lock().unwrap().expect("drew a frame");
    (worker, id, dropped)
}

/// Its own threads, apart from the other tests'.
fn threads() -> &'static RenderThreads {
    Box::leak(Box::new(RenderThreads::new()))
}

#[test]
fn a_view_made_after_another_gets_its_render_thread() {
    let gpu = gpu();
    let threads = threads();
    let (first, thread, _) = noting(&gpu, threads);
    drop(first);
    let (second, again, _) = noting(&gpu, threads);
    assert_eq!(again, thread, "the same thread");
    assert_ne!(again, thread::current().id());
    drop(second);
}

#[test]
fn only_one_render_thread_waits() {
    let gpu = gpu();
    let threads = threads();
    let (a, thread_a, _) = noting(&gpu, threads);
    let (b, thread_b, _) = noting(&gpu, threads);
    assert_ne!(thread_a, thread_b, "two views at once, two threads");
    drop(a);
    drop(b);
    assert_eq!(threads.idle().len(), 1, "b's thread ended");
    let (c, thread_c, _) = noting(&gpu, threads);
    assert_eq!(thread_c, thread_a);
    let (d, thread_d, _) = noting(&gpu, threads);
    assert!(thread_d != thread_a && thread_d != thread_b, "a new thread when none waits");
    drop((c, d));
}

/// fy_view_free relies on it: when it returns, the GPU objects are gone.
#[test]
fn a_dropped_worker_has_dropped_everything_it_owned() {
    let gpu = gpu();
    let threads = threads();
    for _ in 0..20 {
        let (worker, _, dropped) = noting(&gpu, threads);
        worker.post(2, worker.scene(), &mut Vec::new()).unwrap();
        drop(worker);
        assert!(*dropped.lock().unwrap(), "the target went with the worker");
    }
}

#[test]
fn a_thread_whose_worker_panicked_serves_the_next_view() {
    let gpu = gpu();
    let threads = threads();
    let renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
    let boom = Worker::start_on(threads, gpu.clone(), renderer, Boom(headless(&gpu).0)).unwrap();
    boom.post(1, RenderScene::default(), &mut Vec::new()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while boom.failure().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    drop(boom);
    let (next, _, _) = noting(&gpu, threads);
    // Counted just after the frame is read.
    wait_for(&next, |stats| stats.rendered == 1);
}

// Hardening R8: a frame makes at most a budget of uploads, oldest first;
// the rest wait for the next, in order. Removing costs nothing, and a
// frame always makes at least one, however big.
#[test]
fn a_frame_takes_its_budget_of_uploads_and_leaves_the_rest_in_order() {
    use fynkel_render::{ColorSpace, MeshKey, TextureKey};
    use fynkel_world::ImageData;
    let image = |side: u32| Arc::new(ImageData::from_rgba(side, side, vec![0; (side * side * 4) as usize]).unwrap());
    let texture = |k: u64, side: u32| ResourceOp::UploadTexture(TextureKey(k), ColorSpace::Srgb, image(side));
    let names = |ops: &[ResourceOp]| -> Vec<String> {
        ops.iter()
            .map(|op| match op {
                ResourceOp::UploadTexture(k, ..) | ResourceOp::UploadAtlas(k, _) => format!("t{}", k.0),
                ResourceOp::RemoveTexture(k) => format!("-t{}", k.0),
                ResourceOp::UploadMesh(k, _) => format!("m{}", k.0),
                ResourceOp::RemoveMesh(k) => format!("-m{}", k.0),
                ResourceOp::UploadShader(k, _) => format!("s{}", k.0),
                ResourceOp::RemoveShader(k) => format!("-s{}", k.0),
            })
            .collect()
    };
    // 1024² is 4 MiB: three of them over a 10 MiB budget.
    let mut waiting = vec![
        texture(1, 1024),
        ResourceOp::RemoveTexture(TextureKey(7)),
        texture(2, 1024),
        ResourceOp::UploadMesh(MeshKey(3), Arc::new(fynkel_world::MeshData::cube())),
        texture(4, 1024),
        ResourceOp::RemoveMesh(MeshKey(5)),
    ];
    let mut frame = Vec::new();
    take_within(&mut waiting, &mut frame, 10 << 20);
    assert_eq!(names(&frame), ["t1", "-t7", "t2", "m3"]);
    assert_eq!(names(&waiting), ["t4", "-m5"]);

    let mut big = vec![texture(1, 4096), texture(2, 16)];
    frame.clear();
    take_within(&mut big, &mut frame, 10 << 20);
    assert_eq!(names(&frame), ["t1"], "one too big for the budget still goes, alone");
}
