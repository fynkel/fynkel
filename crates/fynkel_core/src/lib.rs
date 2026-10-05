//! Fynkel runtime core (blueprint §4, §5; ADR-0007, ADR-0008, ADR-0014).
//!
//! The core is a library: it creates no threads and owns no loop. A host
//! drives one **frame** as
//!
//! ```text
//! let plan = world.begin_frame(dt);          // accumulator, MissedDeadline
//! for _ in 0..plan.steps {
//!     let events = world.drain_events();     // pull-only, general + last tick's physics
//!     world.prepare_gameplay_tick();
//!     world.edit_gameplay(|edit| { /* gameplay */ Ok(()) })?;
//!     let report = world.step();             // §5.1 fixed tick
//!     // ... afterPhysics reads world.after_physics_events() ...
//! }
//! world.edit_gameplay(|edit| { /* update/lateUpdate */ Ok(()) })?;
//! let extract = world.extract();             // interpolated by alpha
//! ```
//!
//! Everything Dart-facing is typed Rust here; the wire encoding lives in
//! `fynkel_native` (M1-06) and is generated from these types (ADR-0012).

pub mod clock;
pub mod command;
pub mod components;
pub mod event;
pub mod extensions;
pub mod input;
pub mod load;
pub mod material;
pub mod memory;
pub mod modules;
pub mod registry;
pub mod scheduler;
pub mod schema;
pub mod settings;
pub mod volume;
pub mod world;

pub use clock::{FrameClock, FramePlan};
pub use command::{AnimateOp, AudioOp, Command, Lane};
pub use components::{
    AnimationBinding, Animator, Camera, ComponentValue, DirectionalLight, Emitter, Environment,
    ExposureMode, Follow, LifetimeDespawn, LookAtCamera, MeshRenderer, MotionSpace, Oscillate,
    PointLight, Projection, Rotator, Skin, SpotLight, ThirdPersonCamera, TriggerZone, Volume,
    VolumeShape,
};
pub use event::{Event, EventPayload};
/// The clip core (`fynkel_animation`, the animation module of ADR-0029);
/// `animation::Skin` (the data) is reached through the module, `Skin` at
/// the root is the component.
pub use fynkel_animation as animation;
pub use fynkel_animation::{
    BlendPoint, BlendSpace, BlendSync, Channel, Clip, Interpolation, Joint, Pose, PoseEntry,
    Property,
};
pub use fynkel_audio::{
    self as audio, Audio, AudioBus, AudioClip, AudioListener, AudioSource, AudioStats, PlayError,
    PlayParams, VoiceId, VoiceState,
};
pub use fynkel_contracts::{
    AssetId, AssetRef, ComponentId, DiagnosticCode, Entity, EventKind, Status, Tick, Uuid, WorldId,
};
pub use fynkel_physics::{
    self as physics, BodyCommand, BodyType, CharacterController, CharacterState, Collider,
    ColliderShape, Contact, ContactPoint, Physics, PhysicsEvent, RigidBody,
};
pub use fynkel_scene::{Layer, Tags, Transform};
pub use input::{
    ActionKind, ActionSpec, ActionState, Device, Input, InputEvent, Phase, PreparedInputBindings,
};
pub use load::{
    Cloned, EntityDescription, LoadError, LoadMode, LoadedScene, SceneDescription, UnloadedScene,
};
pub use modules::{Module, Modules};
pub use settings::Settings;
pub use world::gameplay::{
    EditCommit, EditError, EditResult, GameplayClone, GameplayComponentValue, GameplayEdit,
    GameplayEntity, GameplayObjectRef, GameplayPrefab, GameplayQuery, GameplaySession,
    PendingEntity, RegisteredBatch,
};
pub use world::{
    projection_matrix, view_matrix, Diagnostic, Extract, ExtractCamera, ExtractDirectionalLight,
    ExtractEnvironment, ExtractMesh, ExtractPointLight, ExtractRecord, ExtractSpotLight,
    ExtractVolume, Name, PhysicsOwned, PhysicsSystemProfile, ProjectionInfo, SnapshotRecord,
    StepReport, World, WorldConfig, MAX_PROJECTION_ANCHORS, PROJECTED_DROPPED, PROJECTED_IN_FRONT,
    PROJECTED_MISSING, PROJECTED_NO_CAMERA, PROJECTED_OCCLUDED, PROJECTED_ON_SCREEN,
    PROJECTED_VISIBLE,
};

pub use world::command_resources::{CommandResourceSource, PreparedCommandResources};

pub use registry::{
    AuthoredRegisteredValue, ComponentSchema, RegisteredComponent, RegisteredDescription,
    RegisteredField, RegistryError, ScalarValue,
};

pub use scheduler::{
    RegisteredSystem, RegisteredSystemTime, ScheduleError, SystemCallback, SystemContext,
    SystemPhase, SystemSpec,
};

pub use extensions::{
    ExtensionError, ExtensionManifest, ExtensionRegistrar, NativeExtension, EXTENSION_API_VERSION,
};

mod lifecycle;
