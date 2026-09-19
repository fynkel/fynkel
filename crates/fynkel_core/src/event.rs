//! Pull-only events in two lanes (ADR-0008).

use fynkel_contracts::{Entity, EventKind, Tick};

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Tick that produced the event (the tick number *after* stepping).
    pub tick: Tick,
    pub payload: EventPayload,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventPayload {
    Spawned {
        temp_id: u64,
        entity: Entity,
    },
    Destroyed {
        entity: Entity,
    },
    ParentChanged {
        child: Entity,
        parent: Option<Entity>,
    },
    MissedDeadline {
        dropped_seconds: f32,
    },
    SceneReady,
    // Physics lane (no producer in M1; the lane exists so the contract does).
    CollisionEnter {
        a: Entity,
        b: Entity,
    },
    CollisionStay {
        a: Entity,
        b: Entity,
    },
    CollisionExit {
        a: Entity,
        b: Entity,
    },
    TriggerEnter {
        a: Entity,
        b: Entity,
    },
    TriggerExit {
        a: Entity,
        b: Entity,
    },
}

impl EventPayload {
    pub fn kind(&self) -> EventKind {
        match self {
            EventPayload::Spawned { .. } => EventKind::SPAWNED,
            EventPayload::Destroyed { .. } => EventKind::DESTROYED,
            EventPayload::ParentChanged { .. } => EventKind::PARENT_CHANGED,
            EventPayload::MissedDeadline { .. } => EventKind::MISSED_DEADLINE,
            EventPayload::SceneReady => EventKind::SCENE_READY,
            EventPayload::CollisionEnter { .. } => EventKind::COLLISION_ENTER,
            EventPayload::CollisionStay { .. } => EventKind::COLLISION_STAY,
            EventPayload::CollisionExit { .. } => EventKind::COLLISION_EXIT,
            EventPayload::TriggerEnter { .. } => EventKind::TRIGGER_ENTER,
            EventPayload::TriggerExit { .. } => EventKind::TRIGGER_EXIT,
        }
    }

    pub fn is_physics(&self) -> bool {
        self.kind().is_physics()
    }
}

/// General lane: queued until the next drain. Physics lane: events of the
/// last stepped tick, readable in `afterPhysics` and included in the next
/// drain, then discarded.
#[derive(Default, Debug)]
pub(crate) struct EventQueue {
    general: Vec<Event>,
    physics_current: Vec<Event>,
    physics_undrained: Vec<Event>,
}

impl EventQueue {
    pub fn push(&mut self, event: Event) {
        if event.payload.is_physics() {
            self.physics_current.push(event);
        } else {
            self.general.push(event);
        }
    }

    /// Called at the end of `step()`: tick n's physics events become
    /// readable via `after_physics` and are queued for the next drain.
    pub fn publish_tick(&mut self) {
        self.physics_undrained
            .extend(self.physics_current.iter().cloned());
    }

    pub fn begin_tick(&mut self) {
        self.physics_current.clear();
    }

    pub fn after_physics(&self) -> &[Event] {
        &self.physics_current
    }

    pub fn drain(&mut self) -> Vec<Event> {
        let mut out = std::mem::take(&mut self.general);
        out.append(&mut self.physics_undrained);
        out
    }

    pub fn pending(&self) -> usize {
        self.general.len() + self.physics_undrained.len()
    }
}
