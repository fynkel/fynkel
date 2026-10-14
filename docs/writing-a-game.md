# Writing a game with Fynkel

This guide teaches Fynkel from the smallest game up. Each section adds one
idea, with code that compiles as it stands (`tools/check_guide.py` checks
it). If you know Unity, most of it will look familiar: Fynkel works the
way Unity works, in Dart, and §20 maps one to the other.

The finished examples are in `examples/`: `hello` (one turning cube),
`garden` (models and an orbit camera), `coins` (triggers and a HUD),
`arena` (a small game using most of what's here), `bowling` (a game on
physics) and `sheepdog` (a game that feels like an app: a title screen,
settings, pause, a HUD, labels in the world, a minimap and the app's
theme). `examples/lab` has a scene for each feature, to try by hand.

## 1. A new game

You need Flutter 3.47 or later and Rust ([rustup](https://rustup.rs)); the
repository's `rust-toolchain.toml` picks the version. The engine is built
from source the first time you run a game, and again only when it changes.
That first build takes a few minutes and prints nothing while it runs, the
first `flutter test` too: it isn't stuck.

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

**Android needs Android 11 or later.** Set it in the app before its first
Android build, or the build stops at "minSdkVersion 24 cannot be smaller
than version 30 declared in library [:fynkel_flutter]". In
`android/app/build.gradle.kts`, `flutter create` wrote `minSdk =
flutter.minSdkVersion`; make it 30:

```kotlin
// my_game/android/app/build.gradle.kts
android {
    defaultConfig {
        minSdk = 30 // was flutter.minSdkVersion
    }
}
```

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
- **Always call `super`** in the `on…` methods. Fynkel's own do nothing
  today, but a class you extend may do something in its own, and the
  analyzer warns when a call is missing.

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
  names or tags (the `name` checks in §14 are there to keep the examples
  short; a subclass or a component is the sturdier test). As Unity's, they leave out objects that are turned off
  (and the components on them) unless `includeInactive: true`.

**Pooling.** A game that makes and destroys many short-lived objects
(shots, sparks, enemies of a wave) can reuse them instead, with a
`Pool`, as Unity's `ObjectPool`: `get` hands out a hidden object or makes
a new one, and `release` hides it again for the next `get`.

```dart
class Shot extends GameObject {
  Shot(Material material) : super(components: [MeshRenderer(Mesh.sphere(), material)]);

  var speed = 0.0;
  var flown = 0.0;

  @override
  void onEnable() {
    super.onEnable();
    flown = 0; // each time it's handed out
  }

  @override
  void onUpdate() {
    super.onUpdate();
    translate(Vec3.forward * (speed * time.delta));
    flown += time.delta;
    if (flown > 2) release(); // back to its pool
  }
}

class Turret extends GameObject {
  final shotMaterial = Material(color: const Color(0xFFFFD040));
  late final shots = Pool(this, () => Shot(shotMaterial));
  ObjectHandle<Shot>? lastShot;

  void fire() {
    final shot = shots.get((shot) {
      shot.position = position;
      shot.rotation = rotation;
      shot.speed = 12;
    });
    lastShot = ObjectHandle(shot);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (lastShot?.object case final shot?) debug.line(position, shot.position);
  }
}
```

- **Reset in `get`'s function, or in `onEnable`.** The function runs
  first, then the object is shown, so its `onEnable` sees what the
  function set. A new object is made by the pool's function and added
  below the pool's parent.
- **A released object starts clean**: it's hidden, the routines and
  motions it and everything below it ran are stopped (a hidden object's
  would only pause, and resume stale), what they listened to with `on`
  while shown (from `onEnable` on) is cancelled, and it's back below the
  pool's parent. So listen in `onEnable`, and each `get` listens once
  again; what it listened to in `onLoad` stays, through every reuse. Its
  fields are the game's.
- **`release()` on the object** gives it back to its pool, or destroys
  it if it didn't come from one: write it where you'd write `destroy()`.
- **Hold pooled objects with an `ObjectHandle`.** A field pointing at a
  shot still points at it once it's released and handed out again as a
  new shot; `ObjectHandle(shot).object` is null from the release on (and
  once it's destroyed).
- `countActive` and `countInactive` say how many are out and waiting;
  `prewarm(n)` makes them ahead, so play doesn't make (and load) them:
  a new one `get` makes shows from the next frame, once it's loaded,
  where a waiting one shows at once;
  `activeAt(i)` walks the ones out without making a list (walk from the
  end to release as you go), and `active` gives a copy;
  `clear()` destroys the waiting ones; with `maxSize`, a release beyond
  that many waiting destroys. Hidden objects aren't in `world.all`, so a
  wave's "are they all gone" works the same with a pool. Getting and
  releasing allocates nothing.

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
- **To keep a behaviour in a typed field**, make it in a constructor of
  your own and pass it on, so the field and the components share it:
  `Enemy() : this._(Health(3));` then `Enemy._(this.health) :
  super(components: [health]);` with `final Health health;`.

**Which one?** If a subclass's `onUpdate` would make sense on another kind
of object, it belongs in a behaviour. Keep a class for what an object *is*
(its state, its parts, its typed fields), and behaviours for what it
*does* that others could do too, or that comes and goes while it plays.

**A bigger game** keeps the same shape. The root object holds what the
whole game shares (the score, the lives: `ValueNotifier`s the HUD shows)
and the managers, each a child. Pass each object what it needs when you
make it, the root or a manager, rather than searching for it. And let
data, not subclasses, tell kinds apart: a runner and a tank are one
`Enemy` class given a different `EnemyKind`. Put everything that belongs
to one level under one object, so starting over is destroying it and
adding a new one (§16):

```dart
// lib/defense.dart
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

/// What makes one kind of enemy differ from another: data, not a subclass.
class EnemyKind {
  const EnemyKind({required this.name, required this.speed, required this.health, required this.color});

  final String name;
  final double speed;
  final int health;
  final Color color;

  static const runner = EnemyKind(name: 'runner', speed: 4, health: 1, color: Color(0xFFE0A030));
  static const tank = EnemyKind(name: 'tank', speed: 1, health: 10, color: Color(0xFF708090));
}

/// The root: what the whole game shares, and the level it plays.
class Defense extends GameObject {
  final gold = ValueNotifier(100);
  final lives = ValueNotifier(10);
  late Level level;

  @override
  void onLoad() {
    super.onLoad();
    level = add(Level(this));
  }

  /// Starts over in place: the same root and HUD, a new level.
  void restart() {
    level.destroy(); // its waves, enemies and routines go with it
    level = add(Level(this));
    gold.value = 100;
    lives.value = 10;
  }
}

/// One level: the ground, and the waves that cross it.
class Level extends GameObject {
  Level(this.game)
    : super(
        name: 'level',
        components: [MeshRenderer(Mesh.plane(), Material())],
        transform: Transform(scale: Vec3(20, 1, 20)),
      );

  final Defense game;

  @override
  void onStart() {
    super.onStart();
    startRoutine(() async {
      for (var wave = 1; ; wave++) {
        for (var i = 0; i < 5 + wave; i++) {
          add(Enemy(i.isEven ? EnemyKind.runner : EnemyKind.tank, game));
          await wait(0.8);
        }
        await wait(5);
      }
    });
  }
}

class Enemy extends GameObject {
  Enemy(this.kind, this.game)
    : super(
        name: kind.name,
        transform: Transform(position: Vec3(-10, 0.5, 0)),
        components: [MeshRenderer(Mesh.cube(), Material(color: kind.color))],
      );

  final EnemyKind kind;
  final Defense game;
  late int health = kind.health;

  @override
  void onUpdate() {
    super.onUpdate();
    translate(Vec3(kind.speed * time.delta, 0, 0), relativeTo: Space.world);
    if (position.x > 10) {
      game.lives.value--;
      destroy();
    }
  }
}
```

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
  change. To change one object alone, give its renderer a `color` of its
  own (Unity's `MaterialPropertyBlock`): it replaces the material's color
  for that object, and `null` gives it back. `material.copy()` makes a
  material of its own to change every value.
- **Lights**: `Light.sun` (from the object's forward), `Light.ambient`,
  and lamps: `Light.point` and `Light.spot` (below), and their shadows. An object has at
  most one `Light`, so each light goes on an object of its own.
- **Camera**: `fovY` (radians: `60.deg`), `near`, `far`, `clearColor`,
  `orthographic` with an `orthographicSize`, and a `cullingMask` (§13). The view draws
  from the active one, `world.camera` (Unity's `Camera.main`). Keep `near`
  below `far`: a view drawing from a camera that isn't throws an
  `ArgumentError` naming it, each frame, rather than drawing nothing. It's
  checked only then, so setting `near` and then `far` is fine.

You never free a mesh, material or texture: the engine's copy goes when
nothing in the game reaches it any more.

### Quality levels

A world is drawn at one of three levels, `Quality.low`, `medium` or
`high`. Each is meant to hold its devices' frame rate, and the effects
(shadows, and to come a sky and bloom) each say what they do at each
level.

When the game doesn't choose, the device picks: high on Apple's GPUs and
desktops', medium on phones' Mali and Adreno, low on a software GPU or
one it doesn't know. `Quality.device` says which. A game can choose at
any time, from its settings page for example, and every view of the
world draws at the new level from its next frame:

```dart
/// The settings page's choice: a level, or null to let the device pick.
/// Gives the level drawn at.
Quality chooseQuality(World world, Quality? chosen) {
  world.quality = chosen;
  return world.quality ?? Quality.device;
}
```

Above low, the scene is drawn into a half-float image first and copied
to the screen by a last pass, which is where bloom will go. At low it's
drawn straight to the screen, in one pass.

### Lamps and spotlights

A point light shines from its object's position every way; a spotlight
shines from there along the object's forward, in a cone. Both reach
`range` metres and fade smoothly to nothing there:

```dart
/// A warm lamp on a post, reaching 8 m.
GameObject lampPost(Vec3 at) => GameObject(
  name: 'lamp post',
  transform: Transform(position: at + Vec3(0, 2.5, 0)),
  components: [Light.point(color: Color.rgb(1, 0.8, 0.55), intensity: 10, range: 8)],
);

/// A torch that shines where its object faces: turn the object to aim it.
GameObject handTorch() => GameObject(
  name: 'torch',
  components: [Light.spot(intensity: 40, range: 20, angle: 30.deg, softness: 0.3)],
);
```

- **Brightness** is the sun's, at 1 m: a lamp of `intensity` 1 lights a
  surface 1 m away as a sun of 1 does, and a quarter as much at 2 m (light
  falls off with the square of the distance). A room lamp 2–3 m from what
  it lights wants 5 to 20; a torch across a room, 50 and up.
- **`angle`** is the cone's width, edge to edge, up to 179°.
  **`softness`** is how much of it fades, from the rim in: 0 is a hard
  edge, 1 fades all the way to the middle.
- **Every sun lights everything**, up to four of them.
- **Each object is lit by the lamps that light it most**: up to 4 at low
  and medium quality, 8 at high. Among lamps alike, those are the nearest.
  A spotlight counts only for what's in its cone, and a lamp only for
  what it reaches. So a big floor under many lamps gets only the few that
  light its middle most: make it of tiles a lamp's range or less across,
  and each tile gets its own. The lab's 'Look: lamps' scene is a room
  of 1 m tiles.
- **A frame takes 64 lamps** in view, the ones nearest the camera. A
  world with more than 64 point and spot lights, or more than four suns,
  says so once.

### Shadows

A sun or a spotlight casts shadows when asked; every object then casts
them and has them fall on it, unless its renderer says not, as in Unity:

```dart
/// A low evening sun, casting long shadows.
GameObject eveningSun() => GameObject(
  transform: Transform(rotation: Quat.lookRotation(Vec3(-0.6, -0.5, -0.4))),
  components: [Light.sun(intensity: 2, shadows: true)],
);

/// A puddle: it shadows nothing, but shadows fall on it.
MeshRenderer puddle(Material water) => MeshRenderer(Mesh.cylinder(), water)..castShadows = false;
```

- **The sun's shadows** reach a set distance from the camera and fade
  out there: 40 m at low quality, 60 m at medium, 100 m at high (less if
  the camera's `far` is nearer). They're drawn into maps seen from the
  sun: at low one map with hard edges, at medium two with soft ones, at
  high three softer ones. With more than one, the nearest covers least,
  so shadows near the camera are sharpest. The maps move with the camera
  a whole texel at a time, so a still shadow's edge doesn't crawl as the
  camera moves. Only the first sun with `shadows` casts them.
- **A spotlight's shadows** reach as far as its `range`. Each frame
  gives a few spotlights shadows, the ones nearest the camera: 1 at low,
  2 at medium, 4 at high. The others still light, without shadows.
- **Point lights don't cast shadows** yet.
- **`castShadows: false`** is for what shouldn't darken the ground (a
  glow, water, a ghost) and for what can't shadow anything (a flat
  floor), which also saves drawing it into the maps.
  **`receiveShadows: false`** keeps shadows off an object.
- `light.shadows`, `renderer.castShadows` and `renderer.receiveShadows`
  can change at any time. The lab's 'Look: shadows' scene shows the sun's
  and a spotlight's, at each level.

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
| `onApplicationPause` | When the app goes to the background (`true`) and comes back (`false`): save here, as the app may be closed while it's away. `world.applicationPaused` says which. |

A frame goes: input → starts → fixed steps → updates → routines and
tweens → late updates → cameras → drawing. This order is fixed and
tested.

**Fixed steps** cut the game's time into equal steps of `time.fixedDelta`
(1/50 s), so what runs in them goes the same at 30 fps and at 120. A
frame runs as many as its time holds: at 120 fps most frames have none,
at 30 some have two. A frame runs at most `time.maxFixedSteps` (5), and
drops the time past that, so a step slower than its frame can't snowball.
Inside a step `time.delta` is `time.fixedDelta`. To
draw something a step moves without it stepping, draw it between where
the last two steps left it, `time.fixedAlpha` of the way (0 to 1).

**`time`** is on every object and behaviour:

- `time.delta`: this frame's seconds, scaled. Multiply every speed by it.
- `time.scale`: 0 stops everything that uses `delta` (moves, waits,
  tweens); 0.5 is slow motion. To pause the game, use `world.paused`
  (§11), which stops the input too.
- `time.unscaledDelta`, `time.sinceStart`, `time.unscaledSinceStart`,
  `time.frame`.
- `time.maxDelta` (1/3 s): a long hitch counts as at most this much game
  time, whatever the scale, so nothing jumps through walls.
- `time.maxFixedSteps` (5): the most fixed steps a frame runs. Raise it
  for a game that runs `time.scale` fast at a low frame rate.
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

One call is different: **an error in `onLoad`** (an object's, or one of
its behaviours') **destroys the object**, since it never finished
loading; it goes to `world.errors` too, and its `loaded` future fails
with it. A level whose file is missing doesn't appear half-made.

**Your own mistakes throw Dart's errors**, at the line that made them:
an `ArgumentError` for a bad value (a NaN position, a rotation that isn't
`.normalized`, a negative radius), a `StateError` for a call at the wrong
time or on something gone (a destroyed object, a mesh still in use). A
file that can't be read throws a `FormatException`. `FynkelException` is
only for the engine's own failures: no GPU, a lost device, a panic.

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
    sequence([colorTo(Color.white, seconds: 0.05), colorTo(Color(0xFF806040), seconds: 0.3)]); // this door alone
  }

  void hover() => moveBy(Vec3(0, 0.2, 0), seconds: 1, ease: Ease.inOutSine, loops: -1, yoyo: true);

  void fadeLight() => tween(1.0, 0.0, seconds: 2, onUpdate: (v) => get<Light>()?.intensity = v);
}
```

- **Helpers**: `moveTo`, `moveBy`, `rotateTo`, `rotateBy`, `lookAtOver`,
  `scaleTo`, `colorTo`, `punchScale`, `shake`. Each takes `seconds:`;
  all but `colorTo`, `punchScale` and `shake` take `speed:` instead. `moveTo`, `moveBy` and `rotateTo` are in the world, as
  `position`; `localMoveTo` and `localRotateTo` relative to the parent;
  `scaleTo` sets `localScale`, and `rotateBy` turns about the object's own
  axis. `colorTo` changes the renderer's own `color` (§5), so other
  objects sharing its material don't flash too; give it `material:` to
  change a material.
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
  isn't enabled, except during their own load: an `onLoad` may await a
  routine (or a tween) of its own.
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

- A signal with nothing to tell is a `Signal<void>`, emitted with
  `emit(null)`.
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
  gets the rest of that drag. With physics, a collider takes taps too,
  even with nothing drawn (an invisible button); triggers don't
  (`physics.pointerHitsColliders = false` turns it off).
- **Keys** are `KeyCode`s, by where they are on the keyboard, so W, A, S,
  D stay put on any layout. `input.bindAxis2` and `input.bindAction` add
  your own names.
- **Rebinding** is binding again: `input.bindAction('dash', [key])` changes
  the same `InputAction`, so code that kept it sees the new keys. A key
  held at that moment that's no longer bound lets go (`wasReleased` comes
  next frame); a `press()` goes on holding. On a controls page,
  `keyCodeOf(event)` (from `package:fynkel_flutter`) turns the key pressed
  in a "press a key" dialog into the game's, and `key.label` names it for
  the player (`Space`, `W`, `↑`). §15 has that dialog and a row of a
  controls page.
- A Flutter widget over the game (a HUD button) takes its own taps; they
  don't reach the game.

**Flutter controls** feed the same axes and actions as the keys, as
Unity's on-screen controls do, so the game reads one path: a joystick
widget calls `input.setAxis2('move', Vec2(x, y))` (it adds to the keys,
and stays until set again), and a button calls `input.action('jump')
.press()` when it goes down and `.release()` when it comes up (or both at
once for a tap).

**In `onFixedUpdate`**, `wasPressed` and `wasReleased` (an action's too)
mean since the last fixed step, so each press is seen once there: a frame
that runs two fixed steps doesn't jump twice, and one that runs none
passes the press to the next. Unity's `GetKeyDown` in `FixedUpdate`
misses or repeats presses; here it doesn't.

**Keys and Flutter.** The view hears every key and gives each to the game
(`onKeyDown` hears them all), but it keeps from Flutter only the keys the
game uses: those bound to an axis or an action, and those it has asked
`isPressed`, `wasPressed` or `wasReleased` about (`input.usesKey`). The
others go on to Flutter's shortcuts and focus, so Tab and a dialog's
Escape work. Keys for the app rather than the game go in `FynkelApp`'s
`shortcuts:`, or a `CallbackShortcuts` around the view.

**Pause** with `world.paused = true`: the game's time stops (`delta` is 0,
no fixed steps; updates still run, with `unscaledDelta`), held keys,
fingers and actions let go, and input that comes while paused is dropped.
The view claims no keys then, so the pause menu's shortcuts get every one.
The first frame after it counts no time, however long the pause was:

```dart
// lib/paused_app.dart
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

void main() => runApp(
  FynkelApp(
    Score.new, // §15's
    overlay: (context, game) => ScoreHud(game),
    shortcuts: (game) => {
      const SingleActivator(LogicalKeyboardKey.escape): () => game.world.paused = !game.world.paused,
    },
  ),
);
```

§2's `show runApp` is too narrow for this: import all of `widgets.dart`,
and `services.dart` for the keys, as here. A pause panel shows itself
when the game pauses, from wherever it's paused, with
`ValueListenableBuilder(valueListenable: game.world.pausedListenable,
...)`; game code hears it with `on(world.onPausedChanged, ...)`.

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
    final model = add(await assets.instantiate('props/tree_oak.glb'));
    model.find('leaves')?.get<MeshRenderer>()?.color = Color.rgb(0.8, 0.5, 0.2); // this tree's alone
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

- `assets.instantiate(name)` gives a new copy of the model, a tree of objects
  sharing its meshes and textures with every other copy. Find its parts by
  name.
- Each file is loaded once, off the UI thread, and shared. It's freed when
  the last object that asked for it is destroyed.
- `assets.preload([...])` loads ahead, so a level appears at once later.
- A package can add a kind of asset of its own (a sound package's `.wav`):
  it's loaded, preloaded, shared and freed in the same way.
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

- **Rigs** are behaviours: `Follow(target, offset:, space:, damping:,
  lookAhead:)`, `LookAt(target)`, and `Orbit(target)`, which the player
  turns with a drag and zooms with a pinch or the wheel. A rig works on a
  plain camera too, without a `VirtualCamera`. `Follow`'s `offset` is in
  the world unless `space:` says otherwise, as Cinemachine's binding
  modes: `FollowSpace.target` turns it with the target, so the camera
  stays behind a car, and `FollowSpace.targetYaw` turns it only as the
  target turns left and right.
- `world.cameras.shake(strength, frequency:)`, `setBlend(blend, from:, to:)`, `live`.
  A shake moves the picture, not the camera: its transform stays where
  the game or the virtual camera put it, so moving it during a shake
  never drifts. `world.cameras.shakeOf(camera)` says how far it's moved
  now; world widgets (§15) move with it.
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
    MeshCollider.addToParts(add(await assets.instantiate('rock.glb')));
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
`rigidbody.position` reads where physics has it. Queries (raycasts,
overlaps) and character controllers meet the body there too, where the
last step left it, up to a step ahead of what's drawn, as in Unity. A
controller riding an interpolated platform (`moveWithGround`) goes with
the platform's steps. A small, fast body can
pass through a thin one between steps: Rapier sweeps every fast body
against static colliders, and `CollisionDetectionMode.continuous` sweeps
it against bodies too. `physics.gravity`, `defaultSolverIterations` and
`sleepThreshold` are the world's settings; a new `gravity` wakes every
body, so sleeping ones fall the new way. `sleep()` and `wakeUp()` do
nothing for a body not in the world yet, and what can't be done there
says why (no object yet, not enabled, not shown, destroyed).

**Moving a kinematic body by hand** (a platform, a door, a bullet that's a
trigger): `movePosition` and `moveRotation` in `onFixedUpdate`. It goes
there over the next step, so it pushes what's in the way, carries what
stands on it, and is drawn smoothly when interpolated. Setting `position`
(or the transform) teleports it, as in Unity: right for a respawn, wrong
for a move. Either way a trigger sees only what it overlaps at the end
of each step, so a thing that moves further than its own size in a step
can pass something by; see bullets below.

**The step runs between frames**, on the physics' own thread: it starts
when a frame is done, and its results are written at the start of the
next, before `onFixedUpdate` and the updates. Queries in the updates
never wait for it, and see the world as the last step left it (an
interpolated body's drawn pose trails that by up to a step). The costs: a force
shows a frame later, and `await fixedStep` resumes before that step has
run. `physics.stepBetweenFrames = false` steps
within the frame instead, as Unity does.

A Flutter widget that reads a body as it builds (`body.velocity` for a
speedometer) waits for the step running then, giving back the time the
thread saved. So read it in the game, in `onUpdate`, and hand the
widget the number through a `ValueNotifier`. The views and world widgets
never wait: they read the objects' transforms, which the step's results
were written to at the start of the frame.

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
finds what's drawn, with no colliders; taps find both (§11).
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
    if (other.gameObject.has<CharacterController>()) destroy(); // the player walked into it
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
`onControllerColliderHit`. It rides what it stands on: `ground` is that
collider, and when it moves or turns (a platform moved by a tween, say)
the controller goes along in the same frame, unless `moveWithGround` is
off; `groundVelocity` says how fast the ground goes, for a jump off it:

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
the world), and `anchor` is where it holds, in the object's own space,
scale included, as in Unity: `Vec3(-0.5, 0, 0)` is the left face of a
cube however wide the object is scaled (the door below is 1 wide; scaled
to 3, the hinge would still be on its edge). `connectedAnchor` is the
same on the other body, or in the world with none. It holds the bodies
as they are when it joins them, at the scale they have then. With a `breakForce` or
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

**Bullets** can be done two ways. A bullet that's an object, with a
small trigger collider and a kinematic rigidbody moved with
`movePosition`, hears what it hits in `onTriggerEnter`, and is simple; it
can pass through something thinner than it moves in a step. Many fast
bullets are better as plain data in one manager, which casts a ray along
each one's move every frame and so never misses: no bodies, one query
each. On a Mac, 500 bullets among 100 targets cost about 0.17 ms a frame
as bodies and 0.05 ms as rays (a phone takes a few times longer). Draw
either kind from a `Pool` (§3) so firing makes nothing new:

```dart
// lib/guns.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Moves every bullet in one place, as rays: none can skip a thin wall.
class Bullets extends GameObject {
  final hit = Signal<RaycastHit>();
  final _at = <Vec3>[], _was = <Vec3>[], _velocity = <Vec3>[];

  void fire(Vec3 from, Vec3 velocity) {
    _at.add(from);
    _was.add(from);
    _velocity.add(velocity);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    for (var i = _at.length - 1; i >= 0; i--) {
      final to = _at[i] + _velocity[i] * time.delta;
      final path = to - _was[i];
      final struck = physics.raycast(Ray(_was[i], path), maxDistance: path.length);
      if (struck != null || to.length > 100) {
        if (struck != null) hit.emit(struck);
        _at.removeAt(i);
        _was.removeAt(i);
        _velocity.removeAt(i);
      } else {
        _was[i] = _at[i];
        _at[i] = to;
      }
    }
  }
}
```

Each ray starts where the bullet was a frame ago, not where it is, so it
goes over the last move again. A ray doesn't hit what it starts inside:
started where the bullet is, it would miss a target that walked onto the
bullet since the last frame, and miss more often the longer the frames.
A test of just that:

```dart
// test/bullets_test.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:my_game/guns.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('a target that walks onto a bullet is hit', () {
    final world = World();
    addTearDown(world.dispose);
    final bullets = world.add(Bullets());
    final target = world.add(GameObject(components: [BoxCollider()]))
      ..position = Vec3(0, 0, 10);
    final hits = <GameObject>[];
    bullets.hit.listen((hit) => hits.add(hit.gameObject));
    bullets.fire(Vec3.zero, Vec3(0, 0, -30));
    world.update(1 / 60); // the bullet is at z = -0.5
    target.position = Vec3(0, 0, -0.7); // and now inside the target
    world.update(1 / 60);
    expect(hits, [target]);
  });
}
```

**Dragging a body** with a finger, as Unity's old `DragRigidbody`: a ray
finds the body, a `SpringJoint` ties it to a kinematic "hand", and the
hand follows the finger across a plane facing the camera. The spring pulls
the body along, so it still collides, swings and stays joined:

```dart
// lib/drag.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Lets a finger pick up and carry any dynamic body.
class Dragger extends GameObject {
  Dragger(this.camera) : super(components: [Rigidbody(isKinematic: true)]);

  final Camera camera;
  late final Rigidbody hand = get<Rigidbody>()!;
  SpringJoint? _spring;
  Plane? _plane;

  @override
  void onStart() {
    super.onStart();
    on(input.onFingerDown, (finger) => _grab(finger.position));
    on(input.onFingerMove, (finger) => _drag(finger.position));
    on(input.onFingerUp, (_) => _let());
  }

  void _grab(ViewPoint point) {
    final hit = physics.raycast(camera.screenPointToRay(point));
    final body = hit?.collider.attachedRigidbody;
    if (hit == null || body == null || body.isKinematic) return;
    position = hit.point;
    _plane = Plane(camera.gameObject.forward, hit.point);
    // Stiff and damped for the body's mass: it follows within centimetres.
    _spring = body.gameObject.addComponent(
      SpringJoint(
        connectedBody: hand,
        anchor: body.gameObject.inverseTransformPoint(hit.point),
        connectedAnchor: Vec3.zero,
        spring: body.mass * 1000,
        damper: body.mass * 40,
      ),
    );
  }

  void _drag(ViewPoint point) {
    final plane = _plane;
    final at = plane == null ? null : camera.screenPointToPlane(point, plane);
    if (at != null) hand.movePosition(at);
  }

  void _let() {
    final spring = _spring;
    if (spring != null && !spring.gameObject.isDestroyed) spring.gameObject.removeComponent(spring);
    _spring = null;
    _plane = null;
  }
}
```

The lab's "Joints: a chain and a weight" scene does this, and works out
the spring from the weight it lifts.

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

**From the game to the UI and back.** A `Signal` has a `stream`, for
`StreamBuilder` or a `listen` that opens a dialog when the game is over
(it hears each value a moment after `emit`). The other way, any
`Listenable` (a `ValueNotifier` the settings page changes) has a
`changed` signal, from `package:fynkel_flutter`, so an object listens with
`on(settings.volume.changed, (_) => ...)` and stops when it's destroyed.

**Calling the game from a Flutter callback is safe**: a button's
`onPressed` can call `game.restart()`, add an object or cast a ray. It
runs between frames, on the same thread as the game; a physics call made
then waits for the step running between frames to finish first.

`FynkelApp` is a `MaterialApp` with the game as its page, so the overlay
can hold any Material widget (a `Slider`, a `TextField`, a button), open a
dialog with `showDialog`, or push a page over the game with
`Navigator.of(context)`. Give it a `theme:` (Material's dark one if not),
and a `navigatorKey:` to open pages from the game's code.

**Keep the HUD in its own file.** Flutter and Fynkel both have a `Color`
(and a `Material` and a `Transform`), so a game's files import
`package:fynkel/fynkel.dart` (and `package:flutter/foundation.dart` for
`ValueNotifier`), and its widgets' files import Flutter, as the examples
do. A file that needs both imports one with a prefix, or only the names
it uses from the other (`show ThemeData`).

**One look for the app and the game.** Write the palette once, as a
Flutter theme:

```dart
// lib/palette.dart
import 'package:flutter/material.dart';

/// The colour everything is grown from.
const seed = Color(0xFF4CA6E0);

ThemeData gameTheme(Brightness brightness) =>
    ThemeData(colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: brightness));
