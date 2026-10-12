# API review: package:fynkel, the rest of the core

Scope: `src/math.dart`, `motion.dart`, `input.dart`, `keys.dart`, `pointing.dart`, `cameras.dart`,
`resources.dart`, `assets.dart`, `model.dart`, `lib/platform.dart` and `lib/hooks.dart`. All paths below are
relative to `packages/fynkel/lib/`. Line numbers are at `70c49469` (branch `stage6-plan`).

## Summary

**Verdict: good, with a few false friends to fix.** The surface hangs together. Names are consistent inside
each file, value types are immutable and `const`, and units are written down almost everywhere. The thrown
errors are among the best I have seen in a game framework: they say what to do. Examples: `tween` says
"(or write 0.0, not 0)", `axis2` lists the bound names, `Assets` says "make it with World(source: ...)", and
`Quat.lookRotation` says to give `target - eye`.

The real problems are a few places where a Unity user's guess compiles and does something else:

- **`translate`** moves in the parent's space. Unity's `Translate` moves in the object's own space by default.
- **`position` / `moveTo`** are local. Unity's `position` and DOTween's `DOMove` are world.
- **`Quat.euler`** takes radians, yet its doc cites Unity's `Quaternion.Euler`, which takes degrees.
- **`isDown`** means "held". Unity's `GetKeyDown` means "went down this frame".
- **`Plane.distance`** has the opposite sign to Unity's.
- **`Mesh.plane()`** is 1×1. Unity's Plane is 10×10.
- **`colorTo`** changes the shared material (#12).
- **`vibrato`** means two different things in `punchScale` and `shake`.

Two findings from the newcomers are missing features in this scope: Flutter controls can't feed axes and
actions (#3), and `Follow` has no target-relative offset (#22).

**Counts**

| Item | Count |
|---|---|
| Public members in scope | about 405 (each enum counted once) |
| Rows in the table below | 58 |
| Members those rows cover | about 75 |
| Judged "keep" | about 330 |
| Breaking proposals | 17 |
| Doc-only proposals | 19 |

`pointing.dart` has no public surface. Its object handlers live in `game_object.dart` and `components.dart`.

**Size (criterion 8)**

- **`motion.dart`, 1,126 lines.** The public surface is coherent: `Ease`, `Motion`, and one family of
  DOTween-like helpers that share the same options. About 470 lines are private machinery (`_Tween`, `_Group`,
  `seek`) and could move to their own part. Don't split the API.
- **`input.dart`, 1,033 lines.** The surface is coherent but mixes two audiences on one class: about 12
  methods that feed events (for the view and for tests) sit next to the polling and signals a game reads. Move
  the feeding methods behind one object (row I-6). `Finger`, `Swipe`, `Pinch` and `KeyPress` could move to
  their own part (`gestures.dart`), but that's optional.

## Proposals

Severity: **H**igh, **M**edium, **L**ow.

| # | Name | File:line | Verdict | Proposal | Why | Breaking? |
|---|---|---|---|---|---|---|
| M-1 | `translate(offset)` (on GameObject and Behaviour) | src/motion.dart:644 | fix behaviour (**H**) | Make it Unity's: `translate(Vec3 offset, {Space space = Space.self})`, with `enum Space { self, parent }` (Unity's `Space.Self` / `Space.World`; "parent" because `position` is parent-relative). Give `rotate` (:640) the same `space:` parameter; its default `self` already matches Unity. | Unity's `transform.Translate(v)` moves in the object's own frame. Here it moves in the parent's space, so `translate(Vec3.forward * speed * dt)` walks along the world's -Z whichever way the object faces. The guide's own `Walker` (writing-a-game.md:487–490) turns with `rotate` and then walks in world axes. Today `rotate` is in the own frame and `translate` in the parent's, so the two inconsistent defaults sit side by side. | yes (behaviour) |
| M-2 | `position` / `rotation` / `scale`, `moveTo` / `moveBy` / `rotateTo` / `scaleTo` | src/motion.dart:628–637, 828, 856, 884, 979 | document (**H**) | Keep the Godot-style naming (local `position`, plus `worldPosition`). Say in each doc comment "local, as Unity's `localPosition`; DOTween's `DOLocalMove`". Add a "Unity → Fynkel" note to guide §7. The alternative is Unity-exact `localPosition` + `position` = world. That is a huge break, and it's the user's call. | False friend. In Unity, `position` and `DOMove` are world space. They match here only for root objects, so the mistake shows up late, when something gets a parent. | no |
| M-3 | `worldRotation` (missing) | src/motion.dart:648 | add counterpart (**M**) | Add `Quat get worldRotation` with a setter next to `worldPosition`. `cameras.dart:125 _setWorldRotation` already implements the setter privately. Optionally add a read-only `worldScale`, Unity's `lossyScale`. | `worldPosition` has a setter but rotation has none. Today a game has to do the parent-inverse multiply itself, which the engine already does for its own rigs. | no |
| M-4 | `forward` / `right` / `up` (object, world space) vs `Transform.forward` (parent space) | src/motion.dart:655–661; src/components.dart:96 | merge (**M**) | Remove `Transform.forward`, or rename it `localForward`. `obj.forward` stays world. | Same name, two frames: `obj.forward` is world but `obj.transform.forward` is parent-relative. In Unity, `transform.forward` is world, so the second one is the false friend. (`Transform` belongs to the other reviewer; flagged here because the pair is split across files.) | yes (small) |
| M-5 | `transformDirection` / `inverseTransformDirection` (missing) | src/motion.dart:676–687 | add counterpart (**L**) | Add both: rotation only, no scale or position, as Unity's. | `transformPoint` exists. Turning a local direction (a muzzle's aim, a local velocity) into world space is the other everyday need, and today it is `worldTransform.rotation.rotate(v)`. | no |
| M-6 | `colorTo` | src/motion.dart:1008 | fix behaviour (**H**) | Default target: the object's own tint. Add a per-renderer `color` (or `tint`) to `MeshRenderer` that multiplies the material, as Unity's `MaterialPropertyBlock` / `renderer.material` do, and tween that. `material:` keeps the explicit shared case. Until then, the doc must say "changes the material, so every object sharing it changes". Today the doc doesn't say so. | Finding #12: a hit flash flashes every enemy. DOTween's `DOColor` on `renderer.material` touches only that renderer, because Unity instantiates the material. | yes (behaviour) |
| M-7 | Where motions run (the `Motion` class doc) | src/motion.dart:105–107 | fix behaviour (**M**) | Either step motions before the updates, so a rider's `onUpdate` sees this frame's platform position, or add DOTween's update type: `update: MotionUpdate.normal \| late \| fixed`, with `fixed` stepping in the physics step for moving platforms. | Finding #20: a platform moved by `moveTo` is a frame behind its rider. | yes (timing) if the default moves |
| M-8 | `vibrato` | src/motion.dart:1046 (`punchScale`, int, swings over the whole motion), 1079 (`shake`, double, per second); src/cameras.dart:361 (`Cameras.shake`, per second) | rename (**M**) | Keep `vibrato` for `punchScale`; it's DOTween's meaning, a count. Rename the two per-second ones to `frequency` (in Hz). | One word, two units. `vibrato: 6` gives 6 swings in total for a punch but 6 a second for a shake. | yes (named arg) |
| M-9 | `shake` (object) vs `Cameras.shake` | src/motion.dart:1076; src/cameras.dart:361 | change shape (**L**) | Align them. Both give `seconds` a default of 0.4 (it is `required` on the object and optional on the camera). Make `Cameras.shake` return something awaitable (a `Motion` or a `Future<void>`) as the object's does. | Same verb, different required parameters and return types. | yes (small) |
| M-10 | `lookAtOver` | src/motion.dart:951 | rename (**L**) | `rotateToLookAt(point, …)`, or `rotateToFace`. | Every other helper is `<verb>To` or `<verb>By` (`moveTo`, `rotateTo`, `scaleTo`, `colorTo`). "Over" is the only preposition of its kind and reads oddly. DOTween calls it `DOLookAt`. | yes (2 uses) |
| M-11 | `scaleBy` (missing) | src/motion.dart:979 | add counterpart (**L**) | `scaleBy(Vec3 factor, …)`, multiplying from the scale it has when it begins. | `moveTo`/`moveBy` and `rotateTo`/`rotateBy` come in pairs; `scaleTo` has no `By`. | no |
| M-12 | `Motion` pause/resume (missing) | src/motion.dart:108 | add (**L**) | `pause()`, `resume()` and `isPaused`, as DOTween's `Pause`/`Play`. Today a motion holds still only while its owner is hidden or disabled. | A pause menu that freezes one door but not the HUD tween has no handle today, short of `unscaled` tricks. | no |
| M-13 | `Ease.*` (31 functions) | src/motion.dart:22–89 | document (**L**) | Give each one a one-line doc, or link to easings.net for the shapes. | No doc comment of their own. The class doc covers the families, so this is low priority. | no |
| M-14 | Guide: "Each takes `seconds:` or `speed:`" | docs/writing-a-game.md:357 | document (**L**) | Say that `colorTo`, `punchScale` and `shake` take `seconds:` only (it's `required`). | The guide claims all helpers take either; three don't. | no |
| Q-1 | `Quat.euler(pitch, yaw, roll)` | src/math.dart:138 | document + add (**H**) | Keep radians, the Dart convention, but make it impossible to miss: "radians, **unlike** Unity's `Quaternion.Euler`, which takes degrees". Add the Mathf constants `deg2Rad` and `rad2Deg` (or a `double radians(double degrees)` helper) so `Quat.euler(0, 90 * deg2Rad, 0)` reads naturally. | The doc points readers at Unity's `Euler`, which takes degrees. `Quat.euler(0, 90, 0)` compiles and spins about 14 turns. Physics joints and `slopeLimit` take degrees (as Unity does), so a newcomer meets both units. | no |
| Q-2 | `Quat` counterparts | src/math.dart:114 | add (**M**) | `Vec3 get eulerAngles` (read the angles back, radians), `static Quat rotateTowards(Quat from, Quat to, double maxRadians)` (as `Vec3.moveTowards`), and `factory Quat.fromToRotation(Vec3 from, Vec3 to)`. | They're Unity's everyday quaternion helpers. "Turn toward at most N rad/s" is otherwise a hand-rolled `slerp` plus `angleTo`. | no |
| Q-3 | `Vec2` counterparts | src/math.dart:5 | add counterpart (**L**) | `Vec2.normalized` (throwing as `Vec3`'s does) and `closeTo`. Both exist on `Vec3`. | Joystick and swipe code normalises `Vec2`s; today it has to be done by hand. | no |
| Q-4 | `Vec3` helpers (missing) | src/math.dart:47 | add (**L**) | `distanceTo`, `lengthSquared` (Unity's `sqrMagnitude`), `angleTo` (radians) and `projectOnPlane`. | The most-used Vector3 statics after `lerp` and `moveTowards`. | no |
| Q-5 | `Vec3.normalized` error | src/math.dart:82 | fix error (**L**) | Callers pass the name. `Quat.axisAngle` throws "axis is zero: give a direction", and `Ray`/`Plane` say "direction" or "normal". | `ArgumentError('Vec3(0.0, 0.0, 0.0) has no direction')` surfaces from deep inside `rotateBy`, `Ray(...)` or `Plane(...)` without saying which argument. | no |
| Q-6 | `Plane.distance` | src/math.dart:302 | document (**M**) | Add: "Unity's `Plane.distance` has the opposite sign (`n·p + d = 0`)". | False friend: same name, opposite sign. `distanceTo` agrees with Unity's `GetDistanceToPoint`, so only the field differs. | no |
| Q-7 | `ViewPoint` class doc | src/math.dart:328 | document (**L**) | Remove "package:fynkel_flutter's `FrameInput`". There is no `FrameInput`; say `Finger.position` and `Input.mousePosition` instead. | Stale reference to a class that doesn't exist. | no |
| Q-8 | `randomInSphere` / `randomOnCircle` | src/math.dart:383, 392 | add counterpart (**L**) | Add `randomOnSphere` and `randomInCircle`, so there are all four of Unity's `insideUnitSphere`, `onUnitSphere`, `insideUnitCircle` and `onUnitCircle`. | One "in" and one "on" with different shapes: a guesser who writes `randomInCircle` finds nothing. Unity's best-known one is *inside* the circle. | no |
| Q-9 | `SmoothDamp3` | src/math.dart:443–469 | change shape (**M**) | Make it match `SmoothDamp`: a public `double maxSpeed` (getter and setter; today the private `_maxSpeed` can't be read or changed after construction), and a `velocity` setter to reset it. | Its doc refers to "`maxSpeed`" in backticks because it isn't a member. The two classes are siblings with different shapes. | no |
| Q-10 | `dot`, `cross`, `length`, operators on `Vec2`/`Vec3`/`Quat`; `Quat.length`, `Quat.dot` | src/math.dart:14–21, 65–76, 201, 228 | document (**L**) | One-liners. `Quat.length` especially: "1 for a rotation; see `normalized`". | No docs. Self-evident to most readers, but criterion 5 asks for them. | no |
| I-1 | `Input.isDown(button)`, `InputAction.isDown` | src/input.dart:158, 961 | rename (**H**) | `isPressed`, the Input System's `isPressed`, pairing with the existing `wasPressed`/`wasReleased`. `Finger.isDown` stays: a finger *is* down. | `isDown` means "held". Unity's `GetKeyDown`/`GetMouseButtonDown` mean "went down this frame", so a Unity user's `if (input.isDown(KeyCode.space)) jump()` fires every frame. It also mixes "down" and "pressed" in one trio. | yes (about 18 uses) |
| I-2 | Axes and actions fed from Flutter (missing) | src/input.dart:195–227 | add (**H**) | `InputAction.press()` / `release()` (or `bool held` settable), counting as one more button. Add `Input.setAxis2(String name, Vec2? value)`, a virtual value combined with the keys and clamped to length 1, cleared with `null`. Optionally a `JoystickArea`/`OnScreenButton` in fynkel_flutter, as Unity's `OnScreenStick`/`OnScreenButton`. | Finding #3: a Flutter joystick or button can't drive `axis2('move')` or `action('jump')`, so games read two paths. | no |
| I-3 | `axis2` returns a value, `action` returns an object | src/input.dart:195, 221 | document (**L**) | Keep both shapes. Say in `axis2`'s doc that it is the "2D Vector composite" of Unity's Input System, and that there is no 1D axis: use `axis2('move').x`. | Two shapes for "named control"; a newcomer looks for `axis('horizontal')`. If I-2 is done as an object (`InputAxis2`), the two can merge instead. | no |
| I-4 | `scroll` | src/input.dart:187 | document (**M**) | Add "**opposite** to Unity's `mouseScrollDelta` (wheel up is +y there)". Better, flip it to Unity's sign, which also matches `axis2`'s y-up; `Orbit` would flip with it. | False friend: the doc gives Flutter's sign without saying Unity's differs. | yes, if flipped |
| I-5 | `Pinch.centre` | src/input.dart:923 | rename (**M**) | `center`. | The only British spelling in a public identifier. The API says `color`, and Flutter and Dart say `center` (and Flutter's scale gesture calls it `focalPoint`). | yes (few uses) |
| I-6 | Feeding methods on `Input`: `resize`, `fingerDown`, `fingerMove`, `fingerUp`, `fingerCancel`, `hover`, `leave`, `scrollBy`, `trackpad`, `keyDown`, `keyUp`, `releaseKeys` | src/input.dart:249–298 | change shape (**M**) | Move them, names unchanged, behind one object, `input.simulate` (or `input.feed`), as Unity's `InputTestFixture`. `Input` itself is then what the game reads: signals, polling, axes, actions, settings. | 12 of `Input`'s ~48 members are for the view and for tests. `input.keyDown(KeyCode.a)` sits next to `onKeyDown` and `isDown` and reads like a query. Autocomplete on `input.` in a game shows injectors first. | yes (tests, about 80 call sites, mechanical) |
| I-7 | `leave(id)` | src/input.dart:284 | rename (**L**) | `exit(id)`. | Flutter's event is `PointerExitEvent` (the view calls `_input.exit` → `leave`), and objects get `onPointerExit`. Three words for one event. | yes (few) |
| I-8 | `keyDown` / `keyUp` | src/input.dart:293–294 | document (**L**) | "Key [key] went down (up); queued for the next frame. Repeats are the caller's to drop." | No doc. | no |
| I-9 | `Finger.kind`, `Finger.isDown`, `Swipe.finger`, `KeyPress.key` / `shift` / `control` / `alt`, `InputAction.name` / `buttons` / `isDown`, `FingerKind` and its values, `SwipeSide` | src/input.dart:712, 842, 887, 940–943, 958–961, 683, 908 | document (**L**) | One-liners. For `SwipeSide`, say "up is towards the view's top". | No doc comments. | no |
| I-10 | `Finger.isTap` | src/input.dart:846 | document (**L**) | Split the doc into two sentences, "while down: …" and "after up: …", or add `didTap`. | One getter with two meanings depending on the phase. | no |
| K-1 | `KeyCode` values | src/keys.dart:3–8 | document (**L**) | Say the value names are Flutter's (W3C's) `PhysicalKeyboardKey` names (`shiftLeft`, `arrowUp`, `bracketLeft`), not Unity's (`LeftShift`, `UpArrow`, `LeftBracket`). | The doc says "as Unity's Input System's `Key`" and a reader then guesses `KeyCode.leftShift`. Keeping Flutter's names is right for Dart users; just say so. | no |
| C-1 | `Follow` offset space | src/cameras.dart:133–139 | add (**M**) | `Follow(target, offset:, space: FollowSpace.world \| target \| targetYaw)`: Cinemachine's binding modes (`WorldSpace`, `LockToTarget`, `LockToTargetWithWorldUp`). `targetYaw` is the third-person camera. | Finding #22: behind-the-player cameras can't be made with `Follow`. Cinemachine's default is target-relative. | no |
| C-2 | `Orbit.pitch` | src/cameras.dart:238 | document (**L**) | Add "positive is above the target, looking down, which is a *negative* pitch in `Quat.euler`". Or rename it `elevation`. | Same word, opposite sign within one package. `Quat.euler`'s positive pitch "looks up". | no (doc) |
| C-3 | `Orbit.minPitch`, `maxPitch`, `minDistance`, `maxDistance` | src/cameras.dart:244 | document (**L**) | Units (radians, metres) and that `_move` clamps to them each frame. | No docs and no units. | no |
| C-4 | `Orbit.controls` | src/cameras.dart:247, 259–276 | fix behaviour (**L**) | Respond to drag, pinch and scroll only while the object's `VirtualCamera` is live (or it has none). | Two orbit cameras both turn on one drag, including the one not shown. | yes (behaviour, small) |
| C-5 | `Follow.target`, `LookAt.target`, `Orbit.target`, `Blend.seconds`, `Blend.ease` | src/cameras.dart:136, 177, 230, 18–19 | document (**L**) | One-liners. Say what happens when the target is destroyed (the rig holds still). | No docs. | no |
| C-6 | `LookAt` up vector | src/cameras.dart:174 | add (**L**) | `worldUp:` (default `Vec3.up`), as `lookAt(point, worldUp:)` has. | The rig and the method should take the same parameters. | no |
| R-1 | `Mesh.plane()` | src/resources.dart:42–44 | document (**M**) | Add "1×1: Unity's Plane primitive is 10×10. This one is Unity's Quad, facing up." | False friend: the size is wrong by 10×, so a ground built from Unity habits is tiny. | no |
| R-2 | `Mesh.capsule()`, `Mesh.cylinder()`, `Mesh.quad()` (missing) | src/resources.dart:34–48 | add (**M**) | Unity's primitives, at Unity's sizes. Capsule: radius 0.5, height 2, matching `CapsuleCollider`'s and the character controller's defaults. Cylinder: radius 0.5, height 2. Quad: 1×1, facing +Z. | Finding #13: physics has capsule colliders and a capsule character with nothing to draw them. | no |
| R-3 | `Material.copy()` (missing) | src/resources.dart:334 | add (**H**) | `Material copy({String? name})`, Unity's `new Material(source)`: same values and textures, not shared. The guide says when to copy (a tint that must differ per object, if M-6's per-renderer tint isn't done). | Finding #12. Today the only way to get a second material is to repeat its 15 constructor arguments. | no |
| R-4 | `Material.dispose` message | src/resources.dart:393 | document (**L**) | "destroy the object", not "despawn the object". | "Despawn" appears nowhere else in the public API; the word is `destroy`. | no |
| R-5 | `isDisposed` on `Mesh`, `Texture2D`, `Material`, `Model` | src/resources.dart:32, 305, 388; src/model.dart:95 | document (**L**) | "Whether [dispose] was called; a disposed X can't be used in a world." | No docs. | no |
| R-6 | `Mesh.raw(positions, …)` | src/resources.dart:111 | change shape (**L**) | `required Float32List positions` as a named parameter, as `Mesh.custom` has. | The sibling constructors take the same thing two ways. | yes (7 uses) |
| R-7 | `Mesh` / `Texture2D` name (missing) | src/resources.dart:12, 230 | add counterpart (**L**) | `String? name`, as `Material.name` (used in `toString` and in messages). | Unity's `Object.name` is on all three. A model's meshes are just "Model mesh 3" in errors. | no |
| R-8 | `Texture2D` sampling (missing) | src/resources.dart:230 | add (**L**) | `wrapMode` (repeat/clamp/mirror) and `filterMode` (point/bilinear/trilinear), if the engine can. | Pixel-art and UI textures need point filtering; tiling needs a wrap mode. Unity has both on every texture. | no |
| A-1 | `Assets.model(name)` | src/assets.dart:30 | rename (**M**) | `instantiate(name)` (Addressables' `InstantiateAsync`; `Model.instantiate` already exists). That frees `model(name)` for a later `Future<Model>`, the shared asset itself, to read `warnings`/`materials`. | `assets.texture` returns the asset; `assets.model` returns a *copy* as a `GameObject`, not a `Model`. Same shape of name, different kind of result. | yes (about 15 uses) |
| A-2 | `Model.load` doc | src/model.dart:42–45 | document (**L**) | Point first to `world.assets.instantiate` / `assets.model`, which are shared and counted. Keep `rootBundle` as the manual route. | Two ways to load a model. The doc steers to the one that doesn't share or free. | no |
| H-1 | `rustTriple` | hooks.dart:87 | make internal (**L**) | `_rustTriple`. | Used only inside `hooks.dart`, yet public in a library other packages import. | yes (none outside) |
| H-2 | `HookError` | hooks.dart:138 | rename (**L**) | `HookException`. | Effective Dart: a class that `implements Exception` isn't an `Error`. Builds failing for a missing NDK are runtime conditions, not bugs. | yes (internal users) |
| H-3 | `hooks.dart` library | hooks.dart:1 | document (**L**) | A `library;` doc: "for Fynkel's own packages' build hooks (fynkel, fynkel_physics); not for games". | The only doc is a `//` comment, so dartdoc shows nothing. | no |
| X-1 | Object handlers vs `Input` signals | src/input.dart:89–115 vs game_object.dart/components.dart `onClick`, `onPointerDown`, `onPointerExit` | rename or document (**M**) | Pick one word per event. Either rename the object's `onClick` → `onTap` (the input's word) and keep `onPointer*`, or document the mapping in one table (`onTap` ↔ `onClick`, `onFingerDown` ↔ `onPointerDown`, `leave` ↔ `onPointerExit`). | Same event, two vocabularies: Lean Touch's on `Input`, Unity's EventSystem on objects. Unity itself says `OnPointerClick`, not `OnClick`. Coordinate with the core reviewer. | maybe |
| X-2 | `Finger.over` / `startedOver` return `RayHit`; physics queries return `PhysicsHit` | src/input.dart:862–875; fynkel_physics/lib/src/queries.dart | document (**L**) | Say in the guide which hit you get where. Or merge into one `RaycastHit` shape (`object`, `point`, `normal`, `distance`) that the physics hit extends. | Two hit types for one idea across packages; Unity has one, `RaycastHit`. | maybe |

## Judged "keep"

About **330** members: every `Vec2`/`Vec3`/`Quat`/`Ray`/`Plane`/`ViewPoint` member not listed above, the
Mathf top-level functions, `SmoothDamp`, `EaseFunction`, `Motion`'s state and `Future` members, `tween`,
`sequence`, `together`, `moveTo`/`moveBy`/`rotateTo`/`rotateBy`, `punchScale`, `rotate`, `lookAt`,
`transformPoint`/`inverseTransformPoint`, the rest of `Input` (signals, `fingers`, `wasPressed`,
`wasReleased`, `mousePosition`, `bindAxis2`, `action`, `bindAction`, `settings`, `layers`, `width`/`height`,
`update`), `InputSettings`, `MouseButton`, the rest of `Finger`/`Swipe`/`Pinch`/`KeyPress`/`InputAction`, the
`KeyCode` enum itself, `VirtualCamera`, `Blend`, `Cameras`, `Follow`/`LookAt`/`Orbit`'s other fields, the rest
of `Mesh`/`Texture2D`/`Material`/`Model`, `AssetSource`, `Assets.texture`/`preload`, `buildRustCrate`, and
`platform.dart`.

Kept deliberately:

- **`Motion`, not DOTween's `Tween`:** `Tween` would clash with Flutter's `Tween<T>`.
- **`KeyCode`, not `Key`:** Flutter has a `Key`.
- **`Texture2D`:** Flutter has a `Texture` widget, and `Texture2D` is Unity's name anyway.
- **`Quat.axisAngle(axis, angle)`:** Unity's argument order is the other way round, but this order is the
  same in `rotate`, `rotateBy` and `axisAngle`.
- **`Vec3.normalized` throws on zero (Unity returns zero):** deliberate and documented.
- **`ViewPoint` y-down from top-left (Unity's screen is y-up from bottom-left):** Flutter's convention,
  documented.

## Cross-cutting observations (ranked)

1. **Local vs world is where a Unity user's guesses go wrong** (M-1, M-2, M-3, M-4). `position`, `moveTo` and
   `Transform.forward` are parent-relative, while Unity's `position`, `DOMove` and `transform.forward` are
   world. `translate` uses the parent's space where Unity uses the object's own. `worldRotation` is missing.
   Each is defensible alone, but together they're the biggest source of silent bugs in this scope. Fix
   `translate` (behaviour) and add `worldRotation`; document the rest in Unity's terms.

2. **Angles: radians in the core, degrees elsewhere, and no bridge** (Q-1).
   - Radians: `Quat.euler`, `axisAngle`, `rotate`, `rotateBy`, `Orbit.yaw`/`pitch`, `Pinch.twist`.
   - Degrees: `Camera.fovY` and `VirtualCamera.fovY` (here), and in fynkel_physics the joints' limits, motors
     and springs, plus `CharacterController.slopeLimit`.

   Unity is degrees throughout. Either rule works, but write it down once in the guide ("rotations in
   radians; field of view and physics limits in degrees, as in Unity's inspector"), add `deg2Rad`/`rad2Deg`,
   and fix `Quat.euler`'s doc, which cites a degrees API.

3. **Shared materials have no per-object way out** (M-6, R-3; finding #12). `colorTo` silently recolours
   everything that shares the material, and there's no `Material.copy()`. Add `copy()` at minimum, and
   ideally a per-renderer tint that `colorTo` uses by default.

4. **Input is keyboard-first** (I-1, I-2, I-4; finding #3).
   - Flutter widgets can't feed actions or axes.
   - `isDown` reads like Unity's `GetKeyDown`.
   - `scroll` has the opposite sign to Unity's.

   The touch side (fingers, gestures, signals) is strong and needs no changes.

5. **Same word, different meaning:**
   - `vibrato` (M-8)
   - `shake` (M-9)
   - `forward` (M-4)
   - `pitch` (C-2)
   - `Down` (I-1)
   - `Plane.distance` vs Unity (Q-6)
   - `Mesh.plane` vs Unity (R-1)

6. **One event, several vocabularies across files** (X-1, I-7). Input says finger, tap and leave; objects say
   pointer, click and exit; Flutter says pointer and exit.

7. **Name clashes with Flutter.** `Material` (here) and `Transform` and `Color` (core reviewer's files)
   collide with `package:flutter/material.dart` and `widgets.dart`. Every example imports
   `flutter/widgets.dart show runApp` to get around it. Keep the Unity names (they're right), but give the
   guide a short "imports" note with the `hide`/`show` pattern, since HUD files import both packages.
   Elsewhere the API already avoids clashes on purpose (`KeyCode`, `Texture2D`, `Motion`).

8. **Units vocabulary.** Most docs say "units" (SmoothDamp, tweens' `speed`), but `Orbit.distance` and
   `Cameras.shake` say "metres". Pick one ("metres", since physics uses newtons and kilograms). `seconds` is a
   named parameter everywhere except `Blend(seconds)`; that's fine for a `const` value class.

9. **Doc gaps are small and mechanical.** There are about 40 undocumented members (I-8, I-9, C-3, C-5, R-5,
   Q-10, M-13) and three doc bugs: the stale `FrameInput` (Q-7), the guide's `seconds:`/`speed:` claim
   (M-14), and "despawn" (R-4).

10. **Errors sampled are good.** `tween` and `_builtIn`, `axis2`/`action` (lists the bound names), `Assets`
    with no source, a motion on a destroyed object, the group rules, `Material`'s ranges, `Mesh.raw`'s
    lengths, `Input.update` on a world's input, and `Quat.lookRotation` all say what to do. The one weak spot
    is `Vec3.normalized`'s "has no direction" surfacing from a caller without naming the argument (Q-5).

## Top 10 proposals

1. **M-1:** `translate` defaults to the object's own space (`Space.self`), as Unity's does. Breaking
   (behaviour).
2. **I-1:** `isDown` → `isPressed` on `Input` and `InputAction`. Breaking.
3. **I-2:** `InputAction.press()`/`release()` and `Input.setAxis2`, so Flutter controls drive the same input
   (#3).
4. **M-6 + R-3:** `colorTo` tints only its own object by default; add `Material.copy()` (#12).
5. **Q-1:** state the radians rule plainly on `Quat.euler` (Unity's `Euler` takes degrees) and add
   `deg2Rad`/`rad2Deg`; write the core-radians / physics-degrees rule in the guide.
6. **M-2 + M-3 + M-4:** document `position`/`moveTo` as Unity's *local* ones, add `worldRotation`, and remove
   or rename `Transform.forward`.
7. **C-1:** `Follow(space: world | target | targetYaw)` (#22).
8. **M-8:** `vibrato` → `frequency` on `shake` and `Cameras.shake`; `punchScale` keeps `vibrato` as a count.
9. **R-1 + R-2:** document `Mesh.plane()` as 1×1 (Unity's is 10×10) and add `Mesh.capsule()`, `cylinder()` and
   `quad()` (#13).
10. **A-1:** `assets.model(name)` → `assets.instantiate(name)`, since it returns a copy, not a `Model`.
