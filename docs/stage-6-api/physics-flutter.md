# API review: package:fynkel_physics and package:fynkel_flutter

Reviewer scope: everything exported by `packages/fynkel_physics/lib/fynkel_physics.dart`
and `packages/fynkel_flutter/lib/fynkel_flutter.dart`, at branch `stage6-plan`
(70c49469). Paths below are relative to `packages/`. Line numbers are the
current file's. Missing docs were checked against `/tmp/s6/api/nodoc-*.txt`
(the `public_member_api_docs` lint) and by reading.

---

## 1. package:fynkel_physics: summary

**Verdict: strong, keep its shape.** It is a close and mostly faithful copy
of Unity's PhysX API. The class names, member names, defaults (mass 1,
angularDamping 0.05, maxAngularVelocity 50, friction 0.6, slopeLimit 45,
stepOffset 0.3, skinWidth 0.08, CharacterJoint twist -20..70 / swing 40,
SpringJoint 10 / 0.2) and callback rules (callbacks reach disabled behaviours,
a collider's object and its rigidbody's object both hear) all match Unity.
Units are mostly stated, and the checks are thorough and say what's allowed.
The problems fall into five groups:

1. **The unit for angles differs between packages.** Physics uses degrees
   (joints, `slopeLimit`, `HingeJoint.angle/velocity`, `JointMotor.targetVelocity`),
   while core uses radians everywhere (`Quat`, `rotate`, `Orbit`, input
   twist) except `Camera.fovY`.
2. **Two "position"s.** On a Rigidbody, `position` and `rotation` mean the
   body's world pose. Everywhere else in Fynkel they mean the object's local
   transform, and `worldPosition`, `translate` and the rest that Rigidbody
   inherits still act on the object.
