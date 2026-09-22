//! Wire record definitions and the schema registry (ADR-0012).
//!
//! Every message that crosses the bridge is a `#[derive(Schema)]` struct
//! here or a component elsewhere; [`ALL`] is the registry `fynkel codegen`
//! reads. The wire *framing* (buffer headers, per-record kind tags) is the
//! ABI's job (`fynkel_native`); these are the record bodies.
//!
//! Ids: commands use [`CommandKind`] values, events use `EventKind` values,
//! components use `ComponentId` values. Append only; never renumber.

use fynkel_audio::{AudioBus, AudioListener, AudioSource};
use fynkel_contracts::schema::{fingerprint, RecordSchema};
use fynkel_contracts::{AssetRef, Entity, HasSchema};
use fynkel_physics::{CharacterController, Collider, ColliderShape, RigidBody};
use fynkel_scene::Transform;
use fynkel_schema_derive::Schema;
use glam::{Quat, Vec3};

use crate::components::{
    Animator, Camera, DirectionalLight, Emitter, Environment, Follow, LifetimeDespawn,
    LookAtCamera, MeshRenderer, Oscillate, PointLight, Rotator, Skin, SpotLight, ThirdPersonCamera,
    TriggerZone, Volume,
};
use crate::material::PbrMaterial;
use crate::settings::Settings;
use crate::world::Name;

/// Stable command ids (wire `kind` of a command record).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CommandKind {
    Spawn = 1,
    Destroy = 2,
    SetTransform = 3,
    SetParent = 4,
    SetName = 5,
    SetComponent = 6,
    RemoveComponent = 7,
    Body = 8,
    Animate = 9,
    Audio = 10,
    PlaySound = 11,
    AudioBus = 12,
}

impl CommandKind {
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            1 => Self::Spawn,
            2 => Self::Destroy,
            3 => Self::SetTransform,
            4 => Self::SetParent,
            5 => Self::SetName,
            6 => Self::SetComponent,
            7 => Self::RemoveComponent,
            8 => Self::Body,
            9 => Self::Animate,
            10 => Self::Audio,
            11 => Self::PlaySound,
            12 => Self::AudioBus,
            _ => return None,
        })
    }
}

/// Spawns an entity; `temp_id` is echoed by `Spawned` with the real handle.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 1, name = "fynkel.cmd.Spawn")]
pub struct SpawnCommand {
    /// Non-zero, unique within the batch.
    pub temp_id: u64,
    pub transform: Transform,
    pub parent: Option<Entity>,
    /// Empty for no name.
    pub name: String,
}

/// Destroys the entity and its subtree.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 2, name = "fynkel.cmd.Destroy")]
pub struct DestroyCommand {
    pub entity: Entity,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 3, name = "fynkel.cmd.SetTransform")]
pub struct SetTransformCommand {
    pub entity: Entity,
    pub transform: Transform,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 4, name = "fynkel.cmd.SetParent")]
pub struct SetParentCommand {
    pub child: Entity,
    pub parent: Option<Entity>,
    /// Keep the world transform (adjust local) instead of the local one.
    pub keep_world: bool,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 5, name = "fynkel.cmd.SetName")]
pub struct SetNameCommand {
    pub entity: Entity,
    pub name: String,
}

/// Inserts or replaces one native component (M3-07a): the body is this
/// record followed by the component's own wire bytes (`component` is its
/// `ComponentId`; strings in the batch tail), so one command kind carries
/// every component `ComponentValue` knows. `fynkel.Transform` here is a
/// `SetTransform`; `fynkel.Name` is not a value (`SetName`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 6, name = "fynkel.cmd.SetComponent")]
pub struct SetComponentCommand {
    pub entity: Entity,
    /// `ComponentId`.
    pub component: u32,
}

/// Removes one native component; `Transform`, the hierarchy and `Name`
/// cannot be removed (`INVALID_PAYLOAD`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 7, name = "fynkel.cmd.RemoveComponent")]
pub struct RemoveComponentCommand {
    pub entity: Entity,
    /// `ComponentId`.
    pub component: u32,
}

