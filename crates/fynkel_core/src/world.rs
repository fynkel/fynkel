//! The runtime world: lifecycle, gameplay transactions, `step()`, snapshots,
//! events, diagnostics, render extraction.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

/// `SystemKind` in declaration order — the index into `system_times`.
const SYSTEMS: [SystemKind; 15] = [
    SystemKind::Step,
    SystemKind::Commands,
    SystemKind::Characters,
    SystemKind::Animation,
    SystemKind::Rotator,
    SystemKind::Oscillate,
    SystemKind::LifetimeDespawn,
    SystemKind::Emitter,
    SystemKind::Physics,
    SystemKind::CameraRigs,
    SystemKind::Follow,
    SystemKind::LookAtCamera,
    SystemKind::Transforms,
    SystemKind::Audio,
    SystemKind::Publish,
];

use fynkel_contracts::{
    AssetRef, ComponentId, DiagnosticCode, Entity, Status, Tick, Uuid, WorldId, DEFAULT_FIXED_DT,
    DEFAULT_MAX_STEPS_PER_FRAME, MAX_FRAME_DT,
};
use fynkel_ecs::internal::Handle;
use fynkel_ecs::World as Ecs;
pub use fynkel_physics::PhysicsOwned;
use fynkel_physics::{
    BodyCommand, CharacterController, Physics, PhysicsEvent, QueryKind, QuerySpec, RejectionKind,
};
use fynkel_scene::{self as scene, Layer, Parent, SceneError, Transform, WorldTransform};
use fynkel_schema_derive::Schema;
use glam::{Mat4, Quat, Vec3};

use crate::clock::{FrameClock, FramePlan};
use crate::command::{Command, Lane};
use crate::components::{
    Camera, ComponentValue, DirectionalLight, Environment, MeshRenderer, PointLight, SpotLight,
    ThirdPersonCamera, TriggerZone, Volume,
};
use crate::event::{Event, EventPayload, EventQueue};
use crate::input::{ActionSpec, ActionState, BindingError, Input, InputEvent};
use crate::load::SceneDescription;
use crate::schema::{SystemKind, SystemTimeRecord};

mod animation;
mod audio;
mod catalogue;
pub mod command_resources;
mod controller;
pub mod gameplay;
mod gameplay_live;
mod ik;
pub use animation::{AnimatorRun, Fade, SkinRun};
pub use audio::AudioRun;
pub use catalogue::{EmitterRun, Oscillation};
pub use controller::ControllerPlay;
mod projection;
mod resources;
pub use resources::TransientAnimationStamp;
mod state;
mod synchronization;
mod systems;
pub use projection::{
    projection_matrix, view_matrix, ProjectionInfo, MAX_PROJECTION_ANCHORS, PROJECTED_DROPPED,
    PROJECTED_IN_FRONT, PROJECTED_MISSING, PROJECTED_NO_CAMERA, PROJECTED_OCCLUDED,
    PROJECTED_ON_SCREEN, PROJECTED_VISIBLE,
};
use state::{AnimationState, EditState, Scratch};

#[derive(Clone, Debug)]
pub struct WorldConfig {
    pub fixed_dt: f32,
    pub max_steps_per_frame: u32,
    pub max_frame_dt: f32,
    /// The editor's edit-preview world (ADR-0016, blueprint §19.2e): a
    /// `step()` delivers committed effects, propagates transforms and
    /// publishes the snapshot — and runs **no** system (no characters,
    /// animation, catalogue movers, physics, camera rigs, followers,
    /// billboards, audio, input). Nothing moves unless a command moved
    /// it, so 60 s open on a scene leave every body where the document put
    /// it. In edit mode `extract()` also honours [`World::set_edit_camera`]
    /// (the editor's own camera in the scene camera's place) and
    /// [`World::set_edit_hidden`] (entities left out of the picture), and
    /// interpolation is off — the picture is always the current
    /// transforms. A game never sets this.
    pub edit_mode: bool,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            fixed_dt: DEFAULT_FIXED_DT as f32,
            max_steps_per_frame: DEFAULT_MAX_STEPS_PER_FRAME,
            max_frame_dt: MAX_FRAME_DT as f32,
            edit_mode: false,
        }
    }
}

/// Human-readable entity name (`fynkel.Name`).
#[derive(Clone, Debug, PartialEq, Eq, Schema)]
#[schema(kind = "component", id = 3, name = "fynkel.Name", authored = false)]
pub struct Name {
    #[fynkel(read_only)]
    pub value: String,
}

