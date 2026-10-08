//! The render worker (stage 1 plan §3.5): one thread that owns the
//! renderer and the target, and draws the scenes the frame clock posts.
//!
//! Posting never waits for the GPU. Scenes go through a mailbox where the
//! newest wins: a scene posted while another is still waiting replaces it
//! (it is *coalesced*), so a slow GPU means fewer, fresher frames rather
//! than a growing queue. Mesh and texture changes are not coalesced; they queue up in
//! order and are applied before the next scene is drawn.
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

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;

use fynkel_present::Target;
use fynkel_render::{Gpu, RenderScene, Renderer};

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
    /// GPU memory the renderer's textures and meshes hold, as of the last
    /// scene taken.
    pub(crate) texture_bytes: u64,
    pub(crate) mesh_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct Worker {
    shared: Arc<Shared>,
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
    ops: Vec<ResourceOp>,
    /// A new size for the target, applied before the next scene is drawn.
    size: Option<(u32, u32)>,
    stop: bool,
    /// The worker stopped and dropped everything it owned.
    finished: bool,
    /// Why the worker stopped, if it panicked.
    failure: Option<String>,
    stats: WorkerStats,
}

impl Worker {
    /// Starts the worker on a render thread, which owns `gpu`, `renderer`
    /// and `target` from now on.
    pub(crate) fn start<T: Target + 'static>(gpu: Gpu, renderer: Renderer, target: T) -> std::io::Result<Self> {
        Self::start_on(&RENDER_THREADS, gpu, renderer, target)
    }

    /// [`Worker::start`], on a thread of `threads`.
    fn start_on<T: Target + 'static>(
        threads: &'static RenderThreads,
        gpu: Gpu,
        renderer: Renderer,
        target: T,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared { mailbox: Mutex::default(), wake: Condvar::new(), finished: Condvar::new() });
        let job = Job {
            work: Box::new({
                let shared = shared.clone();
                move || {
                    // Everything the worker owns is dropped inside, on this
                    // thread, before it counts as finished.
                    let result = panic::catch_unwind(AssertUnwindSafe(|| run(&shared, gpu, renderer, target)));
                    let failure = match result {
                        Ok(Ok(())) => return,
                        Ok(Err(failure)) => failure,
                        Err(payload) => format!("the render worker panicked: {}", panic_message(&*payload)),
                    };
                    let mut mailbox = shared.lock();
                    mailbox.failure = Some(failure);
                    mailbox.pending = None;
                }
            }),
            shared: shared.clone(),
        };
        threads.run(job)?;
        Ok(Self { shared })
    }

    /// A scene buffer to fill: a spare one when there is one.
    pub(crate) fn scene(&self) -> RenderScene {
        self.shared.lock().spare.pop().unwrap_or_default()
    }

    /// Hands `scene` (frame `number`) and the mesh changes in `ops` to the
    /// worker. `ops` is left empty. Fails only when the worker has stopped
    /// after a panic.
    pub(crate) fn post(&self, number: u64, scene: RenderScene, ops: &mut Vec<ResourceOp>) -> Result<(), String> {
        let mut mailbox = self.shared.lock();
        if let Some(failure) = &mailbox.failure {
            return Err(failure.clone());
        }
        mailbox.ops.append(ops);
        mailbox.stats.posted += 1;
        if let Some((_, replaced)) = mailbox.pending.replace((number, scene)) {
            mailbox.spare.push(replaced);
            mailbox.stats.coalesced += 1;
        }
        drop(mailbox);
        self.shared.wake.notify_one();
        Ok(())
    }

    /// Draws the scenes posted from now on at `width` × `height`. Fails
    /// only when the worker has stopped.
    pub(crate) fn resize(&self, width: u32, height: u32) -> Result<(), String> {
        let mut mailbox = self.shared.lock();
        if let Some(failure) = &mailbox.failure {
            return Err(failure.clone());
        }
        mailbox.size = Some((width, height));
        Ok(())
    }

    /// Why the worker stopped, if it did.
    pub(crate) fn failure(&self) -> Option<String> {
        self.shared.lock().failure.clone()
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
}

/// Draws until stopped. Fails when the target can't take a new size.
fn run<T: Target>(shared: &Shared, gpu: Gpu, mut renderer: Renderer, mut target: T) -> Result<(), String> {
    let mut ops = Vec::new();
    loop {
        let (number, scene, size) = {
            let mut mailbox = shared.lock();
            loop {
                if mailbox.stop {
                    return Ok(());
                }
                if let Some((number, scene)) = mailbox.pending.take() {
                    std::mem::swap(&mut mailbox.ops, &mut ops);
                    break (number, scene, mailbox.size.take());
                }
                mailbox = shared.wake.wait(mailbox).unwrap_or_else(PoisonError::into_inner);
            }
        };

        // One pool per pass, so what Metal autoreleases on this thread is
        // freed every frame and every resize, not when the view goes.
        let drawn = fynkel_present::autorelease_pool(|| -> Result<bool, String> {
            if let Some((width, height)) = size {
                target
                    .resize(width, height)
                    .map_err(|e| format!("couldn't resize the view to {width} × {height}: {e}"))?;
            }
            for op in ops.drain(..) {
                match op {
                    ResourceOp::UploadMesh(key, mesh) => renderer.upload_mesh(key, &mesh),
                    ResourceOp::RemoveMesh(key) => {
                        renderer.remove_mesh(key);
                    }
                    ResourceOp::UploadTexture(key, space, image) => {
                        renderer.upload_texture(key, space, image.width(), image.height(), image.pixels());
                    }
                    ResourceOp::RemoveTexture(key) => {
                        renderer.remove_texture(key);
                    }
                }
            }

            let drawn = match target.begin() {
                Some(frame) => {
                    let (width, height) = target.size();
                    renderer.prepare(&scene, width, height);
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
        })?;

        let mut mailbox = shared.lock();
        mailbox.stats.texture_bytes = renderer.texture_bytes();
        mailbox.stats.mesh_bytes = renderer.mesh_bytes();
        mailbox.spare.push(scene);
        if drawn {
            mailbox.stats.rendered += 1;
        } else {
            mailbox.stats.skipped += 1;
        }
    }
}

pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)")
}

#[cfg(test)]
mod tests;
