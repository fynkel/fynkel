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
- **`await fixedStep()`** in a routine, as Unity's `WaitForFixedUpdate`.

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

**Step 2: the fixed step, and the core's part.** §3.2 and §3.1's
`extend.dart`. *Done when* tests fix the fixed step's order in the frame,
the number of steps for any frame time, scale and maxDelta, `time.delta`
inside it, and `fixedStep()`; a world without systems allocates nothing in
a frame, and the stage 4 bench stays within its noise; a test system sees
each point of the frame. Lab: "Fixed step: the basics".

**Step 3: colliders and queries.** §3.3's colliders (box, sphere,
capsule) on static objects, §3.5's layers in queries, §3.6, and
`physics.drawColliders`. *Done when* every query is tested against each
shape, with masks, triggers and scale, a query that misses allocates
nothing, and moved colliders are found where they were moved. Lab:
"Physics queries: the basics".

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