impl Name {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

/// Previous local pose and parent for hierarchy-preserving interpolation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PrevTransform(Transform, Option<Entity>);

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// Index in the batch the command came from; `u32::MAX` for diagnostics
    /// not tied to a command (missed deadline).
    pub command_index: u32,
    pub lane: Lane,
    pub code: DiagnosticCode,
    pub entity: Entity,
    pub component: ComponentId,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepReport {
    pub status: Status,
    pub tick: Tick,
    pub applied: u32,
    pub rejected: u32,
    pub snapshot_records: u32,
    pub event_records: u32,
    pub diagnostic_records: u32,
    pub live_entities: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapshotRecord {
    pub entity: Entity,
    pub local: Transform,
    pub world: Transform,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractRecord {
    pub entity: Entity,
    pub world: Mat4,
    /// The bit of the entity's `fynkel.Layer` (ADR-0020; `1`, `default`,
    /// without one): a camera whose `culling_mask` lacks it neither draws
    /// the object nor lets it cast a shadow, and a point or spot light
    /// whose mask lacks it does not light it.
    pub layer_bit: u32,
}

/// A visible `MeshRenderer`: `record` indexes [`Extract::records`].
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractMesh {
    pub record: u32,
    pub mesh: AssetRef,
    pub material: AssetRef,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// The joint palette of a skinned draw (M3-19): a range of
    /// [`Extract::joints`]. `None` draws the mesh as it was cooked.
    pub skin: Option<ExtractSkin>,
}

/// One skinned mesh's matrices in [`Extract::joints`], in the skin's
/// joint order: mesh space → posed joint → mesh space, from the joints'
/// interpolated world matrices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtractSkin {
    pub first_joint: u32,
    pub joint_count: u32,
}

/// The camera the frame is rendered from: the enabled `Camera` with the
/// highest priority (ties: the earliest record).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractCamera {
    pub record: u32,
    pub world: Mat4,
    pub camera: Camera,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractDirectionalLight {
    pub record: u32,
    /// World-space direction the light shines along (the entity's -Z).
    pub direction: Vec3,
    pub light: DirectionalLight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractPointLight {
    pub record: u32,
    pub position: Vec3,
    pub light: PointLight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractSpotLight {
    pub record: u32,
    pub position: Vec3,
    /// World-space direction the light shines along (the entity's -Z).
    pub direction: Vec3,
    pub light: SpotLight,
}

/// The scene's `Environment`: the first record that has one.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractEnvironment {
    pub record: u32,
    pub environment: Environment,
}

/// An enabled `Volume` (R3a) and where it stands; the renderer resolves
/// the frame's look from these (`crate::volume`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractVolume {
    pub record: u32,
    pub world: Mat4,
    pub volume: Volume,
}

/// What a frame draws (blueprint §8.5): every spatial entity's world
/// matrix, plus the renderable components resolved against those records.
/// The renderer reads this and nothing else from the world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extract {
    pub tick: Tick,
    pub alpha: f32,
    /// Accepted gameplay elapsed, independent of simulation catch-up drops
    /// and transform interpolation. Frozen by gameplay pause and editor Step.
    /// Shaders read it as `globals.time` (ADR-0031, ADR-0047).
    pub time: f32,
    pub records: Vec<ExtractRecord>,
    pub meshes: Vec<ExtractMesh>,
    /// Every skinned mesh's palette, back to back (`ExtractMesh::skin`).
    pub joints: Vec<Mat4>,
    pub camera: Option<ExtractCamera>,
    pub directional_lights: Vec<ExtractDirectionalLight>,
    pub point_lights: Vec<ExtractPointLight>,
    pub spot_lights: Vec<ExtractSpotLight>,
    pub environment: Option<ExtractEnvironment>,
    /// Scene order (the tie-break among equal priorities).
    pub volumes: Vec<ExtractVolume>,
}

pub struct World {
    pub(crate) extensions: std::collections::BTreeMap<String, u32>,
    schedule: Vec<systems::ScheduledStage>,
    registered_system_times: Vec<crate::RegisteredSystemTime>,
    command_resources: Option<Arc<dyn command_resources::CommandResourceSource>>,
    pub(crate) config: WorldConfig,
    clock: FrameClock,
    pub(crate) gameplay: state::GameplayState,
    identity_stamp: Option<fynkel_ecs::ComponentStamp>,
    identity_revision: u64,
    /// Roots whose committed poses must reset interpolation history.
    teleported: Vec<Entity>,
    /// The subtrees whose `Inactive` markers are stale.
    scene_sync: scene::SceneSync,
    render_membership: std::sync::Mutex<state::RenderMembership>,
    publication: synchronization::Publication,
    applied_this_tick: u32,
    poisoned: bool,
    /// The binding table and raw control state (§5.1 input rules).
    input: Input,
    /// The Rapier adapter (M3-08), stepped at the §5.1 physics slot.
    physics: Physics,
    /// The optional modules the project uses (ADR-0029 §3): one that is
    /// off runs no systems and its commands are rejected.
    modules: crate::Modules,
    /// Prefab descriptions `Emitter`s may instantiate (M3-10).
    prefabs: HashMap<Uuid, Arc<SceneDescription>>,
    /// Clips, blend spaces, controllers and skins, and the animators'
    /// scratch (M3-19).
    anim: AnimationState,
    /// The audio engine (M3-11): clips, voices, buses; the device the host
    /// opened.
    audio: fynkel_audio::Audio,
    pub(crate) lifecycle: crate::lifecycle::SceneLifecycle,
    table_revision: std::sync::Mutex<synchronization::TableRevision>,
    /// Edit mode only (M4-06).
    edit: EditState,
    /// The last step's per-phase wall time, one entry per `SystemKind`.
    system_times: Vec<SystemTimeRecord>,
    /// Each occupied zone's event name by `(zone, occupant)`, so the exit
    /// still fires when the zone itself is destroyed (foundation 2.6).
    zone_names: BTreeMap<(Entity, Entity), String>,
    /// Scratch of the native systems, kept for its capacity.
    scratch: Scratch,
}

fn micros(since: Instant) -> u64 {
    since.elapsed().as_micros() as u64
}

/// One camera rig's tick, computed while the ECS is borrowed for reading
/// and applied afterwards.
struct RigPlacement {
    rig: Entity,
    yaw: f32,
    pitch: f32,
    position: Vec3,
    rotation: Quat,
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        Self::try_new(config).expect("world identity exhausted")
    }

    pub fn try_new(config: WorldConfig) -> Result<Self, fynkel_contracts::IdentityExhausted> {
        let clock = FrameClock::new(
            config.fixed_dt,
            config.max_steps_per_frame,
            config.max_frame_dt,
        );
        let input = Input::with_tick_dt(config.fixed_dt);
        Ok(Self {
            extensions: Default::default(),
            schedule: systems::builtin_schedule(),
            registered_system_times: Vec::new(),
            gameplay: state::GameplayState::new(Ecs::try_new()?),
            config,
            clock,
            identity_stamp: None,
            identity_revision: 0,
            teleported: Vec::new(),
            scene_sync: scene::SceneSync::default(),
            render_membership: Default::default(),
            publication: Default::default(),
            applied_this_tick: 0,
            poisoned: false,
            input,
            physics: Physics::new(),
            modules: crate::Modules::COMPILED,
            prefabs: HashMap::new(),
            anim: AnimationState::default(),
            audio: fynkel_audio::Audio::new(),
            lifecycle: Default::default(),
            table_revision: Default::default(),
            edit: EditState::default(),
            system_times: SYSTEMS
                .iter()
                .map(|&system| SystemTimeRecord {
                    system,
                    entities: 0,
                    micros: 0,
                })
                .collect(),
            zone_names: BTreeMap::new(),
            scratch: Scratch::default(),
            command_resources: None,
        })
    }

    /// The last step's wall time per phase (`SystemKind` order, M3-10):
    /// what `FYNKEL_QUERY_SYSTEMS` answers.
    pub fn system_times(&self) -> &[SystemTimeRecord] {
        &self.system_times
    }

    fn set_time(&mut self, system: SystemKind, entities: u32, micros: u64) {
        let i = system as usize;
        self.system_times[i].entities = entities;
        self.system_times[i].micros = micros.min(u32::MAX as u64) as u32;
    }

    pub fn physics(&self) -> &Physics {
        &self.physics
    }

    /// Before a host's queries: the colliders made since the last step
    /// (`instantiate`, a clone, `apply_gameplay_now`) join the query tree, so a
    /// query sees them at once, as Unity's (foundation 2.7). Nothing is
    /// simulated.
    pub fn refresh_physics(&mut self) {
        self.refresh_active();
        self.propagate();
        if self.modules.physics {
            self.physics.refresh(&mut self.gameplay.ecs);
        }
    }

    /// The optional modules on (ADR-0029 §3); all until the project says.
    pub fn modules(&self) -> crate::Modules {
        self.modules
    }

