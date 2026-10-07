//! The render worker (stage 1 plan §3.5): one thread that owns the
//! renderer and the target, and draws the scenes the frame clock posts.
//!
//! Posting never waits for the GPU. Scenes go through a mailbox where the
//! newest wins: a scene posted while another is still waiting replaces it
//! (it is *coalesced*), so a slow GPU means fewer, fresher frames rather
//! than a growing queue. Mesh changes are not coalesced; they queue up in
//! order and are applied before the next scene is drawn.
//!
//! Scene buffers circulate between the poster and the worker, so a steady
//! stream of frames allocates nothing on either side. There are never more
//! than three: one being filled, one waiting and one being drawn.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use fynkel_present::Target;
use fynkel_render::{Gpu, MeshData, RenderScene, Renderer};
use fynkel_world::MeshSource;

use crate::extract::MeshOp;

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
}

#[derive(Debug)]
pub(crate) struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct Mailbox {
    pending: Option<(u64, RenderScene)>,
    /// Scene buffers ready for reuse.
    spare: Vec<RenderScene>,
    ops: Vec<MeshOp>,
    stop: bool,
    /// Why the worker stopped, if it panicked.
    failure: Option<String>,
    stats: WorkerStats,
}

impl Worker {
    /// Starts the worker thread, which owns `renderer` and `target` from
    /// now on.
    pub(crate) fn start<T: Target + 'static>(gpu: Gpu, renderer: Renderer, target: T) -> std::io::Result<Self> {
        let shared = Arc::new(Shared { mailbox: Mutex::default(), wake: Condvar::new() });
        let thread = thread::Builder::new().name("fynkel render".into()).spawn({
            let shared = shared.clone();
            move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| run(&shared, &gpu, renderer, target)));
                if let Err(payload) = result {
                    let mut mailbox = shared.lock();
                    mailbox.failure = Some(format!("the render worker panicked: {}", panic_message(&*payload)));
                    mailbox.pending = None;
                }
            }
        })?;
        Ok(Self { shared, thread: Some(thread) })
    }

    /// A scene buffer to fill: a spare one when there is one.
    pub(crate) fn scene(&self) -> RenderScene {
        self.shared.lock().spare.pop().unwrap_or_default()
    }

    /// Hands `scene` (frame `number`) and the mesh changes in `ops` to the
    /// worker. `ops` is left empty. Fails only when the worker has stopped
    /// after a panic.
    pub(crate) fn post(&self, number: u64, scene: RenderScene, ops: &mut Vec<MeshOp>) -> Result<(), String> {
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

    /// Why the worker stopped, if it did.
    pub(crate) fn failure(&self) -> Option<String> {
        self.shared.lock().failure.clone()
    }

    #[cfg(test)]
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
    /// for it. A scene still waiting is dropped.
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            // A panic was already caught and recorded inside the thread.
            let _ = thread.join();
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

fn run<T: Target>(shared: &Shared, gpu: &Gpu, mut renderer: Renderer, mut target: T) {
    let mut ops = Vec::new();
    loop {
        let (number, scene) = {
            let mut mailbox = shared.lock();
            loop {
                if mailbox.stop {
                    return;
                }
                if let Some(posted) = mailbox.pending.take() {
                    std::mem::swap(&mut mailbox.ops, &mut ops);
                    break posted;
                }
                mailbox = shared.wake.wait(mailbox).unwrap_or_else(PoisonError::into_inner);
            }
        };

        for op in ops.drain(..) {
            match op {
                MeshOp::Upload(key, source) => renderer.upload_mesh(key, &mesh_data(source)),
                MeshOp::Remove(key) => {
                    renderer.remove_mesh(key);
                }
            }
        }

        let drawn = match target.begin() {
            Some(frame) => {
                let (width, height) = target.size();
                renderer.prepare(&scene, width, height);
                let mut encoder = gpu
                    .device
                    .create_command_encoder(&fynkel_render::wgpu::CommandEncoderDescriptor { label: Some("frame") });
                renderer.encode(&mut encoder, target.view(&frame));
                target.finish(frame, encoder, number);
                true
            }
            None => false,
        };

        let mut mailbox = shared.lock();
        mailbox.spare.push(scene);
        if drawn {
            mailbox.stats.rendered += 1;
        } else {
            mailbox.stats.skipped += 1;
        }
    }
}

/// The built-in shapes. The sphere's 48 × 24 matches the golden images.
fn mesh_data(source: MeshSource) -> MeshData {
    match source {
        MeshSource::Cube => MeshData::cube(),
        MeshSource::Sphere => MeshData::sphere(48, 24),
        MeshSource::Plane => MeshData::plane(),
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
