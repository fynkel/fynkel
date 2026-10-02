//! The tick's native systems (§5.1) and the one place their order is
//! declared (foundation 6.1). `World::step` runs the registered schedule front to back;
//! each system says when it runs and reports its entity count, and the
//! step times it under its [`SystemKind`].
//!
//! Transform work comes in four pieces between the others (S[n] recorded,
//! the tick's propagate, and two settles after the systems that move
//! things late); they report together as one `Transforms` entry.

use std::time::Instant;

use fynkel_contracts::{ComponentId, Entity, Tick};

use super::{micros, World};
use crate::command::Lane;
use crate::schema::SystemKind;

/// What the systems of one tick share.
pub(super) struct TickRun {
    pub(super) tick: Tick,
    pub(super) applied: u32,
    pub(super) rejected: u32,
    /// World matrices recomputed this tick, all propagates counted.
    pub(super) propagated: u32,
    /// The transform pieces' time so far.
    pub(super) transforms_us: u64,
    /// A system moved something after the tick's propagate: the next
    /// settle propagates again.
    pub(super) moved: bool,
}

/// One native system of the tick.
pub(super) trait System {
    const KIND: SystemKind;
    /// Whether it runs this tick; one that does not reports 0 entities.
    fn runs(world: &World) -> bool;
    /// Runs it; the entity count for `FYNKEL_QUERY_SYSTEMS`.
    fn run(world: &mut World, t: &mut TickRun) -> u32;
}

/// A [`System`] as the tick's table holds it.
#[derive(Clone, Copy)]
pub(super) struct Stage {
    pub(super) kind: SystemKind,
    runs: fn(&World) -> bool,
    run: fn(&mut World, &mut TickRun) -> u32,
}

const fn stage<S: System>() -> Stage {
    Stage {
        kind: S::KIND,
        runs: S::runs,
        run: S::run,
    }
}

#[derive(Clone)]
pub(super) enum Runner {
    Builtin(Stage),
    Custom(crate::SystemCallback),
}
#[derive(Clone)]
pub(super) struct ScheduledStage {
    pub spec: crate::SystemSpec,
    pub runner: Runner,
}

pub(super) fn builtin_schedule() -> Vec<ScheduledStage> {
    use crate::SystemPhase::*;
    let builtins = [
        ("characters", Input, stage::<Characters>()),
        ("previous", Input, stage::<RecordPrevious>()),
        ("effects", Input, stage::<Effects>()),
        ("animation", Simulation, stage::<Animation>()),
        ("rotators", Simulation, stage::<Rotators>()),
        ("oscillators", Simulation, stage::<Oscillators>()),
        ("lifetimes", Simulation, stage::<Lifetimes>()),
        ("emitters", Simulation, stage::<Emitters>()),
        ("physics", Physics, stage::<PhysicsStep>()),
        ("propagate", PostPhysics, stage::<Propagate>()),
        ("camera_rigs", PostPhysics, stage::<CameraRigs>()),
        ("settle_rigs", PostPhysics, stage::<SettleRigs>()),
        ("followers", PostPhysics, stage::<Followers>()),
        ("billboards", PostPhysics, stage::<Billboards>()),
        ("settle", PostPhysics, stage::<Settle>()),
        ("audio", Audio, stage::<self::Audio>()),
    ];
    let mut previous = None;
    builtins
        .into_iter()
        .map(|(id, phase, stage)| {
            let mut spec = crate::SystemSpec::new(format!("fynkel.{id}"), phase);
            spec.after = previous.take().into_iter().collect();
            previous = Some(spec.id.clone());
            spec.reads.push("*".into());
            spec.writes.push("*".into());
            spec.structural = true;
            spec.requires = match id {
                "characters" | "physics" => vec![crate::Module::Physics],
                "animation" => vec![crate::Module::Animation],
                "audio" => vec![crate::Module::Audio],
                _ => vec![],
            };
            ScheduledStage {
                spec,
                runner: Runner::Builtin(stage),
            }
        })
        .collect()
}