    /// The project's module set: from the next tick a module that is off
    /// runs no systems and its commands are rejected (`MODULE_OFF`). A
    /// module this build does not carry stays off (`Modules::COMPILED`).
    pub fn set_modules(&mut self, modules: crate::Modules) {
        self.modules = modules.and(crate::Modules::COMPILED);
    }

    /// Rejects `cmd` when the module it needs is off; true when it may go on.
    fn module_allows(&mut self, index: u32, lane: Lane, cmd: &Command) -> bool {
        use crate::Module;
        let module = match cmd {
            Command::Body { .. } => Module::Physics,
            Command::Animate { .. } => Module::Animation,
            Command::Audio { .. } | Command::PlaySound { .. } | Command::AudioBus { .. } => {
                Module::Audio
            }
            _ => return true,
        };
        if self.modules.has(module) {
            return true;
        }
        self.reject(
            index,
            lane,
            DiagnosticCode::MODULE_OFF,
            cmd.target().unwrap_or(Entity::NULL),
            ComponentId::NONE,
            format!(
                "FYN-R0014: the {} module is off (modules.{} in fynkel.project.json)",
                module.key(),
                module.key()
            ),
        )
    }
    pub fn physics_mut(&mut self) -> &mut Physics {
        &mut self.physics
    }

