//! The handles (`FyWorld`, `FyView`) and the helpers that read and write
//! through pointers.

use std::sync::{Arc, Mutex, MutexGuard, Weak};

use fynkel_present::HeadlessFrames;
use fynkel_render::{Fog, Quality, Sky};
use fynkel_world::{
    AnimationEvent, MaterialId, MeshId, ObjectId, ParticleEvent, Raycaster, ShaderId, TextureId, Vec3, World,
};

use crate::device::WorldGpu;
use crate::error::{Error, FyStatus};
use crate::extract::Extract;
use crate::worker::Worker;

// --- Handles -----------------------------------------------------------------

/// A world. Opaque to C.
#[derive(Debug)]
pub struct FyWorld {
    pub(super) world: Mutex<World>,
    /// Kept between `fy_raycast` calls so they don't allocate. Locked
    /// only while `world` is.
    pub(super) raycaster: Mutex<Raycaster>,
    /// Its meshes and textures on the GPU, shared by its views: on the
    /// device its views draw on, made by the first. Held by the views, so
    /// they're freed with the last one, as a view's own were.
    pub(super) gpu: Mutex<Weak<WorldGpu>>,
    /// The camera shake: which camera, and how far it's moved when drawn.
    pub(super) shake: Mutex<Option<(ObjectId, Vec3)>>,
    /// The quality its views draw at, or `None` for the device's level's.
    pub(super) quality: Mutex<Option<Quality>>,
    /// Its sky and fog, as its views draw them.
    pub(super) sky: Mutex<Option<Sky>>,
    pub(super) fog: Mutex<Option<Fog>>,
    /// What the last `fy_world_animate` reported, for
    /// `fy_animation_events_get`. Locked only while `world` is.
    pub(super) events: Mutex<Vec<AnimationEvent>>,
    /// What the last `fy_world_particles_step` reported, for
    /// `fy_particle_events_get`. Locked only while `world` is.
    pub(super) particle_events: Mutex<Vec<ParticleEvent>>,
}

impl FyWorld {
    /// The world, unless an earlier call panicked while changing it.
    pub(super) fn lock(&self) -> Result<MutexGuard<'_, World>, Error> {
        self.world.lock().map_err(|_| {
            Error::new(
                FyStatus::Poisoned,
                "an earlier call panicked while changing this world; it can't be used any more",
            )
        })
    }
}

/// A view: a world drawn into a target by its own render worker, on the
/// process's GPU device, with the meshes and textures the world's other
/// views have too. Opaque to C.
#[derive(Debug)]
pub struct FyView {
    pub(super) world: Arc<FyWorld>,
    pub(super) gpu: Arc<WorldGpu>,
    /// The frame clock's side: the extract and the frame numbers.
    pub(super) clock: Mutex<Clock>,
    pub(super) worker: Worker,
    pub(super) output: Output,
}

/// Where a view's frames can be had.
#[derive(Debug)]
pub(super) enum Output {
    Headless(HeadlessFrames),
    #[cfg(target_vendor = "apple")]
    Apple(fynkel_present::ApplePresenter),
    #[cfg(target_vendor = "apple")]
    AppleLayer(fynkel_present::LayerPresenter),
    #[cfg(target_os = "android")]
    Android(fynkel_present::AndroidPresenter),
}

impl FyView {
    /// The platform side of an Android view, for the plugin's JNI calls.
    #[cfg(target_os = "android")]
    pub(crate) fn android(&self) -> Option<&fynkel_present::AndroidPresenter> {
        match &self.output {
            Output::Android(presenter) => Some(presenter),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Clock {
    pub(super) extract: Extract,
    pub(super) last_number: u64,
}

/// `p` as a reference, or an error when it's null.
///
/// # Safety
///
/// `p` is null or points at a live `T`.
pub(super) unsafe fn handle<'a, T>(p: *const T, what: &str) -> Result<&'a T, Error> {
    // SAFETY: the caller promises p is null or live.
    unsafe { p.as_ref() }.ok_or_else(|| Error::null(what))
}

/// Writes `value` through `out`, or fails when it's null.
///
/// # Safety
///
/// `out` is null or valid for a write of `T`.
pub(super) unsafe fn write<T>(out: *mut T, value: T, what: &str) -> Result<(), Error> {
    if out.is_null() {
        return Err(Error::null(what));
    }
    // SAFETY: not null, and the caller promises it's valid for writes.
    unsafe { out.write(value) };
    Ok(())
}

/// Reads `*p`, or fails when it's null.
///
/// # Safety
///
/// `p` is null or valid for a read of `T`.
pub(super) unsafe fn read<T: Copy>(p: *const T, what: &str) -> Result<T, Error> {
    // SAFETY: the caller promises p is null or valid.
    unsafe { p.as_ref() }.copied().ok_or_else(|| Error::null(what))
}

pub(super) fn object(id: u64) -> Result<ObjectId, Error> {
    ObjectId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoObject, "0 is not an object id"))
}

pub(super) fn mesh(id: u64) -> Result<MeshId, Error> {
    MeshId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoMesh, "0 is not a mesh id"))
}

pub(super) fn material(id: u64) -> Result<MaterialId, Error> {
    MaterialId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoMaterial, "0 is not a material id"))
}

pub(super) fn texture(id: u64) -> Result<TextureId, Error> {
    TextureId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoTexture, "0 is not a texture id"))
}

pub(super) fn shader(id: u64) -> Result<ShaderId, Error> {
    ShaderId::from_raw(id).ok_or_else(|| Error::new(FyStatus::NoShader, "0 is not a shader id"))
}
