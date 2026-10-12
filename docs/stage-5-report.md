# Stage 5 report: physics

Step 10 of [the stage 5 plan](stage-5.md). Steps 1 to 9 and the fixes
after them are merged, the last as 93ce4afe. The details of each step,
and how each number is taken, are in [stage-5.md §4 and
§6](stage-5.md#4-steps); the numbers here are from the profile build (AOT)
with package:fynkel_measure's tool.

## The result

Things in a Fynkel game can now fall, land, bump, roll, hang and push, as
in Unity. Physics is a package, `fynkel_physics`, on
[Rapier](https://rapier.rs): a game adds it to its pubspec, and one
without it ships none of it and runs as before. Once added it reads as
Unity's: colliders (box, sphere, capsule, mesh) and a `Rigidbody` are
behaviours on an object; `physics.raycast` and the other queries are on
every object; `with Triggers` and `with Collisions` give the six
callbacks; layers that don't meet pass through each other; a
`CharacterController` walks up steps and slopes; and four joints hold
bodies together. The core gained only what isn't physics: a fixed step
(`onFixedUpdate`, `time.fixedDelta`, `await fixedStep`) and
`package:fynkel/extend.dart`, through which a package takes part in the
world's frame.

The step runs **between frames, on the physics' own thread**: it starts
when a frame is done, and its poses and callbacks come at the start of
the next. A query in an update never waits, and sees the world as drawn.
This is what holds 500 bodies at 60 on the Note10+ (§ below), where the
step inside the frame gave 44. A game can still step in the fixed step,
as Unity does (`physics.stepBetweenFrames = false`).

`examples/bowling` is the fourth game: a lane, a ball and ten pins of
real size and weight, placed by a drag and bowled by a flick, scored over
ten frames, with a gutter and a pit that are triggers. 475 lines of game,
203 of HUD and 90 of scoring. coins' pickups are now triggers.

| Each game (fresh fps, median) | Mac (M3 Max, 120 Hz) | iPhone 16 Pro Max (120 Hz) | Note10+ (60 Hz, 30 s) |
|---|---|---|---|
| hello, garden, arena | **120** | **120** | **59.9** |
| coins | **120** | **120** | **57.9** (stage 4: 57.9) |
| bowling | **120** | **120** | **58.9** |

| Physics alone | Mac | iPhone 16 Pro Max | Note10+ |
|---|---|---|---|
| 500 moving rigidbodies, 10 rays an update | 120 | 120 | **59.9** |
| 500, every cube hearing its contacts | 60* | 120 | 59.9 |
| A step of 1,000 cubes, a whole drop (median) | 1.17 ms | 0.86 ms | 3.94 ms |
| 1,000 cubes live, the step on its own thread | 120 | 120 | 55.9 |
| What the package adds to an app | | 2.3 MB (≈1.2 to download) | 2.3 MB (arm64) |

\* On a 60 Hz external display that day; no slow frames.

**Every bar the plan set was met**: a game without physics runs as fast
as at the end of stage 4 and a frame allocates nothing (tested, with fixed
steps and a system); 500 moving bodies hold the full frame rate on all
three; the cost of 1,000 bodies and the package's size are written down.
The same 1,000 cubes in Unity 6 (`tools/unity_tumble`) hold 120 on the Mac
and the iPhone too; a PhysX step there takes 4.5 ms on the iPhone, against
Rapier's 0.86 here, though Unity runs it on its job threads.

## What stage 5 built

- **Rapier in a package** (step 1): crates `fynkel_physics` and
  `fynkel_physics_ffi`, `packages/fynkel_physics` with a build hook that
  builds the library with the app, and `tumble`, the 1,000-cube bench.
- **The fixed step** (step 2): `onFixedUpdate`, `fixedDelta`,
  `fixedAlpha`, `inFixedStep`, `await fixedStep`, and `WorldSystem` in
  `extend.dart`.
- **Colliders and queries** (step 3): box, sphere and capsule; every
  query of Unity's `Physics` (rays, casts and overlaps, single and
  `All`) with layer masks and triggers;
  `collider.raycast`, `closestPoint`; `physics.drawColliders`. A query
  that misses allocates nothing.
- **Rigidbodies** (step 4): Unity's properties, the six force calls and
  four `ForceMode`s, `movePosition`, constraints, interpolation,
  continuous detection, sleep, compound bodies, teleports,
  `PhysicsMaterial`, gravity, and the step between frames. Physics
  follows only the objects that moved.
- **Callbacks and the layer matrix** (step 5): `Triggers`, `Collisions`,
  `Collision`, `ContactPoint`, every row of Unity's table and every exit,
  `ignoreLayerCollision`, `ignoreCollision`. Only listeners pay.
- **Mesh colliders** (step 6): `MeshCollider(mesh:, convex:)` and
  `addToParts` for a model; triangles on static colliders, the hull on a
  dynamic body.
- **The character controller** (step 7): `move`, `simpleMove`,
  `isGrounded`, `CollisionFlags`, `slopeLimit`, `stepOffset`,
  `onControllerColliderHit`, and pushing bodies by its `mass`.
- **Joints** (step 8 and after): `FixedJoint`, `HingeJoint` (limits, a
  motor, a spring), `SpringJoint`, `CharacterJoint` (a ball joint with
  twist and swing limits), breaking by force and torque, `onJointBreak`.
- **The bowling game and coins** (step 9), measured on the three devices
  and played on the phones.
- **The lab**: 14 new scenes, 28 in all, each feature with its "the
  basics" scene; among them a stack, a ball through walls, a rock, a
  course for the character, a bridge to break, and a chain with a weight
  to drag.
- **Tests**: 107 in Rust (the physics crate and its C interface), 89 in
  package:fynkel_physics, 378 in package:fynkel, 56 in coins, bowling and the lab.
  CI now builds and tests the lab and bowling, which it had left out.

## What was found on the way

- **The Note10+ couldn't hold 500 bodies with the step in the frame**
  (44.9 fps with 500, 30 with 1,000). Rapier's `parallel` build was
  slower still (its threads fight Flutter's and the GPU driver's for the
  cores); stepping on a thread of its own, waited for in the same frame,
  was worse. Stepping between frames, a step late, was the win: 59.9 with
  500 and 10 rays an update.
- **Rapier turns a body at most a quarter turn a step**: 39 rad/s at 50
  steps a second, below Unity's default `maxAngularVelocity`. The bowling
  ball skidded and slowed; bowling steps 100 times a second.
- **A joint chain pulls apart when dragged**: at 50 steps a second the
  chain's twelve joints opened by up to 74 cm in all, and snapped back,
  which looked like jumping. At 120 steps a second, 16 cm. Heavy ends do
  the same while hanging (42 cm under 50 kg on 0.2 kg links), as in
  PhysX; Unity advises mass ratios under 10:1.
- **A spring joint was a rod within its distances**: Rapier holds a motor
  with no stiffness and no damping still. Found in the lab ("pull the
  crate down" did nothing) and fixed (#60).
- **Rapier lets a body sleep while a joint's drive still pulls**, so a
  hinge's spring stopped short of its target; such bodies are woken each
  step until they get there.
- **Rapier's swing limits are a square cone**, Unity's an elliptical one:
  swung on a slant, an arm on 40° limits goes to 54°.
- **Allocation in the JIT**: a struct's `.ref`, and a double passed to a
  call the JIT doesn't inline, each allocate; the query paths read and
  write through buffers. A behaviour put on a model's part never had
  `onEnable` (a core fix, step 6).
- **A test merged red** (#54): CI was checked after merging, not before.
  PRs are now merged only once `gh pr checks` is green.

## Still open

- **`contacts_alloc_test` flakes on CI** now and then (a rerun passes).
- **Between frames, `await fixedStep` resumes before that step's physics
  has run**, not after as in Unity.
- **Not built, until a game asks**: `ConfigurableJoint`, Rapier's
  multibody joints (exact chains, no stretch), limits' springs and
  bounciness, `staticFriction`, `bounceThreshold`, contact offsets, and
  Unity's `Physics.ClosestPoint` with a pose.
- **1,000 bodies on the Note10+** run at 55.9, not 60; the step takes
  the time between frames and more.
- **Unity on the Note10+** wasn't measured: no Android support in Unity,
  by choice.
- **Not in stage 5, by plan**: 2D physics (waits for 2D), and from
  stage 4: audio, animation of models, point and spot lights and
  shadows, saving, gamepads, scene files, an editor.
- **From earlier stages**: the Note10+ at full resolution, the loop's
  cost there, the view's GPU device made on the UI isolate, finalizers
  that don't tell the GC their size, 64-bit Android only.

## Seen on each device

Step 9 is done when bowling and coins are played on each device.
Confirmed on 2026-10-12 ("all working very good"), with the lab's physics
scenes:

- [x] Mac
- [x] iPhone 16 Pro Max
- [x] Galaxy Note10+

Step 10 is done when you have read this and [the guide's physics
part](writing-a-game.md#14-physics-colliders-rigidbodies-queries-and-callbacks).
Closed on 2026-10-12 ("ok merge #63 and continue step 10"):

- [x] Read

**Stage 5 is done.**