impl World {
    /// Runs the registered schedule for tick `tick`; what it applied and rejected.
    pub(super) fn run_tick(&mut self, tick: Tick) -> TickRun {
        let mut t = TickRun {
            tick,
            applied: 0,
            rejected: 0,
            propagated: 0,
            transforms_us: 0,
            moved: false,
        };
        // Owned schedule descriptors contain only immutable metadata and function
        // pointers. Temporarily move the list to avoid aliasing the world during callbacks.
        let schedule = std::mem::take(&mut self.schedule);
        let mut fatal_panic = None;
        if self.registered_system_times.len() != schedule.len()
            || self
                .registered_system_times
                .iter()
                .zip(&schedule)
                .any(|(time, stage)| time.id != stage.spec.id)
        {
            self.registered_system_times = schedule
                .iter()
                .map(|stage| crate::RegisteredSystemTime {
                    id: stage.spec.id.clone(),
                    micros: 0,
                    entities: 0,
                    rejected: false,
                })
                .collect();
        }
        for (index, stage) in schedule.iter().enumerate() {
            let t0 = Instant::now();
            let mut rejected = false;
            let n = match stage.runner {
                Runner::Builtin(builtin) => {
                    let n = if (builtin.runs)(self) {
                        (builtin.run)(self, &mut t)
                    } else {
                        0
                    };
                    if builtin.kind == SystemKind::Transforms {
                        t.transforms_us += micros(t0);
                        self.set_time(SystemKind::Transforms, t.propagated, t.transforms_us);
                    } else {
                        self.set_time(builtin.kind, n, micros(t0));
                    }
                    n
                }
                Runner::Custom(run) => {
                    if self.edit() {
                        0
                    } else if let Some(module) =
                        stage.spec.requires.iter().find(|&&m| !self.modules.has(m))
                    {
                        rejected = true;
                        t.rejected += 1;
                        self.reject(
                            u32::MAX,
                            Lane::Simulation,
                            fynkel_contracts::DiagnosticCode::MODULE_OFF,
                            Entity::NULL,
                            ComponentId::NONE,
                            format!("{} requires {}", stage.spec.id, module.key()),
                        );
                        0
                    } else {
                        let context = crate::SystemContext {
                            tick,
                            delta_seconds: self.config.fixed_dt,
                        };
                        // A system failure rejects only this system's edit. Other systems
                        // run in declared order; they never observe partial component data.
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            self.edit_native_data_at_tick(tick, |edit| run(edit, context))
                        }));
                        match result {
                            Ok(Ok(commit)) => {
                                t.moved = true;
                                commit.value
                            }
                            failure => {
                                rejected = true;
                                t.rejected += 1;
                                let why = match failure {
                                    Ok(Err(e)) => e.to_string(),
                                    Err(panic) => {
                                        if self.is_poisoned() {
                                            fatal_panic = Some(panic);
                                        }
                                        "callback panicked".into()
                                    }
                                    Ok(Ok(_)) => unreachable!(),
                                };
                                self.reject(
                                    u32::MAX,
                                    Lane::Simulation,
                                    fynkel_contracts::DiagnosticCode::INVALID_PAYLOAD,
                                    Entity::NULL,
                                    ComponentId::NONE,
                                    format!("{}: {why}", stage.spec.id),
                                );
                                0
                            }
                        }
                    }
                }
            };
            let time = &mut self.registered_system_times[index];
            time.micros = micros(t0);
            time.entities = n;
            time.rejected = rejected;
            if self.is_poisoned() {
                break;
            }
        }
        self.schedule = schedule;
        if let Some(panic) = fatal_panic {
            std::panic::resume_unwind(panic);
        }
        t
    }

    /// Install a complete batch atomically, allowing forward dependencies within
    /// that batch. Existing systems cannot be silently replaced.
    pub fn register_systems(
        &mut self,
        systems: Vec<crate::RegisteredSystem>,
    ) -> Result<(), crate::ScheduleError> {
        use crate::ScheduleError as E;
        if self.is_poisoned() {
            return Err(E::Poisoned);
        }
        let mut candidate = self.schedule.clone();
        for mut system in systems {
            if system.spec.id.starts_with("fynkel.") {
                return Err(E::ReservedIdentity(system.spec.id));
            }
            match system.spec.phase {
                crate::SystemPhase::Simulation => {}
                crate::SystemPhase::PostPhysics => {
                    system.spec.after.push("fynkel.propagate".into());
                    system.spec.before.push("fynkel.settle".into());
                }
                _ => return Err(E::UnsupportedPhase(system.spec.id)),
            }
            for &module in &system.spec.requires {
                if !self.modules.has(module) {
                    return Err(E::MissingModule(system.spec.id.clone(), module));
                }
            }
            for name in system.spec.reads.iter().chain(&system.spec.writes) {
                if name != "*"
                    && !crate::schema::ALL.iter().any(|s| s.name == name)
                    && !self.gameplay.registered.schemas().any(|s| s.id == *name)
                {
                    return Err(E::UnknownComponent(system.spec.id.clone(), name.clone()));
                }
            }
            candidate.push(ScheduledStage {
                spec: system.spec,
                runner: Runner::Custom(system.run),
            });
        }
        // Explicit dependencies on optional systems require their modules.
        // Built-in ordering barriers still exist when their implementation is off.
        loop {
            let mut changed = false;
            for i in 0..candidate.len() {
                if !matches!(candidate[i].runner, Runner::Custom(_)) {
                    continue;
                }
                let dependencies: Vec<_> = candidate[i]
                    .spec
                    .before
                    .iter()
                    .chain(&candidate[i].spec.after)
                    .cloned()
                    .collect();
                for dependency in dependencies {
                    let required = candidate
                        .iter()
                        .find(|s| s.spec.id == dependency)
                        .map(|s| s.spec.requires.clone())
                        .unwrap_or_default();
                    for module in required {
                        if !self.modules.has(module) {
                            return Err(E::MissingModule(candidate[i].spec.id.clone(), module));
                        }
                        if !candidate[i].spec.requires.contains(&module) {
                            candidate[i].spec.requires.push(module);
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let specs: Vec<_> = candidate.iter().map(|s| s.spec.clone()).collect();
        let order = crate::scheduler::compile_schedule(&specs)?;
        self.schedule = order.into_iter().map(|i| candidate[i].clone()).collect();
        Ok(())
    }
    pub fn registered_systems(&self) -> impl Iterator<Item = &crate::SystemSpec> {
        self.schedule.iter().map(|s| &s.spec)
    }
    pub fn registered_system_times(&self) -> &[crate::RegisteredSystemTime] {
        &self.registered_system_times
    }

    fn edit(&self) -> bool {
        self.config.edit_mode
    }
}

// In edit mode (M4-06) only commands, propagation and the snapshot run
// (and animation when the editor previews it).

struct Characters;
impl System for Characters {
    const KIND: SystemKind = SystemKind::Characters;
    fn runs(w: &World) -> bool {
        !w.edit() && w.modules.physics
    }
    fn run(w: &mut World, _: &mut TickRun) -> u32 {
        w.drive_characters()
    }
}

/// S[n] becomes the interpolation source.
struct RecordPrevious;
impl System for RecordPrevious {
    const KIND: SystemKind = SystemKind::Transforms;
    fn runs(_: &World) -> bool {
        true
    }
    fn run(w: &mut World, _: &mut TickRun) -> u32 {
        if !std::mem::take(&mut w.gameplay.gameplay_tick_prepared) {
            w.record_prev_world();
        }
        0
    }
}

/// Delivers committed effects at the tick boundary.
struct Effects;
impl System for Effects {
    const KIND: SystemKind = SystemKind::Commands;
    fn runs(_: &World) -> bool {
        true
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        // Only committed typed effects reach the tick; authored data is already published.
        for effect in std::mem::take(&mut w.gameplay.gameplay_effects) {
            if effect.target().is_some_and(|e| !w.gameplay.ecs.contains(e)) {
                continue;
            }
            let applied = w.deliver_committed_effect(u32::MAX, Lane::Simulation, effect);
            if applied {
                t.applied += 1;
            } else {
                t.rejected += 1;
            }
        }
        w.refresh_active();
        t.applied
    }
}

struct Animation;
impl System for Animation {
    const KIND: SystemKind = SystemKind::Animation;
    fn runs(w: &World) -> bool {
        (!w.edit() || w.edit.animation) && w.modules.animation
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        w.run_animation(t.tick)
    }
}

struct Rotators;
impl System for Rotators {
    const KIND: SystemKind = SystemKind::Rotator;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        w.run_rotators(t.tick)
    }
}

struct Oscillators;
impl System for Oscillators {
    const KIND: SystemKind = SystemKind::Oscillate;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        w.run_oscillators(t.tick)
    }
}

struct Lifetimes;
impl System for Lifetimes {
    const KIND: SystemKind = SystemKind::LifetimeDespawn;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        w.run_lifetimes(t.tick)
    }
}

struct Emitters;
impl System for Emitters {
    const KIND: SystemKind = SystemKind::Emitter;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        w.run_emitters(t.tick)
    }
}

