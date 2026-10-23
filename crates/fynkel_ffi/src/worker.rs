//! The render worker (stage 1 plan §3.5): one thread that owns the
//! renderer and the target, and draws the scenes the frame clock posts.
//!
//! Posting never waits for the GPU. Scenes go through a mailbox where the
//! newest wins: a scene posted while another is still waiting replaces it
//! (it is *coalesced*), so a slow GPU means fewer, fresher frames rather
//! than a growing queue. Mesh and texture changes are not coalesced; they
//! queue up in order, in the world's ledger, and the first worker of the
//! world to draw next makes them (the world's views share its meshes and
//! textures, and the device: `device.rs`).
//!
//! Scene buffers circulate between the poster and the worker, so a steady
//! stream of frames allocates nothing on either side. There are never more
//! than three: one being filled, one waiting and one being drawn.
//!
//! A render thread outlives its view: when the view is freed, the thread
//! waits for the next one (one thread at most waits). A GPU driver may keep
//! state for every thread that calls it and never give it back, even when
//! the thread ends: the Android emulator's keeps 4 MB (docs/stage-1.md §7).
//! So a view made after another, as at every hot restart, costs no new
//! thread and leaves nothing behind.

use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, TryLockError, Weak, mpsc};
use std::thread;

use fynkel_present::Target;
#[cfg(test)]
use fynkel_render::Gpu;
use fynkel_render::{MadeMesh, MadeTexture, MeshKey, RenderScene, Renderer, TextureKey, wgpu};

#[cfg(test)]
use crate::device::Device;
use crate::device::WorldGpu;
use crate::error::{Error, FyStatus};
use crate::extract::ResourceOp;

/// Counts since the worker started. Every scene posted is rendered,
/// coalesced, skipped, or still waiting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkerStats {
    pub(crate) posted: u64,
    pub(crate) rendered: u64,
    /// Replaced by a newer scene before the worker got to it.
    pub(crate) coalesced: u64,
    /// The target had nowhere to draw. The next scene posted brings the
    /// picture up to date.
    pub(crate) skipped: u64,
    /// GPU memory the world's textures and meshes hold, as of the last
    /// scene taken.
    pub(crate) texture_bytes: u64,
    pub(crate) mesh_bytes: u64,
    /// GPU memory the sky's light holds, as of the last scene drawn.
    pub(crate) sky_bytes: u64,
    /// Time the render thread spent on scenes, from taking one to being
    /// done with it, in microseconds. On Android that includes waiting for
    /// the GPU to finish the frame.
    pub(crate) busy_micros: u64,
}

#[derive(Debug)]
pub(crate) struct Worker {
    shared: Arc<Shared>,
    world: Arc<WorldGpu>,
}

/// Stops a worker from outside, for its device: lost, or in error.
#[derive(Debug)]
pub(crate) struct Stopper(Weak<Shared>);

impl Stopper {
    pub(crate) fn stop(&self, message: String) {
        if let Some(shared) = self.0.upgrade() {
            shared.fail(message);
        }
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.0.strong_count() > 0
    }
}

#[derive(Debug)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
    /// Signalled once the worker has stopped and dropped everything it
    /// owned.
    finished: Condvar,
}

#[derive(Debug, Default)]
struct Mailbox {
    pending: Option<(u64, RenderScene)>,
    /// Scene buffers ready for reuse.
    spare: Vec<RenderScene>,
    /// A new size for the target, applied before the next scene is drawn.
    size: Option<(u32, u32)>,
    stop: bool,
    /// A test asked the worker to destroy the device ([`Worker::lose_device`]).
    lose_device: bool,
    /// A test asked the worker to panic ([`Worker::panic`]).
    panic: bool,
    /// The worker stopped and dropped everything it owned.
    finished: bool,
    /// `pending` is a scene the target had no room for, waiting for a
    /// frame to free one ([`Target::on_free`]).
    blocked: bool,
    /// Whether `pending` was put back after the target had no room: if a
    /// newer scene replaces it, it counts as skipped, not coalesced.
    retried: bool,
    /// Frames the target finished and freed room with, so far.
    freed: u64,
    /// Why the worker stopped, if it did on its own: it panicked or the GPU
    /// reported an error in its frame ([`FyStatus::WorkerFailed`]), or the
    /// device was lost or reported an error elsewhere
    /// ([`FyStatus::DeviceLost`]).
    failure: Option<(FyStatus, String)>,
    stats: WorkerStats,
}

