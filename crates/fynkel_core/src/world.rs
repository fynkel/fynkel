//! The runtime world: lifecycle, command lanes, `step()`, snapshots,
//! events, diagnostics, render extraction.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

use fynkel_audio::{AudioBus, AudioListener, AudioSource, PlayParams};
use fynkel_contracts::{
    AssetRef, ComponentId, DiagnosticCode, Entity, Status, Tick, Uuid, WorldId, DEFAULT_FIXED_DT,
    DEFAULT_MAX_STEPS_PER_FRAME, MAX_FRAME_DT,
};
use fynkel_ecs::internal::Handle;
use fynkel_ecs::{EcsError, World as Ecs};
pub use fynkel_physics::PhysicsOwned;
use fynkel_physics::{
    BodyCommand, CharacterController, Collider, Physics, PhysicsEvent, QueryKind, QuerySpec,
    RejectionKind, RigidBody,
};
use fynkel_scene::{self as scene, Layer, Parent, SceneError, Tags, Transform, WorldTransform};
use fynkel_schema_derive::Schema;
use glam::{Mat4, Quat, Vec3};

use crate::animation::Pose;
use crate::clock::{FrameClock, FramePlan};
use crate::command::{Command, Lane};
use crate::components::{
    Animator, Camera, ComponentValue, DirectionalLight, Emitter, Environment, Follow,
    LifetimeDespawn, LookAtCamera, MeshRenderer, Oscillate, PointLight, Rotator, Skin, SpotLight,
    ThirdPersonCamera, TriggerZone, Volume,
};
use crate::event::{Event, EventPayload, EventQueue};
use crate::input::{ActionSpec, ActionState, BindingError, Input, InputEvent};
use crate::load::{LoadedScene, SceneDescription};
use crate::schema::{SystemKind, SystemTimeRecord};

mod animation;
mod audio;
mod catalogue;
mod controller;
mod ik;
pub use animation::{AnimatorRun, Fade, SkinRun};
pub use audio::AudioRun;
pub use catalogue::{EmitterRun, Oscillation};
pub use controller::ControllerPlay;
mod projection;
pub use projection::{
    projection_matrix, view_matrix, ProjectionInfo, MAX_PROJECTION_ANCHORS, PROJECTED_DROPPED,
    PROJECTED_IN_FRONT, PROJECTED_MISSING, PROJECTED_NO_CAMERA, PROJECTED_OCCLUDED,
    PROJECTED_ON_SCREEN, PROJECTED_VISIBLE,
};

#[derive(Clone, Debug)]
pub struct WorldConfig {
    pub id: WorldId,
    pub fixed_dt: f32,
    pub max_steps_per_frame: u32,
    pub max_frame_dt: f32,
    /// The editor's edit-preview world (ADR-0016, blueprint §19.2e): a
    /// `step()` applies the staged commands, propagates transforms and
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
            id: WorldId(1),
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
    pub value: String,
}