/// What a `fynkel.cmd.Body` does with its `vector`/`rotation`
/// (`fynkel_physics::BodyCommand`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.BodyOp")]
pub enum BodyOp {
    /// `vector` newtons, this tick (dynamic).
    ApplyForce,
    /// `vector` newton-seconds, once (dynamic).
    ApplyImpulse,
    /// `vector` newton-metres, this tick (dynamic).
    ApplyTorque,
    /// `vector` newton-metre-seconds, once (dynamic).
    ApplyTorqueImpulse,
    /// `vector` metres per second (dynamic).
    SetLinearVelocity,
    /// `vector` radians per second (dynamic).
    SetAngularVelocity,
    /// `vector` + `rotation`: the world pose a kinematic body reaches at
    /// the end of this tick.
    SetKinematicTarget,
    /// `vector` + `rotation`: the world pose, now, without interpolation
    /// (any body type).
    Teleport,
    /// `vector` metres per second for this tick (a `CharacterController`;
    /// M3-09).
    Move,
    /// `vector.y` take-off speed, `0` for the controller's `jump_speed`
    /// (a `CharacterController`; M3-09).
    Jump,
}

/// Moves a `fynkel.RigidBody` — or, with `move`/`jump`, a
/// `fynkel.CharacterController` — (M3-08; simulation lane only). Rejected
/// at the tick with `MISSING_COMPONENT` (no body / no controller) or
/// `INVALID_PAYLOAD` (wrong body type for the op, non-finite value).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 8, name = "fynkel.cmd.Body")]
pub struct BodyCommandRecord {
    pub entity: Entity,
    pub op: BodyOp,
    pub vector: Vec3,
    /// Used by `setKinematicTarget` and `teleport`; identity otherwise.
    pub rotation: Quat,
}

/// What a `fynkel.cmd.Animate` does (`fynkel_core::command::AnimateOp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.AnimateOp")]
pub enum AnimateOp {
    /// `clip`, `looping`, `speed`: switch to the clip from its start and
    /// play.
    Play,
    /// `clip`, `looping`, `speed`, `seconds`: like `play`, blending the
    /// previous clip out over `seconds`.
    Crossfade,
    /// Rewind and stop.
    Stop,
    /// Stop, keeping the time.
    Pause,
    Resume,
    /// `speed`.
    SetSpeed,
    /// `seconds`: seek.
    SetTime,
    /// `value`: the layer weight, within `[0, 1]` (M3-19c).
    SetWeight,
    /// `name`, `value`: a blend-space parameter (M3-19c).
    SetParameter,
}

/// Drives a `fynkel.Animator` (M3-19; simulation lane only). Rejected at
/// the tick with `MISSING_COMPONENT` (no animator) or `INVALID_PAYLOAD`
/// (non-finite value, negative fade, a weight outside `[0, 1]`, an empty
/// parameter name).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 9, name = "fynkel.cmd.Animate")]
pub struct AnimateCommandRecord {
    pub entity: Entity,
    pub op: AnimateOp,
    /// Fade length for `crossfade`, the target time for `setTime`.
    pub seconds: f32,
    /// Playback rate for `play`/`crossfade`/`setSpeed`.
    pub speed: f32,
    /// For `play`/`crossfade`.
    pub looping: bool,
    /// For `play`/`crossfade`.
    pub clip: AssetRef,
    /// The weight for `setWeight`, the parameter value for `setParameter`.
    pub value: f32,
    /// The parameter name for `setParameter`.
    pub name: String,
}

/// What a `fynkel.cmd.Audio` does (`fynkel_core::command::AudioOp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.AudioOp")]
pub enum AudioOp {
    /// Start the source's clip from the beginning; sets `playing`.
    Play,
    /// Stop the voice; clears `playing` (no `PLAYBACK_ENDED`).
    Stop,
    /// Hold the voice where it is.
    Pause,
    Resume,
    /// `value`: the source's `volume`, within `[0, 1]`.
    SetVolume,
    /// `value`: the source's `pitch`, within `[0.05, 8]`.
    SetPitch,
}

/// Drives a `fynkel.AudioSource` (M3-11; simulation lane only). Rejected
/// at the tick with `MISSING_COMPONENT` (no source) or `INVALID_PAYLOAD`
/// (a value outside its rule).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 10, name = "fynkel.cmd.Audio")]
pub struct AudioCommandRecord {
    pub entity: Entity,
    pub op: AudioOp,
    /// The volume for `setVolume`, the pitch for `setPitch`.
    pub value: f32,
}