/// Physics at the §5.1 slot: bodies see the batch's transforms, step, and
/// write their poses back before the tick's own propagate.
struct PhysicsStep;
impl System for PhysicsStep {
    const KIND: SystemKind = SystemKind::Physics;
    fn runs(w: &World) -> bool {
        !w.edit() && w.modules.physics && w.physics.has_work(&w.gameplay.ecs)
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        t.propagated += w.propagate() as u32;
        w.physics.step(
            &mut w.gameplay.ecs,
            w.config.fixed_dt,
            &w.publication.subscriptions,
        );
        // A body command counted as applied above may fail here.
        let failed = w.physics_rejections();
        t.rejected += failed;
        t.applied = t.applied.saturating_sub(failed);
        w.physics_events(t.tick);
        let ps = w.physics.stats();
        ps.bodies + ps.colliders + ps.characters
    }
}

/// Deferred structural changes, then the tick's propagate. A queued
/// despawn goes as `Command::Destroy` does — the subtree, the bookkeeping
/// and the `Destroyed` events (foundation 3.10); one already gone with its
/// parent (or twice in the queue) is skipped. New entities start with
/// PrevTransform == current so they do not interpolate from the origin.
struct Propagate;
impl System for Propagate {
    const KIND: SystemKind = SystemKind::Transforms;
    fn runs(_: &World) -> bool {
        true
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        let pending = w.gameplay.ecs.take_deferred();
        if !pending.is_empty() {
            let result = w.edit_native_data_at_tick(t.tick, |edit| {
                for &entity in &pending {
                    if edit.contains(entity)? {
                        edit.destroy(entity)?;
                    }
                }
                Ok(())
            });
            if let Err(error) = result {
                for entity in pending {
                    w.gameplay.ecs.despawn_deferred(entity);
                }
                w.reject(
                    u32::MAX,
                    Lane::Simulation,
                    fynkel_contracts::DiagnosticCode::INVALID_PAYLOAD,
                    Entity::NULL,
                    ComponentId::NONE,
                    format!("deferred destruction: {error}"),
                );
                t.rejected += 1;
            }
        }
        t.propagated += w.propagate() as u32;
        0
    }
}

