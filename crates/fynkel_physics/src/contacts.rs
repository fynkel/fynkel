//! Who touches whom (docs/stage-5.md §3.4 and §3.5): after each step, the
//! pairs of followed colliders that touch, as Unity's trigger and collision
//! messages see them, against those that touched after the step before:
//! which started (enter), go on (stay) and ended (exit). Only pairs with a
//! collider that listens are looked at, so a world where nothing listens
//! pays nothing.
//!
//! Pairs are known by the colliders' keys, the caller's numbers for them,
//! which stay the same when a collider is put on another body (taken out
//! of Rapier and put in again): that pair goes on touching. A pair ends
//! when its colliders stop touching, or either is removed, gets a layer
//! that doesn't meet the other's, or becomes or stops being a trigger.
//!
//! Also here: the layer matrix, and pairs of colliders that ignore each
//! other.

use std::collections::{HashMap, HashSet};

use rapier3d::prelude::*;

use crate::{Physics, PhysicsError, Vector};

/// What a collider listens for, as [`crate::ColliderSpec::listens`] has it.
pub const LISTEN_TRIGGERS: u32 = 1;
pub const LISTEN_COLLISIONS: u32 = 2;

/// Where a pair of colliders is in its touching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactKind {
    /// They touch, and didn't after the step before.
    Enter,
    /// They touched after the step before and still do, and at least one
    /// is on a body that's awake.
    Stay,
    /// They touched after the step before and don't now.
    Exit,
}

/// A pair of colliders, `a` and `b` by their keys (`a` < `b`), starting,
/// going on with or ending their touching. A collision's [`impulse`],
/// [`relative_velocity`] and points are as `a` sees them: from `b`'s side
/// each is the other way. An exit has none.
///
/// [`impulse`]: Self::impulse
/// [`relative_velocity`]: Self::relative_velocity
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactEvent {
    pub kind: ContactKind,
    /// A trigger pair (at least one is a trigger) rather than a collision.
    pub trigger: bool,
    pub a: u64,
    pub b: u64,
    /// The impulse the step gave `a`'s body to keep the two apart.
    pub impulse: Vector,
    /// How fast `b` moved relative to `a` before the step: `b`'s body's
    /// velocity less `a`'s (a static collider's is zero).
    pub relative_velocity: Vector,
    /// Its points: [`Physics::contact_points`] from `first_point`, `point_count` of them.
    pub first_point: u32,
    pub point_count: u32,
}

/// Where two colliders touch, as `a` sees it: the point, the normal
/// pointing from `b` towards `a`, and how far apart they are there
/// (negative: how deep they're in each other).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactPoint {
    pub point: Vector,
    pub normal: Vector,
    pub separation: f32,
}

#[derive(Clone, Copy, Debug)]
struct Pair {
    a: u64,
    b: u64,
    trigger: bool,
    // Their handles now; stale for a pair that ended.
    ha: ColliderHandle,
    hb: ColliderHandle,
}

impl Pair {
    fn order(&self) -> (u64, u64, bool) {
        (self.a, self.b, self.trigger)
    }
}

/// The contact bookkeeping of a world.
#[derive(Debug)]
pub(crate) struct Contacts {
    /// How many followed colliders listen for anything.
    pub(crate) listening: usize,
    // The pairs that touched after the last step, in order.
    touching: Vec<Pair>,
    // Scratch, kept for its room.
    current: Vec<Pair>,
    enters: Vec<Pair>,
    stays: Vec<Pair>,
    exits: Vec<Pair>,
    events: Vec<ContactEvent>,
    points: Vec<ContactPoint>,
    // Each body's velocity before the step, by its handle's index, with the
    // step it was read at; a body not read then (asleep, fixed) had none.
    velocities: Vec<(u64, Vector)>,
    steps: u64,
    /// Which layers meet which: bit `b` of `layers[a]`.
    pub(crate) layers: [u32; 32],
    /// The pairs of keys that ignore each other (smaller first), and how
    /// many such pairs each key is in.
    ignored: HashSet<(u64, u64)>,
    ignoring: HashMap<u64, u32>,
}

impl Default for Contacts {
    fn default() -> Self {
        Self {
            listening: 0,
            touching: Vec::new(),
            current: Vec::new(),
            enters: Vec::new(),
            stays: Vec::new(),
            exits: Vec::new(),
            events: Vec::new(),
            points: Vec::new(),
            velocities: Vec::new(),
            steps: 0,
            layers: [u32::MAX; 32],
            ignored: HashSet::new(),
            ignoring: HashMap::new(),
        }
    }
}

