# Stage 5 plan: physics

Status: accepted 2026-10-10, with the recommended answer to each question
(§5). Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
Stage 4 is done ([report](stage-4-report.md)).

Stage 4 made Fynkel work the way Unity works, but nothing in it can
touch anything else: an object can't fall, land, bump, or know that a
player walked into it. Collisions and queries are only a part of this;
without a simulation behind them they don't hold together. Stage 5 adds
**physics, as Unity has it**: colliders, rigidbodies, triggers, the six
callbacks, a layer matrix, constraints, queries, a character controller
and joints, on [Rapier](https://rapier.rs), a physics engine in Rust.

Physics is **a package a game adds**, `package:fynkel_physics`, not a part
of the core: a game without it ships no physics and runs exactly as now.
What the core gains is general: a fixed time step (`onFixedUpdate`), which
games use without physics too, and a way for a package to take part in
the world's frame. Once a game adds the package, it should feel built in.

Stage 5 ends when a fourth game, a small physics game, is written with it,
coins' pickups are triggers, and every game runs as well as before on all
three devices.

What a game looks like at the end:

```dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

const ground = 0, balls = 8, pins = 9;

class Ball extends GameObject with Collisions {
  Ball(Vec3 at) : super(
    layer: balls,
    transform: Transform(position: at, scale: Vec3.all(0.4)),
    components: [
      MeshRenderer(Mesh.sphere(), Material(color: Color(0xFFE04040))),
      SphereCollider(material: PhysicMaterial(bounciness: 0.3)),
      Rigidbody(mass: 6, collisionDetection: CollisionDetection.continuous),
    ],
  );

  @override
  void onCollisionEnter(Collision hit) {
    if (hit.gameObject is Pin) hit.rigidbody!.addForce(hit.relativeVelocity, mode: ForceMode.impulse);
  }
}

class Gutter extends GameObject with Triggers {
  Gutter() : super(components: [BoxCollider(size: Vec3(20, 1, 1), isTrigger: true)]);

  @override
  void onTriggerEnter(Collider other) => other.gameObject.destroy();
}

class Lane extends GameObject {
  @override
  void onStart() {
    physics.ignoreLayerCollision(balls, balls);
    on(input.onTap, (finger) {
      final ray = world.camera!.screenToRay(finger.position);
      final hit = physics.raycast(ray, layers: LayerMask.of([ground]));
      if (hit != null) add(Ball(hit.point + Vec3(0, 3, 0)));
    });
  }
}
```

Not in stage 5: 2D physics (it comes with 2D, on Rapier's 2D build, in
the same shape), cloth, soft bodies, vehicles (Unity's WheelCollider),
terrain colliders, articulations, ragdoll tools, physics across a network.

## 1. Unity's way, and Fynkel's

| In Unity | In Fynkel after stage 5 |
|---|---|
| Physics is built in. | A package, `fynkel_physics`: add it to `pubspec.yaml` and it's there; leave it out and nothing of it is built. |
| `FixedUpdate`, `Time.fixedDeltaTime` (0.02 s); `Time.deltaTime` reads the fixed step inside it. | `onFixedUpdate()`, `time.fixedDelta` (1/50 s); `time.delta` reads it inside. In the core, with or without physics. |
| `BoxCollider`, `SphereCollider`, `CapsuleCollider`, `MeshCollider` (convex or not), `isTrigger`, `center`, `enabled`. | The same, as behaviours. |
| `Rigidbody`: mass, drag, `useGravity`, `isKinematic`, `velocity`, `AddForce` and its `ForceMode`s, `MovePosition`, `constraints`, `interpolation`, `collisionDetectionMode`, sleeping. | The same, with Unity 6's names (`linearDamping`, `linearVelocity`). |
| `PhysicMaterial`: friction and bounciness, with combine modes. | The same. |
| `OnTriggerEnter/Stay/Exit(Collider)`, `OnCollisionEnter/Stay/Exit(Collision)` as magic methods. | The same methods, on an object or behaviour that mixes in `Triggers` or `Collisions` (§3.4). |
| Exit isn't called when an object is destroyed or turned off. | It is (§5, 3). |
| The layer collision matrix; `Physics.IgnoreCollision`. | `physics.ignoreLayerCollision(a, b)`, `physics.ignoreCollision(c1, c2)`, on the 32 layers stage 4 made. |
| `Physics.Raycast`, `RaycastAll`, `SphereCast`, `BoxCast`, `CapsuleCast`, `OverlapSphere/Box/Capsule`, `CheckSphere`, `ClosestPoint`, with a layer mask and `QueryTriggerInteraction`. | The same on `physics`, which every object and behaviour has, as `time`. `world.raycast` still hits what's drawn (for taps). |
| `CharacterController`. | The same, on Rapier's character controller. |
| `FixedJoint`, `HingeJoint`, `SpringJoint`, break force. | The same. |
| Physics debug view in the editor. | `physics.drawColliders = true`, drawn with stage 4's debug lines. |

## 2. What's new, by package

```
crates/
  fynkel_physics/       NEW: the world of bodies and colliders on rapier3d; safe Rust, tested with cargo
  fynkel_physics_ffi/   NEW: its C boundary, built as its own library
packages/
  fynkel/               the fixed step; package:fynkel/extend.dart, for packages that take part in the frame
  fynkel_physics/       NEW: colliders, rigidbodies, materials, callbacks, layers, queries, character
                        controller, joints; hook/build.dart builds fynkel_physics_ffi
examples/
  lab/                  a scene for each feature, each with "the basics"
  bowling/              NEW: the fourth game
  coins/                its pickups become triggers
```

The engine (`fynkel_ffi`) doesn't link Rapier, and Rapier doesn't know the
engine: they are two native libraries, and the Dart side of the physics
package moves poses between them, in one call each way per step (§3.1).
As before, the loop lives in Dart and nothing native calls into Dart, so
physics is tested in `dart test` step by step, without a screen.

## 3. Design decisions

### 3.1 A package, and what the core gives it

- **Two native libraries.** `fynkel_physics_ffi` is built by the physics
  package's own build hook and bundled only into games that depend on it.
  Rapier keeps its own copy of where each body is; the engine keeps the
  transforms it draws.
- **One call each way per step.** Before a step, the package sends the
  colliders and bodies the game moved itself (a teleport, a moved static
  collider, a kinematic body's target) in one buffer; after it, it reads
  back every awake body's pose in one buffer and writes them to the
  objects' transforms in one call. A world of resting bodies costs almost
  nothing.
- **What the core gives a package**, in `package:fynkel/extend.dart` (not
  in `fynkel.dart`, so games don't see it):
  - a `WorldSystem` the world runs at fixed points in the frame (before and
    after each fixed step, after the late updates), found per world with
    `world.system<T>()` and made the first time it's asked for, so a game
    never sets physics up: the first collider makes it;
  - the objects whose transforms were set since it last asked (stage 4
    already keeps this list, to send them in one call);
  - world transforms read, and transforms written, for many objects in one
    call.
  Nothing in it knows about physics; audio and animation can use the same.
- **Colliders, rigidbodies and joints are behaviours.** They get
  `onEnable`, `onDisable` and `onDestroy` from the lifecycle stage 4 made,
  so hiding, showing, removing and destroying need nothing new.

### 3.2 The fixed step

- **`onFixedUpdate()`** on objects and behaviours, and `time.fixedDelta`
  (1/50 s by default, settable). Each frame runs as many fixed steps as the
  frame's time holds, 0 or more, after the starts and before the updates,
  as Unity does: for each step, every `onFixedUpdate`, then the systems'
  step (physics simulates and its callbacks come), then the routines that
  wait for a fixed step.
- **`time.delta` reads `fixedDelta` inside `onFixedUpdate`**, as Unity's;
  `time.inFixedStep` says which it is. `time.scale` scales the fixed steps'
  time (0 runs none); `time.maxDelta` limits how many run after a hitch.
- **`time.fixedAlpha`**: how far the frame is between the last fixed step
  and the next, for interpolation.
- **`await fixedStep`** in a routine, as Unity's `WaitForFixedUpdate`.

### 3.3 Colliders and rigidbodies

- **Colliders**: `BoxCollider(size:, center:)`, `SphereCollider(radius:,
  center:)`, `CapsuleCollider(radius:, height:, direction:, center:)`,
  `MeshCollider(mesh:, convex:)`, each with `isTrigger`, `material` and
  `enabled`. They follow the object's world scale as Unity's do (a sphere
  takes the largest axis).
- **A collider without a rigidbody is static.** It can be moved (it's
  sent before the next step), as in Unity.
- **`Rigidbody`**: `mass`, `useGravity`, `isKinematic`, `linearDamping`,
  `angularDamping`, `linearVelocity`, `angularVelocity`, `centerOfMass`,
  `maxAngularVelocity`; `addForce(force, mode:)`, `addTorque`,
  `addForceAtPosition`, `addExplosionForce`; `movePosition`, `moveRotation`
  (for kinematic bodies, moved through the step); `constraints`
  (`RigidbodyConstraints.freezePositionY | freezeRotation`, …),
  `interpolation`, `collisionDetection` (discrete or continuous),
  `sleep()`, `wakeUp()`, `isSleeping`.
- **Compound bodies**: colliders on the rigidbody's object and on its
  children with no rigidbody of their own move with it, as one body.
- **A body's object can have a parent.** Its pose is written back relative
  to the parent, so it stays where physics put it in the world.
- **Setting a body's transform teleports it**, as in Unity; for moving a
  body through the world, `movePosition` or forces.
- **Interpolation**: with `interpolation: Interpolation.interpolate`, the
  transform the view draws is between the last two steps, so a body
  moves smoothly at 120 Hz on a 50 Hz step. `rigidbody.position` reads the
  physics pose; `transform` reads what's drawn.
- **`PhysicMaterial(dynamicFriction:, staticFriction:, bounciness:,
  frictionCombine:, bounceCombine:)`**, with Unity's defaults (0.6, 0.6, 0,
  average, average).
- **World settings** on `physics`: `gravity` (0, -9.81, 0), the solver's
  iterations, `sleepThreshold`, `bounceThreshold`, `defaultContactOffset`.

### 3.4 Callbacks

- **The physics package can't add methods to `GameObject`**, so an object
  or behaviour that wants them says so with a mixin: `with Triggers` gives
  `onTriggerEnter/Stay/Exit(Collider other)`, `with Collisions` gives
  `onCollisionEnter/Stay/Exit(Collision hit)`. Only those are called, so
  a world of objects that don't listen pays nothing.
- **Who gets them, as Unity's table**: the object of each collider and its
  rigidbody's object (compound bodies), with their behaviours. Triggers
  need a rigidbody (kinematic is fine) on one side; collisions need a
  non-kinematic rigidbody on one side. Two triggers don't meet.
- **When**: after each physics step, in the fixed step, enters before
  stays before exits, each in a stable order. Stay comes every step while
  they touch, as Unity's (not while both bodies sleep).
- **Exit always comes**: when a collider is disabled, removed or destroyed,
  its object hidden, its layer changed so the pair no longer meets, or
  `isTrigger` changed. Unity skips these, and games work around it.
- **`Collision`**: `gameObject`, `collider`, `rigidbody`, `contacts`
  (point, normal, separation), `impulse`, `relativeVelocity`.
- **Errors** in a callback go to `world.errors`, and the step goes on.

### 3.5 Layers

- **The layer matrix**: `physics.ignoreLayerCollision(a, b, [ignore = true])`
  and `physics.getIgnoreLayerCollision(a, b)`, on stage 4's 32 layers. It
  maps onto Rapier's collision groups, so the broad phase skips the pairs.
- **`physics.ignoreCollision(c1, c2)`** for one pair of colliders.
- **Changing an object's layer** takes effect at the next step.

### 3.6 Queries

- On `physics`: `raycast`, `raycastAll`, `sphereCast`, `sphereCastAll`,
  `boxCast`, `capsuleCast`, `overlapSphere`, `overlapBox`, `overlapCapsule`,
  `checkSphere`, `checkBox`, `checkCapsule`, `closestPoint`, and
  `collider.raycast` and `collider.closestPoint` for one collider.
- Each takes `maxDistance`, `layers` (a `LayerMask`) and `triggers`
  (`QueryTriggers.useGlobal`, `ignore`, `collide`, with
  `physics.queriesHitTriggers` as Unity's global).
- A hit is a `PhysicsHit`: `point`, `normal`, `distance`, `collider`,
  `rigidbody`, `gameObject`. (Stage 4's `RayHit`, from `world.raycast`,
  stays as it is.)
- They see the world as of the last step, plus what the game moved since
  (sent first), as Unity's do with auto sync.

### 3.7 The character controller and joints

- **`CharacterController(height:, radius:, slopeLimit:, stepOffset:,
  skinWidth:)`** with `move(motion)` returning `CollisionFlags`,
  `isGrounded`, `velocity`, and `onControllerColliderHit` (with a mixin, as
  §3.4), on Rapier's kinematic character controller.
- **Joints**: `FixedJoint`, `HingeJoint` (limits, motor, spring),
  `SpringJoint`, each with `connectedBody`, `anchor`, `breakForce`,
  `breakTorque`, and `onJointBreak`.

### 3.8 Reliable

- **Each rule is a test first**, in `dart test` through the real Rapier, and
  the numbers are Unity's where Unity's are known: a body dropped from 10 m
  lands within a step of √(2h/g); a stack of ten boxes stands for 10 s; a
  0.1 m ball at 300 m/s with continuous detection doesn't pass a 0.1 m
  wall; enter, stay and exit come exactly as §3.4 says, for every row of
  Unity's table; frozen axes don't move; the same input gives the same
  output, step for step.
- **The Rust side** has its own tests (`cargo test`), and every step's new
  code is checked with mutants, as in stage 4.

## 4. Steps

Each step is built, tested and merged on its own, in this order. Each
step's tests fail first where that makes sense. Every existing test, the
guide's check and the five examples pass at every step, unchanged unless a
step says so. Each step adds its lab scenes ("the basics" first) and is
tried on the iOS simulator; steps 4, 5 and 9 on all three devices.

**Step 1: Rapier in a package.** `crates/fynkel_physics` (a world of
bodies on rapier3d, stepping, poses in and out in buffers),
`crates/fynkel_physics_ffi`, and `packages/fynkel_physics` with its build
hook and bindings. Nothing for games yet. *Done when* a `dart test` drops a
body through the package and it lands when the formula says; the package
builds into an app on the Mac, the iPhone and the Note10+; and we know what
it adds to each app's size and what a step of 1,000 bodies (falling, then
resting) costs on the Note10+, poses in and out included. If two native
libraries turn out costly, this step says so and we decide again before
step 2.
✅ on all three devices.
`crates/fynkel_physics` (9 tests), `crates/fynkel_physics_ffi` (8, its
header pinned as the engine's is), `packages/fynkel_physics` (6 Dart tests,
through the build hook) and its example, `tumble`: 1,000 cubes dropped
again every 8 s. Both hooks now build through `package:fynkel/hooks.dart`.
Numbers in §6. **Two libraries cost little**: writing 1,000 bodies' poses
back adds about 0.6 ms a step on the Note10+. **Rapier itself is the cost**
with 1,000 cubes all touching, and on the Note10+ it doubles inside the live
app (10 ms a step, against 4.5 ms in the bench with no view drawing): the
scene holds 120 fps on the Mac and 117 on the iPhone (a step there is
0.86 ms), but 30 fps on the Note10+ with 1,000 cubes and 44.9 with 500
(57.9 with physics off). Step 4's bar for the Note10+ is
at risk; before it, we look at running the step on a thread of its own,
alongside the frame. LTO, Rapier's `simd8` and its `parallel` build gained
nothing or 20 % at most (`cargo run --release -p fynkel_physics --example
tumble`), so they stay off.

**Step 2: the fixed step, and the core's part.** §3.2 and §3.1's
`extend.dart`. *Done when* tests fix the fixed step's order in the frame,
the number of steps for any frame time, scale and maxDelta, `time.delta`
inside it, and `fixedStep`; a world without systems allocates nothing in
a frame, and the stage 4 bench stays within its noise; a test system sees
each point of the frame. Lab: "Fixed step: the basics".
✅ on all three devices.
`onFixedUpdate`, `time.fixedDelta`, `fixedAlpha` and `inFixedStep`, `await
fixedStep`, and `WorldSystem` with `world.system(Make.new)` in
`package:fynkel/extend.dart` (16 tests in `fixed_step_test.dart`, 10
mutants all killed; the allocation test now runs fixed steps and a system,
and still allocates nothing). Inside a step `time.sinceStart` is the
steps' own time, so it moves by `fixedDelta` too. **Not in this step:**
the moved-objects list and the batched transform calls of §3.1; nothing
uses them yet, so they come with the first step that does (3 or 4). A
fixed step costs every object a call, as `onLateUpdate` does: 0.03–0.11 ms
per 10,000 objects per step (§6). `tumble` now steps in `onFixedUpdate`.
Its `fixedStep` constant is now `stepSeconds`, because `fixedStep` is now
a wait, next to `nextFrame`.

**Step 3: colliders and queries.** §3.3's colliders (box, sphere,
capsule) on static objects, §3.5's layers in queries, §3.6, and
`physics.drawColliders`. *Done when* every query is tested against each
shape, with masks, triggers and scale, a query that misses allocates
nothing, and moved colliders are found where they were moved. Lab:
"Physics queries: the basics".
✅ on the iOS simulator; queries cost 0.2–0.9 µs on the Mac (§6).
`BoxCollider`, `SphereCollider` and `CapsuleCollider` (behaviours with
`center` and `isTrigger`, scaled as Unity's), `physics` on every object,
behaviour and world, the queries of §3.6 with `layers` and `triggers`
(`physics.queriesHitTriggers`), `PhysicsHit`, `collider.raycast` and
`collider.closestPoint`, and `physics.drawColliders`. Tests: 12 in
`queries.rs` and 6 new unit tests (`cargo test -p fynkel_physics`), 12
through the C interface, 13 in `queries_test.dart` (every query against
each shape, masks, triggers, scale, centres, moves, parents, turns,
enabling and destroying, removal from the middle, shape changes), and
`alloc_test.dart`: a query that misses allocates nothing. 14 mutants of
the new code; 13 killed, and the one left only skips drawing work when
debug lines are off. The engine's ABI is 14
(`fy_world_transform_get_many`: many objects' world transforms in one
call), the physics' 2. The core's part is `world.transformVersion` and
`world.readWorldTransforms` in `extend.dart`.

How colliders follow objects: while no transform, parent or layer was set
since the last query, a query does nothing first. Otherwise it reads every
collider's object's world transform in one call and gives them to Rapier
in another, which moves those that changed. Queries put what was added or
moved into Rapier's tree at once, without a step, so a collider moved this
frame is found where it went (a test shows this doesn't change what the
next step does). **The cost** is about 42 ns per collider on the Mac, on
any frame where anything moved: 0.04 ms with 1,000 colliders, 0.21 ms with
5,000. Step 4 makes it the moved objects' only, with the list of moved
objects §3.1 planned, since bodies move every step.

Unity's rules, kept: a ray doesn't hit a collider it starts inside, a
single cast doesn't hit one it starts in, and an `All` cast gives those at
distance 0, with a zero point and the normal against the cast. Unity's
`Physics.ClosestPoint(point, collider, position, rotation)`, which places
a collider somewhere else first, is left out: `collider.closestPoint` does
what games use it for. `MeshCollider` waits for a game that needs one.

**Allocation in the JIT**: a struct's `.ref`, and a double passed to a
function the JIT doesn't inline, each allocate. So the queries write their
inputs into one float buffer and read hits from another, by index, and the
C calls take pointers (`FpRay`, `FpSweep`, `FpQueryShape`) and no floats.
AOT, on the devices, needs none of this, but the test runs in the JIT.

**Step 4: rigidbodies.** The rest of §3.3: dynamic and kinematic bodies,
forces, `movePosition`, constraints, interpolation, continuous detection,
sleeping, compound bodies, parents, teleports, materials, gravity. *Done
when* §3.8's falling, stacking, tunnelling and frozen-axis tests pass,
interpolated bodies move smoothly on the three devices, and 500 moving
bodies hold the full frame rate on all three. Labs: "Rigidbodies: the
basics", a stack, a ball through walls.

**Step 5: callbacks and the layer matrix.** §3.4 and §3.5. *Done when*
every row of Unity's trigger and collision table is a test, exits come in
every case §3.4 lists, errors in callbacks reach `world.errors`, and a
step where no contact starts or ends allocates nothing. Labs: "Triggers
and collisions: the basics", "The layer matrix".

**Step 6: mesh colliders.** Convex hulls from any mesh, and triangle
meshes for static colliders (as Unity's non-convex ones), from a
`Mesh` or a model's parts. *Done when* rays and bodies hit a model's
shape. Lab: a ball run over a model.

**Step 7: the character controller.** §3.7. *Done when* tests show it
walks up steps below `stepOffset` and not above, stops at `slopeLimit`,
slides along walls, knows `isGrounded`, and pushes bodies. Lab: "Character
controller: the basics", a small course.

**Step 8: joints.** §3.7. *Done when* each joint holds as Unity's does
(a hinge's limits and motor, a spring's rest length), and breaks at its
force. Lab: a door, a chain, a bridge.

**Step 9: the bowling game, and coins.** `examples/bowling`: roll a ball
by swiping, pins to knock down, a gutter (a trigger), the score, a camera
that follows the ball. coins' pickups become triggers. *Done when* both
are played on the three devices, and every game runs as well as before.

**Step 10: the stage 5 check.** A report, as before, and a physics part in
[the guide](writing-a-game.md), checked by `tools/check_guide.py`. *Done
when* both are committed and you have read them.

## 5. Decisions (2026-10-10)

1. **Rapier**, not our own physics.
2. **A package**, `fynkel_physics`, with its own native library; the core
   gains only the fixed step and `extend.dart`, which aren't physics.
3. **Exits always come**, also on destroy, disable and hide (Unity's
   don't).
4. **The character controller** is in this stage.
5. **Joints** too, as a late step, after the rest is solid.
6. **The fourth game** is a new small physics game (bowling), and coins'
   pickups move to triggers.
7. **2D physics** waits for 2D.

## 6. Measurements

Bars, each on the Mac, the iPhone 16 Pro Max and the Note10+ (profile
builds):

- A game without physics: as fast as at the end of stage 4, and a frame
  allocates nothing (as stage 4's test).
- 500 moving bodies: the full frame rate (120, 120, 60).
- 1,000 bodies: the step's cost, with poses in and out, written down (step
  1 measures it first).
- What the package adds to an app's size, written down.

### Step 1: Rapier in a package (2026-10-11)

`tumble`'s timings (`measure bench`, profile builds): a fixed step of 1,000
cubes 0.5 m across, dropped in columns of 10 and left for 8 s (400 steps);
"with poses" writes each moved cube's pose to its object and sends them to
the engine, as a frame does.

| | Mac (M3 Max) | iPhone 16 Pro Max | Note10+ |
|---|---:|---:|---:|
| step, falling (first 3 s) | 1.32 ms | | 4.44 ms |
| step, a whole drop | | 0.86 ms (slowest 2.26) | 3.94 ms (slowest 7.82) |
| step with poses, a whole drop | 1.85 ms (first 3 s) | 1.02 ms (90th 1.29, slowest 3.73) | 4.54 ms (90th 5.33, slowest 14.57) |
| step with poses, resting (asleep) | 0.00 ms | 0.00 ms | 0.00 ms |

Live (`measure rate`, 30 s), the cubes on screen with a sun's shadows:

| | Mac | iPhone | Note10+ |
|---|---:|---:|---:|
| 1,000 cubes | 120 fps | 117 fps (min 106) | 30.0 fps (physics 10 ms a step, measured in the app) |
| 500 cubes | | | 44.9 fps |
| 1,000 cubes, physics off | | | 57.9 fps |

How many cubes the Note10+ holds (2026-10-11, live, 30 s each, every cube
awake): up to 300 it stays near 60, and its few slow frames are the GPU's
(shadowed cubes), not physics'; from 400 the physics' share shows.

| Cubes | 100 | 200 | 300 | 400 | 500 | 1,000 |
|---|---:|---:|---:|---:|---:|---:|
| fps (median) | 59.9 | 58.9 | 57.9 | 50.9 | 44.9 | 30.0 |
| slowest second | 54.9 | 44.9 | 47.9 | 43.9 | | |
| slow frames: Dart / GPU | 0 / 48 | 6 / 87 | 4 / 77 | 67 / 50 | | |

Before step 4 we tried, on the Note10+, Rapier's `parallel` build and the
step on a thread of its own, and built the same scene in Unity: "Before
step 4" below.

What the package adds to an app: its library is 2.3 MB on an iPhone and
2.3 MB on 64-bit Android (arm64, stripped), about 1.2 MB of that to
download (gzip -9); the engine's is 6.0 and 7.2 MB.

### Step 2: the fixed step (2026-10-11)

Stage 4's bench (`measure bench`, profile builds), 10,000 objects in 100
trees of 100, median of 300 steady frames. Main and step 2 were measured
the same day, the same way. The new row runs frames of 1/50 s, so every
frame has exactly one fixed step. The other rows run at 1/120 s, so about
two frames in five have one.

| 10,000 objects, main / step 2 | Mac (M3 Max) | iPhone 16 Pro Max | Note10+ |
|---|---:|---:|---:|
| moving themselves | 0.95 / 0.96 ms | 0.87 / 0.88 ms | 3.05 / 3.12 ms |
| each moved by a behaviour | 1.05 / 1.09 ms | 0.94 / 0.96 ms | 3.58 / 3.70 ms |
| reading their transforms | 0.13 / 0.14 ms | 0.11 / 0.12 ms | 0.30 / 0.34 ms |
| with nothing to do | 0.06 / 0.07 ms | 0.06 / 0.06 ms | 0.20 / 0.22 ms |
| **with nothing to do, a step every frame** | 0.06 / **0.10** ms | 0.05 / **0.08** ms | 0.19 / **0.30** ms |

A fixed step's walk, where every object and behaviour gets
`onFixedUpdate`, costs 0.04 ms per 10,000 objects on the Mac, 0.03 on the
iPhone and 0.11 on the Note10+. That's the same as stage 4's walk for
`onLateUpdate`. At 50 steps a second, a game with 1,000 objects on the
Note10+ spends about 0.5 ms a second on it. The other rows are within
the bench's noise.

### Step 3: colliders and queries (2026-10-11)

The Mac (M3 Max, JIT, warmed): a ray across a grid of static colliders,
half boxes and half spheres, 20,000 times; then the same with one object
moved before each ray, so every query follows every collider first.

| Colliders | 100 | 1,000 | 5,000 |
|---|---:|---:|---:|
| `physics.raycast` | 0.20 µs | 0.52 µs | 0.89 µs |
| after a move (following them all, then the ray) | 4.8 µs | 43 µs | 210 µs |

### Before step 4: threads, and Unity (2026-10-11)

`tumble` live (`measure rate`, 30 s, profile builds), 1,000 cubes unless
said. "Own thread" is `--dart-define=TUMBLE_THREAD=1`: each fixed step
waits for the step before (`fp_step_end`), writes its poses and starts the
next on the physics' own thread (`fp_step_begin`), so the step runs while
the frame goes on and the cubes show a step (20 ms) late. Inline and own
thread were measured one after the other, twice on the Note10+; the second
pair is in brackets.

| fps (median, slowest second) | Mac | iPhone 16 Pro Max | Note10+ |
|---|---:|---:|---:|
| inline | 120 (6.6) | 119 (109), 600 slow frames | 35.9 (30.0) [36.0 (29.5)] |
| **own thread** | 120 (119) | **120 (117), 2 slow frames** | **54.9 (42.9)** [55.9 (41.3)] |
| own thread, waited for in `onLateUpdate` | | | 24.6 (18.7) |
| Rapier `parallel`, inline | | | 28.5 (21.6) |
| Rapier `parallel`, own thread | | | 51.9 (38.0) |
| 500 cubes, inline / own thread | | | 53.9 / **59.9 (53.9)** |

- **The step on its own thread, a step late, is the win**: 1,000 cubes on
  the Note10+ go from 36 to 55 fps, 500 hold 60, and the iPhone's slow
  frames go. The step runs in the time the frame doesn't use.
- **Waiting for it within the frame is worse than inline** (24.6): the
  frame waits anyway, and the step then runs on whichever core the thread
  got, often a slow one. The gain needs the step a step late.
- **Rapier's `parallel` build is slower** on the Note10+ (28.5 against
  35.9 inline): its threads compete with Flutter's, the engine's and the
  GPU driver's for the phone's cores. It stays off.
- Results are the same either way: a step on the thread gives back the
  same poses, bit for bit, as one on the caller's (tested through the C
  interface and through Dart).

The same scene in Unity 6 (6000.3.9f1, built-in pipeline, PhysX, the
quality each platform gets by default: Ultra on the Mac, Medium on the
phones), `tools/unity_tumble`: 1,000 cubes of 0.5 m, the same drop every
8 s, 1/50 s steps, a sun with hard shadows, the same camera, as fast as the
screen goes. It times each `Physics.Simulate` itself.

| 1,000 cubes | Mac | iPhone 16 Pro Max | Note10+ |
|---|---:|---:|---:|
| Unity: fps (slowest second) | 120 (108; 113) | 120 (114) | needs Android Build Support |
| Unity: a step, median (90th) | 0.97 ms (1.10); 1.11 ms (1.28) | 4.50 ms (5.48) | |
| Fynkel, own thread: fps (slowest second) | 120 (119) | 120 (117) | 55.9 (41.3) |
| Fynkel: a step over a whole drop, median | 1.17 ms | 0.86 ms | 3.94 ms |

On the Mac and the iPhone both hold the full rate. A step costs about the
same on the Mac. On the iPhone, PhysX in Unity takes five times Rapier's
time, but it runs on Unity's job threads and the frame doesn't wait. The
Note10+ row waits for Unity's Android Build Support (Unity Hub → Installs →
6000.3.9f1 → Add modules).

**For step 4**: the step runs on the physics' own thread, a step late, as
`tumble` does here. A frame shows the last finished step. Interpolation
shows that step, eased from the one before. Anything that needs the world
mid-step waits for it first: a query, a body's position, a force. This is
the user's call before step 4 starts (§5).