impl Name {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

/// World transform at the start of the last tick (S[n]) for interpolation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PrevWorld(Transform);

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

/// A command rejected before it reached the world (bridge decode failure).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub code: DiagnosticCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubmitReport {
    pub status: Status,
    /// Simulation lane: staged. Presentation lane: applied.
    pub accepted: u32,
    pub rejected: u32,
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
    /// True when a presentation override replaced interpolation this frame.
    pub overridden: bool,
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
    /// Simulated seconds at this extract, interpolated between ticks
    /// (`tick * fixed_dt` + `alpha * fixed_dt`): a paused world's time
    /// stands still. Shaders read it as `globals.time` (ADR-0031).
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
    config: WorldConfig,
    clock: FrameClock,
    ecs: Ecs,
    /// Simulation commands for the next tick with their batch index and
    /// whether they were staged by the presentation lane. An `Err` is a
    /// command the caller could not decode: it is reported as rejected at
    /// the tick boundary so its diagnostic sits with the batch's others.
    staged: Vec<(u32, Result<Command, Rejected>, bool)>,
    /// The tick's spawns by temp id, what a temp reference
    /// (`Entity::temp`) in a later command of the same tick resolves to.
    spawned_this_tick: HashMap<u64, Entity>,
    /// A spawn, reparent or active flag changed since the last
    /// `scene::refresh_active`.
    active_dirty: bool,
    /// Entities whose next tick must not interpolate from S[n] (their
    /// authoritative value came from a presentation override already shown).
    teleported: Vec<Entity>,
    /// Presentation-lane transform overrides for the current frame.
    overrides: HashMap<Entity, Transform>,
    events: EventQueue,
    subscriptions: BTreeSet<Entity>,
    snapshot: Vec<SnapshotRecord>,
    diagnostics: Vec<Diagnostic>,
    last_events: u32,
    applied_this_tick: u32,
    poisoned: bool,
    /// The binding table and raw control state (§5.1 input rules).
    input: Input,
    /// The Rapier adapter (M3-08), stepped at the §5.1 physics slot.
    physics: Physics,
    /// The optional modules the project uses (ADR-0029 §3): one that is
    /// off runs no systems and its commands are rejected.
    modules: crate::Modules,
    /// Named-target component (`ThirdPersonCamera`, `Follow`) → the entity
    /// its `target` name resolved to (M3-09); re-resolved when that entity
    /// dies or is renamed.
    named_targets: HashMap<Entity, Entity>,
    /// Prefab descriptions `Emitter`s may instantiate (M3-10).
    prefabs: HashMap<Uuid, Arc<SceneDescription>>,
    /// Clips `Animator`s may play and skins `Skin`s deform with (M3-19).
    clips: animation::ClipTable,
    /// Blend spaces an `Animator` may play in a clip's place (M3-19c).
    blends: animation::BlendTable,
    /// Animator Controllers an `Animator` may run (ADR-0032).
    controllers: controller::ControllerTable,
    skins: animation::SkinTable,
    /// The audio engine (M3-11): clips, voices, buses; the device the host
    /// opened.
    audio: fynkel_audio::Audio,
    /// The scenes this world holds, in load order (M3-12): what a replace
    /// load or `unload_scene` takes down.
    pub(crate) scenes: Vec<LoadedScene>,
    /// Edit mode only (M4-06): the editor camera `extract()` reports in
    /// the scene camera's place — its world matrix and the camera
    /// component it renders with.
    edit_camera: Option<(Mat4, Camera)>,
    /// Edit mode only: entities `extract()` leaves out (their meshes,
    /// lights and camera) — the editor's hidden flag, never the document's.
    edit_hidden: HashSet<Entity>,
    /// [`World::set_edit_animation`].
    edit_animation: bool,
    /// The last step's per-phase wall time, one entry per `SystemKind`.
    system_times: Vec<SystemTimeRecord>,
    /// Scratch of the native systems, kept for its capacity.
    rig_scratch: Vec<RigPlacement>,
    driver_scratch: Vec<(Entity, BodyCommand)>,
    hits_scratch: Vec<fynkel_physics::Hit>,
    /// Each occupied zone's event name by `(zone, occupant)`, so the exit
    /// still fires when the zone itself is destroyed (foundation 2.6).
    zone_names: BTreeMap<(Entity, Entity), String>,
    entity_scratch: Vec<Entity>,
    emission_scratch: Vec<catalogue::Emission>,
    placement_scratch: Vec<catalogue::Placement>,
    #[cfg_attr(not(feature = "animation"), allow(dead_code))]
    animator_scratch: Vec<animation::AnimatorTick>,
    /// The animators of a tick, (depth, entity), ancestor-first.
    #[cfg_attr(not(feature = "animation"), allow(dead_code))]
    animator_order_scratch: Vec<(u32, Entity)>,
    #[cfg_attr(not(feature = "animation"), allow(dead_code))]
    pose_scratch: Pose,
    #[cfg_attr(not(feature = "animation"), allow(dead_code))]
    fade_pose_scratch: Pose,
    #[cfg_attr(not(feature = "animation"), allow(dead_code))]
    layer_pose_scratch: Pose,
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
        let clock = FrameClock::new(
            config.fixed_dt,
            config.max_steps_per_frame,
            config.max_frame_dt,
        );
        let input = Input::with_tick_dt(config.fixed_dt);
        Self {
            ecs: Ecs::new(config.id),
            config,
            clock,
            staged: Vec::new(),
            spawned_this_tick: HashMap::new(),
            active_dirty: false,
            teleported: Vec::new(),
            overrides: HashMap::new(),
            events: EventQueue::default(),
            subscriptions: BTreeSet::new(),
            snapshot: Vec::new(),
            diagnostics: Vec::new(),
            last_events: 0,
            applied_this_tick: 0,
            poisoned: false,
            input,
            physics: Physics::new(),
            modules: crate::Modules::COMPILED,
            named_targets: HashMap::new(),
            prefabs: HashMap::new(),
            clips: HashMap::new(),
            blends: HashMap::new(),
            controllers: HashMap::new(),
            skins: HashMap::new(),
            audio: fynkel_audio::Audio::new(),
            scenes: Vec::new(),
            edit_camera: None,
            edit_hidden: HashSet::new(),
            edit_animation: false,
            system_times: SYSTEMS
                .iter()
                .map(|&system| SystemTimeRecord {
                    system,
                    entities: 0,
                    micros: 0,
                })
                .collect(),
            rig_scratch: Vec::new(),
            driver_scratch: Vec::new(),
            hits_scratch: Vec::new(),
            zone_names: BTreeMap::new(),
            entity_scratch: Vec::new(),
            emission_scratch: Vec::new(),
            placement_scratch: Vec::new(),
            animator_scratch: Vec::new(),
            animator_order_scratch: Vec::new(),
            pose_scratch: Pose::default(),
            fade_pose_scratch: Pose::default(),
            layer_pose_scratch: Pose::default(),
        }
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
    /// (`instantiate`, a clone, `apply_now`) join the query tree, so a
    /// query sees them at once, as Unity's (foundation 2.7). Nothing is
    /// simulated.
    pub fn refresh_physics(&mut self) {
        if self.modules.physics {
            self.physics.refresh(&mut self.ecs);
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
        self.edit_camera = camera;
        true
    }
    pub fn edit_camera(&self) -> Option<&(Mat4, Camera)> {
        self.edit_camera.as_ref()
    }
    /// Edit mode only: the entities `extract()` leaves out — exactly this
    /// set (the caller passes the subtree it wants hidden), replacing the
    /// previous one. Dead entities are dropped. Returns `false` in a game
    /// world.
    pub fn set_edit_hidden(&mut self, entities: &[Entity]) -> bool {
        if !self.config.edit_mode {
            return false;
        }
        self.edit_hidden = entities
            .iter()
            .copied()
            .filter(|e| self.ecs.contains(*e))
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
        self.edit_animation = on;
        true
    }
    pub fn edit_hidden(&self) -> impl Iterator<Item = Entity> + '_ {
        self.edit_hidden.iter().copied()
    }
    pub fn id(&self) -> WorldId {
        self.config.id
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
        self.ecs.len()
    }
    /// Direct storage access for native systems and hosts; gameplay goes
    /// through commands.
    pub fn ecs(&self) -> &Ecs {
        &self.ecs
    }
    pub fn ecs_mut(&mut self) -> &mut Ecs {
        &mut self.ecs
    }

    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    /// Marks the world unusable after a caught panic (ADR-0014).
    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    // ----------------------------------------------------------------- frame