    pub fn config(&self) -> &WorldConfig {
        &self.config
    }
    /// `WorldConfig::edit_mode`: the editor's edit-preview world.
    pub fn is_edit_mode(&self) -> bool {
        self.config.edit_mode
    }
    /// Edit mode only: the camera `extract()` reports instead of the
    /// scene's — the editor's own, at `world` with `camera`'s projection
    /// (perspective or orthographic, exposure included). `None` hands the
    /// picture back to the scene camera. Returns `false`, and changes
    /// nothing, in a game world.
    pub fn set_edit_camera(&mut self, camera: Option<(Mat4, Camera)>) -> bool {
        if !self.config.edit_mode {
            return false;
        }
        self.edit.camera = camera;
        true
    }
    pub fn edit_camera(&self) -> Option<&(Mat4, Camera)> {
        self.edit.camera.as_ref()
    }
    /// Edit mode only: the entities `extract()` leaves out — exactly this
    /// set (the caller passes the subtree it wants hidden), replacing the
    /// previous one. Dead entities are dropped. Returns `false` in a game
    /// world.
    pub fn set_edit_hidden(&mut self, entities: &[Entity]) -> bool {
        if !self.config.edit_mode {
            return false;
        }
        self.edit.hidden = entities
            .iter()
            .copied()
            .filter(|e| self.gameplay.ecs.contains(*e))
            .collect();
        true
    }
    /// Edit mode only: `step()` also runs the animators and resolves the
    /// skins — the Inspector's preview stages (ADR-0030), which seek an
    /// `Animator` to the time the editor shows and draw the skinned mesh
    /// posed. Every other system stays off. Returns `false` in a game
    /// world.
    pub fn set_edit_animation(&mut self, on: bool) -> bool {
        if !self.config.edit_mode {
            return false;
        }
        self.edit.animation = on;
        true
    }
    pub fn edit_hidden(&self) -> impl Iterator<Item = Entity> + '_ {
        self.edit.hidden.iter().copied()
    }
    pub fn id(&self) -> WorldId {
        self.gameplay.ecs.id()
    }
    pub fn clock(&self) -> &FrameClock {
        &self.clock
    }
    pub fn tick(&self) -> Tick {
        self.clock.tick()
    }
    pub fn alpha(&self) -> f32 {
        self.clock.alpha()
    }
    pub fn live_entities(&self) -> u32 {
        self.gameplay.ecs.len()
    }
    /// Tracked storage access for native systems and hosts; gameplay writes
    /// use the shared transaction API. ECS stamps cover storage changes, not
    /// scene/physics service invariants.
    pub fn ecs(&self) -> &Ecs {
        &self.gameplay.ecs
    }
    pub fn ecs_mut(&mut self) -> &mut Ecs {
        &mut self.gameplay.ecs
    }

    /// World-local cursor for committed identity/name reconciliation. Zero is
    /// reserved for a consumer's initial snapshot. None means a native write
    /// guard is still outstanding; consumers must not publish a cache then.
    pub fn identity_revision(&mut self) -> Option<u64> {
        let stamp = self.gameplay.ecs.component_stamp::<Name>()?;
        if self.identity_stamp != Some(stamp) {
            self.identity_revision = self
                .identity_revision
                .checked_add(1)
                .expect("identity revision exhausted");
            self.identity_stamp = Some(stamp);
        }
        Some(self.identity_revision)
    }

    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    /// Marks the world unusable after a caught panic (ADR-0014).
    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    // ----------------------------------------------------------------- frame

    /// Starts a host frame: accumulates `dt`, records `MissedDeadline` when time is dropped.
    pub fn begin_frame(&mut self, dt: impl Into<f64>) -> FramePlan {
        self.begin_frame_scaled(dt.into(), 1.0)
    }
    pub fn begin_frame_scaled(&mut self, dt: f64, time_scale: f64) -> FramePlan {
        let plan = self.clock.begin_frame_scaled(dt, time_scale);
        if plan.dropped_seconds > 0.0 {
            self.publication.diagnostics.push(Diagnostic {
                command_index: u32::MAX,
                lane: Lane::Simulation,
                code: DiagnosticCode::MISSED_DEADLINE,
                entity: Entity::NULL,
                component: ComponentId::NONE,
                message: format!(
                    "dropped {:.4}s after {} catch-up ticks",
                    plan.dropped_seconds, plan.steps
                ),
            });
            self.publication.events.push(Event {
                tick: self.clock.tick(),
                payload: EventPayload::MissedDeadline {
                    dropped_seconds: plan.dropped_seconds,
                },
            });
        }
        plan
    }

    // ----------------------------------------------------------------- input

    /// Compiles the project's actions into the binding table (a load does
    /// this from the cooked index; a host may rebind). Bindings that do
    /// not parse are skipped and returned.
    pub fn bind_input(&mut self, actions: &[ActionSpec]) -> Vec<(String, String, BindingError)> {
        self.input.bind(actions)
    }

    /// Applies a host frame's raw control events (once per frame, before
    /// the ticks). Edges wait for the next tick, held state is immediate.
    pub fn capture_input(&mut self, events: &[InputEvent]) {
        self.input.capture(events);
    }

    /// The bound actions as the next tick — and this frame — see them.
    pub fn input_view(&self) -> Vec<ActionState> {
        self.input.view()
    }

    /// [`World::input_view`] into `out`, reusing it (foundation 4.8).
    pub fn input_view_into(&self, out: &mut Vec<ActionState>) {
        self.input.view_into(out)
    }

    pub fn input(&self) -> &Input {
        &self.input
    }

    // -------------------------------------------------------------- commands

    fn reject(
        &mut self,
        index: u32,
        lane: Lane,
        code: DiagnosticCode,
        entity: Entity,
        component: ComponentId,
        message: impl Into<String>,
    ) -> bool {
        self.publication.diagnostics.push(Diagnostic {
            command_index: index,
            lane,
            code,
            entity,
            component,
            message: message.into(),
        });
        false
    }

    /// Records a diagnostic on behalf of a caller that rejected a command
    /// before it reached the world (e.g. the bridge on an unknown kind).
    pub fn push_diagnostic(
        &mut self,
        command_index: u32,
        lane: Lane,
        code: DiagnosticCode,
        entity: Entity,
        component: ComponentId,
        message: impl Into<String>,
    ) {
        self.reject(command_index, lane, code, entity, component, message);
    }

    fn check_entity(&mut self, index: u32, lane: Lane, e: Entity, what: &str) -> bool {
        if e.world() != self.gameplay.ecs.id() {
            return self.reject(
                index,
                lane,
                DiagnosticCode::WRONG_WORLD,
                e,
                ComponentId::NONE,
                format!("{what}: entity belongs to world {}", e.world().0),
            );
        }
        if !self.gameplay.ecs.contains(e) {
            return self.reject(
                index,
                lane,
                DiagnosticCode::STALE_ENTITY,
                e,
                ComponentId::NONE,
                format!("{what}: stale or unknown entity"),
            );
        }
        true
    }

    fn validate_audio_bus(&self, volume: f32) -> Result<(), String> {
        if !self.modules.audio {
            return Err("audio module is disabled".into());
        }
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(format!(
                "audioBus: volume must be within [0, 1], got {volume}"
            ));
        }
        Ok(())
    }

    /// Destroys `entity` and its subtree with the bookkeeping and the
    /// `Destroyed` events of `Command::Destroy` (also what an expired
    /// `LifetimeDespawn` does). Returns how many entities went.
    pub(crate) fn destroy_subtree(
        &mut self,
        entity: Entity,
        tick: Tick,
    ) -> Result<usize, SceneError> {
        let gone = scene::despawn_subtree(&mut self.gameplay.ecs, entity)?;
        let count = gone.len();
        for e in gone {
            self.publication.subscriptions.remove(&e);
            self.publication.events.push(Event {
                tick,
                payload: EventPayload::Destroyed { entity: e },
            });
        }
        Ok(count)
    }

    /// Data removal bookkeeping shared with the gameplay transaction boundary.
    /// Subsystem effects reconcile in their existing phases, not during staging.
    fn remove_component_data(&mut self, entity: Entity, component: ComponentId) -> Option<bool> {
        if component == ComponentId::AUDIO_SOURCE {
            // The voice stops at the phase (its source is gone).
            let _ = self.gameplay.ecs.remove::<AudioRun>(entity);
        }
        if component == ComponentId::OSCILLATE {
            // A later add starts a fresh motion clock/rest offset. A coalesced
            // remove/re-add within one edit remains the same final component.
            let _ = self.gameplay.ecs.remove::<catalogue::Oscillation>(entity);
        }
        if component == ComponentId::ANIMATOR {
            // A subsequent add is a new playback instance, even before a tick.
            let _ = self.gameplay.ecs.remove::<AnimatorRun>(entity);
            for effect in &mut self.gameplay.gameplay_effects {
                if let gameplay::CommittedEffect::Animation {
                    entity: owner,
                    invalidated,
                    ..
                } = effect
                {
                    if *owner == entity {
                        *invalidated = true;
                    }
                }
            }
        }
        ComponentValue::remove_from(&mut self.gameplay.ecs, entity, component)
    }

    // ------------------------------------------------------------------ step

    /// One fixed tick (§5.1). Delivers committed effects, runs the
    /// native phases, propagates transforms, publishes S[n+1], the tick's
    /// events and the snapshot.
    pub fn step(&mut self) -> StepReport {
        if self.poisoned {
            return StepReport {
                status: Status::Poisoned,
                tick: self.clock.tick(),
                applied: 0,
                rejected: 0,
                snapshot_records: 0,
                event_records: 0,
                diagnostic_records: 0,
                live_entities: self.gameplay.ecs.len(),
            };
        }
        let step_start = Instant::now();
        self.publication.diagnostics.clear();
        self.publication.events.begin_tick();
        let next_tick = self.clock.tick() + 1;
        let edit = self.config.edit_mode;
        // The native systems in their declared order (the compiled registered schedule).
        let run = self.run_tick(next_tick);
        let (applied, rejected) = (run.applied, run.rejected);

        // Every native system of the tick has read the same view the game
        // did (the rigs above read `look` last); this tick consumed the
        // pending edges and relative motion (§5.1), so catch-up ticks in the
        // same frame see held state only.
        if !edit {
            self.input.end_tick();
        }

        let tick = self.clock.consume_step();
        debug_assert_eq!(tick, next_tick);
        self.applied_this_tick = applied;

        let t0 = Instant::now();
        self.publish_snapshot();
        self.publication.events.publish_tick();
        self.publication.last_events = self.publication.events.pending() as u32;
        let records = self.publication.snapshot.len() as u32;
        self.set_time(SystemKind::Publish, records, micros(t0));
        self.set_time(
            SystemKind::Step,
            self.gameplay.ecs.len(),
            micros(step_start),
        );

        let diagnostic_records = self.publication.diagnostics.len() as u32;
        StepReport {
            status: if rejected == 0 {
                Status::Ok
            } else {
                Status::CommandsRejected
            },
            tick,
            applied,
            rejected,
            snapshot_records: self.publication.snapshot.len() as u32,
            event_records: self.publication.last_events,
            diagnostic_records,
            live_entities: self.gameplay.ecs.len(),
        }
    }

    /// Brings the derived `Inactive` markers up to date after a spawn,
    /// reparent or flag change (nothing to do otherwise).
    pub(crate) fn refresh_active(&mut self) {
        self.scene_sync.refresh_active(&mut self.gameplay.ecs);
    }

    pub(crate) fn propagate(&mut self) -> usize {
        self.scene_sync.propagate(&mut self.gameplay.ecs)
    }

    /// Work counters for qualification of sparse scene synchronization.
    pub fn scene_sync_candidates(&self) -> (usize, usize) {
        (
            self.scene_sync.transform_candidates,
            self.scene_sync.activity_candidates,
        )
    }

    /// Displays the completed simulation pose at any interpolation fraction.
    /// Hosts call this after a manual fixed sequence, before presentation.
    /// Leaves clocks and the accumulator unchanged; the next tick records a
    /// fresh interpolation interval normally.
    pub fn snap_interpolation(&mut self) {
        self.record_prev_world();
    }

    /// Captures the interpolation source before host fixed callbacks publish
    /// gameplay edits. The next step consumes this boundary exactly once.
    pub fn prepare_gameplay_tick(&mut self) {
        if !self.gameplay.gameplay_tick_prepared {
            self.record_prev_world();
            self.gameplay.gameplay_tick_prepared = true;
        }
    }

    /// Abandons a prepared host tick without undoing already committed edits.
    /// Their current values remain visible, as for ordinary explicit edits.
    pub fn cancel_gameplay_tick(&mut self) {
        if std::mem::take(&mut self.gameplay.gameplay_tick_prepared) {
            self.record_prev_world();
        }
    }

    fn record_prev_world(&mut self) {
        let h = &mut self.gameplay.ecs;
        for (local, parent, prev) in
            h.query_mut::<(&Transform, Option<&Parent>, &mut PrevTransform)>()
        {
            *prev = PrevTransform(*local, parent.map(|p| p.0));
        }
    }

    /// Committed teleports reset interpolation history throughout the subtree
    /// so extraction cannot blend back toward an obsolete pose.
    fn apply_teleports(&mut self) {
        let roots = std::mem::take(&mut self.teleported);
        for root in roots {
            for e in scene::subtree(&self.gameplay.ecs, root) {
                let Ok(local) = self.gameplay.ecs.copy::<Transform>(e) else {
                    continue;
                };
                let parent = scene::parent_of(&self.gameplay.ecs, e);
                if let Ok(mut prev) = self.gameplay.ecs.get_mut::<PrevTransform>(e) {
                    *prev = PrevTransform(local, parent);
                }
            }
        }
    }

    /// Physics rejections become runtime diagnostics with no transport index.
    fn physics_rejections(&mut self) -> u32 {
        let mut count = 0;
        for r in self.physics.take_rejections() {
            let (code, component) = match r.kind {
                RejectionKind::MissingBody => {
                    (DiagnosticCode::MISSING_COMPONENT, ComponentId::RIGID_BODY)
                }
                RejectionKind::NotFinite if self.physics.character_state(r.entity).is_some() => (
                    DiagnosticCode::INVALID_PAYLOAD,
                    ComponentId::CHARACTER_CONTROLLER,
                ),
                RejectionKind::NotDynamic
                | RejectionKind::NotKinematic
                | RejectionKind::NotFinite => {
                    (DiagnosticCode::INVALID_PAYLOAD, ComponentId::RIGID_BODY)
                }
                RejectionKind::UnsupportedTransform => {
                    (DiagnosticCode::INVALID_PAYLOAD, ComponentId::TRANSFORM)
                }
                RejectionKind::MeshOnMovingBody => {
                    (DiagnosticCode::INVALID_PAYLOAD, ComponentId::COLLIDER)
                }
                RejectionKind::MeshUnavailable => {
                    (DiagnosticCode::MESH_UNAVAILABLE, ComponentId::COLLIDER)
                }
                // ADR-0025: the collider still collides, on its own
                // values; the loader's diagnostic says why the asset is
                // missing, this one says which collider it affects.
                RejectionKind::MaterialUnavailable => {
                    (DiagnosticCode::INVALID_PAYLOAD, ComponentId::COLLIDER)
                }
                RejectionKind::MissingCharacter => (
                    DiagnosticCode::MISSING_COMPONENT,
                    ComponentId::CHARACTER_CONTROLLER,
                ),
                RejectionKind::CharacterWithBody => (
                    DiagnosticCode::INVALID_PAYLOAD,
                    ComponentId::CHARACTER_CONTROLLER,
                ),
            };
            if r.index != u32::MAX {
                count += 1;
            }
            self.publication.diagnostics.push(Diagnostic {
                command_index: r.index,
                lane: Lane::Simulation,
                code,
                entity: r.entity,
                component,
                message: r.message,
            });
        }
        count
    }

    /// The characters' action driver (M3-09, the pre-tick slot): every
    /// `CharacterController` naming a `move_action` walks at its `speed`
    /// along that 2D action, relative to the camera rig targeting it (x
    /// right, y forward) or the world axes without one; a `jump_action`
    /// press jumps when grounded (the edge is consumed by this tick, §5.1).
    /// Queued like body commands, outside any batch (`u32::MAX`).
    fn drive_characters(&mut self) -> u32 {
        let mut cmds = std::mem::take(&mut self.scratch.driver);
        cmds.clear();
        let mut driven = 0;
        {
            let h = &self.gameplay.ecs;
            for (handle, c) in h
                .query::<(&Handle, &CharacterController)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                if c.move_action.is_empty() && c.jump_action.is_empty() {
                    continue;
                }
                driven += 1;
                let entity = handle.0;
                if !c.move_action.is_empty() {
                    if let Some(a) = self.input.action(&c.move_action) {
                        let mut v = glam::Vec2::new(a.x, a.y);
                        if !v.is_finite() {
                            v = glam::Vec2::ZERO;
                        }
                        if v.length_squared() > 1.0 {
                            v = v.normalize();
                        }
                        // The rig that targets this entity gives the frame.
                        let yaw = h
                            .query::<&ThirdPersonCamera>()
                            .iter()
                            .find(|r| r.target.get() == Some(entity))
                            .map(|r| r.yaw)
                            .unwrap_or(0.0);
                        let (sin, cos) = yaw.sin_cos();
                        let forward = Vec3::new(-sin, 0.0, -cos);
                        let right = Vec3::new(cos, 0.0, -sin);
                        let velocity = (right * v.x + forward * v.y) * c.speed;
                        cmds.push((entity, BodyCommand::Move(velocity)));
                    }
                }
                if !c.jump_action.is_empty()
                    && self.input.action(&c.jump_action).is_some_and(|a| a.pressed)
                    && self
                        .physics
                        .character_state(entity)
                        .is_some_and(|s| s.grounded)
                {
                    cmds.push((entity, BodyCommand::Jump(0.0)));
                }
            }
        }
        for (entity, cmd) in cmds.drain(..) {
            let rigid = self
                .gameplay
                .ecs
                .copy::<crate::RigidBody>(entity)
                .ok()
                .map(|body| body.body_type);
            if let Err((_, message)) =
                cmd.validate_for(rigid, self.gameplay.ecs.has::<CharacterController>(entity))
            {
                self.reject(
                    u32::MAX,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_PAYLOAD,
                    entity,
                    ComponentId::CHARACTER_CONTROLLER,
                    format!("character input: {message}"),
                );
                continue;
            }
            // Preserve pre-command driver order; the same typed validator is
            // used by gameplay effects and rechecked by the physics runtime.
            self.physics.queue(u32::MAX, entity, cmd);
        }
        self.scratch.driver = cmds;
        driven
    }

    /// The camera rigs (M3-09), after the tick's propagate: each
    /// `ThirdPersonCamera` turns by its `look_action`, orbits its target's
    /// pivot at `distance` (pulled in to the first collider a sphere cast
    /// from the pivot meets, never under `min_distance`) and looks at it;
    /// yaw/pitch are written back. Returns the rig count and whether any
    /// moved.
    fn place_camera_rigs(&mut self, tick: Tick) -> (u32, bool) {
        let mut placements = std::mem::take(&mut self.scratch.rigs);
        placements.clear();
        let dt = self.config.fixed_dt;
        let mut invalid = None;
        {
            let h = &self.gameplay.ecs;
            for (handle, rig) in h
                .query::<(&Handle, &ThirdPersonCamera)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                let entity = handle.0;
                if let Err(message) = rig.validate() {
                    invalid = Some(gameplay::EditError::InvalidComponent {
                        entity: entity.into(),
                        component: ComponentId::THIRD_PERSON_CAMERA,
                        message,
                    });
                    break;
                }
                let Some(target) = self.live(rig.target) else {
                    continue;
                };
                let Ok(target_world) = self.gameplay.ecs.get::<WorldTransform>(target) else {
                    continue;
                };
                let mut yaw = rig.yaw;
                let mut pitch = rig.pitch;
                if !rig.look_action.is_empty() {
                    if let Some(a) = self.input.action(&rig.look_action) {
                        if a.x.is_finite() && a.y.is_finite() {
                            yaw -= a.x * rig.sensitivity * dt;
                            pitch -= a.y * rig.sensitivity * dt;
                        }
                    }
                }
                yaw = yaw.rem_euclid(core::f32::consts::TAU);
                if yaw > core::f32::consts::PI {
                    yaw -= core::f32::consts::TAU;
                }
                pitch = pitch.clamp(rig.min_pitch, rig.max_pitch);
                let pivot = target_world.matrix.transform_point3(rig.pivot);
                let (sy, cy) = yaw.sin_cos();
                let (sp, cp) = pitch.sin_cos();
                let dir = Vec3::new(cp * sy, sp, cp * cy);
                let mut distance = rig.distance;
                // Without physics the rig does not pull in on walls.
                if rig.collision_radius > 0.0 && self.modules.physics {
                    self.scratch.hits.clear();
                    self.physics.query(
                        &QuerySpec {
                            kind: QueryKind::ShapeCast,
                            shape: fynkel_physics::ColliderShape::Sphere,
                            size: Vec3::new(rig.collision_radius, 0.0, 0.0),
                            origin: pivot,
                            rotation: Quat::IDENTITY,
                            direction: dir,
                            max_distance: rig.distance,
                            mask: rig.mask,
                            exclude: Some(target),
                            max_hits: 1,
                            hit_sensors: false,
                        },
                        &mut self.scratch.hits,
                    );
                    if let Some(hit) = self.scratch.hits.first() {
                        distance = hit.distance.clamp(rig.min_distance, rig.distance);
                    }
                }
                placements.push(RigPlacement {
                    rig: entity,
                    yaw,
                    pitch,
                    position: pivot + dir * distance,
                    rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch),
                });
            }
        }
        let rigs = placements.len() as u32;
        let result = if let Some(error) = invalid {
            Err(error)
        } else if placements.is_empty() {
            Ok(false)
        } else {
            let desired: Vec<_> = placements
                .iter()
                .map(|p| (p.rig, p.position, p.rotation))
                .collect();
            scene::prepare_world_poses(&self.gameplay.ecs, &desired)
                .map_err(|error| {
                    gameplay::EditError::InvalidEffect(Entity::NULL.into(), format!("{error:?}"))
                })
                .and_then(|poses| {
                    self.edit_native_data_at_tick(tick, |edit| {
                        for p in &placements {
                            let Some(gameplay::GameplayComponentValue::ThirdPersonCamera(mut rig)) =
                                edit.component(p.rig, ComponentId::THIRD_PERSON_CAMERA)?
                            else {
                                return Err(gameplay::EditError::MissingComponent(
                                    p.rig.into(),
                                    ComponentId::THIRD_PERSON_CAMERA,
                                ));
                            };
                            rig.yaw = p.yaw;
                            rig.pitch = p.pitch;
                            edit.set_component(
                                p.rig,
                                gameplay::GameplayComponentValue::ThirdPersonCamera(rig),
                            )?;
                        }
                        let mut moved = false;
                        for &(entity, pose) in &poses {
                            moved |= edit.local_transform(entity)? != pose;
                            edit.set_transform(entity, pose)?;
                        }
                        Ok(moved)
                    })
                    .map(|commit| commit.value)
                })
        };
        let moved = match result {
            Ok(moved) => moved,
            Err(error) => {
                self.native_pose_rejection(ComponentId::THIRD_PERSON_CAMERA, "camera rigs", error);
                false
            }
        };
        placements.clear();
        self.scratch.rigs = placements;
        (rigs, moved)
    }

    /// The step's contacts on the physics lane (readable by
    /// `after_physics_events` this tick, drained next).
    fn physics_events(&mut self, tick: Tick) {
        // A trigger pair with a `TriggerZone` on either side also fires the
        // zone's named event on the general lane (M3-12); `once` zones are
        // removed after their entry, collected while the events are read.
        let mut fired: Vec<Entity> = Vec::new();
        let mut emitted = Vec::new();
        for e in self.physics.events() {
            let payload = match *e {
                PhysicsEvent::CollisionEnter(c) => EventPayload::CollisionEnter(c),
                PhysicsEvent::CollisionStay(c) => EventPayload::CollisionStay(c),
                PhysicsEvent::CollisionExit(c) => EventPayload::CollisionExit(c),
                PhysicsEvent::TriggerEnter(c) => EventPayload::TriggerEnter(c),
                PhysicsEvent::TriggerExit(c) => EventPayload::TriggerExit(c),
                PhysicsEvent::TriggerStay(c) => EventPayload::TriggerStay(c),
                PhysicsEvent::ControllerHit(c) => EventPayload::ControllerHit(c),
            };
            emitted.push(Event { tick, payload });
            let (entered, c) = match *e {
                PhysicsEvent::TriggerEnter(c) => (true, c),
                PhysicsEvent::TriggerExit(c) => (false, c),
                _ => continue,
            };
            for (zone, other) in [(c.a, c.b), (c.b, c.a)] {
                if fired.contains(&zone) {
                    continue;
                }
                let remembered = self.zone_names.remove(&(zone, other));
                let (event, once) = match self.gameplay.ecs.get::<TriggerZone>(zone) {
                    Ok(z) => (z.event.clone(), z.once),
                    // Destroyed: its occupants still hear it go.
                    Err(_) => match remembered {
                        Some(event) if !entered => (event, false),
                        _ => continue,
                    },
                };
                if entered && !once {
                    self.zone_names.insert((zone, other), event.clone());
                }
                let payload = if entered {
                    EventPayload::ZoneEntered { zone, other, event }
                } else {
                    EventPayload::ZoneExited { zone, other, event }
                };
                emitted.push(Event { tick, payload });
                if entered && once {
                    fired.push(zone);
                }
            }
        }
        if !fired.is_empty() {
            let result = self.edit_native_data_at_tick(tick, |edit| {
                for &zone in &fired {
                    edit.remove_component(zone, ComponentId::TRIGGER_ZONE)?;
                }
                Ok(())
            });
            if let Err(error) = result {
                // Contact events describe completed solver work. A one-shot
                // named event is published only when its consumption commits.
                emitted.retain(|event| {
                    !matches!(&event.payload,
                    EventPayload::ZoneEntered { zone, .. } if fired.contains(zone))
                });
                self.reject(
                    u32::MAX,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_PAYLOAD,
                    Entity::NULL,
                    ComponentId::TRIGGER_ZONE,
                    format!("trigger zone output: {error}"),
                );
            }
        }
        for event in emitted {
            self.publication.events.push(event);
        }
    }

    fn seed_prev_world(&mut self) {
        let mut fresh = Vec::new();
        {
            let h = &self.gameplay.ecs;
            for (e, _wt, local, parent) in h
                .query::<fynkel_ecs::internal::hecs::Without<
                    (&Handle, &WorldTransform, &Transform, Option<&Parent>),
                    &PrevTransform,
                >>()
                .iter()
            {
                fresh.push((e.0, PrevTransform(*local, parent.map(|p| p.0))));
            }
        }
        let h = &mut self.gameplay.ecs;
        for (e, t) in fresh {
            let _ = h.insert(e, t);
        }
    }

    fn publish_snapshot(&mut self) {
        self.publication
            .publish(&self.gameplay.ecs, self.clock.tick());
    }

    /// Immutable, owned snapshot suitable for retaining or handing to another
    /// thread. No live World/ECS/native buffer reference crosses that boundary.
    pub fn published_snapshot(&self) -> (Tick, Arc<[SnapshotRecord]>) {
        (self.publication.tick, self.publication.snapshot.clone())
    }

    // --------------------------------------------------------------- reads

    /// Subscribes (or unsubscribes) entities for the transform snapshot.
    /// Unknown/stale handles are ignored; destroyed entities drop out.
    /// Direct native ECS destruction is reconciled at the next publication.
    pub fn subscribe(&mut self, entities: &[Entity], enable: bool) {
        for &e in entities {
            if enable {
                if self.gameplay.ecs.contains(e) {
                    self.publication.subscriptions.insert(e);
                }
            } else {
                self.publication.subscriptions.remove(&e);
            }
        }
    }

    pub fn subscriptions(&self) -> impl Iterator<Item = Entity> + '_ {
        self.publication.subscriptions.iter().copied()
    }

    pub fn subscription_count(&self) -> usize {
        self.publication.subscriptions.len()
    }

    /// Coherent S[n+1] of subscribed entities, in handle order.
    pub fn snapshot(&self) -> &[SnapshotRecord] {
        &self.publication.snapshot
    }

    /// Diagnostics since the last `step()` began (rejections from that step
    /// and from presentation submits after it).
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.publication.diagnostics
    }

    /// Pull-only drain: general events plus the last tick's physics events.
    pub fn drain_events(&mut self) -> Vec<Event> {
        self.publication.events.drain()
    }

    /// [`World::drain_events`] into `out` (cleared first), keeping both
    /// buffers' capacity (foundation 4.8).
    pub fn drain_events_into(&mut self, out: &mut Vec<Event>) {
        self.publication.events.drain_into(out)
    }

    /// Physics events of the tick just stepped (for `afterPhysics`).
    pub fn after_physics_events(&self) -> &[Event] {
        self.publication.events.after_physics()
    }

    /// Queues a general-lane event from a native system or host (e.g.
    /// `SceneReady` after a load completes).
    pub fn push_event(&mut self, payload: EventPayload) {
        self.publication.events.push(Event {
            tick: self.clock.tick(),
            payload,
        });
    }

    // -------------------------------------------------------------- extract

    /// Render extraction: every spatial entity's world matrix interpolated
    /// S[n]..S[n+1] by alpha. Persistent edits share the authoritative pose.
    pub fn extract(&self) -> Extract {
        let mut out = Extract::default();
        self.extract_into(&mut out, true);
        out
    }

    /// Fill an owned snapshot, retaining its buffers for the next frame.
    /// `placeholders` includes empty spatial objects for editor/debug drawing.
    /// Otherwise only render component owners are published; ancestors and
    /// skin joints are resolved as dependencies without extra draw records.
    /// The exclusive output borrow prevents mutation while a renderer reads it.
    pub fn extract_into(&self, out: &mut Extract, placeholders: bool) {
        let mut membership = self.render_membership.lock().unwrap();
        membership.refresh(&self.gameplay.ecs);
        let alpha = self.clock.alpha();
        let mut memo = std::mem::take(&mut membership.memo);
        memo.clear();
        out.tick = self.clock.tick();
        out.alpha = alpha;
        out.time = self.clock.gameplay_elapsed() as f32;
        out.records.clear();
        out.meshes.clear();
        out.joints.clear();
        out.directional_lights.clear();
        out.point_lights.clear();
        out.spot_lights.clear();
        out.volumes.clear();
        out.camera = None;
        out.environment = None;
        let entities = if placeholders {
            &membership.entities
        } else {
            &membership.renderable
        };
        for &e in entities {
            if !self.gameplay.ecs.has::<WorldTransform>(e) {
                continue;
            }
            if self.gameplay.ecs.has::<scene::Inactive>(e) {
                // Not active in the hierarchy: as absent as a hidden one.
                continue;
            }
            if self.config.edit_mode && self.edit.hidden.contains(&e) {
                // The editor's hidden flag: no record at all — nothing to
                // draw, light with or look through.
                continue;
            }
            let world = self.resolve_extract(e, alpha, &mut memo);
            let record = out.records.len() as u32;
            out.records.push(ExtractRecord {
                entity: e,
                world,
                layer_bit: self
                    .gameplay
                    .ecs
                    .copy::<Layer>(e)
                    .map_or(1, |l| l.bit().max(1)),
            });
            if let Ok(m) = self.gameplay.ecs.get::<MeshRenderer>(e) {
                if m.visible {
                    // A skinned draw's palette from the joints' own
                    // interpolated matrices (M3-19).
                    let skin = self.skin_joints(e).map(|(skin, joints)| {
                        let first_joint = out.joints.len() as u32;
                        let mut world_of = |j: Entity| {
                            self.gameplay
                                .ecs
                                .has::<WorldTransform>(j)
                                .then(|| self.resolve_extract(j, alpha, &mut memo))
                        };
                        World::skin_palette(world, &skin, &joints, &mut world_of, &mut out.joints);
                        ExtractSkin {
                            first_joint,
                            joint_count: skin.joints.len() as u32,
                        }
                    });
                    out.meshes.push(ExtractMesh {
                        record,
                        mesh: m.mesh.clone(),
                        material: m.material.clone(),
                        cast_shadows: m.cast_shadows,
                        receive_shadows: m.receive_shadows,
                        skin,
                    });
                }
            }
            if let Ok(c) = self.gameplay.ecs.copy::<Camera>(e) {
                let better = match &out.camera {
                    Some(cur) => c.priority > cur.camera.priority,
                    None => true,
                };
                if better {
                    out.camera = Some(ExtractCamera {
                        record,
                        world,
                        camera: c,
                    });
                }
            }
            if let Ok(l) = self.gameplay.ecs.copy::<DirectionalLight>(e) {
                let direction = world.transform_vector3(-Vec3::Z).normalize_or(-Vec3::Z);
                out.directional_lights.push(ExtractDirectionalLight {
                    record,
                    direction,
                    light: l,
                });
            }
            if let Ok(l) = self.gameplay.ecs.copy::<PointLight>(e) {
                out.point_lights.push(ExtractPointLight {
                    record,
                    position: world.transform_point3(Vec3::ZERO),
                    light: l,
                });
            }
            if let Ok(l) = self.gameplay.ecs.copy::<SpotLight>(e) {
                out.spot_lights.push(ExtractSpotLight {
                    record,
                    position: world.transform_point3(Vec3::ZERO),
                    direction: world.transform_vector3(-Vec3::Z).normalize_or(-Vec3::Z),
                    light: l,
                });
            }
            if let Ok(v) = self.gameplay.ecs.get::<Volume>(e) {
                if v.enabled && v.weight > 0.0 {
                    out.volumes.push(ExtractVolume {
                        record,
                        world,
                        volume: v.clone(),
                    });
                }
            }
            if out.environment.is_none() {
                if let Ok(env) = self.gameplay.ecs.get::<Environment>(e) {
                    out.environment = Some(ExtractEnvironment {
                        record,
                        environment: env.clone(),
                    });
                }
            }
        }
        if self.config.edit_mode {
            if let Some((world, camera)) = &self.edit.camera {
                // The editor looks through its own camera: no record backs
                // it (`u32::MAX`), the renderer reads `world` and `camera`.
                out.camera = Some(ExtractCamera {
                    record: u32::MAX,
                    world: *world,
                    camera: *camera,
                });
            }
        }
        membership.memo = memo;
    }

    fn resolve_extract(&self, e: Entity, alpha: f32, memo: &mut HashMap<Entity, Mat4>) -> Mat4 {
        if let Some(r) = memo.get(&e) {
            return *r;
        }
        let parent = self.gameplay.ecs.get::<Parent>(e).ok().map(|p| p.0);
        let parent_res = parent.map(|p| self.resolve_extract(p, alpha, memo));
        let result = {
            let cur = self
                .gameplay
                .ecs
                .get::<WorldTransform>(e)
                .map(|w| w.matrix)
                .unwrap_or(Mat4::IDENTITY);
            let local = self.gameplay.ecs.copy::<Transform>(e).unwrap_or_default();
            match self.gameplay.ecs.copy::<PrevTransform>(e) {
                // Reparenting changes coordinate space: snap this entity to
                // the current hierarchy rather than mixing unrelated locals.
                Ok(prev) if prev.1 != parent => cur,
                Ok(prev) => {
                    let pw = parent_res.unwrap_or(Mat4::IDENTITY);
                    pw * lerp(prev.0, local, alpha).matrix()
                }
                _ => cur,
            }
        };
        memo.insert(e, result);
        result
    }

    pub fn render_membership_candidates(&self) -> usize {
        self.render_membership.lock().unwrap().examined
    }

    /// Total events published by the last step (test/stat hook).
    pub fn last_event_count(&self) -> u32 {
        self.publication.last_events
    }
}

