# Stage 14 public API pass: fynkel_physics

Scope: `packages/fynkel_physics/lib` (fynkel_physics.dart, raw.dart, src/), plus the guide's physics text where it names this API.
Known rows in docs/stage-14-findings.md and §21 are not repeated.
Throwaway tests: /tmp/s14api/scratch-physics/test/probe*_test.dart.

Ratings: 2 bug (P1, P2), 4 misleading (P6, P7, P11, P12), 3 unhelpful error (P4, P8, P13), 4 awkward (P3, P5, P9, P10), 0 minor. 13 findings in all.

---

## P1. A joint the native world refuses is lost for good, and the bad value stays (bug)

- **Where**: joints.dart:100, 107, 123, 288–290 (`connectedBody`, `anchor`, `connectedAnchor`, `axis` setters go through `_rejoin`, not `_setOrUndo`); physics.dart:1091–1100 (`Physics._rejoin` puts the joint back on `_waiting` only if `_id != 0`); physics.dart:1118–1120 (`_joinWaiting` does `waiting.removeAt(i)` *before* `fp_joint_add`, which can throw).
- **What happens**: set a joint's `connectedBody` to its own rigidbody, or set `anchor`, `connectedAnchor` or a hinge `axis` to a non-finite vector (the Dart check only rejects a zero axis). The setter throws `ArgumentError: a joint can't join a rigidbody to itself` or `a joint's anchor: NaN or infinity in [NaN, 0, 0]`, which is fine. But the joint has already been taken out of the native world and off the waiting list, so it is never joined again. Setting a good value back (`connectedBody = null`, `anchor = Vec3.zero`) throws nothing and joins nothing: `isJoined` stays false, `jointCount` still counts it, and the door just falls off. The bad anchor is also kept (`anchor` reads `Vec3(NaN, 0, 0)` afterwards), where every other physics setter undoes a refused value.
  The same happens with `HingeJoint(connectedBody: rb)` added to rb's object before rb: the throw comes from the *Rigidbody's* `onEnable`, is reported as `GameObject#1: Invalid argument(s): a joint can't join a rigidbody to itself`, and the joint is gone.
- **Why it matters**: a game that catches the error, or a designer tweaking values at runtime, ends up with a joint that silently does nothing until the object is disabled and enabled again.
- **Fix**: check in Dart, before anything changes: `connectedBody` isn't the rigidbody on the joint's own object, and anchors and axes are finite (as sizes already are). Wrap the four setters in `_setOrUndo`-style undo. In `_joinWaiting`, remove the joint from `_waiting` only after `fp_joint_add` succeeds. In `Physics._rejoin`, put the joint on `_waiting` whenever it isn't there, not only when it was joined.
- **Breaks code?** No.
- **Verified**: test (probe_test.dart "joint to itself, then fixed", "anchor NaN"; probe2_test.dart "joint to own body via constructor").

## P2. Query results are unmodifiable only when nothing was hit (bug)