/// Plays a clip once on no entity (M3-11; either lane): a fire-and-forget
/// sound — no `PLAYBACK_ENDED`, no handle. `SOUND_UNAVAILABLE` at the tick
/// for a clip the world has not been given; refused silently under the
/// voice limit (`fynkel.rec.AudioStats.refused` counts).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 11, name = "fynkel.cmd.PlaySound")]
pub struct PlaySoundCommandRecord {
    pub clip: AssetRef,
    pub bus: AudioBus,
    /// `0..=1`.
    pub volume: f32,
    /// `0.05..=8`.
    pub pitch: f32,
    pub priority: i32,
}

/// Sets a bus gain and the mute (M3-11; either lane): what the
/// `fynkel.Settings` volumes are applied through. `master` scales the
/// other two; `muted` silences everything.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "command", id = 12, name = "fynkel.cmd.AudioBus")]
pub struct AudioBusCommandRecord {
    pub bus: AudioBus,
    /// `0..=1`.
    pub volume: f32,
    pub muted: bool,
}

/// `FYNKEL_QUERY_AUDIO` (M3-11): a `fynkel.AudioSource`'s playback state as
/// the last step left it (no record without one).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.AudioState")]
pub struct AudioStateRecord {
    pub entity: Entity,
    /// The source has a voice (paused or not).
    pub playing: bool,
    pub paused: bool,
    /// Seconds into the clip.
    pub time: f32,
    /// The clip's length; `0` while the clip is unknown to the world.
    pub duration: f32,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub clip: AssetRef,
    pub bus: AudioBus,
    /// The voice handle (`generation << 8 | slot`), `0` without one.
    pub voice: u32,
}

/// `FYNKEL_QUERY_AUDIO_STATS` (M3-11): the world's audio engine at a
/// glance — the device (or the null backend), the voices in use and the
/// counters since the world was created.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.AudioStats")]
pub struct AudioStatsRecord {
    /// A real output device is open (else the null backend).
    pub device: bool,
    pub sample_rate: u32,
    pub channels: u32,
    /// Voices playing now.
    pub voices: u32,
    pub max_voices: u32,
    pub started: u32,
    pub stolen: u32,
    pub refused: u32,
    pub ended: u32,
    /// Clips registered.
    pub clips: u32,
    /// Device callbacks (or null renders) so far.
    pub callbacks: u32,
    /// Device underruns reported by the backend.
    pub underruns: u32,
    pub suspended: bool,
    /// Streamed clips registered (M3-11 part 2: decoded as they play).
    pub streams: u32,
    /// Callbacks in which a streamed voice's decoder had fallen behind.
    pub starved: u32,
}

/// A spatial query kind (`fynkel.rec.PhysicsQuery`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Schema)]
#[schema(name = "fynkel.PhysicsQueryKind")]
pub enum PhysicsQueryKind {
    /// A ray from `origin` along `direction`, up to `max_distance`.
    Raycast,
    /// A shape swept from `origin` along `direction`; the first hit only.
    ShapeCast,
    /// Every collider intersecting a shape at `origin`.
    Overlap,
}

/// One spatial query in a `PhysicsQueries` buffer
/// (`fynkel_world_physics_query`); answered by `fynkel.rec.PhysicsHit`
/// records carrying its `id`. Colliders are candidates when their `layer`
/// shares a bit with `mask`; the `exclude` entity's colliders are skipped;
/// sensors are hit only with `hit_sensors`.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.PhysicsQuery")]
pub struct PhysicsQueryRecord {
    /// Echoed by every hit of this query.
    pub id: u32,
    pub kind: PhysicsQueryKind,
    /// `shapeCast`/`overlap`: `box`, `sphere` or `capsule` (never `mesh`)
    /// with `size` as `fynkel.Collider` defines it.
    pub shape: ColliderShape,
    pub size: Vec3,
    pub origin: Vec3,
    /// The shape's orientation.
    pub rotation: Quat,
    /// `raycast`/`shapeCast`; need not be normalised.
    pub direction: Vec3,
    pub max_distance: f32,
    pub mask: u32,
    pub exclude: Option<Entity>,
    /// `raycast`/`overlap`: at most this many hits, nearest first (0 = 1).
    pub max_hits: u32,
    pub hit_sensors: bool,
}

