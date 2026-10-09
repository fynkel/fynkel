# Stage 3 plan: how a game is written

Status: accepted 2026-10-09, with the five answers in §5. Scope and
rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 2 is done
([report](stage-2-report.md)).

Stage 2 showed the engine can draw a scene and load it from bytes. Writing
the garden showed how much code even a small scene needs: 132 lines of code in
`garden.dart`, plus the app's part of `main.dart`, for "a character in a garden; tap it
and its head turns; drag to look around". Most of it is plumbing: one
`onFrame` that does everything, a raycast by hand to find what was
tapped, a five-step unload, and assets passed in from outside.

Stage 3 settles **one way** to write a game, before animation, physics
and audio each invent their own: where logic lives, when it runs, how it
hears about input, how it gets its assets, and how it goes away again.
Everything after this builds on it.

Stage 3 ends when the garden is written like this, runs as it does now
on all three devices, and passes the same memory cycles:

```dart
void main() => runApp(FynkelApp(Garden()));

/// The level. It's a GameObject like any other: what it adds is its
/// content, and destroying it destroys all of it.
class Garden extends GameObject {
  @override
  Future<void> onLoad() async {
    add(OrbitCamera(target: Vec3(0, 1.3, 0)));
    add(GameObject(transform: Transform(forward: Vec3(-0.4, -1, -0.3)), components: [Light.sun(intensity: 2.5)]));
    add(Ground());
    for (final (seed, name) in props.indexed) add(Props(name, seed: seed));
    add(Character());
  }
}

/// Self-contained: it loads its own model and handles its own taps.
class Character extends GameObject {
  late final GameObject head;
  double _turn = 0;

  @override
  Future<void> onLoad() async {
    add(await assets.model('character.glb'));   // shared by name; freed with its last user
    head = find('head')!;
  }

  @override
  bool onTap(Tap tap) {
    _turn += pi / 2;
    return true;
  }

  @override
  void onUpdate(double dt) {
    final step = _turn.clamp(-6 * dt, 6 * dt);
    head.rotate(Vec3.up, step);
    _turn -= step;
  }
}
```

`OrbitCamera`, `Ground` and `Props` are the garden's own classes, a few
lines each (§3.8). The engine has no ground, sun or orbit camera: those
are one game's choices. The names follow the decisions in §5.

Not in stage 3: animation, physics, audio, saving, an editor, several
behaviours on one object (§3.1), object input beyond taps (§3.7), prefab
variants and overrides, and scene files. Each comes back later on its own, on top of what stage 3
settles.

## 1. Lessons from Fynkel 1, and what we do differently

