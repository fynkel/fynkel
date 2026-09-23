# Physics

The physics contract: blueprint §5.1 (the physics slot and transform
ownership), §9 "Physics", §19.2d item 8 (M3-08) and item 9 (M3-09, the
character controller and the third-person camera rig, below).
`fynkel_physics` is the `RigidBody`/`Collider`/`CharacterController`
components plus a Rapier adapter the core steps inside the fixed tick. Rapier never leaks: a body is addressed by its
entity, shapes are metres, masses kilograms, layers are names from the
project file (an object's layer is its own, ADR-0020 — not the collider's), and everything a game does goes through the same
commands, queries and events every other native system uses.

## Components

All are ordinary native components: schema-generated, scene-validated,
in the starter's registry, reachable from Dart with `get<T>()`/`set`/
`add`/`remove` (M3-07a). `RigidBody` and `Collider` here; the
`CharacterController` and the core's `ThirdPersonCamera` in their own
sections below.

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
| `mask` | all | the layers it collides with; bit `i` is the project's `layers[i]`. Its own layer is its entity's `fynkel.Layer` (absent = 0, `default`; ADR-0020): two colliders touch when each one's mask has the other's layer |

`fynkel validate` checks what the schema cannot: `FYN-H0001` a body
value the runtime would reject (mass, damping), `H0002` a collider value
(size per shape, friction, restitution, a mesh collider without `mesh`),
`H0003` a mesh collider on a dynamic or kinematic body, `H0004` (warning)
a dynamic body without a collider (it falls through everything), `H0005`
a character controller value, `H0006` a body or collider next to a
character controller, `H0007` a camera rig value (the corpus fixture
`fixtures/scene/physics_rules.scene.json` lists them all). The same
rules run at the tick for a component set from Dart (`INVALID_PAYLOAD`);
a mesh the cache does not hold, or one above the limit, is
`MESH_UNAVAILABLE` (code 13) once per entity and the collider is skipped.

## The project file

```json
"layers": ["default", "props"],
"physics": { "gravity": [0, -9.81, 0] }
```

Gravity defaults to `[0, -9.81, 0]` (`FYN-P0013` when not finite, at
`/physics/gravity`). The layer names are the project's, not physics'
(ADR-0020, project `formatVersion` 2 moved `physics.layers` to the top
level): at most 32 unique, non-empty names, `default` first (`FYN-P0014`),
default `["default"]`. An object's layer is its `fynkel.Layer`; its
colliders and its character capsule collide as that layer (Rapier's
membership is `1 << layer`), and changing the component at run time
regroups them in the same tick. The cook copies the blocks into the cache
index; the world applies them on load (`LoadReport.physics` and
`world.physics` in Dart: `gravity`, `meshColliders` registered;
`LoadReport.layersAndTags` and `world.layersAndTags`: the names);
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

The step is skipped entirely while no entity has a `RigidBody`,
`Collider` or `CharacterController` (`has_work`), so a scene without
physics pays nothing.
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

## Character controller (M3-09)

`fynkel.CharacterController` (id 12, in `fynkel_physics`) is a kinematic
capsule walked by gameplay over Rapier's `KinematicCharacterController`:
no `RigidBody`/`Collider` next to it (its own capsule replaces them —
`FYN-H0006` at authoring, `INVALID_PAYLOAD` once at the tick, then the
body is ignored until the controller is removed, when the body takes
over). The entity's origin is the capsule's centre minus `center`, so
with the default `center` the origin is at the feet.