/// One hit of a `fynkel.rec.PhysicsQuery`: `point`/`normal` on the hit
/// collider in world space (zero for `overlap`), `distance` along the
/// ray or sweep (0 for `overlap`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.PhysicsHit")]
pub struct PhysicsHitRecord {
    pub query: u32,
    pub entity: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

/// `FYNKEL_QUERY_BODY`: the motion state of an entity's `fynkel.RigidBody`
/// (`MISSING_COMPONENT` without one).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.BodyState")]
pub struct BodyStateRecord {
    pub entity: Entity,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub sleeping: bool,
}

/// `FYNKEL_QUERY_CHARACTER` (M3-09): a `fynkel.CharacterController`'s held
/// state as the last step left it (no record without one).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.CharacterState")]
pub struct CharacterStateRecord {
    pub entity: Entity,
    /// Standing on ground no steeper than `max_slope`.
    pub grounded: bool,
    /// Touching a slope steeper than `max_slope` (sliding down it).
    pub sliding: bool,
    /// The movement applied over the last tick, metres per second.
    pub velocity: Vec3,
}

/// `FYNKEL_QUERY_ANIMATOR` (M3-19): a `fynkel.Animator`'s playback state
/// as the last step left it (no record without one).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.AnimatorState")]
pub struct AnimatorStateRecord {
    pub entity: Entity,
    /// Seconds into the clip.
    pub time: f32,
    /// The clip's length; `0` while the clip is unknown to the world.
    pub duration: f32,
    pub speed: f32,
    /// Seconds left of a crossfade, `0` when none is running.
    pub fade_remaining: f32,
    pub playing: bool,
    pub looping: bool,
    /// The clip being played.
    pub clip: AssetRef,
    /// The layer weight (M3-19c).
    pub weight: f32,
    /// The value of the parameter a playing blend space reads, `0` for a
    /// clip (M3-19c).
    pub parameter: f32,
    /// Root motion taken out of the pose over the last tick, in the
    /// entity's parent space (M3-19c); zero without `rootMotion`.
    pub root_delta: Vec3,
    /// Root yaw taken out over the last tick, radians (M3-19c).
    pub root_yaw: f32,
}

/// A phase of `World::step` whose time is measured (M3-10, blueprint
/// §15.4): the whole step, then its phases in tick order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Schema)]
#[schema(name = "fynkel.SystemKind")]
pub enum SystemKind {
    /// The whole `step`; `entities` is the live count.
    Step,
    /// Validating and applying the tick's command batch; `entities` is
    /// the number applied.
    Commands,
    /// The characters' action driver (M3-09).
    Characters,
    /// Clip playback onto the animators' subtrees and the skins' joint
    /// lookups (M3-19); `entities` is animators + skins.
    Animation,
    /// Catalogue movers in the pre-tick slot.
    Rotator,
    Oscillate,
    LifetimeDespawn,
    /// `entities` is the number of emitters; instances spawned count in
    /// their own components.
    Emitter,
    /// Sync, the Rapier step and the write-back; `entities` is bodies +
    /// colliders + characters.
    Physics,
    CameraRigs,
    Follow,
    LookAtCamera,
    /// Propagation, deferred destroys and the interpolation bookkeeping;
    /// `entities` is the number of world matrices recomputed.
    Transforms,
    /// The audio sources reconciled with their voices and the voice clock
    /// advanced (M3-11); `entities` is the number of sources.
    Audio,
    /// Snapshot and event publication; `entities` is the snapshot's
    /// records.
    Publish,
}

impl SystemKind {
    /// The schema's enum value (`lifetimeDespawn`), for reports.
    pub const fn name(self) -> &'static str {
        match self {
            SystemKind::Step => "step",
            SystemKind::Commands => "commands",
            SystemKind::Characters => "characters",
            SystemKind::Animation => "animation",
            SystemKind::Rotator => "rotator",
            SystemKind::Oscillate => "oscillate",
            SystemKind::LifetimeDespawn => "lifetimeDespawn",
            SystemKind::Emitter => "emitter",
            SystemKind::Physics => "physics",
            SystemKind::CameraRigs => "cameraRigs",
            SystemKind::Follow => "follow",
            SystemKind::LookAtCamera => "lookAtCamera",
            SystemKind::Transforms => "transforms",
            SystemKind::Audio => "audio",
            SystemKind::Publish => "publish",
        }
    }
}

/// `FYNKEL_QUERY_SYSTEMS` (M3-10): one per [`SystemKind`], the last
/// step's wall time and the number of things that phase touched. What the
/// HUD and the headless report print as per-system time.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.SystemTime")]
pub struct SystemTimeRecord {
    pub system: SystemKind,
    pub entities: u32,
    /// Microseconds of wall time in the last step.
    pub micros: u32,
}

