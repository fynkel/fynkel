//! The world's subsystem states (foundation 6.1): what one part of the
//! tick keeps between ticks, grouped so `World` reads as its parts.

use std::collections::{HashMap, HashSet};

use fynkel_contracts::Entity;
use fynkel_scene::Transform;
use glam::Mat4;

use super::{animation, catalogue, controller, Rejected, RigPlacement};
use crate::animation::Pose;
use crate::command::Command;
use crate::components::Camera;

/// The command lanes between ticks.
#[derive(Default)]
pub(super) struct Lanes {
    /// Simulation commands for the next tick with their batch index and
    /// whether they were staged by the presentation lane. An `Err` is a
    /// command the caller could not decode: it is reported as rejected at
    /// the tick boundary so its diagnostic sits with the batch's others.
    pub(super) staged: Vec<(u32, Result<Command, Rejected>, bool)>,
    /// The tick's spawns by temp id, what a temp reference
    /// (`Entity::temp`) in a later command of the same tick resolves to.
    pub(super) spawned_this_tick: HashMap<u64, Entity>,
    /// Entities whose next tick must not interpolate from S[n] (their
    /// authoritative value came from a presentation override already shown).
    pub(super) teleported: Vec<Entity>,
    /// Presentation-lane transform overrides for the current frame.
    pub(super) overrides: HashMap<Entity, Transform>,
}

/// The derived `Inactive` markers' staleness.
#[derive(Default)]
pub(super) struct ActiveMarkers {
    /// The subtrees whose markers a spawn, reparent or active flag made
    /// stale since the last refresh: each is refreshed alone (foundation
    /// 3.4).
    pub(super) roots: Vec<Entity>,
    /// Every marker may be stale (a load): the next refresh walks the
    /// whole hierarchy.
    pub(super) all: bool,
}

/// What animators play (M3-19) and their scratch.
#[derive(Default)]
#[cfg_attr(not(feature = "animation"), allow(dead_code))]
pub(super) struct AnimationState {
    /// Clips `Animator`s may play and skins `Skin`s deform with.
    pub(super) clips: animation::ClipTable,
    /// Blend spaces an `Animator` may play in a clip's place (M3-19c).
    pub(super) blends: animation::BlendTable,
    /// Animator Controllers an `Animator` may run (ADR-0032).
    pub(super) controllers: controller::ControllerTable,
    pub(super) skins: animation::SkinTable,
    pub(super) animators: Vec<animation::AnimatorTick>,
    /// The animators of a tick, (depth, entity), ancestor-first.
    pub(super) order: Vec<(u32, Entity)>,
    pub(super) pose: Pose,
    pub(super) fade_pose: Pose,
    pub(super) layer_pose: Pose,
}

/// Edit mode only (M4-06).
#[derive(Default)]
pub(super) struct EditState {
    /// The editor camera `extract()` reports in the scene camera's place —
    /// its world matrix and the camera component it renders with.
    pub(super) camera: Option<(Mat4, Camera)>,
    /// Entities `extract()` leaves out (their meshes, lights and camera) —
    /// the editor's hidden flag, never the document's.
    pub(super) hidden: HashSet<Entity>,
    /// [`World::set_edit_animation`](super::World::set_edit_animation).
    pub(super) animation: bool,
}

/// Scratch of the native systems, kept for its capacity.
#[derive(Default)]
pub(super) struct Scratch {
    pub(super) rigs: Vec<RigPlacement>,
    pub(super) driver: Vec<(Entity, fynkel_physics::BodyCommand)>,
    pub(super) hits: Vec<fynkel_physics::Hit>,
    pub(super) entities: Vec<Entity>,
    pub(super) emissions: Vec<catalogue::Emission>,
    pub(super) placements: Vec<catalogue::Placement>,
}
