# Stage 14 API pass: package:fynkel's core

Reviewed at main `bf27879a`: `lib/fynkel.dart`, `extend.dart`, `testing.dart`,
`platform.dart`, `hooks.dart`, and `src/` game_object, world, world_loop,
world_objects, world_resources, components, systems, input, pointing, keys,
time, routines, signals, pool, assets, layers, error, frame_list, view, debug,
profile, system_info. Also the `_Moving` part of motion.dart that gives
objects and behaviours `position`, `rotate` and the rest. Nothing below is in
docs/stage-14-findings.md or in §21 of the guide.

Every behaviour claim was checked with a throwaway test in
`/tmp/s14api/scratch-core/test/` (a_test … e_test, run with `dart test`).
The messages quoted are the actual output.

---

## C1 — Values the engine checks are reported with Rust names, engine units and Rust ranges — **unhelpful error**

- **Where:** `crates/fynkel_world/src/describe.rs:135`
  (`"{ty}.{field}: {value} is outside {}..={}"`), field tables in
  `crates/fynkel_world/src/components.rs:255` (Camera) and `:416` (Light);
  passed on as `ArgumentError(message)` with no name by
  `packages/fynkel/lib/src/native.dart:62`. Every Camera, Light and
  MeshRenderer setter and constructor reaches it, and so do `World.quality`
  and `setParent`.
- **What a game sees:**
  - `Camera(fovY: 60)` (a Unity user writing degrees) →
    `Invalid argument(s): Camera.fov_y: 3437.7468 is outside 1..=179`. The
    Dart name is `fovY` and it takes radians. The message shows degrees and
    never says "radians" or "60.deg".
  - `light.angle = 3.5` → `Light.spot_angle: 200.53523 is outside 0..=179`.
    The Dart name is `angle`, again in radians.
  - `camera.orthographicSize = 0` → `Camera.size: 0 is outside 0.0001..=1000000`.
  - `Quality.high.copyWith(shadowCascades: 5)` → `quality: 5 cascades: 1 to 3`.
    The Dart field is `shadowCascades`.
  - `a.parent = b` (b below a) → `object#7 can't be the parent of object#6: it is object#6 or below it`.
    These are engine ids. The Dart objects print as `GameObject#7` or
    `Player#7`, and their names are not used.
  - `ArgumentError.name` is null in all of these, so even a debugger shows no
    parameter.