3. **Calls on a body that isn't in the world.** `wakeUp`, `sleep`,
   `isSleeping`, `position` and `centerOfMass` throw when the body isn't in
   the world, and the error gives the wrong reason for a destroyed object
   (findings #14, #15). Changing gravity leaves sleeping bodies asleep (#14).
4. **Plumbing is public.** The `WorldSystem` hooks are public on `Physics`
   (`physics.dispose()` and `physics.afterFrame()` can be called), and so
   are `PhysicsQueries`, `PhysicsLayers` and `PhysicsStatus`.
5. **About 45 public members have no doc** (struct fields, shape sizes,
   enum values). Two docs are wrong: `JointBreaks` and `Rigidbody.position`.

Counts: about **235** public members, counting each enum or constant set as
one per type and each property as one. **47 are flagged** in the table, and
the other **~188 are "keep"**.

## 2. package:fynkel_physics: table

| Name | File:line | Verdict | Proposal | Why | Breaking? |
|---|---|---|---|---|---|
| Joint angles: `JointLimits.min/max`, `SoftJointLimit.limit`, `JointMotor.targetVelocity`, `JointSpring.targetPosition`, `HingeJoint.angle/velocity`, `CharacterController.slopeLimit` | fynkel_physics/lib/src/joints.dart:12-45, 287, 290; characters.dart:75 | change shape | Pick one rule for the whole framework. Recommended: radians, as core uses (`Quat.euler`, `rotate`, `Orbit.yaw`), plus a core constant `const deg = pi / 180` so `JointLimits(min: -90 * deg)` still reads naturally. The range checks (±π, 0..π) catch most values passed in degrees by mistake; they should say "in radians: -90° is -90 * deg". If the user keeps degrees, rename the fields so the unit is in the name (`minDegrees`) or state the rule in the guide's §20. | Core's rule is "angles are in radians" (math.dart:113). A Unity user and a Fynkel user each guess wrong in one package. Rigidbody's `angularVelocity` is rad/s right next to the hinge's deg/s. | yes |
| `Rigidbody.position` / `rotation` (overrides) | rigidbody.dart:191-200 | fix behaviour + document | Keep Unity's meaning (the body's world pose). (a) Reading it before the body is in the world returns the object's `worldTransform` instead of throwing, and writing it then sets the object. (b) Rewrite the doc. The current one is "(Not its object's Transform.position, as a behaviour's position is elsewhere.)" It should say: "World space, unlike `Behaviour.position`, which is the object's local position; `worldPosition` and `translate` still move the object." | `Behaviour.position` (motion.dart:628) is local, and this override is world. `rb.position` and `rb.worldPosition` agree only for root objects, and `rb.translate` bypasses the body. | no (a is looser) |
| `Rigidbody.wakeUp` / `sleep` / `isSleeping` | rigidbody.dart:204-210 | fix behaviour | When the body isn't in the world: `wakeUp` and `sleep` do nothing (as in Unity), and `isSleeping` returns false. | Finding #14: there is no way to ask first, and the throw comes from a harmless call. | no |
| `Rigidbody._inWorld` / `Collider._inWorld` messages | rigidbody.dart:340-343; colliders.dart:132-135 | fix behaviour | Name the real reason: "...: its object was destroyed", "...: it was removed", "...: it's disabled", "...: its object is hidden (activeInHierarchy false)", "...: its object isn't added to a world yet". | Finding #15. Errors should say what to do. | no |
| `Physics.gravity` setter | physics.dart:63-68 | fix behaviour | Wake every dynamic body when gravity changes (state the deliberate difference from Unity in the doc). | Finding #14: bodies hang in the air. Unity has the same trap, but it's a known complaint there too. | no |
| WorldSystem hooks on `Physics`: `beforeFrame`, `beforeFixedUpdate`, `afterFixedUpdate`, `beforeUpdate`, `afterLateUpdate`, `afterFrame`, `dispose`, `componentAdded` | physics.dart:233-346 (core: fynkel/lib/src/systems.dart:16-50) | make internal | In core, annotate the hooks `@protected` (or `@visibleForOverriding`), and `dispose` too. Physics needs no change. | `world.physics.dispose()` and `physics.afterFrame()` show up in autocomplete and break the world. Cross-package: this is core's `extend.dart`. | no (analyzer hint only) |
| `PhysicsQueries` (extension) | queries.dart:168 | make internal (merge) | Move the 14 query methods into `class Physics` (same library through `part`) and drop the extension and its export. | An extension on a final class of the same library has no purpose. It splits Physics' docs in two and adds an exported name. | only for code that names the extension (none in examples) |
| `PhysicsLayers` (extension) | contacts.dart:147 | make internal (merge) | Same: make the 4 methods members of `Physics`. | Same. | as above |
| `PhysicsStatus` | raw.dart:14; fynkel_physics.dart:54 | make internal | Export it only from `raw.dart` and keep `PhysicsException` (with `message`) in the main library. Document `PhysicsException.status/message` (raw.dart:48-49). | A game can't act on `nullPointer` or `poisoned`; game mistakes already throw `ArgumentError`/`StateError`. | barely |
| `GameObjectPhysics`, `BehaviourPhysics`, `WorldPhysics` | physics.dart:1007-1024 | keep (noted) | They must stay named to be exported. If core gives `GameObject` and `Behaviour` a shared public supertype with `world`, merge the first two into one extension. | The names are noise, but Dart needs them. | no |
| `ignoreLayerCollision(l1, l2, [bool ignore = true])`, `ignoreCollision(c1, c2, [bool ignore])` | contacts.dart:153, 172 | change shape | Make it `{bool ignore = true}` (named). | Effective Dart: avoid positional booleans. `ignoreLayerCollision(a, b, false)` reads as the opposite of what it does. Unity's positional bool is the reason it's like this, but named reads better. | yes (call sites with false) |
| `PhysicsHit` | queries.dart:20 | rename | Rename to `RaycastHit` (Unity's name for every cast's hit). In core, rename `RayHit.object` to `gameObject` (world.dart:505) so both hits name the object the same way. | There are three names for a hit: Unity's `RaycastHit`, core's `RayHit` and this `PhysicsHit`. Core says `.object` and physics says `.gameObject`. Agents praised "Unity names working as guessed". | yes |
| `PhysicsHit.collider` | queries.dart:23 | document + add counterpart | Doc: "The collider hit." Add `Rigidbody? get rigidbody => collider.attachedRigidbody` (Unity's `RaycastHit.rigidbody`). | Undocumented. The body is the usual next step after a hit (shooters, drag). | no |
| `PhysicsHit.point` doc | queries.dart:25-27 | fix behaviour / document | A cast that starts inside a collider (distance 0) returns `point` = zero, so the hit lands at the world origin. Return the cast's origin instead, or document it in `boxCast` and the other casts. | A silent (0,0,0) is a trap. | no |
| `raycast` / `world.raycast` pair | queries.dart:171; fynkel/lib/src/world.dart:489 | document | Each doc should name the other and say which to use: `world.raycast` for taps on what's drawn, `physics.raycast` for colliders. The guide says it; the API docs don't. | Two `raycast`s with different targets, results and normal conventions. | no |
| Add `linecast` | queries.dart (new) | add | `PhysicsHit? linecast(Vec3 start, Vec3 end, {layers, triggers})`. | Unity's most common line-of-sight query. The guide's own `Sight` example builds it by hand (Ray plus maxDistance = length). | no |
| `QueryTriggers` | queries.dart:8 | keep (noted) | Keep the shorter name; add "Unity's `QueryTriggerInteraction`" to the doc's first line (it is already there). | A deliberate, readable difference (`triggers:`). | no |
| `PhysicsMaterial.frictionCombine` / `bounceCombine` | colliders.dart:34-35 | document | "How this friction mixes with the other collider's ([PhysicsMaterialCombine]); average unless set." | No doc. | no |
| `PhysicsMaterial.dynamicFriction` | colliders.dart:27-29 | keep (noted) | Its doc already says there's no static friction. Keep the Unity name so ported code compiles. | Fine. | no |
| `PhysicsMaterialCombine` values | colliders.dart:12 | document | One line per value; the enum's own doc already gives the priority order. | No doc on the values. | no |
| `BoxCollider.size`, `SphereCollider.radius`, `CapsuleCollider.radius/height/direction`, `CharacterController.radius` | colliders.dart:196, 226, 267, 275, 283; characters.dart:65 | document | Give each its unit and space ("in metres, in its object's space: the object's scale multiplies it"), default and range ("more than 0; throws ArgumentError"). | The main fields of the main classes have no doc. | no |
| `Collider.center` | colliders.dart:55 | document | Say that the object's scale applies. | Same scale question as #16. | no |
| `CapsuleDirection` values | colliders.dart:248 | document | `x`, `y` and `z` are the object's axes. | No doc. | no |
| `CollisionDetectionMode` | rigidbody.dart:43 | document (or merge) | `continuous`, `continuousDynamic` and `continuousSpeculative` all write the same flag (rigidbody.dart:336). Say so: "kept for ported code; all three are Rapier's CCD against everything". Or keep only `discrete` and `continuous`. | Three names for one behaviour, with no doc per value. Unity users will think they're choosing something. | no (doc) |
| `RigidbodyConstraints(int)` constructor and `value` | rigidbody.dart:48 | change shape | Make the constructor private (`._`), as `CollisionFlags` does (characters.dart:11). Add `minus(other)` (core's `LayerMask.minus` word) to clear axes. Document the constants (lint: rigidbody.dart:49-60). | Any int can be built and then fails later in `_check`. There is no way to unfreeze one axis. There's no doc on 10 constants. | yes (rarely used) |
| `RigidbodyConstraints.has` / `CollisionFlags.has` vs core `LayerMask.contains` | rigidbody.dart:63; characters.dart:25 | keep (noted) | `has(flags)` (a subset test) differs from `contains(layer)` (a single bit), so the different words fit. Keep them. | — | no |
| `Rigidbody.linearDamping` / `angularDamping` | rigidbody.dart:106-114 | document | Units (per second, a drag coefficient: velocity × (1 − d·dt)) and defaults (0; 0.05). | No unit; angularDamping has no default in its doc. | no |
| `Rigidbody.centerOfMass` / `worldCenterOfMass` getters | rigidbody.dart:144-158 | document / fix behaviour | Before the body is in the world: return the set value or `Vec3.zero` (and the object's world position for the world getter) instead of throwing, or document the throw. | Undocumented throw. | no |
| `Rigidbody.addForce` & co. | rigidbody.dart:212-249 | document | Say "Throws StateError while not in the world" and "call in onFixedUpdate for `force`/`acceleration`; impulses anywhere". The guide says this; the docs don't. | Edge cases are missing from the docs. | no |
| `Rigidbody.movePosition` on a dynamic body | rigidbody.dart:251-252 | document | Spell out that a dynamic body "goes there at once" as a teleport (contacts aren't swept). Add the kinematic-trigger recipe (finding #28: `movePosition` vs `position =`). | Finding #28. | no |
| `Rigidbody` kinematic sleep | rigidbody.dart:99-104 | fix behaviour or document | A kinematic body moved by `movePosition`, or by setting its object's transform, should be woken (it already is by `movePosition` in Rapier; check the transform path). Doc on `isKinematic`: "an unmoved kinematic body sleeps, and trigger stay stops". | Finding #18. | no |
| Missing Rigidbody counterparts | rigidbody.dart | add (low) | `getPointVelocity(Vec3)` (needed by vehicles and drag) and `detectCollisions`. Leave `inertiaTensor`, `sweepTest` and `maxLinearVelocity` for later. | Unity members people reach for. Low priority. | no |
| `Collision.contacts` | contacts.dart:110 | keep | Unity has `contacts`, `GetContact` and `contactCount` too, with the same allocating/non-allocating split. | — | no |
| `ContactPoint.thisCollider` / `otherCollider` | contacts.dart:130-131 | document | "This side's collider" / "The other side's". | No doc. | no |
| `CollisionFlags.value` | characters.dart:11, 27 | document | One line: the bits, as Unity's `CollisionFlags`. | Lint. | no |
| `CharacterController.velocity` / `move` time | characters.dart:122, 136 | fix behaviour | When `time.delta == 0` (paused with `time.scale = 0`), `move` divides by `fixedDelta` and `velocity` lies. Use `time.unscaledDelta`, or keep the last velocity when delta is 0. | Pause (finding #2) makes this common. | no |
| `CharacterController` upright | characters.dart:37-38, 198-206 | document | The capsule follows the object's full rotation; Unity's is always upright. Say "keep its object upright; turn a child for the model". | A quiet difference from Unity. | no |
| `ControllerColliderHit.moveLength` | characters.dart:237 | document | Give it its own line ("how far, in metres"). | It shares a doc with `moveDirection`. | no |
| `Joint.anchor` / `connectedAnchor` | joints.dart:74-91 | document | "In its object's space, scale included: (0.5, 0, 0) is the +X face of a unit cube however the object is scaled" (that's what `_scaled` does, joints.dart:193). | Finding #16. | no |
| `JointLimits.min/max`, `SoftJointLimit.limit`, `JointMotor.targetVelocity/force`, `JointSpring.spring/damper/targetPosition` | joints.dart:15-16, 24, 33-34, 43-45 | document | One line each, with the unit. The class docs carry some of it, but fields show alone in IDE hovers. | Lint, 8 fields. | no |
| `HingeJoint.limits/motor/spring`, `CharacterJoint.highTwistLimit/swing2Limit`, `SpringJoint.maxDistance` | joints.dart:262, 271, 280, 450, 460, 369 | document | Give each its own doc (most share their neighbour's). | Lint. | no |
| `JointBreaks` doc | joints.dart:512-516 | document (wrong) | It says the callback comes "after the joint is taken off its object". It actually comes before: `_tellBroken` calls, then `removeComponent` (physics.dart:870-878), which is Unity's order. Fix the sentence: "the joint is out of the physics but still on its object; it's removed right after". | Wrong doc. | no |
| `Physics.stepBetweenFrames` | physics.dart:59 | keep (noted) | Later, if a manual step is added, turn it into Unity's `simulationMode` enum (`betweenFrames`, `fixedUpdate`, `script`) with `physics.simulate(dt)`. | — | no |
| `Physics.colliderCount/rigidbodyCount/jointCount/meshCount` | physics.dart:224-231, 467 | keep | Diagnostics, documented. | — | no |
| Query cost after any move | physics.dart:476-524 | fix behaviour | Core keeps a list of objects moved since the last read (stage 5's plan). `_followMoved` walks only those, not every body and collider. | Finding #9: 4.7 ms per frame, silently. Also hit by `attachedRigidbody` (colliders.dart:88), `closestPoint` and `move`. | no |
| No drag helper | — | add (later) | A small `DragRigidbody` behaviour (a spring joint to a kinematic point under the finger), or a guide recipe. | Finding #17: every physics game writes 70 lines. | no |
| `MeshCollider.addToParts` | colliders.dart:328 | keep | Not Unity's, but small and well named. | — | no |

Errors (criterion 7, sampled): the `ArgumentError`s give the value, the name
and the allowed range (good). `MeshCollider on X: no mesh. Give it one, or a
MeshRenderer with one` is a model message. The weak ones are the shared
`"it isn't in the world (not enabled, or its object isn't shown)"` (gives
the reason only, often the wrong one, and no fix) and `ignoreCollision`'s
`'the colliders are in another world'`, which doesn't name the method.

---

## 3. package:fynkel_flutter: summary

**Verdict: a good small core, but not yet a Flutter-shaped API.** The three
pieces are right: `FynkelApp` as a shortcut, `FynkelGame` as the
restartable scope, and `FynkelView` as the surface. The `Listener`-based
input that stays out of the gesture arena is the correct choice, and so are
the bounded-size error, the error panel and F5. Where it falls short of
Flutter's conventions:

- `FynkelApp` is a bare `WidgetsApp`: no Navigator, Overlay, Material or
  Theme (finding #1, a blocker).
- The view takes every key (#4).
- There's no pause that ties the game to the app lifecycle, routes or
  dialogs (#2).
- There's a `maybeOf` without an `of`, and the root-or-world choice is an
  assert rather than two constructors.
- A plugin failure stops the ticker for good. This is probably finding #25:
  a widget test without the channel mock gets `MissingPluginException`, and
  `_fail` stops the ticker (fynkel_view.dart:375, 405).
- There's no bridge between the two `Color`s (#6), and restart/dispose
  doesn't destroy the game (#7).

Counts: about **40** public members. **14 are flagged** and **~26 are "keep"**.

## 4. package:fynkel_flutter: table

| Name | File:line | Verdict | Proposal | Why | Breaking? |
|---|---|---|---|---|---|
| `FynkelApp` (built on `WidgetsApp` with only `builder`) | fynkel_flutter/lib/src/fynkel_app.dart:33-47 | change shape | Build on `MaterialApp` with `home:` a page holding `FynkelGame` → `Stack[FynkelView, overlay]`, so the HUD sits inside a route: `showDialog`, `Slider`, `Tooltip`, `ScaffoldMessenger` and `Navigator.push` all work. Add `theme`, `darkTheme`, `themeMode` and `navigatorKey`. Name the Material dependency in the doc; a game that wants no Material uses the pieces. | Finding #1, a blocker: "No Overlay widget found". The guide's own HUD route leads there. | no (more works) |
| `FynkelApp.overlay` | fynkel_app.dart:22 | rename | `hud:` (same signature), or `builder:` to match `FynkelGame.builder`. | "overlay" is Flutter's `Overlay`, whose absence is the error message users see. Calling the HUD slot "overlay" makes that error harder to read. | yes (6 call sites) |
| `FynkelApp.source` | fynkel_app.dart:24 | document | "Where assets load from; the app bundle under `assets/` ([bundleSource]) unless given." | Lint: no doc. | no |
| `FynkelApp` pass-through options | fynkel_app.dart:15 | add counterpart | Add `resolutionScale`, `showErrors` and the pause option below, passed to the view and game. | Today one `FynkelView` option forces rewriting the app from the pieces. | no |
| `FynkelApp(make)` positional vs `FynkelGame(make:)` named | fynkel_app.dart:15; fynkel_game.dart:24 | keep (noted) | Either is fine; the inconsistency is small and `FynkelApp(Garden.new)` reads well. If touched, make both positional. | — | — |
| `FynkelGame.maybeOf` | fynkel_game.dart:38-39 | add counterpart + change shape | Add `static FynkelGameState<T> of<T extends GameObject>(BuildContext)`, which throws a `FlutterError` that says "no FynkelGame above this context; wrap ... in FynkelGame". Make `maybeOf` generic too: today it returns `FynkelGameState<GameObject>?`, so `.game` needs a cast. | Flutter convention: `of`/`maybeOf` come as a pair (Navigator, Theme, MediaQuery). | no |
| `FynkelGameState.restart` and leaving the page | fynkel_game.dart:54-57; fynkel_view.dart:285 | fix behaviour | Restart, and the view's dispose of its own world, destroy the root first (`onDisable`/`onDestroy` run, `isDestroyed` becomes true), then free the world. The core part is `World.dispose`. | Finding #7: the guide promises "Leaving the page destroys the game", but saving in `onDestroy` never runs. | no |
| `FynkelView({root, world, ...})` with an xor assert | fynkel_view.dart:121-134 | change shape | Use two constructors: `FynkelView({required GameObject root, source, ...})` and `FynkelView.world(World world, {onFrame, ...})`. `onFrame` only belongs to the second. | The shape should state which arguments go together; Flutter does this with `ListView` / `ListView.builder`. The release-mode assert disappears and both can then be null. | yes (tests/tools only) |
| `FynkelView.resolutionScale` | fynkel_view.dart:153 | document | "Pixels drawn per physical pixel, above 0 and at most 4; 1 unless given. Below 1 is faster and softer." | Lint: the field has no doc, and the range is only in an assert. | no |
| Key handling: `Focus.onKeyEvent` returns `handled` for every key | fynkel_view.dart:450-456; input_feed.dart:77-89 | fix behaviour | Still feed every key to `world.input`, but return `handled` only for keys Flutter would otherwise use for focus or activation (arrows, Tab, Space, Enter) and for keys the game has bound in an action or axis. Everything else (Escape, F-keys, shortcuts) returns `ignored`, so `Shortcuts`/`CallbackShortcuts` above the view see it. Add `bool Function(KeyEvent)? onKeyEvent` (Flutter's name) to override. While paused, return `ignored` for everything. | Finding #4: Escape-to-pause can't be an app shortcut today. The F5 special case (line 452) is the same problem patched once. | no |
| `FynkelView` ticker and `TickerMode` / lifecycle | fynkel_view.dart:183, 190, 289-296 | fix behaviour + document | The ticker is a `SingleTickerProviderStateMixin` ticker, so an opaque route pushed over the game silently pauses it, and a dialog does not. On resume, the first `dt` is the whole time away (capped at `time.maxDelta`, 1/3 s). Reset `_lastMicros` when `TickerMode.of` turns back on, and when the app resumes. Document "a route over the game pauses it, a dialog doesn't", and tie both to the pause below. | Finding #2, and the jump makes bodies and tweens leap. | no |
| `_fail` stops the ticker | fynkel_view.dart:396-407 | fix behaviour | A registration or plugin failure shows `ErrorWidget` but keeps the world ticking (no drawing). In a test environment, `FynkelView` falls back to `HeadlessView` with no channel, or the guide's §17 shows the channel mock. | Probably finding #25: a widget test without the mock gets `MissingPluginException` → `_fail` → the ticker stops → "never ticked". Needs a check, but the path is there. | no |
| `FynkelViewState` (public) | fynkel_view.dart:183-188 | keep (noted) | Fine as `GlobalKey` access (like `NavigatorState`). Better: also expose `world` from `FynkelGame.of(context)`, so a HUD doesn't need a key. | — | no |
| `FynkelView.onStats` and `onViewStats` | fynkel_view.dart:156, 163 | keep | Two rates (a per-second window and per frame) for two uses, both documented. | — | — |
| `bundleSource([AssetBundle?])` | fynkel_view.dart:80 | keep (noted) | Possibly add `{String prefix = 'assets/'}`. | Hard-coded prefix. | no |
| Colour bridge | (new) | add | In fynkel_flutter: `extension on fynkel.Color { ui.Color toFlutter() }` and `extension on ui.Color { fynkel.Color toFynkel() }`. | Finding #6: every themed game writes its palette twice. | no |
| Pause | (new) | add | See §6: `paused:` and `pauseWhen...` on `FynkelGame`/`FynkelView`. | Finding #2. | no |

`FynkelViewStats` (8 members), `FynkelGame.make/builder/restartOnReload`,
`FynkelGameState.game`, `FynkelView.root/source/world/onFrame/focusNode/autofocus/showErrors`
and `FynkelApp.title/restartOnReload` are keep. `autofocus = true` breaks
Flutter's default of false, but it's the right default for a game view and
is documented.

---

## 5. Cross-cutting observations (ranked)

1. **Angles: radians in core, degrees in physics.** Physics uses degrees in
   joints and `slopeLimit` while `Rigidbody.angularVelocity` and core use
   radians, so a joint and a body sit next to each other in different
   units. Pick one rule for every package and write it in the guide.
   Recommended: radians plus a `deg` constant.
2. **`position` means two things.** It's local on `GameObject`/`Behaviour`
   and world on `Rigidbody`. Unity's `transform.position` is world, so
   people from Unity carry over the wrong guess. At least document both
   ends. Core's reviewer should weigh whether `position` should be world
   (Unity) and `localPosition` local.
3. **Lifecycle hooks are public API.** `WorldSystem` hooks, including
   `dispose`, show on `world.physics`. Mark them `@protected` in core.
4. **Calling into the world from outside works but isn't documented** at
   the API level (finding #19). Put one line on `Physics` ("safe from Flutter
   callbacks: a call waits for the step") and one on `FynkelGame`.
5. **Hit types are named three ways** (`RayHit.object`, `PhysicsHit.gameObject`,
   Unity's `RaycastHit`). Align on `RaycastHit` and `gameObject`.
6. **Behaviour while not in the world.** Core's rule (for example
   `get` on a destroyed object) and physics' rule (throw) should match
   Unity's quiet no-op for state changes like wake and sleep, and throw only
   where an answer would be wrong.
7. **Docs**: physics has about 45 undocumented members and flutter about 5
   (excluding non-exported `ErrorPanel`/`InputFeed`). Turn on
   `public_member_api_docs` in both packages' analysis options once they're
   filled, so CI keeps them filled.
8. **Two Colors.** Core and Flutter each have a `Color`. Bridge them in
   fynkel_flutter rather than renaming core's.
9. **Game → UI messages.** `Signal` has no `Listenable` or `Stream` view
   (finding #5). A `signal.asListenable()`/`stream` in core, or an adapter in
   fynkel_flutter, would let `ValueListenableBuilder` and `StreamBuilder`
   work without hand-made notifiers.

## 6. Proposal: the Flutter-facing API (essentials, for the stage on Flutter and the game)

```dart
// The shortcut: a Material app with one page, the HUD inside a route.
runApp(FynkelApp(
  Arena.new,
  hud: (context, game) => Hud(game),        // was overlay:
  theme: ThemeData.dark(),                    // MaterialApp's
  pauseWhenInactive: true,                    // app lifecycle → pause
));

// The pieces, inside any app:
FynkelGame(
  make: Arena.new,
  paused: _menuOpen,                          // declarative, like TickerMode(enabled:)
  builder: (context, game) => Stack(children: [FynkelView(root: game), Hud(game)]),
);

// From any widget below it:
final scope = FynkelGame.of<Arena>(context);  // game, world, isPaused (ValueListenable<bool>),
scope.pause(); scope.resume(); scope.restart();

// A dialog that pauses the game while it's open:
await showGameDialog(context, builder: ...);  // = pause; showDialog; resume
```

- **Pause** means three things together: game time stops (`time.scale` is
  kept and restored, not overwritten), the game's input stops (pointers and
  keys go to Flutter, and `input` reports nothing held), and drawing goes on
  with the last frame (or the ticker stops, as an option). The sources: the
  `paused:` argument, `pause()`/`resume()`, the app lifecycle
  (`inactive`/`hidden` with `pauseWhenInactive`), and `TickerMode` off (a
  route over the game). The view resets its `dt` on resume.
- **Keys**: the game sees every key. Flutter sees every key the game hasn't
  bound, so Escape → pause menu is an ordinary `CallbackShortcuts`.
- **Themes into the game**: `Color.toFynkel()`/`toFlutter()`, and an
  optional `FynkelGame(theme: ...)` that puts `Theme.of(context)` on the
  world as a value the game can read (`world.get<ThemeData>`-style), so a
  game's palette follows the app's theme.
- **Game → UI**: a `Signal` gives a `Listenable`/`Stream` view (core), so
  "game over → open a dialog" is a `StreamBuilder` or a listener in the
  page, with no hand-written notifiers.
- **Ending**: leaving or restarting destroys the root (its `onDestroy`
  runs) before the world is freed.
- **Tests**: `FynkelView` in a widget test without the plugin runs headless
  and keeps ticking, and §17 shows a `testWidgets` of the whole app (finding
  #24).
