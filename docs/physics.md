# Physics

The physics contract: blueprint §5.1 (the physics slot and transform
ownership), §9 "Physics", §19.2d item 8 (M3-08). `fynkel_physics` is the
`RigidBody`/`Collider` components plus a Rapier adapter the core steps
inside the fixed tick. Rapier never leaks: a body is addressed by its
entity, shapes are metres, masses kilograms, layers are names from the
project file, and everything a game does goes through the same
commands, queries and events every other native system uses.

## Components

Both are ordinary native components: schema-generated, scene-validated,
in the starter's registry, reachable from Dart with `get<T>()`/`set`/
`add`/`remove` (M3-07a).

`fynkel.RigidBody` (id 10):

| Field | Default | Meaning |
| --- | --- | --- |
| `bodyType` | `dynamic` | `static` (never moves on its own), `dynamic` (simulated; physics owns the transform), `kinematic` (moved by gameplay, pushes dynamic bodies, pushed by nothing) |
| `mass` | 1 | kilograms; `> 0` for a dynamic body, inertia follows the collider shape |
| `linearDamping` / `angularDamping` | 0 / 0.05 | per-second velocity decay, `>= 0` |
| `gravityScale` | 1 | multiplies the world's gravity (0 floats) |
| `ccd` | false | continuous collision detection for fast bodies |

`fynkel.Collider` (id 11) — alone it is static geometry, with a
`RigidBody` it is that body's shape (one collider per entity in V1):

| Field | Default | Meaning |
| --- | --- | --- |
| `shape` | `box` | `box` (`size` = half extents), `sphere` (`size.x` = radius), `capsule` (along local Y: `size.x` radius, `size.y` half height of the cylinder), `mesh` (the triangles of `mesh`) |
| `size` | (0.5, 0.5, 0.5) | metres, scaled by the entity's world scale |
| `mesh` | — | the cooked mesh sub-asset (`mesh/<n>`) for `shape: mesh`; static only, at most 32 768 triangles (`MAX_MESH_COLLIDER_TRIANGLES`) |
| `offset` | 0 | local translation of the shape from the entity's origin |
| `friction` | 0.5 | Coulomb coefficient, `>= 0` |
| `restitution` | 0 | bounciness `0..=1` |
| `isSensor` | false | reports trigger events, applies no forces |
| `layer` / `mask` | 1 / all | membership bits and the bits it collides with — both sides must agree; bit `i` is `physics.layers[i]` |

`fynkel validate` checks what the schema cannot: `FYN-H0001` a body
value the runtime would reject (mass, damping), `H0002` a collider value
(size per shape, friction, restitution, a mesh collider without `mesh`),
`H0003` a mesh collider on a dynamic or kinematic body, `H0004` (warning)
a dynamic body without a collider (it falls through everything). The same
rules run at the tick for a component set from Dart (`INVALID_PAYLOAD`);
a mesh the cache does not hold, or one above the limit, is
`MESH_UNAVAILABLE` (code 13) once per entity and the collider is skipped.

## The project file

```json
"physics": { "gravity": [0, -9.81, 0], "layers": ["default", "props"] }
```

Defaults when absent: gravity `[0, -9.81, 0]`, layers `["default"]`. At
most 32 unique, non-empty names (`FYN-P0013`, at `/physics/layers/<i>` or
`/physics/gravity`). The cook copies the block into the cache index; the
world applies it on load (`LoadReport.physics`, `world.physics` in Dart:
`gravity`, `layers`, `meshColliders` registered, `mask(names)`);
`world.layerMask(['props'])` turns names into bits and throws on a name
the project does not declare.

## The step (§5.1)

```text
World::step
  apply C[n]                 body commands are queued, components inserted
  propagate                  world matrices reflect the commands
  Physics::step(ecs, dt)     sync components → bodies (only entries that
                             changed: new, restructured, moved by a
                             command), apply the queued commands, Rapier
                             step, write dynamic and kinematic poses back
                             to the entities' local transforms, collect
                             the tick's events
  propagate                  the written locals become world matrices
  Physics::record(ecs)       remember the written bodies' world matrices,
                             so the next sync sees only external moves
```

The step is skipped entirely while no entity has a `RigidBody` or
`Collider` (`has_work`), so a scene without physics pays nothing.
Ownership: an entity with a dynamic body carries `PhysicsOwned`; a
presentation-lane transform write on it is rejected `PHYSICS_OWNED`
(code 9), a simulation-lane write (`setLocalPosition` in `fixedUpdate`)
is a teleport. A kinematic body follows its entity's transform or a
`setKinematicTarget`; static bodies and plain colliders move only when
their entity does. A parent transform moving a child body is an external
move too (the sync compares world matrices, not locals). Scale changes
rebuild the shape; a `RigidBody` value change is applied in place.

Determinism: bodies are created in entity order and Rapier is
deterministic on one platform, so the same scene and commands give the
same transforms — the headless checksum covers the stress scene's 500
bodies (`fdce91f4c8a52bfc` at 600 ticks). Bodies at rest sleep and cost
the solver nothing. Rendering interpolates every entity between the
previous and the current tick (`World::extract` by the clock's alpha,
ADR-0008), bodies included, so 60 Hz physics draws smoothly at 120 Hz.

## Body commands (Dart, simulation lane)