```

and give it to the app: `FynkelApp(Score.new, theme:
gameTheme(Brightness.light), darkTheme: gameTheme(Brightness.dark))`,
light or dark as the phone is (`themeMode:` to choose). The game wears
it too: `world.theme` is the app's `ThemeData` (from `Theme.of` where the
game is shown), there before the root is added, and
`world.onThemeChanged` tells of a new one. `.toFynkel()` turns a Flutter
colour into a Fynkel one, `.toFlutter()` the other way, both from
`package:fynkel_flutter`:

```dart
// lib/sky.dart
import 'package:flutter/material.dart' show ThemeData;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// On the camera: its sky is the theme's surface, and follows it.
class Sky extends Behaviour {
  @override
  void onStart() {
    super.onStart();
    wear(world.theme);
    on(world.onThemeChanged, wear);
  }

  void wear(ThemeData theme) => get<Camera>()!.clearColor = theme.colorScheme.surface.toFynkel();
}
```

When the phone goes dark, the app fades to its dark theme, and
`onThemeChanged` hears each step of the fade, so the sky fades with it.
`world.isDark` says which it is. A widget that shows a game's colour
imports Fynkel with a prefix:

```dart
// lib/team_chip.dart
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' as fynkel;
import 'package:fynkel_flutter/fynkel_flutter.dart';