impl Contacts {
    /// The groups of a collider on `layer`: its layer, and the layers it
    /// meets. Queries look at the first only.
    pub(crate) fn groups(&self, layer: u32) -> InteractionGroups {
        InteractionGroups::new(
            Group::from_bits_retain(1 << layer),
            Group::from_bits_retain(self.layers[layer as usize]),
            InteractionTestMode::And,
        )
    }

    /// The hooks a collider with `key` needs: the pair filters, if it
    /// ignores another.
    pub(crate) fn hooks(&self, key: u64) -> ActiveHooks {
        if self.ignoring.contains_key(&key) {
            ActiveHooks::FILTER_CONTACT_PAIRS | ActiveHooks::FILTER_INTERSECTION_PAIR
        } else {
            ActiveHooks::empty()
        }
    }

    /// Whether the colliders with keys `a` and `b` ignore each other.
    pub(crate) fn ignores_keys(&self, a: u64, b: u64) -> bool {
        !self.ignored.is_empty() && self.ignored.contains(&(a.min(b), a.max(b)))
    }

    pub(crate) fn filter(&self) -> IgnoredPairs<'_> {
        IgnoredPairs(&self.ignored)
    }
}

/// Rapier's pair filters, for the pairs that ignore each other.
pub(crate) struct IgnoredPairs<'a>(&'a HashSet<(u64, u64)>);

impl IgnoredPairs<'_> {
    fn meet(&self, context: &PairFilterContext) -> bool {
        let key = |h: ColliderHandle| context.colliders.get(h).map(|c| c.user_data as u64);
        match (key(context.collider1), key(context.collider2)) {
            (Some(a), Some(b)) => !self.0.contains(&(a.min(b), a.max(b))),
            _ => true,
        }
    }
}

impl PhysicsHooks for IgnoredPairs<'_> {
    fn filter_contact_pair(&self, context: &PairFilterContext) -> Option<SolverFlags> {
        self.meet(context).then_some(SolverFlags::COMPUTE_RIGID_IMPULSES)
    }

    fn filter_intersection_pair(&self, context: &PairFilterContext) -> bool {
        self.meet(context)
    }
}

impl Physics {
    /// Whether colliders on layers `a` and `b` meet (Unity's
    /// `!Physics.GetIgnoreLayerCollision`): all do, unless
    /// [`Self::set_layer_collision`] said otherwise.
    pub fn layer_collision(&self, a: u32, b: u32) -> Result<bool, PhysicsError> {
        check_layer(a)?;
        check_layer(b)?;
        Ok(self.contacts.layers[a as usize] & (1 << b) != 0)
    }

    /// Makes colliders on layers `a` and `b` meet or not (Unity's
    /// `Physics.IgnoreLayerCollision`), from the next step. Queries don't
    /// look at it.
    pub fn set_layer_collision(&mut self, a: u32, b: u32, meet: bool) -> Result<(), PhysicsError> {
        check_layer(a)?;
        check_layer(b)?;
        if self.layer_collision(a, b)? == meet {
            return Ok(());
        }
        for (x, y) in [(a, b), (b, a)] {
            let row = &mut self.contacts.layers[x as usize];
            *row = if meet { *row | (1 << y) } else { *row & !(1 << y) };
        }
        for index in 0..self.followed.len() {
            let Some(followed) = &self.followed[index] else { continue };
            let layer = followed.spec.layer;
            if layer != a && layer != b {
                continue;
            }
            let groups = self.contacts.groups(layer);
            if let Some((collider, _)) = self.world.colliders.get_unknown_gen_mut(index as u32) {
                collider.set_collision_groups(groups);
            }
        }
        Ok(())
    }

    /// Whether two followed colliders ignore each other (Unity's
    /// `Physics.GetIgnoreCollision`).
    pub fn ignores(&self, collider1: u64, collider2: u64) -> Result<bool, PhysicsError> {
        let (a, b) = (self.followed_key(collider1)?, self.followed_key(collider2)?);
        Ok(self.contacts.ignored.contains(&(a.min(b), a.max(b))))
    }

