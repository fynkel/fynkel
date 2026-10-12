# Writing a game with Fynkel

This guide teaches Fynkel from the smallest game up. Each section adds one
idea, with code that compiles as it stands (`tools/check_guide.py` checks
it). If you know Unity, most of it will look familiar: Fynkel works the
way Unity works, in Dart, and §20 maps one to the other.

The finished examples are in `examples/`: `hello` (one turning cube),
`garden`, `coins` and `arena` (a small game using everything here).
`examples/lab` has a scene for each feature, to try by hand.

## 1. A new game

You need Flutter 3.47 or later and Rust ([rustup](https://rustup.rs)); the
repository's `rust-toolchain.toml` picks the version. The engine is built
from source the first time you run a game, and again only when it changes.

Make a Flutter app and add Fynkel to it:

```sh
flutter create --platforms macos,ios,android --empty my_game
```

```yaml
# my_game/pubspec.yaml
dependencies:
  flutter:
    sdk: flutter
  fynkel:
    path: <the repository>/packages/fynkel
  fynkel_flutter:
    path: <the repository>/packages/fynkel_flutter
```

Then run it as any Flutter app: `flutter run -d macos`, or on a phone
(`flutter devices` lists them). `flutter build apk`, `ipa` and `macos`
make the app to ship. On Android, Fynkel runs on 64-bit devices, which is
nearly all since Android 11; a 32-bit one gets an error saying so.

## 2. Hello

The whole of `lib/main.dart` for a lit, turning cube:

```dart
// lib/main.dart
import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Hello.new));

class Hello extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 1.5, 4), rotation: Quat.lookRotation(Vec3(0, -1.5, -4))),
        components: [Camera()],
      ),
    );
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.3))),
        components: [Light.sun()],
      ),
    );
    add(Cube());
  }
}

class Cube extends GameObject {
  Cube() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]);

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta);
  }
}
```

- **Everything in the game is a `GameObject`**, in a tree. `Hello` is the
  root; `FynkelApp(Hello.new)` makes one and shows it full screen.
- **An object has a transform** (where it is, relative to its parent) and
  **components**: a `Camera` to see from, a `Light`, a `MeshRenderer` to
  be drawn.
- **`onLoad`** is where an object adds what it's made of. **`onUpdate`**
  runs every frame. `time.delta` is the frame's time in seconds, so the
  cube turns one radian a second whatever the frame rate.
- **Always call `super`** in the `on…` methods.

Y is up, and a camera looks down its −Z. Lengths are metres and angles
radians everywhere, the camera's and the joints' too: `90.deg` writes
one in degrees, and `angle.inDegrees` reads one back.

## 3. Objects

An object can be built in one expression, the way a Flutter widget tree
is, or made in a subclass:

```dart
GameObject lamp() => GameObject(
  name: 'lamp',
  transform: Transform(position: Vec3(2, 0, 0)),
  components: [MeshRenderer(Mesh.cube(), Material(color: Color.white))],
  children: [
    GameObject(
      name: 'bulb',
      transform: Transform(position: Vec3(0, 1, 0), scale: Vec3.one * 0.3),
      components: [MeshRenderer(Mesh.sphere(), Material(emissive: Color(0xFFFFE0A0), emissiveIntensity: 4))],
    ),
  ],
);
```

**A subclass is what an object is**: a `Player`, an `Enemy`, a `Level`.
It holds its state in fields, builds its parts in its constructor or
`onLoad`, and comes back typed wherever the game finds it:

```dart
class Crate extends GameObject {
  Crate(Vec3 at)
    : super(
        transform: Transform(position: at),
        components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFB08040)))],
      );

  int hits = 0;
}

class Yard extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    for (var i = 0; i < 3; i++) {
      add(Crate(Vec3(i * 1.5, 0.5, 0)));
    }
    add(lamp());
  }

  @override
  void onStart() {
    super.onStart();
    final crates = children.whereType<Crate>().toList(); // typed: they're Crates
    crates.first.hits++;
    find('lamp')?.find('bulb')?.active = false; // by name, below this one
    crates.last.destroy(); // with everything below it
  }
}
```

- **`add(child)`** puts an object below this one and returns it, typed:
  `final crate = add(Crate(at));`. The world's root is added with
  `world.add`; `FynkelApp` does that for you.
- **`destroy()`** takes an object and everything below it out of the game.
  A field still pointing at it says `isDestroyed`; reading its transform
  then throws a clear error instead of giving a stale value.
- **`active = false`** hides an object and everything below it: not drawn,
  not touched, not updated. `activeInHierarchy` says whether it and all its
  parents are on.
- **Finding**: `parent`, `children`, `find(name)` below this one,
  `ancestor<T>()` the nearest above of a type, and across the whole game
  `world.all<T>()`, `world.first<T>()` and `world.only<T>()` (which throws
  unless there's exactly one). These find by type, so a game rarely needs
  names or tags. As Unity's, they leave out objects that are turned off
  (and the components on them) unless `includeInactive: true`.

## 4. Behaviours

**A behaviour is what an object does**, written once and put on any
object: health, a spin, a flash when hit. It's a `Behaviour` subclass,
added in `components:` or with `addComponent`, and it gets every call an
object gets (`onStart`, `onUpdate`, …):

```dart
class Spin extends Behaviour {
  Spin({this.speed = 1});

  double speed;

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, speed * time.delta); // its object's transform
  }
}

class Coin extends GameObject {
  Coin() : super(components: [MeshRenderer(Mesh.sphere(), Material(color: Color(0xFFFFC030))), Spin(speed: 3)]);

  void stop() => get<Spin>()?.enabled = false; // still there, but not called
}
```

From a behaviour, its object is near: `gameObject`, and the same
shortcuts an object has (`transform`, `position`, `get<T>()`, `world`,
`time`, `input`, `assets`, `start`, `on`).

- **`addComponent(b)`** puts a behaviour on at any time and returns it;
  **`removeComponent(b)`** takes it off for good.
- **`get<T>()`** gives the first component of type `T` (or a subtype, or an
  interface it implements), or null; `getAll<T>()` all of them;
  `has<T>()`. `getInParent<T>()` and `getInChildren<T>()` look up or down
  the tree too.
- **`enabled = false`** stops its calls, its routines and its tweens
  until it's turned back on.
- A destroyed object still has its components, to read: a dead enemy's
  `get<Health>()` still says how much it took.

**Which one?** If a subclass's `onUpdate` would make sense on another kind
of object, it belongs in a behaviour. Keep a class for what an object *is*
(its state, its parts, its typed fields), and behaviours for what it
*does* that others could do too, or that comes and goes while it plays.

## 5. The engine's components

`Camera`, `Light` and `MeshRenderer` are components too, found with
`get<T>()` and changed in place. Each change goes to the engine at once:

```dart
class Beacon extends GameObject {
  Beacon()
    : super(
        components: [
          Light.sun(intensity: 2),
          MeshRenderer(Mesh.sphere(), Material(color: Color.rgb(0.2, 0.6, 1), roughness: 0.2)),
        ],
      );

  void dim() {
    get<Light>()!.intensity = 0.5;
    get<MeshRenderer>()!.material!.color = Color.rgb(0.1, 0.2, 0.4);
  }
}
```

- **Meshes**: `Mesh.cube()`, `Mesh.sphere()`, `Mesh.capsule()`,
  `Mesh.cylinder()`, `Mesh.plane()` and `Mesh.quad()`, Unity's shapes and
  sizes (the capsule and cylinder are 2 high, the quad stands facing +Z),
  or your own with `Mesh.custom(positions:, indices:, …)`. Models come
  from files (§12).
- **Materials** are physically based: `color`, `metallic`, `roughness`,
  `emissive` and `emissiveIntensity` (lit from within), and textures.
  One material can be shared by many objects; change it and all of them
  change.
- **Lights**: `Light.sun` (from the object's forward) and `Light.ambient`.
- **Camera**: `fovY` (radians: `60.deg`), `near`, `far`, `clearColor`,
  `orthographic` with a `size`, and a `cullingMask` (§13). The view draws
  from the active one, `world.camera` (Unity's `Camera.main`).

You never free a mesh, material or texture: the engine's copy goes when
nothing in the game reaches it any more.

## 6. The frame, the lifecycle and time

Each object and behaviour gets these calls, in the order they were added:

| Call | When |
|---|---|
| `onLoad` | Once, before it appears. It may be `async` (to load assets, §12); the object appears when everything in it has loaded. |
| `onEnable` | Each time it starts being shown: when it appears, and when it or a parent is turned back on. |
| `onStart` | Once, at the start of its first frame. |
| `onFixedUpdate` | Every fixed step: 50 times a second, whatever the frame rate. For forces and what physics needs. |
| `onUpdate` | Every frame. |
| `onLateUpdate` | Every frame, after every `onUpdate`, the routines and the tweens: for what follows where things ended up. |
| `onDisable` | Each time it stops being shown, and before `onDestroy`. |
| `onDestroy` | Once, when it's destroyed. |

A frame goes: input → starts → fixed steps → updates → routines and
tweens → late updates → cameras → drawing. This order is fixed and
tested.

**Fixed steps** cut the game's time into equal steps of `time.fixedDelta`
(1/50 s), so what runs in them goes the same at 30 fps and at 120. A
frame runs as many as its time holds: at 120 fps most frames have none,
at 30 some have two. Inside a step `time.delta` is `time.fixedDelta`. To
draw something a step moves without it stepping, draw it between where
the last two steps left it, `time.fixedAlpha` of the way (0 to 1).

**`time`** is on every object and behaviour:

- `time.delta`: this frame's seconds, scaled. Multiply every speed by it.
- `time.scale`: 0 pauses everything that uses `delta` (moves, waits,
  tweens); 0.5 is slow motion.
- `time.unscaledDelta`, `time.sinceStart`, `time.unscaledSinceStart`,
  `time.frame`.
- `time.maxDelta` (1/3 s): a long hitch counts as at most this, so nothing
  jumps through walls.
- `time.fixedDelta`, `time.fixedAlpha`, `time.inFixedStep`: the fixed
  steps.

**Errors don't stop the game.** An exception in any of these calls (or a
routine, a handler, a tween) goes to `world.errors` with its object, and
the rest of the frame goes on. In a debug build the view also shows it
over the game, with the object and the line in your code it came from,
and prints it to the console, so a broken game is never just a black
screen; a widget test fails on it. In a release build, or with
`FynkelView(showErrors: false)`, it's the game's to handle: listen to
`world.errors`, or it's an uncaught error.

## 7. Moving things

On objects and behaviours alike:

```dart
class Drone extends Behaviour {
  final _follow = SmoothDamp3(smoothTime: 0.3);

  @override
  void onUpdate() {
    super.onUpdate();
    translate(Vec3.forward * time.delta); // the way it looks
    rotate(Vec3.up, 0.5 * time.delta); // about its own up
    final player = world.first<Pilot>();
    if (player == null) return;
    lookAt(player.position);
    position = _follow.step(position, player.position + Vec3(0, 3, 0), time.delta);
  }
}

class Pilot extends GameObject {}
```

- **As Unity's**: `position` and `rotation` are in the world, whatever
  the object's parents; `localPosition`, `localRotation` and `localScale`
  are relative to its parent, and `lossyScale` is its size in the world.
  `forward`, `right` and `up` are in the world. For a root the two are the
  same.
- `translate` and `rotate` go by the object's own axes; give
  `relativeTo: Space.world` for the world's. `lookAt(point)`,
  `transformPoint` and `inverseTransformPoint` too.
- `parent = other` keeps the object where it is in the world, as Unity's;
  `setParent(other, worldPositionStays: false)` keeps its local transform
  instead, so it moves with its new parent.
- The local transform is one value, a position, a rotation and a scale:
  `transform = Transform(position: …, rotation: …)` sets it at once, and
  `worldTransform` reads the world's.
- **A rotation is a `Quat`**, as in Unity: `Quat.lookRotation(direction)`
  to face a way, `Quat.euler(pitch, yaw, roll)` by angles (radians), and
  `Quat.axisAngle(axis, angle)`. A camera at `eye` looking at `target` is
  `Transform(position: eye, rotation: Quat.lookRotation(target - eye))`.
- Unity's Mathf: `lerp`, `inverseLerp`, `clamp01`, `moveTowards`,
  `repeat`, `pingPong`, `randomInSphere`, `randomOnCircle`, and
  `Vec3.lerp`, `Vec3.moveTowards`, `Quat.slerp`, `Color.lerp`.
  `SmoothDamp` and `SmoothDamp3` keep their velocity between frames.

## 8. Tweens

A tween moves something over time, with an ease, and stops by itself when
its object is destroyed:

```dart
class Door extends GameObject {
  Door() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFF806040)))]);

  Motion open() => moveBy(Vec3(0, 2, 0), seconds: 0.6, ease: Ease.outBack);

  void wobble() {
    punchScale(Vec3.one * 0.2, seconds: 0.3);
    sequence([colorTo(Color.white, seconds: 0.05), colorTo(Color(0xFF806040), seconds: 0.3)]);
  }

  void hover() => moveBy(Vec3(0, 0.2, 0), seconds: 1, ease: Ease.inOutSine, loops: -1, yoyo: true);

  void fadeLight() => tween(1.0, 0.0, seconds: 2, onUpdate: (v) => get<Light>()?.intensity = v);
}
```

- **Helpers**: `moveTo`, `moveBy`, `rotateTo`, `rotateBy`, `lookAtOver`,
  `scaleTo`, `colorTo`, `punchScale`, `shake`. Each takes `seconds:` or
  `speed:`. `moveTo`, `moveBy` and `rotateTo` are in the world, as
  `position`; `localMoveTo` and `localRotateTo` relative to the parent;
  `scaleTo` sets `localScale`, and `rotateBy` turns about the object's own
  axis.
- **`tween(from, to, …, onUpdate:)`** for any value: `double`, `Vec2`,
  `Vec3`, `Quat`, `Color`, or anything with a `lerp:` you give.
- **Options**: `ease:` (`Ease.outQuad` by default; any `double
  Function(double)`, so Flutter's `Curves.x.transform` too), `delay:`,
  `loops:` (-1 for ever), `yoyo:`, `unscaled:` (goes on while paused),
  `onComplete:`.
- **`sequence([...])`** plays one after another, **`together([...])`** all
  at once.
- Each returns a **`Motion`**: `stop()`, `complete()`, and it's a future,
  so `await open();` waits for it to end.

## 9. Routines

A routine is `async` code that waits in game time, Unity's coroutine in
plain Dart:

```dart
class Spawner extends GameObject {
  final score = ValueNotifier(0);
  final clock = ValueNotifier(false);

  @override
  void onStart() {
    super.onStart();
    startRoutine(() async {
      for (var wave = 1; ; wave++) {
        for (var i = 0; i < wave * 3; i++) {
          add(Crate(Vec3(i.toDouble(), 0.5, -5)));
          await wait(0.4);
        }
        await waitUntil(() => world.all<Crate>().isEmpty);
        await wait(2);
      }
    });
  }

  /// Ten points in ten seconds, or not.
  Future<bool> beatTheClock() async {
    clock.value = true;
    try {
      return await race([
        startRoutine(() async {
          await waitUntil(() => score.value >= 10);
          return true;
        }),
        startRoutine(() async {
          await wait(10);
          return false;
        }),
      ]);
    } finally {
      clock.value = false; // runs even when the routine is stopped
    }
  }

  Future<void> bonusRound() async {
    final won = await startRoutine(beatTheClock); // a routine returns a value
    if (won) score.value += 100;
  }
}
```

- **`startRoutine(() async {...})`** runs one, on an object or behaviour
  (Unity's `StartCoroutine`); `stopAllRoutines()` stops the ones it
  started. It
  **stops by itself** when its object is destroyed (or its behaviour
  removed), and **pauses** while its object is hidden or the behaviour
  isn't enabled.
- **Waits**: `await nextFrame`, `await fixedStep` (right after the next
  fixed step's `onFixedUpdate` calls), `await wait(seconds)`
  (`unscaled: true` to go on while paused), `await waitUntil(() => test)`,
  `await waitWhile(() => test)`, a tween, a signal's `next`, or any
  `Future`.
- **`race([...])`** gives the first to end and stops the rest (routines,
  waits and tweens); **`waitAll([...])`**
  waits for every one. `race([x, wait(3)])` is a timeout.
- `startRoutine` returns a **`Routine`**: `stop()`, `isRunning`, and it's a
  future of its result. When it's stopped, its `finally` blocks run, and
  routines started inside it stop too.
- An error a routine doesn't catch goes to `world.errors`.

## 10. Signals: telling others

A `Signal` is an event any code can listen to. Listen with **`on`**, and
the listening ends by itself when the listener is destroyed, so nothing
ever calls into a dead object:

```dart
class Health extends Behaviour {
  Health(this.max) : left = max;

  final int max;
  int left;
  final died = Signal<GameObject>();

  void damage(int amount) {
    if (left == 0) return;
    left = (left - amount).clamp(0, max);
    if (left == 0) died.emit(gameObject);
  }
}

class Scoreboard extends GameObject {
  int kills = 0;

  void watch(GameObject enemy) {
    on(enemy.get<Health>()!.died, (_) => kills++);
  }

  Future<void> waitForBoss(GameObject boss) async {
    await boss.get<Health>()!.died.next; // in a routine: one of its waits
  }
}
```

- `emit(value)` calls every listener at once, in order. A handler that
  throws goes to `world.errors`, and the others still hear.
- `signal.listen(handler)` listens without an owner: cancel it yourself.
- `await signal.next` waits for the next value.

## 11. Input

`input` is on every object and behaviour. Poll it, as Unity's `Input`:

```dart
class Walker extends GameObject {
  @override
  void onUpdate() {
    super.onUpdate();
    final move = input.axis2('move'); // W, A, S, D and the arrows
    translate(Vec3(move.x, 0, -move.y) * (4 * time.delta));
    if (input.action('jump').wasPressed) jump(); // space
    if (input.wasPressed(KeyCode.escape)) time.scale = time.scale == 0 ? 1 : 0;
    if (input.isPressed(MouseButton.right)) rotate(Vec3.up, time.delta);
  }

  void jump() => moveBy(Vec3(0, 1, 0), seconds: 0.25, yoyo: true, loops: 2);
}
```

Or listen to events, Lean Touch's way:

```dart
class Gestures extends GameObject {
  @override
  void onStart() {
    super.onStart();
    on(input.onTap, (finger) {
      if (finger.over case final hit?) debug.sphere(hit.point, 0.2, seconds: 1);
    });
    on(input.onSwipe, (swipe) => print('swiped ${swipe.direction} at ${swipe.speed} px/s'));
    on(input.onPinch, (pinch) => localScale = localScale * pinch.scale);
    on(input.onKeyDown, (press) => print('${press.key} ${press.shift ? 'with shift' : ''}'));
  }
}
```

Or let the object under the finger handle it, as Unity's pointer
interfaces do:

```dart
class Button3D extends GameObject {
  Button3D() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFF40A060)))]);

  @override
  bool onClick(Finger finger) {
    punchScale(Vec3.one * 0.3, seconds: 0.2);
    return true; // taken: the parent doesn't get it
  }

  @override
  void onPointerEnter(Finger finger) => get<MeshRenderer>()!.material!.color = Color.white;

  @override
  void onPointerExit(Finger finger) => get<MeshRenderer>()!.material!.color = Color(0xFF40A060);
}
```

- **A `Finger`** is one touch, or the mouse: `position`, `startPosition`,
  `delta`, `totalDelta`, `velocity`, `age`, `tapCount`, `isPressed`,
  `isDragging`, and `over` (what's under it in the world: `gameObject`,
  `point`, `normal`). `input.fingers` lists them.
- **Events**: `onFingerDown`, `onFingerUp`, `onFingerMove`, `onHover`,
  `onTap` (a double tap is two, the second with `tapCount` 2),
  `onLongPress`, `onDragStart`, `onDrag`, `onDragEnd`, `onSwipe`,
  `onPinch`, `onTwist`, `onScroll`, `onKeyDown`, `onKeyUp`.
- **On objects**: `onPointerDown`, `onPointerUp`, `onPointerEnter`,
  `onPointerExit`, `onClick`, `onLongPress`, `onDragStart`, `onDrag`,
  `onDragEnd`, on behaviours too. They go to the object under the finger,
  then up to its parents until one returns true. What takes a drag's start
  gets the rest of that drag.
- **Keys** are `KeyCode`s, by where they are on the keyboard, so W, A, S,
  D stay put on any layout. `input.bindAxis2` and `input.bindAction` add
  your own names.
- A Flutter widget over the game (a HUD button) takes its own taps; they
  don't reach the game.

## 12. Assets

Models (`.glb`) and textures (`.png`, `.jpg`) are loaded by name, from the
app's `assets/` folder (list it under `flutter: assets:` in
`pubspec.yaml`):

```dart
class Tree extends GameObject {
  Tree(Vec3 at) : super(transform: Transform(position: at));

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.model('props/tree_oak.glb'));
    model.find('leaves')?.get<MeshRenderer>()?.material?.color = Color.rgb(0.8, 0.5, 0.2);
    final bark = await assets.texture('bark.png');
    add(
      GameObject(
        transform: Transform(scale: Vec3(0.3, 2, 0.3)),
        components: [MeshRenderer(Mesh.cube(), Material(baseColorTexture: bark))],
      ),
    );
  }
}
```

- `assets.model(name)` gives a new copy of the model, a tree of objects
  sharing its meshes and textures with every other copy. Find its parts by
  name.
- Each file is loaded once, off the UI thread, and shared. It's freed when
  the last object that asked for it is destroyed.
- `assets.preload([...])` loads ahead, so a level appears at once later.
- A level that's `async` in `onLoad` appears only when all of it is there:
  `await add(level).loaded` waits for that.

## 13. Cameras, layers and debug drawing

**Cameras** work as Cinemachine's. Put a `VirtualCamera` on objects; the
enabled one with the highest `priority` drives the view's camera, and a
change of priority blends to it:

```dart
class CameraRig extends GameObject {
  CameraRig(GameObject player) : _player = player;

  final GameObject _player;
  late final VirtualCamera close, wide;

  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(components: [Camera()])); // the view's camera
    close = add(
      GameObject(components: [VirtualCamera(priority: 1), Follow(_player, offset: Vec3(0, 3, 6), damping: 0.2), LookAt(_player)]),
    ).get<VirtualCamera>()!;
    wide = add(GameObject(components: [VirtualCamera(fovY: 70.deg), Orbit(_player, distance: 15)])).get<VirtualCamera>()!;
  }

  void zoomOut() {
    world.cameras.defaultBlend = Blend(1.5, ease: Ease.inOutSine);
    wide.priority = 2;
  }

  void hit() => world.cameras.shake(0.3);
}
```

- **Rigs** are behaviours: `Follow(target, offset:, damping:,
  lookAhead:)`, `LookAt(target)`, and `Orbit(target)`, which the player
  turns with a drag and zooms with a pinch or the wheel. A rig works on a
  plain camera too, without a `VirtualCamera`.
- `world.cameras.shake(strength)`, `setBlend(blend, from:, to:)`, `live`.
- **Between the screen and the world**: on a `Camera`, such as
  `world.camera!`, `screenPointToRay(point)`, `screenPointToPlane(point,
  Plane.ground)` and `worldToScreenPoint(point)`; `world.raycast(ray)`
  gives what a ray hits first (a `RayHit`: `gameObject`, `point`,
  `normal`, `distance`).
- **A camera that follows a physics body** sees each fixed step's jump:
  a body steps 50 or 100 times a second, the screen draws at 60 or 120,
  and the camera moves smoothly, so the body seems to jitter. Give the
  body `interpolation: RigidbodyInterpolation.interpolate` (§14), as in
  Unity.

**Layers** keep things apart. Each object has a `layer`, 0 to 31; a
camera's `cullingMask` leaves layers undrawn, `world.raycast(ray,
layers:)` goes through some, and `input.layers` makes taps go through
some (sparks and other effects):

```dart
const effects = 1;

class Sparkle extends GameObject {
  Sparkle() : super(layer: effects, components: [MeshRenderer(Mesh.sphere(), Material(emissive: Color.white))]);

  @override
  void onStart() {
    super.onStart();
    input.layers = LayerMask.except([effects]); // taps go through sparkles
  }
}
```

**Debug drawing**, for a frame or for `seconds:`, on top of everything:
`debug.line(a, b)`, `debug.ray(origin, direction)`, `debug.box`,
`debug.sphere`, `debug.path`, each with a `color`. It's off in release
builds.

## 14. Physics: colliders, rigidbodies, queries and callbacks

Physics is a package, `fynkel_physics`, on Rapier. Add it to the
pubspec, next to `fynkel`, and a game without it ships none of it:

```yaml
  fynkel_physics:
    path: <the repository>/packages/fynkel_physics
```

**Colliders** are behaviours, as Unity's: `BoxCollider(size:)`,
`SphereCollider(radius:)` and `CapsuleCollider(radius:, height:,
direction:)`, each with `center` and `isTrigger`. Their sizes are in the
object's own space, and they follow its world position, rotation and
scale (a sphere takes the largest axis). Their defaults fit
`Mesh.cube()`, `Mesh.sphere()` and `Mesh.capsule()`. A collider counts while it's enabled
and its object shown. Each has a `material`: `PhysicsMaterial(
dynamicFriction:, bounciness:, frictionCombine:, bounceCombine:)`, Unity's
0.6 and 0 by default. A collider with no `Rigidbody` is static; a game
moves it by moving its object.

**A `MeshCollider`** takes a mesh's own shape: `MeshCollider(mesh:)`, or
with no mesh its object's `MeshRenderer`'s. Its triangles suit the ground
and what stands on it (static, or on a kinematic rigidbody); with
`convex: true` it's the mesh's hull, which a falling body can be (on a
dynamic rigidbody a mesh is its hull either way). The object's scale
stretches it on each axis. A model's parts each get one with
`MeshCollider.addToParts`:

```dart
// lib/rock.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A rock model that balls roll over, along its real shape.
class Rock extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    MeshCollider.addToParts(add(await assets.model('rock.glb')));
  }
}
```

**A `Rigidbody`** makes its object move with physics, as Unity's: give it
a collider and it falls, tumbles and lands. Colliders on its children
that have no rigidbody of their own are part of the same body, and the
body's mass is spread over them. It has Unity's `mass`, `useGravity`,
`isKinematic`, `linearDamping`, `angularDamping`, `linearVelocity`,
`angularVelocity`, `centerOfMass`, `maxAngularVelocity`, `constraints`
(`RigidbodyConstraints.freezeRotation | .freezePositionY`, …),
`interpolation` and `collisionDetectionMode`. Push it with `addForce`
(and its `ForceMode`s), `addTorque`, `addRelativeForce`,
`addForceAtPosition` and `addExplosionForce`, in `onFixedUpdate`:

```dart
// lib/ball.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Rolls its ball with W, A, S, D or the arrows, and jumps on space.
class Roll extends Behaviour {
  late final Rigidbody body = gameObject.get<Rigidbody>()!;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    final move = input.axis2('move');
    body.addForce(Vec3(move.x, 0, -move.y) * 10);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (input.action('jump').wasPressed) body.addForce(Vec3(0, 5, 0), mode: ForceMode.impulse);
  }
}

GameObject ball() => GameObject(
  transform: Transform(position: Vec3(0, 3, 0)),
  components: [
    MeshRenderer(Mesh.sphere(), Material()),
    SphereCollider(material: PhysicsMaterial(bounciness: 0.5)),
    Rigidbody(mass: 2, interpolation: RigidbodyInterpolation.interpolate),
    Roll(),
  ],
);
```

As in Unity, **setting a body's transform teleports it**; to move it
through the world use forces, or `movePosition` and `moveRotation` for a
kinematic body, which pushes what it meets. A body's object can have a
parent: physics writes its pose back relative to it. With
`RigidbodyInterpolation.interpolate`, what's drawn is between the body's
last two steps, so it moves smoothly at 120 fps on 50 steps a second;
`rigidbody.position` reads where physics has it. A small, fast body can
pass through a thin one between steps: Rapier sweeps every fast body
against static colliders, and `CollisionDetectionMode.continuous` sweeps
it against bodies too. `physics.gravity`, `defaultSolverIterations` and
`sleepThreshold` are the world's settings.

**The step runs between frames**, on the physics' own thread: it starts
when a frame is done, and its results are written at the start of the
next, before `onFixedUpdate` and the updates. Queries in the updates
never wait for it, and see the world that's drawn. The costs: a force
shows a frame later, and `await fixedStep` resumes before that step has
run. `physics.stepBetweenFrames = false` steps
within the frame instead, as Unity does.

**Queries** are on `physics`, which every object and behaviour has:
`raycast`, `sphereCast`, `boxCast`, `capsuleCast` (and `…All`, nearest
first), `overlapSphere`, `overlapBox`, `overlapCapsule`, and `checkSphere`,
`checkBox`, `checkCapsule`. Each takes `layers:` and `triggers:`
(`QueryTriggers.ignore` goes through triggers), and sees colliders where
their objects are now, moved this frame or not:

```dart
// lib/sight.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

const walls = 2;

/// Sees the player when nothing on the walls layer is in between.
class Sight extends Behaviour {
  Sight(this.player);

  final GameObject player;
  var seesPlayer = false;

  @override
  void onUpdate() {
    super.onUpdate();
    final to = player.position - gameObject.position;
    final hit = physics.raycast(
      Ray(gameObject.position, to),
      maxDistance: to.length,
      layers: LayerMask.of([walls]),
      triggers: QueryTriggers.ignore,
    );
    seesPlayer = hit == null;
  }
}

/// A wall: a cube with a box collider, on the walls layer.
GameObject wall(Vec3 at) => GameObject(
  layer: walls,
  transform: Transform(position: at, scale: Vec3(4, 2, 0.2)),
  components: [MeshRenderer(Mesh.cube(), Material()), BoxCollider()],
);
```

A hit is a `RaycastHit`, as Unity's: `point`, `normal`, `distance`,
`collider`, `gameObject` and `rigidbody`. As Unity's, a ray doesn't hit what it starts inside, and a
query that misses allocates nothing. `collider.raycast(ray)` and
`collider.closestPoint(point)` ask one collider. `world.raycast` still
finds what's drawn, with no colliders: use it for taps.
`physics.drawColliders = true` draws every collider with the debug lines.

**Callbacks** come to an object or behaviour that asks for them with a
mixin: `with Triggers` for `onTriggerEnter`, `onTriggerStay` and
`onTriggerExit(Collider other)`, `with Collisions` for
`onCollisionEnter`, `onCollisionStay` and `onCollisionExit(Collision
collision)`. As in Unity, they come to a collider's object and its
rigidbody's object; a trigger needs a rigidbody (kinematic will do) or a
character controller on one side, and a collision a dynamic one. Enter comes the step two
colliders start to touch, stay every step after while either body is
awake, and exit the step they part, or after one is disabled, removed,
destroyed, hidden, put on a layer that doesn't meet, or switched between
trigger and solid (Unity skips those; here the other side always hears).
Between frames, they come at the start of the next frame, before
`onFixedUpdate`. What a callback throws goes to `world.errors`:

```dart
// lib/pickups.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A coin the player picks up by touching it.
class Coin extends GameObject with Triggers {
  Coin(Vec3 at)
    : super(
        transform: Transform(position: at),
        components: [MeshRenderer(Mesh.sphere(), Material()), SphereCollider(isTrigger: true)],
      );

  @override
  void onTriggerEnter(Collider other) {
    if (other.gameObject.name == 'player') destroy();
  }
}

/// Plays a thud when its object lands hard.
class Thud extends Behaviour with Collisions {
  @override
  void onCollisionEnter(Collision collision) {
    if (collision.relativeVelocity.length > 3) print('thud on ${collision.gameObject.name}');
  }
}
```

A `Collision` has the other side's `collider`, `gameObject` and
`rigidbody`, the `impulse` that pushed this side, the `relativeVelocity`,
and the points they touch at (`contactCount`, `getContact(i)`,
`contacts`), each with a `point`, a `normal` out of the other side, and a
`separation`. It's the same object every time: read it in the callback.
A behaviour gets callbacks while it's disabled too, as in Unity.

**Layers that don't meet** pass through each other:
`physics.ignoreLayerCollision(ghosts, 0)` lets everything on the
`ghosts` layer fall through layer 0, from the next step, and
`physics.ignoreCollision(a, b)` does the same for two colliders until one
leaves the world. Queries don't read the matrix; their `layers:` say what
they hit.

**A `CharacterController`** walks a player as Unity's does: a capsule
(`height:`, `radius:`, `center:`) that the game moves with
`move(motion)`, and that goes as far as the world lets it. It slides
along walls, walks up steps up to `stepOffset` (0.3 m) and slopes up to
`slopeLimit` (`45.deg`), and stops at anything higher or steeper. Gravity is
the game's to add, as in Unity, or `simpleMove(speed)` adds it. `move`
returns `CollisionFlags`: `flags.has(CollisionFlags.below)` (the
ground), `.sides`, `.above`; `isGrounded` and `velocity` say how the last
move went. It pushes the rigidbodies it walks into, as if it weighed
`mass` (70 kg; 0 pushes nothing), and meets triggers as a kinematic
rigidbody does. `with ControllerHits` hears each collider a move hits, in
`onControllerColliderHit`:

```dart
// lib/player.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Walks with W, A, S, D or the arrows, and jumps on space.
class Walk extends Behaviour with ControllerHits {
  late final CharacterController controller = gameObject.get<CharacterController>()!;
  var fall = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    final move = input.axis2('move');
    if (controller.isGrounded) fall = input.action('jump').wasPressed ? 5 : -1;
    fall -= 9.81 * time.delta;
    controller.move(Vec3(move.x * 4, fall, -move.y * 4) * time.delta);
  }

  @override
  void onControllerColliderHit(ControllerColliderHit hit) {
    if (hit.gameObject.name == 'lava') gameObject.position = Vec3(0, 1.1, 0);
  }
}

GameObject player() => GameObject(
  name: 'player',
  transform: Transform(position: Vec3(0, 1.1, 0)),
  components: [MeshRenderer(Mesh.capsule(), Material()), CharacterController(), Walk()],
);
```

**Joints** hold a rigidbody to another, or to the world: `FixedJoint`
holds them as one, `HingeJoint` lets them turn about an `axis` (with
`limits`, a `motor` and a `spring`, in radians, each on with `useLimits`,
`useMotor`, `useSpring`), `CharacterJoint` lets them turn every way, as
a ball in a socket (twisting about its `axis` within `lowTwistLimit` and
`highTwistLimit`, and swinging within `swing1Limit` and `swing2Limit`,
each a `SoftJointLimit`; `180.deg` frees it), and `SpringJoint`
pulls them back together when they are further apart than `maxDistance`. A joint is a behaviour on
the object with the rigidbody; `connectedBody` is the other one (none for
the world), and `anchor` is where it holds, in the object's own space. It
holds the bodies as they are when it joins them. With a `breakForce` or
`breakTorque`, a joint that holds harder than that breaks: `with
JointBreaks` hears of it in `onJointBreak`, and the joint is taken off.

```dart
// lib/door.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A door on a hinge at its left edge, that swings shut, and comes off
/// when kicked too hard.
class Door extends GameObject with JointBreaks {
  Door()
    : super(
        name: 'door',
        transform: Transform(position: Vec3(0, 1.1, 0), scale: Vec3(1, 2, 0.1)),
        components: [
          MeshRenderer(Mesh.cube(), Material()),
          BoxCollider(),
          Rigidbody(mass: 10),
          HingeJoint(
            anchor: Vec3(-0.5, 0, 0),
            axis: Vec3(0, 1, 0),
            useLimits: true,
            limits: JointLimits(min: -90.deg, max: 90.deg),
            useSpring: true,
            spring: const JointSpring(spring: 20, damper: 8),
            breakForce: 2000,
          ),
        ],
      );

  @override
  void onJointBreak(Joint joint, double breakForce) => print('the door came off');
}
```

## 15. Flutter around the game

The game is a Flutter widget, so menus, HUDs and settings are plain
Flutter. Keep what the HUD shows in `ValueNotifier`s on the game, and give
the HUD to `FynkelApp`'s `overlay`, which gets the game's root:

```dart
class Score extends GameObject {
  final points = ValueNotifier(0);
}
```

```dart
// lib/hud.dart
import 'package:flutter/widgets.dart';

import 'game.dart';

class ScoreHud extends StatelessWidget {
  const ScoreHud(this.game, {super.key});

  final Score game;

  @override
  Widget build(BuildContext context) => SafeArea(
    child: ValueListenableBuilder(
      valueListenable: game.points,
      builder: (context, points, _) => Text('★ $points', style: const TextStyle(fontSize: 24)),
    ),
  );
}
```

and in `main`: `runApp(FynkelApp(Score.new, overlay: (context, game) =>
ScoreHud(game)))`.

`FynkelApp` is a `MaterialApp` with the game as its page, so the overlay
can hold any Material widget (a `Slider`, a `TextField`, a button), open a
dialog with `showDialog`, or push a page over the game with
`Navigator.of(context)`. Give it a `theme:` (Material's dark one if not),
and a `navigatorKey:` to open pages from the game's code.

**Keep the HUD in its own file.** Flutter and Fynkel both have a `Color`,
so a game's files import `package:fynkel/fynkel.dart` (and
`package:flutter/foundation.dart` for `ValueNotifier`), and its widgets'
files import `package:flutter/widgets.dart`, as the examples do.

A game inside a bigger app (a title screen, a menu) uses the pieces
`FynkelApp` is made of: a **`FynkelGame`**, which makes the root and can
start it over, and a **`FynkelView`**, which shows it:

```dart
// lib/play_page.dart
import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

class PlayPage extends StatelessWidget {
  const PlayPage({super.key});

  @override
  Widget build(BuildContext context) => FynkelGame(
    make: Score.new,
    builder: (context, game) => Stack(
      fit: StackFit.expand,
      children: [
        FynkelView(root: game),
        ScoreHud(game),
        Align(
          alignment: Alignment.bottomCenter,
          child: GestureDetector(
            onTap: () => FynkelGame.maybeOf(context)?.restart(),
            child: const Text('again'),
          ),
        ),
      ],
    ),
  );
}
```

Leaving the page, or starting over, destroys the game as a scene unloads
in Unity: every object gets `onDisable` and `onDestroy`, so clean-up and
saving there run, and then the engine's memory is freed.

## 16. Hot reload and starting over

- **Hot reload** (`r`) changes methods at once, in objects and behaviours
  alike: the next frame runs the new `onUpdate`.
- It doesn't run `onLoad` or `onStart` again, nor change a field that's
  already set, and a routine that's running keeps its old code.
- **F5** (in a debug build) starts the game over with the code as it is
  now: a new root, a new world. That's how a changed `onLoad`, field or
  routine takes effect, in a second, without restarting the app.
- On a phone, which has no F5, `FynkelApp(Game.new, restartOnReload:
  true)` makes every hot reload start the game over.
- **Hot restart** (`R`) starts the whole app again.

## 17. Testing a game

package:fynkel has no Flutter in it, so a game runs in a plain test, frame
by frame, with made-up input:

```dart
// test/walker_test.dart
import 'package:fynkel/fynkel.dart';
import 'package:my_game/game.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('D walks right', () {
    final world = World();
    addTearDown(world.dispose);
    final walker = world.add(Walker());
    world.input.keyDown(KeyCode.d);
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60); // a second at 60 fps
    }
    expect(walker.position.x, closeTo(4, 0.1));
  });
}
```

`world.input` also takes `fingerDown`, `fingerMove`, `fingerUp`, `resize`
and the rest, so taps, drags and swipes can be tested too.
`HeadlessView(world, width:, height:)` renders a frame without a window,
for a picture of the game. `examples/arena/test` tests a whole game this
way.

## 18. Speed

What keeps a game fast:

- **Every speed times `time.delta`.** Then the game is the same at 60 and
  120 fps.
- **Many alike things, one manager.** coins moves its 300 motes from one
  object's `onUpdate` (`Swarm`): one call a frame, not 300.
- **What the camera can't see costs the GPU nothing.** An object wholly
  out of view isn't drawn; it still costs its update and its copy to the
  renderer, so hide or destroy what's far away for good.
- **Share materials and meshes.** `Mesh.cube()` is always the same mesh;
  give many objects one `Material`.
- **Let bodies sleep.** A body at rest costs nothing a step; one woken
  every step (a force each `onFixedUpdate`) costs as one moving.
- **Listen only where you need to.** Contacts are worked out only for
  colliders whose object or rigidbody is `with Triggers` or `with
  Collisions`.
- **Move and query in any order.** A query first brings physics up to
  what moved since the last one, and looks only at those objects and
  what's below them: moving one bullet among 5,000 colliders and then
  casting costs about 2 µs on a Mac.
- **Query from the updates.** The step runs between frames, so a query in
  `onUpdate` never waits for it; one from a Flutter handler between frames
  may.
- **More steps for fast spins and chains.** Physics turns a body at most
  a quarter turn a step, and a chain of joints pulls apart more the
  longer a step is: `time.fixedDelta = 1 / 100` (or 1 / 120) where a
  game needs it. Keep a joint's two bodies within 10 times each other's
  mass, as Unity advises.
- **Measure** on the slowest phone you support. Each example has a
  `lib/measured.dart` for package:fynkel_measure's tool, which gives the
  frame rate and memory: `dart run fynkel_measure:measure rate`.

## 19. What isn't there yet

Audio, animation of models, point and spot lights and shadows, saving,
gamepads, scene files and an editor. In physics: 2D physics,
`ConfigurableJoint`, and chains of joints that don't stretch (Rapier's
multibody joints).

## 20. From Unity

| Unity | Fynkel |
|---|---|
| `GameObject`, a prefab | `GameObject`, a subclass of it |
| `MonoBehaviour` | `Behaviour` |
| `AddComponent<T>()`, `GetComponent<T>()` | `addComponent(T())`, `get<T>()` |
| `Destroy(go)` | `go.destroy()` |
| `SetActive(false)`, `enabled = false` | `active = false`, `enabled = false` |
| `Awake`, `Start`, `Update`, `LateUpdate`, `OnDestroy` | `onLoad`, `onStart`, `onUpdate`, `onLateUpdate`, `onDestroy` |
| `FixedUpdate`, `Time.fixedDeltaTime` | `onFixedUpdate`, `time.fixedDelta` |
| `Time.deltaTime`, `Time.timeScale` | `time.delta`, `time.scale` |
| `yield return new WaitForFixedUpdate()` | `await fixedStep` |
| `StartCoroutine`, `StopAllCoroutines`, `yield return new WaitForSeconds(1)` | `startRoutine(() async {...})`, `stopAllRoutines()`, `await wait(1)` |
| `WaitUntil`, `WaitWhile` | `await waitUntil(...)`, `await waitWhile(...)` |
| `FindObjectOfType<T>()`, `FindObjectsByType<T>()` | `world.first<T>()`, `world.all<T>()` (`includeInactive:` too) |
| Degrees (`Quaternion.Euler(0, 90, 0)`, joint limits, `fieldOfView`) | radians, with `90.deg`: `Quat.euler(0, 90.deg, 0)` |
| `Camera.main`, `ScreenPointToRay`, `WorldToScreenPoint` | `world.camera`, `screenPointToRay`, `worldToScreenPoint` |
| `RaycastHit` | `RaycastHit` (physics) and `RayHit` (`world.raycast`), each with `gameObject` |
| `transform.position`, `rotation`, `localPosition`, `localRotation`, `localScale`, `lossyScale` | the same, on the object or a behaviour |
| `transform.Translate(v)`, `Rotate(axis, angle)`, `Space.World` | `translate(v)`, `rotate(axis, angle)` (radians), `relativeTo: Space.world` |
| `transform.parent = p`, `SetParent(p, false)` | `parent = p`, `setParent(p, worldPositionStays: false)` |
| `transform.Find("name")` | `find('name')` |
| `GetComponentInParent<T>()` | `getInParent<T>()`, or `ancestor<T>()` for an object |
| C# events, `UnityEvent` | `Signal<T>` and `on(signal, ...)` |
| `Input.GetKeyDown(KeyCode.Space)`, `Input.GetKey` | `input.wasPressed(KeyCode.space)`, `input.isPressed` |
| `IPointerClickHandler` | `onClick` |
| Cinemachine | `VirtualCamera`, `Follow`, `LookAt`, `Orbit`, `world.cameras` |
| DOTween | `moveTo`, `tween`, `sequence`, `Ease` |
| `BoxCollider`, `SphereCollider`, `CapsuleCollider`, `MeshCollider` | the same, from `fynkel_physics` |
| A non-convex `MeshCollider` on a dynamic `Rigidbody` (refused) | collides as its hull |
| `Rigidbody`, `ForceMode`, `RigidbodyConstraints`, `PhysicMaterial` | `Rigidbody`, `ForceMode`, `RigidbodyConstraints`, `PhysicsMaterial` |
| `Rigidbody.velocity` | `linearVelocity` (as Unity 6's) |
| A small ball rolling fast at 50 steps a second | physics turns a body at most a quarter turn a step: take more steps (`time.fixedDelta = 1 / 100`) |
| `Physics.gravity`, `Physics.autoSimulation` | `physics.gravity`, `physics.stepBetweenFrames` |
| `Physics.Raycast`, `SphereCast`, `OverlapSphere`, … | `physics.raycast`, `sphereCast`, `overlapSphere`, … |
| `Physics.Raycast` against what's drawn (no colliders) | `world.raycast` |
| `OnTriggerEnter(Collider)`, `OnCollisionEnter(Collision)` | `with Triggers`, `with Collisions`, then `onTriggerEnter`, `onCollisionEnter` |
| `Physics.IgnoreLayerCollision`, `Physics.IgnoreCollision` | `physics.ignoreLayerCollision`, `physics.ignoreCollision` |
| `CharacterController`, `Move`, `SimpleMove`, `CollisionFlags` | the same, as `move`, `simpleMove`, `CollisionFlags.below` |
| `OnControllerColliderHit(ControllerColliderHit)` | `with ControllerHits`, then `onControllerColliderHit` |
| Pushing rigidbodies from `OnControllerColliderHit` | done for you, by the controller's `mass` |
| `FixedJoint`, `HingeJoint`, `SpringJoint`, `CharacterJoint`, `JointLimits`, `JointMotor`, `JointSpring`, `SoftJointLimit` | the same, from `fynkel_physics` |
| `OnJointBreak(float breakForce)` | `with JointBreaks`, then `onJointBreak(joint, breakForce)` |
| `HingeJoint` with both a motor and a spring | the motor wins |
| `Debug.DrawLine` | `debug.line` |
| Scenes, the Inspector | Classes; Flutter for UI |

## 21. What changed

For code written before: each change that breaks it, newest first.

- **Stage 6** (2026-10-12):
  - `Mesh.sphere()` has 720 triangles, as Unity's, not 2,208: a little
    less round up close. A world with two `Light.sun`s says so once:
    only one lights the scene.
  - Angles are radians everywhere: `Camera.fovY`, `VirtualCamera.fovY`,
    the joints' limits, motors, springs, `angle` and `velocity`, and
    `CharacterController.slopeLimit` were degrees. Write `60.deg`.
  - `isDown` is `isPressed`, on `input`, an action and a finger.
  - Routines: `start` is `startRoutine`, `until` is `waitUntil`,
    `whileTrue` is `waitWhile`, `all` is `waitAll`; `stopAllRoutines` is
    new.
  - `world.camera` is the `Camera`, not its object (`.gameObject`).
    `screenToRay`, `screenToPlane` and `worldToScreen` moved from the
    object to the `Camera`, as `screenPointToRay`, `screenPointToPlane`
    and `worldToScreenPoint`.
  - `RayHit.object` and `ObjectError.object` are `gameObject`; physics'
    `PhysicsHit` is `RaycastHit`, with `rigidbody`.
  - `world.all`, `first` and `only` leave out objects that are turned off
    unless `includeInactive: true`.
  - `position` and `rotation` are the world's, as Unity's; what they were
    (relative to the parent) is now `localPosition` and `localRotation`.
    `scale` is `localScale`, and `worldPosition` is `position`. `moveTo`,
    `moveBy` and `rotateTo` go in the world; `localMoveTo` and
    `localRotateTo` do what they did. For a root nothing changes.
  - `translate` moves along the object's own axes, not its parent's; give
    `relativeTo: Space.world` for the world's.
  - `parent = p` keeps the object where it is in the world;
    `setParent(p, worldPositionStays: false)` does what it did.
  - `worldTransform`'s rotation is the parents' rotations times the
    object's, as Unity's; under a parent with an uneven scale it can
    differ a little from before.
  - `World.dispose` destroys every object first: `onDisable` and
    `onDestroy` run, and the objects say `isDestroyed`. Leaving a
    `FynkelView`'s page and F5 dispose the world, so they run there too.
  - What a routine schedules outside the routines step (`await` on a
    routine that ended at once, say) runs in the next routines step, not in
    Dart's own microtask queue. A test's plain loop of `world.update`
    drains it; take a routine's `done` before running the frame it ends in.
  - `FynkelApp` is a `MaterialApp` in Material's dark theme: text in an
    overlay with no style of its own is light and in Material's font.
