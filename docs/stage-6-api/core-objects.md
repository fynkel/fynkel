# API review: package:fynkel core objects

Scope: `packages/fynkel/lib` — game_object.dart, components.dart, world.dart,
world_objects.dart, world_loop.dart, world_resources.dart, systems.dart,
time.dart, routines.dart, signals.dart, layers.dart, debug.dart, error.dart,
color.dart, view.dart and lib/extend.dart (also what lib/fynkel.dart and
lib/platform.dart export from them). world_objects/world_loop/world_resources
turned out to have no public API (only the private extensions `_Objects`,
`_Loop` and `_Resources`), so they matter only for how the public calls behave.

## Summary

**Verdict: sound, with a few fixes.** The core API is small, consistent and
well documented. Each way an object or component ends has its own verb,
used the same way everywhere: `destroy`/`isDestroyed` for objects,
`removeComponent`/`isRemoved` for components, `dispose`/`isDisposed` for
worlds and views, `stop`/`isStopped` for routines and motions,
`cancel`/`isCancelled` for subscriptions. The Unity names work as people
guess them (`onUpdate`, `enabled`, `activeInHierarchy`, `getInParent`,
`cullingMask`, `LayerMask`, `fixedDelta`). The problems fall into four groups:

1. **Two behaviour bugs that are part of the API's promise**: `World.dispose`
   runs no `onDisable`/`onDestroy` (finding #7), and `world.all/first/only`
   count hidden objects, unlike Unity's `FindObjectsByType`, so pooling breaks
   wave logic (#8).
2. **Things exported by mistake**: the systems-only extension
   `GameObjectSystems` (`transformStamp`, `componentCount`, `componentAt`) is
   exported to every game through `fynkel.dart`, and `World.pointer` is
   public.
3. **Common words taken, and names that mislead** (#21): `start` (pairs
   visually with `onStart` but is unrelated), the top-level `all` (a
   different idea from `World.all`), `until`/`whileTrue` (Unity says
   `WaitUntil`/`WaitWhile`), `id`, and three camera-only methods sitting on
   every `GameObject`.
4. **Unity false friends that aren't documented**: `parent =` keeps the
   *local* transform (Unity keeps the world pose); `MeshRenderer.material`
   is Unity's `sharedMaterial`; `find` searches every descendant (Unity's
   `Transform.Find` searches children only, by path); `position`/`rotation`
   are local (Unity's are world).

Counts: about **321 public members** reviewed (classes, constructors,
fields, getters/setters, methods, enum values, top-level functions). The
table has **52 rows covering about 80 members**: 11 rename, 1 remove,
3 make internal, 6 change shape, 1 fix behaviour, 10 add counterpart, and
20 document-only rows (the `public_member_api_docs` lint gives 50 hits in
these files; all are covered below).

## Table: everything that isn't a plain "keep"

Priority: **H** do in stage 6, **M** worth it, **L** only if that area is touched anyway.

| Name | File:line | Verdict | Proposal | Why | Breaking? |
|---|---|---|---|---|---|
| `World.dispose` | world.dart:85 | fix behaviour | Before freeing, destroy every root as `destroy()` does (onDisable, onDestroy, routines stopped, `isDestroyed` true), then free. Doc: "Destroys every object, then frees the world." **H** | #7: leaving the page or `restart()` runs no `onDestroy`; saving and cleanup in `onDestroy` never run, and the guide (§15) says leaving "destroys the game". Unity runs `OnDisable`/`OnDestroy` when a scene unloads. | Behaviour only (more calls run) |
| `World.all<T>()` | world.dart:435 | change shape | `List<T> all<T>({bool includeInactive = false})`: by default only objects that are `activeInHierarchy` (and components on them). Doc that destroyed objects drop out at once. **H** | #8: pooled (hidden) objects still count, so the guide's own `until(() => world.all<Crate>().isEmpty)` never ends once you pool. Unity's `FindObjectsByType` leaves out inactive objects unless you ask (`FindObjectsInactive.Include`). Today "live" in the doc hides this. | Yes (semantics) |
| `World.first<T>()`, `World.only<T>()` | world.dart:462, 444 | change shape + document | Same `includeInactive` flag as `all`. `first`: either keep add order across subclasses or document "the first of the first class added: objects are grouped by class". **M** | They should match `all`. Today `first<Enemy>()` returns the first `Grunt` even when a `Boss` was added before it, because results are grouped by class (`_byClass`). The doc says "the first … that the game added". | Yes (semantics) |
| `GameObjectSystems` (`transformStamp`, `componentCount`, `componentAt`) | systems.dart:58; fynkel.dart:9 | make internal | Add `GameObjectSystems` to the `hide` list in fynkel.dart; keep it in extend.dart only. **H** | It is exported to every game by mistake: fynkel.dart hides `WorldSystem`, `WorldSystems` and `SendTransforms`, but not this one. That puts 3 more names on every object, and `componentAt` returns removed components. | No for games (physics imports extend.dart) |
| `World.pointer` | world.dart:76 | make internal | Move it into a hidden extension (as `SendTransforms`, world.dart:523), e.g. `extension WorldNative on World { Pointer<FyWorld> get pointer }`, hidden in fynkel.dart. **H** | It hands a raw FFI pointer to games, and its type `FyWorld` isn't even exported. Only view.dart (same package) uses it. | No (no outside users) |
| `WorldView.address` | view.dart:104 | make internal | Move to an extension exported only from platform.dart. **L** | Plugin-only ("for the platform plugins' calls"), but it's on `HeadlessView` too, through fynkel.dart. | No |
| `GameObject.start`, `Behaviour.start` | game_object.dart:218, components.dart:359 | rename | `Routine<T> startRoutine<T>(Future<T> Function() body)` **H** | (a) `start` takes a very common game verb (`void start()` for a round or a timer doesn't compile in any subclass; #21). (b) Next to `onStart` it looks like the pair `update`/`onUpdate`, but it's unrelated. (c) Unity's name is `StartCoroutine`. | Yes |
| (new) `stopAllRoutines()` | game_object.dart:218, components.dart:359 | add counterpart | `void stopAllRoutines()` on GameObject and Behaviour (Unity's `StopAllCoroutines`). **M** | There's no way to stop an object's routines without keeping each `Routine`. #8: a pooled object's routines pause and later resume stale; `onDisable() => stopAllRoutines()` is the fix Unity users reach for. | No |
| top-level `all` | routines.dart:313 | rename | `Future<List<T>> waitAll<T>(Iterable<Future<T>> futures)` **H** | It's the same word as `World.all<T>()` for a different idea (a query vs. a join). It is a global name in every file that imports fynkel, so a local `all` hides it (#21), and Dart already has `Future.wait`. | Yes |
| `until` | routines.dart:268 | rename | `waitUntil(bool Function() test)` **H** | Unity's `WaitUntil`. It's a common word in the global namespace, and it reads as a wait only with `await` in front. | Yes |
| `whileTrue` | routines.dart:272 | rename | `waitWhile(bool Function() test)` **H** | Unity's `WaitWhile`. `whileTrue` doesn't say that it waits, and it's no one's existing name. | Yes |
| `fixedStep` | routines.dart:255 | rename | `nextFixedStep` **L** | It pairs with `nextFrame`. `fixedStep` reads as a value (a step's length?) next to `Time.fixedDelta`/`inFixedStep`. | Yes |
| `Routine.done` | routines.dart:105 | remove | Drop it; `await routine` already works (Routine implements Future). **M** | It's two ways to do one thing. `Motion` (also a Future) has no `done`, and only tests use it. | Yes (tests only) |
| (new) `Routine.isCompleted` | routines.dart:97 | add counterpart | `bool get isCompleted` (returned or failed, not stopped), as `Motion.isCompleted` (motion.dart:169). **L** | Routine and Motion are the same kind of thing with different state getters. | No |
| `GameObject.on`, `Behaviour.on` | game_object.dart:226, components.dart:363 | rename (optional) | `listen(Signal<T> signal, void Function(T) handler)` **L** | `on(...)` looks like one more `on…` hook, and takes a short word (`bool on` for a lamp or switch, #21). `listen` matches `Signal.listen`: owned vs. unowned is told by who calls it. Do it only if `start` is renamed in the same pass. | Yes |
| `GameObject.id` | game_object.dart:127 | rename | `instanceId` (Unity's `GetInstanceID`) **M** | `id` is the word games want for their own ids (save, network, level). Only systems need the engine id (physics.dart:516). | Yes (physics + tests) |
| `GameObject.screenToRay`, `screenToPlane`, `worldToScreen` | game_object.dart:580, 604, 615 | change shape | Move them to `Camera`: `Camera.screenPointToRay(ViewPoint)`, `Camera.screenPointToPlane(ViewPoint, Plane)`, `Camera.worldToScreenPoint(Vec3, {width, height})` (Unity's names, on Unity's class). **M** | Three methods on every object (and so in every subclass's namespace) that throw `FynkelException(noComponent)` unless the object has a Camera. Unity has them on `Camera`. | Yes (~50 call sites) |
| `World.camera` | world.dart:242 | change shape + document | Return `Camera?` (Unity's `Camera.main` is a Camera); `.gameObject` gives the object. Doc: "the shown object (activeInHierarchy) with a Camera and the lowest id". **M** | It goes with the row above: `world.camera?.screenPointToRay(p)`. The doc doesn't say that hidden cameras are skipped (hierarchy.rs:216). | Yes |
| `GameObject.parent` (setter) | game_object.dart:431 | fix behaviour / change shape | Match Unity: `parent = p` keeps the **world** pose, and add `void setParent(GameObject? parent, {bool worldPositionStays = true})` for the local case. Throw `ArgumentError` (not `FynkelException(invalidValue)`) for another world's object or a cycle. **M** | False friend: Unity's `transform.parent = x` and `SetParent(x)` keep the world position, so a picked-up item jumps when a Unity user sets it here. Today's behaviour is Unity's `SetParent(x, false)`. Mixed error types: `add` throws StateError for the same mistakes. | Yes (behaviour) |
| `GameObject.find` | game_object.dart:549 | document | Say "Unlike Unity's `Transform.Find` (direct children, `'a/b'` paths), this searches every descendant, depth first." Optionally accept `'a/b'` paths. **L** | False friend. | No |
| `GameObject()` | game_object.dart:37 | add counterpart + document | Add `bool active = true`; mention `layer` in the doc. **L** | `active` can be set before adding (it's in `_Description`) but not in the one-expression form that `name`, `transform` and `layer` have. | No |
| (new) `getAllInChildren<T>()` | game_object.dart:759 (and Component:56) | add counterpart | `List<T> getAllInChildren<T extends Object>()`, self first, depth first (Unity's `GetComponentsInChildren`). **M** | `getAll` only covers the object itself. Every model or rig game needs "all renderers / colliders below" and writes the walk by hand. | No |
| `GameObject.isPartOf` | game_object.dart:453 | document | Add "(Unity's `Transform.IsChildOf`, which is also true for itself)". **L** | The better name is fine; Unity users need the mapping. | No |
| `RayHit.object` | world.dart:505 | rename + document | `gameObject` **M** | `PhysicsHit.gameObject`, `Collision.gameObject`, `Component.gameObject` and `ControllerColliderHit.gameObject` all say `gameObject`; this one says `object`. It also has no doc. | Yes (`Finger.over?.object` users) |
| `ObjectError.object` | world.dart:536 | rename + document | `gameObject`; doc: "the object whose code threw, whose load failed, or that was being destroyed; null for the world's own (a system, a freed asset)". **L** | The same word for the same idea. The current doc only lists load and destroy, but update/handler/routine errors come here too. | Yes (small) |
| `ObjectError` (class doc), `error`, `stackTrace` | world.dart:529, 537, 538 | document | The class doc says "from an object's load or from freeing what it held". Say: anything `World.errors` carries (see its doc). Document both fields. **M** | Wrong today: most errors come from `onUpdate` and the other callbacks. | No |
| `World.isDisposed` | world.dart:81 | document | "Whether [dispose] was called; then every call but this one throws a StateError." **L** | No doc. | No |
| `World.errors` | world.dart:148 | document | Add that a failed `onLoad` destroys the object (it's there) **and** that a behaviour's failed load on an appeared object only removes the behaviour, plus one line on tests: "with no listener a test fails". **L** | #23, #26. | No |
| `MeshRenderer.material` | components.dart:265 | document + add counterpart | Doc: "Shared: changing the material changes every object that uses it (Unity's `sharedMaterial`, not `material`, which Unity copies per renderer)." Add a per-renderer override, e.g. `MeshRenderer.color` (tint, null = the material's), or `Material.copy()`. **M** | False friend + #12: `colorTo` / `material.color =` flashes every enemy. | No |
| `MeshRenderer()`, `.mesh` | components.dart:257, 262 | document | Say that both are optional and that nothing is drawn without both. Document `mesh`. **L** | No doc (lint). | No |
| `Camera.size` | components.dart:160 | rename | `orthographicSize` (and `VirtualCamera.size`, cameras.dart:66) **M** | Unity's name. `size` is vague on a camera (size of the view? the near plane?) and it's used only when `orthographic`. | Yes |
| `Camera` (class doc) | components.dart:120 | document | Replace "lowest id that also has a [Transform]" (every object has one) with "the shown object with a Camera and the lowest id". **L** | Wrong/stale. | No |
| `Camera()`, `near`, `far` | components.dart:124, 143, 146 | document | Constructor: defaults. `near`/`far`: metres; `near > 0`, `far > near`; what the engine refuses (the setter reverts and throws). **L** | No doc (lint). | No |
| `Camera.clearColor` | components.dart:150 | document | Add "(Unity's `backgroundColor`)". **L** | Mapping only. | No |
| `Transform.position`, `rotation`, `scale`, `copyWith` | components.dart:91–93, 106 | document | Relative to the parent; metres; unit quaternion; what is refused (NaN, non-unit rotation). **L** | No doc (lint). | No |
| `Transform.forward` | components.dart:96 | document | "In the parent's space; `GameObject.forward` is in the world." Consider `right`/`up` for symmetry. **L** | It's the same word as `GameObject.forward` (motion.dart) in a different frame. | No |
| `LightKind`, `Light.ambient`, `kind`, `color`, `intensity` | components.dart:203, 219, 227, 230, 233 | document | Enum doc; `sun` = Unity's `LightType.Directional`; `ambient` = Unity's `RenderSettings.ambientLight`; intensity's unit and range. **L** | No doc (lint). | No |
| (new) `Behaviour.isActiveAndEnabled` | components.dart:318 | add counterpart | `bool get isActiveAndEnabled` (Unity's): enabled, loaded and its object shown (`_enabledNow`). **L** | `enabled` alone doesn't say whether it runs, and Unity users look for this. | No |
| `onClick`, `onDragStart`, `onDragEnd` | game_object.dart:323, 335, 343 (+ Behaviour) | rename (optional) | `onPointerClick`, `onBeginDrag`, `onEndDrag` (Unity's `IPointerClickHandler`, `IBeginDragHandler`, `IEndDragHandler`) **L** | The family follows Unity for down/up/enter/exit and then switches to web/Flutter words. Do it only in a pass that already breaks the handlers. | Yes |
| `WorldSystem` | systems.dart:12, 14 | document | Constructor and `world` field. **L** | No doc (lint). | No |
| (new) `WorldSystem.componentRemoved` | systems.dart:50 | add counterpart | `void componentRemoved(GameObject object, Component component)`, called from `removeComponent` and when an object is destroyed. **M** | `componentAdded` has no counterpart. Physics has to rely on each behaviour's `onDisable`, so a system can't follow engine components or third-party behaviours going away. | No |
| `Signal` | signals.dart:21 | add counterpart | `Stream<T> get stream` (a broadcast stream fed by a listener made on first listen), so UI code can `StreamBuilder`/`listen` without a notifier. **M** | #5. package:fynkel has no Flutter, so not `Listenable`, but `Stream` is plain Dart. | No |
| `Signal.next` | signals.dart:78 | document | Outside a routine it's a plain future that listens until the next emit: one that never comes keeps the handler forever. **L** | Edge case not said. | No |
| `LayerMask.contains` | layers.dart:42 | document | "Whether [layer] is in it; RangeError outside 0–31." **L** | No doc (lint). | No |
| `FynkelStatus.ok`, `noMesh`, `noMaterial`, `noTexture`, `noObject` | error.dart:4, 11, 12, 47, 7 | document | Document the four that have no doc. `noObject`: replace the engine words "despawned/spawned" with "destroyed / never added". **L** | Lint, and the vocabulary should be the game's. | No |
| `FynkelException()`, `status`, `message` | error.dart:54, 56, 57 | document | Fields. **L** | No doc (lint). | No |
| Game mistakes thrown as `FynkelException` | game_object.dart:433, 636, 874 | change shape | Throw `ArgumentError` for a bad Transform (NaN, non-unit rotation: say "use `.normalized`"), `ArgumentError` for a parent from another world, and `StateError("$this has no Camera: add one, or use world.camera")` for a camera call without a camera. Keep `FynkelException` for what the engine itself refuses. **M** | The same mistake (wrong parent) throws `StateError` from `add` and `FynkelException` from `parent =`. Dart users catch `ArgumentError`/`StateError` for their own mistakes. "Transform.rotation: rotation is not a unit quaternion" doesn't say what to do. | Yes (exception type) |
| `Color.rgb`, `r`, `g`, `b`, `a`, `white`, `black` | color.dart:11–19 | document | 0–1 sRGB per channel; `a` 1 = opaque. **L** | No doc (lint). | No |
| (new) `Color.withAlpha` / a few constants | color.dart:18 | add counterpart | `Color withAlpha(double a)`; `clear`, `red`, `green`, `blue`, `gray` as Unity's `Color`. **L** | A fade needs `Color.rgb(c.r, c.g, c.b, 0)` today. | No |
| `WorldView.height`, `isDisposed`, `stats` | view.dart:98, 100, 130 | document | **L** | No doc (lint). | No |
| `HeadlessView()`, `ViewStats()` | view.dart:160, 12 | document | Constructor docs: `ViewStats()` is all zeros, for subtraction and tests. **L** | No doc (lint). | No |
| `Time` / pause | time.dart:46 | add counterpart (see cross-cutting #3) | `World.paused` (or `time.paused`) that stops updates, fixed steps, routines, motions and pointer delivery to objects while the view keeps drawing; `scale` stays for slow motion. **M** | #2: `scale = 0` still delivers taps and still calls `onUpdate`, and every game rebuilds pause by hand. | No |

**Judged "keep": about 240 members.** That's the rest of the ~321, including
all of `race` and `WorldSystems.maybeSystem` (the `maybeOf` idiom), `Signal.emit` (a `Signal<void>` takes `emit(null)`, as the class doc shows), all of `Time` (shortened Unity names: `delta`, `unscaledDelta`, `sinceStart`,
`scale`, `maxDelta`, `fixedDelta`, `inFixedStep`, `frame`, `fixedAlpha`;
consistent and documented), `Debug`, `LayerMask` (but `contains`),
`SignalSubscription`, `RoutineStopped`, `Routine.stop/isRunning/isStopped`,
`World.update/add/raycast/errors/time/input/cameras/debug/assets`, every
lifecycle hook, `active`/`activeInHierarchy`/`layer`/`name`/`children`/
`ancestor`, the `get`/`getAll`/`has`/`getInParent`/`getInChildren` family,
`addComponent`/`removeComponent`, `Behaviour.enabled`, `loaded`, `isAlive`,
`isDestroyed`, `Component.isRemoved`, `HeadlessView.read/resize`, `WorldView.frame`.

### What a subclass inherits (names only)

**GameObject** (game_object.dart, plus the `_Moving` mixin in motion.dart):
`world`, `id`, `isAlive`, `isDestroyed`, `loaded`, `assets`, `input`,
`debug`, `time`, `start`, `on`, `onLoad`, `onEnable`, `onStart`,
`onFixedUpdate`, `onUpdate`, `onLateUpdate`, `onDisable`, `onDestroy`,
`onPointerDown`, `onPointerUp`, `onPointerEnter`, `onPointerExit`,
`onClick`, `onLongPress`, `onDragStart`, `onDrag`, `onDragEnd`, `add`,
`destroy`, `parent`, `children`, `isPartOf`, `ancestor`, `active`,
`layer`, `activeInHierarchy`, `name`, `find`, `worldTransform`,
`screenToRay`, `screenToPlane`, `worldToScreen`, `addComponent`,
`removeComponent`, `get`, `getAll`, `has`, `getInParent`,
`getInChildren`, `transform`; from `_Moving`: `position`, `rotation`,
`scale`, `rotate`, `translate`, `worldPosition`, `forward`, `right`, `up`,
`lookAt`, `transformPoint`, `inverseTransformPoint`, `tween`, `sequence`,
`together`, `moveTo`, `moveBy`, `rotateTo`, `rotateBy`, `lookAtOver`,
`scaleTo`, `colorTo`, `punchScale`, `shake`; leaked by mistake (see table):
`transformStamp`, `componentCount`, `componentAt`; with fynkel_physics
imported: `physics`. Total: **79 names** (+3 leaked, +1 physics).

**Behaviour** (components.dart, from `Component` and `_Moving`):
`gameObject`, `isRemoved`, `transform`, `get`, `getAll`, `has`,
`ancestor`, `getInParent`, `getInChildren`, `enabled`, `world`, `input`,
`debug`, `time`, `assets`, `start`, `on`, `onLoad`, `onEnable`, `onStart`,
`onFixedUpdate`, `onUpdate`, `onLateUpdate`, `onDisable`, `onDestroy`,
the nine pointer handlers, and the same 24 `_Moving` names as above; with
fynkel_physics: `physics`. Total: **67 names**.

**Which common game words they take.** High risk (a game wants these for
its own fields or methods, and a clash is a compile error or a silent
override): **`start`**, **`id`**, **`time`**, **`on`**, **`shake`**,
`scale`, `up`, `right`, `forward`, `add`, `get`, `has`, `find`, `loaded`,
`debug`, `sequence`, `together`, `name`, `layer`, `active`. Low risk (the
game rarely wants another meaning): the `on…` hooks, `transform`, `world`,
`input`, `assets`, `parent`, `children`, `destroy`, `position`,
`rotation`. The top-level functions take `wait`, `until`, `all`, `race`,
`nextFrame`, `fixedStep`, `whileTrue` in every file that imports fynkel.
The table proposes freeing `start`, `id`, `until`, `all`, `whileTrue` and
the three camera methods, and leaves `time` (central to the guide:
`time.delta`), `get` and `add`. `shake`, `scale`, `up`/`right`/`forward`,
`sequence` and `together` belong to the motion reviewer; I'd flag `shake`
(an object-shake tween that reads like a game verb) and
`sequence`/`together` (generic words for tween combinators) to them.

## Cross-cutting observations (most important first)

1. **Lifecycle promises break at the edges of the world.** `World.dispose`
   skips `onDisable`/`onDestroy` (#7), `world.all` sees hidden objects (#8),
   and hidden (pooled) objects keep paused routines that resume stale. The
   same rule should hold everywhere: what the game can't see doesn't count,
   and every object that goes gets its `onDestroy`. Fix `dispose`, add
   `includeInactive` to the queries, add `stopAllRoutines`, and write a
   pooling section in the guide.

2. **The surface of every object is large (79 names) and flat.** It holds
   the lifecycle, finding, components, transform shortcuts, 12 tween helpers,
   3 camera methods, routines and signals. A Unity user's `MonoBehaviour`
   inherits about as many, but Unity's tweens (DOTween) are extension
   methods on `Transform`, and camera maths is on `Camera`. Moving the
   camera methods to `Camera` and renaming `start`/`id` frees the most
   common words. Consider (motion reviewer) whether the tween helpers
   belong on objects or on a `tweens`/`transform` facade.

3. **Pause is missing** (#2). `time.scale = 0` keeps `onUpdate` running and
   taps reaching objects. A Fynkel-level `paused` (no updates, steps,
   routines, motions or object pointer events; the view still draws;
   `unscaled` motions and Flutter UI still run) would be one switch for
   menus, dialogs and the app going to the background. Unity has no such
   switch, but every Unity game builds one, and a code-first framework can
   do better.

4. **Unity false friends need a column in §20 and a line in each doc**:
   `parent =` (local kept vs. Unity's world kept), `position`/`rotation`
   (local here, world in Unity: Godot-style `position`/`global_position`;
   keep the names, but say it in the guide's Unity table and add a
   `worldRotation` getter/setter to go with `worldPosition`),
   `MeshRenderer.material` (shared), `find` (all descendants), and
   `world.all` (includes inactive). The names are fine; the surprises are
   the problem.

5. **Two error families for game mistakes.** `StateError`/`ArgumentError`
   (most of the core, with good messages that say what to do: `_notAdded`,
   `only`, `_addWait`, `add`) next to `FynkelException(invalidValue/noComponent)`
   for the transform, parent and camera checks, which are the game's
   mistakes, not the engine's. Engine vocabulary also leaks into messages
   and docs: "despawned", "never spawned" (game_object.dart:840,
   error.dart:6, 50), where the game's words are "destroyed" and "added".
   Rule: Dart errors for what the caller did wrong, `FynkelException` only
   for what the engine refused or failed at.

6. **The same word for the same thing across packages.** `gameObject` is
   the word on Component, PhysicsHit, Collision and ControllerColliderHit;
   only `RayHit.object` and `ObjectError.object` differ. The hit types are
   `RayHit` (core) and `PhysicsHit` (physics) for Unity's one
   `RaycastHit`. That's acceptable (they hit different things), but the
   fields should match: `point`, `normal`, `distance` already do. The
   `layers:` parameter is the same in `World.raycast`, the physics queries
   and `Input.layers`. Good.

7. **Name clashes with Flutter.** `Color`, `Transform` and `Material`
   collide with flutter/widgets.dart and material.dart, so every game file
   with a HUD needs `hide Transform` (examples do: arena, lab) or `show`
   lists (#6). Renaming is too costly. Instead: (a) document the import
   pattern in §15, (b) give fynkel_flutter conversions
   (`toFlutterColor()`/`FynkelColor.fromFlutter`), (c) consider a
   `package:fynkel/fynkel.dart` + `package:fynkel_flutter/hud.dart` split
   so HUD files import one Flutter library that already hides the three.

8. **Two ways where one would do.** `Routine.done` vs. awaiting the routine.
   `isAlive` vs. `isDestroyed` is acceptable (isAlive also covers "not
   added" and "world disposed"), but the docs should cross-reference each
   other. `LayerMask.plus/minus` (one layer) vs. `|`/`&` (masks) is fine.
   `on` vs. `Signal.listen` is fine: owned vs. unowned.

9. **Docs are good, and the gaps are mechanical.** 50
   `public_member_api_docs` hits in this scope, almost all on simple
   fields and constructors (Transform, Camera.near/far, Light, MeshRenderer,
   Color, FynkelStatus, view getters). The few *wrong* docs are the
   `Camera` class doc and `World.camera` ("also has a Transform"; hidden
   cameras not mentioned), the `ObjectError` class and `object` docs
   (only load/destroy listed), and `World.first` ("first added", but
   grouped by class). Turning on `public_member_api_docs` for
   packages/fynkel in CI would keep it at zero.

10. **Extension boundary.** extend.dart's contract is small and clear, but
    `GameObjectSystems` leaks through fynkel.dart, `World.pointer` and
    `WorldView.address` are public plumbing, and `WorldSystem` has
    `componentAdded` with no `componentRemoved`. A rule worth writing down:
    fynkel.dart exports only what a game calls, extend.dart what a package
    builds on, and platform.dart what a plugin needs. Nothing in the
    first should take an FFI type.
