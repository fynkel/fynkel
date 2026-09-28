//! Pull-only events in two lanes (ADR-0008).

use fynkel_contracts::{AssetRef, Entity, EventKind, Tick, Uuid};
pub use fynkel_physics::Contact;

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
    SceneReady {
        scene: Uuid,
    },
    /// M3-12: a scene's still-alive entities were destroyed (their
    /// `Destroyed` events come first).
    SceneUnloaded {
        scene: Uuid,
        entities: u32,
    },
    /// M3-12: a `TriggerZone`'s sensor saw `other` enter / leave; general
    /// lane, produced with the tick's physics events.
    ZoneEntered {
        zone: Entity,
        other: Entity,
        event: String,
    },
    ZoneExited {
        zone: Entity,
        other: Entity,
        event: String,
    },
    /// M3-19: an `Animator`'s non-looping clip reached its end this tick
    /// (`playing` is cleared with it).
    AnimationEnded {
        entity: Entity,
        clip: AssetRef,
    },
    /// M3-19c: an `Animator`'s clock crossed a marker of `clip` this tick.
    AnimationMarker {
        entity: Entity,
        clip: AssetRef,
        name: String,
    },
    /// ADR-0032: a layer of the `Animator`'s controller entered `state`
    /// this tick, leaving `from` (empty on the first tick).
    AnimatorStateEntered {
        entity: Entity,
        layer: u32,
        state: String,
        from: String,
    },
    /// ADR-0032: a layer of the `Animator`'s controller stopped playing
    /// `state` this tick (its crossfade out ended).
    AnimatorStateExited {
        entity: Entity,
        layer: u32,
        state: String,
    },
    /// M3-11: an `AudioSource`'s non-looping clip reached its end this
    /// tick (`playing` is cleared with it).
    PlaybackEnded {
        entity: Entity,
        clip: AssetRef,
    },
    // Physics lane (M3-08): produced by the physics step, drained by
    // `after_physics`.
    CollisionEnter(Contact),
    CollisionStay(Contact),
    CollisionExit(Contact),
    TriggerEnter(Contact),
    TriggerExit(Contact),
    TriggerStay(Contact),
    ControllerHit(Contact),
}

impl EventPayload {
    pub fn kind(&self) -> EventKind {
        match self {
            EventPayload::Spawned { .. } => EventKind::SPAWNED,
            EventPayload::Destroyed { .. } => EventKind::DESTROYED,
            EventPayload::ParentChanged { .. } => EventKind::PARENT_CHANGED,
            EventPayload::MissedDeadline { .. } => EventKind::MISSED_DEADLINE,
            EventPayload::SceneReady { .. } => EventKind::SCENE_READY,
            EventPayload::SceneUnloaded { .. } => EventKind::SCENE_UNLOADED,
            EventPayload::ZoneEntered { .. } => EventKind::ZONE_ENTERED,
            EventPayload::ZoneExited { .. } => EventKind::ZONE_EXITED,
            EventPayload::AnimationEnded { .. } => EventKind::ANIMATION_ENDED,
            EventPayload::AnimationMarker { .. } => EventKind::ANIMATION_MARKER,
            EventPayload::AnimatorStateEntered { .. } => EventKind::ANIMATOR_STATE_ENTERED,
            EventPayload::AnimatorStateExited { .. } => EventKind::ANIMATOR_STATE_EXITED,
            EventPayload::PlaybackEnded { .. } => EventKind::PLAYBACK_ENDED,
            EventPayload::CollisionEnter(_) => EventKind::COLLISION_ENTER,
            EventPayload::CollisionStay(_) => EventKind::COLLISION_STAY,
            EventPayload::CollisionExit(_) => EventKind::COLLISION_EXIT,
            EventPayload::TriggerEnter(_) => EventKind::TRIGGER_ENTER,
            EventPayload::TriggerExit(_) => EventKind::TRIGGER_EXIT,
            EventPayload::TriggerStay(_) => EventKind::TRIGGER_STAY,
            EventPayload::ControllerHit(_) => EventKind::CONTROLLER_HIT,
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

    /// [`EventQueue::drain`] into `out` (cleared first): the queue and
    /// `out` keep their capacity, so a warm drain allocates nothing
    /// (foundation 4.8).
    pub fn drain_into(&mut self, out: &mut Vec<Event>) {
        out.clear();
        out.append(&mut self.general);
        out.append(&mut self.physics_undrained);
    }

    pub fn pending(&self) -> usize {
        self.general.len() + self.physics_undrained.len()
    }
}