/// One anchor of a projection request (`fynkel_world_project`, M3-13,
/// blueprint §10): a point `offset` from the entity's origin in its local
/// frame, projected into the viewport the request names. `mask` (collider
/// layers) enables the explicit occlusion policy: a raycast from the
/// camera to the point that hits a collider on those layers first marks
/// the anchor `OCCLUDED`; 0 tests nothing.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.ProjectionAnchor")]
pub struct ProjectionAnchorRecord {
    pub entity: Entity,
    pub offset: Vec3,
    pub mask: u32,
}

/// The answer to one `fynkel.rec.ProjectionAnchor`, in request order:
/// `x`/`y` in the requested viewport's pixels (origin top-left, y down),
/// `depth` metres in front of the camera along its view axis, `flags` the
/// `PROJECTED_*` bits (`fynkel_core::projection`). `x`/`y` are meaningful
/// only with `PROJECTED_IN_FRONT`.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.ProjectedPoint")]
pub struct ProjectedPointRecord {
    pub entity: Entity,
    pub x: f32,
    pub y: f32,
    pub depth: f32,
    pub flags: u32,
}

/// `FYNKEL_QUERY_ENTITIES` (M3-08b): one per live entity, entity order —
/// its parent (`NULL` at the root) and name (empty without one). What
/// `World.find`/`findAll`/`findChild` read, one query per tick.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.EntityInfo")]
pub struct EntityInfoRecord {
    pub entity: Entity,
    pub parent: Entity,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 1, name = "fynkel.ev.Spawned")]
pub struct SpawnedEvent {
    pub temp_id: u64,
    pub entity: Entity,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 2, name = "fynkel.ev.Destroyed")]
pub struct DestroyedEvent {
    pub entity: Entity,
}

#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 3, name = "fynkel.ev.ParentChanged")]
pub struct ParentChangedEvent {
    pub child: Entity,
    pub parent: Option<Entity>,
}

/// Time dropped after `max_steps_per_frame` catch-up ticks (ADR-0008).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 4, name = "fynkel.ev.MissedDeadline")]
pub struct MissedDeadlineEvent {
    pub dropped_seconds: f32,
}

/// A scene's entities exist and its components are applied (M3-12: names
/// the scene, so a manager loading several tells them apart).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 5, name = "fynkel.ev.SceneReady")]
pub struct SceneReadyEvent {
    /// The scene's asset id.
    pub scene: AssetRef,
}

/// A scene was unloaded (M3-12): its still-alive entities were destroyed
/// (their `Destroyed` events precede this one) and the host released its
/// asset residency.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 6, name = "fynkel.ev.SceneUnloaded")]
pub struct SceneUnloadedEvent {
    pub scene: AssetRef,
    /// Entities destroyed by the unload.
    pub entities: u32,
}

/// A `fynkel.TriggerZone` fired (M3-12): the body of `ZONE_ENTERED` (7)
/// and `ZONE_EXITED` (8), on the general lane the tick the sensor saw it.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 7, name = "fynkel.ev.Zone")]
pub struct ZoneEvent {
    /// The entity carrying the `TriggerZone`.
    pub zone: Entity,
    /// The collider that entered or left.
    pub other: Entity,
    /// The zone's `event` name.
    pub event: String,
}

/// A `fynkel.Animator`'s non-looping clip reached its end (M3-19): the
/// body of `ANIMATION_ENDED` (9), general lane, the tick it happened;
/// `playing` is cleared the same tick.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 9, name = "fynkel.ev.AnimationEnded")]
pub struct AnimationEndedEvent {
    pub entity: Entity,
    pub clip: AssetRef,
}

/// An animator's clock crossed a clip marker (M3-19c): the body of
/// `ANIMATION_MARKER` (10), general lane, once per crossing — a loop
/// crosses every marker every lap, a negative speed crosses them
/// backwards, a fading clip's markers are not raised.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 10, name = "fynkel.ev.AnimationMarker")]
pub struct AnimationMarkerEvent {
    pub entity: Entity,
    /// The clip whose marker it is (a blend space's dominant clip).
    pub clip: AssetRef,
    pub name: String,
}

/// A `fynkel.AudioSource`'s non-looping clip reached its end (M3-11): the
/// body of `PLAYBACK_ENDED` (11), general lane, the tick it happened;
/// `playing` is cleared the same tick. A stopped or stolen voice raises
/// nothing.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 11, name = "fynkel.ev.PlaybackEnded")]
pub struct PlaybackEndedEvent {
    pub entity: Entity,
    pub clip: AssetRef,
}

