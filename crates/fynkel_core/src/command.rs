//! Commands are the only way gameplay mutates the world (§5.4).

use fynkel_audio::AudioBus;
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
    /// Drives a `fynkel.AudioSource` (M3-11): play, stop, pause, resume,
    /// re-volume or re-pitch its voice. Applied at the tick boundary in
    /// batch order (a source set in the same batch receives it); the audio
    /// system then reconciles the component this tick. Simulation lane
    /// only; `MISSING_COMPONENT` without a source.
    Audio {
        entity: Entity,
        op: AudioOp,
    },
    /// Plays a clip once, on no entity (M3-11): a fire-and-forget sound
    /// with no `PLAYBACK_ENDED`. `SOUND_UNAVAILABLE` for a clip the world
    /// has not been given; refused silently under the voice limit. Either
    /// lane.
    PlaySound {
        clip: AssetRef,
        bus: AudioBus,
        volume: f32,
        pitch: f32,
        priority: i32,
    },
    /// Sets a bus gain and the mute (M3-11); `master` scales the other
    /// two. What the `fynkel.Settings` volumes are applied through. Either
    /// lane.
    AudioBus {
        bus: AudioBus,
        volume: f32,
        muted: bool,
    },
}

/// What `Command::Audio` does to a source's voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioOp {
    /// Start the source's clip from the beginning (restarting a playing
    /// voice); sets `playing`.
    Play,
    /// Stop the voice; clears `playing`. No `PLAYBACK_ENDED`.
    Stop,
    /// Hold the voice where it is (`playing` stays set).
    Pause,
    Resume,
    /// Set the component's `volume` (`0..=1`) and the voice's.
    SetVolume(f32),
    /// Set the component's `pitch` (`MIN_PITCH..=MAX_PITCH`) and the
    /// voice's.
    SetPitch(f32),
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
    /// Set the layer weight (M3-19c), within `[0, 1]`.
    SetWeight(f32),
    /// Set a blend-space parameter by name (M3-19c); an animator keeps its
    /// parameters across clips, a blend space reads the one it names.
    SetParameter {
        name: String,
        value: f32,
    },
}

impl Command {
    /// Presentation lane accepts plain-data writes only (§5.1 table).
    pub fn allowed_on_presentation(&self) -> bool {
        matches!(
            self,
            Command::SetTransform { .. }
                | Command::SetName { .. }
                | Command::SetComponent { .. }
                | Command::PlaySound { .. }
                | Command::AudioBus { .. }
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
            | Command::Animate { entity, .. }
            | Command::Audio { entity, .. } => Some(*entity),
            Command::SetParent { child, .. } => Some(*child),
            Command::PlaySound { .. } | Command::AudioBus { .. } => None,
        }
    }
}