    /// Makes two followed colliders ignore each other, or not (Unity's
    /// `Physics.IgnoreCollision`): no contact, no trigger, from the next
    /// step. It holds, by their keys, until [`Self::forget_key`].
    pub fn set_ignores(&mut self, collider1: u64, collider2: u64, ignore: bool) -> Result<(), PhysicsError> {
        let (h1, h2) = (self.collider_handle(collider1)?, self.collider_handle(collider2)?);
        let (a, b) = (self.followed_key(collider1)?, self.followed_key(collider2)?);
        if a == b {
            return Err(PhysicsError::Value("a collider can't ignore itself".into()));
        }
        let pair = (a.min(b), a.max(b));
        let c = &mut self.contacts;
        let changed = if ignore { c.ignored.insert(pair) } else { c.ignored.remove(&pair) };
        if !changed {
            return Ok(());
        }
        for key in [a, b] {
            if ignore {
                *c.ignoring.entry(key).or_insert(0) += 1;
            } else if let Some(count) = c.ignoring.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    c.ignoring.remove(&key);
                }
            }
        }
        for (handle, key) in [(h1, a), (h2, b)] {
            let hooks = self.contacts.hooks(key);
            self.world.colliders[handle].set_active_hooks(hooks);
        }
        Ok(())
    }

    /// Forgets the pairs the collider with `key` ignores, for a key that
    /// will be used again for another collider.
    pub fn forget_key(&mut self, key: u64) {
        let c = &mut self.contacts;
        if !c.ignoring.contains_key(&key) {
            return;
        }
        let pairs: Vec<_> = c.ignored.iter().copied().filter(|&(a, b)| a == key || b == key).collect();
        for (a, b) in pairs {
            c.ignored.remove(&(a, b));
            let other = if a == key { b } else { a };
            if let Some(count) = c.ignoring.get_mut(&other) {
                *count -= 1;
                if *count == 0 {
                    c.ignoring.remove(&other);
                }
            }
        }
        c.ignoring.remove(&key);
        // Colliders still in the world that no longer ignore anything keep
        // the filter hooks, which then let every pair through.
    }

    /// The contact events of the last step: enters, then stays, then
    /// exits, each in the order of their keys.
    pub fn contact_events(&self) -> &[ContactEvent] {
        &self.contacts.events
    }

    /// The points of the last step's collision events.
    pub fn contact_points(&self) -> &[ContactPoint] {
        &self.contacts.points
    }

    pub(crate) fn listening_changed(&mut self, before: u32, after: u32) {
        match (before != 0, after != 0) {
            (false, true) => self.contacts.listening += 1,
            (true, false) => self.contacts.listening -= 1,
            _ => {}
        }
    }

    /// Before a step: each moving body's velocity, for the collisions'
    /// relative velocities.
    pub(crate) fn contacts_before_step(&mut self) {
        let c = &mut self.contacts;
        c.steps += 1;
        if c.listening == 0 {
            return;
        }
        let w = &self.world;
        for handle in w.islands.active_bodies() {
            let Some(body) = w.bodies.get(handle) else { continue };
            let index = handle.into_raw_parts().0 as usize;
            if c.velocities.len() <= index {
                c.velocities.resize(index + 1, (0, Vector::ZERO));
            }
            c.velocities[index] = (c.steps, body.linvel());
        }
    }

    /// After a step: the pairs that touch now, against those that touched
    /// before, as events.
    pub(crate) fn contacts_after_step(&mut self) {
        let c = &mut self.contacts;
        c.events.clear();
        c.points.clear();
        if c.listening == 0 && c.touching.is_empty() {
            return;
        }
        c.current.clear();
        let w = &self.world;
        for (index, slot) in self.followed.iter().enumerate() {
            let Some(followed) = slot else { continue };
            let listens = followed.spec.listens;
            if listens == 0 {
                continue;
            }
            let Some((_, handle)) = w.colliders.get_unknown_gen(index as u32) else { continue };
            if listens & LISTEN_COLLISIONS != 0 {
                for pair in w.narrow_phase.contact_pairs_with(handle) {
                    if pair.has_any_active_contact()
                        && let Some(p) = meeting(w, &self.followed, pair.collider1, pair.collider2, false)
                    {
                        c.current.push(p);
                    }
                }
            }
            if listens & LISTEN_TRIGGERS != 0 {
                for (h1, h2, touching) in w.narrow_phase.intersection_pairs_with(handle) {
                    if touching && let Some(p) = meeting(w, &self.followed, h1, h2, true) {
                        c.current.push(p);
                    }
                }
            }
        }
        c.current.sort_unstable_by_key(Pair::order);
        c.current.dedup_by_key(|p| p.order());
        c.enters.clear();
        c.stays.clear();
        c.exits.clear();
        let (mut i, mut j) = (0, 0);
        while i < c.touching.len() || j < c.current.len() {
            let before = c.touching.get(i).map(Pair::order);
            let now = c.current.get(j).map(Pair::order);
            match (before, now) {
                (Some(x), Some(y)) if x == y => {
                    c.stays.push(c.current[j]);
                    i += 1;
                    j += 1;
                }
                (Some(x), Some(y)) if x < y => {
                    c.exits.push(c.touching[i]);
                    i += 1;
                }
                (Some(_), None) => {
                    c.exits.push(c.touching[i]);
                    i += 1;
                }
                _ => {
                    c.enters.push(c.current[j]);
                    j += 1;
                }
            }
        }
        std::mem::swap(&mut c.touching, &mut c.current);
        for k in 0..c.enters.len() {
            let pair = c.enters[k];
            push_event(w, c, ContactKind::Enter, pair);
        }
        for k in 0..c.stays.len() {
            let pair = c.stays[k];
            if awake(w, pair.ha) || awake(w, pair.hb) {
                push_event(w, c, ContactKind::Stay, pair);
            }
        }
        for k in 0..c.exits.len() {
            let pair = c.exits[k];
            let Pair { a, b, trigger, .. } = pair;
            c.events.push(ContactEvent {
                kind: ContactKind::Exit,
                trigger,
                a,
                b,
                impulse: Vector::ZERO,
                relative_velocity: Vector::ZERO,
                first_point: 0,
                point_count: 0,
            });
        }
    }

    fn followed_key(&self, id: u64) -> Result<u64, PhysicsError> {
        let handle = self.followed_handle(id)?;
        Ok(self.world.colliders[handle].user_data as u64)
    }
}