struct CameraRigs;
impl System for CameraRigs {
    const KIND: SystemKind = SystemKind::CameraRigs;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        let (rigs, moved) = w.place_camera_rigs(t.tick);
        t.moved |= moved;
        rigs
    }
}

/// Propagates again when a rig moved.
struct SettleRigs;
impl System for SettleRigs {
    const KIND: SystemKind = SystemKind::Transforms;
    fn runs(_: &World) -> bool {
        true
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        if std::mem::take(&mut t.moved) {
            t.propagated += w.propagate() as u32;
        }
        0
    }
}

struct Followers;
impl System for Followers {
    const KIND: SystemKind = SystemKind::Follow;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        let (n, moved) = w.run_followers(t.tick);
        t.moved |= moved;
        n
    }
}

struct Billboards;
impl System for Billboards {
    const KIND: SystemKind = SystemKind::LookAtCamera;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        let (n, moved) = w.run_billboards(t.tick);
        t.moved |= moved;
        n
    }
}

/// Propagates again when a follower or billboard moved, then the
/// interpolation bookkeeping.
struct Settle;
impl System for Settle {
    const KIND: SystemKind = SystemKind::Transforms;
    fn runs(_: &World) -> bool {
        true
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        if std::mem::take(&mut t.moved) {
            t.propagated += w.propagate() as u32;
        }
        if w.modules.physics {
            w.physics.record(&w.gameplay.ecs);
        }
        w.seed_prev_world();
        w.apply_teleports();
        if w.edit() {
            // No interpolation in the editor: the picture is the tick's
            // own transforms whatever `alpha` the clock holds.
            w.record_prev_world();
        }
        0
    }
}

