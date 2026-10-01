//! Executable F0 contract model. Not a shipping ABI or resource service.
//! Identity/readiness/phase models are deliberately small; clock planning
//! uses the actual core FrameClock. Integration remains in F1/F2/F5/F6.
#![allow(dead_code)]

use fynkel_core::clock::FrameClock;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Error {
    Exhausted,
    Stale,
    Phase,
    Pending,
    Failed,
    Cancelled,
}

// The host owns exactly one authority across all its runtime instances.
// Private authorities in independently loaded libraries cannot exchange IDs.
struct IdentityAuthority {
    next: AtomicU64,
    limit: u64,
}

impl IdentityAuthority {
    fn new(limit: u64) -> Self {
        Self {
            next: AtomicU64::new(0),
            limit,
        }
    }

    fn world(&self) -> Result<IdentityWorld, Error> {
        let previous = self
            .next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < self.limit).then(|| n + 1)
            })
            .map_err(|_| Error::Exhausted)?;
        Ok(IdentityWorld {
            id: previous + 1,
            slots: Vec::new(),
            free: Vec::new(),
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntityRef {
    world: u64,
    slot: u32,
    generation: u32,
}

struct Slot {
    generation: u32,
    live: bool,
}

struct IdentityWorld {
    id: u64,
    slots: Vec<Slot>,
    free: Vec<u32>,
}

impl IdentityWorld {
    fn spawn(&mut self) -> Result<EntityRef, Error> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                let slot = u32::try_from(self.slots.len()).map_err(|_| Error::Exhausted)?;
                self.slots.push(Slot {
                    generation: 1,
                    live: false,
                });
                slot
            }
        };
        let record = &mut self.slots[slot as usize];
        record.live = true;
        Ok(EntityRef {
            world: self.id,
            slot,
            generation: record.generation,
        })
    }

    fn valid(&self, entity: EntityRef) -> bool {
        entity.world == self.id
            && self
                .slots
                .get(entity.slot as usize)
                .is_some_and(|s| s.live && s.generation == entity.generation)
    }

    fn destroy(&mut self, entity: EntityRef) -> Result<(), Error> {
        if !self.valid(entity) {
            return Err(Error::Stale);
        }
        let record = &mut self.slots[entity.slot as usize];
        record.live = false;
        if let Some(next) = record.generation.checked_add(1) {
            record.generation = next;
            self.free.push(entity.slot);
        }
        // A generation at MAX retires its slot permanently.
        Ok(())
    }
}

struct Timeline {
    simulation: FrameClock,
    real_elapsed: f64,
    game_elapsed: f64,
    game_delta: f64,
    max_game_delta: f64,
}

impl Timeline {
    fn new() -> Self {
        Self {
            simulation: FrameClock::default(),
            real_elapsed: 0.0,
            game_elapsed: 0.0,
            game_delta: 0.0,
            max_game_delta: 0.25,
        }
    }

    fn begin(&mut self, raw: f64, scale: f64) -> Result<u32, Error> {
        if !scale.is_finite() || scale < 0.0 {
            return Err(Error::Failed);
        }
        if self.simulation.pending_steps() != 0 {
            return Err(Error::Phase);
        }
        let real_delta = if raw.is_finite() { raw.max(0.0) } else { 0.0 };
        // Avoid 0 * infinity when raw and scale are individually finite.
        let game_delta = if scale == 0.0 {
            0.0
        } else {
            (real_delta * scale).min(self.max_game_delta)
        };
        self.real_elapsed += real_delta;
        self.game_delta = game_delta;
        self.game_elapsed += game_delta;
        Ok(self.simulation.begin_frame(game_delta as f32).steps)
    }

    fn run_planned_ticks(&mut self) {
        while self.simulation.pending_steps() > 0 {
            self.simulation.consume_step();
        }
    }

