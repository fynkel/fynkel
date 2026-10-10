# Writing a game with Fynkel

This guide teaches Fynkel from the smallest game up. Each section adds one
idea, with code that compiles as it stands (`tools/check_guide.py` checks
it). If you know Unity, most of it will look familiar: Fynkel works the
way Unity works, in Dart, and §19 maps one to the other.

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
(`flutter devices` lists them). On Android, Fynkel builds for 64-bit
devices only: build with `--target-platform android-arm64,android-x64`.

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
        transform: Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero),
        components: [Camera()],
      ),
    );
    add(
      GameObject(
        transform: Transform(forward: Vec3(-0.4, -1, -0.3)),
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
radians.

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
  names or tags.

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

- **Meshes**: `Mesh.cube()`, `Mesh.sphere()`, `Mesh.plane()`, or your own
  with `Mesh.custom(positions:, indices:, …)`. Models come from files (§12).
- **Materials** are physically based: `color`, `metallic`, `roughness`,
  `emissive` and `emissiveIntensity` (lit from within), and textures.
  One material can be shared by many objects; change it and all of them
  change.
- **Lights**: `Light.sun` (from the object's forward) and `Light.ambient`.
- **Camera**: `fovY`, `near`, `far`, `clearColor`, `orthographic` with a
  `size`, and a `cullingMask` (§13). The view draws from the active one.

You never free a mesh, material or texture: the engine's copy goes when
nothing in the game reaches it any more.

## 6. The frame, the lifecycle and time

Each object and behaviour gets these calls, in the order they were added:

| Call | When |
|---|---|
| `onLoad` | Once, before it appears. It may be `async` (to load assets, §12); the object appears when everything in it has loaded. |
| `onEnable` | Each time it starts being shown: when it appears, and when it or a parent is turned back on. |
| `onStart` | Once, at the start of its first frame. |
| `onUpdate` | Every frame. |
| `onLateUpdate` | Every frame, after every `onUpdate`, the routines and the tweens: for what follows where things ended up. |
| `onDisable` | Each time it stops being shown, and before `onDestroy`. |
| `onDestroy` | Once, when it's destroyed. |

A frame goes: input → starts → updates → routines and tweens → late
updates → cameras → drawing. This order is fixed and tested.

**`time`** is on every object and behaviour:

- `time.delta`: this frame's seconds, scaled. Multiply every speed by it.
- `time.scale`: 0 pauses everything that uses `delta` (moves, waits,
  tweens); 0.5 is slow motion.
- `time.unscaledDelta`, `time.sinceStart`, `time.unscaledSinceStart`,
  `time.frame`.
- `time.maxDelta` (1/3 s): a long hitch counts as at most this, so nothing
  jumps through walls.

**Errors don't stop the game.** An exception in any of these calls (or a
routine, a handler, a tween) goes to `world.errors` with its object, and
the rest of the frame goes on. If nothing listens to `world.errors`, it's
reported as an uncaught error, so you see it in the console and a test
fails.

## 7. Moving things

On objects and behaviours alike:

```dart
class Drone extends Behaviour {
  final _follow = SmoothDamp3(smoothTime: 0.3);

  @override
  void onUpdate() {
    super.onUpdate();
    translate(Vec3(0, 0, -1) * time.delta); // by an offset
    rotate(Vec3.up, 0.5 * time.delta); // about an axis
    final player = world.first<Pilot>();
    if (player == null) return;
    lookAt(player.worldPosition);
    position = _follow.step(position, player.position + Vec3(0, 3, 0), time.delta);
  }
}

class Pilot extends GameObject {}
```

- `position`, `rotation` and `scale` read and set the local transform;
  `worldPosition` and `forward`, `right` and `up` are in the world.
- `rotate`, `translate`, `lookAt(point)`, `transformPoint` and
  `inverseTransformPoint`.
- The whole transform is one value: `transform = Transform(position: …,
  rotation: …)` sets it at once.
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
  `speed:`.
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
    start(() async {
      for (var wave = 1; ; wave++) {
        for (var i = 0; i < wave * 3; i++) {
          add(Crate(Vec3(i.toDouble(), 0.5, -5)));
          await wait(0.4);
        }
        await until(() => world.all<Crate>().isEmpty);
        await wait(2);
      }
    });
  }

  /// Ten points in ten seconds, or not.
  Future<bool> beatTheClock() async {
    clock.value = true;
    try {
      return await race([
        start(() async {
          await until(() => score.value >= 10);
          return true;
        }),
        start(() async {
          await wait(10);
          return false;
        }),
      ]);
    } finally {
      clock.value = false; // runs even when the routine is stopped
    }
  }

  Future<void> bonusRound() async {
    final won = await start(beatTheClock); // a routine returns a value
    if (won) score.value += 100;
  }
}
```

- **`start(() async {...})`** runs one, on an object or behaviour. It
  **stops by itself** when its object is destroyed (or its behaviour
  removed), and **pauses** while its object is hidden or the behaviour
  isn't enabled.
- **Waits**: `await nextFrame`, `await wait(seconds)` (`unscaled: true` to
  go on while paused), `await until(() => test)`, `await whileTrue(() =>
  test)`, a tween, a signal's `next`, or any `Future`.
- **`race([...])`** gives the first to end and stops the rest (routines,
  waits and tweens); **`all([...])`**
  waits for every one. `race([x, wait(3)])` is a timeout.
- `start` returns a **`Routine`**: `stop()`, `isRunning`, and it's a
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
    if (input.isDown(MouseButton.right)) rotate(Vec3.up, time.delta);
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
    on(input.onPinch, (pinch) => scale = scale * pinch.scale);
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
  `delta`, `totalDelta`, `velocity`, `age`, `tapCount`, `isDown`,
  `isDragging`, and `over` (what's under it in the world: `object`,
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
    wide = add(GameObject(components: [VirtualCamera(fovY: 70), Orbit(_player, distance: 15)])).get<VirtualCamera>()!;
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
- **Between the screen and the world**: `camera.screenToRay(point)`,
  `screenToPlane(point, Plane.ground)` and `worldToScreen(point)`, on the
  camera's object (`world.camera`); `world.raycast(ray)` gives what a ray
  hits first.

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

## 14. Flutter around the game

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

Leaving the page destroys the game and frees the engine's memory.

## 15. Hot reload and starting over

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

## 16. Testing a game

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

## 17. Speed

What keeps a game fast:

- **Every speed times `time.delta`.** Then the game is the same at 60 and
  120 fps.
- **Many alike things, one manager.** coins moves its 300 motes from one
  object's `onUpdate` (`Swarm`): one call a frame, not 300.
- **Share materials and meshes.** `Mesh.cube()` is always the same mesh;
  give many objects one `Material`.
- **Measure** on the slowest phone you support. Each example has a
  `lib/measured.dart` for package:fynkel_measure's tool, which gives the
  frame rate and memory: `dart run fynkel_measure:measure rate`.

## 18. What isn't there yet

Physics and collisions, audio, animation of models, point and spot lights
and shadows, saving, gamepads, scene files and an editor. Until physics
comes, distance checks (`(a.position - b.position).length < 1`) and
`world.raycast` do the work, as the arena's do.

## 19. From Unity

| Unity | Fynkel |
|---|---|
| `GameObject`, a prefab | `GameObject`, a subclass of it |
| `MonoBehaviour` | `Behaviour` |
| `AddComponent<T>()`, `GetComponent<T>()` | `addComponent(T())`, `get<T>()` |
| `Destroy(go)` | `go.destroy()` |
| `SetActive(false)`, `enabled = false` | `active = false`, `enabled = false` |
| `Awake`, `Start`, `Update`, `LateUpdate`, `OnDestroy` | `onLoad`, `onStart`, `onUpdate`, `onLateUpdate`, `onDestroy` |
| `Time.deltaTime`, `Time.timeScale` | `time.delta`, `time.scale` |
| `StartCoroutine`, `yield return new WaitForSeconds(1)` | `start(() async {...})`, `await wait(1)` |
| `FindObjectOfType<T>()`, `FindObjectsOfType<T>()` | `world.first<T>()`, `world.all<T>()` |
| `transform.Find("name")` | `find('name')` |
| `GetComponentInParent<T>()` | `getInParent<T>()`, or `ancestor<T>()` for an object |
| C# events, `UnityEvent` | `Signal<T>` and `on(signal, ...)` |
| `Input.GetKeyDown(KeyCode.Space)` | `input.wasPressed(KeyCode.space)` |
| `IPointerClickHandler` | `onClick` |
| Cinemachine | `VirtualCamera`, `Follow`, `LookAt`, `Orbit`, `world.cameras` |
| DOTween | `moveTo`, `tween`, `sequence`, `Ease` |
| `Physics.Raycast` | `world.raycast` |
| `Debug.DrawLine` | `debug.line` |
| Scenes, the Inspector | Classes; Flutter for UI |