/// Physics-lane events share one body (collision/trigger enter/stay/exit):
/// `point` and `normal` in world space, the normal pointing from `a`
/// towards `b`; `impulse` the pair's total impulse over the step (0 for
/// triggers and exits).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "event", id = 256, name = "fynkel.ev.Contact")]
pub struct ContactEvent {
    pub a: Entity,
    pub b: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub impulse: f32,
}

/// Snapshot record for a subscribed transform.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.SnapshotTransform")]
pub struct SnapshotTransformRecord {
    pub entity: Entity,
    pub local: Transform,
    pub world: Transform,
}

/// One rejected command or runtime diagnostic (ADR-0014).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.Diagnostic")]
pub struct DiagnosticRecord {
    /// Index in the submitted batch; `u32::MAX` when not tied to a command.
    pub command_index: u32,
    /// `DiagnosticCode`.
    pub code: u32,
    pub entity: Entity,
    /// `ComponentId`, `0` when not component-specific.
    pub component: u32,
    /// `Lane`: 0 simulation, 1 presentation.
    pub lane: u32,
    pub message: String,
}

/// Command envelope in a command buffer: `kind` selects the body schema
/// (a `CommandKind`), `size` lets readers skip bodies they do not know.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.CommandHeader")]
pub struct CommandHeader {
    pub kind: u32,
    /// Body bytes following this header.
    pub size: u32,
}

/// Component envelope in a `FYNKEL_QUERY_COMPONENTS` response: one per
/// native component the entity has, `component` its `ComponentId`, `size`
/// the body bytes following (the component's wire layout).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.ComponentHeader")]
pub struct ComponentHeader {
    pub component: u32,
    /// Body bytes following this header.
    pub size: u32,
}

/// Event envelope in an event buffer: `kind` selects the body schema (an
/// `EventKind`; every physics kind uses `ContactEvent`).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.EventHeader")]
pub struct EventHeader {
    pub kind: u32,
    /// Body bytes following this header.
    pub size: u32,
    /// Low 32 bits of the tick that produced the event.
    pub tick_lo: u32,
    pub flags: u32,
}

/// One raw control event of a host frame (`fynkel_world_capture_input`,
/// §5.1): `device` a `fynkel_core::input::Device`, `phase` a `Phase`;
/// `x`/`y` the axis value or movement (a button's `x` is 1 down, 0 up).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.InputEvent")]
pub struct InputEventRecord {
    pub device: u32,
    /// The control's name in the binding grammar (`Space`, `left`,
    /// `leftStick`, `overlay/jump`).
    pub control: String,
    pub phase: u32,
    pub x: f32,
    pub y: f32,
}

/// One bound action's state (`fynkel_world_input_view`): `kind` an
/// `ActionKind`; `pressed`/`released` are the edges the next tick
/// consumes, `frame_pressed`/`frame_released` the current frame's.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "record", name = "fynkel.rec.InputAction")]
pub struct InputActionRecord {
    pub name: String,
    pub kind: u32,
    pub held: bool,
    pub pressed: bool,
    pub released: bool,
    pub frame_pressed: bool,
    pub frame_released: bool,
    /// `axis1d` value, `axis2d` x, a button 1/0 (the tick view). A relative
    /// control's motion pending for the next tick, as the rate that covers
    /// it in one tick (`x · fixed_dt` is the motion).
    pub x: f32,
    pub y: f32,
    /// The frame's view of `x`/`y`: a relative control's motion summed
    /// over the current frame (what `update` reads); otherwise the same.
    pub frame_x: f32,
    pub frame_y: f32,
}