fn check_layer(layer: u32) -> Result<(), PhysicsError> {
    if layer < 32 { Ok(()) } else { Err(PhysicsError::Value(format!("a layer is 0 to 31, not {layer}"))) }
}

/// The body under a collider, if it moves: dynamic or kinematic.
fn moving_body(w: &PhysicsWorld, handle: ColliderHandle) -> Option<&RigidBody> {
    let body = w.bodies.get(w.colliders.get(handle)?.parent()?)?;
    (!body.is_fixed()).then_some(body)
}

fn awake(w: &PhysicsWorld, handle: ColliderHandle) -> bool {
    moving_body(w, handle).is_some_and(|b| !b.is_sleeping())
}

/// Two touching followed colliders as a pair, if Unity's table sends them
/// messages: a trigger pair needs a moving body on one side, a collision a
/// dynamic one.
fn meeting(
    w: &PhysicsWorld,
    followed: &[Option<crate::colliders::Followed>],
    h1: ColliderHandle,
    h2: ColliderHandle,
    trigger: bool,
) -> Option<Pair> {
    let (Some(Some(f1)), Some(Some(f2))) =
        (followed.get(h1.into_raw_parts().0 as usize), followed.get(h2.into_raw_parts().0 as usize))
    else {
        return None;
    };
    let (b1, b2) = (moving_body(w, h1), moving_body(w, h2));
    let sends = if trigger {
        b1.is_some() || b2.is_some() || f1.character || f2.character
    } else {
        b1.is_some_and(RigidBody::is_dynamic) || b2.is_some_and(RigidBody::is_dynamic)
    };
    if !sends {
        return None;
    }
    let k1 = w.colliders[h1].user_data as u64;
    let k2 = w.colliders[h2].user_data as u64;
    Some(if k1 < k2 {
        Pair { a: k1, b: k2, trigger, ha: h1, hb: h2 }
    } else {
        Pair { a: k2, b: k1, trigger, ha: h2, hb: h1 }
    })
}

fn velocity_before(w: &PhysicsWorld, c: &Contacts, handle: ColliderHandle) -> Vector {
    let Some(body) = w.colliders.get(handle).and_then(Collider::parent) else { return Vector::ZERO };
    match c.velocities.get(body.into_raw_parts().0 as usize) {
        Some(&(step, velocity)) if step == c.steps => velocity,
        _ => Vector::ZERO,
    }
}

fn push_event(w: &PhysicsWorld, c: &mut Contacts, kind: ContactKind, pair: Pair) {
    let Pair { a, b, trigger, ha, hb } = pair;
    let mut event = ContactEvent {
        kind,
        trigger,
        a,
        b,
        impulse: Vector::ZERO,
        relative_velocity: Vector::ZERO,
        first_point: c.points.len() as u32,
        point_count: 0,
    };
    if !trigger {
        event.relative_velocity = velocity_before(w, c, hb) - velocity_before(w, c, ha);
        if let Some(contact) = w.narrow_phase.contact_pair(ha, hb)
            && let Some(co1) = w.colliders.get(contact.collider1)
        {
            // Rapier's normal points from its first collider to its second,
            // and the impulse it gives pushes the second along it.
            let a_first = contact.collider1 == ha;
            let towards_a = if a_first { -1.0 } else { 1.0 };
            event.impulse = contact.total_impulse() * towards_a;
            if let PairContacts::Rigid(rigid) = &contact.contacts {
                let pose = co1.position();
                for manifold in &rigid.manifolds {
                    let normal = pose.rotation * manifold.local_n1 * towards_a;
                    for point in &manifold.points {
                        c.points.push(ContactPoint {
                            point: pose.transform_point(point.local_p1),
                            normal,
                            separation: point.dist,
                        });
                    }
                }
            }
            event.point_count = c.points.len() as u32 - event.first_point;
        }
    }
    c.events.push(event);
}
