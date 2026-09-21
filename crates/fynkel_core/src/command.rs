//! Commands are the only way gameplay mutates the world (§5.4).

use fynkel_contracts::{AssetRef, ComponentId, Entity};
use fynkel_physics::BodyCommand;
use fynkel_scene::Transform;

use crate::components::ComponentValue;

/// Which frame phase a command batch belongs to (ADR-0008).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Lane {
    /// Applied at the next fixed tick boundary.
    Simulation = 0,
    /// Applied before the current frame's extraction; transforms also become
    /// the staged authoritative value for the next tick.
    Presentation = 1,
}

impl Lane {
    pub fn from_u32(v: u32) -> Option<Lane> {
        match v {
            0 => Some(Lane::Simulation),
            1 => Some(Lane::Presentation),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `temp_id` (non-zero, unique within the batch) is echoed in the
    /// `Spawned` event with the real handle.
    Spawn {
        temp_id: u64,
        transform: Transform,
        parent: Option<Entity>,
        name: Option<String>,
    },
    /// Destroys the entity and its subtree.
    Destroy(Entity),
    SetTransform {
        entity: Entity,
        transform: Transform,
    },
    SetParent {
        child: Entity,
        parent: Option<Entity>,
        keep_world: bool,
    },
    SetName {
        entity: Entity,
        name: String,
    },
    /// Adds or replaces one built-in component. A `Transform` value behaves
    /// exactly like `SetTransform`. Scene loading uses this same path.
    SetComponent {
        entity: Entity,
        value: ComponentValue,
    },
    /// Removes a built-in component; `Transform`, hierarchy and `Name` are
    /// not removable (`INVALID_PAYLOAD`).
    RemoveComponent {
        entity: Entity,
        component: ComponentId,
    },
    /// Moves a `RigidBody` (M3-08): forces, impulses, velocities, a
    /// kinematic target or a teleport. Applied by the physics step of this
    /// tick, after the batch's component writes (a body set in the same
    /// batch receives it). Simulation lane only.
    Body {
        entity: Entity,
        op: BodyCommand,
    },
    /// Drives a `fynkel.Animator` (M3-19): play, crossfade, stop, pause,
    /// resume, re-speed or seek. Applied at the tick boundary in batch
    /// order (an animator set in the same batch receives it); the
    /// animator system then samples the result this tick. Simulation lane
    /// only; `MISSING_COMPONENT` without an animator.
    Animate {
        entity: Entity,
        op: AnimateOp,
    },
}

/// What `Command::Animate` does to an animator.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimateOp {
    /// Switch to `clip` from its start (or its end when `speed < 0`) and
    /// play.
    Play {
        clip: AssetRef,
        looping: bool,
        speed: f32,
    },
    /// Like `Play`, but the clip playing so far keeps advancing and its
    /// pose is blended out over `seconds` (`0` is a plain `Play`).
    Crossfade {
        clip: AssetRef,
        looping: bool,
        speed: f32,
        seconds: f32,
    },
    /// Rewind and clear `playing`; the entities keep their last pose.
    Stop,
    /// Clear `playing`, keeping the time.
    Pause,
    /// Set `playing`.
    Resume,
    SetSpeed(f32),
    /// Seek to `seconds` (clamped to the clip).
    SetTime(f32),
}

impl Command {
    /// Presentation lane accepts plain-data writes only (§5.1 table).
    pub fn allowed_on_presentation(&self) -> bool {
        matches!(
            self,
            Command::SetTransform { .. } | Command::SetName { .. } | Command::SetComponent { .. }
        )
    }

    pub fn target(&self) -> Option<Entity> {
        match self {
            Command::Spawn { .. } => None,
            Command::Destroy(e) => Some(*e),
            Command::SetTransform { entity, .. }
            | Command::SetName { entity, .. }
            | Command::SetComponent { entity, .. }
            | Command::RemoveComponent { entity, .. }
            | Command::Body { entity, .. }
            | Command::Animate { entity, .. } => Some(*entity),
            Command::SetParent { child, .. } => Some(*child),
        }
    }
}