/// The audio sources reconciled with their voices and the voice clock
/// advanced (M3-11).
struct Audio;
impl System for Audio {
    const KIND: SystemKind = SystemKind::Audio;
    fn runs(w: &World) -> bool {
        !w.edit() && w.modules.audio
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        w.run_audio(t.tick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_deferred_destruction_preserves_queue_and_retries_without_duplicate_events() {
        let mut w = World::new(crate::WorldConfig::default());
        let commit = w
            .edit_gameplay(|edit| {
                let root = edit.spawn("root", fynkel_scene::Transform::IDENTITY, None)?;
                let child = edit.spawn(
                    "child",
                    fynkel_scene::Transform::IDENTITY,
                    Some(root.into()),
                )?;
                Ok((root, child))
            })
            .unwrap();
        let root = commit.resolve(commit.value.0).unwrap();
        let child = commit.resolve(commit.value.1).unwrap();
        w.gameplay.ecs.despawn_deferred(child);
        w.gameplay.ecs.despawn_deferred(root);
        w.gameplay.ecs.despawn_deferred(child);
        let sequence = w.gameplay.next_gameplay_edit;
        w.gameplay.next_gameplay_edit = u64::MAX;
        let failed = w.step();
        assert_eq!(w.live_entities(), 2);
        assert_eq!(failed.rejected, 1);
        assert!(w.drain_events().is_empty());
        assert!(w
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("deferred destruction")));
        w.gameplay.next_gameplay_edit = sequence;
        let committed = w.step();
        let events = w.drain_events();
        let mut destroyed: Vec<_> = events
            .into_iter()
            .filter_map(|event| match event.payload {
                crate::EventPayload::Destroyed { entity } => {
                    assert_eq!(event.tick, committed.tick);
                    Some(entity)
                }
                _ => None,
            })
            .collect();
        destroyed.sort_unstable();
        let mut expected = vec![root, child];
        expected.sort_unstable();
        assert_eq!(destroyed, expected);
        assert_eq!(w.live_entities(), 0);
        w.step();
        assert!(w.drain_events().is_empty());
    }

    #[test]
    fn the_tick_runs_every_system_once_in_the_declared_order() {
        use SystemKind::*;
        let kinds: Vec<SystemKind> = builtin_schedule()
            .iter()
            .filter_map(|s| match s.runner {
                Runner::Builtin(b) => Some(b.kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                Characters,
                Transforms,
                Commands,
                Animation,
                Rotator,
                Oscillate,
                LifetimeDespawn,
                Emitter,
                Physics,
                Transforms,
                CameraRigs,
                Transforms,
                Follow,
                LookAtCamera,
                Transforms,
                Audio,
            ]
        );
        // Every kind but the step's own frame is a system of the tick.
        for kind in super::super::SYSTEMS {
            let expected = !matches!(kind, Step | Publish);
            assert_eq!(kinds.contains(&kind), expected, "{kind:?}");
        }
    }

    #[test]
    fn edit_mode_runs_only_commands_transforms_and_a_previewed_animation() {
        let mut w = World::new(crate::WorldConfig {
            edit_mode: true,
            ..Default::default()
        });
        let running = |w: &World| -> Vec<SystemKind> {
            builtin_schedule()
                .into_iter()
                .filter_map(|s| match s.runner {
                    Runner::Builtin(b) if (b.runs)(w) => Some(b.kind),
                    _ => None,
                })
                .collect()
        };
        use SystemKind::*;
        let base = [Transforms, Commands, Transforms, Transforms, Transforms];
        assert_eq!(running(&w), base);
        w.edit.animation = true;
        let with_animation = [
            Transforms, Commands, Animation, Transforms, Transforms, Transforms,
        ];
        assert_eq!(
            running(&w),
            if w.modules.animation {
                &with_animation[..]
            } else {
                &base[..]
            }
        );
    }
}