class TeamChip extends StatelessWidget {
  const TeamChip(this.team, {super.key});

  final fynkel.Color team;

  @override
  Widget build(BuildContext context) => Chip(label: const Text('team'), backgroundColor: team.toFlutter());
}
```

A test sets the theme itself: `world.theme = gameTheme(Brightness.dark)`.

**Widgets in the world.** A health bar over an enemy, a name tag, a
damage number or a "press E" hint is a Flutter widget on the object: a
`WorldWidget`, from `package:fynkel_flutter`. Every view of the world
shows it over the game, where its camera sees the object, each frame:

```dart
// lib/tagged.dart
import 'package:flutter/material.dart' show Alignment, Colors, Text, TextStyle;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// A cube with its name over it.
class Tagged extends GameObject {
  Tagged(String name) : super(name: name, components: [MeshRenderer(Mesh.cube(), Material())]) {
    addComponent(
      WorldWidget(
        (context) => Text(name, style: const TextStyle(color: Colors.white)),
        offset: Vec3(0, 0.8, 0), // above it, in the world's directions
        alignment: Alignment.bottomCenter, // the tag's bottom middle there
        shrinkBeyond: 8, // smaller farther than 8 from the camera
      ),
    );
  }
}
```

The widget is built once and then only moved, so a hundred cost little
(100 health bars in the arena hold 120 fps on the Mac and the iPhone).
To change what it shows, it rebuilds itself as any widget does: a
`ValueListenableBuilder` of a `ValueNotifier` on the object, or a
`StreamBuilder` of a signal's `stream` (the arena's `HealthBar`). It's
hidden while its object is inactive, behind the camera or off the view,
and the nearer ones are drawn over the farther ones. A tap on it is its
own, a button in it works, and the game doesn't see that tap. For a widget
at a point, a damage number say, put it on an empty object there, and
move or destroy that.

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

`FynkelGame` makes the game's world and adds the root to it before the
builder runs, so `game.world` is there from the start: a widget beside
the view can take `game.world.input` while it's built, as an on-screen
stick does (`Stick(game.world.input)`). The view shows the game's world;
a game whose assets don't come from the app's bundle gives
`FynkelGame(source: ...)`.

Leaving the page, or starting over, destroys the game as a scene unloads
in Unity: every object gets `onDisable` and `onDestroy`, so clean-up and
saving there run, and then the engine's memory is freed.

**Menus, pop-ups and pages** are Flutter's own: `showDialog`,
`Navigator.push`, `PopScope`. Whatever covers the view's page pauses the
game (`world.paused`), so it hears no taps or keys, and closing the last
one goes on. The game stays alive behind it: its objects, routines and
score wait where they were. A game that was paused already stays paused.
For a game that runs on under a see-through pop-up, give
`FynkelView(pauseWhenCovered: false)`.

```dart
// lib/menus.dart
import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

