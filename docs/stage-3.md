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
  fynkel_ffi/       fy_active_get / fy_active_set
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

### 3.2 The loop and its order

Each frame, `world.update(dt, input)` does, in this order:

1. **Taps.** Each tap is cast into the world from the active camera. The hit object's `onTap(tap)` is called; if it returns `false`, its parent's is, and so on up to the root, until one returns `true` (a DOM-style bubble). So a tap on the character's arm reaches `Character.onTap` without the game knowing the arm. *Decision 4.*
2. **Starts.** Each object that became live since the last frame gets `onStart()`, once, parents before children.
3. **Updates.** Each live object's `onUpdate(dt)`, parents before children, in the order they were added. Objects added during an update start next frame; objects destroyed during it get no more calls, even this frame.
4. **The frame.** The view copies the world for the render worker, as now.

Everyone reads the same input: `input.drag`, `input.scroll`, `input.keys` and so on, as `FrameInput` does today, through `input` on any object. The order is fixed and documented, and tests check it, so a game can rely on "my parent updated before me".

Only objects whose class overrides `onUpdate` are visited each frame. Dart can't see overrides, so `GameObject`'s own `onUpdate` takes the object off the update list the first time it's called: a class that doesn't override it is visited once, never again, and nothing has to be declared. Model nodes and plain objects get no Dart object at all until asked for, so they cost nothing per frame. The budget: 10,000 updating objects in trees, each moving itself, in under 1 ms on the Note10+.

### 3.3 Loading, and when an object appears

- **`onLoad()` is async**, and runs when the object is added. It's where an object gets its assets and adds its children.
- **An object appears when it and everything added during its load are loaded.** Until then it's inactive: not drawn, not hit, not updated. So the garden appears whole, never piece by piece, and the character never shows without its head. A parent's load waits for its children's.
- **The active flag is in the engine** (the one Rust change): an inactive object hides its subtree from the extract and from rays. Games use it too: `object.active = false` (Unity's `SetActive`).
- **A load that fails** destroys the object and reports the error through `world.errors` (a stream), and the parent's load goes on without it. A game that would rather stop can rethrow.
- **Destroyed while loading** (the player leaves before the sword is ready): the object is gone at once, its `onLoad` carries on to its next `await` and then finds the object destroyed (`add` and asset calls on it throw a `StateError` the loop catches and ignores), and whatever it loaded is released. Nothing appears, nothing leaks, and a test checks both.
- **Loading ahead.** `await assets.preload(['character.glb'])` loads without adding anything, so a later `Character()` appears on its first frame. 

### 3.4 Assets

- **An object asks for its own assets**, by name: `await assets.model('character.glb')`, `await assets.texture('grass.png')`. `assets` is available on every object (it's the world's).
- **The bytes come from a source the app gives** (`Future<Uint8List> Function(String name)`). FynkelApp and FynkelView use Flutter's asset bundle under `assets/` by default; a game can pass its own (files, the network). package:fynkel and the Rust engine still never touch files.
- **Shared by name.** Ten characters load `character.glb` once and share its meshes, materials and textures. Two calls in the same frame wait for the same load.
- **Owned by who asked.** Each object that asked holds a reference. When it's destroyed its references go; when an asset's last reference goes, it's freed completely: its objects first, then its materials, meshes and textures, then the model (stage 2's full unload, done by the engine). Memory after destroying the garden must come back to where it was before adding it.
- **`assets.model(name)` returns a new instance each call**: a GameObject tree to add, sharing the loaded data. The `Model` itself stays inside `Assets`. Code that builds meshes and textures itself (`Mesh.custom`, `Texture2D.rgba`) keeps working, and gives them to an object to own (`own(texture)`), so they're freed with it.

### 3.5 Flutter: one line to start, measuring outside

- **`FynkelView(root: Garden())`** makes the world, adds the root, runs `update` each frame and shows the active camera. It goes anywhere in a Flutter app, so menus and HUDs are Flutter widgets on top (Fynkel 1's lesson: game UI is Flutter).
- **`FynkelApp(root)`** is the whole app for a game that is only the view: `runApp(FynkelApp(Garden()))`.
- **The world is reachable** from any object (`world`) and from the view's state, for code outside the game that needs it (a HUD showing the score reads the game's own objects).
- **Measuring leaves the game's code.** `examples/garden/lib/main.dart` is the one line above; `lib/measured.dart` wraps the same root with package:fynkel_measure, and the tool runs that entry point (`-t lib/measured.dart`). The lifecycle check becomes generic: it destroys the root and adds a new one, N times, so it measures loading and freeing the whole scene, not only the character. The on-screen clock and the bench move into fynkel_measure.
- `FynkelView(world:, onFrame:)` stays for tests and tools that drive a world by hand, documented as the low-level way.

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

## 4. Steps

Each step is built, tested, and merged on its own, in this order. Each
step's tests fail first where that makes sense.

**Step 1: objects you can extend.** `GameObject` non-final, built then
added (`add`, `world.add`), the id-to-object map, `destroy`, the same
instance from `find`, `children`, `parent` and rays. `world.spawn` and
`spawnModel` go; hello and the tests move to the new way. `all<T>`,
`first<T>`, `ancestor<T>`. *Done when* a subclass's fields survive a
round trip through `find` and a ray, the type lookups find what was
added and forget what was destroyed, and every stage 2 test passes on
the new API.

**Step 2: the active flag.** `fynkel_world`, the C interface and Dart.
*Done when* an inactive object's subtree is missing from a golden image
and from rays, and the extract's budget and its zero-allocation test
still pass.

**Step 3: the loop.** `world.update(dt, input)`, `onStart`, `onUpdate`,
`onDestroy`, `onTap` with its bubble; `Input` and `Tap` in package:fynkel,
filled by fynkel_flutter; `@mustCallSuper` on the lifecycle. *Done when*
tests fix the order in §3.2 (starts, parents first, added mid-update,
destroyed mid-update, the bubble, two mixins' updates both running), and
10,000 updating objects are measured on the Mac, the iPhone and the
Note10+ against the 1 ms budget.

**Step 4: loading and assets.** `onLoad`, inactive until loaded, load
errors, `Assets` with its source, sharing, ownership and release,
`preload`, `own`, `loaded`. *Done when* tests show a parent appears only with its
children loaded, two objects share one load, destroying the last
user frees exactly what was loaded (`ViewStats` back to where it was),
and an object destroyed while loading never appears and leaks nothing.

**Step 5: Flutter and measuring.** `FynkelView(root:)`, `FynkelApp`, the
bundle as the default source, fynkel_measure from outside the app's
code, the generic lifecycle check. *Done when* a widget test runs a
small game from its root and hello's measurements match stage 1's.

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

To come, step by step.