| Field | Default | Meaning |
| --- | --- | --- |
| `radius` / `height` | 0.4 / 1.8 | the capsule, metres (`height >= 2·radius`, caps included), scaled by the entity's scale |
| `center` | (0, 0.9, 0) | the capsule's centre in the entity's frame |
| `skinWidth` | 0.02 | the gap kept from every surface (Rapier's offset) |
| `maxSlope` | π/4 | radians; ground steeper than this is not walkable — the character slides down it (`sliding`) |
| `maxStepHeight` | 0.3 | steps up to this height are climbed without a jump (0 disables) |
| `snapToGround` | 0.2 | walking off a bump within this distance keeps the feet down (0 disables) |
| `gravityScale` | 1 | multiplies the world's gravity while airborne; 0 flies (no gravity, `move.y` honoured) |
| `mass` | 70 | kilograms, what it pushes dynamic bodies with (0 pushes nothing) |
| `speed` / `jumpSpeed` | 4 / 5 | m/s, what the action driver walks and jumps at |
| `turnSpeed` | 10 | rad/s, how fast the entity turns to face its motion (0 never turns) |
| `moveAction` / `jumpAction` | "" | named: the runtime drives the character from these actions (below) |
| `mask` | all | as a collider's (the capsule's layer is its entity's) |

Commands (`fynkel.cmd.Body` ops on the simulation lane, `MISSING_COMPONENT`
12 without a controller, `INVALID_PAYLOAD` 12 when not finite; a body
command on a character is `MISSING_COMPONENT` 10): `move(v)` — the
velocity wanted this tick, m/s in world space, planar (`y` ignored unless
flying); a tick without a `move` stands still — and `jump([speed])` —
sets the vertical speed (the controller's `jumpSpeed` when 0) whether or
not grounded: the caller decides when, normally on the action's press
edge while `characterState().grounded`. Gravity accumulates in the air
and is zero on the ground. `characterState()` (`FYNKEL_QUERY_CHARACTER`,
9, `fynkel.rec.CharacterState` 24 B) is held state as the last tick left
it: `grounded` (walkable ground within `skinWidth` + 5 cm under the feet,
never while rising — our own test, Rapier's counts any upward contact,
a ledge brushed mid-jump included), `sliding` (pressed against a contact
steeper than `maxSlope`, slope or overhang) and `velocity` (the movement
actually applied, m/s, gravity included, zero at rest). The character is
not `PhysicsOwned`: like a kinematic body it follows an external
transform write (a teleport; the vertical speed resets), and physics
writes its pose back every tick. Contact events are reported like a
kinematic body's.

Inside `Physics::step` the characters move after the queued commands and
before Rapier's step, against the previous step's broad phase (a
character created this tick stands still one tick). Each `move_shape`
slides along walls, climbs `maxStepHeight`, snaps to ground, and pushes
dynamic bodies with `mass`; a resting character still asks for a hair of
downward motion, because Rapier's zero-translation path depenetrates
through parry's contact query, which is unreliable for a capsule resting
exactly on a box (it can shove the character into the floor by a quarter
of its height) — the cast path is robust there and the residual motion is
reported as standing still. A character placed exactly on the ground
stays there; one that lands rests `skinWidth` above the surface.

**The action driver.** With `moveAction`/`jumpAction` named, `World::step`
walks the character before the batch's commands from the tick's input:
the 2D action (clamped to the unit disc) × `speed` in the frame of the
`fynkel.ThirdPersonCamera` whose `target` is the entity's `Name` (forward
= `(−sin yaw, 0, −cos yaw)`, right = `(cos yaw, 0, −sin yaw)`; world axes
without a rig), and a `jump()` on the action's press while grounded — the
press edge is consumed by exactly one tick (§5.1), so a 50 ms frame's
three ticks jump once. The driver's commands are queued ahead of the
batch's, so a `move`/`jump` from a behaviour in the same tick wins; a
controller with empty actions is driven by behaviours only
(`packages/fynkel/example/player_controller.dart` is the pattern, with a
sprint and a double jump). The fixture project's Playground scene (`fynkel
test --headless --scene c0000000-0000-4000-8000-000000000031 --input
test/platform.input.json`) walks its Player onto a 0.8 m platform from a
recorded stream — the item 9 gate, a headless test with a stable
checksum.

**Root motion (M3-19c).** An `Animator` with `root_motion` on the same
entity hands the tick's extracted travel to the character as a velocity
(`Physics::add_root_motion(entity, v)`, summed into the tick's pending
`move` after the batch's commands are applied — so a clip that walks
adds to, or by itself is, the movement): the capsule then collides,
climbs and snaps like any `move`, `characterState().velocity` reports
what was applied, and the entity turns toward its motion at `turnSpeed`
(the clip's yaw is reported in `AnimatorState.root_yaw`, not applied —
the controller owns the facing; `docs/animation.md` § Root motion).
`Physics::has_character(entity)` is what the animator asks.

**The character's animation (M3-19, [animation.md](animation.md)).** A
rigged character on a controller is the skinned mesh and the joint
entities under the controller's entity, and the controller stays the one
thing that moves the entity: clips carry no root motion (the V1 policy),
so a walk cycles the legs in place while `move` carries the body, and a
jump clip's hips dip while `jump()` lifts the entity. The behaviour reads
`characterState()` and picks the clip (the reference game's
`game.Player.pose`); nothing in the physics step reads the animator, and
a collider on a joint entity follows the pose because animators run
before the physics slot.

## Third-person camera (M3-09)

`fynkel.ThirdPersonCamera` (id 13, in `fynkel_core`) turns the entity it
sits on — normally the one with `fynkel.Camera` — into a rig that orbits a
target and looks at it; `Follow` from the blueprint §3.5 catalogue,
specialised:

| Field | Default | Meaning |
| --- | --- | --- |
| `target` | "" | the followed entity's `fynkel.Name` (an authored component cannot hold an entity reference; the lowest entity of that name wins and is cached while it keeps the name); empty or unknown: the rig does nothing |
| `pivot` | (0, 1.5, 0) | the orbit centre in the target's frame |
| `distance` / `minDistance` | 5 / 0.5 | metres from the pivot, and the closest the pull-in brings it |
| `yaw` / `pitch` | 0 / 0.35 | radians, the current orbit angles — written back every tick, so a scene value is the start and a `set` from Dart snaps the camera |
| `minPitch` / `maxPitch` | −0.5 / 1.3 | the pitch clamp |
| `lookAction` | "" | an axis2d action: `yaw −= x·sensitivity·dt`, `pitch −= y·sensitivity·dt` per tick. A relative control (`mouse:delta`) is a rate consumed by one tick ([input.md](input.md) rule 5, decided by item 14): the rig turns by the frame's motion once, whatever the tick count; bind it with a scale (`mouse:delta*0.0025`) so pixels become stick units and `sensitivity` is one knob for every device |
| `sensitivity` | 2.5 | rad/s per unit of the action |
| `collisionRadius` | 0.2 | a sphere of this radius cast from the pivot along the rig's direction (the target excluded, `mask` applied) pulls the camera in to the first collider, never under `minDistance`; 0 disables |
| `mask` | all | which colliders the cast sees |

The rig is placed by `World::step` after the tick's transform propagate
(the target's world pose is this tick's, the character's included) and
before the physics record: position = pivot + `(cos p·sin y, sin p,
cos p·cos y)`·distance, rotation = `Ry(yaw)·Rx(−pitch)`, written to the
rig's local transform through its parent (`fynkel_scene::set_world_pose`)
— a rig with a parent still orbits in world space. No smoothing or lag,
decided by item 14: the rig snaps to its pose every tick, so what the
player sees is the tick's state (the presenter interpolates between
ticks); a game that wants a lazy camera writes its own rig behaviour.
`fynkel validate` checks the values (`FYN-H0007`), the tick rejects a bad
`set` with `INVALID_PAYLOAD` 13.

## Contacts

The step's events go on the physics lane and reach behaviours the same
tick, right after the step and before `afterPhysics`: `onCollisionEnter`,
`onCollisionStay`, `onCollisionExit`, `onTriggerEnter`, `onTriggerExit`
on every behaviour of both objects (side `a` then `b`, contact order).
`Contact` carries `a`, `b`, `point` (world, the deepest point), `normal`
(from `a` towards `b`; `normalFor(entity)` flips it), `impulse` (N·s the
solver applied to the pair this step; 0 for exits and triggers),
`other(entity)`, `involves`, `isTrigger`. A sensor overlaps dynamic
*and* kinematic bodies — a character's capsule walking into a static
pickup fires its trigger (Rapier's default pairs a fixed collider with
dynamic bodies only; sensors are widened) — while two non-sensor
kinematic/static colliders never report a contact. A `fynkel.TriggerZone`
next to a sensor turns its enter/exit into a named general-lane event
(`ZONE_ENTERED`/`ZONE_EXITED`, [scenes.md](scenes.md)). Enter/exit are
exact (a removed collider still exits); **stay** is reported only for pairs where at least
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
N characters, collisions N enter / N stay / N exit, triggers N enter / N
exit` and the JSON report gains `physics: { bodies, colliders,
mesh_colliders, characters, collision_enter, collision_stay,
collision_exit, trigger_enter, trigger_exit, diagnostics }` when anything
stepped (a character's kinematic capsule counts among the bodies and
colliders too). The fixture project's
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
changed. Since M3-10 the stress scene's 5 000 statics move
(`docs/catalogue.md`) and keep their colliders, so every tick moves 5 000
fixed bodies through the broad phase: the settled step is ≈ 2.0 ms of
physics for 540 bodies and 5 541 colliders — the price of moving geometry
that can be walked on; a visual-only mover should carry no collider.

## Not in V1

One collider per entity (compound shapes by child entities); no joints;
the character controller has no crouch, no moving-platform carry and no
depenetration at rest (a platform pushed into a standing character
tunnels — walk, and the casts handle it); the camera rig has no smoothing;
mesh colliders are static; no Android/web-specific notes — the adapter is
pure Rust and runs wherever the core does.