- **Why it matters:** these are a beginner's most common mistakes: degrees
  for radians, a zero size, a negative intensity. The message names a field
  that doesn't exist in Dart, in units the API never uses, with `..=`
  syntax. Compare the messages written in Dart, which are good: the spot
  light's 150° message (`... this one is 170.0°. Narrow it, or turn its
  shadows off`), `targetFrameRate`, `time.scale`, `only<T>`.
- **Fix:** check in Dart, in each setter and in `World.quality`, with the Dart
  name, the Dart unit and a hint. For example:
  `ArgumentError.value(60, 'fovY', 'is in radians, 1° to 179° (0.017 to 3.12): did you mean 60.deg?')`.
  Or map the engine's field names and units in `check()` for the component
  kinds. The parent cycle is cheap to check in Dart before the call (walk
  `_parent`), so its message can print the Dart objects.
- **Breaks game code?** No. Only message text changes, and the error types
  stay the same.
- **Verified:** test (a_test.dart).

## C2 — Values aren't checked until the object is added, though the docs say "from the setter" — **misleading**

- **Where:** `components.dart:308-310` (Camera's class doc: "A value the
  engine refuses ... throws an ArgumentError from the setter"),
  `components.dart:752-754` (`Light.intensity`: "Out of range or NaN throws
  ... and keeps the old value"), `game_object.dart:919-925`
  (`GameObject.transform`: "Setting it checks the value at once"), and
  `components.dart:140-155` (Transform's fields). The checks run only in
  `_write()` on a live component (`_live` is null before add) and in the live
  `transform` setter (`game_object.dart:935-946`). A described (not yet
  added) object skips them.
- **What happens:** each of these is accepted silently, and the error comes
  from the later `world.add` or `parent.add`, with a stack pointing there:
  - `Light.point(intensity: -1)`
  - `Camera(fovY: 60)`
  - `GameObject(transform: Transform(rotation: Quat(0, 0, 0, 2)))`
  - `GameObject()..position = Vec3(nan, 0, 0)`

  The messages name no object (`Light.intensity: -1 is outside 0..=1000000`,
  `Transform.rotation: rotation is not a unit quaternion`). When the add
  runs in a parent's `onLoad`, the error goes to `world.errors` and the
  object is destroyed. The developer then has to find which of a prefab's
  components or children it was.
- **Inconsistent:** the Light constructor already runs one check at once
  (`_refused()`, the 150° spot-shadow rule, `components.dart:714`), and the
  ranges it skips are checked only later.
- **Fix:** run the same checks while the object isn't added. `Transform._check`
  is static already, so call it in the described branch of the `transform`
  setter, and in `GameObject(transform:)`. For components, use a Dart range
  table (which also gives C1's messages) or a world-less engine "validate"
  call. Then the error comes from the line that made the value.
- **Breaks game code?** No. Code that would throw at `add` throws earlier, at
  the line at fault.
- **Verified:** test (a_test.dart, e_test.dart).

## C3 — `world.all/only/first` return objects whose `onLoad` hasn't finished — **bug**

- **Where:** `world.dart:838-887`. `_isActive` checks only each object's own
  `_activeSelf` up the tree, not `_hiddenForLoad` or `_appeared`.
- **What happens:** an `Enemy` with `late int hp` set in an async `onLoad`:
  `world.add(Enemy()); world.all<Enemy>().first.hp` →
  `LateInitializationError`, and so does `world.only<Enemy>().hp`. Right
  after the add, `all<Enemy>()` has 1 object, with
  `activeInHierarchy == false`. Code that walks `all<Enemy>()` in `onUpdate`
  (a HUD, an AI manager) touches enemies that haven't loaded, haven't
  started and aren't drawn.
- **Why it's wrong:** the doc says hidden objects are left out unless
  `includeInactive` ("a pool's waiting objects aren't found"). A loading
  object is hidden in every other sense: not drawn, not hit, not updated.
  Unity's `FindObjectsByType` never returns an object before its `Awake`.
- **Fix:** leave out objects that haven't appeared unless `includeInactive`
  (`_isActive` also checks `!x._hiddenForLoad`), and say so in the doc.
- **Breaks game code?** Only code that relies on finding objects while they
  load, which is unlikely and would be fragile. `includeInactive: true` keeps
  the old result.
- **Verified:** test (b_test.dart, "all includes loading").

## C4 — `get<T>`, `getAll`, `has`, `getInParent` and `getInChildren` with an object type return null with no error — **unhelpful error**

- **Where:** `game_object.dart:806-851`, and the same on `Component`
  (`components.dart:75-95`). `T extends Object` is allowed so that
  interfaces work, but only components are searched.
- **What happens:** `child.getInParent<Level>()` (Level is a GameObject
  subclass) returns `null`, always. The game meant `ancestor<Level>()`.
  `get<Player>()` and `getInChildren<Coin>()` likewise. The same `<Level>`
  works in `world.all<Level>()`, `world.only<Level>()` and
  `world.first<Level>()`, which search objects and components. So the type
  argument means "objects too" in one place and silently "never" in the
  other. The guide's Unity table (§22) maps `GetComponentInParent<T>()` to
  `getInParent<T>()`, which invites exactly this.
- **Fix:** when `T` is a GameObject type, throw (or assert in debug) with a
  pointer to the right call:
  `ArgumentError('Level is a GameObject, not a component: use ancestor<Level>() (or find) to reach an object')`.
  The test is cheap: `<T>[] is List<GameObject>`. Or make
  `getInParent`/`getInChildren` find objects too, as `world.all` does.
- **Breaks game code?** No. Such calls return null today in every case.
- **Verified:** test (c_test.dart: `getInParent<Level>: null`).

## C5 — `getInChildren` and `getInParent` find components on inactive objects, unlike Unity's and unlike `world.all` — **misleading**

- **Where:** `game_object.dart:830-851`. Nothing checks `active`, and there
  is no `includeInactive`.
- **What happens:** a child set `active = false` with a `Health` →
  `root.getInChildren<Health>()` returns it, while `world.all<Health>()`
  returns `[]`. A pool's hidden objects (under `Pool.parent`) are found by
  `parent.getInChildren<Shot>()`.
- **Why it matters:** Unity's `GetComponentInChildren<T>()` and
  `GetComponentInParent<T>()` skip inactive objects unless
  `includeInactive: true`. The same name doing the opposite surprises a
  Unity user, and the doc doesn't say. Within Fynkel, `world.all`, `only`
  and `first` take `includeInactive` and default to false, so the idea is
  named in one place and missing in the other.
- **Fix:** add `{bool includeInactive = false}` to both, as `world.all` has,
  and document it.
- **Breaks game code?** Changing the default to false changes what finds an
  inactive object today. To keep that code working, say it in the release
  notes; the old result is one argument away (`includeInactive: true`). If
  that is too much, keep the default true but add the parameter and
  document the difference from Unity.
- **Verified:** test (c_test.dart).

## C6 — "start" left over from before `startRoutine`: in an error message, a doc reference and the guide — **unhelpful error**

- **Where:**
  - `routines.dart:239`: `StateError('$what waits only inside a routine: start(() async { await $what; })')`.
    For `wait(1)` outside a routine this prints
    `wait waits only inside a routine: start(() async { await wait; })`.
    `start` doesn't exist (it's `startRoutine`), and `await wait;` isn't
    valid Dart for a function.
  - `world.dart:645`: `([GameObject.start])` is a dangling doc reference.
  - `docs/writing-a-game.md:343` lists `start` among a behaviour's
    shortcuts. The rename is recorded at line 4930 ("`start` is
    `startRoutine`").
- **Fix:** have the message say what to write, e.g.
  `"wait(...) works only inside a routine: startRoutine(() async { await wait(1); })"`
  (use `await nextFrame` for the getters). Change the doc reference to
  `[GameObject.startRoutine]`, and the guide's word to `startRoutine`.
- **Breaks game code?** No.
- **Verified:** test (a_test.dart) and read.

## C7 — `ArgumentError.value(someObject, …)` prints "Instance of 'X'", not the object — **unhelpful error**

- **Where:** Dart's `ArgumentError.value` formats the value with
  `Error.safeToString`, which never calls a class's own `toString`. Sites:
  - `pool.dart:102` → `isn't from this pool: Instance of 'Shot'`
  - `game_object.dart:766` → `is not on GameObject#3: Instance of 'Health'`
  - `world_objects.dart:68` → `in another world, so it can't be the parent of GameObject#6: Instance of 'GameObject'`
  - `world.dart:578` → `height must be above 0: Instance of 'Fog'` (Fog has a good `toString`, which isn't used)
  - `routines.dart:285` → `is empty: Instance(length:0) of '_GrowableList'`
  - also `components.dart:532` (`ViewPoint`) and `systems.dart:208` (an `AssetKind`)
- **Fix:** put the object in the message text, e.g.
  `ArgumentError('$object isn\'t from this pool (it came from ${object._pool ?? 'none'})')`,
  or pass `'$object'` as the value.
- **Breaks game code?** No.
- **Verified:** test (b_test.dart).

## C8 — Putting a removed component back says it "is on" its object "already" — **unhelpful error**

- **Where:** `game_object.dart:728-730`. `_owner` stays set after
  `removeComponent`, so the first check fires.
- **What happens:** `o.removeComponent(light); o.addComponent(light)` →
  `Light.point(...) is on GameObject#1 already; a component belongs to one object`.
  It isn't on it any more. A behaviour gives
  `Health on GameObject#1 is on GameObject#1 already` (its `toString`
  already says where it is). Moving the removed light to another object
  gives the same message.
- **Why it matters:** "can't be put on again" is documented on
  `removeComponent`, but the message points the developer at the wrong
  cause.
- **Fix:** check `_removed` first:
  `StateError('$component was taken off $owner, and a component taken off can\'t be put on again: make a new one')`.
- **Breaks game code?** No.
- **Verified:** test (d_test.dart).

## C9 — Messages name an internal step, not what the game did — **minor**

- **Where:** `motion.dart:1075-1077` (`_setPosition` reads `_above`, which
  is `parent`), and `game_object.dart:179-187` (`_engine(what)` ignores
  `what` before the object is added: `world` throws
  `_notAdded('world')`).
- **What happens:**
  - Setting `position` on a destroyed object → `GameObject#5 was destroyed, so its parent can't be read`.
  - Reading `position` → `… so its worldTransform can't be read`.
  - Before the add, `parent`, `children`, `find`, `isPartOf`, `ancestor`
    and `startRoutine` all say `GameObject (not added) has no world yet`.
    (The advice that follows, "touch the world from its onLoad on", is good.)
- **Also inconsistent:** before the add, `getInParent` and `getInChildren`
  walk the objects given so far (DC7), but `ancestor`, `parent`, `children`
  and `isPartOf` throw, though the private `_above` already knows the
  parent-to-be.
- **Fix:** have `_engine` pass `what` to `_notAdded`. Have the position and
  rotation setters call `_engine('its position can\'t be set')` before
  `_above`. Consider letting `ancestor` and `isPartOf` walk `_queuedIn`
  before the add, as `getInParent` does.
- **Breaks game code?** No.
- **Verified:** test (a_test.dart, c_test.dart).

## C10 — Errors name objects by id only, and Transform's give no value — **minor**

- **Where:** `game_object.dart:1338` (`toString` is `'$runtimeType#$_id'`,
  with no name), and `game_object.dart:957-994` (`_check` and `_refuse`).
- **What happens:**
  - `GameObject#5 has a Camera already`, `GameObject#5 was destroyed, so …`.
    In a scene of hundreds of plain `GameObject`s, the id doesn't say which
    one; the name the game gave (`'player'`) would.
  - `Transform.rotation: rotation is not a unit quaternion (use .normalized)`
    and `Transform.position: NaN or infinity` give neither the value nor its
    length, and don't say they came from setting `rotation` or `position`
    on an object.
- **Fix:** print the name in `toString` when there is one
  (`GameObject#5 'player'`, `Player#7`). Put the value in Transform's
  messages
  (`rotation Quat(0, 0, 0, 2) has length 2, not 1: use .normalized`).
- **Breaks game code?** Only code that parses `toString`, which no game
  should do.
- **Verified:** test (a_test.dart, c_test.dart).

## C11 — Unity's `childCount` and `GetChild(i)` exist but are hidden in extend.dart; no "all in children" — **awkward**

- **Where:** `systems.dart` `GameObjectSystems` (`childCount`, `childAt`,
  `componentCount`, `componentAt`) is exported only by `extend.dart`, whose
  doc says "Games don't need it". `GameObject.children`
  (`game_object.dart:558`) makes a new unmodifiable list on each read.
- **Why it matters:** walking an object's children each frame (a
  formation, a row of HUD markers, a model's parts) is common. Unity's
  everyday API for it is `transform.childCount` and `GetChild(i)`, which
  allocate nothing. Here a game either allocates each frame (GD9's concern)
  or imports the packages' library. There is also no counterpart of
  `GetComponentsInChildren<T>()`: tinting every `MeshRenderer` of a model
  copy, or turning off every particle system under an effect, takes a
  hand-written recursion.
- **Fix:** move `childCount` and `childAt` onto `GameObject` in fynkel.dart,
  and keep the extension members as forwarders. Add
  `getAllInChildren<T>({includeInactive})` (and maybe `getAllInParent`)
  next to `getInChildren`.
- **Breaks game code?** No. These are additions, and the extension's names
  stay.
- **Verified:** read.

## C12 — Two docs out of date — **minor**

- `assets.dart:93-96` (`Assets.preload`) lists models, textures, sky images,
  shaders and package kinds, but not fonts. `_kindOf` loads `.ttf`, `.otf`
  and `.ttc` as fonts (its own error lists them).
- `systems.dart` (`WorldSystem`'s constructor doc) gives
  `world.system(Fog.new)` as its example. `Fog` is core's fog settings
  class (`World.fog`), not a system, so the example names the wrong thing
  and wouldn't compile.
- **Fix:** add fonts to the list; use a made-up name (`Wind.new`).
  **Breaks:** no. **Verified:** read and test (the preload error text).

## C13 — `worldToScreenPoint` with no view size doesn't say what to do — **minor**

- **Where:** `components.dart:568`:
  `worldToScreenPoint needs a view with an area; got 0.0x0.0`.
- **Why it matters:** it happens in a test, or before the view's first
  frame. The fix is to pass `width:` and `height:`, or call
  `world.input.simulate.resize(w, h)` in a test, as testing.dart's own
  `_sized` message says. The method's own message doesn't say so.
- **Fix:** reuse testing.dart's wording. **Breaks:** no. **Verified:** test (e_test.dart).

## C14 — `World.particleStepTime` and `World.meshChangeTime` duplicate `world.profile` — **minor**

- **Where:** `world.dart:358-372`. `meshChangeTime` *is* `profile.meshes`.
  `particleStepTime` is the particle system's entry in `profile.systems`,
  read from a separate stopwatch.
- **Why:** since GT7 there are two names for each of these numbers, and the
  guide's profiler story is `world.profile`.
- **Fix:** mark both `@Deprecated('use world.profile')`. **Breaks:** no
  (deprecated, not removed). **Verified:** read.

## C15 — `Input.simulate` is also the real feed — **minor**

- **Where:** `input.dart:128`, and `InputSimulation`.
- **Why:** the name says "tests". The doc itself says it's "what feeds this
  input: the view's events, or a test's", and `FynkelView` sends real
  touches and keys through it. A reader of the Flutter code wonders why
  production input is "simulated". Low priority.
- **Fix:** leave the name and say so in the class's first line, or alias it
  as `input.feed` and keep `simulate`. **Breaks:** no with an alias.
  **Verified:** read.

---

## Checked and found fine

- **Time** (`scale`, `maxDelta`, `fixedDelta`, `maxFixedSteps`,
  `World.update`'s `dt`, `targetFrameRate`): each throws an
  `ArgumentError.value` with the name, the value and the allowed range.
  The defaults match Unity's (1/50 fixed, 1/3 max delta). The non-Unity
  names (`delta`, `sinceStart`, `frame`) are deliberate and in §22.
- **Layers:** `RangeError` "layers are 0 to 31: … 40". `LayerMask`'s API is
  coherent.
- **Lookups:** `world.only<T>()` (none or many, and points at `all<T>()`).
  `input.axis2` and `input.action` for unbound names list the bound ones.
  `Signal.emit()` without a value says what to do.
- **Errors at the wrong moment:** re-entrant `World.update`; `world.add` or
  `parent.add` twice, of a destroyed object, or below itself; a second
  engine component of a kind. All have clear messages.
- **Assets:** no source gives "make it with World(source: ...)". An unknown
  extension lists every kind and its extensions. A name loaded as one kind
  and asked for as another is caught.
- **Views:** `HeadlessView` and `resize` at size 0 or above 8192 say
  "each side is from 1 to 8192 on this device".
- **Lights and cameras:** the spot-light shadow width check gives the angle
  and two remedies. `Camera.pixels` gives a `RangeError` with its bounds.
- **Pool:** its names match Unity's `ObjectPool` (`get`, `release`,
  `countActive`, `countInactive`, `countAll`, `maxSize`, `clear`). The
  double-release difference between `pool.release` and `object.release` is
  documented. `ObjectHandle` is clear.
- **Routines:** `race`/`waitAll`, `Routine` as a `Future`, `stop`, and
  unscaled waits during a pause behave as their docs say.
- **Pause and the app:** `World.paused` vs `time.scale = 0` vs
  `applicationPaused` are documented and named as in Unity.
- **`world.first<T>()`** gives the oldest live match across subclasses
  (checked).
- **Light and Camera defaults** match Unity's `AddComponent` defaults: no
  shadows, range 10, spot 30°, 60° field of view, near 0.1, far 1000.
- **testing.dart:** `world.run`/`tap`/`viewPoint` (a tap reaches
  `onClick`, checked), `expectImage`/`expectFrame` messages, and the bless
  flow.
- **The rest:** hooks.dart's build errors (missing cargo, missing target,
  stale lock) give the command to run. The `FynkelStatus` and
  `FynkelException` docs match `check()`. `SystemInfo` names follow
  Unity's.