/// The game's page: back (Android's button) opens the pause menu.
class GamePage extends StatelessWidget {
  const GamePage({super.key});

  @override
  Widget build(BuildContext context) => FynkelGame(
    make: Score.new,
    builder: (context, game) => PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) showPauseMenu(context);
      },
      child: Stack(fit: StackFit.expand, children: [FynkelView(root: game), ScoreHud(game)]),
    ),
  );
}

/// The pause menu, over the game's page at [context].
Future<void> showPauseMenu(BuildContext context) {
  final navigator = Navigator.of(context);
  return showDialog<void>(
    context: context,
    builder: (context) => SimpleDialog(
      title: const Text('Paused'),
      children: [
        SimpleDialogOption(onPressed: navigator.pop, child: const Text('Resume')),
        SimpleDialogOption(
          onPressed: () => navigator.push(MaterialPageRoute<void>(builder: (_) => const SettingsPage())),
          child: const Text('Settings'),
        ),
        SimpleDialogOption(
          onPressed: () => navigator
            ..pop() // the menu
            ..pop(), // and the game's page
          child: const Text('Leave'),
        ),
      ],
    ),
  );
}

class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key});

  @override
  Widget build(BuildContext context) => Scaffold(appBar: AppBar(title: const Text('Settings')));
}
```

The settings page goes over the menu, and the game stays paused under
both until the menu closes. `PopScope(canPop: false)` turns Android's
back button into "open the menu". In a dialog, Escape is a back too: a
menu that leaves the game on back, as the arena's does, maps Escape to
closing itself with a `CallbackShortcuts` (`examples/arena/lib/menus.dart`,
which has a title screen, this menu and a settings page, and tests that
click through them).

**A controls page** rebinds with a "press a key" dialog. Its `Focus`
takes every key before the app's shortcuts do, and Escape closes it
keeping the key there was:

```dart
// lib/controls.dart
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show Input, KeyCode;
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// Waits for the player to press a key for [what], and gives it; null for
/// Escape.
Future<KeyCode?> askForKey(BuildContext context, String what) => showDialog<KeyCode>(
  context: context,
  builder: (context) => Focus(
    autofocus: true,
    onKeyEvent: (node, event) {
      if (event is KeyDownEvent) {
        Navigator.of(context).pop(event.logicalKey == LogicalKeyboardKey.escape ? null : keyCodeOf(event));
      }
      return KeyEventResult.handled;
    },
    child: AlertDialog(title: Text('Press a key for $what')),
  ),
);

