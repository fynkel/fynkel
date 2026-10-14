//! The process's GPU device, shared by every view (architecture R13), and
//! each world's meshes and textures on it, shared by the world's views.
//!
//! The device is made by the first view and kept after the last one goes,
//! so the next view (a hot restart's, a second one beside it) starts at
//! once and compiles no shaders. When it's lost, or reports an error
//! outside a view's frame, every view on it stops with
//! `FY_STATUS_DEVICE_LOST`; the next view made gets a new device, and each
//! world uploads its meshes and textures again. An error in a view's frame
//! stops that view alone (`FY_STATUS_WORKER_FAILED`, the worker's scopes).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use fynkel_render::{Assets, Gpu, GpuError, Kit, SharedAssets};

use crate::extract::Ledger;
use crate::worker::Stopper;

/// A GPU device and what every renderer on it shares.
#[derive(Debug)]
pub(crate) struct Device {
    pub(crate) gpu: Gpu,
    pub(crate) kit: Arc<Kit>,
    lost: AtomicBool,
    /// The render workers drawing on it, to stop when it's lost.
    workers: Mutex<Vec<Stopper>>,
}

/// The device views are made on, until it's lost.
static CURRENT: Mutex<Option<Arc<Device>>> = Mutex::new(None);

impl Device {
    /// The process's device: the one there is, or a new one when there's
    /// none or it was lost.
    pub(crate) fn current() -> Result<Arc<Self>, GpuError> {
        let mut current = CURRENT.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(device) = current.as_ref().filter(|d| !d.is_lost()) {
            return Ok(device.clone());
        }
        let device = Self::new(Gpu::headless()?);
        *current = Some(device.clone());
        Ok(device)
    }

    /// A device of its own on `gpu`, for tests: not the process's.
    pub(crate) fn new(gpu: Gpu) -> Arc<Self> {
        let device =
            Arc::new(Self { kit: Kit::new(&gpu), gpu, lost: AtomicBool::new(false), workers: Mutex::default() });
        // wgpu's own handlers panic, on whichever thread calls next. These
        // stop every view on the device instead, with a status the views
        // act on: a new view has a new device, and uploads everything again.
        let weak = Arc::downgrade(&device);
        device.gpu.device.on_uncaptured_error(Arc::new({
            let weak = weak.clone();
            move |error| lose(&weak, format!("the GPU reported an error: {error}"))
        }));
        // Dropping the device doesn't call it, and nothing here destroys it
        // but a test.
        device.gpu.device.set_device_lost_callback(move |_reason, message| {
            lose(
                &weak,
                if message.is_empty() {
                    "the GPU device was lost".to_owned()
                } else {
                    format!("the GPU device was lost: {message}")
                },
            )
        });
        device
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Stops `worker` when the device is lost: at once if it is already.
    pub(crate) fn watch(&self, worker: Stopper) {
        let mut workers = self.workers();
        workers.retain(Stopper::is_alive);
        if self.is_lost() {
            worker.stop("the GPU device was lost".to_owned());
        }
        workers.push(worker);
    }

    /// Marks the device lost and stops every view on it, for `message`.
    pub(crate) fn lose(&self, message: String) {
        self.lost.store(true, Ordering::Release);
        for worker in self.workers().drain(..) {
            worker.stop(message.clone());
        }
    }

    /// For tests: destroys the device, as a driver reset would.
    pub(crate) fn destroy(&self) {
        self.gpu.device.destroy();
        // wgpu calls the device-lost callback only once the device is
        // polled with nothing left to do; stop now, as a real loss does
        // before the next frame.
        self.lose("the GPU device was lost".to_owned());
    }

    fn workers(&self) -> MutexGuard<'_, Vec<Stopper>> {
        self.workers.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn lose(device: &Weak<Device>, message: String) {
    if let Some(device) = device.upgrade() {
        device.lose(message);
    }
}

/// One world's meshes and textures on a device, shared by its views, and
/// the changes to them still to be made.
#[derive(Debug)]
pub(crate) struct WorldGpu {
    pub(crate) device: Arc<Device>,
    pub(crate) assets: SharedAssets,
    /// What the GPU has or is about to have: each view's extract adds to
    /// it, and the first render worker to draw after makes the changes.
    pub(crate) ledger: Mutex<Ledger>,
    /// Held by the render worker making the ledger's changes.
    pub(crate) uploading: Mutex<()>,
}

impl WorldGpu {
    pub(crate) fn new(device: &Arc<Device>) -> Arc<Self> {
        Arc::new(Self {
            device: device.clone(),
            assets: Assets::shared(&device.kit),
            ledger: Mutex::default(),
            uploading: Mutex::default(),
        })
    }

    /// Shares `assets` (made on `device`'s kit) rather than new ones.
    #[cfg(test)]
    pub(crate) fn with_assets(device: &Arc<Device>, assets: SharedAssets) -> Arc<Self> {
        Arc::new(Self { device: device.clone(), assets, ledger: Mutex::default(), uploading: Mutex::default() })
    }

    pub(crate) fn ledger(&self) -> MutexGuard<'_, Ledger> {
        // Changed only by plain pushes and swaps, so whole after a panic.
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