    fn paused_step(&mut self) -> Result<(), Error> {
        if self.game_delta != 0.0 || self.simulation.pending_steps() != 0 {
            return Err(Error::Phase);
        }
        self.simulation.consume_step();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Boundary,
    Fixed,
    Simulation,
    AfterPhysics,
    Update,
    LateUpdate,
    Publish,
}

struct FrameOrder {
    phase: Phase,
    ticks_left: u32,
    completed_tick: u64,
    revision: u64,
    value: i32,
    render_override: Option<i32>,
    published: Option<(u64, u64, i32)>,
}

impl FrameOrder {
    fn new() -> Self {
        Self {
            phase: Phase::Boundary,
            ticks_left: 0,
            completed_tick: 0,
            revision: 0,
            value: 0,
            render_override: None,
            published: None,
        }
    }

    fn begin(&mut self, ticks: u32) -> Result<(), Error> {
        if self.phase != Phase::Boundary {
            return Err(Error::Phase);
        }
        self.render_override = None;
        self.ticks_left = ticks;
        self.phase = if ticks == 0 {
            Phase::Update
        } else {
            Phase::Fixed
        };
        Ok(())
    }

    fn commit(&mut self, new_value: i32) -> Result<(), Error> {
        if !matches!(
            self.phase,
            Phase::Boundary
                | Phase::Fixed
                | Phase::AfterPhysics
                | Phase::Update
                | Phase::LateUpdate
        ) {
            return Err(Error::Phase);
        }
        self.value = new_value;
        self.revision += 1;
        Ok(())
    }

    fn override_visual(&mut self, value: i32) -> Result<(), Error> {
        if !matches!(self.phase, Phase::Update | Phase::LateUpdate) {
            return Err(Error::Phase);
        }
        self.render_override = Some(value);
        Ok(())
    }