- **Where**: queries.dart:123 (`_overlap`: `if (count == 0) return const [];`) and queries.dart:156 (`_readHits`, used by `raycastAll` and every `…CastAll`).
- **What happens**: with hits, `raycastAll`, `sphereCastAll`, `boxCastAll`, `capsuleCastAll`, `overlapSphere`, `overlapBox` and `overlapCapsule` return a growable list. With none they return `const []`, so `hits.sort(...)`, `hits.removeWhere(...)` and `found.add(...)` throw `UnsupportedError: Cannot modify an unmodifiable list`, but only on the miss path.
- **Why it matters**: `overlapSphere(...)..removeWhere((c) => c.isTrigger)` or sorting overlaps by distance is ordinary code. It passes every test that hits something and then crashes in the game the first time an explosion hits nothing. The return type `List<…>` and the docs don't say the result may be unmodifiable.
- **Fix**: either (a) return a growable empty list (`<Collider>[]`; one small allocation on a miss, which breaks the guide's "a query that misses allocates nothing"), or (b) keep `const []` and return `List.unmodifiable`/an unmodifiable view for hits too, with docs saying "unmodifiable; copy it to change it". (a) keeps every caller working. (b) is consistent and keeps a miss allocation-free, but breaks code that mutates a result; that code is already broken on misses.
- **Breaks code?** (a) no; (b) yes, for code that mutates results (fix: `[...hits]`).
- **Verified**: test (probe_test.dart "empty results are unmodifiable").

## P3. A joint on an object with no Rigidbody waits silently for good (awkward / uncaught misuse)

- **Where**: physics.dart:1108–1109 (`_joinWaiting`: `if (body == null || body._id == 0) continue;`); joints.dart:74–77 (doc: "Its object needs a rigidbody; the joint waits for it").
- **What happens**: `GameObject(components: [BoxCollider(), HingeJoint()])` gives no error, no warning and no `world.errors` entry. After a second `isJoined` is false, and the joint still counts in `physics.jointCount`. Unity's joints `[RequireComponent(typeof(Rigidbody))]` and add one.
- **Why it matters**: this is the most likely beginner mistake with joints (the joint goes on the static door frame, or the Rigidbody is forgotten), and nothing shows why the door doesn't swing.
- **Fix**: listing the joint before its Rigidbody must keep working (it does now; probe2 "joint listed before its rigidbody"), so don't throw in `onEnable`. Instead, after the frame a joint was enabled in, report once through `world.report` / a debug print when its object still has no Rigidbody: "HingeJoint on door: its object has no Rigidbody, so it joins nothing. Add a Rigidbody (isKinematic for a fixed frame)". Alternatively, add a Rigidbody as Unity does.
- **Breaks code?** No (a warning). Adding a Rigidbody automatically would change behaviour.
- **Verified**: test (probe2_test.dart "joint with no rigidbody: errors": joined false, errors []).

## P4. Rigidbody errors name the wrong member (unhelpful error)

- **Where**: rigidbody.dart:241 (`set position` reads the `rotation` getter), 249 (`set rotation` reads `position`), 284 (`addRelativeForce` reads `rotation`), 291 (`addRelativeTorque`).
- **What happens**: before the body is in the world,
  - `rb.position = Vec3(1, 2, 3)` throws `Bad state: Rigidbody.rotation: it isn't in the physics world: …`
  - `rb.addRelativeForce(Vec3.up)` throws `Bad state: Rigidbody.rotation: it isn't in the physics world: …`

  `addForce` correctly says `Rigidbody.addForce`.
- **Why it matters**: the game never touched `rotation`, so the message sends them looking at the wrong line.
- **Fix**: call `_inWorld('position')` / `_inWorld('addRelativeForce')` first in those four members, then read the pose.
- **Breaks code?** No.
- **Verified**: test (probe_test.dart "rigidbody before add").

## P5. `Rigidbody.position`/`rotation` throw off-world, where the `Behaviour.position` they override works (awkward)

- **Where**: rigidbody.dart:238–249 (`@override position`/`rotation`, both `_readState`/`_inWorld`).
- **What happens**: `Behaviour.position` (the object's) works before add and while disabled. The Rigidbody override throws a `StateError` in both cases: reading `rb.position` on a disabled (pooled) body, or setting a spawn point with `rb.position = spawn` before `world.add` or in `onLoad`. Unity's `Rigidbody.position` can be read and set on an inactive body.
- **Why it matters**: pooling with `active = false` and respawning with `rb.position = …` is a common pattern, and it throws here only in the off state.
- **Fix**: while `_id == 0`, read and write the object's world transform, as `Behaviour.position` does. A body added later starts there anyway (`_addBody` reads `worldTransform`).
- **Breaks code?** No; calls that threw now work.
- **Verified**: test (probe_test.dart "rigidbody before add": `o.position` fine, `rb.position` StateError "it isn't enabled").

## P6. The guide maps `Physics.autoSimulation` to `stepBetweenFrames`, which is something else (misleading)

- **Where**: docs/writing-a-game.md:4529 (§22 table: "`Physics.gravity`, `Physics.autoSimulation` | `physics.gravity`, `physics.stepBetweenFrames`"); physics.dart:80–93.
- **What happens**: Unity's `autoSimulation = false` (Unity 6's `simulationMode = Script`) means the game steps physics itself with `Physics.Simulate(dt)`, for trajectory previews, replays, rewinds and turn-based physics. `stepBetweenFrames` only chooses which thread the automatic step runs on. Fynkel has no manual step at all: no `simulate`, and nothing in `Physics` or §21 says so.
- **Why it matters**: a Unity developer looking up how to predict a throw's arc is told an equivalent exists, and it doesn't.
- **Fix**: take `autoSimulation` out of that row. Add "stepping physics by hand (`Physics.Simulate`, Simulation Mode Script)" to §21's Physics line, or add `physics.simulate(seconds)` with an `autoSimulation`-like flag.
- **Breaks code?** No.
- **Verified**: read (grep for simulate/autoSimulation in lib: none).

## P7. The guide names `body.velocity`, which doesn't exist (misleading)

- **Where**: docs/writing-a-game.md:3029 ("A Flutter widget that reads a body as it builds (`body.velocity` for a speedometer)").
- **What happens**: `Rigidbody` has `linearVelocity` (as Unity 6), not `velocity`. §22 says so. Only `CharacterController` has a `velocity`.
- **Fix**: `body.linearVelocity`.
- **Breaks code?** No.
- **Verified**: read (rigidbody.dart:207).

## P8. Errors from the native checks carry no name, no component and sometimes the wrong call site (unhelpful error)

- **Where**: raw.dart:113–114 (`PhysicsStatus.invalidValue` becomes `ArgumentError(message)`, unnamed); colliders.dart:72–75 (`center` isn't checked in Dart, though sizes are); joints.dart:84–93 (anchors aren't checked); physics.dart:557 (mesh add).
- **What happens**: Dart-side checks give `Invalid argument (radius): must be finite and more than 0: -1.0`. Values only the Rust side checks come out as bare `Invalid argument(s): …`, with nothing saying which component or object:
  - `BoxCollider(center: Vec3(nan, 0, 0))` is accepted by the constructor (`BoxCollider(size: nan…)` isn't). It fails at `world.add` and goes to `world.errors` as `a collider's center: NaN or infinity in [NaN, 0, 0]`.
  - `MeshCollider(mesh: Mesh())` gives `a mesh needs at least one triangle`, with no collider or object named.
  - Joint refusals come from wherever the next `_follow()` runs: a later Rigidbody's `onEnable`, or a query (P1).
- **Why it matters**: in a scene of many colliders the developer can't tell which one is bad, and a NaN centre from a bad computation is reported away from where it was made.
- **Fix**: check `center`, `anchor`, `connectedAnchor` and the axes for finiteness in Dart, in constructors and setters, as sizes are, with the parameter name. Where a native refusal can still happen (a mesh), catch it in `_addNative`/`_respec` and rethrow as `ArgumentError('$runtimeType on $gameObject: $message')`.
- **Breaks code?** No (errors come earlier and say more).
- **Verified**: test (probe_test.dart "collider center NaN", "bad query values" mesh cases; probe2 "joint to own body…").

## P9. raw.dart's split: `PhysicsException.status` has a type the main library doesn't export (awkward)

- **Where**: fynkel_physics.dart:54 (`export 'src/raw.dart' show PhysicsException;`); raw.dart:6 (`PhysicsStatus` only there); raw.dart (lib/src) 57–69, 175 (`RawPhysics.gravity` is setter-only).
- **What happens**: a game that catches `PhysicsException` to tell a poisoned world (`status == PhysicsStatus.poisoned`) from a one-off panic must import `package:fynkel_physics/raw.dart`, which says "A game uses `package:fynkel_physics/fynkel_physics.dart`". raw.dart's doc also says it's "what package:fynkel_physics builds its colliders and rigidbodies on", which reads as if `world.physics` could be reached through it. Every `RawPhysics()` is a separate world, though, and a game can't get `world.physics`'s. `RawPhysics` has a `gravity` setter with no getter.
- **Fix**: export `PhysicsStatus` from fynkel_physics.dart. Add to raw.dart's library doc: "each `RawPhysics()` is a world of its own, unconnected to `world.physics`; for tests and benches". Add a `gravity` getter or make it `setGravity`.
- **Breaks code?** No.
- **Verified**: read.

## P10. Joint and material value classes have no `copyWith`, and the obvious rewrite silently zeroes the motor (awkward)

- **Where**: joints.dart:12–72 (`JointLimits`, `SoftJointLimit`, `JointMotor`, `JointSpring`); colliders.dart:28–63 (`PhysicsMaterial`).
- **What happens**: Unity's are structs changed in place (`var m = hinge.motor; m.targetVelocity = v; hinge.motor = m;`). Here they're immutable with every field defaulted, so the natural port `hinge.motor = JointMotor(targetVelocity: v)` sets `force` to its default 0, and the motor stops pushing with no error. Changing one field takes `JointMotor(targetVelocity: v, force: hinge.motor.force)`, and the same for springs, limits and materials. package:fynkel's value types have `copyWith` (`Transform`, `Quality`).
- **Why it matters**: a car's wheel motor or a door's spring target changed every step is the normal use of a motor or spring.
- **Fix**: add `copyWith` to the five classes; the guide's door example can show `hinge.motor = hinge.motor.copyWith(targetVelocity: v)`.
- **Breaks code?** No.
- **Verified**: read; test (probe2_test.dart "motor changed every step" with the full rewrite works).

## P11. `ControllerColliderHit.moveLength` is Unity's name with another meaning (misleading)

- **Where**: characters.dart:297–301, 324–325.
- **What happens**: it's the length of the motion *asked for* (`motion.length`). Unity's `moveLength` is "how far the character has travelled until it hit the collider". The doc is accurate to the code, but it says nothing about differing from Unity, and the class doc says "(Unity's `ControllerColliderHit`)".
- **Why it matters**: code ported from Unity that uses `moveLength` to tell a graze from a head-on stop (or to scale a push) gets the full requested length every time.
- **Fix**: report the distance travelled before the hit if the native side has it (the move's TOI). Otherwise, say in the doc "the length asked for; Unity's is how far it went before the hit" and add it to §22's differences.
- **Breaks code?** Changing the value would; the doc fix doesn't.
- **Verified**: read.

## P12. Docs missing defaults and throws on a few members (misleading, docs)

- **Where**:
  - rigidbody.dart:156–161 `maxAngularVelocity`: no default (50) and no "0 or more, or it throws", though the siblings say both.
  - joints.dart:139–147 `breakForce`/`breakTorque`: the setters don't say 0 or less throws; only the constructors do.
  - joints.dart:286, 480 `HingeJoint.axis`/`CharacterJoint.axis`: no default (X) and no zero-axis throw.
  - characters.dart:84–110 `slopeLimit`, `stepOffset`, `skinWidth`, `mass`: no throws, and `slopeLimit` doesn't say up to pi (a value in degrees, say `45`, is refused, which is good, but undocumented).
  - queries.dart:168–172: the extension doc doesn't say radius/half-extents must be more than 0 and `maxDistance` 0 or more, or they throw.
- **Fix**: one clause each, in the style the neighbouring members already use.
- **Breaks code?** No.
- **Verified**: read; test for the throws (probe_test.dart "bad query values", "character slopeLimit in degrees").

## P13. `ignoreCollision`'s other-world error names nothing (unhelpful error)

- **Where**: contacts.dart:182: `throw StateError('the colliders are in another world');`.
- **What happens**: it doesn't say which collider, which world, or what to do. Every other message in the file names the component (`SphereCollider.ignoreCollision: …`).
- **Fix**: `StateError('ignoreCollision: $collider1 and $collider2 must both be in this world; ${a == this ? collider2 : collider1} is in another')`.
- **Breaks code?** No.
- **Verified**: read.

---

## Checked and found fine

- **Names against Unity 6**: `Rigidbody` (`linearVelocity`, `linearDamping`, `angularDamping`, `isKinematic`, `useGravity`, `constraints`, `interpolation`, `collisionDetectionMode`, `centerOfMass`/`resetCenterOfMass`, `worldCenterOfMass`, `movePosition`/`moveRotation`, `addForce`/`addRelativeForce`/`addTorque`/`addRelativeTorque`/`addForceAtPosition`/`addExplosionForce` with Unity's parameter names, `sleep`/`wakeUp`/`isSleeping`). Also `ForceMode`, `RigidbodyInterpolation`, `CollisionDetectionMode` (the three continuous modes documented as one), `RigidbodyConstraints` (Unity's values, `|`, `has`), and `PhysicsMaterial`/`PhysicsMaterialCombine` (order matches Unity's priority; one friction documented).
- **Collider types**: `BoxCollider`, `SphereCollider`, `CapsuleCollider`/`CapsuleDirection`, `MeshCollider` (hull on dynamic documented), `isTrigger`, `center`, `attachedRigidbody`, `closestPoint`, `Collider.raycast`.
- **Contacts and callbacks**: `Triggers`, `Collisions`, `Collision` (`impulse`, `relativeVelocity` sign = other less own, `contactCount`, `getContact`, `contacts`), `ContactPoint`, and `JointBreaks.onJointBreak`.
- **Queries**: `raycast`, `raycastAll`, `sphereCast`, `boxCast`, `capsuleCast` and their `All` forms, `overlap*`, `check*`. Arguments are in Unity's order. `layers:`/`maxDistance` match `world.raycast`, and `RaycastHit` vs `RayHit` is documented in §22. `QueryTriggers` = `QueryTriggerInteraction`. A zero direction gives no hits as in Unity, and `Ray` itself refuses a zero direction with a clear message.
- **Layers**: `ignoreLayerCollision`/`getIgnoreLayerCollision` (a `RangeError` naming `layer1`/`layer2` and the value) and `ignoreCollision`/`getIgnoreCollision`. The character controller respects both and goes through triggers (tested).
- **CharacterController**: Unity's defaults (2 m, 0.5, 45°, 0.3, skin 0.08), `move`/`simpleMove`/`isGrounded`/`collisionFlags`/`velocity`, `CollisionFlags` names. The extras `mass`, `ground`, `moveWithGround` and `groundVelocity` are documented. A controller together with a Rigidbody on one object behaves sanely (tested).
- **Joints**: Unity's defaults (SpringJoint 10/0.2, CharacterJoint −20/70/40/40, hinge axis X), radians documented, and a limits error that catches degrees passed by habit (`must be -pi to pi radians … "-45.0 to 90.0"`). A joint listed before its Rigidbody joins.
- **Settings**: `gravity` (Unity's), `defaultSolverIterations` (6), `sleepThreshold` (0.005), `queriesHitTriggers` (true), `stepBetweenFrames` and `drawColliders` (on in debug builds through `world.debug.enabled`).
- **Dart-side error messages**: name the value and the parameter (`mass`, `radius`, `size.x`, damping, `constraints`, `breakForce`, `slopeLimit`, swing/twist limits, spring distances, `bounciness`). The `_notInWorld` messages say why (no object, not added, disabled, hidden, destroyed). Native messages for bad query values, NaN gravity, velocities, forces and explosions are clear apart from P8's missing names.
- **Queries from a destroyed object's `physics`** still work rather than throwing; that's harmless.