/// The registry, in fingerprint order. Append only.
pub const ALL: &[&RecordSchema] = &[
    &Transform::SCHEMA,
    &Name::SCHEMA,
    &SpawnCommand::SCHEMA,
    &DestroyCommand::SCHEMA,
    &SetTransformCommand::SCHEMA,
    &SetParentCommand::SCHEMA,
    &SetNameCommand::SCHEMA,
    &SpawnedEvent::SCHEMA,
    &DestroyedEvent::SCHEMA,
    &ParentChangedEvent::SCHEMA,
    &MissedDeadlineEvent::SCHEMA,
    &SceneReadyEvent::SCHEMA,
    &ContactEvent::SCHEMA,
    &SnapshotTransformRecord::SCHEMA,
    &DiagnosticRecord::SCHEMA,
    &CommandHeader::SCHEMA,
    &EventHeader::SCHEMA,
    &MeshRenderer::SCHEMA,
    &Camera::SCHEMA,
    &DirectionalLight::SCHEMA,
    &PointLight::SCHEMA,
    &PbrMaterial::SCHEMA,
    &SpotLight::SCHEMA,
    &Environment::SCHEMA,
    &InputEventRecord::SCHEMA,
    &InputActionRecord::SCHEMA,
    &SetComponentCommand::SCHEMA,
    &RemoveComponentCommand::SCHEMA,
    &ComponentHeader::SCHEMA,
    &RigidBody::SCHEMA,
    &Collider::SCHEMA,
    &BodyCommandRecord::SCHEMA,
    &PhysicsQueryRecord::SCHEMA,
    &PhysicsHitRecord::SCHEMA,
    &BodyStateRecord::SCHEMA,
    &EntityInfoRecord::SCHEMA,
    &CharacterController::SCHEMA,
    &ThirdPersonCamera::SCHEMA,
    &CharacterStateRecord::SCHEMA,
    &Rotator::SCHEMA,
    &Oscillate::SCHEMA,
    &Follow::SCHEMA,
    &LookAtCamera::SCHEMA,
    &LifetimeDespawn::SCHEMA,
    &Emitter::SCHEMA,
    &SystemTimeRecord::SCHEMA,
    &TriggerZone::SCHEMA,
    &SceneUnloadedEvent::SCHEMA,
    &ZoneEvent::SCHEMA,
    &Settings::SCHEMA,
    &ProjectionAnchorRecord::SCHEMA,
    &ProjectedPointRecord::SCHEMA,
    &Animator::SCHEMA,
    &Skin::SCHEMA,
    &AnimateCommandRecord::SCHEMA,
    &AnimationEndedEvent::SCHEMA,
    &AnimatorStateRecord::SCHEMA,
    &AnimationMarkerEvent::SCHEMA,
    &AudioSource::SCHEMA,
    &AudioListener::SCHEMA,
    &AudioCommandRecord::SCHEMA,
    &PlaySoundCommandRecord::SCHEMA,
    &AudioBusCommandRecord::SCHEMA,
    &PlaybackEndedEvent::SCHEMA,
    &AudioStateRecord::SCHEMA,
    &AudioStatsRecord::SCHEMA,
    &Volume::SCHEMA,
];