`fynkel.cmd.Body` (kind 8, 40 B: entity, op, vector, rotation) is applied
by the tick's physics step, after the batch's component writes, so a body
added and pushed in the same `fixedUpdate` moves (its mass properties are
recomputed from the collider at creation). On `GameObject` and, for the
common ones, `Behaviour`:

| Method | Body | Unit |
| --- | --- | --- |
| `applyForce(v)` / `applyTorque(v)` | dynamic | N / N·m, this tick only |
| `applyImpulse(v)` / `applyTorqueImpulse(v)` | dynamic | N·s / N·m·s, once |
| `setLinearVelocity(v)` / `setAngularVelocity(v)` | dynamic | m/s / rad/s |
| `setKinematicTarget(p, [q])` | kinematic | the pose reached at the end of the tick; its velocity follows, so it pushes |
| `teleport(p, [q])` | any | the pose now, velocities kept |

A command on an entity without a body is `MISSING_COMPONENT` (component
10), the wrong body type or a non-finite value `INVALID_PAYLOAD`, a
presentation-lane body command `WRONG_LANE` — all diagnostics carrying the
command's batch index. A command on an object spawned this tick is
addressed by its temp reference and lands in the spawn's tick, after the
batch's component writes (M3-08b) — `spawn`, `add(RigidBody)`,
`add(Collider)`, `applyImpulse` in one `fixedUpdate` is one tick; the
body's mass is computed at creation, so the impulse counts. `bodyState()` is one
`FYNKEL_QUERY_BODY` (7): `linearVelocity`, `angularVelocity`, `sleeping`,
or null without a body.

## Contacts

The step's events go on the physics lane and reach behaviours the same
tick, right after the step and before `afterPhysics`: `onCollisionEnter`,
`onCollisionStay`, `onCollisionExit`, `onTriggerEnter`, `onTriggerExit`
on every behaviour of both objects (side `a` then `b`, contact order).
`Contact` carries `a`, `b`, `point` (world, the deepest point), `normal`
(from `a` towards `b`; `normalFor(entity)` flips it), `impulse` (N·s the
solver applied to the pair this step; 0 for exits and triggers),
`other(entity)`, `involves`, `isTrigger`. Enter/exit are exact (a removed
collider still exits); **stay** is reported only for pairs where at least
one entity is subscribed — every `GameObject`, so every object with a
behaviour — so
the stay stream scales with what gameplay observes, not with the scene
(the stress scene's 500 bodies produce 1 430 enters and 657 exits over
600 ticks and no stays). `fynkel.rec.ContactEvent` (48 B) is the wire
record, `fynkel_world_after_physics_events` the raw view (the same tick's
events; drained with the general events next).

## Queries

Spatial queries read the world as the last step left it (one FFI call
each; `physicsQuery(records)` batches many):

```dart
final hit = world.raycast(origin, dir, maxDistance: 20, mask: world.layerMask(['default']), exclude: gameObject);
final all = world.raycastAll(origin, dir, maxHits: 8);
final sweep = world.shapeCast(ColliderShape.capsule, const Vec3(0.3, 0.6, 0), from, dir);
final inside = world.overlap(ColliderShape.sphere, const Vec3(2, 0, 0), centre, hitSensors: true);
```

`PhysicsHit`: `entity`, `point`, `normal` (zero for an overlap),
`distance` along the ray or cast (0 for an overlap); nearest first
within a query; a shape cast reports the first hit only; sensors are
skipped unless `hitSensors`; `exclude` skips the caster's own collider.
Wire: `fynkel.rec.PhysicsQuery` (88 B) in a `PHYSICS_QUERIES` buffer
(kind 8) to `fynkel_world_physics_query`, answered by a `QUERY` buffer of
`fynkel.rec.PhysicsHit` (48 B) in query order; a framing error is
`FYNKEL_STATUS_PROTOCOL` with an empty view and the world stays usable.

## Hosts

`fynkel test --headless` prints `physics: N bodies, N colliders (N mesh),
collisions N enter / N stay / N exit, triggers N enter / N exit` and the
JSON report gains `physics: { bodies, colliders, mesh_colliders,
collision_enter, collision_stay, collision_exit, trigger_enter,
trigger_exit, diagnostics }` when anything stepped. The fixture project's
Crate B falls onto its Floor and sleeps (a test); the starter's crate
does the same under `flutter test`, pushed by the kinematic cube the
Mover walks. `Physics::debug_lines` outlines every collider for a host's
debug draw (`docs/renderer.md`); no host draws them yet.

## Cost

`benchmarks/m3/capture.py` measures the stress scene's 500 dynamic
bodies (cubes, mass `size³·2`, on the floor and 5 000 static box
colliders): p50 tick 0.64 ms while they fall and 0.64 ms settled, against
0.28 ms for the same scene without physics; the gate wants ≥ 500 bodies
touching, p50 under 4 ms both phases and the checksum repeated. The sync
is the non-Rapier cost: one ECS query per tick over the entities with a
body or collider, work only for the ones whose world matrix or components
changed.

## Not in V1

One collider per entity (compound shapes by child entities); no joints,
no character controller (item 9, on Rapier's kinematic controller); mesh
colliders are static; no Android/web-specific notes — the adapter is
pure Rust and runs wherever the core does.
