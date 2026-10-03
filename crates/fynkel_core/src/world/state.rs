//! The world's subsystem states (foundation 6.1): what one part of the
//! tick keeps between ticks, grouped so `World` reads as its parts.

use std::collections::HashSet;

use fynkel_contracts::Entity;
use glam::Mat4;

use super::{animation, catalogue, controller, RigPlacement};
use crate::animation::Pose;
use crate::components::Camera;

/// What animators play (M3-19) and their scratch.
#[derive(Default)]
#[cfg_attr(not(feature = "animation"), allow(dead_code))]
pub(super) struct AnimationState {
    pub(super) binding_reports: HashSet<(Entity, crate::ComponentId, crate::AssetRef, String)>,
    pub(super) binding_cursor: Option<fynkel_ecs::ChangeCursor>,
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
    pub(super) lifetimes: Vec<(Entity, crate::LifetimeDespawn)>,
    pub(super) emissions: Vec<catalogue::Emission>,
    pub(super) placements: Vec<catalogue::Placement>,
}

/// Membership is derived from ECS deltas; published Extract values remain owned.
#[derive(Default)]
pub(super) struct RenderMembership {
    cursor: Option<fynkel_ecs::ChangeCursor>,
    pub(super) entities: std::collections::BTreeSet<Entity>,
    pub(super) examined: usize,
    pub(super) renderable: std::collections::BTreeSet<Entity>,
    pub(super) memo: std::collections::HashMap<Entity, glam::Mat4>,
}
impl RenderMembership {
    pub(super) fn refresh(&mut self, ecs: &fynkel_ecs::World) {
        // Membership depends on component presence, not on their values. Pose,
        // visibility and light changes are read fresh during extraction; watching
        // them here needlessly rebuilds membership for moving scenes.
        let changes = ecs
            .changes_since(self.cursor, &[], true)
            .expect("settled render storage");
        let candidates = if changes.reset {
            self.entities.clear();
            self.renderable.clear();
            ecs.entities().collect()
        } else {
            changes.entities
        };
        self.examined = candidates.len();
        for e in candidates {
            if ecs.has::<fynkel_scene::WorldTransform>(e) && !ecs.has::<fynkel_scene::Inactive>(e) {
                self.entities.insert(e);
                if ecs.has::<crate::MeshRenderer>(e)
                    || ecs.has::<crate::Camera>(e)
                    || ecs.has::<crate::DirectionalLight>(e)
                    || ecs.has::<crate::PointLight>(e)
                    || ecs.has::<crate::SpotLight>(e)
                    || ecs.has::<crate::Environment>(e)
                    || ecs.has::<crate::Volume>(e)
                {
                    self.renderable.insert(e);
                } else {
                    self.renderable.remove(&e);
                }
            } else {
                self.entities.remove(&e);
                self.renderable.remove(&e);
            }
        }
        self.cursor = Some(changes.cursor);
    }
}

/// The sole authority for gameplay component storage and transaction epochs.
/// No scene, bridge or resource owner keeps a second component store.
pub(crate) struct GameplayState {
    pub(crate) ecs: fynkel_ecs::World,
    pub(crate) registered: crate::registry::ComponentRegistry,
    pub(crate) next_gameplay_edit: u64,
    pub(super) gameplay_effects: Vec<super::gameplay::CommittedEffect>,
    pub(crate) gameplay_tick_prepared: bool,
}
impl GameplayState {
    pub(crate) fn new(ecs: fynkel_ecs::World) -> Self {
        Self {
            ecs,
            registered: Default::default(),
            next_gameplay_edit: 0,
            gameplay_effects: Vec::new(),
            gameplay_tick_prepared: false,
        }
    }
}
