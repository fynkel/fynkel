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

pub mod clock;
pub mod command;
pub mod event;
pub mod schema;
pub mod world;

pub use clock::{FrameClock, FramePlan};
pub use command::{Command, Lane};
pub use event::{Event, EventPayload};
pub use fynkel_contracts::{ComponentId, DiagnosticCode, Entity, EventKind, Status, Tick, WorldId};
pub use fynkel_scene::Transform;
pub use world::{
    Diagnostic, Extract, ExtractRecord, Name, PhysicsOwned, Rejected, SnapshotRecord, StepReport,
    SubmitReport, World, WorldConfig,
};