/// Derived, never hand-maintained (ADR-0012). Generated code embeds this
/// value and `fynkel codegen --check` fails when it drifts.
pub fn schema_fingerprint() -> u64 {
    fingerprint(ALL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::schema::{FieldType, SchemaKind};

    #[test]
    fn transform_schema_is_forty_bytes() {
        let s = &Transform::SCHEMA;
        assert_eq!(s.kind, SchemaKind::Component);
        assert_eq!(s.id, 1);
        assert_eq!(s.rust_path, "fynkel_scene::Transform");
        assert_eq!(
            s.fields.iter().map(|f| f.ty).collect::<Vec<_>>(),
            vec![FieldType::Vec3, FieldType::Quat, FieldType::Vec3]
        );
        assert_eq!(s.wire_size(), 40);
        assert_eq!(s.fields[0].doc, "Metres.");
    }

    #[test]
    fn nested_and_string_layouts() {
        let s = &SpawnCommand::SCHEMA;
        // temp_id u64 @0, transform 40 @8, parent u64 @48, name (u32,u32) @56
        assert_eq!(s.offsets(), vec![0, 8, 48, 56]);
        assert_eq!(s.wire_size(), 64);
        assert!(s.has_strings());
        // M3-12: the scene's AssetRef (16 id bytes + a str slot).
        assert_eq!(SceneReadyEvent::SCHEMA.wire_size(), 24);
        assert_eq!(SceneUnloadedEvent::SCHEMA.wire_size(), 32);
        assert_eq!(ZoneEvent::SCHEMA.wire_size(), 24);
        assert_eq!(TriggerZone::SCHEMA.wire_size(), 16);
        assert_eq!(SnapshotTransformRecord::SCHEMA.wire_size(), 88);
        // M3-08: entity @0, op @8, vector @12, rotation @24 → 40.
        assert_eq!(BodyCommandRecord::SCHEMA.offsets(), vec![0, 8, 12, 24]);
        assert_eq!(BodyCommandRecord::SCHEMA.wire_size(), 40);
        assert_eq!(ContactEvent::SCHEMA.wire_size(), 48);
        assert_eq!(PhysicsQueryRecord::SCHEMA.wire_size(), 88);
        assert_eq!(PhysicsHitRecord::SCHEMA.wire_size(), 48);
        assert_eq!(BodyStateRecord::SCHEMA.wire_size(), 40);
        // M3-09: entity @0, grounded @8, sliding @9, velocity @12 → 24.
        assert_eq!(CharacterStateRecord::SCHEMA.offsets(), vec![0, 8, 9, 12]);
        assert_eq!(CharacterStateRecord::SCHEMA.wire_size(), 24);
        assert_eq!(ThirdPersonCamera::SCHEMA.wire_size(), 64);
        // M3-10: axis 12 @0, speed @12, space u32 @16 → 24 (padded).
        assert_eq!(Rotator::SCHEMA.offsets(), vec![0, 12, 16]);
        assert_eq!(Rotator::SCHEMA.wire_size(), 24);
        assert_eq!(Oscillate::SCHEMA.wire_size(), 32);
        // target (u32,u32) @0, offset @8, smoothing @20, look_at @24 → 32.
        assert_eq!(Follow::SCHEMA.offsets(), vec![0, 8, 20, 24]);
        assert_eq!(Follow::SCHEMA.wire_size(), 32);
        assert_eq!(LookAtCamera::SCHEMA.wire_size(), 8);
        assert_eq!(LifetimeDespawn::SCHEMA.wire_size(), 8);
        // prefab 24 @0, rate @24, lifetime @28, max_alive @32, velocity
        // @36, enabled @48 → 56.
        assert_eq!(Emitter::SCHEMA.offsets(), vec![0, 24, 28, 32, 36, 48]);
        assert_eq!(Emitter::SCHEMA.wire_size(), 56);
        assert_eq!(SystemTimeRecord::SCHEMA.wire_size(), 16);
        // M3-19: clip 24 @0, speed @24, looping @28, playing @29; M3-19c:
        // weight @32, additive @36, root_motion @37 → 40.
        assert_eq!(Animator::SCHEMA.offsets(), vec![0, 24, 28, 29, 32, 36, 37]);
        assert_eq!(Animator::SCHEMA.wire_size(), 40);
        assert_eq!(Skin::SCHEMA.wire_size(), 32);
        // entity @0, op @8, seconds @12, speed @16, looping @20, clip @24,
        // value @48, name @52 → 64.
        assert_eq!(
            AnimateCommandRecord::SCHEMA.offsets(),
            vec![0, 8, 12, 16, 20, 24, 48, 52]
        );
        assert_eq!(AnimateCommandRecord::SCHEMA.wire_size(), 64);
        assert_eq!(AnimationEndedEvent::SCHEMA.wire_size(), 32);
        // entity @0, clip @8, name @32 → 40.
        assert_eq!(AnimationMarkerEvent::SCHEMA.wire_size(), 40);
        // entity @0, time @8, duration @12, speed @16, fade @20, playing
        // @24, looping @25, clip @32, weight @56, parameter @60, root_delta
        // @64, root_yaw @76 → 80.
        assert_eq!(
            AnimatorStateRecord::SCHEMA.offsets(),
            vec![0, 8, 12, 16, 20, 24, 25, 32, 56, 60, 64, 76]
        );
        assert_eq!(AnimatorStateRecord::SCHEMA.wire_size(), 80);
    }

    #[test]
    fn component_ids_match_the_contracts_table() {
        for s in ALL.iter().filter(|s| s.kind == SchemaKind::Component) {
            assert_eq!(
                fynkel_contracts::ComponentId(s.id).name(),
                Some(s.name),
                "{} id {} disagrees with fynkel_contracts::ids",
                s.name,
                s.id
            );
        }
    }

    #[test]
    fn registry_ids_are_unique_per_kind() {
        let mut seen = std::collections::HashSet::new();
        for s in ALL {
            assert!(seen.insert(s.name), "duplicate name {}", s.name);
            if s.kind != SchemaKind::Record {
                assert!(
                    seen.insert(format!("{}#{}", s.kind.name(), s.id).leak() as &str),
                    "duplicate id {} {}",
                    s.kind.name(),
                    s.id
                );
            }
        }
        assert_ne!(schema_fingerprint(), 0);
    }
}