impl Worker {
    /// Starts a worker of its own (a device and a world of its own, the
    /// renderer's assets) on a render thread, for tests.
    #[cfg(test)]
    pub(crate) fn start<T: Target + 'static>(gpu: Gpu, renderer: Renderer, target: T) -> std::io::Result<Self> {
        Self::start_on(&RENDER_THREADS, gpu, renderer, target)
    }

    /// [`Worker::start`], on a thread of `threads`.
    #[cfg(test)]
    fn start_on<T: Target + 'static>(
        threads: &'static RenderThreads,
        gpu: Gpu,
        renderer: Renderer,
        target: T,
    ) -> std::io::Result<Self> {
        let world = WorldGpu::with_assets(&Device::new(gpu), renderer.assets().clone());
        Self::start_in(threads, world, renderer, target)
    }

    /// Starts the worker of a view of `world` on a render thread, which owns
    /// `renderer` (sharing `world`'s assets) and `target` from now on.
    pub(crate) fn start_for<T: Target + 'static>(
        world: Arc<WorldGpu>,
        renderer: Renderer,
        target: T,
    ) -> std::io::Result<Self> {
        Self::start_in(&RENDER_THREADS, world, renderer, target)
    }

    fn start_in<T: Target + 'static>(
        threads: &'static RenderThreads,
        world: Arc<WorldGpu>,
        renderer: Renderer,
        target: T,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared { mailbox: Mutex::default(), wake: Condvar::new(), finished: Condvar::new() });
        world.device.watch(Stopper(Arc::downgrade(&shared)));
        let job = Job {
            work: Box::new({
                let shared = shared.clone();
                let world = world.clone();
                move || {
                    // Everything the worker owns is dropped inside, on this
                    // thread, before it counts as finished.
                    let result = panic::catch_unwind(AssertUnwindSafe(|| run(&shared, &world, renderer, target)));
                    let failure = match result {
                        Ok(Ok(())) => return,
                        Ok(Err(failure)) => failure,
                        Err(payload) => format!("the render worker panicked: {}", panic_message(&*payload)),
                    };
                    let mut mailbox = shared.lock();
                    // A lost device is the reason, when that's what made it
                    // panic.
                    mailbox.failure.get_or_insert((FyStatus::WorkerFailed, failure));
                    mailbox.pending = None;
                }
            }),
            shared: shared.clone(),
        };
        threads.run(job)?;
        Ok(Self { shared, world })
    }

    /// A scene buffer to fill: a spare one when there is one.
    pub(crate) fn scene(&self) -> RenderScene {
        self.shared.lock().spare.pop().unwrap_or_default()
    }

    /// Takes back a scene buffer from [`Worker::scene`] that won't be
    /// posted.
    pub(crate) fn recycle(&self, scene: RenderScene) {
        self.shared.lock().spare.push(scene);
    }

    /// Hands `scene` (frame `number`) to the worker, and the mesh changes in
    /// `ops` to the world's ledger, for the first of its workers to draw.
    /// `ops` is left empty. Fails only when the worker has stopped
    /// ([`Worker::failure`]); `scene` is then kept for reuse.
    pub(crate) fn post(&self, number: u64, scene: RenderScene, ops: &mut Vec<ResourceOp>) -> Result<(), Error> {
        let mut mailbox = self.shared.lock();
        if let Some((status, message)) = &mailbox.failure {
            let error = Error::new(*status, message.clone());
            mailbox.spare.push(scene);
            return Err(error);
        }
        if !ops.is_empty() {
            self.world.ledger().ops.append(ops);
        }
        mailbox.stats.posted += 1;
        if let Some((_, replaced)) = mailbox.pending.replace((number, scene)) {
            mailbox.spare.push(replaced);
            if mailbox.retried {
                mailbox.stats.skipped += 1;
            } else {
                mailbox.stats.coalesced += 1;
            }
        }
        mailbox.retried = false;
        drop(mailbox);
        self.shared.wake.notify_one();
        Ok(())
    }

    /// Draws the scenes posted from now on at `width` × `height`. Fails
    /// only when the worker has stopped.
    pub(crate) fn resize(&self, width: u32, height: u32) -> Result<(), Error> {
        let mut mailbox = self.shared.lock();
        if let Some((status, message)) = &mailbox.failure {
            return Err(Error::new(*status, message.clone()));
        }
        mailbox.size = Some((width, height));
        Ok(())
    }

    /// For tests: destroys the device, as a driver reset would, which stops
    /// every view on it. It's done on the render thread, between frames.
    pub(crate) fn lose_device(&self) {
        self.shared.lock().lose_device = true;
        self.shared.wake.notify_one();
    }

    /// For tests: makes the worker panic before its next frame, as a bug in
    /// it would.
    pub(crate) fn panic(&self) {
        self.shared.lock().panic = true;
        self.shared.wake.notify_one();
    }

    /// Why the worker stopped, if it did.
    pub(crate) fn failure(&self) -> Option<Error> {
        self.shared.lock().failure.as_ref().map(|(status, message)| Error::new(*status, message.clone()))
    }

    pub(crate) fn stats(&self) -> WorkerStats {
        self.shared.lock().stats
    }

    #[cfg(test)]
    pub(crate) fn spare_scenes(&self) -> usize {
        self.shared.lock().spare.len()
    }
}