/// The controls page's row for jumping: the key, and a tap to change it.
class JumpKey extends StatefulWidget {
  const JumpKey(this.input, {super.key});

  final Input input;

  @override
  State<JumpKey> createState() => _JumpKeyState();
}

class _JumpKeyState extends State<JumpKey> {
  var _key = KeyCode.space;

  @override
  Widget build(BuildContext context) => ListTile(
    title: const Text('Jump'),
    trailing: OutlinedButton(
      onPressed: () async {
        final key = await askForKey(context, 'Jump');
        if (key == null) return;
        widget.input.bindAction('jump', [key]);
        setState(() => _key = key);
      },
      child: Text(_key.label),
    ),
  );
}
```

A real game keeps its keys in a `ChangeNotifier` the settings page and
the game share, and the game binds them again when it changes:
`on(controls.changed, (_) => controls.bindTo(input))`
(`examples/sheepdog/lib/game.dart`).

**Several views of one world**: a minimap, split screen. Each is a
`FynkelView` of the same game, with a `camera:` of its own; without one,
a view draws from the main camera (`world.camera`). However many views
show the world, it updates once a frame. Its meshes and textures are on
the GPU once; each view only adds its own frames. A view whose camera is
gone (destroyed, inactive) draws black rather than the main picture.

```dart
// lib/minimap.dart
import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// A player and a camera high above it, looking straight down.
class Field extends GameObject {
  late final GameObject player;
  final map = Camera(orthographic: true, orthographicSize: 8);

  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(transform: Transform(position: Vec3(0, 4, 9)), components: [Camera()]));
    player = add(GameObject(components: [MeshRenderer(Mesh.cube(), Material())]));
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(0, -1, 0), up: Vec3(0, 0, -1))),
        components: [map, Follow(player, offset: Vec3(0, 20, 0))],
      ),
    );
  }
}

