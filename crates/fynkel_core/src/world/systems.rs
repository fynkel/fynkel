//! The tick's native systems (§5.1) and the one place their order is
//! declared (foundation 6.1). `World::step` runs [`TICK`] front to back;
//! each system says when it runs and reports its entity count, and the
//! step times it under its [`SystemKind`].
//!
//! Transform work comes in four pieces between the others (S[n] recorded,
//! the tick's propagate, and two settles after the systems that move
//! things late); they report together as one `Transforms` entry.

use std::time::Instant;

use fynkel_contracts::{ComponentId, Entity, Tick};
use fynkel_scene as scene;

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

/// The tick, in order. `Step` and `Publish` are the step's own frame
/// around it.
pub(super) const TICK: [Stage; 16] = [
    // Native pre-tick systems read the tick's input first (the characters'
    // action driver, M3-09): its commands are queued ahead of the batch's,
    // so an explicit `Move`/`Jump` from the game in the same tick wins.
    stage::<Characters>(),
    stage::<RecordPrevious>(),
    stage::<Commands>(),
    // Animators pose their subtrees first among the pre-tick systems
    // (M3-19): a mover or a body on a posed joint follows the pose.
    stage::<Animation>(),
    // The catalogue's pre-tick movers (§3.5, M3-10): they see the batch's
    // writes and physics sees theirs.
    stage::<Rotators>(),
    stage::<Oscillators>(),
    stage::<Lifetimes>(),
    stage::<Emitters>(),
    stage::<PhysicsStep>(),
    stage::<Propagate>(),
    // Camera rigs follow the tick's final poses (a rig's own subtree is
    // the only thing left dirty, so the settle after it is cheap).
    stage::<CameraRigs>(),
    stage::<SettleRigs>(),
    // Followers and billboards read the propagated poses, the camera's
    // included (M3-10).
    stage::<Followers>(),
    stage::<Billboards>(),
    stage::<Settle>(),
    // After the destroys of the tick, so a dead source's voice stops this
    // tick.
    stage::<Audio>(),
];

impl World {
    /// Runs [`TICK`] for tick `tick`; what it applied and rejected.
    pub(super) fn run_tick(&mut self, tick: Tick) -> TickRun {
        let mut t = TickRun {
            tick,
            applied: 0,
            rejected: 0,
            propagated: 0,
            transforms_us: 0,
            moved: false,
        };
        for stage in &TICK {
            let t0 = Instant::now();
            let n = if (stage.runs)(self) {
                (stage.run)(self, &mut t)
            } else {
                0
            };
            if stage.kind == SystemKind::Transforms {
                t.transforms_us += micros(t0);
                self.set_time(SystemKind::Transforms, t.propagated, t.transforms_us);
            } else {
                self.set_time(stage.kind, n, micros(t0));
            }
        }
        t
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
        w.record_prev_world();
        0
    }
}

/// Validates and applies C[n] at the tick boundary.
struct Commands;
impl System for Commands {
    const KIND: SystemKind = SystemKind::Commands;
    fn runs(_: &World) -> bool {
        true
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        let staged = std::mem::take(&mut w.lanes.staged);
        w.lanes.spawned_this_tick.clear();
        for (index, cmd, from_presentation) in staged {
            let cmd = match cmd {
                Ok(cmd) => cmd,
                Err(r) => {
                    w.reject(
                        index,
                        Lane::Simulation,
                        r.code,
                        Entity::NULL,
                        ComponentId::NONE,
                        r.message,
                    );
                    t.rejected += 1;
                    continue;
                }
            };
            let Some(cmd) = w.resolve_temps(index, cmd) else {
                t.rejected += 1;
                continue;
            };
            let target = cmd.target();
            if w.apply_simulation(index, cmd, t.tick) {
                if from_presentation {
                    w.lanes.teleported.extend(target);
                }
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
    fn run(w: &mut World, _: &mut TickRun) -> u32 {
        w.run_rotators()
    }
}

struct Oscillators;
impl System for Oscillators {
    const KIND: SystemKind = SystemKind::Oscillate;
    fn runs(w: &World) -> bool {
        !w.edit()
    }
    fn run(w: &mut World, _: &mut TickRun) -> u32 {
        w.run_oscillators()
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
        !w.edit() && w.modules.physics && w.physics.has_work(&w.ecs)
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        t.propagated += scene::propagate(&mut w.ecs) as u32;
        w.physics
            .step(&mut w.ecs, w.config.fixed_dt, &w.subscriptions);
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
/// PrevWorld == current so they do not interpolate from the origin.
struct Propagate;
impl System for Propagate {
    const KIND: SystemKind = SystemKind::Transforms;
    fn runs(_: &World) -> bool {
        true
    }
    fn run(w: &mut World, t: &mut TickRun) -> u32 {
        for e in w.ecs.take_deferred() {
            if w.ecs.contains(e) {
                let _ = w.destroy_subtree(e, t.tick);
            }
        }
        t.propagated += scene::propagate(&mut w.ecs) as u32;
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
        let (rigs, moved) = w.place_camera_rigs();
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
            t.propagated += scene::propagate(&mut w.ecs) as u32;
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
        let (n, moved) = w.run_followers();
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
        let (n, moved) = w.run_billboards();
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
            t.propagated += scene::propagate(&mut w.ecs) as u32;
        }
        if w.modules.physics {
            w.physics.record(&w.ecs);
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
    fn the_tick_runs_every_system_once_in_the_declared_order() {
        use SystemKind::*;
        let kinds: Vec<SystemKind> = TICK.iter().map(|s| s.kind).collect();
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
            TICK.iter()
                .filter(|s| (s.runs)(w))
                .map(|s| s.kind)
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