    /// Starts a host frame: clears last frame's presentation overrides,
    /// accumulates `dt`, records `MissedDeadline` when time is dropped.
    pub fn begin_frame(&mut self, dt: f32) -> FramePlan {
        self.overrides.clear();
        let plan = self.clock.begin_frame(dt);
        if plan.dropped_seconds > 0.0 {
            self.diagnostics.push(Diagnostic {
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
            self.events.push(Event {
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

    pub fn input(&self) -> &Input {
        &self.input
    }

    // -------------------------------------------------------------- commands

    /// Submits a batch. Simulation commands are staged for the next tick and
    /// only validated then; presentation commands are validated and applied
    /// now. Rejections are in [`World::diagnostics`].
    pub fn submit(&mut self, lane: Lane, commands: Vec<Command>) -> SubmitReport {
        self.submit_indexed(
            lane,
            commands
                .into_iter()
                .enumerate()
                .map(|(i, c)| (i as u32, Ok(c)))
                .collect(),
        )
    }

    /// [`World::submit`] with explicit batch indexes, for callers that
    /// decode a batch and must keep diagnostics aligned with it. `Err`
    /// entries are rejected (now for presentation, at the tick for
    /// simulation) with the given code and message.
    pub fn submit_indexed(
        &mut self,
        lane: Lane,
        commands: Vec<(u32, Result<Command, Rejected>)>,
    ) -> SubmitReport {
        if self.poisoned {
            return SubmitReport {
                status: Status::Poisoned,
                accepted: 0,
                rejected: commands.len() as u32,
            };
        }
        let mut accepted = 0;
        let mut rejected = 0;
        match lane {
            Lane::Simulation => {
                accepted = commands.len() as u32;
                self.staged
                    .extend(commands.into_iter().map(|(i, c)| (i, c, false)));
            }
            Lane::Presentation => {
                for (i, cmd) in commands {
                    let ok = match cmd {
                        Ok(cmd) => self.apply_presentation(i, cmd),
                        Err(r) => {
                            self.reject(i, lane, r.code, Entity::NULL, ComponentId::NONE, r.message)
                        }
                    };
                    if ok {
                        accepted += 1;
                    } else {
                        rejected += 1;
                    }
                }
            }
        }
        SubmitReport {
            status: if rejected == 0 {
                Status::Ok
            } else {
                Status::CommandsRejected
            },
            accepted,
            rejected,
        }
    }

    /// Applies `commands` now, at the current tick, without stepping: the
    /// editor's live Play mode (ADR-0021 P7) changing a running — possibly
    /// paused — game. Each command goes through the simulation lane's
    /// checks, transforms are propagated, and what moved is not
    /// interpolated from where it was. No system runs, the tick does not
    /// advance, no event is emitted for the edit itself. `Spawn` is refused
    /// (its temp id answers through a step's event; a live creation is
    /// [`World::spawn_now`]). Returns the
    /// rejections, which are also in [`World::diagnostics`].
    pub fn apply_now(&mut self, commands: Vec<Command>) -> Vec<Diagnostic> {
        if self.poisoned {
            return Vec::new();
        }
        let first = self.diagnostics.len();
        let tick = self.clock.tick();
        let mut moved = Vec::new();
        for (i, cmd) in commands.into_iter().enumerate() {
            let index = i as u32;
            if matches!(cmd, Command::Spawn { .. }) {
                self.reject(
                    index,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_PAYLOAD,
                    Entity::NULL,
                    ComponentId::NONE,
                    "apply_now does not spawn",
                );
                continue;
            }
            let target = cmd.target();
            if self.apply_simulation(index, cmd, tick) {
                moved.extend(target);
            }
        }
        self.refresh_active();
        scene::propagate(&mut self.ecs);
        for e in moved {
            if self.ecs.contains(e) {
                self.overrides.remove(&e);
                self.teleported.push(e);
            }
        }
        self.apply_teleports();
        self.diagnostics[first..].to_vec()
    }

    fn reject(
        &mut self,
        index: u32,
        lane: Lane,
        code: DiagnosticCode,
        entity: Entity,
        component: ComponentId,
        message: impl Into<String>,
    ) -> bool {
        self.diagnostics.push(Diagnostic {
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
        if let Some(t) = e.temp_id() {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_TEMP_ID,
                e,
                ComponentId::NONE,
                format!("{what}: temp reference #{t} is only valid on the simulation lane"),
            );
        }
        if e.world() != self.config.id {
            return self.reject(
                index,
                lane,
                DiagnosticCode::WRONG_WORLD,
                e,
                ComponentId::NONE,
                format!("{what}: entity belongs to world {}", e.world().0),
            );
        }
        if !self.ecs.contains(e) {
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

    fn apply_presentation(&mut self, index: u32, cmd: Command) -> bool {
        let lane = Lane::Presentation;
        if !self.module_allows(index, lane, &cmd) {
            return false;
        }
        if !cmd.allowed_on_presentation() {
            return self.reject(
                index,
                lane,
                DiagnosticCode::WRONG_LANE,
                cmd.target().unwrap_or(Entity::NULL),
                ComponentId::NONE,
                "structural/simulation command on the presentation lane",
            );
        }
        match cmd {
            Command::SetTransform { entity, transform } => {
                if !self.check_entity(index, lane, entity, "set_transform") {
                    return false;
                }
                if !transform.is_finite() {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::INVALID_PAYLOAD,
                        entity,
                        ComponentId::TRANSFORM,
                        "transform is not finite",
                    );
                }
                if self.ecs.has::<PhysicsOwned>(entity) {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::PHYSICS_OWNED,
                        entity,
                        ComponentId::TRANSFORM,
                        "transform is physics-owned; use simulation commands",
                    );
                }
                if !self.ecs.has::<Transform>(entity) {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::MISSING_COMPONENT,
                        entity,
                        ComponentId::TRANSFORM,
                        "entity has no Transform",
                    );
                }
                // Override this frame's extract and become authoritative at
                // the next tick (ADR-0008).
                self.overrides.insert(entity, transform);
                self.staged
                    .push((index, Ok(Command::SetTransform { entity, transform }), true));
                true
            }
            Command::SetName { entity, name } => {
                if !self.check_entity(index, lane, entity, "set_name") {
                    return false;
                }
                self.ecs.insert(entity, Name::new(name)).is_ok()
            }
            Command::SetComponent {
                entity,
                value: ComponentValue::Transform(transform),
            } => self.apply_presentation(index, Command::SetTransform { entity, transform }),
            // Plain-data writes (§5.1: light colour, visibility, camera
            // parameters) apply immediately; nothing interpolates them.
            Command::SetComponent { entity, value } => {
                self.set_component(index, lane, entity, value)
            }
            Command::PlaySound {
                clip,
                bus,
                volume,
                pitch,
                priority,
            } => self.apply_play_sound(
                index,
                lane,
                clip,
                PlayParams {
                    bus,
                    volume,
                    pitch,
                    looping: false,
                    priority,
                },
            ),
            Command::AudioBus { bus, volume, muted } => {
                self.apply_audio_bus(index, lane, bus, volume, muted)
            }
            _ => unreachable!("allowed_on_presentation"),
        }
    }

    /// Replaces every temp reference (`Entity::temp`, M3-08b) in `cmd`
    /// with the entity a spawn earlier in this tick produced; `None` (and a
    /// diagnostic) when no applied spawn carried that temp id.
    fn resolve_temps(&mut self, index: u32, mut cmd: Command) -> Option<Command> {
        let mut failed = None;
        let mut resolve = |e: &mut Entity| {
            if let Some(t) = e.temp_id() {
                match self.spawned_this_tick.get(&(t as u64)) {
                    Some(real) => *e = *real,
                    None => failed = Some(t),
                }
            }
        };
        match &mut cmd {
            Command::Spawn { parent, .. } => {
                if let Some(p) = parent {
                    resolve(p);
                }
            }
            Command::Destroy(e) => resolve(e),
            Command::SetTransform { entity, .. }
            | Command::SetName { entity, .. }
            | Command::SetActive { entity, .. }
            | Command::SetComponent { entity, .. }
            | Command::RemoveComponent { entity, .. }
            | Command::Body { entity, .. }
            | Command::Animate { entity, .. }
            | Command::Audio { entity, .. } => resolve(entity),
            Command::SetParent { child, parent, .. } => {
                resolve(child);
                if let Some(p) = parent {
                    resolve(p);
                }
            }
            Command::PlaySound { .. } | Command::AudioBus { .. } => {}
        }
        match failed {
            None => Some(cmd),
            Some(t) => {
                self.reject(
                    index,
                    Lane::Simulation,
                    DiagnosticCode::INVALID_TEMP_ID,
                    Entity::temp(t),
                    ComponentId::NONE,
                    format!("temp reference #{t}: no spawn of this tick, earlier in its batch, carries that temp id"),
                );
                None
            }
        }
    }

    pub(crate) fn apply_simulation(&mut self, index: u32, cmd: Command, tick: Tick) -> bool {
        let lane = Lane::Simulation;
        if !self.module_allows(index, lane, &cmd) {
            return false;
        }
        match cmd {
            Command::Spawn {
                temp_id,
                transform,
                parent,
                name,
            } => {
                if temp_id == 0 {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::INVALID_TEMP_ID,
                        Entity::NULL,
                        ComponentId::NONE,
                        "spawn temp id must be non-zero",
                    );
                }
                if !transform.is_finite() {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::INVALID_PAYLOAD,
                        Entity::NULL,
                        ComponentId::TRANSFORM,
                        "transform is not finite",
                    );
                }
                if let Some(p) = parent {
                    if !self.check_entity(index, lane, p, "spawn parent") {
                        return false;
                    }
                }
                let e = match scene::spawn(&mut self.ecs, transform, parent) {
                    Ok(e) => e,
                    Err(err) => return self.reject_scene(index, lane, Entity::NULL, err),
                };
                if let Some(n) = name {
                    let _ = self.ecs.insert(e, Name::new(n));
                }
                self.spawned_this_tick.insert(temp_id, e);
                self.active_dirty |= parent.is_some();
                self.events.push(Event {
                    tick,
                    payload: EventPayload::Spawned { temp_id, entity: e },
                });
                true
            }
            Command::Destroy(entity) => {
                if !self.check_entity(index, lane, entity, "destroy") {
                    return false;
                }
                match self.destroy_subtree(entity, tick) {
                    Ok(_) => true,
                    Err(err) => self.reject_scene(index, lane, entity, err),
                }
            }
            Command::SetTransform { entity, transform } => {
                if !self.check_entity(index, lane, entity, "set_transform") {
                    return false;
                }
                if !transform.is_finite() {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::INVALID_PAYLOAD,
                        entity,
                        ComponentId::TRANSFORM,
                        "transform is not finite",
                    );
                }
                match scene::set_local(&self.ecs, entity, transform) {
                    Ok(()) => true,
                    Err(err) => self.reject_scene(index, lane, entity, err),
                }
            }
            Command::SetParent {
                child,
                parent,
                keep_world,
            } => {
                if !self.check_entity(index, lane, child, "set_parent child") {
                    return false;
                }
                if let Some(p) = parent {
                    if !self.check_entity(index, lane, p, "set_parent parent") {
                        return false;
                    }
                }
                let r = if keep_world {
                    // Cached world matrices must reflect earlier commands in
                    // this batch.
                    scene::propagate(&mut self.ecs);
                    scene::set_parent_keep_world(&mut self.ecs, child, parent)
                } else {
                    scene::set_parent_keep_local(&mut self.ecs, child, parent)
                };
                match r {
                    Ok(()) => {
                        self.active_dirty = true;
                        self.events.push(Event {
                            tick,
                            payload: EventPayload::ParentChanged { child, parent },
                        });
                        true
                    }
                    Err(err) => self.reject_scene(index, lane, child, err),
                }
            }
            Command::SetName { entity, name } => {
                if !self.check_entity(index, lane, entity, "set_name") {
                    return false;
                }
                self.ecs.insert(entity, Name::new(name)).is_ok()
            }
            Command::SetActive { entity, active } => {
                if !self.check_entity(index, lane, entity, "set_active") {
                    return false;
                }
                match scene::set_active(&mut self.ecs, entity, active) {
                    Ok(changed) => {
                        self.active_dirty |= changed;
                        true
                    }
                    Err(err) => self.reject_scene(index, lane, entity, err),
                }
            }
            Command::SetComponent {
                entity,
                value: ComponentValue::Transform(transform),
            } => self.apply_simulation(index, Command::SetTransform { entity, transform }, tick),
            Command::SetComponent { entity, value } => {
                self.set_component(index, lane, entity, value)
            }
            Command::RemoveComponent { entity, component } => {
                if !self.check_entity(index, lane, entity, "remove_component") {
                    return false;
                }
                let removed = match component {
                    ComponentId::MESH_RENDERER => self.ecs.remove::<MeshRenderer>(entity).is_ok(),
                    ComponentId::CAMERA => self.ecs.remove::<Camera>(entity).is_ok(),
                    ComponentId::DIRECTIONAL_LIGHT => {
                        self.ecs.remove::<DirectionalLight>(entity).is_ok()
                    }
                    ComponentId::POINT_LIGHT => self.ecs.remove::<PointLight>(entity).is_ok(),
                    ComponentId::SPOT_LIGHT => self.ecs.remove::<SpotLight>(entity).is_ok(),
                    ComponentId::ENVIRONMENT => self.ecs.remove::<Environment>(entity).is_ok(),
                    ComponentId::RIGID_BODY => self.ecs.remove::<RigidBody>(entity).is_ok(),
                    ComponentId::COLLIDER => self.ecs.remove::<Collider>(entity).is_ok(),
                    ComponentId::CHARACTER_CONTROLLER => {
                        self.ecs.remove::<CharacterController>(entity).is_ok()
                    }
                    ComponentId::THIRD_PERSON_CAMERA => {
                        self.ecs.remove::<ThirdPersonCamera>(entity).is_ok()
                    }
                    ComponentId::ROTATOR => self.ecs.remove::<Rotator>(entity).is_ok(),
                    ComponentId::OSCILLATE => self.ecs.remove::<Oscillate>(entity).is_ok(),
                    ComponentId::FOLLOW => self.ecs.remove::<Follow>(entity).is_ok(),
                    ComponentId::LOOK_AT_CAMERA => self.ecs.remove::<LookAtCamera>(entity).is_ok(),
                    ComponentId::LIFETIME_DESPAWN => {
                        self.ecs.remove::<LifetimeDespawn>(entity).is_ok()
                    }
                    ComponentId::EMITTER => self.ecs.remove::<Emitter>(entity).is_ok(),
                    ComponentId::TRIGGER_ZONE => self.ecs.remove::<TriggerZone>(entity).is_ok(),
                    ComponentId::ANIMATOR => self.ecs.remove::<Animator>(entity).is_ok(),
                    ComponentId::SKIN => self.ecs.remove::<Skin>(entity).is_ok(),
                    ComponentId::AUDIO_SOURCE => {
                        // The voice stops at the phase (its source is gone).
                        let _ = self.ecs.remove::<AudioRun>(entity);
                        self.ecs.remove::<AudioSource>(entity).is_ok()
                    }
                    ComponentId::AUDIO_LISTENER => self.ecs.remove::<AudioListener>(entity).is_ok(),
                    ComponentId::VOLUME => self.ecs.remove::<Volume>(entity).is_ok(),
                    ComponentId::LAYER => self.ecs.remove::<Layer>(entity).is_ok(),
                    ComponentId::TAGS => self.ecs.remove::<Tags>(entity).is_ok(),
                    ComponentId::TRANSFORM | ComponentId::HIERARCHY | ComponentId::NAME => {
                        return self.reject(
                            index,
                            lane,
                            DiagnosticCode::INVALID_PAYLOAD,
                            entity,
                            component,
                            format!("{} cannot be removed", component.name().unwrap()),
                        )
                    }
                    other => {
                        return self.reject(
                            index,
                            lane,
                            DiagnosticCode::UNKNOWN_COMPONENT,
                            entity,
                            other,
                            format!("unknown component id {}", other.0),
                        )
                    }
                };
                if !removed {
                    return self.reject(
                        index,
                        lane,
                        DiagnosticCode::MISSING_COMPONENT,
                        entity,
                        component,
                        format!("entity has no {}", component.name().unwrap()),
                    );
                }
                true
            }
            Command::Body { entity, op } => {
                if !self.check_entity(index, lane, entity, "body") {
                    return false;
                }
                // Applied by the physics step, after this batch's component
                // writes; a rejection there carries this index. A teleport
                // is shown at its target, never interpolated towards it.
                if matches!(op, BodyCommand::Teleport { .. }) {
                    self.teleported.push(entity);
                }
                self.physics.queue(index, entity, op);
                true
            }
            Command::Animate { entity, op } => self.apply_animate(index, entity, op),
            Command::Audio { entity, op } => self.apply_audio(index, entity, op),
            Command::PlaySound {
                clip,
                bus,
                volume,
                pitch,
                priority,
            } => self.apply_play_sound(
                index,
                lane,
                clip,
                PlayParams {
                    bus,
                    volume,
                    pitch,
                    looping: false,
                    priority,
                },
            ),
            Command::AudioBus { bus, volume, muted } => {
                self.apply_audio_bus(index, lane, bus, volume, muted)
            }
        }
    }

    /// `Command::AudioBus` (either lane): a bus gain and the mute.
    fn apply_audio_bus(
        &mut self,
        index: u32,
        lane: Lane,
        bus: AudioBus,
        volume: f32,
        muted: bool,
    ) -> bool {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_PAYLOAD,
                Entity::NULL,
                ComponentId::NONE,
                format!("audioBus: volume must be within [0, 1], got {volume}"),
            );
        }
        self.audio.set_bus(bus, volume);
        self.audio.set_muted(muted);
        true
    }

    /// Destroys `entity` and its subtree with the bookkeeping and the
    /// `Destroyed` events of `Command::Destroy` (also what an expired
    /// `LifetimeDespawn` does). Returns how many entities went.
    pub(crate) fn destroy_subtree(
        &mut self,
        entity: Entity,
        tick: Tick,
    ) -> Result<usize, SceneError> {
        let gone = scene::despawn_subtree(&mut self.ecs, entity)?;
        let count = gone.len();
        for e in gone {
            self.subscriptions.remove(&e);
            self.overrides.remove(&e);
            self.named_targets.remove(&e);
            self.events.push(Event {
                tick,
                payload: EventPayload::Destroyed { entity: e },
            });
        }
        Ok(count)
    }

    /// Shared by both lanes and by `load_scene`: validates the value and
    /// inserts (or replaces) the component. Not for `Transform`.
    fn set_component(
        &mut self,
        index: u32,
        lane: Lane,
        entity: Entity,
        value: ComponentValue,
    ) -> bool {
        if !self.check_entity(index, lane, entity, "set_component") {
            return false;
        }
        if let Err(msg) = value.validate() {
            return self.reject(
                index,
                lane,
                DiagnosticCode::INVALID_PAYLOAD,
                entity,
                value.id(),
                msg,
            );
        }
        let r = match value {
            ComponentValue::Transform(_) => unreachable!("handled as SetTransform"),
            ComponentValue::MeshRenderer(v) => self.ecs.insert(entity, v),
            ComponentValue::Camera(v) => self.ecs.insert(entity, v),
            ComponentValue::DirectionalLight(v) => self.ecs.insert(entity, v),
            ComponentValue::PointLight(v) => self.ecs.insert(entity, v),
            ComponentValue::SpotLight(v) => self.ecs.insert(entity, v),
            ComponentValue::Environment(v) => self.ecs.insert(entity, v),
            ComponentValue::RigidBody(v) => self.ecs.insert(entity, v),
            ComponentValue::Collider(v) => self.ecs.insert(entity, v),
            ComponentValue::CharacterController(v) => self.ecs.insert(entity, v),
            ComponentValue::ThirdPersonCamera(v) => self.ecs.insert(entity, v),
            ComponentValue::Rotator(v) => self.ecs.insert(entity, v),
            ComponentValue::Oscillate(v) => self.ecs.insert(entity, v),
            ComponentValue::Follow(v) => self.ecs.insert(entity, v),
            ComponentValue::LookAtCamera(v) => self.ecs.insert(entity, v),
            ComponentValue::LifetimeDespawn(v) => self.ecs.insert(entity, v),
            ComponentValue::Emitter(v) => self.ecs.insert(entity, v),
            ComponentValue::TriggerZone(v) => self.ecs.insert(entity, v),
            ComponentValue::Animator(v) => self.ecs.insert(entity, v),
            ComponentValue::Skin(v) => self.ecs.insert(entity, v),
            ComponentValue::AudioSource(v) => self.ecs.insert(entity, v),
            ComponentValue::AudioListener(v) => self.ecs.insert(entity, v),
            ComponentValue::Volume(v) => self.ecs.insert(entity, v),
            ComponentValue::Layer(v) => self.ecs.insert(entity, v),
            ComponentValue::Tags(v) => self.ecs.insert(entity, v),
        };
        r.is_ok()
    }

    fn reject_scene(&mut self, index: u32, lane: Lane, entity: Entity, err: SceneError) -> bool {
        let (code, entity, component, msg) = match err {
            SceneError::Cycle { child, parent } => (
                DiagnosticCode::HIERARCHY_CYCLE,
                child,
                ComponentId::HIERARCHY,
                format!("parent {parent:?} is {child:?} or a descendant of it"),
            ),
            SceneError::NotFinite(e) => (
                DiagnosticCode::INVALID_PAYLOAD,
                if e.is_null() { entity } else { e },
                ComponentId::TRANSFORM,
                "transform is not finite".to_string(),
            ),
            SceneError::Ecs(EcsError::StaleEntity(e)) => (
                DiagnosticCode::STALE_ENTITY,
                e,
                ComponentId::NONE,
                "stale or unknown entity".to_string(),
            ),
            SceneError::Ecs(EcsError::MissingComponent(e)) => (
                DiagnosticCode::MISSING_COMPONENT,
                e,
                ComponentId::TRANSFORM,
                "entity has no Transform".to_string(),
            ),
        };
        self.reject(index, lane, code, entity, component, msg)
    }

    // ------------------------------------------------------------------ step

    /// One fixed tick (§5.1). Applies staged simulation commands, runs the
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
                live_entities: self.ecs.len(),
            };
        }
        let step_start = Instant::now();
        let mut transforms_us: u64 = 0;
        let mut propagated: u32 = 0;
        self.diagnostics.clear();
        self.events.begin_tick();
        let next_tick = self.clock.tick() + 1;
        // Native pre-tick systems read the tick's input first (the
        // characters' action driver, M3-09): its commands are queued ahead
        // of the batch's, so an explicit `Move`/`Jump` from the game in the
        // same tick wins.
        let edit = self.config.edit_mode;
        let t0 = Instant::now();
        let modules = self.modules;
        let characters = if edit || !modules.physics {
            0
        } else {
            self.drive_characters()
        };
        self.set_time(SystemKind::Characters, characters, micros(t0));

        // S[n] becomes the interpolation source.
        let t0 = Instant::now();
        self.record_prev_world();
        transforms_us += micros(t0);

        // Validate/apply C[n] at the tick boundary.
        let t0 = Instant::now();
        let staged = std::mem::take(&mut self.staged);
        self.spawned_this_tick.clear();
        let mut applied: u32 = 0;
        let mut rejected = 0;
        for (index, cmd, from_presentation) in staged {
            let cmd = match cmd {
                Ok(cmd) => cmd,
                Err(r) => {
                    self.reject(
                        index,
                        Lane::Simulation,
                        r.code,
                        Entity::NULL,
                        ComponentId::NONE,
                        r.message,
                    );
                    rejected += 1;
                    continue;
                }
            };
            let Some(cmd) = self.resolve_temps(index, cmd) else {
                rejected += 1;
                continue;
            };
            let target = cmd.target();
            if self.apply_simulation(index, cmd, next_tick) {
                if from_presentation {
                    self.teleported.extend(target);
                }
                applied += 1;
            } else {
                rejected += 1;
            }
        }
        self.refresh_active();
        self.set_time(SystemKind::Commands, applied, micros(t0));

        // Animators pose their subtrees first among the pre-tick systems
        // (M3-19): a mover or a body on a posed joint follows the pose.
        // In edit mode (M4-06) every system below is skipped: commands,
        // propagation and the snapshot are the whole tick.
        let t0 = Instant::now();
        let n = if (edit && !self.edit_animation) || !modules.animation {
            0
        } else {
            self.run_animation(next_tick)
        };
        self.set_time(SystemKind::Animation, n, micros(t0));

        // The catalogue's pre-tick movers (§3.5, M3-10): they see the
        // batch's writes and physics sees theirs.
        let t0 = Instant::now();
        let n = if edit { 0 } else { self.run_rotators() };
        self.set_time(SystemKind::Rotator, n, micros(t0));
        let t0 = Instant::now();
        let n = if edit { 0 } else { self.run_oscillators() };
        self.set_time(SystemKind::Oscillate, n, micros(t0));
        let t0 = Instant::now();
        let n = if edit {
            0
        } else {
            self.run_lifetimes(next_tick)
        };
        self.set_time(SystemKind::LifetimeDespawn, n, micros(t0));
        let t0 = Instant::now();
        let n = if edit {
            0
        } else {
            self.run_emitters(next_tick)
        };
        self.set_time(SystemKind::Emitter, n, micros(t0));

        // Physics at the §5.1 slot: bodies see the batch's transforms,
        // step, and write their poses back before the tick's own
        // propagate.
        let t0 = Instant::now();
        let mut physics_entities = 0;
        if !edit && modules.physics && self.physics.has_work(&self.ecs) {
            propagated += scene::propagate(&mut self.ecs) as u32;
            self.physics
                .step(&mut self.ecs, self.config.fixed_dt, &self.subscriptions);
            // A body command counted as applied above may fail here.
            let failed = self.physics_rejections();
            rejected += failed;
            applied = applied.saturating_sub(failed);
            self.physics_events(next_tick);
            let ps = self.physics.stats();
            physics_entities = ps.bodies + ps.colliders + ps.characters;
        }
        self.set_time(SystemKind::Physics, physics_entities, micros(t0));

        // Deferred structural changes.
        let t0 = Instant::now();
        for e in self.ecs.flush_deferred() {
            self.subscriptions.remove(&e);
            self.overrides.remove(&e);
        }

        // Transform/hierarchy work for the tick; new entities start with
        // PrevWorld == current so they do not interpolate from the origin.
        propagated += scene::propagate(&mut self.ecs) as u32;
        transforms_us += micros(t0);
        // Camera rigs follow the tick's final poses (a rig's own subtree
        // is the only thing left dirty, so the second propagate is cheap).
        let t0 = Instant::now();
        let (rigs, moved) = if edit {
            (0, false)
        } else {
            self.place_camera_rigs()
        };
        self.set_time(SystemKind::CameraRigs, rigs, micros(t0));
        if moved {
            let t0 = Instant::now();
            propagated += scene::propagate(&mut self.ecs) as u32;
            transforms_us += micros(t0);
        }
        // Followers and billboards read the propagated poses, the camera's
        // included (M3-10).
        let t0 = Instant::now();
        let (followers, moved_followers) = if edit {
            (0, false)
        } else {
            self.run_followers()
        };
        self.set_time(SystemKind::Follow, followers, micros(t0));
        let t0 = Instant::now();
        let (billboards, moved_billboards) = if edit {
            (0, false)
        } else {
            self.run_billboards()
        };
        self.set_time(SystemKind::LookAtCamera, billboards, micros(t0));
        let t0 = Instant::now();
        if moved_followers || moved_billboards {
            propagated += scene::propagate(&mut self.ecs) as u32;
        }
        if modules.physics {
            self.physics.record(&self.ecs);
        }
        self.seed_prev_world();
        self.apply_teleports();
        if edit {
            // No interpolation in the editor: the picture is the tick's
            // own transforms whatever `alpha` the clock holds.
            self.record_prev_world();
        }
        transforms_us += micros(t0);
        self.set_time(SystemKind::Transforms, propagated, transforms_us);

        // The audio sources reconciled with their voices and the voice
        // clock advanced (M3-11); after the destroys of the tick, so a
        // dead source's voice stops this tick.
        let t0 = Instant::now();
        let n = if edit || !modules.audio {
            0
        } else {
            self.run_audio(next_tick)
        };
        self.set_time(SystemKind::Audio, n, micros(t0));

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
        self.events.publish_tick();
        self.last_events = self.events.pending() as u32;
        let records = self.snapshot.len() as u32;
        self.set_time(SystemKind::Publish, records, micros(t0));
        self.set_time(SystemKind::Step, self.ecs.len(), micros(step_start));

        let diagnostic_records = self.diagnostics.len() as u32;
        StepReport {
            status: if rejected == 0 {
                Status::Ok
            } else {
                Status::CommandsRejected
            },
            tick,
            applied,
            rejected,
            snapshot_records: self.snapshot.len() as u32,
            event_records: self.last_events,
            diagnostic_records,
            live_entities: self.ecs.len(),
        }
    }

    /// Brings the derived `Inactive` markers up to date after a spawn,
    /// reparent or flag change (nothing to do otherwise).
    pub(crate) fn refresh_active(&mut self) {
        if std::mem::take(&mut self.active_dirty) {
            scene::refresh_active(&mut self.ecs);
        }
    }

    /// Whether the derived `Inactive` markers are current.
    pub(crate) fn active_markers_current(&self) -> bool {
        !self.active_dirty
    }

    /// Brings the markers up to date after a load spawned `roots`'
    /// subtrees into a world whose markers were current: those subtrees
    /// alone (foundation 3.1).
    pub(crate) fn refresh_active_subtrees(&mut self, roots: &[Entity]) {
        for &root in roots {
            scene::refresh_active_subtree(&mut self.ecs, root);
        }
        self.active_dirty = false;
    }

    /// Marks the markers stale (a load spawned under a parent).
    pub(crate) fn mark_active_dirty(&mut self) {
        self.active_dirty = true;
    }

    fn record_prev_world(&mut self) {
        let h = self.ecs.hecs_mut();
        for (wt, prev) in h.query_mut::<(&WorldTransform, &mut PrevWorld)>() {
            prev.0 = Transform::from_matrix(wt.matrix);
        }
    }

    /// A value already shown through a presentation override must not be
    /// interpolated towards from S[n] once it becomes authoritative, or the
    /// object would visibly snap back for one tick.
    fn apply_teleports(&mut self) {
        let roots = std::mem::take(&mut self.teleported);
        for root in roots {
            for e in scene::subtree(&self.ecs, root) {
                let Ok(wt) = self.ecs.copy::<WorldTransform>(e) else {
                    continue;
                };
                if let Ok(mut prev) = self.ecs.get_mut::<PrevWorld>(e) {
                    prev.0 = Transform::from_matrix(wt.matrix);
                }
            }
        }
    }

    /// Physics rejections become diagnostics: a command's carries its batch
    /// index; a component the sync could not realise carries `u32::MAX`.
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
            self.diagnostics.push(Diagnostic {
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
        let mut cmds = std::mem::take(&mut self.driver_scratch);
        cmds.clear();
        let mut driven = 0;
        {
            let h = self.ecs.hecs();
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
                        let yaw = self
                            .ecs
                            .get::<Name>(entity)
                            .ok()
                            .and_then(|name| {
                                h.query::<&ThirdPersonCamera>()
                                    .iter()
                                    .find(|r| !r.target.is_empty() && r.target == name.value)
                                    .map(|r| r.yaw)
                            })
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
            self.physics.queue(u32::MAX, entity, cmd);
        }
        self.driver_scratch = cmds;
        driven
    }

    /// The camera rigs (M3-09), after the tick's propagate: each
    /// `ThirdPersonCamera` turns by its `look_action`, orbits its target's
    /// pivot at `distance` (pulled in to the first collider a sphere cast
    /// from the pivot meets, never under `min_distance`) and looks at it;
    /// yaw/pitch are written back. Returns the rig count and whether any
    /// moved.
    fn place_camera_rigs(&mut self) -> (u32, bool) {
        let mut placements = std::mem::take(&mut self.rig_scratch);
        placements.clear();
        let mut placements_targets: Vec<(Entity, Entity)> = Vec::new();
        let dt = self.config.fixed_dt;
        {
            let h = self.ecs.hecs();
            for (handle, rig) in h
                .query::<(&Handle, &ThirdPersonCamera)>()
                .without::<&scene::Inactive>()
                .iter()
            {
                let entity = handle.0;
                if rig.target.is_empty() {
                    continue;
                }
                let Some(target) = self.resolve_named(entity, &rig.target) else {
                    continue;
                };
                let Ok(target_world) = self.ecs.get::<WorldTransform>(target) else {
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
                    self.hits_scratch.clear();
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
                        &mut self.hits_scratch,
                    );
                    if let Some(hit) = self.hits_scratch.first() {
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
                placements_targets.push((entity, target));
            }
        }
        for (rig, target) in placements_targets.drain(..) {
            self.named_targets.insert(rig, target);
        }
        let moved = !placements.is_empty();
        let rigs = placements.len() as u32;
        for p in placements.drain(..) {
            if let Ok(mut rig) = self.ecs.get_mut::<ThirdPersonCamera>(p.rig) {
                rig.yaw = p.yaw;
                rig.pitch = p.pitch;
            }
            let _ = scene::set_world_pose(&self.ecs, p.rig, p.position, p.rotation);
        }
        self.rig_scratch = placements;
        (rigs, moved)
    }

    /// The step's contacts on the physics lane (readable by
    /// `after_physics_events` this tick, drained next).
    fn physics_events(&mut self, tick: Tick) {
        // A trigger pair with a `TriggerZone` on either side also fires the
        // zone's named event on the general lane (M3-12); `once` zones are
        // removed after their entry, collected while the events are read.
        let mut fired: Vec<Entity> = Vec::new();
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
            self.events.push(Event { tick, payload });
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
                let (event, once) = match self.ecs.get::<TriggerZone>(zone) {
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
                self.events.push(Event { tick, payload });
                if entered && once {
                    fired.push(zone);
                }
            }
        }
        for zone in fired {
            let _ = self.ecs.remove::<TriggerZone>(zone);
        }
    }

    fn seed_prev_world(&mut self) {
        let mut fresh = Vec::new();
        {
            let h = self.ecs.hecs();
            for (e, wt) in h
                .query::<fynkel_ecs::internal::hecs::Without<
                    (fynkel_ecs::internal::hecs::Entity, &WorldTransform),
                    &PrevWorld,
                >>()
                .iter()
            {
                fresh.push((e, Transform::from_matrix(wt.matrix)));
            }
        }
        let h = self.ecs.hecs_mut();
        for (e, t) in fresh {
            let _ = h.insert_one(e, PrevWorld(t));
        }
    }

    fn publish_snapshot(&mut self) {
        self.snapshot.clear();
        for &e in &self.subscriptions {
            let (Ok(local), Ok(wt)) = (
                self.ecs.copy::<Transform>(e),
                self.ecs.get::<WorldTransform>(e),
            ) else {
                continue;
            };
            self.snapshot.push(SnapshotRecord {
                entity: e,
                local,
                world: Transform::from_matrix(wt.matrix),
            });
        }
    }

    // --------------------------------------------------------------- reads

    /// Subscribes (or unsubscribes) entities for the transform snapshot.
    /// Unknown/stale handles are ignored; destroyed entities drop out.
    pub fn subscribe(&mut self, entities: &[Entity], enable: bool) {
        for &e in entities {
            if enable {
                if self.ecs.contains(e) {
                    self.subscriptions.insert(e);
                }
            } else {
                self.subscriptions.remove(&e);
            }
        }
    }

    pub fn subscriptions(&self) -> impl Iterator<Item = Entity> + '_ {
        self.subscriptions.iter().copied()
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn staged_count(&self) -> usize {
        self.staged.len()
    }

    /// Coherent S[n+1] of subscribed entities, in handle order.
    pub fn snapshot(&self) -> &[SnapshotRecord] {
        &self.snapshot
    }

    /// Diagnostics since the last `step()` began (rejections from that step
    /// and from presentation submits after it).
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Pull-only drain: general events plus the last tick's physics events.
    pub fn drain_events(&mut self) -> Vec<Event> {
        self.events.drain()
    }

    /// Physics events of the tick just stepped (for `afterPhysics`).
    pub fn after_physics_events(&self) -> &[Event] {
        self.events.after_physics()
    }

    /// Queues a general-lane event from a native system or host (e.g.
    /// `SceneReady` after a load completes).
    pub fn push_event(&mut self, payload: EventPayload) {
        self.events.push(Event {
            tick: self.clock.tick(),
            payload,
        });
    }

    // -------------------------------------------------------------- extract

    /// Render extraction: every spatial entity's world matrix interpolated
    /// S[n]..S[n+1] by alpha, with presentation overrides applied (an
    /// overridden entity and its descendants skip interpolation).
    pub fn extract(&self) -> Extract {
        let alpha = self.clock.alpha();
        let mut memo: HashMap<Entity, (Mat4, bool)> = HashMap::new();
        let mut out = Extract {
            tick: self.clock.tick(),
            alpha,
            time: (self.clock.elapsed() + alpha as f64 * self.clock.fixed_dt() as f64) as f32,
            records: Vec::with_capacity(self.ecs.len() as usize),
            ..Extract::default()
        };
        let entities: Vec<Entity> = self.ecs.entities().collect();
        for e in entities {
            if !self.ecs.has::<WorldTransform>(e) {
                continue;
            }
            if self.ecs.has::<scene::Inactive>(e) {
                // Not active in the hierarchy: as absent as a hidden one.
                continue;
            }
            if self.config.edit_mode && self.edit_hidden.contains(&e) {
                // The editor's hidden flag: no record at all — nothing to
                // draw, light with or look through.
                continue;
            }
            let (world, overridden) = self.resolve_extract(e, alpha, &mut memo);
            let record = out.records.len() as u32;
            out.records.push(ExtractRecord {
                entity: e,
                world,
                overridden,
                layer_bit: self.ecs.copy::<Layer>(e).map_or(1, |l| l.bit().max(1)),
            });
            if let Ok(m) = self.ecs.get::<MeshRenderer>(e) {
                if m.visible {
                    // A skinned draw's palette from the joints' own
                    // interpolated matrices (M3-19).
                    let skin = self.skin_joints(e).map(|(skin, joints)| {
                        let first_joint = out.joints.len() as u32;
                        let mut world_of = |j: Entity| {
                            self.ecs
                                .has::<WorldTransform>(j)
                                .then(|| self.resolve_extract(j, alpha, &mut memo).0)
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
            if let Ok(c) = self.ecs.copy::<Camera>(e) {
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
            if let Ok(l) = self.ecs.copy::<DirectionalLight>(e) {
                let direction = world.transform_vector3(-Vec3::Z).normalize_or(-Vec3::Z);
                out.directional_lights.push(ExtractDirectionalLight {
                    record,
                    direction,
                    light: l,
                });
            }
            if let Ok(l) = self.ecs.copy::<PointLight>(e) {
                out.point_lights.push(ExtractPointLight {
                    record,
                    position: world.transform_point3(Vec3::ZERO),
                    light: l,
                });
            }
            if let Ok(l) = self.ecs.copy::<SpotLight>(e) {
                out.spot_lights.push(ExtractSpotLight {
                    record,
                    position: world.transform_point3(Vec3::ZERO),
                    direction: world.transform_vector3(-Vec3::Z).normalize_or(-Vec3::Z),
                    light: l,
                });
            }
            if let Ok(v) = self.ecs.get::<Volume>(e) {
                if v.enabled && v.weight > 0.0 {
                    out.volumes.push(ExtractVolume {
                        record,
                        world,
                        volume: v.clone(),
                    });
                }
            }
            if out.environment.is_none() {
                if let Ok(env) = self.ecs.get::<Environment>(e) {
                    out.environment = Some(ExtractEnvironment {
                        record,
                        environment: env.clone(),
                    });
                }
            }
        }
        if self.config.edit_mode {
            if let Some((world, camera)) = &self.edit_camera {
                // The editor looks through its own camera: no record backs
                // it (`u32::MAX`), the renderer reads `world` and `camera`.
                out.camera = Some(ExtractCamera {
                    record: u32::MAX,
                    world: *world,
                    camera: *camera,
                });
            }
        }
        out
    }

    fn resolve_extract(
        &self,
        e: Entity,
        alpha: f32,
        memo: &mut HashMap<Entity, (Mat4, bool)>,
    ) -> (Mat4, bool) {
        if let Some(r) = memo.get(&e) {
            return *r;
        }
        let parent = self.ecs.get::<Parent>(e).ok().map(|p| p.0);
        let parent_res = parent.map(|p| self.resolve_extract(p, alpha, memo));
        let parent_overridden = parent_res.is_some_and(|r| r.1);
        let result = if let Some(t) = self.overrides.get(&e) {
            let pw = parent_res.map(|r| r.0).unwrap_or(Mat4::IDENTITY);
            (pw * t.matrix(), true)
        } else if parent_overridden {
            let local = self.ecs.copy::<Transform>(e).unwrap_or_default();
            (parent_res.unwrap().0 * local.matrix(), true)
        } else {
            let cur = self
                .ecs
                .get::<WorldTransform>(e)
                .map(|w| Transform::from_matrix(w.matrix))
                .unwrap_or_default();
            let prev = self.ecs.copy::<PrevWorld>(e).map(|p| p.0).unwrap_or(cur);
            (lerp(prev, cur, alpha).matrix(), false)
        };
        memo.insert(e, result);
        result
    }

    /// Total events published by the last step (test/stat hook).
    pub fn last_event_count(&self) -> u32 {
        self.last_events
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