class FieldPage extends StatelessWidget {
  const FieldPage({super.key});

  @override
  Widget build(BuildContext context) => FynkelGame(
    make: Field.new,
    builder: (context, game) => Stack(
      fit: StackFit.expand,
      children: [
        FynkelView(root: game),
        Align(
          alignment: Alignment.topRight,
          child: SizedBox(
            width: 160,
            height: 160,
            child: FynkelView(root: game, camera: game.map, input: false, worldWidgets: false),
          ),
        ),
      ],
    ),
  );
}
```

The main camera is the first one added (the lowest id), so add the
game's own camera before the minimap's. `input: false` keeps taps on the
minimap from reaching the game or the view under it. For split screen,
put two views in a `Row`, each with the camera that follows its player.
Give `input: false` to all but one: that one takes the keys for both
players, and aims what's under a finger from its own camera
(`world.input.camera`). `worldWidgets` stays on there, so each half
shows the name tags placed by its own camera. The lab's "Minimap" and
"Split screen" scenes show both, with each view's frames per second.

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

**Starting a level over** inside the game (a "try again" button) needs
no new world: destroy the object the level hangs from and add a new one,
as `Defense.restart` does in §4. Everything below it goes at once, with
its `onDestroy`, its routines, tweens and listeners; what the root keeps
(the score, the HUD's notifiers, the settings) stays. `FynkelGame`'s
`restart()` is the bigger hammer: a new root in a new world.

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
    world.input.simulate.keyDown(KeyCode.d);
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60); // a second at 60 fps
    }
    expect(walker.position.x, closeTo(4, 0.1));
  });
}
```

`world.input.simulate` also takes `fingerDown`, `fingerMove`, `fingerUp`,
`resize` and the rest, so taps, drags and swipes can be tested too; it's
what the view itself calls. A plain test has no view, so give the input a
size first, `world.input.simulate.resize(800, 600)`, and then a finger
lands where you say. To put one on an object, ask the camera where it is
on screen: `world.camera!.worldToScreenPoint(target.position, width: 800,
height: 600)`.
`HeadlessView(world, width:, height:)` renders a frame without a window,
for a picture of the game: `File('shot.png').writeAsBytesSync(view.png())`
saves it to look at, which finds an unlit side or a camera too far fast. `examples/arena/test` tests a whole game this
way.

**An error in the game fails the test** by itself, as it would be an
uncaught error: nothing listens to `world.errors`. To test that something
throws, listen first, and let the stream deliver before you look:
`world.errors` is a stream, so what the frames put in it arrives after
them, not during them. Without the `await`, the list is still empty and
the test passes whatever happened:

```dart
// test/errors_test.dart
import 'package:fynkel/fynkel.dart';
import 'package:flutter_test/flutter_test.dart';

class Fragile extends GameObject {
  @override
  void onUpdate() {
    super.onUpdate();
    throw StateError('broke');
  }
}

void main() {
  test('a fragile object reports its error every frame', () async {
    final world = World();
    addTearDown(world.dispose);
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    world.add(Fragile());
    for (var i = 0; i < 3; i++) {
      world.update(1 / 60);
    }
    await pumpEventQueue(); // let the stream deliver what the frames sent
    expect(errors, hasLength(3));
  });
}
```

**Compare positions with a tolerance.** `Vec3` and `Quat` have
`closeTo(other)`, so `expect(a.position.closeTo(Vec3(1, 0, 0)), isTrue)`.
An object with a rigidbody needs it most: physics keeps poses in 32-bit
floats, so after a step its position reads `0.20000000298`, not `0.2`.

**The whole app works in a widget test too**: the real `FynkelApp`, its
view and its overlay, real taps. With no GPU plugin in a test, the game
runs unseen, frame by frame as `tester.pump` moves the clock, and the view
is the test window's size. Reach the game with `FynkelGame.of`:

```dart
// test/app_test.dart
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:my_game/game.dart';
import 'package:my_game/hud.dart';

/// The Score game with a cube to tap, in front of a camera.
class TapToScore extends Score {
  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(transform: Transform(position: Vec3(0, 0, 4)), components: [Camera()]));
    add(Target());
  }
}

class Target extends GameObject {
  Target() : super(components: [MeshRenderer(Mesh.cube(), Material())]);

  @override
  bool onClick(Finger finger) {
    (parent! as Score).points.value++;
    return true;
  }
}

void main() {
  testWidgets('a tap on the cube scores, and the HUD shows it', (tester) async {
    await tester.pumpWidget(FynkelApp(TapToScore.new, overlay: (context, game) => ScoreHud(game)));
    await tester.pump(const Duration(milliseconds: 16)); // a frame: the game loads and starts
    final game = FynkelGame.of<TapToScore>(tester.element(find.byType(FynkelView))).game;
    final input = game.world.input;
    final cube = game.world.camera!.worldToScreenPoint(Vec3.zero, width: input.width, height: input.height)!;
    await tester.tapAt(Offset(cube.x, cube.y));
    await tester.pump(const Duration(milliseconds: 16)); // the frame that hears it
    expect(game.points.value, 1);
    await tester.pump();
    expect(find.text('★ 1'), findsOneWidget);
  });
}
```

Two things differ from other widget tests. **`pumpAndSettle` never
settles** while a view is on screen, since the game asks for every frame:
pump a duration instead, `tester.pump(const Duration(milliseconds: 16))`
for a frame. And input waits for the next frame, as on a device: a tap
reaches the game in the `pump` after it.

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
- **Short-reaching lamps.** A lamp costs something for each object it
  reaches, and the pixels it lights; one that reaches no further than it
  needs to costs least. Turn a lamp off with `intensity = 0`: it's left
  out altogether.
- **A lower quality level for an older phone.** `world.quality =
  Quality.low` draws in one pass, with no image between the scene and the
  screen (§5).
- **Measure** on the slowest phone you support. Each example has a
  `lib/measured.dart` for package:fynkel_measure's tool, which gives the
  frame rate and memory: `dart run fynkel_measure:measure rate`.

## 19. What isn't there yet

Audio, animation of models, shadows, saving,
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
| `ObjectPool<T>`: `Get`, `Release` | `Pool(parent, make)`: `get`, `release` |
| `Awake`, `Start`, `Update`, `LateUpdate`, `OnDestroy` | `onLoad`, `onStart`, `onUpdate`, `onLateUpdate`, `onDestroy` |
| `FixedUpdate`, `Time.fixedDeltaTime` | `onFixedUpdate`, `time.fixedDelta` |
| `Time.deltaTime`, `Time.timeScale` | `time.delta`, `time.scale` |
| `Time.timeScale = 0` for a pause menu | `world.paused = true`: time and input stop |
| On-screen stick and button (`OnScreenStick`, `OnScreenButton`) | a Flutter widget calling `input.setAxis2` and `action.press()`/`release()` |
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
| `MaterialPropertyBlock` for one object's colour | `renderer.color` |
| `new Material(material)` | `material.copy()` |
| `Camera.orthographicSize` | `orthographicSize`, on `Camera` and `VirtualCamera` |
| Cinemachine's binding modes (world space, lock to target, lock to target on assign/with world up) | `Follow(space: FollowSpace.world / target / targetYaw)` |
| `Input.mouseScrollDelta` | `input.scroll`: +y with the wheel turned away, as Unity's |
| `Plane`, `Plane.distance`, `GetDistanceToPoint` | `Plane`, `distance`, `distanceTo` |
| `Instantiate(prefab)` of a model | `await assets.instantiate('model.glb')` |
| A character on a moving platform (parenting it, by hand) | done for you: `moveWithGround`, `ground`, `groundVelocity` |
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

**What stays different**, on purpose or by the tools underneath:

- **Axes**: right-handed, +Y up, forward is **−Z** (`Vec3.forward`), as
  glTF and most of Flutter's 3D. Unity is left-handed with forward +Z, so
  a scene copied number for number comes out mirrored.
- **Angles are radians** everywhere, joints and `slopeLimit` too; write
  `90.deg`. `Orbit.pitch` is positive looking down.
- **Key names are Flutter's**: `KeyCode.arrowUp`, `shiftLeft`, `digit1`,
  not `UpArrow`, `LeftShift`, `Alpha1`.
- **`MeshRenderer.material` is shared**, as Unity's `sharedMaterial`: change
  `renderer.color` for one object, or give it `material.copy()`.