impl Drop for Worker {
    /// Stops the worker after the frame it is drawing, if any, and waits
    /// until it has dropped everything it owned (the GPU device, the
    /// target). A scene still waiting is dropped.
    fn drop(&mut self) {
        let mut mailbox = self.shared.lock();
        mailbox.stop = true;
        self.shared.wake.notify_one();
        while !mailbox.finished {
            mailbox = self.shared.finished.wait(mailbox).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// The render threads waiting for a view: process-wide.
static RENDER_THREADS: RenderThreads = RenderThreads::new();

/// Render threads that finished a view and wait for the next one.
#[derive(Debug)]
struct RenderThreads {
    /// How to hand each waiting thread its next job.
    idle: Mutex<Vec<mpsc::Sender<Job>>>,
}

/// A worker's whole life, for a render thread to run.
struct Job {
    work: Box<dyn FnOnce() + Send>,
    shared: Arc<Shared>,
}

impl RenderThreads {
    /// At most this many threads wait. One covers a view replaced by
    /// another; each more would keep its driver state for nothing.
    const MAX_IDLE: usize = 1;

    const fn new() -> Self {
        Self { idle: Mutex::new(Vec::new()) }
    }

    fn idle(&self) -> MutexGuard<'_, Vec<mpsc::Sender<Job>>> {
        self.idle.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `job` on a waiting thread, or on a new one when none waits.
    fn run(&'static self, job: Job) -> std::io::Result<()> {
        let mut job = job;
        while let Some(thread) = self.idle().pop() {
            // A thread that ended (it can't, but a send can fail) gives the
            // job back.
            match thread.send(job) {
                Ok(()) => return Ok(()),
                Err(mpsc::SendError(back)) => job = back,
            }
        }
        let (sender, jobs) = mpsc::channel();
        sender.send(job).expect("the receiver is right here");
        thread::Builder::new().name("fynkel render".into()).spawn(move || self.serve(&sender, &jobs))?;
        Ok(())
    }

    /// A render thread's life: jobs until there is room for no more.
    fn serve(&self, sender: &mpsc::Sender<Job>, jobs: &mpsc::Receiver<Job>) {
        while let Ok(Job { work, shared }) = jobs.recv() {
            // The work catches its own panics; this is only so `finished`
            // is always set.
            let _ = panic::catch_unwind(AssertUnwindSafe(work));
            // Waiting again before the worker counts as finished, so a view
            // made right after this one always finds this thread.
            let wait = {
                let mut idle = self.idle();
                let room = idle.len() < Self::MAX_IDLE;
                if room {
                    idle.push(sender.clone());
                }
                room
            };
            shared.lock().finished = true;
            shared.finished.notify_all();
            if !wait {
                return;
            }
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Mailbox> {
        // The worker's panics are caught outside any lock, so the mailbox
        // is always consistent.
        self.mailbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stops the worker after what it's drawing, for `message`: a GPU
    /// error or a lost device. The first reason stays.
    fn fail(&self, message: String) {
        let mut mailbox = self.lock();
        if mailbox.failure.is_none() {
            mailbox.failure = Some((FyStatus::DeviceLost, message));
        }
        mailbox.pending = None;
        drop(mailbox);
        self.wake.notify_one();
    }
}

/// How many bytes of meshes and textures a frame makes on the GPU, at
/// most: a 2048² texture is 16 MiB. One bigger still goes, alone.
const UPLOAD_BUDGET: u64 = 32 << 20;

/// Moves the oldest of `waiting` into `frame`, up to `budget` bytes of
/// uploads and at least one change; removing costs nothing.
fn take_within(waiting: &mut Vec<ResourceOp>, frame: &mut Vec<ResourceOp>, budget: u64) {
    let mut spent = 0;
    let mut count = 0;
    for op in waiting.iter() {
        let cost = match op {
            ResourceOp::UploadMesh(_, mesh) => {
                (std::mem::size_of_val(mesh.vertices()) + std::mem::size_of_val(mesh.indices())) as u64
            }
            ResourceOp::UploadTexture(_, _, image) => image.pixels().len() as u64,
            ResourceOp::RemoveMesh(_)
            | ResourceOp::RemoveTexture(_)
            | ResourceOp::UploadShader(..)
            | ResourceOp::RemoveShader(_) => 0,
        };
        if count > 0 && spent + cost > budget {
            break;
        }
        spent += cost;
        count += 1;
    }
    frame.extend(waiting.drain(..count));
}

/// A change to the world's assets, made on the GPU and waiting to be put
/// in.
enum Made {
    Mesh(MeshKey, MadeMesh),
    /// A dynamic mesh, written where it is under the assets' lock while
    /// it fits (`Assets::upload_mesh`).
    WriteMesh(MeshKey, Arc<fynkel_world::MeshData>),
    RemoveMesh(MeshKey),
    Texture(TextureKey, MadeTexture),
    RemoveTexture(TextureKey),
    Shader(fynkel_render::ShaderKey, Arc<fynkel_render::CustomShader>),
    RemoveShader(fynkel_render::ShaderKey),
}

/// Draws until stopped. Fails when the target can't take a new size.
fn run<T: Target>(shared: &Arc<Shared>, world: &WorldGpu, mut renderer: Renderer, mut target: T) -> Result<(), String> {
    let gpu = &world.device.gpu;
    let (mut ops, mut made) = (Vec::new(), Vec::new());
    let mut first = true;
    // A scene the target has no room for waits for a frame to free some.
    let waits = target.on_free(Arc::new({
        let shared = Arc::downgrade(shared);
        move || {
            if let Some(shared) = shared.upgrade() {
                let mut mailbox = shared.lock();
                mailbox.freed += 1;
                mailbox.blocked = false;
                drop(mailbox);
                shared.wake.notify_one();
            }
        }
    }));
    loop {
        let (number, scene, size, freed) = {
            let mut mailbox = shared.lock();
            loop {
                if mailbox.stop || mailbox.failure.is_some() {
                    return Ok(());
                }
                if mailbox.panic {
                    mailbox.panic = false;
                    drop(mailbox);
                    panic!("a test made the render worker panic");
                }
                if mailbox.lose_device {
                    mailbox.lose_device = false;
                    drop(mailbox);
                    world.device.destroy();
                    return Ok(());
                }
                if !mailbox.blocked
                    && let Some((number, scene)) = mailbox.pending.take()
                {
                    mailbox.retried = false;
                    break (number, scene, mailbox.size.take(), mailbox.freed);
                }
                mailbox = shared.wake.wait(mailbox).unwrap_or_else(PoisonError::into_inner);
            }
        };

        let started = std::time::Instant::now();
        // One pool per pass, so what Metal autoreleases on this thread is
        // freed every frame and every resize, not when the view goes. A GPU
        // error raised in it stops this view alone (the scopes are this
        // thread's); one raised elsewhere stops every view on the device.
        let drawn = scoped(&gpu.device, || {
            fynkel_present::autorelease_pool(|| -> Result<bool, String> {
                if let Some((width, height)) = size {
                    target
                        .resize(width, height)
                        .map_err(|e| format!("couldn't resize the view to {width} × {height}: {e}"))?;
                }
                // The world's changes, by whichever of its workers comes first,
                // a frame's budget of them (all of them, for the view's first
                // frame, so it starts on the whole scene); the rest wait, in
                // order, for the next frame. They're made on the GPU without the assets held,
                // so the world's other views draw meanwhile (without them, for
                // a frame), then put in together under the assets' lock. One
                // worker at a time makes them, so they go in in order; another
                // doesn't wait for it, but in its view's first frame.
                let uploading = match world.uploading.try_lock() {
                    Ok(guard) => Some(guard),
                    Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
                    Err(TryLockError::WouldBlock) if first => {
                        Some(world.uploading.lock().unwrap_or_else(PoisonError::into_inner))
                    }
                    Err(TryLockError::WouldBlock) => None,
                };
                if let Some(_uploading) = uploading {
                    let budget = if first { u64::MAX } else { UPLOAD_BUDGET };
                    take_within(&mut world.ledger().ops, &mut ops, budget);
                    let kit = &world.device.kit;
                    made.extend(ops.drain(..).map(|op| match op {
                        ResourceOp::UploadMesh(key, mesh) if mesh.is_dynamic() => Made::WriteMesh(key, mesh),
                        ResourceOp::UploadMesh(key, mesh) => Made::Mesh(key, kit.make_mesh(&mesh)),
                        ResourceOp::RemoveMesh(key) => Made::RemoveMesh(key),
                        ResourceOp::UploadTexture(key, space, image) => {
                            Made::Texture(key, kit.make_texture(space, image.width(), image.height(), image.pixels()))
                        }
                        ResourceOp::RemoveTexture(key) => Made::RemoveTexture(key),
                        // Its pipelines are made when it's first drawn.
                        ResourceOp::UploadShader(key, shader) => Made::Shader(key, shader),
                        ResourceOp::RemoveShader(key) => Made::RemoveShader(key),
                    }));
                    let mut assets = fynkel_render::lock_assets(&world.assets);
                    for made in made.drain(..) {
                        match made {
                            Made::Mesh(key, mesh) => assets.insert_mesh(key, mesh),
                            Made::WriteMesh(key, mesh) => assets.upload_mesh(key, &mesh),
                            Made::RemoveMesh(key) => {
                                assets.remove_mesh(key);
                            }
                            Made::Texture(key, texture) => assets.insert_texture(key, texture),
                            Made::RemoveTexture(key) => {
                                assets.remove_texture(key);
                            }
                            Made::Shader(key, shader) => assets.upload_shader(key, shader),
                            Made::RemoveShader(key) => {
                                assets.remove_shader(key);
                            }
                        }
                    }
                    // Before the frame is finished: a reader woken by it must
                    // see the memory its uploads took.
                    let mut mailbox = shared.lock();
                    mailbox.stats.texture_bytes = assets.texture_bytes();
                    mailbox.stats.mesh_bytes = assets.mesh_bytes();
                }

                let drawn = match target.begin() {
                    Some(frame) => {
                        let (width, height) = target.size();
                        let stats = renderer.prepare(&scene, width, height);
                        shared.lock().stats.sky_bytes = stats.sky_bytes;
                        let mut encoder =
                            gpu.device.create_command_encoder(&fynkel_render::wgpu::CommandEncoderDescriptor {
                                label: Some("frame"),
                            });
                        renderer.encode(&mut encoder, target.view(&frame));
                        target.finish(frame, encoder, number);
                        true
                    }
                    None => false,
                };
                Ok(drawn)
            })
        })?;

        let mut mailbox = shared.lock();
        mailbox.stats.busy_micros += started.elapsed().as_micros() as u64;
        if drawn {
            first = false;
            mailbox.spare.push(scene);
            mailbox.stats.rendered += 1;
        } else if waits && mailbox.pending.is_none() {
            // Drawn when a frame frees room (unless a newer scene comes
            // meanwhile): at once if one already has.
            mailbox.pending = Some((number, scene));
            mailbox.retried = true;
            mailbox.blocked = mailbox.freed == freed;
        } else {
            mailbox.spare.push(scene);
            mailbox.stats.skipped += 1;
        }
    }
}

/// Runs `body` inside error scopes on this thread: a validation or
/// out-of-memory error it raises fails it, instead of reaching the
/// device's handler.
fn scoped<R>(device: &wgpu::Device, body: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = body();
    let error = now(validation.pop()).or_else(|| now(memory.pop()));
    match error {
        Some(error) => Err(format!("the GPU reported an error: {error}")),
        None => result,
    }
}

/// A scope's error: on native backends it's ready at once.
fn now(pop: impl Future<Output = Option<wgpu::Error>>) -> Option<wgpu::Error> {
    let mut pop = std::pin::pin!(pop);
    match pop.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(error) => error,
        std::task::Poll::Pending => None,
    }
}

pub(crate) use fynkel_cabi::panic_message;

#[cfg(test)]
mod tests;