    fn next(&mut self) -> Result<(), Error> {
        self.phase = match self.phase {
            Phase::Boundary => return Err(Error::Phase),
            Phase::Fixed => Phase::Simulation,
            Phase::Simulation => {
                self.completed_tick += 1;
                self.ticks_left -= 1;
                Phase::AfterPhysics
            }
            Phase::AfterPhysics => {
                if self.ticks_left == 0 {
                    Phase::Update
                } else {
                    Phase::Fixed
                }
            }
            Phase::Update => Phase::LateUpdate,
            Phase::LateUpdate => Phase::Publish,
            Phase::Publish => {
                self.published = Some((
                    self.completed_tick,
                    self.revision,
                    self.render_override.unwrap_or(self.value),
                ));
                Phase::Boundary
            }
        };
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preparation {
    Pending,
    Ready,
    Failed,
}

struct Dependency {
    required: bool,
    cpu: Preparation,
    simulation: Preparation,
}

struct Load {
    dependencies: BTreeMap<u32, Dependency>,
    cancelled: bool,
    activated: bool,
    // Device generation, not a global ready flag. Loss of a device removes
    // GPU readiness but cannot remove CPU/simulation readiness.
    gpu_ready: BTreeSet<(u64, u32)>,
}

impl Load {
    fn ready(&self) -> Result<(), Error> {
        if self.cancelled {
            return Err(Error::Cancelled);
        }
        if self.dependencies.values().any(|d| {
            d.required && (d.cpu == Preparation::Failed || d.simulation == Preparation::Failed)
        }) {
            return Err(Error::Failed);
        }
        if self.dependencies.values().any(|d| {
            d.required && (d.cpu != Preparation::Ready || d.simulation != Preparation::Ready)
        }) {
            return Err(Error::Pending);
        }
        Ok(())
    }

    fn activate(&mut self, frame: &FrameOrder) -> Result<(), Error> {
        if frame.phase != Phase::Boundary || self.activated {
            return Err(Error::Phase);
        }
        self.ready()?;
        self.activated = true;
        Ok(())
    }

    fn complete_cpu(&mut self, id: u32, result: Preparation) -> Result<(), Error> {
        if self.cancelled {
            return Err(Error::Cancelled);
        }
        let dependency = self.dependencies.get_mut(&id).ok_or(Error::Failed)?;
        if dependency.cpu != Preparation::Pending || result == Preparation::Pending {
            return Err(Error::Phase);
        }
        dependency.cpu = result;
        Ok(())
    }

    fn upload(&mut self, device: u64, id: u32) -> Result<(), Error> {
        if self.cancelled {
            return Err(Error::Cancelled);
        }
        if self
            .dependencies
            .get(&id)
            .is_none_or(|d| d.cpu != Preparation::Ready)
        {
            return Err(Error::Pending);
        }
        self.gpu_ready.insert((device, id));
        Ok(())
    }

    fn cancel(&mut self) -> Result<(), Error> {
        if self.activated {
            return Err(Error::Phase);
        }
        self.cancelled = true;
        self.gpu_ready.clear();
        Ok(())
    }
}

#[derive(Default)]
struct Retention(Arc<Mutex<BTreeMap<u32, usize>>>);

struct RetentionToken {
    counts: Arc<Mutex<BTreeMap<u32, usize>>>,
    ids: BTreeSet<u32>,
}

impl Retention {
    fn acquire(&self, ids: impl IntoIterator<Item = u32>) -> RetentionToken {
        let ids: BTreeSet<_> = ids.into_iter().collect();
        let mut counts = self.0.lock().unwrap();
        for id in &ids {
            *counts.entry(*id).or_default() += 1;
        }
        RetentionToken {
            counts: self.0.clone(),
            ids,
        }
    }

    fn retained(&self, id: u32) -> bool {
        self.0.lock().unwrap().contains_key(&id)
    }
}

impl Drop for RetentionToken {
    fn drop(&mut self) {
        let mut counts = self.counts.lock().unwrap();
        for id in &self.ids {
            let count = counts.get_mut(id).unwrap();
            *count -= 1;
            if *count == 0 {
                counts.remove(id);
            }
        }
    }
}

fn main() {
    println!("F0 identity/clock/frame/readiness contract model: run cargo test --example foundation_contracts");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_worlds_restart_and_multiple_runtimes_share_identity_authority() {
        let authority = IdentityAuthority::new(u64::MAX);
        let mut first = authority.world().unwrap();
        let original = first.spawn().unwrap();
        let mut second_runtime_world = authority.world().unwrap();
        let other = second_runtime_world.spawn().unwrap();
        assert_ne!(original, other);
        assert!(!second_runtime_world.valid(original));
        drop(first);
        let mut restarted = authority.world().unwrap();
        assert_ne!(restarted.spawn().unwrap(), original);
        assert!(!restarted.valid(original));
        // A Play session can create more than 255 worlds without recycling.
        for _ in 0..300 {
            authority.world().unwrap();
        }
        assert!(authority.world().unwrap().id > 255);
        assert_eq!(std::mem::size_of::<EntityRef>(), 16);
    }

    #[test]
    fn authority_exhaustion_never_wraps_or_recycles() {
        let authority = IdentityAuthority::new(2);
        assert_eq!(authority.world().unwrap().id, 1);
        assert_eq!(authority.world().unwrap().id, 2);
        assert!(matches!(authority.world(), Err(Error::Exhausted)));
        assert!(matches!(authority.world(), Err(Error::Exhausted)));
        let authority = IdentityAuthority {
            next: AtomicU64::new(u64::MAX - 1),
            limit: u64::MAX,
        };
        assert_eq!(authority.world().unwrap().id, u64::MAX);
        assert!(matches!(authority.world(), Err(Error::Exhausted)));
    }

    #[test]
    fn slot_reuse_rejects_stale_handles_and_exhausted_generation_retires_slot() {
        let mut world = IdentityAuthority::new(1).world().unwrap();
        let old = world.spawn().unwrap();
        world.destroy(old).unwrap();
        let reused = world.spawn().unwrap();
        assert_eq!(old.slot, reused.slot);
        assert!(!world.valid(old));
        assert_eq!(world.destroy(old), Err(Error::Stale));
        world.slots[reused.slot as usize].generation = u32::MAX;
        let last = EntityRef {
            generation: u32::MAX,
            ..reused
        };
        world.destroy(last).unwrap();
        let next = world.spawn().unwrap();
        assert_ne!(last.slot, next.slot);
        assert!(!world.valid(last));
    }

    #[test]
    fn concurrent_creation_has_one_authority_across_runtimes() {
        let authority = IdentityAuthority::new(u64::MAX);
        let mut ids = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        (0..100)
                            .map(|_| authority.world().unwrap().id)
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().unwrap())
                .collect::<Vec<_>>()
        });
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 400);
    }

    #[test]
    fn hitch_wait_and_tween_use_game_time_while_simulation_reports_only_completed_ticks() {
        let mut clock = Timeline::new();
        let wait_deadline = clock.game_elapsed + 0.1;
        let tween_start = clock.game_elapsed;
        assert_eq!(clock.begin(0.25, 1.0).unwrap(), 4);
        clock.run_planned_ticks();
        assert_eq!(clock.game_elapsed, 0.25);
        assert_eq!(clock.game_delta, 0.25);
        assert!(clock.game_elapsed >= wait_deadline);
        assert_eq!((clock.game_elapsed - tween_start) / 1.0, 0.25);
        assert!(clock.simulation.elapsed() < 0.1);
        assert_eq!(clock.simulation.tick(), 4);
        let before = clock.game_elapsed;
        clock.begin(2.0, 1.0).unwrap();
        clock.run_planned_ticks();
        assert_eq!(clock.game_elapsed - before, 0.25);
        assert_eq!(clock.real_elapsed, 2.25);
    }

    #[test]
    fn zero_tick_pause_step_scale_and_invalid_time() {
        let mut clock = Timeline::new();
        assert_eq!(clock.begin(1.0 / 120.0, 1.0).unwrap(), 0);
        assert_eq!(clock.game_elapsed, 1.0 / 120.0);
        assert_eq!(clock.simulation.elapsed(), 0.0);
        let elapsed = clock.game_elapsed;
        assert_eq!(clock.begin(1.0, 0.0).unwrap(), 0);
        clock.paused_step().unwrap();
        assert_eq!(clock.game_elapsed, elapsed);
        assert_eq!(clock.simulation.tick(), 1);
        clock.begin(0.04, 0.5).unwrap();
        clock.run_planned_ticks();
        assert!((clock.game_elapsed - elapsed - 0.02).abs() < 1e-12);
        assert_eq!(clock.paused_step(), Err(Error::Phase));
        for dt in [-1.0, f64::NAN, f64::INFINITY] {
            clock.begin(dt, 1.0).unwrap();
            assert_eq!(clock.game_delta, 0.0);
        }
        assert_eq!(clock.begin(1.0, f64::NAN), Err(Error::Failed));
        assert_eq!(clock.begin(1.0, -1.0), Err(Error::Failed));
        clock.begin(f64::MAX, f64::MAX).unwrap();
        assert_eq!(clock.game_delta, 0.25);
    }

    #[test]
    fn zero_tick_edits_commit_before_publication_and_visual_override_expires() {
        let mut frame = FrameOrder::new();
        frame.commit(1).unwrap(); // Paused/out-of-frame edits don't step.
        frame.begin(0).unwrap();
        frame.commit(2).unwrap(); // update
        frame.next().unwrap();
        frame.commit(3).unwrap(); // lateUpdate
        frame.override_visual(99).unwrap();
        frame.next().unwrap();
        assert_eq!(frame.commit(4), Err(Error::Phase));
        frame.next().unwrap();
        let previous = frame.published.unwrap();
        assert_eq!(previous, (0, 3, 99));
        assert_eq!(frame.value, 3);
        frame.begin(0).unwrap();
        assert_eq!(frame.render_override, None);
        frame.commit(4).unwrap();
        assert_eq!(previous, (0, 3, 99)); // Published frame is immutable.
    }

    #[test]
    fn catch_up_has_fixed_simulation_after_physics_then_one_update() {
        let mut frame = FrameOrder::new();
        frame.begin(2).unwrap();
        let mut seen = Vec::new();
        while frame.phase != Phase::Boundary {
            seen.push(frame.phase);
            if frame.phase == Phase::Fixed {
                frame.commit(frame.value + 1).unwrap();
            }
            if frame.phase == Phase::Simulation {
                assert_eq!(frame.commit(10), Err(Error::Phase));
            }
            if frame.phase == Phase::AfterPhysics {
                frame.commit(frame.value + 2).unwrap();
            }
            frame.next().unwrap();
        }
        assert_eq!(
            seen,
            vec![
                Phase::Fixed,
                Phase::Simulation,
                Phase::AfterPhysics,
                Phase::Fixed,
                Phase::Simulation,
                Phase::AfterPhysics,
                Phase::Update,
                Phase::LateUpdate,
                Phase::Publish
            ]
        );
        assert_eq!(frame.published, Some((2, 4, 6)));
    }

    fn load() -> Load {
        Load {
            dependencies: BTreeMap::from([
                (
                    1,
                    Dependency {
                        required: true,
                        cpu: Preparation::Pending,
                        simulation: Preparation::Ready,
                    },
                ),
                (
                    2,
                    Dependency {
                        required: false,
                        cpu: Preparation::Failed,
                        simulation: Preparation::Failed,
                    },
                ),
            ]),
            cancelled: false,
            activated: false,
            gpu_ready: BTreeSet::new(),
        }
    }

    #[test]
    fn required_failure_is_terminal_for_request_and_optional_failure_allows_fallback() {
        let mut load = load();
        assert_eq!(load.ready(), Err(Error::Pending));
        load.complete_cpu(1, Preparation::Failed).unwrap();
        assert_eq!(load.activate(&FrameOrder::new()), Err(Error::Failed));
        assert!(!load.activated);
        assert_eq!(load.complete_cpu(1, Preparation::Ready), Err(Error::Phase));
        let mut retry = super::tests::load(); // Retry is a new request.
        retry.complete_cpu(1, Preparation::Ready).unwrap();
        assert_eq!(retry.ready(), Ok(()));
        retry.activate(&FrameOrder::new()).unwrap(); // No GPU needed.
        assert!(retry.activated);
        assert!(retry.gpu_ready.is_empty());
    }

    #[test]
    fn required_simulation_preparation_failure_blocks_activation() {
        let mut load = load();
        load.complete_cpu(1, Preparation::Ready).unwrap();
        load.dependencies.get_mut(&1).unwrap().simulation = Preparation::Failed;
        assert_eq!(load.activate(&FrameOrder::new()), Err(Error::Failed));
        assert!(!load.activated);
    }

    #[test]
    fn ownership_transfer_and_frame_pins_keep_resources_until_final_release() {
        let residency = Retention::default();
        let pending_request = residency.acquire([1, 1, 2]);
        let other_request = residency.acquire([1]);
        drop(other_request); // Cancelled consumer does not evict another's data.
        assert!(residency.retained(1));
        let active_scene = pending_request; // Transfer, never release/reacquire.
        let persistent_entity = residency.acquire([1]);
        let published_frame = residency.acquire([2]);
        drop(active_scene); // Scene unloaded.
        assert!(residency.retained(1));
        assert!(residency.retained(2));
        drop(persistent_entity);
        assert!(!residency.retained(1));
        drop(published_frame); // Render fence/last consumer completed.
        assert!(!residency.retained(2));
    }

    #[test]
    fn cancellation_prevents_late_completion_and_activation() {
        let mut load = load();
        load.cancel().unwrap();
        assert_eq!(
            load.complete_cpu(1, Preparation::Ready),
            Err(Error::Cancelled)
        );
        assert_eq!(load.upload(1, 1), Err(Error::Cancelled));
        assert_eq!(load.activate(&FrameOrder::new()), Err(Error::Cancelled));
        assert!(!load.activated);
    }

    #[test]
    fn activation_requires_boundary_and_device_loss_preserves_simulation_readiness() {
        let mut load = load();
        load.complete_cpu(1, Preparation::Ready).unwrap();
        let mut frame = FrameOrder::new();
        frame.begin(0).unwrap();
        assert_eq!(load.activate(&frame), Err(Error::Phase));
        load.upload(10, 1).unwrap();
        assert!(load.gpu_ready.contains(&(10, 1)));
        assert!(!load.gpu_ready.contains(&(11, 1)));
        load.gpu_ready.retain(|(device, _)| *device != 10);
        assert_eq!(load.ready(), Ok(()));
        load.upload(11, 1).unwrap();
        assert!(!load.gpu_ready.contains(&(10, 1)));
        assert!(load.gpu_ready.contains(&(11, 1)));
    }
}