- **`find('name')` searches everything below**, not only the children
  (Unity's `Transform.Find`), and takes no paths.
- **`Mesh.plane()` is 1 × 1**, Unity's Quad facing up; Unity's Plane is
  10 × 10.
- **An error in `onLoad` destroys the object** (§6); one in Unity's `Awake`
  leaves it half-made.
- **The physics step runs between frames** (§14), so a force shows a
  frame later; `physics.stepBetweenFrames = false` steps as Unity does.
- **A new `physics.gravity` wakes every body**; Unity leaves sleeping ones
  hanging.
- **Exit callbacks always come**: for a collider disabled, destroyed,
  hidden or moved to a layer that doesn't meet; Unity skips some.
- **A `CharacterController` turns with its object**; Unity's stays
  upright. Keep its object upright and turn a child for the model.
- **A HUD is Flutter**: widgets over the view, not a Canvas in the scene.

## 21. What changed

For code written before: each change that breaks it, newest first.

- **Hardening** (2026-10-14):
  - `time.maxDelta` limits the game's time after `time.scale`, as
    Unity's does: at a scale of 5 a hitch counts for 1/3 s, not 5/3.
  - A frame runs at most `time.maxFixedSteps` (5) fixed steps, and drops
    the rest of its time: a frame of 1/3 s runs 5 where it ran 16. Set it
    higher to keep the old behaviour.
  - A pool's `release` cancels what the object, and everything below
    it, listened to with `on` while shown; what it listened to in its
    load stays. A pooled object listening in `onEnable` no longer hears
    once more per reuse.
  - An object's or behaviour's routines and tweens run during its own
    load, so an `onLoad` that awaits one finishes; they hung there.
  - `wasPressed` and `wasReleased` in `onFixedUpdate` mean since the
    last fixed step, where they meant this frame: a press is seen by one
    fixed step, not by each of the frame's, nor by none.
  - A finger's times (a tap's, a long press's, `age`, `velocity`) are
    its events' own, from Flutter, and not limited by `time.maxDelta`:
    a 1 s hold through a hitch is no longer a tap, and a swipe's speed
    is no longer overstated.
  - A still finger or mouse gets `onPointerExit` when what was under it
    moves away or is hidden, and `onPointerEnter` when something comes
    under it, at the next frame's input step; before, only when it moved.

- **Stage 8** (2026-10-14):
  - Every `Light.sun` lights the scene, up to four, where only the first
    did; a world with two no longer says so. One with a fifth does.

- **Stage 7** (2026-10-13):
  - A camera shake no longer moves the camera's object: its `position`
    stays put while the picture shakes. Code that read the shaken place
    there adds `world.cameras.shakeOf(camera)`.
  - A camera whose `near` isn't below its `far` makes its views' frames
    throw an `ArgumentError`, where they drew nothing.
  - Two `FynkelView`s of one world update it once a frame between them,
    where each used to update it. A test that counted on that pumps
    twice as many frames.
  - A view's GPU device is the app's, shared by every view: losing it
    stops them all (`FynkelStatus.deviceLost`), and each makes a new one.
    `ViewStats.textureBytes` and `meshBytes` are the world's, the same
    in each of its views.
  - `input.bindAction` on a name already bound changes that
    `InputAction` and returns it, rather than making a new one;
    `InputAction.buttons` follows the latest binding.
  - A `FynkelView` pauses its world while a page or dialog covers it
    (`pauseWhenCovered`, on by default), and goes on when it's closed. A
    game that kept running under its own menus pauses there now; give
    `pauseWhenCovered: false` to keep it running.
  - A `FynkelView` that makes its own world (given a `root:` outside a
    `FynkelGame`) adds the root in `didChangeDependencies`, after
    `initState`, so the world has the app's theme first.
  - `FynkelGame` makes the world and adds the root before its builder
    runs, so `game.world` works there. A `source:` for its assets goes on
    the `FynkelGame` now: given to its `FynkelView`, it's reported and
    not used.

- **Stage 6, the review's bugs** (2026-10-13):
  - Changing the parent of an object that's being destroyed, or putting
    something under one, throws a `StateError` (Unity's "can't change
    the hierarchy while deactivating"). It used to split what Dart and
    the engine knew. To keep a child, move it out before `destroy()`.
  - A behaviour that's being taken off or destroyed can't `on(...)` or
    `startRoutine(...)` any more: both throw a `StateError`.
  - A physics setter that throws for a bad value (`body.mass = 0`) keeps
    the value it had.

- **Stage 6** (2026-10-12):
  - A game's mistakes throw Dart's errors, not `FynkelException`: an
    `ArgumentError` for a bad value (a NaN or not-unit transform, a
    parent in another world, a value the engine refuses), a `StateError`
    for something gone or still in use (a destroyed object, a mesh a
    renderer uses). `Texture2D.load` and `Model.load` throw a
    `FormatException` for a file they can't read. `FynkelException` and
    `PhysicsException` are left for the engine's own failures; catch
    those types for what you caught before. `PhysicsStatus` is only in
    `package:fynkel_physics/raw.dart`.
  - `World.pointer` is gone from `package:fynkel/fynkel.dart`, and a
    `WorldSystem`'s hooks (`beforeFrame` … `dispose`) are `@protected`:
    `world.physics.dispose()` is the world's to call.
  - `FynkelApp`'s overlay is laid out loosely from the top-left corner,
    so a HUD covers only itself and taps around it reach the game (a
    `Text` used to cover the screen and take them all). Wrap it in a
    `SizedBox.expand` if it must fill the screen.
  - For packages (`package:fynkel/extend.dart`): `WorldSystem.componentAdded`
    is also called for the components an object comes with when it's
    added, not only for `addComponent`; `componentRemoved` is new. A
    system whose hooks throw in 5 frames in a row is turned off
    (`isTurnedOff`), and that's reported once.
  - `PhysicsStatus` (in `package:fynkel_physics/raw.dart`) has a `code`,
    and its numbers match the engine's `FynkelStatus` where they mean the
    same: compare statuses by name, not by `index`.
  - `RawPhysics.step()` and `beginStep()` take no length: set
    `stepLength` (1/50 at first) once, and each step is that long.
  - `colorTo` changes the renderer's own `color`, not its material, so
    objects sharing the material no longer change with it; give
    `material:` for the old way. `MeshRenderer.color` and
    `Material.copy()` are new.
  - `input.scroll`'s y is Unity's way up: + with the wheel turned away
    from you, the other way from Flutter's `scrollDelta` (as it was).
    `input.simulate.scrollBy` still takes Flutter's.
  - `Plane.distance` is Unity's: the plane y = 3 facing up has -3, not
    3. `distanceTo` is unchanged.
  - `Transform.forward` is gone: it was relative to the parent. Use the
    object's `forward`, or `rotation.rotate(Vec3.forward)`.
  - `Camera.size` and `VirtualCamera.size` are `orthographicSize`.
  - `assets.model(name)` is `assets.instantiate(name)`.
  - `cameras.shake` and the `shake` tween take `frequency:`, not
    `vibrato:` (`punchScale`'s `vibrato` is a count and stays).
  - `Pinch.centre` is `center`.
  - `physics.ignoreLayerCollision` and `ignoreCollision` take
    `ignore:` by name: `ignoreLayerCollision(8, 0, ignore: false)`.
  - The build hook's `HookError` is `HookException`.
  - `FynkelView(world: w)` is `FynkelView.world(w)`; `FynkelView(root:)`
    is as it was. `FynkelGame.of<T>(context)` is new, and `maybeOf<T>`
    gives the game as a `T`.
  - What the view calls to feed input (`keyDown`, `fingerDown`,
    `resize`, …) moved to `input.simulate`: `world.input.simulate
    .keyDown(KeyCode.d)` in a test. `keyDown` and `keyUp` return whether
    the view should claim the key.
  - The view keeps only the keys the game uses (§11); the rest go on to
    Flutter's shortcuts. A key the game hears only through `onKeyDown`,
    and never polls, reaches Flutter too.
  - `onEnable` and `onDisable` run depth first, as Unity's: an object,
    its behaviours, then each child with what's below it; they ran a
    level at a time. `GameObject.release()` is new (see §3's pooling).
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