fn lerp(a: Transform, b: Transform, t: f32) -> Transform {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    Transform {
        position: a.position.lerp(b.position, t),
        rotation: a.rotation.slerp(b.rotation, t),
        scale: a.scale.lerp(b.scale, t),
    }
}

#[cfg(all(test, feature = "physics"))]
mod native_zone_tests {
    use super::*;
    use crate::Collider;

    fn overlapping_zone() -> (World, Entity) {
        let mut world = World::new(WorldConfig::default());
        let zone = scene::spawn(&mut world.gameplay.ecs, Transform::IDENTITY, None).unwrap();
        world
            .gameplay
            .ecs
            .insert(
                zone,
                Collider {
                    is_sensor: true,
                    ..Collider::default()
                },
            )
            .unwrap();
        world
            .gameplay
            .ecs
            .insert(
                zone,
                TriggerZone {
                    event: "once".into(),
                    once: true,
                },
            )
            .unwrap();
        let other = scene::spawn(&mut world.gameplay.ecs, Transform::IDENTITY, None).unwrap();
        world
            .gameplay
            .ecs
            .insert(other, Collider::default())
            .unwrap();
        world
            .gameplay
            .ecs
            .insert(other, crate::RigidBody::default())
            .unwrap();
        (world, zone)
    }

    #[test]
    fn native_once_zone_consumption_publishes_before_its_named_event() {
        let (mut world, zone) = overlapping_zone();
        world.step();
        assert!(!world.gameplay.ecs.has::<TriggerZone>(zone));
        let events = world.drain_events();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.payload,
            EventPayload::ZoneEntered { zone: entered, .. } if entered == zone))
                .count(),
            1
        );
        world.step();
        assert!(!world
            .drain_events()
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ZoneEntered { .. })));
    }

    #[test]
    fn rejected_once_zone_consumption_preserves_configuration_and_contact_events() {
        let (mut world, zone) = overlapping_zone();
        world.gameplay.next_gameplay_edit = u64::MAX;
        world.step();
        assert!(world.gameplay.ecs.has::<TriggerZone>(zone));
        let events = world.drain_events();
        assert!(events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::TriggerEnter(_))));
        assert!(!events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ZoneEntered { .. })));
        assert!(world
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("trigger zone output")));
    }
}