| Fynkel 1 | Fynkel 2 |
|---|---|
| Behaviours were components with fields the editor saved, so they needed code generation, a registry, and a JSON schema per script. | A game object with code is a plain Dart class. Nothing is generated or registered; no editor has to read it. |
| There were several ways to make an object: spawn with components, `ObjectSpec` and `world.build`, prefab files, scene files. Each had its own rules. | One way: make a `GameObject` (or your own subclass), then `add` it. A prefab is a class. A level is a class. |
| Prefabs were files with overrides and variants, kept in step with the scene by the editor. | A prefab is code, so its "variants" are constructor parameters and subclasses, which Dart already checks. |
| Assets were found through an import pipeline, a cache and sidecar files, by path and sub-asset key. | An object asks for an asset by name and gets bytes from a source the app gives (Flutter's asset bundle by default). The engine still never touches files. |
| Unloading was the game's job, and easy to get partly wrong. | What an object loads is released when the object is destroyed, and freed when nothing else uses it. |

## 2. What's new, by package

```
crates/
  fynkel_world/     an active flag: an inactive object and everything below it isn't drawn or hit by rays
  fynkel_ffi/       fy_active_get / fy_active_set / fy_active_in_hierarchy_get (ABI 9); fy_camera_active (ABI 10);
                    fy_transform_set_many (ABI 11)
packages/
  fynkel/           GameObject you can extend: onLoad, onStart, onUpdate, onTap, onDestroy; add and destroy;
                    World.update(dt, input); Input and Tap (plain Dart); Assets (by name, shared, released)
  fynkel_flutter/   FynkelView(root:), FynkelApp; fills Input from Flutter's events; assets from the bundle
  fynkel_measure/   measures any app from outside its code (a separate entry point)
examples/
  garden/           rewritten; hello rewritten
  coins/            NEW: a second small game, to prove the model past one scene (§4, step 6)
```

The update loop lives in Dart, in package:fynkel (ADR-0001: game logic
is Dart above the C interface, and the engine never calls into Dart).
Rust gains only the active flag. package:fynkel still has no Flutter in
it, so a whole game runs in `dart test` without a screen.

## 3. Design decisions

### 3.1 Objects with code

- **`GameObject` can be extended.** A subclass is what Unity calls a prefab: its class says what it is made of and what it does. `Character()` makes an instance; adding it to the world makes it live. Ten characters are ten `Character()` calls, as ten buttons in Flutter are ten `ElevatedButton(...)`s. There's no separate prefab file to keep in step.
- **Defined and spawned are the class and the instance.** Flutter splits a widget (the description) from its element (what's live) because widgets are rebuilt every frame. Game objects aren't: they live and change in place. A second class per object would double the code, which is what this stage is meant to cut.
- **One way to make objects.** `GameObject(name:, transform:, components:)` makes a plain one; a subclass passes the same to `super`. `parent.add(child)` and `world.add(root)` make it live and return it. `world.spawn(...)` and `world.spawnModel(...)` go: `assets.model` gives a model instance as a GameObject to add (§3.4).
- **An object is live once added, and can't be added twice.** Before it's added, its transform, name and components are held in Dart and applied when it is. Its children can be added before or after.
- **The constructor describes; `onLoad` acts.** A constructor only says what the object is made of (transform, name, components, its own fields). Anything that touches the world (assets, `find`, rays, other objects) is done from `onLoad` on, and doing it earlier throws a `StateError` that says so. One rule, so no developer has to learn which members work before `add`.
- **The same object comes back.** `find('head')`, `children`, `parent` and a ray's hit return the instance that was added, not a new handle, so a hit on a character's mesh leads to the `Character` with its fields. The world keeps a map from engine ids to Dart objects; objects a model makes get a plain `GameObject` the first time they're asked for.
- **`destroy()`** removes the object and everything below it, calls their `onDestroy`, and releases what they loaded. It replaces `despawn`.
- **One class per object, no behaviour components, in this stage.** Unity puts several behaviours on one object (a mover, a health, a sound). That's flexible, but every object then has two levels (the object and its behaviours), and they look each other up by type. If real games need more, behaviours can be added on top later without changing what's here. *Decision 1 in §5.*
- **Reuse is with mixins or children**, the two ways documented and shown in the examples:
  - **A mixin** for what is part of the object: `class Barrel extends GameObject with Health`. Every lifecycle method on `GameObject` is `@mustCallSuper`, so a mixin's `onUpdate` that forgets `super.onUpdate(dt)` (and with it the next mixin's) is an analyzer warning, not a silent bug.
  - **A child object** for what can come and go: `add(Shield())` with no components is logic only. It costs one engine entity and its update, and `destroy()` takes it off again. This is Godot's way.

- *Settled in step 1:*
  - **Every object has a `Transform`**, the identity unless `transform:` gives one, as in Unity and Godot. It's never among `components` (an `ArgumentError` says to use `transform:`); in the first conversion of the tests that rule caught a camera whose transform had gone into its components.
  - **`add` adds objects only.** `GameObject.add(Component)` is gone, as Dart has no overloading; components are given to the constructor or set later (`object.camera = ...`), and `remove<T>()` stays.
  - **Equality is identity.** The world keeps one Dart object per engine id that has been asked for, so `==` by id was no longer needed.
  - **`destroy()` twice does nothing**, and destroying an object that waits to be added (added to one that isn't live) takes it out, so it never comes.
  - **A refused component** (a camera with `fovY: 500`) leaves the object not added and still describing itself, to be fixed and added again.
  - **`all<T>()` lists the game's own objects**, grouped by class, each class in the order its objects were added. A model's parts aren't included; they're found by name.
  - **`model.instantiate(transform:)`** replaces `spawnModel`: a copy is an object to add, as any other.
  - **The cost** is on loading, not on frames (§6).

### 3.2 The loop and its order

Each frame, `world.update(dt, input)` does, in this order:

1. **Taps.** Each tap is cast into the world from the active camera. The hit object's `onTap(tap)` is called; if it returns `false`, its parent's is, and so on up to the root, until one returns `true` (a DOM-style bubble). So a tap on the character's arm reaches `Character.onTap` without the game knowing the arm. *Decision 4.*
2. **Starts.** Each object that became live since the last frame gets `onStart()`, once, parents before children.
3. **Updates.** Each live object's `onUpdate(dt)`, parents before children, in the order they were added. Objects added during an update start next frame; objects destroyed during it get no more calls, even this frame.
4. **The frame.** The view copies the world for the render worker, as now.

Everyone reads the same input: `input.drag`, `input.scroll`, `input.keys` and so on, as `FrameInput` does today, through `input` on any object. The order is fixed and documented, and tests check it, so a game can rely on "my parent updated before me".

~~Only objects whose class overrides `onUpdate` are visited each frame~~ (see step 3 below). Model nodes get no Dart object at all until asked for, so they cost nothing per frame. The budget: 10,000 updating objects in trees, each moving itself, in under 1 ms on the Note10+.

- *Settled in step 3:*
  - **Every object the game added is visited**, not only those that override `onUpdate`. Taking an object off the list when `GameObject`'s own `onUpdate` runs can't work with `@mustCallSuper`: an override calls `super.onUpdate(dt)`, which would take it off too. Visiting is cheap: 10,000 objects with nothing to do cost 0.02 ms on the Mac and the iPhone, 0.07 ms on the Note10+.
  - **`@mustCallSuper` works as promised**: a mixin's `onUpdate` without `super.onUpdate(dt)` is an analyzer warning (`must_call_super`). `onTap` isn't marked: it returns whether the tap was taken.
  - **The order is the order of adding**, which puts parents before children. A new `parent` counts from the next frame: the moved object and everything below it go last. Objects added during a start start the same frame; objects added during an update or shown by one start the next.
  - **Hidden objects** (inactive, or below one) are skipped by taps, starts and updates, and start when first shown. Dart works this out itself, as only Dart turns objects off; with none off it costs nothing.
  - **`onDestroy`** comes children first, while each object is still there to read, and before it's gone from the engine. Adding to a dying object throws.
  - **A tap is taken only by returning true**; an object that destroys itself in `onTap` and returns false passes it on. A tap on nothing, or with no camera, goes nowhere. Its ray starts at the camera's near plane, as `rayThrough` always did.
  - **Taps are cast from the camera the view draws from**, which only the engine knows (the lowest shown id with a camera and a transform): `fy_camera_active`, a small query added to the C interface (ABI 10).
  - **Transforms are kept in Dart** (step 3b): reading `transform` makes no call; setting it checks the value at once, as the engine would and with its words, and the frame's moves go to the engine in one `fy_transform_set_many` call when something next reads the engine's transforms (a view's frame, a raycast, `worldTransform`, a tap). A transform reads back exactly as it was set; the engine keeps it in 32-bit floats.
  - **`Input` is plain Dart** in package:fynkel: sizes in logical pixels, `drag` and `scroll` as `Vec2`, keys as `Object`s (Flutter's `LogicalKeyboardKey`s when fynkel_flutter fills it). It replaces fynkel_flutter's `FrameInput`, and `FynkelView` now runs `world.update(dt, input)` each frame, after `onFrame`.

### 3.3 Loading, and when an object appears

- **`onLoad()` is async**, and runs when the object is added. It's where an object gets its assets and adds its children.
- **An object appears when it and everything added during its load are loaded.** Until then it's inactive: not drawn, not hit, not updated. So the garden appears whole, never piece by piece, and the character never shows without its head. A parent's load waits for its children's.
- **The active flag is in the engine** (the one Rust change): an inactive object hides its subtree from the extract and from rays. Games use it too: `object.active = false` (Unity's `SetActive`).
- **A load that fails** destroys the object and reports the error through `world.errors` (a stream), and the parent's load goes on without it. A game that would rather stop can rethrow.
- **Destroyed while loading** (the player leaves before the sword is ready): the object is gone at once, its `onLoad` carries on to its next `await` and then finds the object destroyed (`add` and asset calls on it throw a `StateError` the loop catches and ignores), and whatever it loaded is released. Nothing appears, nothing leaks, and a test checks both.
- **Loading ahead.** `await assets.preload(['character.glb'])` loads without adding anything, so a later `Character()` appears on its first frame. 
- *Settled in step 2:*
  - **Hidden means left out of everything a view uses**: not drawn, and an inactive camera, sun or ambient light doesn't count either, so a level's camera can't take over the view while it loads. Rays pass through, as they hit only what's drawn.
  - **Each object keeps its own flag** (Unity's `activeSelf`); `activeInHierarchy` says whether it's shown. Turning a parent on again shows exactly the children that were on.
  - **An object can be made inactive before it's added** (`GameObject()..active = false`), like its other settings; step 4 uses that for loading.
  - **The flags live beside the hierarchy**, not in a component, and the per-frame pass that computes world matrices works out what's shown in the same walk. A world with every object on pays one `is_empty` a frame for it; turning objects on and off allocates nothing and rebuilds nothing.

### 3.4 Assets

- **An object asks for its own assets**, by name: `await assets.model('character.glb')`, `await assets.texture('grass.png')`. `assets` is available on every object (it's the world's).
- **The bytes come from a source the app gives** (`Future<Uint8List> Function(String name)`). FynkelApp and FynkelView use Flutter's asset bundle under `assets/` by default; a game can pass its own (files, the network). package:fynkel and the Rust engine still never touch files.
- **Shared by name.** Ten characters load `character.glb` once and share its meshes, materials and textures. Two calls in the same frame wait for the same load.
- **Owned by who asked.** Each object that asked holds a reference. When it's destroyed its references go; when an asset's last reference goes, it's freed completely: its objects first, then its materials, meshes and textures, then the model (stage 2's full unload, done by the engine). Memory after destroying the garden must come back to where it was before adding it.
- **`assets.model(name)` returns a new instance each call**: a GameObject tree to add, sharing the loaded data. The `Model` itself stays inside `Assets`. Code that builds meshes and textures itself (`Mesh.custom`, `Texture2D.rgba`) keeps working, and gives them to an object to own (`own(texture)`), so they're freed with it.
- *Settled in step 4:*
  - **`onLoad` returns `FutureOr<void>`**, so an object that loads nothing appears in the same `add`, as before. Only a load that returns a future hides its object until it's done.
  - **Loads run parents first**, once everything added is spawned. Objects added to an object before it was added, or during its load, are waited for alike; one added after it appeared loads by itself while it stays.
  - **The game's `active` is kept apart from loading**: set while the object loads, it's what the object takes when it appears. `activeInHierarchy` is false while it loads.
  - **A failed load with no one listening to `world.errors` is an uncaught error** of the zone the object was added in, as an unawaited future's would be, so a test fails and Flutter's error handler shows it. `loaded` fails with the same error.
  - **Once an object is destroyed, its load's errors are ignored**, whatever they are: anything the load does to an object that's gone fails because it's gone.
  - **A model's copy holds the model too**, as well as the object that asked for it, so a copy added somewhere else (as a root) keeps it alive. The copy's hold goes when the copy is destroyed, whether or not it was ever added.
  - **A load that fails isn't kept**, so asking again tries again. `preload` tells models from textures by their extension. `World(source:)` gives the source, and `world.assets` holds what it loads for the world's life.
  - **What can't be freed is reported, not thrown**: a texture still used by a material no one owns goes to `world.errors`, and destroying goes on. Materials are freed before meshes and textures. Built-in meshes stay, as they always have. `world.dispose()` frees what objects owned and the assets still held.

### 3.5 Flutter: one line to start, measuring outside

- **`FynkelView(root: Garden())`** makes the world, adds the root, runs `update` each frame and shows the active camera. It goes anywhere in a Flutter app, so menus and HUDs are Flutter widgets on top (Fynkel 1's lesson: game UI is Flutter).
- **`FynkelApp(root)`** is the whole app for a game that is only the view: `runApp(FynkelApp(Garden()))`.
- **The world is reachable** from any object (`world`) and from the view's state, for code outside the game that needs it (a HUD showing the score reads the game's own objects).
- **Measuring leaves the game's code.** `examples/garden/lib/main.dart` is the one line above; `lib/measured.dart` wraps the same root with package:fynkel_measure, and the tool runs that entry point (`-t lib/measured.dart`). The lifecycle check becomes generic: it destroys the root and adds a new one, N times, so it measures loading and freeing the whole scene, not only the character. The on-screen clock and the bench move into fynkel_measure.
- `FynkelView(world:, onFrame:)` stays for tests and tools that drive a world by hand, documented as the low-level way.
- *Settled in step 5:*
  - **The root is used once**, when the view is first built. A rebuild with a new root changes nothing, since the game changes level inside its world (`root.world`); to start over, the view gets a new `key`. A new world, or a switch between `root:` and `world:`, starts the view over, and the view's own world is disposed with it.
  - **The world is `FynkelViewState.world`**, the state being public for that (`GlobalKey<FynkelViewState>`).
  - **`bundleSource([bundle])`** reads `assets/<name>` from the app's bundle (or the one given); it's the default `source`. `FynkelApp(root, source:, title:)` is a `WidgetsApp` around the view, without Material.
  - **`MeasuredApp(Hello.new)`** takes a function that makes the root, since its checks make new ones. Its lifecycle points are `destroyed` and `added` (the root replaced in the same world), `created` (a new view, world and root) and the two resizes. It reports each load's time to the screen. The game's own timings join the bench through `timings:`.
  - **The tool builds the Mac's profile build itself**, of `lib/measured.dart` when the app has one (`-t`), so a run is never of stale code. On a phone `flutter run` gets the same `-t`.

### 3.6 The world, levels, and changing things

- **There are no scenes.** A level is a GameObject like any other (`Garden`), and so is anything inside it.
- **A world is the place objects live in**, not a level: everything live, the active camera, the shared assets, and the loop. A view shows one world, and `FynkelView(root:)` makes it. A world can hold several roots at once: a level, a player who outlives levels, a music object.
- **Any object can add and destroy others at any time**, and they can load what they need themselves: a character adds a `Sword()`, later destroys it and adds an `Axe()`. The sword's model is freed with it unless something else still uses it.
- **Changing level** is destroying one root and adding another: `garden.destroy(); world.add(Forest())`. To keep the old level on screen until the new one is ready, add the new one first (it stays inactive while it loads) and destroy the old one once it's there: `final forest = world.add(Forest()); await forest.loaded; garden.destroy();`. `loaded` completes when the object appears. Whatever should survive the change (the player, the score) lives in another root and is simply not destroyed (Unity's `DontDestroyOnLoad`, without a special case).

### 3.7 Working with other objects

- **Finding objects by type.** `world.all<Enemy>()` lists every live `Enemy`; `world.first<Player>()` gives one (or `null`); `ancestor<Level>()` gives the nearest `Level` above an object. Names stay for the parts of a model (`find('head')`). The world keeps a list per class added, so these don't walk the tree.
- **Talking to each other is plain Dart.** An object that has another calls its methods (`player.hurt(10)`), or takes a callback in its constructor. There's no event bus or message system to learn.
- **Showing game state in Flutter** uses what Flutter already has: a `ValueNotifier` on the object (`final score = ValueNotifier(0)`), shown by a `ValueListenableBuilder` over the view. The HUD needs nothing from Fynkel.
- **Many of a thing** (bullets, particles, a crowd of hundreds) shouldn't each be a class with its own `onUpdate`. One manager object moves them, as plain objects with only components, in its own `onUpdate`. The examples show it. Systems that run in Rust over thousands of objects can come later, behind the same manager.
- **Object input beyond taps** comes later: in stage 3, objects get `onTap`, and dragging, hovering and keys are read from the shared `input`. `onDrag` and the like wait for a game that needs them.

### 3.8 The garden after

- `main.dart`: one line. `garden.dart`: `Garden`, `Character`, `OrbitCamera` (a camera that reads `input.drag` and `input.scroll` in `onUpdate`), `Ground` (loads `grass.png`, adds a plane), `Props` (loads one model and adds a few at seeded places).
- The target: the garden's game code in **60 lines or fewer** (non-blank, non-comment), against 132 in `garden.dart` today plus the app's own part of `main.dart`, and nothing about measuring in either.
- hello becomes the smallest game: one class, a spinning cube, in about 15 lines.
- *Settled in step 6:*
  - **The garden is 111 lines, not 60** (`garden.dart`, non-blank, non-comment; `main.dart` is 4), from 146 and 127 counted the same way. The rest of the 60 would come from packing code in ways the formatter undoes: it puts each item of a broken list on its own line (the twelve prop names are 14 lines) and each closing bracket too. Accepted as it is (your call, 2026-10-09).
  - **coins is the second game** (`examples/coins`): two levels, a player who outlives them and carries a different model in each, coins found with `world.all<Coin>()`, a score in a `ValueNotifier` shown by a Flutter HUD, two mixins (`Spins`, `Bobs`) on one class, a logic-only child (`Collector`), and a `Swarm` moving 300 plain objects in its own `onUpdate`. 230 lines of game, 22 of HUD, 20 of `main.dart`.
  - **A model's part takes children** like any object: the player carries its item in `find('arm-right')`, and swapping it destroys the old one.
  - **coins changes level as §3.6 says**: it adds the next level, waits for `loaded`, then destroys the old one, so there's always a level on screen. Coins still loading aren't picked up: the `Collector` skips those not `activeInHierarchy`.
  - **`MeasuredApp(overlay:, change:)`**: `overlay` puts the game's HUD over its view (given the root, so each new root gets its own); `change` is a change the game makes again and again (coins' `nextLevel`), done when what it added has loaded. The lifecycle check makes it first, with a `changed` point after each.
  - **A helper isolate frees its scratch** (`inHelper`): each model, texture or background mesh load made one and left it, about 1.7 KB of native memory a load. What's left, about 3 KB a load on the Mac, is the Dart VM's own cost of an isolate, the same with `Isolate.run(() => 0)`.

## 4. Steps

Each step is built, tested, and merged on its own, in this order. Each
step's tests fail first where that makes sense.

**Step 1: objects you can extend.** ✅ `GameObject` non-final, built then
added (`add`, `world.add`), the id-to-object map, `destroy`, the same
instance from `find`, `children`, `parent` and rays. `world.spawn` and
`spawnModel` go; hello and the tests move to the new way. `all<T>`,
`first<T>`, `ancestor<T>`. *Done when* a subclass's fields survive a
round trip through `find` and a ray, the type lookups find what was
added and forget what was destroyed, and every stage 2 test passes on
the new API. Eight new tests (`test/object_test.dart`), nine mutants
killed; all 62 package tests and the examples' moved over. Numbers in §6.

**Step 2: the active flag.** ✅ `fynkel_world`, the C interface and Dart.
*Done when* an inactive object's subtree is missing from a golden image
and from rays, and the extract's budget and its zero-allocation test
still pass. The stage 1 golden, drawn with an inactive rig (camera,
lights and a cube in front of the camera) added before it, through C and
through Dart; rays, the extract, the random hierarchy test, and the
zero-allocation frame hiding and showing 143 objects; 13 mutants killed.
Numbers in §6.

**Step 3: the loop.** `world.update(dt, input)`, `onStart`, `onUpdate`,
`onDestroy`, `onTap` with its bubble; `Input` and `Tap` in package:fynkel,
filled by fynkel_flutter; `@mustCallSuper` on the lifecycle. *Done when*
tests fix the order in §3.2 (starts, parents first, added mid-update,
destroyed mid-update, the bubble, two mixins' updates both running), and
10,000 updating objects are measured on the Mac, the iPhone and the
Note10+ against the 1 ms budget. Thirteen tests in `test/loop_test.dart`
and one through `FynkelView`; 16 mutants, 15 killed (the one left only
frees memory). **The loop itself is in the budget everywhere; 10,000
objects each moving itself are not, on any device** (§6): the cost is
the transform's read and write through the C interface, one call each
per object, as stage 1 measured.

**Step 3b: transforms kept in Dart.** ✅ Each object's transform is kept
in Dart, so reading it costs nothing; a set is checked at once and the
frame's moves go to the engine in one call (`fy_transform_set_many`,
ABI 11: all checked before any is written, no allocation). *Done when*
reads make no call, a set throws the engine's own error where it's made,
the moves reach the engine before a frame, a raycast, a world transform
or a tap reads it, and the bench is measured again on all three devices.
Nine tests in `test/transform_test.dart`, one in the C interface's, and
the zero-allocation frame sends half its cubes this way; 18 mutants, 17
killed (the one left only frees memory). **Reading is now free everywhere, and moving 10,000 objects
is in the budget on the Mac and the iPhone; the Note10+ takes 2.8 ms**
(§6), of which 0.9 ms is the turn's own arithmetic in Dart.

**Step 4: loading and assets.** `onLoad`, inactive until loaded, load
errors, `Assets` with its source, sharing, ownership and release,
`preload`, `own`, `loaded`. *Done when* tests show a parent appears only with its
children loaded, two objects share one load, destroying the last
user frees exactly what was loaded (`ViewStats` back to where it was),
and an object destroyed while loading never appears and leaks nothing.
✅ 20 tests in `test/load_test.dart`: each of those, plus failed loads
(at once and later, with and without a listener), the order of loads,
the game's flag while loading, `own`, preloading, assets that can't be
freed, and freeing on `world.dispose()`. 28 mutants, 27 killed (the one
left frees a model that arrives after everyone left at once, rather than
when the garbage collector finds it). Adding 10,000 objects costs 4.1 ms
instead of 3.7 (§6).

**Step 5: Flutter and measuring.** `FynkelView(root:)`, `FynkelApp`, the
bundle as the default source, fynkel_measure from outside the app's
code, the generic lifecycle check. *Done when* a widget test runs a
small game from its root and hello's measurements match stage 1's.
✅ `test/fynkel_view_test.dart` runs a small game from its root: it loads
a texture through `bundleSource`, starts, updates, and is freed with its
world when the view goes; a failed load reaches `world.errors`; a new
root is ignored, a world in its place starts over; `FynkelApp` shows the
game. 9 mutants, 8 killed; the one left removed a line that changed
nothing, and the code no longer has it. hello is
`runApp(FynkelApp(Hello()))` plus `lib/measured.dart`; its numbers are in
§6.

**Step 6: the garden, rewritten, and a second game.** The garden shows
loading a level, tapping and a camera. `examples/coins` shows the rest:
two levels and the change between them, a player who outlives them and
swaps what it carries, coins found by type, a score shown by a Flutter
HUD through a `ValueNotifier`, a mixin and a logic-only child, and a
manager moving a few hundred objects. *Done when* the garden's game code
is 60 lines or fewer, both games run at the display rate on all three
devices, destroying and re-adding the whole garden passes the memory
cycles against `--baseline`, and changing coins' level back and forth
does too.

**Step 7: the stage 3 check.** A short report, as in stages 1 and 2.
*Done when* it's committed and you have played the garden and coins on
each device.

## 5. Decisions (2026-10-09)

1. **One class per object** for now. Behaviours on top can come later.
2. **`add` and `destroy`**, **`onUpdate` and `onTap`** (and so `onLoad`,
   `onStart`, `onDestroy`).
3. **Assets come from Flutter's bundle, under `assets/`, by default**,
   with any other source one argument away.
4. **Taps bubble up** to the parents until one handles them.
5. **hello is rewritten** as the smallest game.

## 6. Measurements

### Step 1: objects (Mac, M3 Max, `dart test`, JIT, best of 5)

| 10,000 cubes below one root | before (`spawn`, `parent =`) | after (`add`) |
|---|---:|---:|
| adding them | 3.1 ms | 4.0 ms |
| destroying the root | 0.2 ms | 1.1 ms |

Adding costs a Dart object, its description and its class's set for
each. Destroying walks the subtree once to forget the Dart objects below
(one call into the engine per object). Both happen when a level loads or
goes, not each frame; a single call that returns a subtree's ids would
take most of the destroy's cost away if a game ever needs it.

Step 4 makes every `add` run the object's load: adding the same 10,000
cubes takes 4.1 ms instead of 3.7 (best of 5 after 2 warm-ups, the same
day), and destroying them is unchanged at 1.5 ms.

### Step 2: the active flag (Mac, M3 Max, `extract_probe`, release)

Median of 500 steady frames, 10,000 drawn cubes, three runs each:

| | main | with the flag |
|---|---:|---:|
| 100 × 100 wide | 0.330 – 0.333 ms | 0.348 – 0.353 ms |
| 100 chains 100 deep | 0.340 – 0.350 ms | 0.370 – 0.374 ms |
| no tree (stage 1) | 0.205 – 0.221 ms | 0.222 – 0.233 ms |
| wide, 10 of the 100 trees hidden | – | 0.338 – 0.341 ms |

About 0.02 ms more for 10,000 objects (a flag per object in the matrices
pass, and the lookup of where each object stands), well inside the 1 ms
budget. The iPhone ran stage 2's extract as fast as the Mac; the phones
are measured again with the loop in step 3.

### Step 3: the loop (`measure bench`, profile builds, the UI isolate)

`world.update` with 10,000 objects in 100 trees of 100, median (slowest)
of 300 steady frames:

| 10,000 objects | Mac (M3 Max) | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---:|---:|---:|
| with nothing to do (the loop) | 0.02 (0.04) ms | 0.02 (0.04) ms | 0.07 (0.45) ms |
| reading their transforms | 0.51 (0.68) ms | 0.52 (0.66) ms | 2.03 (4.75) ms |
| moving themselves (`rotate`) | 1.47 (1.82) ms | 1.33 (1.78) ms | **6.10** (8.99) ms |

The loop costs next to nothing. Moving 10,000 objects one call at a time
misses the 1 ms budget everywhere, by six times on the Note10+: each
`rotate` reads the transform from the engine and writes it back
(stage 1 measured the same 1.25 ms on the Mac), and the Note10+ is 4 – 5
times slower per call. The extract and raycast numbers in the same runs
match stage 2's.

### Step 3b: transforms kept in Dart (`measure bench`, as step 3)

The same 10,000 objects; each time now includes sending the moves to the
engine, as the next frame would:

| 10,000 objects | Mac (M3 Max) | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---:|---:|---:|
| with nothing to do (the loop) | 0.03 (0.42) ms | 0.03 (0.37) ms | 0.07 (1.81) ms |
| reading their transforms | 0.05 (0.48) ms | 0.05 (0.41) ms | 0.14 (1.68) ms |
| moving themselves (`rotate`) | 0.80 (1.19) ms | 0.76 (1.04) ms | **2.76** (6.52) ms |

Step 3's 1.47, 1.33 and 6.10 ms for moving, and 0.51, 0.52 and 2.03 ms
for reading. Where moving's time goes, timed apart in the same runs
(Mac, iPhone, Note10+):

| per frame, 10,000 objects | Mac | iPhone | Note10+ |
|---|---:|---:|---:|
| the turn itself (`Transform.rotated`, plain Dart) | 0.30 ms | 0.27 ms | 0.88 ms |
| setting it (checked, kept, queued) | 0.20 ms | 0.18 ms | 0.51 ms |
| the engine's half of the send | 0.15 ms | 0.13 ms | 0.67 ms |

The engine's half was 0.25 ms on the Mac before `insert_all` kept each
object's entity from its check and wrote through `query_one_mut`. What
is left on the Note10+ is Dart's arithmetic and the engine's lookups of
10,000 objects, both slower there per object. (The slowest frames
include the garbage collector's.)


### Step 5: hello as a game (`measure`, profile builds, 2026-10-09)

hello is now `runApp(FynkelApp(Hello()))`, measured through
`lib/measured.dart` (`MeasuredApp(Hello.new)`). 60 s of the turning cube,
against stage 1's (docs/stage-1.md §7):

| | Mac, window 1600 × 1200 | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---|---|---|
| fresh fps (median, min) | **120**, 119 (stage 1: 120, 120) | **120**, 120 (120, 118) | **59.9**, 57.9 (59.9, 58.9) |
| coalesced, skipped, dropped | 0, 0, 1 (0, 0, 0) | 0, 0, 0 (0, 0, 0) | 0, 0, 0 (0, 0, 0) |
| slow Flutter frames | 1 raster (0) | 0 (0) | 1 build, 38 raster (0, 28 – 247) |
| Dart allocation per frame | 14.7 KB, mouse over the view (13.4); 7.8 KB without (7.1) | 13.3 KB (12.0) | 19.0 KB (16.4 – 18.2) |

The frame rate is stage 1's everywhere. Dart allocates about 1 KB a frame
more than in stage 1, and nearly all of it came before this step: main's
hello (step 4, the world driven through `onFrame`), measured the same
day, allocates 7.6 KB a frame on the Mac without the mouse and 13.2 KB on
the iPhone. Step 5 adds 0.1 – 0.25 KB.

**Memory**, 10 cycles of the new lifecycle check, each against
`--baseline` (a spinning Flutter box, the same steps):

| | Mac | iPhone | Note10+ |
|---|---|---|---|
| engine's GPU memory, at full size | 276.1 MB at every destroy and add; 276.1, then 279.7 MB from create 4 on | Metal 167.2 MB at every point (stage 1: 167.1 – 167.3) | Graphics 122.6 – 123.3 MB at every destroy and add; 119.2 MB at every create (stage 1: 119.2) |
| destroyed / added, total | 431.3 – 442.2 MB: one 8 MB step at cycle 3, then +1.9 MB over 7 cycles. Baseline: the same step, then +1.9 MB | 368.6 → 373.7 MB (+5.1). Baseline +5.5 | 252.7 → 262.6 MB (+9.9; +8.9 in a second run). Baseline +8.4 |
| created, total | 442.7 – 447.4 MB (+4.7, the GPU step included). Baseline +2.3 | 374.8 → 378.5 MB (+3.7). Baseline +3.8 | 260.5 → 265.2 MB (+4.7; +6.0). Baseline +3.3 (stage 1: +12.9 over 30, against +10.1) |
| resized, 0.6 / full | 0.5 / 0.9 MB ranges | +2.9 / +3.2 MB. Baseline +2.2 / +2.8 | +1.9 / +1.8 MB (+1.7 / +1.5). Baseline +1.8 / +1.5 |
| start to done | – | +12.7 MB. Baseline +12.9 | +15.3 MB from the first destroy. Baseline +10.2 |
| 10 rotations, landscape / portrait; 10 trips to the background | – | – | +5.2 / +4.3; +5.2 (+3.2 / +3.4; +2.4). Baseline +5.0 / +4.7; +2.7 |

The engine's GPU memory comes back whole every time the root is
destroyed, and when a view and its world go. On the Mac and the iPhone
what grows, grows by as much with Flutter alone. On the Note10+ it grows
about 5 MB more over the run than the baseline's: creating views, as in
stage 1 (Flutter's texture path), and in both runs about 2 MB of Native
memory over 10 destroys and adds (some 200 KB a cycle, against noise of
about 1 MB in the totals). A hello root is four objects and a material;
step 6 destroys and re-adds the whole garden, which would show a cost per
object or per asset many times over, so it's looked at there.

The Mac's one-off GPU step after the first new views is there in main's
hello too (+14.6 MB at its second create), and its
absolute GPU numbers follow the window's size, which differs from run to
run, so they can't be compared with stage 1's 271.7 MB.

**Loading**, reported by `MeasuredApp`: a new hello root is loaded within
a millisecond and on screen 17 – 25 ms later on the Mac and the iPhone
(two or three frames at 120 Hz) and 35 – 49 ms on the Note10+ (two or
three at 60 Hz). A new view with its world and root is on screen in
36 – 45 ms on the Mac, 57 – 69 ms on the iPhone and 353 – 389 ms on the
Note10+, most of it making the engine's view. The app's very first view
takes longer: its first tick holds the UI isolate for 3 – 20 ms on the Mac,
4 ms on the iPhone and 830 ms on the Note10+.
