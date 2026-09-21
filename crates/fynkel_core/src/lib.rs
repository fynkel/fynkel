//! Fynkel runtime core (blueprint §4, §5; ADR-0007, ADR-0008, ADR-0014).
//!
//! The core is a library: it creates no threads and owns no loop. A host
//! drives one **frame** as
//!
//! ```text
//! let plan = world.begin_frame(dt);          // accumulator, MissedDeadline
//! for _ in 0..plan.steps {
//!     let events = world.drain_events();     // pull-only, general + last tick's physics
//!     // ... gameplay reads world.snapshot(), builds commands ...
//!     world.submit(Lane::Simulation, cmds);  // staged for this tick
//!     let report = world.step();             // §5.1 fixed tick
//!     // ... afterPhysics reads world.after_physics_events() ...
//! }
//! // ... update/lateUpdate: presentation commands ...
//! world.submit(Lane::Presentation, cmds);
//! let extract = world.extract();             // interpolated by alpha + overrides
//! ```
//!
//! Everything Dart-facing is typed Rust here; the wire encoding lives in
//! `fynkel_native` (M1-06) and is generated from these types (ADR-0012).

pub mod animation;
pub mod clock;
pub mod command;
pub mod components;
pub mod event;
pub mod input;
pub mod load;
pub mod material;
pub mod schema;
pub mod settings;
pub mod world;

/// The clip core; `animation::Skin` (the data) is reached through the
/// module, `Skin` at the root is the component.
pub use animation::{Channel, Clip, Interpolation, Joint, Pose, PoseEntry, Property};
pub use clock::{FrameClock, FramePlan};
pub use command::{AnimateOp, Command, Lane};
pub use components::{
    Animator, Camera, ComponentValue, DirectionalLight, Emitter, Environment, ExposureMode, Follow,
    LifetimeDespawn, LookAtCamera, MeshRenderer, MotionSpace, Oscillate, PointLight, Projection,
    Rotator, Skin, SpotLight, ThirdPersonCamera, TriggerZone,
};
pub use event::{Event, EventPayload};
pub use fynkel_contracts::{
    AssetId, AssetRef, ComponentId, DiagnosticCode, Entity, EventKind, Status, Tick, Uuid, WorldId,
};
pub use fynkel_physics::{
    self as physics, BodyCommand, BodyType, CharacterController, CharacterState, Collider,
    ColliderShape, Contact, Physics, PhysicsEvent, RigidBody,
};
pub use fynkel_scene::Transform;
pub use input::{ActionKind, ActionSpec, ActionState, Device, Input, InputEvent, Phase};
pub use load::{
    EntityDescription, LoadError, LoadMode, LoadedScene, SceneDescription, UnloadedScene,
};
pub use settings::Settings;
pub use world::{
    projection_matrix, view_matrix, Diagnostic, Extract, ExtractCamera, ExtractDirectionalLight,
    ExtractEnvironment, ExtractMesh, ExtractPointLight, ExtractRecord, ExtractSpotLight, Name,
    PhysicsOwned, ProjectionInfo, Rejected, SnapshotRecord, StepReport, SubmitReport, World,
    WorldConfig, MAX_PROJECTION_ANCHORS, PROJECTED_DROPPED, PROJECTED_IN_FRONT, PROJECTED_MISSING,
    PROJECTED_NO_CAMERA, PROJECTED_OCCLUDED, PROJECTED_ON_SCREEN, PROJECTED_VISIBLE,
};
