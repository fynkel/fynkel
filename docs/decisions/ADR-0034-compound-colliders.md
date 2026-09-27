# ADR-0034: Child colliders join their parent's Rigidbody

**Status:** Accepted 2026-09-27 (the user confirmed it; docs/foundation.md 2.1)  
**Date:** 2026-09-27  
**Related:** blueprint §9 (physics), `docs/physics.md` § Components ("one collider per entity in V1") and § Contacts, ADR-0008 (transform ownership), ADR-0020 (an object's layer), ADR-0012 (schema codegen)

## Context

In Unity a `Collider` on a child of a `Rigidbody` is part of that body, a
**compound**. The child moves with the body, its shape stops the body,
and the body's mass carries it. That is how a car (a box and four
wheels), a character with a sword, or a table (a top and four legs) is
built. In Fynkel every entity with a `Collider` is its own Rapier object.
A collider without a `RigidBody` is static geometry, even when its parent
is a dynamic body. So a body whose only collider is on a child falls
through the floor, and the child's collider stays where the child was
spawned while the body falls away without it.
`crates/fynkel_core/tests/compound.rs` measured it: the body was at
y −75 after 4 s instead of resting at 1.5. `fynkel validate` even warns
(`H0004`) that such a body "falls through everything".

## Decision

1. **Which colliders join.** A `Collider` on an entity that has no
   `RigidBody` and no `CharacterController` joins the body of its
   **nearest ancestor with a `RigidBody`** that is active in the
   hierarchy (Unity's rule), whatever that body's type (static,
   dynamic or kinematic). If no ancestor has one, it stays static
   geometry, as today. A child with its own `RigidBody` is its own body,
   as in Unity. A `CharacterController` ancestor does not take child
   colliders: its capsule is its whole shape (unchanged).
2. **The shape.** The body's shape is its own collider, if it has one,
   plus every joined collider. Each is placed where its entity is
   relative to the body (its `offset` included) and sized by its own
   entity's world scale. Each keeps its own `friction`/`material`,
   `isSensor` and `mask`, and its own object's layer (ADR-0020).
   The body's `mass` is still the `RigidBody`'s value. The centre of
   mass and inertia come from all of its shapes. A joined `mesh`
   collider on a moving body is `MESH_ON_MOVING_BODY`, as today.
3. **Transforms.** Physics never writes a joined child's transform:
   the child follows the body through the hierarchy. Moving the child (a
   transform command, an animation) moves its shape within the body at
   the next tick. Activating, deactivating, adding or removing the child
   or its `Collider` rebuilds the body's shape and keeps its velocity.
   Reparenting the child from one body to another does the same.
3a. **Ownership.** Only the body entity is physics-owned (ADR-0008).
   A joined child is not: a presentation write on it is allowed and
   moves it within the body, as the child of any moving object.
4. **Contacts** say which body each side belongs to. The
   `fynkel.ev.Contact` event gains `bodyA` and `bodyB`: the entity whose
   `RigidBody` the collider on each side belongs to. For a collider
   with its own `RigidBody`, that is its own entity. For static geometry,
   it is null (0). `a`/`b` stay the colliders' entities (Unity's
   `Collision.collider`). The record grows from 48 to 64 bytes, so the
   schema fingerprint changes and caches recook. In Dart,
   `Contact.bodyA`/`bodyB` and `Contact.bodyOf(entity)` are added, and
   `other(entity)`/`normalFor(entity)` also accept a side's body.
   Collision and trigger callbacks reach the collider's object **and**,
   when it is a different object, its body's object, once each. In
   Unity, a compound's `OnCollisionEnter` reaches the Rigidbody's
   object, and triggers reach both.
5. **Queries.** A raycast, shape cast or overlap hit keeps naming the
   collider's entity (Unity's `RaycastHit.collider`). A `body` field on
   the hit (Unity's `RaycastHit.rigidbody`) is left for later
   (foundation 2.9 territory).
6. **Validation.** `H0004` ("a dynamic body without a collider") counts
   the colliders in the body's subtree that would join it. The editor's
   gizmos draw a joined collider where its child is (already true: they
   draw each entity's collider at its world pose).

## Consequences

- Cars, props with several shapes and characters with weapons are built
  the Unity way, and `docs/physics.md` drops "one collider per entity"
  (it becomes "one collider per entity, and a body's shape is its own
  plus its children's").
- The sync pass has to rebuild the body when a joined child changes. A
  joined child's world matrix changes every tick while its body moves,
  so its change detection compares its pose relative to the body, not
  its world matrix. Without that, every child of every moving body would
  rebuild every tick.
- A changed record means a new schema fingerprint. Every cache recooks
  once, and a host built against the old SDK reads the extra 16 bytes
  as unknown tail (the event size is in its header).
- Tests: `crates/fynkel_core/tests/compound.rs`. A body whose only
  collider is a child's lands on it. The child keeps its place. The
  contact names the child as the collider and the parent as the body.
  Deactivating the child drops its shape and keeps the body's velocity.
  A Dart test covers the body's object getting `onCollisionEnter`.
