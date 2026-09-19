//! The runtime world: lifecycle, command lanes, `step()`, snapshots,
//! events, diagnostics, render extraction.

use std::collections::{BTreeSet, HashMap};

use fynkel_contracts::{
    AssetRef, ComponentId, DiagnosticCode, Entity, Status, Tick, WorldId, DEFAULT_FIXED_DT,
    DEFAULT_MAX_STEPS_PER_FRAME, MAX_FRAME_DT,
};
use fynkel_ecs::{EcsError, World as Ecs};
use fynkel_scene::{self as scene, Parent, SceneError, Transform, WorldTransform};
use fynkel_schema_derive::Schema;
use glam::{Mat4, Vec3};

use crate::clock::{FrameClock, FramePlan};
use crate::command::{Command, Lane};
use crate::components::{
    Camera, ComponentValue, DirectionalLight, Environment, MeshRenderer, PointLight, SpotLight,
};
use crate::event::{Event, EventPayload, EventQueue};

#[derive(Clone, Debug)]
pub struct WorldConfig {
    pub id: WorldId,
    pub fixed_dt: f32,
    pub max_steps_per_frame: u32,
    pub max_frame_dt: f32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            id: WorldId(1),
            fixed_dt: DEFAULT_FIXED_DT as f32,
            max_steps_per_frame: DEFAULT_MAX_STEPS_PER_FRAME,
            max_frame_dt: MAX_FRAME_DT as f32,
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

/// Marker: the transform is written by physics; presentation-lane transform
/// commands are rejected with `PHYSICS_OWNED` (ADR-0008).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicsOwned;

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
}

/// A visible `MeshRenderer`: `record` indexes [`Extract::records`].
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractMesh {
    pub record: u32,
    pub mesh: AssetRef,
    pub material: AssetRef,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
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

/// What a frame draws (blueprint §8.5): every spatial entity's world
/// matrix, plus the renderable components resolved against those records.
/// The renderer reads this and nothing else from the world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extract {
    pub tick: Tick,
    pub alpha: f32,
    pub records: Vec<ExtractRecord>,
    pub meshes: Vec<ExtractMesh>,
    pub camera: Option<ExtractCamera>,
    pub directional_lights: Vec<ExtractDirectionalLight>,
    pub point_lights: Vec<ExtractPointLight>,
    pub spot_lights: Vec<ExtractSpotLight>,
    pub environment: Option<ExtractEnvironment>,
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
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        let clock = FrameClock::new(
            config.fixed_dt,
            config.max_steps_per_frame,
            config.max_frame_dt,
        );
        Self {
            ecs: Ecs::new(config.id),
            config,
            clock,
            staged: Vec::new(),
            teleported: Vec::new(),
            overrides: HashMap::new(),
            events: EventQueue::default(),
            subscriptions: BTreeSet::new(),
            snapshot: Vec::new(),
            diagnostics: Vec::new(),
            last_events: 0,
            applied_this_tick: 0,
            poisoned: false,
        }
    }

    pub fn config(&self) -> &WorldConfig {
        &self.config
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
            _ => unreachable!("allowed_on_presentation"),
        }
    }

    pub(crate) fn apply_simulation(&mut self, index: u32, cmd: Command, tick: Tick) -> bool {
        let lane = Lane::Simulation;
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
                let gone = match scene::despawn_subtree(&mut self.ecs, entity) {
                    Ok(v) => v,
                    Err(err) => return self.reject_scene(index, lane, entity, err),
                };
                for e in gone {
                    self.subscriptions.remove(&e);
                    self.overrides.remove(&e);
                    self.events.push(Event {
                        tick,
                        payload: EventPayload::Destroyed { entity: e },
                    });
                }
                true
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
        }
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
        self.diagnostics.clear();
        self.events.begin_tick();
        let next_tick = self.clock.tick() + 1;

        // S[n] becomes the interpolation source.
        self.record_prev_world();

        // Validate/apply C[n] at the tick boundary.
        let staged = std::mem::take(&mut self.staged);
        let mut applied = 0;
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

        // Native pre-tick systems, physics, post-physics sync, animation:
        // no producers in M1; the phase order is fixed here.

        // Deferred structural changes.
        for e in self.ecs.flush_deferred() {
            self.subscriptions.remove(&e);
            self.overrides.remove(&e);
        }

        // Transform/hierarchy work for the tick; new entities start with
        // PrevWorld == current so they do not interpolate from the origin.
        scene::propagate(&mut self.ecs);
        self.seed_prev_world();
        self.apply_teleports();

        let tick = self.clock.consume_step();
        debug_assert_eq!(tick, next_tick);
        self.applied_this_tick = applied;

        self.publish_snapshot();
        self.events.publish_tick();
        self.last_events = self.events.pending() as u32;

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
            records: Vec::with_capacity(self.ecs.len() as usize),
            ..Extract::default()
        };
        let entities: Vec<Entity> = self.ecs.entities().collect();
        for e in entities {
            if !self.ecs.has::<WorldTransform>(e) {
                continue;
            }
            let (world, overridden) = self.resolve_extract(e, alpha, &mut memo);
            let record = out.records.len() as u32;
            out.records.push(ExtractRecord {
                entity: e,
                world,
                overridden,
            });
            if let Ok(m) = self.ecs.get::<MeshRenderer>(e) {
                if m.visible {
                    out.meshes.push(ExtractMesh {
                        record,
                        mesh: m.mesh.clone(),
                        material: m.material.clone(),
                        cast_shadows: m.cast_shadows,
                        receive_shadows: m.receive_shadows,
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
            if out.environment.is_none() {
                if let Ok(env) = self.ecs.get::<Environment>(e) {
                    out.environment = Some(ExtractEnvironment {
                        record,
                        environment: env.clone(),
                    });
                }
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
