# Writing a game with Fynkel

This guide teaches Fynkel from the smallest game up. Each section adds one
idea, with code that compiles as it stands (`tools/check_guide.py` checks
it). If you know Unity, most of it will look familiar: Fynkel works the
way Unity works, in Dart, and §22 maps one to the other.

The finished examples are in `examples/`: `hello` (one turning cube),
`garden` (models, an animated character and an orbit camera), `coins` (triggers, a HUD, and a character that walks, runs and picks things up),
`arena` (a small game using most of what's here), `bowling` (a game on
physics) and `sheepdog` (a game that feels like an app: a title screen,
settings, pause, a HUD, labels in the world, a minimap and the app's
theme). `examples/lab` has a scene for each feature, to try by hand.

## 1. A new game

You need Flutter 3.47 or later and Rust ([rustup](https://rustup.rs)); the
repository's `rust-toolchain.toml` picks the version. The engine is built
from source the first time you run a game, and again only when it changes.
That first build takes a few minutes, the first `flutter test` too, and
Flutter shows cargo working only with `-v` (`flutter run -v`): it isn't
stuck. The README says what rustup needs.

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

Some Flutter versions' `--empty` leaves out Material's icons, so every
`Icon` shows as a box. If the game's Flutter side uses them, check that
`pubspec.yaml` has this line, and add it if not:

```yaml
# my_game/pubspec.yaml
flutter:
  uses-material-design: true
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

**On Android, the game's activity is a `FynkelActivity`.** Then each
`FynkelView` draws straight to the screen, behind Flutter's view, and
Flutter never waits for the game's frames: on a Galaxy Note10+ that took
Sheepdog from 51 to 59 fps ([the report](android-surface-report.md)).
`flutter create` wrote a `FlutterActivity`; change it:

```kotlin
// my_game/android/app/src/main/kotlin/<your package>/MainActivity.kt
import dev.fynkel.fynkel_flutter.FynkelActivity

class MainActivity : FynkelActivity()
```

A game whose activity stays a `FlutterActivity` runs too, with its views
drawn as textures Flutter shows. What this changes for widgets around
the game is in §17.

**On iOS nothing is needed**: each `FynkelView` draws straight to the
screen, on a layer under Flutter's view, which the plugin makes
see-through itself. Flutter then never copies the game's frames, which
keeps an iPhone cool ([the report](ios-surface-report.md)).

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
    add(GameObject(transform: Transform.lookAt(Vec3(0, 1.5, 4), Vec3.zero), components: [Camera()]));
    add(GameObject(components: [Light.sun(direction: Vec3(-0.4, -1, -0.3))]));
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
  `world.add`; `FynkelApp` does that for you. For a component in a
  field, `addWith` adds an object holding it and returns the component:
  `_sun = addWith(Light.sun(shadows: true))`.
- **`destroy()`** takes an object and everything below it out of the game.
  A field still pointing at it says `isDestroyed`; reading its transform
  then throws a clear error instead of giving a stale value.
  `destroy(after: 0.5)` destroys it that many seconds of game time later
  (Unity's `Destroy(go, t)`), for sparks and shots, without a routine
  each; hidden or not, and slowed by `time.scale` and pauses as the game
  is. Destroying again does nothing.
- **`active = false`** hides an object and everything below it: not drawn,
  not touched, not updated. `activeInHierarchy` says whether it and all its
  parents are on.
- **Finding**: `parent`, `children`, `find(name)` below this one,
  `ancestor<T>()` the nearest above of a type, and across the whole game
  `world.all<T>()`, `world.first<T>()` and `world.only<T>()` (which throws
  unless there's exactly one). These find by type, so a game rarely needs
  names or tags (the `name` checks in §16 are there to keep the examples
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
  As `destroy()`, releasing again does nothing (a shot that hits two
  enemies in one step), and `release(after: 2)` releases it later, unless
  it's released sooner: its next use isn't cut short.
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
adding a new one (§18):

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
  or your own with `Mesh.custom(positions:, indices:, …)`, or one made
  and changed while the game runs (§12b). Models come from files (§12).
- **Materials** are physically based: `color`, `metallic`, `roughness`,
  `emissive` and `emissiveIntensity` (lit from within), and textures.
  One material can be shared by many objects; change it and all of them
  change. To change one object alone, give its renderer a `color` of its
  own (Unity's `MaterialPropertyBlock`): it replaces the material's color
  for that object, and `null` gives it back. `material.copy()` makes a
  material of its own to change every value. A material can also take a
  `shader` of your own, a `.wgsl` file, for what these values can't do:
  grass in the wind, water, a dissolve, a toon look; and an `outline`
  round its objects (§14).
- **Lights**: `Light.sun` (along the object's forward, which
  `Light.sun(direction: …)` turns the object to), `Light.ambient`,
  and lamps: `Light.point` and `Light.spot` (below), and their shadows. An object has at
  most one `Light`, so each light goes on an object of its own.
- **Camera**: `fovY` (radians: `60.deg`), `near`, `far`, `clearColor`,
  `orthographic` with an `orthographicSize`, a `cullingMask` (§15), and
  `showSky` (below). The view draws
  from the active one, `world.camera` (Unity's `Camera.main`). Keep `near`
  below `far`: a view drawing from a camera that isn't throws an
  `ArgumentError` naming it, each frame, rather than drawing nothing. It's
  checked only then, so setting `near` and then `far` is fine.
- **`enabled = false`** turns any of them off, as a behaviour's (§4): a
  camera draws nothing (the next one draws), a light lights nothing and
  isn't one of the frame's 64 lamps, a renderer draws nothing. The object
  and its other components go on; scripts and physics still run. Each
  keeps what it was given, and what's changed while it's off shows when
  it's on again.

You never free a mesh, material or texture: the engine's copy goes when
nothing in the game reaches it any more.

### Quality

What a world draws, and how finely, is its `Quality`: whether there's
bloom, ambient occlusion and shadows, and how fine the shadows are. As in
Unity, a game can have one quality or several, and change them as it
likes. Three levels come with the engine, to use as they are or start
from:

| | `Quality.low` | `Quality.medium` | `Quality.high` |
|---|---|---|---|
| `hdr`: drawn into a half-float image, then to the screen | no | yes | yes |
| `bloom` | no | yes | yes |
| `ambientOcclusion` | no | no | yes |
| `lampHighlights`: lamps give shiny surfaces a highlight | no | yes | yes |
| `shadows` | yes | yes | yes |
| `shadowCascades`, `shadowResolution` | 1 × 1024 | 2 × 2048 | 3 × 2048 |
| `shadowSoftness` | hard | soft | softer |
| `shadowDistance` | 40 m | 60 m | 100 m |
| `spotShadows`, `spotShadowResolution` | 1 × 512 | 2 × 1024 | 4 × 1024 |
| `renderScale`: pixels drawn per pixel of the screen | 0.75 | 1 | 1 |
| `antiAliasing`: samples a pixel, smoothing edges (MSAA) | 4 | 4 | 4 |
| `softParticles`: particles fade where they meet what's behind | no | yes | yes |

When the game doesn't choose, the device picks a level: high on Apple's
GPUs and desktops', low on Android phones (Mali, Adreno and the rest), on a
software GPU and on one it doesn't know. `Quality.device` says which.
Phones get low because a Galaxy Note10+ (2019) doesn't hold 60 fps at
medium: about 40–50 in a scene with everything
([the report](android-pacing-report.md)). A newer phone may: a game can
offer medium on its settings page and let the player choose. Most games set
one quality and keep it; a game with a settings page can switch at any
time, and every view draws the new one from its next frame:

```dart
/// A game's one quality: `Quality(...)` starts from medium's settings;
/// this one has no bloom, and one sharp cascade of shadows.
const myQuality = Quality(name: 'mine', bloom: false, shadowCascades: 1, shadowSoftness: ShadowSoftness.hard);

/// Or levels of its own, for a settings page, each from the engine's.
final myLevels = [
  Quality.low.copyWith(shadowDistance: 25),
  Quality.medium,
  Quality.high.copyWith(ambientOcclusion: false),
];

/// The settings page's choice: one of them, or null to let the device
/// pick. Gives what's drawn.
Quality chooseQuality(World world, Quality? chosen) {
  world.quality = chosen;
  return world.quality ?? Quality.device;
}
```

- **Fewer pixels: `renderScale`**, as Unity's render scale. At 0.75 the
  views draw about half the pixels and stretch the picture to fit: a
  little softer, and much cheaper on a big screen (an iPhone 16 Pro Max is
  1320 × 2868). Low draws at 0.75: on a Galaxy Note10+, Sheepdog at low
  went from 43 fps to 55. Put it on the settings page beside the levels:
  `world.quality = Quality.high.copyWith(renderScale: 0.75)`, or
  `Quality.low.copyWith(renderScale: 1)` for low at every pixel. A view's
  own `FynkelView(resolutionScale:)` multiplies it, for one view (a
  minimap).
- **Smoother edges: `antiAliasing`**, as Unity's anti-aliasing (MSAA): 4
  samples a pixel at every level, or 2, or 0 for none. On phones' GPUs the
  samples stay on the chip, so 4 costs little: on the Note10+ at low,
  Sheepdog gained nothing without them, and its sheep's edges were jagged.
  A device that can't draw 2 draws none.
- **A camera asks, the quality decides.** A camera's bloom and ambient
  occlusion (below) are drawn only where the quality has them on, so a
  cheaper quality for older phones can turn bloom off without touching
  the cameras. A debug build says once when a camera asks for one that
  the quality drawn leaves out; for a cheaper quality chosen on purpose,
  that's expected, and there's nothing to change.
- **Qualities compare by value**: `world.quality` gives back what was
  set, and `Quality.medium.copyWith(bloom: false) ==
  Quality.medium.copyWith(bloom: false)`. Give your own a `name` (in the
  constructor or `copyWith`) for a settings page or a log.
- **Settings out of range** (`shadowCascades` 1 to 3, resolutions powers
  of two from 256 to 4096, `spotShadows` 0 to 4, `shadowDistance` above
  0) throw an `ArgumentError`, and the world keeps the quality it had.
- **Without `hdr` or bloom**, the scene is drawn straight to the screen
  in one pass: the cheapest.

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
- **A lamp lights everything it reaches**, whatever it falls on: a big
  floor, terrain or a room in one piece is lit by every lamp over it,
  however many overlap. What costs is how many overlap: each pixel adds
  up every lamp that reaches it. Without `Quality.lampHighlights` (as at
  low) lamps light a surface's colour but give shiny surfaces no
  highlight (suns still do), which keeps a crowded room fast on phones.
- **A frame takes 64 lamps** in view, the ones nearest the camera. A
  world with more than 64 point and spot lights, or more than four suns,
  says so once.

### Shadows

A sun or a spotlight casts shadows when asked; every object then casts
them and has them fall on it, unless its renderer says not, as in Unity:

```dart
/// A low evening sun, casting long shadows.
GameObject eveningSun() =>
    GameObject(components: [Light.sun(direction: Vec3(-0.6, -0.5, -0.4), intensity: 2, shadows: true)]);

/// A spotlight 4 m up, pointing straight down, casting shadows.
GameObject stallLight() => GameObject(
  transform: Transform(position: Vec3(0, 4, 0), rotation: Quat.lookRotation(Vec3(0, -1, 0), up: Vec3(0, 0, -1))),
  components: [Light.spot(range: 8, angle: 60.deg, shadows: true)],
);

/// A puddle: it shadows nothing, but shadows fall on it.
MeshRenderer puddle(Material water) => MeshRenderer(Mesh.cylinder(), water)..castShadows = false;
```

- **The sun's shadows** reach the quality's `shadowDistance` from the
  camera and fade out there (less if the camera's `far` is nearer).
  They're drawn into `shadowCascades` maps seen from the sun, with
  `shadowSoftness` edges: at low one map with hard edges, at medium two
  with soft ones, at high three softer ones. With more than one, the
  nearest covers least, so shadows near the camera are sharpest. A
  quality with `shadows: false` draws none, whatever the lights ask. The maps move with the camera
  a whole texel at a time, so a still shadow's edge doesn't crawl as the
  camera moves. Only the first sun with `shadows` casts them.
- **A spotlight's shadows** reach as far as its `range`. Each frame
  gives a few spotlights shadows, the ones nearest the camera: 1 at low,
  2 at medium, 4 at high. The others still light, without shadows. A
  spotlight with shadows is at most 150° across: a wider one is refused
  with an `ArgumentError`, since its shadow map can't cover the rim.
- **Point lights don't cast shadows** yet.
- **`castShadows: false`** is for what shouldn't darken the ground (a
  glow, water, a ghost) and for what can't shadow anything (a flat
  floor), which also saves drawing it into the maps.
  **`receiveShadows: false`** keeps shadows off an object.
- `light.shadows`, `renderer.castShadows` and `renderer.receiveShadows`
  can change at any time. The lab's 'Look: shadows' scene shows the sun's
  and a spotlight's, at each level.

### The sky and fog

`world.sky` is what's behind everything, and it lights the scene. A new
world has none: the camera's `clearColor` shows.

```dart
/// A clear day: deep blue overhead, pale at the horizon, the far field
/// fading into it.
void clearDay(World world) {
  world.sky = Sky.gradient(
    zenith: Color.rgb(0.25, 0.49, 0.78),
    horizon: Color.rgb(0.83, 0.89, 0.94),
    ground: Color.rgb(0.43, 0.42, 0.38),
  );
  world.fog = const Fog(start: 20, end: 120);
}

/// A photographed sky, which shiny things mirror.
Future<void> photographedSky(World world) async {
  world.sky = Sky.image(await world.assets.skyImage('sky.hdr'), rotation: 90.deg);
}
```

- **`Sky.gradient`** goes from `zenith` straight up to `horizon` level,
  and to `ground` (the horizon's, unless given) straight down. Its
  colours are shown as picked, so a game can take them from its theme:
  `scheme.tertiary.toFynkel()` (§17). Sheepdog sets a day sky in its
  light theme and a night one in its dark.
- **`Sky.image`** is a Radiance `.hdr`, equirectangular (twice as wide as
  high): its middle is ahead (−Z), its top row straight up. Poly Haven
  has hundreds, CC0; the 1k or 2k ones are plenty, as the sky is seen
  only behind everything. Its `intensity` scales its light; `rotation`
  turns it to the right. Load it as an asset, `assets.skyImage(name)`
  (§12), or from bytes with `SkyImage.load`.
- **`Sky.color`** is one colour, as `clearColor` is, but it lights the
  scene too: it adds to the ambient light.
- **It lights the scene.** A gradient or an image is blurred once, on the
  GPU, when it's set (a few milliseconds), into the light each way gets.
  What faces up is lit by the zenith and what faces down by the ground; a
  shiny surface mirrors the sky, a rough one shows only its colour, and
  metal in between shows it blurred. `lighting` scales that light (0 for
  none) without changing what's seen of the sky. A sky that lights does
  the ambient light's work: a scene that had a `Light.ambient` wants it
  dimmer.
- **What's mirrored is the sky alone**: the ground and other objects
  aren't seen in a shiny surface, nor do they hide the sky from it. A
  ball in a room still mirrors the sky outside.
- **`camera.showSky = false`** keeps a camera's `clearColor` behind
  everything (a minimap looking down on the field), while the sky still
  lights what it shows.

**Fog** fades what's further than `start` metres from the camera into
its `color`, all of it by `end`. Without a `color` it's the sky's
horizon, so far things melt into the sky; with no sky, the camera's
clear colour. `height` thins it upwards, halving every `height` metres
above y = 0: mist lying in a valley. It costs almost nothing, at every
level. The lab's 'Look: sky and fog' scene shows each kind of sky, metal
from a mirror to rough, and fog over an avenue 100 m long.

### See-through: glass, water and glows

A material's `opacity` (its colour's alpha) is how much of it there is.
Made with one below 1, the material is see-through:

```dart
/// Tinted glass, and a pool's water: a third and a half of them there.
final glass = Material(color: Color.rgb(1, 0.7, 0.3), opacity: 0.35, roughness: 0.1, doubleSided: true);
final water = Material(color: Color.rgb(0.12, 0.44, 0.55), opacity: 0.55, roughness: 0.05);

/// A spark: black, so all it shows is its glow, added to what's behind.
final spark = Material(color: Color.black, emissive: Color.rgb(1, 0.4, 0.1), blending: Blending.additive);

/// A ghost fading out over a second.
void fadeOut(GameObject ghost) {
  final material = ghost.get<MeshRenderer>()!.material!..blending = Blending.transparent;
  ghost.tween(1.0, 0.0, seconds: 1, onUpdate: (v) => material.opacity = v);
}
```

- **`Blending.transparent`** lays the surface over what's behind it: at
  an opacity of 0.35, about a third of it and two thirds of what's
  behind. Everything it shows fades with it, its highlights and glow
  too (glTF's BLEND, Unity's Fade). A model's BLEND materials load as
  this.
- **`Blending.additive`** adds the surface's light, times its opacity,
  to what's behind: glows, flames, sparks and beams. Black adds nothing,
  so give it an `emissive`. It brightens what's behind, so it shows best
  against the dark.
- **`blending`** is picked for you only by `Material(opacity:)` below 1.
  Setting `opacity` later changes the alpha alone: a material made
  opaque stays opaque (`blending` changes it), and its alpha is
  ignored.
- **They're drawn after everything solid**, the furthest object first
  (by its middle), each over what's behind it, and after the sky. Within
  one object the order is its triangles': a glass jar can draw its far
  side over its near one. If that shows, split the object in two. Two
  see-through objects that cross each other can't both be in front, so
  one of them is wrong where they cross.
- **They hide nothing.** What's drawn later shows through them, so they
  don't write to the depth buffer; a solid object behind one is still
  drawn, and a solid one in front of it hides it.
- **They take light and fog** as solid surfaces do, and **cast no
  shadow**.
- With an `alphaCutoff` too, what's below the cutoff isn't there, and
  the rest is see-through.
- Many see-through objects on top of each other cost more than solid
  ones: each pixel is drawn once per layer. The lab's 'Look: glass, water
  and glows' scene adds 400 panes to measure it.

### Glow and contact: bloom and ambient occlusion

A camera can have two effects, both off unless it's given them, as in
Unity: a game that wants neither writes nothing.

- **Bloom:** light brighter than white glows into what's around it: a
  lamp's bulb, a glowing sword, the sun on a metal ball. Something glows
  when its `emissive` times `emissiveIntensity` (or the light it
  reflects) is above 1. Drawn where `Quality.bloom` is on (medium and
  high).
- **Ambient occlusion:** surfaces darken a little where they meet, in
  corners and creases and under a crate on the ground. Only the ambient
  and sky light darken; a sun and its shadows stay as they are. Drawn
  where `Quality.ambientOcclusion` is on (high): a soft hint of contact,
  not a shadow.

```dart
/// A bulb bright enough to glow, and a camera that makes it glow.
final bulb = Material(color: Color.black, emissive: Color.rgb(1, 0.8, 0.4), emissiveIntensity: 6);
final camera = Camera(bloom: const Bloom());

/// More glow, and from dimmer light, for a dreamy look; darker corners.
void dreamy(Camera camera) {
  camera.bloom = const Bloom(strength: 2, threshold: 0.8);
  camera.ambientOcclusion = const AmbientOcclusion(intensity: 2);
}
```

- **`Bloom(strength:, threshold:)`**: `strength` 1 glows about as much
  as the light gives off, spread wide; 0 not at all. `threshold` is how
  bright light must be to glow: at 1, only what's brighter than white,
  so a lit white wall doesn't glow but the sun's glint on it may.
- **`AmbientOcclusion(intensity:, radius:)`**: `intensity` 1 is as dark
  as a corner is closed in, up to 4; `radius` is how far, in metres, a
  surface darkens one near it (half a metre unless given).
- **Colours shown as picked don't glow:** the camera's `clearColor` and
  a `Sky.gradient`, however bright. A `Sky.image` is light, and its sun
  glows.
- **Each camera has its own**, so a minimap or a second view goes
  without them, and costs nothing. Where the quality doesn't draw one,
  it costs nothing either. The lab's 'Look: glow and contact' page turns
  both on and off, at each level.
- **What they cost:** little on an iPhone 16 Pro Max (120 fps at every
  level with both) or a Mac. On an older Android phone they cost a lot:
  a Galaxy Note10+ at medium went from 58 to 40 fps with bloom (stage 8,
  before the Android frame pacing work). If your
  game runs on phones like it, use a quality without bloom there
  (`Quality.medium.copyWith(bloom: false)`), or `low`.

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

**The frame rate** is at most `world.targetFrameRate` frames a second, as
Unity's `Application.targetFrameRate`: 60 unless the game sets it, on
every device. A game that wants more sets 120 (or 0, for every frame the
display shows); one that wants less, 30. On a 120 Hz screen, 60 is every
other display frame: the world isn't updated and nothing is drawn in the
others, and taps and keys wait for the next frame of the world's. Change
it at any time, from a settings page for example:

```dart
/// The settings page's choice: smoother (the most a ProMotion screen
/// shows), or the default, which keeps the phone cool.
void chooseFrameRate(World world, {required bool smoother}) {
  world.targetFrameRate = smoother ? 120 : 60;
}
```

60 is the default because more costs the phone heat: Sheepdog at 120 on
an iPhone 16 Pro Max made it hot enough within a minute that iOS slowed it
to about 80; at 60 it holds 60 for 10 minutes and more
([the report](iphone-heat-report.md)). For 120 on an iPhone, the app's
`ios/Runner/Info.plist` needs `CADisableMinimumFrameDurationOnPhone`
(`true`), as every example has (`flutter create` leaves it out): without
it, iOS keeps the app's screen at 60, whatever the game asks. Behind a menu, a paused game draws less still
(`pauseWhenCovered`, §17).

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
  `Transform.lookAt(eye, target)`.
- `v.withY(0.5)` (and `withX`, `withZ`) changes one part, `v.flat` puts
  it on the ground (y = 0) for walking, and `Vec3.all(2)` is a uniform
  scale.
- **A zero vector has no direction**: `normalized` throws for one, so a
  vector that should never be zero says so where it is. Where one can be,
  as the way to a target the object may have reached, write
  `normalizedOrZero`, which gives zero (as Unity's `normalized`):
  `position += (target - position).normalizedOrZero * speed * time.delta`.
  `lookAt` the point the object is at leaves it as it is.
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
- Each returns a **`Motion`**: `stop()`, `complete()` (to where its last
  loop ends; one that loops for ever ends the loop it's in), and it's a
  future, so `await open();` waits for it to end.

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
  `emit()`.
- `emit(value)` calls every listener at once, in order; a
  `Signal<void>` is emitted with `emit()`. A handler that
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
  `point`, `normal`). `input.fingers` lists them. `position` is a
  `ViewPoint`, for `screenPointToRay`; `position.toVec2()` for a `Vec2`.
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
  the player (`Space`, `W`, `↑`). §17 has that dialog and a row of a
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
When it ends, the keys still down on the keyboard are held again (and
`wasPressed` that frame). The first frame after it counts no time, however
long the pause was:

```dart
// lib/paused_app.dart
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

void main() => runApp(
  FynkelApp(
    Score.new, // §17's
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

Models (`.glb`), textures (`.png`, `.jpg`) and sky images (`.hdr`, §5)
are loaded by name, from the app's `assets/` folder (list it under
`flutter: assets:` in `pubspec.yaml`):

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

**Flutter's asset folders aren't recursive.** `- assets/` lists the files
in `assets/`, not those in `assets/props/`: list each folder,

```yaml
flutter:
  assets:
    - assets/
    - assets/props/
```

as examples/coins does. A file in a folder that isn't listed isn't in
the app, and loading it fails with its name.

**A level from Blender.** Make the level in Blender and export it as a
`.glb` (File → Export → glTF 2.0, glTF Binary, +Y Up). Every Blender
object comes in as an object of the same name, placed as it was, empties
too: name the places the code needs (`start`, `gate`) and `find` them
(§18 has a level that does). The meshes, their materials and the
animations come in; the file's lights and cameras are skipped
(`Model.warnings` says what was), so light the level from code (§5). To walk on it, give it
colliders: `MeshCollider.addToParts(level)` (§16) gives every part its
own shape.

A model's clips and how to play them are §13.

## 12b. Meshes made while the game runs

A mesh can be made in code and changed while the game runs, as Unity's
`Mesh` is: water rippling, a flag waving, ground dug away. `Mesh()` is
empty; give it its vertices and triangles, channel by channel, in typed
lists:

```dart
// lib/waves.dart
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';

/// Water 4 m across that waves: a grid of 64 × 64 squares whose heights
/// change each frame, paler on the crests.
class Waves extends GameObject {
  static const squares = 64, across = squares + 1, size = 4.0;

  final mesh = Mesh();
  final _positions = Float32List(across * across * 3); // x, y, z a vertex
  final _colors = Uint8List(across * across * 4); // r, g, b, a: 0-255, sRGB

  @override
  void onLoad() {
    super.onLoad();
    final indices = Uint32List(squares * squares * 6);
    var at = 0;
    for (var z = 0; z < squares; z++) {
      for (var x = 0; x < squares; x++) {
        final a = z * across + x;
        // Two triangles a square, counter-clockwise seen from above.
        indices.setAll(at, [a, a + across, a + 1, a + 1, a + across, a + across + 1]);
        at += 6;
      }
    }
    _shape(0);
    mesh
      ..setPositions(_positions)
      ..setColors(_colors)
      ..setTriangles(indices)
      ..markDynamic(); // it changes every frame
    addComponent(MeshRenderer(mesh, Material(roughness: 0.15)));
  }

  void _shape(double t) {
    for (var z = 0; z < across; z++) {
      for (var x = 0; x < across; x++) {
        final i = z * across + x;
        final (px, pz) = (x / squares * size - size / 2, z / squares * size - size / 2);
        final y = math.sin(px * 3 + t * 2) * math.cos(pz * 2 + t) * 0.08;
        _positions
          ..[i * 3] = px
          ..[i * 3 + 1] = y
          ..[i * 3 + 2] = pz;
        final light = (0.5 + y * 6).clamp(0.0, 1.0);
        _colors
          ..[i * 4] = (40 + 170 * light).toInt()
          ..[i * 4 + 1] = (110 + 120 * light).toInt()
          ..[i * 4 + 2] = (170 + 80 * light).toInt()
          ..[i * 4 + 3] = 255;
      }
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _shape(time.sinceStart);
    // The same lists each frame, changed in place: nothing to collect.
    mesh
      ..setPositions(_positions)
      ..setColors(_colors);
  }
}
```

- **The channels**: `setPositions`, `setNormals`, `setTangents`,
  `setUvs` and `setColors` (four bytes a vertex, sRGB: they multiply the
  material's colour, white when none are given). Each is read back with
  `positions`, `normals`, `tangents`, `uvs`, `colors` and `indices`, new
  copies each time. `clear()` empties it.
- **Changes go to the screen once a frame**, at the end of
  `World.update`, however many were made: set a channel ten times in a
  frame and it's sent once. Each `set` copies the list at once, so the
  game can keep one list and change it in place each frame, which makes
  no garbage.
- **Normals and tangents are worked out for you** when they're sent,
  unless the game set its own (tangents only once it has UVs, as they
  come from them); `recalculateNormals()` and
  `recalculateTangents()` do it now, as Unity's. Setting positions of a
  different count clears the other channels: give them again.
- **`markDynamic()`** for a mesh that changes most frames: the GPU keeps
  it where it can be written in place (Unity's `MarkDynamic`). A mesh
  changed now and then doesn't need it.
- **Parts**: `setTriangles(indices, submeshStarts: [n])` makes two parts,
  the second starting at index `n`; `MeshRenderer.materials` gives each
  its material, the first part the first (Unity's submeshes and
  `Renderer.sharedMaterials`). `submeshCount` and `submesh(i)` read them.
- **A `MeshCollider` follows its mesh** (§16): after a change it's made
  again on the next physics step, and what lies on it wakes. Making it
  again takes time (about 0.2 ms for 1,000 triangles and 3 ms for 10,000
  on a Mac), so it suits ground dug where it's tapped, not water that
  moves every frame: that shouldn't have one.
- **What it costs**: the lab's water and flag, about 10,900 vertices
  changed every frame with their normals worked out, take the engine
  about 1.3 ms a frame on a Note10+ and 0.2 ms on a Mac, on top of the
  game's own work out of the new shape. A few thousand vertices a frame
  is nothing; tens of thousands want a look at the Profiler
  (`world.meshChangeTime` says what sending them took).
  Built-in shapes and models' meshes can't be changed: copy what you
  need into a `Mesh()`.

The lab's "Meshes that change" has a flag, water, and ground to dig
(examples/lab/lib/scenes/meshes_change.dart).

### Building meshes: shapes, outlines and a builder

Most meshes a game makes aren't vertex by vertex. There are shapes with
settings, outlines made solid, and a builder for the rest:

```dart
// Shapes with sizes and segments. Boxes and grids are textured by the
// metre (uvPerMetre), so a long wall tiles like a short one.
final wall = Mesh.box(size: Vec3(6, 2.5, 0.2), segmentsX: 6);
final ground = Mesh.grid(width: 40, depth: 40, segmentsX: 80, segmentsZ: 80);
final pillar = Mesh.cylinder(radius: 0.2, height: 3, segments: 12);
final ball = Mesh.sphere(radius: 2, segments: 48, rings: 24);
final roof = Mesh.cone(radius: 1.2, height: 0.8, segments: 4); // a pyramid
final ring = Mesh.torus(radius: 1, tube: 0.1);
final pad = Mesh.disc(radius: 1.5, inner: 1.2); // a ring flat on the ground
final pill = Mesh.capsule(radius: 0.3, height: 1.4);

// An outline with holes, made solid: a window frame 10 cm thick.
final frame = Shape2D.rounded(1.2, 1.6, 0.1).withHole(Shape2D.rect(1, 1.4));
final window = Mesh.extrude(frame, depth: 0.1);

// A profile turned around Y: (distance from the axis, height), with
// the outside on the right of the way it goes.
final bowl = Mesh.lathe([Vec2(0, 0), Vec2(0.3, 0), Vec2(0.5, 0.3), Vec2(0.46, 0.3), Vec2(0.27, 0.04), Vec2(0, 0.04)]);
```

- **The shapes without settings stay as they were**: `Mesh.sphere()` is
  still the one shared sphere. Give any setting and it's a new mesh of
  its own. A setting that makes no shape (2 segments, a size of 0) throws
  an `ArgumentError` that says which.
- **`Shape2D`** is an outline in the XY plane: `Shape2D.rect`, `circle`,
  `rounded`, or any list of points, either way round; `withHole` cuts
  another out of it. `Mesh.extrude` fills the ends and walls the sides;
  corners sharper than 40° are sharp and the rest smooth, so a circle
  comes out round and a square square.
- **`MeshBuilder`** builds the rest a face at a time, in plain Dart:

```dart
/// A shed: four walls (part 0) and a flat roof (part 1).
Mesh shed(double w, double d, double h) {
  final b = MeshBuilder();
  // Each face counter-clockwise seen from outside, bottom left first.
  b
    ..addQuad(Vec3(-w, 0, d), Vec3(w, 0, d), Vec3(w, h, d), Vec3(-w, h, d))
    ..addQuad(Vec3(w, 0, -d), Vec3(-w, 0, -d), Vec3(-w, h, -d), Vec3(w, h, -d))
    ..addQuad(Vec3(w, 0, d), Vec3(w, 0, -d), Vec3(w, h, -d), Vec3(w, h, d))
    ..addQuad(Vec3(-w, 0, -d), Vec3(-w, 0, d), Vec3(-w, h, d), Vec3(-w, h, -d))
    ..submesh(1)
    ..color = Color(0xFF884422)
    ..addQuad(Vec3(-w, h, d), Vec3(w, h, d), Vec3(w, h, -d), Vec3(-w, h, -d));
  return b.build();
}
```

  `addTriangle` and `addQuad` give each face its own flat corners;
  `addVertex` and `addIndices` share vertices, so faces meeting at one
  are smooth there (`MeshBuilder(flat: true)` makes them faceted
  anyway). `submesh(n)` picks the part, `color` colours the vertices
  after it, and `addMesh` adds another mesh, placed. `build()` makes a new
  mesh; `applyTo(mesh)` puts it in one there is, so stairs or a fence
  rebuilt when they change keep one mesh. `clear()` and build again:
  once its lists are big enough a builder makes no garbage.
- **`Mesh.combine`** joins meshes into one (Unity's `CombineMeshes`):
  houses, rocks or fence posts that never move apart, drawn with one draw
  a material instead of one each:

```dart
GameObject town(List<(double, double)> plots, Material walls, Material roofs) {
  final town = Mesh.combine([
    for (final (x, z) in plots) CombineInstance(shed(1, 1, 1), position: Vec3(x, 0, z)),
  ]);
  return GameObject(components: [MeshRenderer(town)..materials = [walls, roofs]]);
}
```

  Part n of each mesh goes into part n of the result, so the town above
  is two draws, however many sheds. A mirroring scale turns a mesh's
  triangles round so it still faces out. A combined mesh is one object:
  it can't be culled a house at a time, so keep each one to a
  neighbourhood rather than the whole map.
- **What it costs**: these are made in Rust. A shape or an extrusion of a
  few thousand triangles takes well under a millisecond; for a big one,
  make it in `Mesh.inBackground(() => Mesh.grid(...))`.

The lab's "Building meshes" has every shape with sliders for its
settings, a letter B with its holes, a vase, and a town built and
combined (examples/lab/lib/scenes/meshes_build.dart).

### Splines: roads, ropes and tracks

A `Spline` is a smooth curve through knots, as Unity's: a road, a river,
a rope, a cart's track, where the fence posts go. It's plain Dart, so a
game uses it to move and place things as much as to make meshes.

```dart
// Smooth through every point (Unity's auto tangents), or straight lines
// between them (Spline.linear), or knots with their own handles.
final river = Spline.smooth([Vec3(0, 0, 0), Vec3(8, 0, -4), Vec3(16, 0, 2)]);
final arch = Spline([
  Knot(Vec3(0, 0, 0), mode: TangentMode.bezier, tangentOut: Vec3(0, 3, 0)),
  Knot(Vec3(5, 0, 0), mode: TangentMode.bezier, tangentIn: Vec3(0, 3, 0)),
]);

// Everything along it is by distance in metres.
final halfway = river.positionAt(river.length / 2);
final facing = river.rotationAt(3); // -Z along it, up as upAt
final closest = river.nearest(Vec3(5, 0, 5)); // (distance:, position:)

// A road: a profile swept along it, caps on its ends, UVs in metres.
final road = Mesh.sweep(river, Shape2D.rect(4, 0.3), segmentsPerMetre: 2);
```

- **By distance, not t.** `positionAt`, `directionAt`, `upAt` and
  `rotationAt` take metres along it, so things go at an even speed and
  are evenly spaced; Unity's normalised t is `distance / length`. A
  `closed` spline joins its last knot to its first and distances go
  round.
- **Up** turns as little as it can along the curve (a rotation-minimising
  frame), so a tube doesn't twist where it bends; it starts as +Y, and a
  knot's `roll` turns it, clockwise seen going along: a banked turn.
- **Changing it**: `spline[i] = Knot(...)`, `add`, `insert`, `removeAt`,
  or `setPositions(floats)` for every knot at once; `version` goes up
  each time. `mesh.setSweep(spline, shape)` sweeps again into the same
  mesh: a road whose knots are dragged, or a rope that swings each frame,
  which makes no garbage when it's filled with `setPositions`.
- **Samples**: `segmentsPerMetre` (2 unless given) and more where it
  bends, at most `maxSegments`. A 100 m road with a 12-point profile
  takes about 0.1 ms in Rust on a Mac.

On objects, as Unity's Splines package has them:

```dart
/// A track round [points] with sleepers along it, and a cart riding it.
List<GameObject> railway(List<Vec3> points, Mesh sleeper, Material wood, GameObject cart) {
  final track = GameObject(components: [
    SplineContainer([Spline.smooth(points, closed: true)]),
    // Sleepers every 35 cm, placed again when the spline changes.
    SplineInstantiate((i) => GameObject(components: [MeshRenderer(sleeper, wood)]), spacing: 0.35),
  ]);
  cart.addComponent(SplineAnimate(track.get<SplineContainer>()!, speed: 2, loop: SplineLoop.loop));
  return [track, cart];
}
```

- **`SplineContainer`** holds splines in its object's space: move the
  object and they move with it. Its `positionAt` and the rest answer in
  the world, distances included: on an object scaled 2, `length()` is
  twice the spline's, and `positionAt(5)` is 5 m along in the world.
- **`SplineInstantiate`** makes copies with its function, hung from its
  object, `spacing` metres apart (along the spline in its own space, so
  scaling the object spaces them out with it) or `count` of them, turned
  along the spline unless `align` is false, `offset` from it.
- **`SplineAnimate`** moves its object along at a `speed` or over
  `seconds`, `once`, `loop` or `pingPong`, turned to face the way it goes;
  `play`, `pause`, `restart`, `distance` and `normalizedTime`.
- **`world.debug.drawSplines = true`** draws every container's splines
  and knots.

The lab's "Splines" has a road whose knots can be dragged, a rope that
swings between two posts, a fence and a cart on a track
(examples/lab/lib/scenes/splines.dart).

## 13. Animation

A model's animations come with it: `model.clips` lists them, and each
copy's `animator` plays them, as Unity gives a model its Animator. Which
clip plays when is the game's code, not a state-machine file.

### Playing clips


```dart
class Hero extends GameObject {
  late final Animator _animator;
  Vec3 _velocity = Vec3.zero;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('character.glb'));
    _animator = model.animator!..play('idle');
    _animator.at('walk', 0.3, () => print('step')); // a footstep
  }

  void jump() {
    _animator
      ..crossFade('jump', duration: 0.1, loop: false)
      ..onEnd = (clip) => _animator.crossFade('idle');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    position += _velocity * time.delta;
    if (_animator.current != 'jump') {
      _animator.crossFade(_velocity.length > 0.1 ? 'walk' : 'idle', duration: 0.2);
    }
  }
}
```

- `play(name)` starts a clip at once; `crossFade(name, duration:)` fades
  to it from what plays. Asking for the clip that's already playing
  changes nothing, so an update can say what it wants every frame.
- `loop: false` holds a clip's last pose at its end (a jump, a fall) and
  calls `onEnd`; clips loop unless told not to.
- `speed` (1 as made, 0 paused, negative backwards), `time`, `length`,
  `current`, `isPlaying(name)` and `weight(name)` are there too, and
  `stop()` keeps the pose it's in. Game time scales it: a paused world
  pauses its clips.
- `at(clip, seconds, callback)` calls back each time a clip passes that
  time: footsteps, the moment a sword hits. Each call adds one more:
  `removeMark(clip, seconds, callback)` takes one away (a pooled object
  that adds it in `onSpawn` takes it away in `onRelease`), and
  `removeMarks(clip)` all of a clip's, or `removeMarks()` all.
- The clips play in the engine, once a frame after `onUpdate` and the
  tweens and before `onLateUpdate`. Each moves only the parts it moves.
  To turn a part a clip moves (a head that looks at you), set it in
  `onLateUpdate`, after the clip; examples/garden does.
- A clip moves the model's parts in place. Moving the character
  through the world is the game's code, so physics stays in charge of
  where it is; match the speed to the walk's pace so the feet don't
  slide.
- The parts a clip moves are the model's objects: a sword added to the
  hand (`model.find('hand')!.add(sword)`) follows it.

### Skinned characters

A skinned character, one whose mesh bends with a skeleton (Mixamo's,
KayKit's, most from Blender), needs nothing more: its joints are its
copy's objects, the clips move them, and the engine bends the mesh to
them on the GPU, its shadow too. Every copy bends with its own joints.
So holding something is the same as for a model whose parts move whole:

```dart
class Rogue extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final rogue = add(await assets.instantiate('rogue.glb'));
    final sword = await assets.instantiate('sword.glb');
    rogue.find('handslot.r')!.add(sword); // KayKit's hands have a slot
    rogue.animator!.play('1H_Melee_Attack_Chop');
  }
}
```

- A file often carries every weapon its character can hold, all shown:
  turn off the ones you don't want (`find('2H_Sword')!.active = false`).
- A skin may have up to 256 joints, and a vertex follows up to four. A
  skin with more isn't bent (the mesh shows as it was bound), and a debug
  build prints why as the model loads, as it does whatever else a model
  skips (`model.warnings`).
- The lab's "Animation: skinned" shows two KayKit characters.

### Blends and layers

Two tools mix clips: a **blend**, for moving at any speed, and
**layers**, for playing one clip on part of the body over another:

```dart
class Runner extends GameObject {
  late final Animator _animator;
  double speed = 0; // m/s, as the game's input says

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('knight.glb'));
    _animator = model.animator!;
    // Layer 1: the right arm, from the shoulder down.
    _animator.layer(1, mask: 'upperarm.r');
  }

  void wave(bool on) {
    final arm = _animator.layer(1);
    on ? arm.crossFade('Cheer', duration: 0.3) : arm.stop(duration: 0.3);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    position += Vec3(0, 0, speed * time.delta);
    // Idle standing, the walk at 1.1 m/s, the run at 3.4.
    _animator.blend('move', const ['Idle', 'Walking_A', 'Running_A'], thresholds: const [0, 1.1, 3.4], at: speed);
  }
}
```

- `blend(name, clips, thresholds:, at:)` is Unity's 1D blend tree, as a
  call: each clip at its threshold (0, 1, 2… if left out), and `at`
  mixes the two around it. The clips keep one pace, each as far through
  itself, so the feet land together at any mix; with thresholds at the
  speeds the clips' feet go, they don't slide. Call it every frame with
  the number: it's made the first time, and fades in (`duration:`) only
  when something else was playing.
- `layer(1…7, mask:, additive:, strength:)` plays over the layers below
  it: on the whole model, or from a part down (`mask`, a name as
  `find` takes). An additive layer adds its clip's movement from the
  clip's first pose (a breath, a flinch over any pose) instead of taking
  the parts' place. `strength` (Unity's layer weight) shows it part-way.
  A layer has the animator's own `play`, `crossFade`, `blend`, `stop`,
  `current`, `time` and `weight`; the animator itself is the base layer.
- Every fade starts from the pose shown, however it got there: mid-fade,
  mid-blend, or held after a `stop()`. A game can change its mind every
  frame and the character never jumps. `stop(duration:)` fades a layer
  out to what's underneath.
- A layer's clips call `onEnd` and `at(…)` callbacks as the base's do.

### A character in a game

Put together, a character is a blend for its legs and a layer or two
over it. This one walks where it's told: it runs to a place far away,
slows to a walk near it and stands there, and waves from the waist up
without stopping. examples/coins's player is the same, with a coin in
its hand:

```dart
class Farmer extends GameObject {
  late final Animator _animator;
  Vec3 _goal = Vec3.zero;
  double _speed = 0; // m/s, eased, so the blend eases too

  static const walk = 2.0, run = 5.0; // the speeds the clips' feet go

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('character.glb'));
    _animator = model.animator!
      ..blend('move', const ['idle', 'walk', 'sprint'], thresholds: const [0, walk, run], at: 0, duration: 0)
      // A gesture is played once; then the layer fades out.
      ..onEnd = (_) => _animator.layer(1).stop(duration: 0.15);
    _animator.layer(1, mask: 'torso'); // the legs stay the blend's
  }

  void walkTo(Vec3 point) => _goal = point.flat;

  void wave() => _animator.layer(1).crossFade('wave', duration: 0.1, loop: false);

  @override
  void onUpdate() {
    super.onUpdate();
    final to = _goal - transform.position;
    final distance = to.length;
    final wanted = distance < 0.05 ? 0.0 : (distance > 3 ? run : walk);
    _speed += (wanted - _speed) * (6 * time.delta).clamp(0, 1);
    if (distance >= 0.05) {
      final step = to.normalized * (_speed * time.delta).clamp(0, distance);
      transform = Transform(position: transform.position + step, rotation: Quat.lookRotation(to));
    }
    _animator.blend('move', const ['idle', 'walk', 'sprint'], thresholds: const [0, walk, run], at: _speed);
  }
}
```

- The speed it moves at is the number the blend gets, and the
  thresholds are the speeds the clips' feet go: the feet don't slide at
  any speed between.
- Easing the speed eases the blend: it slows into a walk, not snaps.
- `onEnd` is on the animator, for every layer: here only the gesture
  layer plays clips that end.
- A test can check all of it frame by frame (§19):
  `animator.weight('sprint')`, `animator.layer(1).current`.

### Many characters

Each character's clips are sampled and blended in the engine, and its
mesh bent on the GPU, so a crowd costs little. The lab's "Animation: a
crowd" shows 1 to 100 KayKit characters, each on its own clip, and says
what the animation costs a frame. Measured with a hundred:

| Device | The animation a frame | Frames a second |
|---|---|---|
| Mac | 0.9–1.3 ms | 120 of 120 asked, at any quality |
| iPhone (ProMotion) | 2.1–2.8 ms | 120 of 120 at low and medium; at high, 80 (high's own cost: one character gives 80 too) |
| Galaxy Note10+ | 6–7 ms | 42 of 60 at low; 30 characters hold 60 |

Each character also takes about 160 KB of the engine's memory (its
objects and its animator); copies of one model share its meshes and
textures. For a crowd on an older Android phone, keep it to a few dozen
or turn the far ones' animators off (`stop()` holds their pose and costs
nothing).

## 13b. Sound

Sound is a package, `fynkel_audio`, with a mixer of its own. Add it to
the pubspec, next to `fynkel`, and a game without it ships none of it:

```yaml
  fynkel_audio:
    path: <the repository>/packages/fynkel_audio
```

**A clip** is a sound file, `.wav`, `.ogg` or `.mp3`, loaded by name as
any asset is (§12): `await assets.audio('bark.ogg')`. It's decoded once,
off the UI thread, shared, and freed when the last object that asked for
it is destroyed. `AudioClip.fromSamples(samples, rate:, channels:)`
makes one from numbers, freed with `dispose()`.

**An `AudioSource`** plays sounds from its object, as Unity's: its `clip`
with `play()`, `pause()`, `unPause()` and `stop()`, and any clip over it
with `playOneShot(clip, volume:, pitch:)`, for footsteps and hits. It has
Unity's `volume`, `pitch` (2 is an octave up and twice as fast), `loop`,
`mute`, `priority`, `playOnAwake` (on unless set: it plays `clip` when
it's enabled), `isPlaying` and `playTime` (Unity's `time`; here `time`
is the game's). `onEnd` is called when `clip` reaches its end, not when
it's stopped. Disabling the source, or destroying its object, stops its
sounds.

```dart
// lib/dog.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

/// Barks on B, and steps as it walks.
class Dog extends Behaviour {
  late final AudioSource voice;
  late final AudioClip bark, step;
  var _walked = 0.0;
  var _left = true;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    bark = await assets.audio('bark.ogg');
    step = await assets.audio('step.wav');
    voice = gameObject.addComponent(AudioSource(clip: bark, playOnAwake: false));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (input.wasPressed(KeyCode.b)) voice.play();
    final move = input.axis2('move');
    gameObject.position += Vec3(move.x, 0, -move.y) * (3 * time.delta);
    _walked += move.length * 3 * time.delta;
    if (_walked > 0.8) {
      _walked = 0;
      _left = !_left;
      voice.playOneShot(step, pitch: _left ? 0.95 : 1.05);
    }
  }
}
```

**The world's sound**, `audio` on every object and behaviour, is Unity's
`AudioListener` and `AudioSettings` together: `audio.volume` for
everything (a settings screen's master volume), `audio.playOneShot(clip)`
for a sound that belongs to no object (a button's click), `audio.stats`
(voices, underruns) and `audio.outputError`.

- **Pausing.** While the game is paused (`world.paused`, §6) its sounds
  hold where they are, and go on with it. A source with `ignorePause`
  plays on, as a pause menu's sounds should, and so does
  `audio.playOneShot(click, ignorePause: true)`. While the app is away
  (in the background, or a call comes in), nothing plays.
- **Slow motion.** `time.scale` doesn't change sound, as in Unity, unless
  a source has `followTimeScale`: then it goes slower and lower with the
  game, and holds when the game's time stands still.
- **32 voices.** As many sounds as that play at once. Past it the least
  important gives way: `priority` 0 is the most important, 256 the
  least, 128 unless set; of equals, one fading out goes first, then the
  oldest. A sound of priority 0 never gives way, as music doesn't, so
  keep 0 for music. A sound fading out (`music.crossFade`, `stop(fadeOut:)`) keeps
  its importance until it's done. The one that gives way fades out over
  a few milliseconds, so it doesn't click.
- **The iPhone's silent switch** mutes the game and the player's own
  music plays on under it, as most games do. `audio.session =
  AudioSession.playback` plays through the switch and stops their music,
  for a game whose sound is the point; on Android it takes the audio
  focus, which stops their music the same way. A game that listens on
  the mic while it plays, like a call, sets `AudioSession.voiceChat`
  before its first sound: the mic works alongside the game's sound,
  which plays from the loudspeaker through the switch, and the mic's
  plugin is told to leave the session alone (`record`'s
  `IosRecordConfig(manageAudioSession: false)`). On both, the game's
  sound waits while a call rings or is on; on Android, with `voiceChat`,
  a call over the internet doesn't count (the game's own voice chat puts
  the phone in that call's mode), only a phone call. On the iPhone it
  waits the same way while Siri, an alarm or a call shown over the game
  holds the sound, and goes on where it was when that's over.
- **The sound starts** when the world is first shown on screen. In a
  test, which shows nothing, the sound is made and timed the same way
  with no device: `world.update` steps it, a clip ends after its length,
  and `onEnd` is called.

**In 3D.** A source is 2D unless told otherwise: the same in both ears
wherever it is, as music and clicks should be. Give it a `spatialBlend`
of 1 and it's heard from where its object is, as Unity's:

- **Left and right** from where it is around whoever hears, and a little
  quieter and duller right behind, so behind isn't in front.
- **Fading with distance**: full within `minDistance` (1 m unless set),
  then by `rolloffMode`. `AudioRolloffMode.logarithmic` (the default)
  halves with each doubling of the distance, as sound does, and stops
  falling at `maxDistance` (500 m); `linear` falls in a straight line to
  silence at `maxDistance`, which is easier to place.
- **Who hears** is the camera, unless an object has an `AudioListener`
  (the player, in a game whose camera flies far behind). With more than
  one, the last enabled hears.
- **It moves** with its object. Once a frame, after the cameras move,
  where every playing 3D source and the listener are is read in one call,
  and the mixer slides each sound to its new place over a few
  milliseconds, so one that flies past doesn't crackle.
- `spatialBlend` between 0 and 1 blends the two; `panStereo` (-1 left to
  1 right) places the 2D part.
- `audio.playAt(clip, position)` plays a one-shot at a place that belongs
  to no object (Unity's `PlayClipAtPoint`): an explosion, a coin picked
  up. It stays where it was played.
- `audio.drawSources = true` draws each 3D source's `minDistance` and
  `maxDistance` as spheres with the debug lines (§13), to see where
  each can be heard.

```dart
// lib/hive.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

/// A hive that hums, heard from where it is, and bursts with a pop.
class Hive extends Behaviour {
  late final AudioClip pop;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    pop = await assets.audio('pop.wav');
    gameObject.addComponent(
      AudioSource(
        clip: await assets.audio('hum.ogg'),
        loop: true,
        spatialBlend: 1,
        maxDistance: 25,
        rolloffMode: AudioRolloffMode.linear,
      ),
    );
  }

  void burst() {
    audio.playAt(pop, gameObject.position);
    gameObject.destroy();
  }
}
```

Sound in 3D costs the frame little: on the Note10+, 32 sounds moving
take under a tenth of a millisecond.

**Music** is `audio.music`: one track at a time, looping unless told
not to, playing on through a pause (as a pause menu's music should), and
never given up for other sounds. `play(clip, fadeIn:)` starts a track
(the one already playing plays on, so a level that starts its music
again doesn't restart it), `crossFade(clip, seconds)` fades the next in
as the last fades out, and `stop(fadeOut:)` ends it. Its `onEnd` is
called when a track that doesn't `loop` reaches its end.

**A long clip streams.** A file over 10 seconds is decoded as it plays,
a second ahead, on a thread of its own, so a three-minute song takes its
file's memory rather than 30 MB of samples. `AudioClip.streamed` says
which; `AudioClip.decode(bytes, stream: true)` streams a short one, and
`stream: false` decodes a long one whole. It plays as it would decoded,
from any `time`, looping without a gap; `audio.stats.starved` counts the
times a stream wasn't ready (it should stay 0).

**Groups** are a settings screen's "music" and "effects" (a group of
Unity's `AudioMixer`): `audio.group('effects')` is made the first time
it's asked for, and up to 15 can be. Each has a `volume` and `mute`
under `audio.volume`, and `fade(to, seconds)`, all heard within a few
milliseconds, without a click. A source joins one with `group`, a
one-shot with `playOneShot(clip, group:)` or `playAt(..., group:)`, and
the music plays in `audio.group('music')`.

```dart
// lib/sound_settings.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

/// Calm music that turns to a chase when an enemy sees the player, and
/// sliders' values for the settings screen.
class Soundtrack extends Behaviour {
  late final AudioClip calm, chase;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    calm = await assets.audio('music/calm.ogg');
    chase = await assets.audio('music/chase.ogg');
    audio.music.play(calm, fadeIn: 2);
  }

  void seen() => audio.music.crossFade(chase, 1.5);
  void lost() => audio.music.crossFade(calm, 4);

  /// From the settings screen's sliders, 0 to 1.
  void settings({required double music, required double effects}) {
    audio.group('music').volume = music;
    audio.group('effects').volume = effects;
  }

  /// Quieter music while a character speaks.
  void speaking(bool on) => audio.group('music').fade(on ? 0.25 : 1, 0.5);
}
```

**Hot reload** reads a changed sound file again: the clip sounds as the
file now does from the next time it's played (what plays plays on). A
file that's no longer sound keeps the old, and the error goes to
`world.errors`.

In a test, `audio.render(frames)` makes the mix as a speaker would have
played it, left and right, to check what plays and where it's heard.

Values out of range (a volume over 1, a pitch of 0) throw an
`ArgumentError`; a file that isn't sound throws a `FormatException` with
its name.

## 13c. Particles

A **`ParticleSystem`** is a component, as Unity's is: smoke, sparks,
dust and splashes, made by the engine. It has Unity's main module and
its defaults (`duration` 5 s, `looping`, `startLifetime`, `startSpeed`,
`startSize`, `startRotation`, `startColor`, `gravityModifier`,
`simulationSpace`, `simulationSpeed`, `maxParticles` 1000, `playOnAwake`,
`stopAction`, `randomSeed`), an `emission` (a rate a second and up to 8
`Burst`s), a `shape` they're made in, and a size and a colour over each
one's life. It draws with its `material`, as a square facing the camera
for each particle; without one, it moves its particles and draws none.

```dart
/// A fountain of water, and sparks that clean up after themselves.
class Fountain extends GameObject {
  Fountain()
    : super(
        name: 'fountain',
        components: [
          ParticleSystem(
            startSpeed: const Range(5, 6),
            startSize: const Range(0.1, 0.25),
            startLifetime: const Range(1.2, 1.6),
            gravityModifier: 1,
            simulationSpace: ParticleSpace.world,
            emission: const Emission(rateOverTime: 120),
            shape: ParticleShape.cone(angle: 12.deg, radius: 0.1),
            sizeOverLifetime: (t) => 1 - 0.6 * t,
            colorOverLifetime: ColorGradient.fadeOut(),
            material: Material(color: const Color(0xFF8FD3FF), blending: Blending.transparent),
          ),
        ],
      );

  static final _spark = Material(color: const Color(0xFFFFB347), blending: Blending.additive);

  /// A puff of sparks at [at], on an object that goes when they do.
  GameObject sparks(Vec3 at) => add(
    GameObject(
      transform: Transform(position: at),
      components: [
        ParticleSystem(
          looping: false,
          duration: 1,
          startSpeed: const Range(2, 5),
          startLifetime: const Range(0.5, 1),
          stopAction: StopAction.destroy,
          emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(60, 80))]),
          shape: const ParticleShape.sphere(radius: 0.1),
          colorOverLifetime: ColorGradient([(0, Color.white), (0.3, const Color(0xFFFFC040)), (1, Color.rgb(1, 0.2, 0, 0))]),
          material: _spark,
        ),
      ],
    ),
  );
}
```

- **Values that vary.** `Range(min, max)` is Unity's "random between two
  constants", picked for each particle as it's made; `Range(5)` is a
  constant. `startColor` is a `Color`, or a `ColorRange(a, b)` for one
  between them.
- **Over a life**, from 0 (made) to 1 (gone): `sizeOverLifetime` is a
  function, an `Ease` or your own, times `startSize`;
  `colorOverLifetime` is a `ColorGradient` of stops, times `startColor`,
  its alpha fading as the colour does (`ColorGradient.fadeOut()`,
  `fadeInOut()`). Both are read 64 times along the life when set, so
  they cost nothing a frame.
- **Shapes** face +Y, as Unity's do, turned with their object:
  `ParticleShape.point()`, `sphere(radius:)`, `hemisphere`,
  `cone(angle:, radius:)` (Unity's default), `box(size:)`,
  `circle(radius:, arc:)` and `edge(radius:)`. `radiusThickness` 0 makes
  them on the surface alone; `randomDirection` 1 sends them any way.
- **Local or world space.** In `ParticleSpace.local` (the default) the
  particles move with their object, as a jet's flame should; in
  `ParticleSpace.world` they stay where they were made, as a trail of
  smoke should. Gravity pulls down the world either way.
- **Playing.** It plays when its object comes, unless `playOnAwake` is
  false. `play()`, `stop()` (no new ones, those in the air live on),
  `stop(clear: true)`, `pause()`, `clear()`, `emit(n)` and
  `simulate(seconds)` (moved on at once: an effect seen half-way
  through); `isPlaying`, `isPaused`, `isEmitting`, `isStopped` and
  `particleCount`. Turned off, or its object hidden, it stops and
  clears; on again, it plays from the start.
- **When it's done**, its last particle gone and not looping,
  `stopAction` does what Unity's does: `destroy` destroys its object (a
  one-off explosion), `disable` turns it off (one from a pool, §7, which
  plays again when it's turned on), `callback` calls `onStopped`.
- **Time.** It goes with the game's time: it holds while the game is
  paused and follows `time.scale`; one with `useUnscaledTime` goes on,
  for a pause menu's.
- **The material.** Particles are unlit and fogged, as Unity's default
  particle material is: each one's colour times the material's `color`
  and its `baseColorTexture` (a soft dot, a puff of smoke), plus its
  `emissive`. `Blending.transparent` lays them over what's behind,
  furthest first; `Blending.additive` adds their light, for fire, sparks
  and magic, and needs no order. A material that isn't blended is drawn
  as transparent. Behind something solid, they're hidden.

**The look.** Unity's other modules, by the same names:

```dart
/// A fire: [flames] a texture of 4 × 4 frames of a flame.
ParticleSystem fire(Texture2D flames) => ParticleSystem(
  startSize: const Range(0.8, 1.2),
  startLifetime: const Range(0.7, 1),
  emission: const Emission(rateOverTime: 40),
  forceOverLifetime: const ParticleVector(y: ParticleCurve(2)),
  noise: const ParticleNoise(strength: ParticleCurve(0.4), frequency: 1.5, scrollSpeed: 1),
  textureSheet: TextureSheet(columns: 4, rows: 4),
  softness: 0.3,
  material: Material(baseColorTexture: flames, blending: Blending.additive),
);
```

- **Curves.** A module's numbers are `ParticleCurve`s, Unity's
  `MinMaxCurve`: `ParticleCurve(2)` a constant,
  `ParticleCurve.between(1, 3)` random between two constants,
  `ParticleCurve.curve((t) => …, scale:)` a curve over each particle's
  life, `ParticleCurve.curves(a, b)` random between two curves. Each
  particle keeps its own place between the two for its whole life.
- **Movement.** `velocityOverLifetime` (a `ParticleVector`, x, y and z
  curves, in local or world space) is added to how it moves;
  `forceOverLifetime` speeds it up; `limitVelocity` holds its speed to a
  `limit` (by `dampen`, 0 to 1, a second) and slows it by `drag`;
  `rotationOverLifetime` turns it, radians a second; `noise` pushes it
  about in a field that moves with `scrollSpeed` (`octaves` up to 4, each
  costing about as much again).
- **By speed.** `sizeBySpeed` and `colorBySpeed` scale its size and
  colour from `minSpeed` to `maxSpeed`: sparks white when fast, red as
  they slow.
- **Texture sheets.** `textureSheet` splits the base colour texture into
  `columns × rows` frames, played over each life (`frameOverTime`,
  `cycles`), from a `startFrame`, or one row each (`randomRow`): flames,
  explosions.
- **How they're drawn.** `renderMode`: `ParticleRenderMode.billboard`
  facing the camera (the default); `.stretched(lengthScale:,
  speedScale:)` long the way they move (sparks, rain);
  `ParticleRenderMode.horizontal` lying flat (ripples, splashes);
  `ParticleRenderMode.vertical` standing up and turning to the camera
  round up; `.mesh(mesh)` a mesh each, turned round up by its rotation
  (leaves, rocks). A mesh in use can't be disposed.
- **Soft particles.** `softness`, in metres: they fade out as they come
  that close to what's behind, so smoke doesn't cut a hard line where it
  meets the floor. At `Quality.low`, which has no depth to read, they're
  hard.
- **Lit particles.** `lit: true` lights them as a surface facing the
  camera, with the material's `metallic` and `roughness`, as Unity's
  Particles/Lit is: smoke the sun falls on.
- **A material's own shader** (§14) shades them, its `unlit` or `surface`
  function, with `particle_age()` (0 to 1 through its life) and
  `particle_random()` (its own number) to tell them apart.

**Particles and the world**, as Unity's modules of the same names:

- **Emission over distance.** `Emission(rateOverDistance: 7)` makes 7 a
  metre its object moves: dust under feet and wheels, none standing
  still.
- **Inherit velocity.** `inheritVelocity: InheritVelocity(curve:)` gives
  each particle that much of its object's speed when it's made
  (`InheritVelocityMode.initial`), or all through its life
  (`.current`); a negative one throws dust back from the way it goes.
- **Sub emitters.** `subEmitters: [SubEmitter(system, type)]`, up to 4:
  another system, a child of this one, makes its burst (its own
  `emission`'s bursts) where a particle is born, dies or hits something
  (`SubEmitterType.birth`, `.death`, `.collision`); a birth one keeps
  making its rate along the particle as it flies. `inherit:
  {SubEmitterInherit.color, .size, .rotation}` passes on what the
  particle had, and `probability` makes only some of them do it. A sub
  emitter runs only for its parent, never on its own timeline:
  fireworks are a shell with a trail on birth and a burst on death.
- **Collision.** `collision: ParticleCollision(planes: [floor])`, up to 6
  objects, each a plane through its position facing its up: the floor,
  a wall. A particle hitting one bounces (`bounce`, 0 to 1, as Unity's),
  loses speed (`dampen`) and life (`lifetimeLoss`), and dies below
  `minKillSpeed`; `radiusScale` sizes it for hitting. With
  `sendCollisionMessages`, `onParticleCollision(plane, count)` is called
  once a frame for each plane hit, with how many particles hit it (Unity's
  `OnParticleCollision`). Collision with the physics world's colliders is
  not in this version.
- **Trails.** A `TrailRenderer` is a component, as Unity's is: a strip
  left behind its object as it moves, `time` seconds long, a point each
  `minVertexDistance`, `widthMultiplier` wide times `widthCurve`
  (`(t) => 1 - t` tapers it), coloured by `colorGradient` from the object
  (0) to the tail (1), drawn with its `material` as particles are.
  `emitting = false` lets it run out; `clear()` drops its points (a
  ball put back on the tee); `positionCount` says how many it has. It
  keeps at most 512.

```dart
/// Sparks off a grinder that bounce on the floor and flash where some hit.
GameObject grinder(GameObject floor, Texture2D dot) {
  final glow = Material(baseColorTexture: dot, blending: Blending.additive);
  final flash = ParticleSystem(
    startLifetime: const Range(0.15),
    startSpeed: const Range(0),
    emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(1))]),
    material: glow,
  );
  return GameObject(
    name: 'grinder',
    components: [
      ParticleSystem(
        startSpeed: const Range(2.5, 4),
        gravityModifier: 1,
        simulationSpace: ParticleSpace.world,
        emission: const Emission(rateOverTime: 70),
        collision: ParticleCollision(
          planes: [floor],
          bounce: const ParticleCurve(0.5),
          lifetimeLoss: const ParticleCurve(0.15),
          sendCollisionMessages: true,
        ),
        subEmitters: [SubEmitter(flash, SubEmitterType.collision, probability: 0.3)],
        onParticleCollision: (plane, count) => print('$count hit ${plane.name}'),
        material: glow,
      ),
    ],
    children: [GameObject(name: 'flash', components: [flash])],
  );
}
```

`Quality.softParticles` turns soft particles on (medium and high) or off
(low, where reading the depth costs too much on a phone), as Unity's
quality setting of the same name. `world.debug.drawShapes = true` draws
each system's shape and the planes it collides with, in the debug lines,
as Unity's Scene view shows them when one is selected.

The lab's "Particles and the world" has fireworks, a rolling ball with
dust and a trail, and sparks bouncing off the floor.

Settings change while it plays, and its particles stay. A value it
can't have (a negative lifetime, a cone over 90°, more than 8 bursts)
throws an `ArgumentError` and the old one stays.

**What it costs.** Each system is one draw, however many particles it
has. The engine moves 10,000 particles in about 0.03 ms a frame on a
Mac, 0.1 ms on an iPhone 16 Pro Max and 0.3 ms on a Galaxy Note10+;
`world.particleStepTime` says how long the last frame's took (Unity's
Profiler's ParticleSystem.Update). A frame with systems playing and bursting makes
no garbage. The modules cost more:
on a Mac, 10,000 particles take about 0.08 ms more for each of velocity
over lifetime, force and limit velocity, 0.04 ms for rotation, and 0.15
ms for noise (0.4 ms with three octaves), as Unity's noise is its most
expensive module. Those 10,000, small, hold 60 fps at every quality
level on all three.

**Drawing over and over** is what costs most on a phone: big
see-through particles covering the screen, each pixel drawn once for
every puff in front of it. The lab's "screen of smoke" fills the whole
screen with puffs two to three metres across, five metres from the
camera (`LAB_SMOKE=200`, or any count). On a Galaxy Note10+ at low
(a quarter fewer pixels, `renderScale` 0.75), 200 and 500 hold 60,
1,000 bring it to between 28 and 48, 2,000 to 27; at medium and high,
drawing every pixel, 200 bring it to 12. An iPhone 16 Pro Max holds 60
with 200 at every level and with 500 at low, and falls to 40 with 500
at high. A Mac holds 60 with 200. So:

- **Keep big puffs to a few hundred on screen at once.** A puff that
  covers a tenth of the screen costs as much as thousands of sparks.
  Sheepdog's dust is under half a metre across and gone in a second.
- **Make them smaller and fewer, and fade them sooner.** A puff that
  grows to twice its size costs four times as much at the end of its
  life; a shorter `startLifetime` means fewer on screen.
- **Fewer pixels help most**: low's `renderScale` and its soft particles
  off (`Quality.softParticles`, which reads the depth for every pixel)
  are why a Note10+ holds 60 with 200 at low and 12 at high.
- **Additive costs as much as transparent**: both draw every pixel; additive
  only saves the sort.

## 14. Shaders

A material's look can be written by hand: a `.wgsl` file (WebGPU's
shading language) with one function in it. The engine does the rest:
lights, shadows, fog, every quality level, skinned characters. Only
what's different goes in the shader. Screen effects are shaders too,
run over the camera's whole picture.

`packages/fynkel/wgsl/fynkel.wgsl` lists every name the engine gives a
shader, with what it does: read it beside your own. The lab's
`examples/lab/assets/shaders/` has a dozen to copy from (lava, water,
grass in the wind, a hologram, a dissolve, a hit flash, toon light, an
old TV), and `dart run fynkel:shader lava` writes a new one to start from,
`assets/shaders/lava.wgsl`, and prints how to use it (`--unlit`,
`--effect`, `--moves` and `--light` choose what it has).

### A first shader

Put it in `assets/shaders/` (list the folder in `pubspec.yaml`, §12):

```wgsl
// assets/shaders/lava.wgsl
struct Values { hot: vec4<f32>, speed: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let flow = fbm(in.uv * 6.0 + vec2<f32>(0.0, frame_time() * values.speed));
    s.color = mix(vec3<f32>(0.05), values.hot.rgb, flow);
    s.emission = values.hot.rgb * flow * flow * 4.0;
    return s;
}
```

and give it to a material:

```dart
class LavaPool extends GameObject {
  late final Material lava;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    lava = Material(shader: await assets.shader('shaders/lava.wgsl'), values: {'hot': Color(0xFFFF6A10), 'speed': 0.2});
    add(GameObject(transform: Transform(scale: Vec3(4, 1, 4)), components: [MeshRenderer(Mesh.plane(), lava)]));
  }

  void cool() => tween(0.2, 0.0, seconds: 2, onUpdate: (v) => lava.set('speed', v));
}
```

- `surface` returns what the surface is (colour, roughness, glow...), and
  the engine lights it. `standard(in)` is the material as the engine has
  it, its textures and colour in, so a shader changes one thing and keeps
  the rest. `unlit` instead returns the colour as it is, unlit.
- `struct Values` is what the game sets: numbers and vectors of `f32`,
  `i32` and `u32`. `values: {...}` and `material.set(name, value)` set them
  by name, with a `double`, an `int`, a `Vec2`, a `Vec3`, a `Color` (given
  to the shader in linear light, as the engine's own colours are) or a
  list of numbers. An array, `colors: array<vec4<f32>, 8>`, takes a list
  of those (`material.set('colors', [red, green])`), up to its length;
  the rest are 0. A name the shader doesn't have, or a value of the wrong
  kind, is an `ArgumentError` that lists what it has. Tween one as above.
- Textures: `var rock: color_texture;` (colours) or
  `var heights: texture_2d<f32>;` (data), read with `sample(rock, in.uv)`,
  set like values: `material.set('rock', await assets.texture('rock.png'))`.
- `frame_time()` is the game's time, stopping when it's paused.
- `particle_age()` (0 to 1 through its life) and `particle_random()`
  (its own number, 0 to 1) tell a particle drawn with the material
  (§13c) from the others; on anything else they're 0.
- `hash`, `noise`, `noise3`, `fbm`, `fresnel`, `triplanar` and `remap`
  are there for patterns, rims and textures without UVs.

**Each object its own numbers.** `struct PerObject` holds up to four
`f32`s each object sets for itself, so many objects share one material
(and one draw) and still differ: a hit flash.

```wgsl
// assets/shaders/flash.wgsl
struct PerObject { flash: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    s.color = mix(s.color, vec3<f32>(1.0), per_object.flash);
    return s;
}
```

```dart
class Slime extends GameObject {
  void hit() {
    final renderer = get<MeshRenderer>()!;
    tween(1.0, 0.0, seconds: 0.3, onUpdate: (v) => renderer.set('flash', v));
  }
}
```

### Moving vertices

A `vertex` function moves the mesh's vertices: grass in the wind, waves,
a wobble. The shadow moves with it. Say how far it can move a vertex
with the material's `reach`, in metres, so the engine doesn't leave the
object out of a frame it has moved into:

```wgsl
// assets/shaders/sway.wgsl
struct Values { wind: f32 }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    // The tip moves, the foot stays: local.y is the height in the mesh.
    let gust = sin(frame_time() * 2.0 + in.origin.x * 0.7) * values.wind;
    v.position.x += gust * in.local.y * in.local.y;
    return v;
}

fn surface(in: SurfaceInput) -> Surface {
    return standard(in);
}
```

`Material(shader: sway, values: {'wind': 0.3}, reach: 0.3)`. Each copy of
a mesh has its own `origin`, so a field of them sways out of step, in one
draw.

### What's behind

A see-through surface can read the scene behind it: `depth_behind(in)`
(how far behind it the scene is, in metres: shallow water foams, a force
field glows where it meets the floor) and `scene_color(screen)` (the
scene's colour there: move `screen` a little to bend it, as water and
glass do). A shader that reads them is drawn see-through, over the
solid scene. They cost memory only in a frame where such a shader is in
view.

### Whole styles

- **Your own light.** Beside `surface`, `fn light(in: LightInput) ->
  vec3<f32>` lights it one sun or lamp at a time, in place of the engine:
  toon bands, light that wraps round. It's given the light's way, colour,
  shadow and distance and what `surface` returned. `standard_light(in)`
  is the engine's, so it can change some lights and not others. The
  ambient and sky light, glow, fog and tone mapping stay the engine's.

```wgsl
// assets/shaders/toon.wgsl
struct Values { bands: f32 }

fn surface(in: SurfaceInput) -> Surface {
    return standard(in);
}

fn light(in: LightInput) -> vec3<f32> {
    let lit = max(dot(in.normal, in.direction), 0.0) * in.shadow;
    let band = mix(0.3, 1.0, floor(lit * values.bands + 0.5) / values.bands);
    return in.surface.color * in.color * band / 3.14159265;
}
```

- **Outlines** are a material's: `material.outline = Outline(width: 0.02,
  color: Color(0xFF181410))`, on any opaque material, with a shader or
  not, skinned and moved meshes too. Where an object stands on the
  ground, the ground hides its outline's lower edge.
- **The pixel look**: `camera.pixels = 240` draws the game 240 pixels
  high and enlarges it with hard edges. The camera keeps to whole pixels,
  so the picture doesn't shimmer as it moves.

### Screen effects

An effect's shader has `fn effect(in: EffectInput) -> vec4<f32>`: the
colour of each pixel of the finished picture, read with `picture(uv)`.
Its `Values` are set by name, as a material's:

```wgsl
// assets/shaders/hurt.wgsl
struct Values { amount: f32 }

fn effect(in: EffectInput) -> vec4<f32> {
    let seen = picture(in.uv).rgb;
    let edge = smoothstep(0.1, 0.5, length(in.uv - 0.5)) * values.amount;
    return vec4<f32>(mix(seen, vec3<f32>(0.9, 0.05, 0.05), edge), 1.0);
}
```

```dart
class Hurt extends GameObject {
  late final ScreenEffect red;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    red = ScreenEffect(await assets.shader('shaders/hurt.wgsl'), values: {'amount': 0.0});
    world.camera!.effects = [red];
  }

  void flash() => tween(1.0, 0.0, seconds: 0.5, onUpdate: (v) => red.set('amount', v));
}
```

A camera's effects run in order, each over what the one before made:
`camera.effects = [palette, oldTv]` isn't `[oldTv, palette]`. They work
at every quality level; a frame without any pays nothing for them.

An effect can have textures, as a material's shader can. A **colour
grade** is one: a table of colours, made in an image editor (or Unity's
neutral table, coloured to taste), each colour of the picture looked up
in it with `color_lut`:

```wgsl
// assets/shaders/grade.wgsl
struct Values { amount: f32 }
var table: color_texture;

fn effect(in: EffectInput) -> vec4<f32> {
    let seen = picture(in.uv).rgb;
    return vec4<f32>(mix(seen, color_lut(table, seen), values.amount), 1.0);
}
```

```dart
class Evening extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final grade = ScreenEffect(
      await assets.shader('shaders/grade.wgsl'),
      values: {'table': await assets.texture('evening_lut.png'), 'amount': 1.0},
    );
    world.camera!.effects = [grade];
  }
}
```

The table is a strip of square tiles side by side, as many as each is
wide (256 × 16 or 1024 × 32 pixels): red across a tile, green down it,
blue from tile to tile. An effect can read **the scene's depth** too:
`scene_depth(in.uv)` is how far the solid scene is at a pixel, in metres
along the view, and `scene_position(in.uv)` where it is in the world, for
fog of your own, a scanner's pulse ringing out from
`camera_position()`, or outlines where the depth jumps. See-through
surfaces and the sky aren't in it; a frame with an effect that reads it
keeps it, and others pay nothing. The lab's effects page has both.

### Mistakes, changes and the first frame

- **A shader that doesn't compile** throws a `ShaderError` with its file,
  line and column (`lava.wgsl:5:12: ...`), its own lines, not the
  engine's around them.
- **Hot reload takes a shader's new code**: change the file, press
  hot reload, and every material using it draws with it from the next
  frame. Code that doesn't compile keeps the old and says why on
  `world.errors`.
- **The first time a shader is drawn** on a device, the GPU's driver
  compiles it: a third of a second on a Mac, more on a phone, then kept
  by the system for every run after. A view on screen doesn't wait for
  it: it draws without those objects for those frames. Behind a loading
  screen, let the view draw the level and `await world.shadersReady()`,
  so nothing pops in when the game shows.

### What shaders cost

Measured in stage 10 (docs/stage-10-report.md): the lab's shader
scenes hold 60 fps on a Mac and an iPhone at every level, and 120 when
asked; a Note10+ holds 60 at low in each.

- **A surface or a vertex shader** costs about what the engine's own
  material does: a few lines more of the same work. 2,000 blades of
  grass swaying in the wind add about 0.1 ms a frame on a Mac.
- **Reading what's behind** (`depth_behind`, `scene_color`) costs most:
  the frame is drawn in three passes instead of one while such a
  surface is in view. The lab's pond and glass ball hold 60 at low on
  a Note10+, but 48 at medium and 41 at high. On a slower phone, give low quality a
  material that doesn't read it: Sheepdog's pond is still water at low,
  which took it from 40 back to 50 fps on the Note10+.
- **A shader's first frame** on a device: on a Note10+ its pipelines
  take up to a second to be ready, then the phone keeps them for every
  run after. On an iPhone, up to about half a second. Wait for them
  behind a loading screen (above).

### Limits

Four textures and four `PerObject` numbers to a shader, 512 bytes of
`Values`: numbers, vectors and arrays of them (no matrices yet, §21).
A shader can't use another's code.

## 15. Cameras, layers and debug drawing

**Cameras** work as Cinemachine's. Put a `VirtualCamera` on objects; the
enabled one with the highest `priority` drives the view's camera, and a
change of priority blends to it (as does destroying the live one's
object: from where the view is to the next):

```dart
class CameraRig extends GameObject {
  CameraRig(GameObject player) : _player = player;

  final GameObject _player;
  late final VirtualCamera close, wide;

  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(components: [Camera()])); // the view's camera
    close = addWith(
      VirtualCamera(priority: 1),
      components: [Follow(_player, offset: Vec3(0, 3, 6), damping: 0.2), LookAt(_player)],
    );
    wide = addWith(VirtualCamera(fovY: 70.deg), components: [Orbit(_player, distance: 15)]);
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
  now; world widgets (§17) move with it.
- **Between the screen and the world**: on a `Camera`, such as
  `world.camera!`, `screenPointToRay(point)`, `screenPointToPlane(point,
  Plane.ground)` and `worldToScreenPoint(point)`; `world.raycast(ray)`
  gives what a ray hits first (a `RayHit`: `gameObject`, `point`,
  `normal`, `distance`).
- **A camera that follows a physics body** sees each fixed step's jump:
  a body steps 50 or 100 times a second, the screen draws at 60 or 120,
  and the camera moves smoothly, so the body seems to jitter. Give the
  body `interpolation: RigidbodyInterpolation.interpolate` (§16), as in
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
builds. `debug.drawShapes = true` draws each particle system's shape and
the planes its particles hit (§13c).

## 16. Physics: colliders, rigidbodies, queries and callbacks

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
stretches it on each axis. When its mesh changes (§12b) it follows by
itself on the next step. A model's parts each get one with
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
as they are when it joins them, at the scale they have then. If the
other body's object is destroyed, `connectedBody` goes null and the
joint holds its body to the world where it is, as in Unity. With a `breakForce` or
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

## 17. Flutter around the game

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

**On a phone the view is under Flutter, not in it** (on Android with a
`FynkelActivity`, §1). Widgets over the view draw over the game, as
everywhere: the HUD, a pause menu, a dialog's dimmed background. Three
things differ from the Mac:

- Whatever is painted under the view doesn't show: the view clears its
  place, so the game shows through. That's what you want.
- A transform, clip or opacity from the widgets above the view doesn't
  reach the game. A page sliding in is followed (the view moves to where
  its widget is, each frame); one that fades or zooms in shows the game
  unfaded and at full size, and a view in a scrolling list isn't clipped
  at the list's edges.
- Two views stack, no more: a minimap over the game. A view lying within
  another goes over it.

Where one of those matters, give that view `onScreen: false`: it's a
texture Flutter draws, as on the Mac, slower on some Android phones and
warmer on an iPhone.

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
score wait where they were, and, as nothing in it moves, it's drawn only
10 times a second, which keeps a phone cool behind a menu. A game that was
paused already stays paused.
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

## 18. Hot reload and starting over

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

**Levels.** There are no scene files: a level is an object, and what it
adds hangs from it. The root holds one at a time, and what outlives them:

```dart
// lib/levels.dart
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';

/// One level, made in Blender (§12). Destroying it clears it: its
/// objects, its routines, and the assets only it held.
class Level extends GameObject {
  Level(this.number) : super(name: 'level $number');

  static const last = 3;
  final int number;

  /// Where the player starts: the empty named `start` in Blender.
  var start = Vec3.zero;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final scene = add(await assets.instantiate('levels/level_$number.glb'));
    start = scene.find('start')?.worldTransform.position ?? Vec3.zero;
    // The next one loads while this one is played, so it comes at once.
    if (number < last) assets.preload(['levels/level_${number + 1}.glb']);
  }
}

/// The game's root.
class Levels extends GameObject {
  Levels({this.first = 1});

  final int first;
  final level = ValueNotifier(0);
  Level? _current;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await go(first);
  }

  /// Swaps the level there is for level [number]. The new one comes
  /// whole before the old one goes: no empty frame, and what both use
  /// stays loaded.
  Future<void> go(int number) async {
    final old = _current;
    final next = _current = add(Level(number));
    await next.loaded;
    old?.destroy();
    level.value = number;
  }
}
```

**Saving.** What's kept between runs is plain data, never objects: the
level reached, the best score, the settings (`Controls` has `toJson` and
`fromJson`, §17). Write it as JSON to a file in the app's own folder,
which the app finds with the `path_provider` package
(`getApplicationSupportDirectory()`) and hands to the game:

```dart
// lib/save.dart
import 'dart:convert';
import 'dart:io';

/// What the game keeps between runs.
class Save {
  Save({this.level = 1, this.best = 0});

  int level;
  int best;

  Map<String, Object?> toJson() => {'level': level, 'best': best};

  /// Reads [file]; a new save when there's none, or it can't be read
  /// (an older game's, or cut short).
  static Save read(File file) {
    try {
      final json = jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
      return Save(level: json['level'] as int? ?? 1, best: json['best'] as int? ?? 0);
    } catch (_) {
      return Save();
    }
  }

  /// Writes it whole: to a file beside [file] first, then renamed over
  /// it, so a crash halfway leaves the last save, not half of this one.
  void write(File file) {
    File('${file.path}.new')
      ..writeAsStringSync(jsonEncode(toJson()))
      ..renameSync(file.path);
  }
}
```

Save when something worth keeping happens (a level done), not every
frame: a write is a few milliseconds on a phone.

## 19. Testing a game

A game runs in a test with no window and no app around it, frame by
frame, with made-up input. Run the tests with **`flutter test`**, not
`dart test`: package:fynkel itself is plain Dart, but a game is a Flutter
package (it uses `ValueNotifier` and the rest of `foundation.dart`, and
its tests `flutter_test`), and only Flutter's tool resolves those.
`flutter test` builds the engine through the same hook as `flutter run`,
for the Mac it runs on.

```dart
// test/walker_test.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:my_game/game.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('D walks right', () {
    final world = World();
    addTearDown(world.dispose);
    final walker = world.add(Walker());
    world.input.simulate.keyDown(KeyCode.d);
    world.run(1); // a second at 60 fps
    expect(walker.position.x, closeTo(4, 0.1));
  });
}
```

package:fynkel/testing.dart gives a world `run(seconds)`, `tap(x, y)`
and `viewPoint(x, y)`. `world.input.simulate` takes `fingerDown`,
`fingerMove`, `fingerUp`, `resize` and the rest, so drags and swipes can
be tested too; it's what the view itself calls. A plain test has no view,
so give the input a size first, `world.input.simulate.resize(800, 600)`,
and then `world.tap(400, 300)` lands where you say, on the next frame. To
put one on an object, ask the camera where it is on screen:
`world.camera!.worldToScreenPoint(target.position, width: 800, height:
600)`.
`HeadlessView(world, width:, height:)` (package:fynkel) renders frames
without a window, for a picture of the game: `view.png()` draws a frame of
the world as it is now and gives it as a PNG, and
`File('shot.png').writeAsBytesSync(view.png())` saves it to look at, which
finds an unlit side or a camera too far fast. To check pixels in a test,
`view.read(view.frame()).rgba` draws a frame and gives its pixels: RGBA,
a byte each, rows from the top. Dispose of the view when done
(`addTearDown(view.dispose)`):

```dart
/// The red, green and blue at (x, y) in a frame drawn now, 0 to 255.
List<int> pixel(HeadlessView view, int x, int y) {
  final i = (y * view.width + x) * 4;
  return view.read(view.frame()).rgba.sublist(i, i + 3);
}
```

 `examples/arena/test` tests a whole game this
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
import 'package:fynkel/testing.dart';
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
    world.run(3 / 60);
    await pumpEventQueue(); // let the stream deliver what the frames sent
    expect(errors, hasLength(3));
  });
}
```

**Compare positions with a tolerance.** `Vec3` and `Quat` have
`closeTo(other)`, so `expect(a.position.closeTo(Vec3(1, 0, 0)), isTrue)`.
An object with a rigidbody needs it most: physics keeps poses in 32-bit
floats, so after a step its position reads `0.20000000298`, not `0.2`.

**Is it fair? A test can say.** Play whole rounds in a test with a
scripted player that only does the obvious thing, and one that does
nothing: the first should win, the second lose. Move the player as the
game would (`walk(...)`, `world.update(1 / 60)`), and loop until the round
is over. Sheepdog's (`examples/sheepdog/test/game_test.dart`, "is fun")
found the wolves herding the flock home better than no dog at all, which
reading the code hadn't.

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

## 20. Speed

What keeps a game fast:

- **Every speed times `time.delta`.** Then the game is the same at 60 and
  120 fps.
- **60 frames a second is plenty** for most games, and keeps a phone cool
  (§6). Ask for 120 only where it shows, a fast game, and measure it for
  10 minutes, not one: a phone slows itself down when it's hot.
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
  needs to costs least. Turn a lamp off with `enabled = false`: it's
  left out altogether.
- **Shaders that read what's behind cost most** (§14): the frame takes
  three passes while one is in view. On an older phone give low quality
  a material that doesn't read it.
- **Views on screen** (`onScreen`, on unless turned off; on Android with
  a `FynkelActivity`, §1), so Flutter neither waits for the game's
  frames nor copies them.
- **A cheaper quality for an older phone.** `world.quality =
  Quality.low` draws in one pass, with no image between the scene and the
  screen; or take from a level only what costs most there, bloom first,
  or draw fewer pixels (`renderScale`) (§5).
- **Measure** on the slowest phone you support. Each example has a
  `tool/measured.dart` for package:fynkel_measure's tool (a
  dev_dependency, so the game itself doesn't carry it), which gives the
  frame rate and memory: `dart run fynkel_measure:measure rate`, at the
  game's own frame rate (`--define FYNKEL_FRAME_RATE=0` for as fast as it
  goes). On an iPhone or a Mac it gives the thermal state too, and each
  minute's frame rate with `--seconds 600`.

## 21. What isn't there yet

- **Drawing:** reflections of the scene itself (a mirror shows the sky
  alone), sprites and 2D, 3D text, a model's own vertex colours (a
  mesh made in code has them, §12b);
  see-through objects don't cast shadows.
- **Shaders** (§14): no matrices in `Values`; a shader can't use
  another's code; no node graph (Shader Graph): shaders are written in
  WGSL.
- **Animation** (§13): morph targets (faces, blend shapes), 2D blends
  (by direction and speed), root motion, IK (feet on slopes, a look
  beyond a turned head), clips moved from one skeleton to another
  (Unity's Humanoid), and clips made in code.
- **Sound** (§13b): effects on the sound (reverb, echo, a low-pass for
  under water), Doppler, 3D over headphones (HRTF), a custom rolloff
  curve, sound blocked by walls, ducking by itself (a group's `fade` does
  it by hand), the microphone, and sound made in code as it plays. On
  Android, another app taking the sound doesn't pause the game's.
- **Particles** (§13c): particles on the GPU (Unity's VFX Graph), lights
  on particles, collision with the physics world's colliders (only
  planes), triggers, wind zones, a mesh's surface as a shape, shadows
  from particles, a `LineRenderer`, and an editor.
- **Input:** gamepads.
- **The game:** navmeshes and path finding; scene files and saving, which
  a game does itself for now (§18 shows how); an editor.
- **Physics:** 2D physics, `ConfigurableJoint`, and chains of joints that
  don't stretch (Rapier's multibody joints).

## 22. From Unity

| Unity | Fynkel |
|---|---|
| `GameObject`, a prefab | `GameObject`, a subclass of it |
| `MonoBehaviour` | `Behaviour` |
| `AddComponent<T>()`, `GetComponent<T>()` | `addComponent(T())`, `get<T>()` |
| `Destroy(go)`, `Destroy(go, t)` | `go.destroy()`, `go.destroy(after: t)` |
| `SetActive(false)`, `enabled = false` | `active = false`, `enabled = false` |
| `ObjectPool<T>`: `Get`, `Release` | `Pool(parent, make)`: `get`, `release` |
| `Awake`, `Start`, `Update`, `LateUpdate`, `OnDestroy` | `onLoad`, `onStart`, `onUpdate`, `onLateUpdate`, `onDestroy` |
| `FixedUpdate`, `Time.fixedDeltaTime` | `onFixedUpdate`, `time.fixedDelta` |
| `Time.deltaTime`, `Time.timeScale` | `time.delta`, `time.scale` |
| `Time.timeScale = 0` for a pause menu | `world.paused = true`: time and input stop |
| `Application.targetFrameRate` | `world.targetFrameRate` (60 unless set) |
| The render scale (URP), `QualitySettings` | `Quality.renderScale`, `world.quality` |
| Anti Aliasing (MSAA) | `Quality.antiAliasing`: 0, 2 or 4 |
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
| `new Mesh()`, `SetVertices`, `SetTriangles`, `MarkDynamic` | `Mesh()`, `setPositions`, `setTriangles`, `markDynamic()` (§12b) |
| ProBuilder's shapes with settings | `Mesh.box`, `grid`, `sphere`, `cylinder`, `cone`, `torus`, `disc`, `capsule` with sizes and segments |
| ProBuilder's extrude, a lathe | `Mesh.extrude(Shape2D, depth:)` with holes, `Mesh.lathe(profile)` |
| `Mesh.CombineMeshes(CombineInstance[])` | `Mesh.combine([CombineInstance(mesh, position:, ...)])`, a part each kept apart |
| Splines: `Spline`, `BezierKnot`, `TangentMode`, `Evaluate`, `GetLength`, `SplineUtility.GetNearestPoint` | `Spline` with `Knot`s and `TangentMode`, `positionAt` / `directionAt` / `upAt` / `rotationAt` by distance, `length`, `nearest` |
| `SplineContainer`, `SplineInstantiate`, `SplineAnimate` | the same three, as components |
| `SplineExtrude` | `Mesh.sweep(spline, shape)` for any profile, `setSweep` to sweep again |
| `BoxCollider`, `SphereCollider`, `CapsuleCollider`, `MeshCollider` | the same, from `fynkel_physics` |
| A non-convex `MeshCollider` on a dynamic `Rigidbody` (refused) | collides as its hull |
| `Rigidbody`, `ForceMode`, `RigidbodyConstraints`, `PhysicMaterial` | `Rigidbody`, `ForceMode`, `RigidbodyConstraints`, `PhysicsMaterial` |
| `Rigidbody.velocity` | `linearVelocity` (as Unity 6's) |
| A small ball rolling fast at 50 steps a second | physics turns a body at most a quarter turn a step: take more steps (`time.fixedDelta = 1 / 100`) |
| `MaterialPropertyBlock` for one object's colour | `renderer.color` |
| `new Material(material)` | `material.copy()` |
| Rendering Mode Fade, Transparent; an additive shader | `Material(opacity:)`, `blending: Blending.transparent` or `.additive` |
| Shader Graph, a Surface Shader, URP's Lit shader with a custom function | a `.wgsl` file with `fn surface` (§14), starting from `standard(in)` |
| An Unlit shader | `fn unlit` |
| Vertex displacement (a Shader Graph vertex position) | `fn vertex`, with the material's `reach` |
| Shader properties, `material.SetFloat/SetColor/SetTexture` | `struct Values`, `material.set(name, value)` |
| `MaterialPropertyBlock` for a shader's numbers | `struct PerObject`, `renderer.set(name, value)` |
| `_CameraDepthTexture`, `_CameraOpaqueTexture` | `depth_behind(in)`, `scene_depth(screen)`, `scene_color(screen)` |
| A custom lighting function (toon) | `fn light` |
| A full-screen pass (a renderer feature, a post-process) | `ScreenEffect`, `camera.effects` |
| Color Grading's lookup texture (a LUT) | an effect with `color_lut(table, colour)` (§14) |
| `_CameraDepthTexture` in a full-screen pass | `scene_depth(in.uv)`, `scene_position(in.uv)` in an effect |
| Shader warm-up, `ShaderVariantCollection.WarmUp` | `await world.shadersReady()` behind a loading screen (§14) |
| Bloom and SSAO (a Volume, a renderer feature) | `Camera(bloom: Bloom(...), ambientOcclusion: AmbientOcclusion(...))`, off unless given |
| Quality Settings, a URP asset per level | `world.quality = Quality(...)` or `Quality.low/medium/high.copyWith(...)` |
| `Camera.orthographicSize` | `orthographicSize`, on `Camera` and `VirtualCamera` |
| Cinemachine's binding modes (world space, lock to target, lock to target on assign/with world up) | `Follow(space: FollowSpace.world / target / targetYaw)` |
| `Input.mouseScrollDelta` | `input.scroll`: +y with the wheel turned away, as Unity's |
| `Plane`, `Plane.distance`, `GetDistanceToPoint` | `Plane`, `distance`, `distanceTo` |
| `Instantiate(prefab)` of a model | `await assets.instantiate('model.glb')` |
| `Animator.Play`, `CrossFade`, `speed`, an Animator Controller | `model.animator!.play`, `crossFade`, `speed`; the game's code is the state machine |
| A 1D blend tree; animator layers with an avatar mask, `SetLayerWeight` | `animator.blend('move', clips, thresholds:, at: speed)`; `animator.layer(1, mask: 'spine', additive:, strength:)` |
| `AudioSource`: `Play`, `Pause`, `UnPause`, `Stop`, `PlayOneShot`, `time` | `AudioSource` (from `fynkel_audio`): `play`, `pause`, `unPause`, `stop`, `playOneShot`, `playTime` |
| `AudioClip` in the project | `await assets.audio('bark.ogg')` (`.wav`, `.ogg`, `.mp3`) |
| `AudioListener.volume`, `AudioListener.pause` | `audio.volume`; `world.paused` holds the game's sounds |
| `ignoreListenerPause` | `ignorePause` |
| `spatialBlend`, `panStereo`, `minDistance`, `maxDistance`, `rolloffMode` | The same names on `AudioSource` (no custom rolloff curve) |
| An `AudioListener` on the camera | `AudioListener`; without one, the world's camera hears |
| `AudioSource.PlayClipAtPoint(clip, position)` | `audio.playAt(clip, position)` |
| The 3D sound gizmos of a selected source | `audio.drawSources = true` |
| `AudioSettings`' Mute Other Audio Sources (iOS) | `audio.session = AudioSession.playback` (on Android too, by the audio focus) |
| An `AudioMixer`'s groups, their volume and mute; `outputAudioMixerGroup` | `audio.group('music')`: `volume`, `mute`, `fade(to, seconds)`; a source's `group` |
| Music on a looping `AudioSource`, crossfades by hand | `audio.music`: `play(clip, fadeIn:)`, `crossFade(clip, seconds)`, `stop(fadeOut:)` |
| An `AudioClip`'s Load Type: Streaming | Clips over 10 s stream; `AudioClip.decode(bytes, stream:)` |
| `ParticleSystem`: main module, `emission`, `shape`, size and colour over lifetime | `ParticleSystem`: the same names; `sizeOverLifetime` a function, `colorOverLifetime` a `ColorGradient` |
| `MinMaxCurve` "Random Between Two Constants", `MinMaxGradient` "Random Between Two Colors" | `Range(min, max)`, `ColorRange(a, b)` |
| `ParticleSystem.Play`, `Stop`, `Pause`, `Clear`, `Emit`, `Simulate`, `particleCount` | `play`, `stop(clear:)`, `pause`, `clear`, `emit`, `simulate`, `particleCount` |
| Stop Action: Destroy, Disable, Callback (`OnParticleSystemStopped`) | `stopAction: StopAction.destroy`, `disable`, `callback` (`onStopped`) |
| Simulation Space: Local, World | `simulationSpace: ParticleSpace.local`, `world` |
| A particle material (Particles/Unlit), Alpha or Additive | A `Material` with `Blending.transparent` or `Blending.additive` |
| `MinMaxCurve` (Constant, Curve, Random Between Two Constants or Curves) | `ParticleCurve(v)`, `.curve(f)`, `.between(a, b)`, `.curves(f, g)` |
| Velocity, Limit Velocity, Force and Rotation over Lifetime; Noise | `velocityOverLifetime`, `limitVelocity`, `forceOverLifetime`, `rotationOverLifetime`, `noise` |
| Size by Speed, Color by Speed | `sizeBySpeed: SizeBySpeed(f, minSpeed:, maxSpeed:)`, `colorBySpeed: ColorBySpeed(gradient, …)` |
| Texture Sheet Animation (Grid) | `textureSheet: TextureSheet(columns:, rows:)` |
| Renderer's Render Mode: Billboard, Stretched, Horizontal, Vertical, Mesh | `renderMode: ParticleRenderMode.billboard`, `.stretched()`, `.horizontal`, `.vertical`, `.mesh(mesh)` |
| Particles/Lit; Soft Particles | `lit: true`; `softness` |
| Emission's Rate over Distance; Inherit Velocity | `Emission(rateOverDistance:)`; `inheritVelocity: InheritVelocity(mode:, curve:)` |
| Sub Emitters: Birth, Collision, Death; Inherit Color, Size, Rotation | `subEmitters: [SubEmitter(system, SubEmitterType.birth, inherit: {SubEmitterInherit.color})]` |
| Collision (Planes): Dampen, Bounce, Lifetime Loss, Min Kill Speed; `OnParticleCollision` | `collision: ParticleCollision(planes:, dampen:, bounce:, lifetimeLoss:, minKillSpeed:)`; `onParticleCollision(plane, count)` |
| `TrailRenderer`: Time, Min Vertex Distance, Width, Color, Emitting, `Clear()` | `TrailRenderer(time:, minVertexDistance:, widthMultiplier:, widthCurve:, colorGradient:, emitting:)`, `clear()` |
| Quality's Soft Particles | `Quality.softParticles` |
| A sound slowed with `Time.timeScale` (by setting its `pitch`) | `followTimeScale: true` |
| Animation events, `StateMachineBehaviour.OnStateExit` | `animator.at(clip, time, callback)`, `removeMark`, `removeMarks`, `animator.onEnd` |
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
  not `UpArrow`, `LeftShift`, `Alpha1`. Letters are plain: `KeyCode.q`,
  not `keyQ`.
- **`MeshRenderer.material` is shared**, as Unity's `sharedMaterial`: change
  `renderer.color` for one object, or give it `material.copy()`.
- **`find('name')` searches everything below**, not only the children
  (Unity's `Transform.Find`), and takes no paths.
- **`Mesh.plane()` is 1 × 1**, Unity's Quad facing up; Unity's Plane is
  10 × 10.
- **An error in `onLoad` destroys the object** (§6); one in Unity's `Awake`
  leaves it half-made.
- **The physics step runs between frames** (§16), so a force shows a
  frame later; `physics.stepBetweenFrames = false` steps as Unity does.
- **A new `physics.gravity` wakes every body**; Unity leaves sleeping ones
  hanging.
- **Exit callbacks always come**: for a collider disabled, destroyed,
  hidden or moved to a layer that doesn't meet; Unity skips some.
- **A `CharacterController` turns with its object**; Unity's stays
  upright. Keep its object upright and turn a child for the model.
- **A HUD is Flutter**: widgets over the view, not a Canvas in the scene.

## 23. What changed

For code written before: each change that breaks it, newest first.

- **Stage 14's fixes** (2026-10-27): behaviour that was wrong, put
  right; code that relied on it changes.
  - A joint whose `connectedBody`'s object is destroyed holds its body
    to the world where it is, and `connectedBody` goes null; it used to
    wait for that body for good.
  - A collider turned off and on again before a step gets no exit from
    the trigger it's in (it used to get enter, then exit).
  - Destroying the live virtual camera's object blends from where the
    view is to the next one, with `onLiveChanged`; it used to make the
    next `update` throw, then cut.
  - An add the engine refuses (a camera's `fovY` of 500°, say) adds
    nothing, wherever below the root the refused component is: the world
    is as it was and the objects can be added once mended. A root used
    to stay in the world half built, its later children could never be
    added, and its lights stayed counted.
  - `Motion.complete()` on one that loops a number of times ends where
    its last loop would: a yoyo of 2 back at the start, a `moveBy`
    sequence of 3 loops three times as far. It used to stop at the end
    of the loop it was in.
  - A `SplineContainer`'s distances are metres in the world, as its
    points are: on an object scaled 2, `positionAt(5)` is 5 m along, not
    10, and a `SplineAnimate` at `speed: 1` goes 1 m a second, not 2.
    `length()` is new, the length in the world. On one scaled unevenly,
    `rotationAt` faces along `directionAt`.
  - `Follow` with `lookAhead` stays as far ahead as it was while the
    game is paused; it used to jump back to the target and out again.
  - `Animator.removeMark(clip, time, callback)` and `removeMarks([clip])`
    take away what `at` added; before, a pooled object that called `at`
    each time it came back was called once more each time.
  - A sound fading out keeps its `priority` until it's done: a less
    important one no longer takes its voice and cuts it off, and a
    sound that takes a voice fades the old one out over a few
    milliseconds instead of cutting it.
  - A sound of `priority` 0 is never given up, as music isn't (it plays
    at 0): with every voice busy at 0, a new one isn't played, where the
    oldest gave way before.
  - On Android, `AudioSession.voiceChat` no longer pauses the game's
    sound while the phone is in a call's mode for talking over the
    internet (`MODE_IN_COMMUNICATION`, which WebRTC's libraries set for
    the game's own voice chat). A phone call, ringing or on, still
    pauses it.

- **Limits instead of crashes** (2026-10-26, stage 14):
  - A particle system's `duration` is from 0.05 to 100,000 s: more
    throws, as less did. `emit`, bursts and the rate stop at what fits
    in `maxParticles`, and `simulate` and a prewarm take at most 1,800
    steps (a minute at 30 a second); a longer `simulate` is coarser,
    not slower.
  - A view's sides are at most what its device draws (8,192 on every
    phone here): `FynkelView` asks for no more, and a bigger one is
    refused instead of losing the device.
  - A material's own shader that uses a name the particles' code has
    too (`fy_soft`, say) is refused as it's made, as any other mistake
    in it is, rather than when a particle system first draws with it.

- **Splines** (2026-10-25): an outline swept along a path has
  `fy_mesh_sweep` and `fy_mesh_set_sweep` (ABI 44). Code that only uses
  package:fynkel changes nothing.

- **Building meshes** (2026-10-24): shapes with settings, outlines
  extruded and lathed, and meshes combined have functions of their own
  (`fy_mesh_new_box` and the rest, `fy_mesh_extrude`, `fy_mesh_lathe`,
  `fy_mesh_combine`; ABI 43). Code that only uses package:fynkel
  changes nothing: `Mesh.sphere()` and the others without settings are
  the shared shapes, as before.

- **Meshes that change** (2026-10-24): a vertex is 52 bytes, with a
  colour; `FyMeshData` has `colors`, `submesh_starts` and
  `submesh_start_count` (zero for none), and a mesh can be changed
  through `fy_mesh_set_*` and `fy_mesh_update` (ABI 42). The physics
  library has `fp_mesh_set` (physics ABI 14). Code that only uses
  package:fynkel changes nothing.

- **Low draws fewer pixels** (2026-10-19): `Quality.low` has a
  `renderScale` of 0.75, so a phone that picks low (Android's) draws at
  three quarters of its pixels across, a little softer and about a
  quarter faster. `Quality.low.copyWith(renderScale: 1)` draws as before.
  `Quality` has `antiAliasing` (4 at every level, as before);
  `FyQuality` has `samples` (ABI 38).

- **A new shader doesn't hold the frame up** (2026-10-18): a view on
  screen draws without a material's objects the first time its shader is
  drawn on the device, while the driver compiles it (a third of a second
  or more), and draws them from the frame it's ready. A loading screen
  waits for them with `await world.shadersReady()` (§14). Screen
  effects can have textures (a colour grade's table) and read the
  scene's depth; `FyScreenEffect` has a `textures` field (ABI 37).
  `Values` can have arrays.

- **The iPhone draws straight to the screen** (2026-10-17):
  - A view draws to a layer of its own under Flutter's view, not to a
    texture, and what's painted under it, or a fade over it, no longer
    shows on it (§17), as on Android.
  - `FynkelView.androidSurface` is `onScreen`, for both phones:
    `onScreen: false` keeps the texture.

- **60 frames a second unless the game asks for more** (2026-10-17):
  - The world runs and draws at most `world.targetFrameRate` (60) frames
    a second; it ran at the display's rate, 120 on a ProMotion iPhone or
    Mac. Set `world.targetFrameRate = 120` (or 0) for that (§6).
  - `FynkelView.onFrame` comes once a frame of the world's, not once a
    display frame. A widget test that pumps 8 ms frames sees every other
    one: pump 17 ms, or set the world's `targetFrameRate = 0`.
  - A view covered by a page or a dialog that paused the game draws 10
    frames a second (§17).

- **Android draws straight to the screen** (2026-10-16):
  - With the activity a `FynkelActivity` (§1), a view draws to the
    screen behind Flutter's, not to a texture, and what's painted under
    it, or a fade over it, no longer shows on it (§17). An activity left a
    `FlutterActivity` works as before.
  - `Quality.device` is `low` on Android phones (§5); it was `medium`.

- **Quality is settings, not three levels** (2026-10-16):
  - `Quality` is a class, not an enum: `Quality.low`, `medium` and
    `high` are still there, and `Quality.levels` replaces
    `Quality.values`. A game can make its own (`Quality(...)`) or change
    a level's (`Quality.medium.copyWith(bloom: false)`) (§5).
  - A camera has no bloom or ambient occlusion unless given them: add
    `bloom: const Bloom()` and `ambientOcclusion: const
    AmbientOcclusion()` to a camera that had them by default.

- **Bloom and ambient occlusion** (2026-10-15):
  - A camera has both unless given `bloom: null` and
    `ambientOcclusion: null` (§5): at medium quality, light brighter than
    white glows; at high, corners darken a little too. A material whose
    `emissiveIntensity` is above 1 now glows, and the sun's glint on
    metal may.

- **See-through materials** (2026-10-15):
  - A model's see-through (BLEND) materials are drawn see-through, as
    glTF means: they were opaque, with a warning. They're
    `Blending.transparent` (§5).
  - A material's alpha shows when it blends: `Material(opacity:)` below
    1 makes it see-through. An opaque one still ignores it.

- **Hardening, the build hook and the guide** (2026-10-15):
  - The build hook builds on any Mac Flutter builds on: Android's linker
    comes from Flutter's NDK and the app's `minSdkVersion`, and cargo
    runs `--locked` (the versions in `Cargo.lock`) with Flutter's
    macOS and iOS deployment targets. `flutter run -v` shows it working.
  - New in the guide: asset folders aren't recursive, and a level made in
    Blender (§12); levels and saving (§18); why `flutter test` (§19).

- **Hardening, what a game writes** (2026-10-15):
  - `Camera`, `Light` and `MeshRenderer` have `enabled`, as behaviours
    did: it's on `Component` now (§5).
  - `lookAt` the point the object is at leaves it as it is; it threw.
    `Vec3.normalizedOrZero` is new (§7).
  - An object's own `release()` on one released already does nothing; it
    threw. `destroy(after:)` and `release(after:)` are new (§3).
  - New helpers: `Vec3.withX`/`withY`/`withZ`, `flat` and `Vec3.all`,
    `Transform.lookAt(eye, target)`, `Light.sun(direction:)`,
    `ViewPoint.toVec2()`, and `emit()` for a `Signal<void>`
    (`emit(null)` still works). `addWith(component)` adds an object
    holding a component and returns the component (§3).
  - `input.stick()` (a finger dragging as a stick), `Follow.portraitFit`
    (a follow camera that goes further back upright), and in
    package:fynkel_flutter `Controls`, `ControlsSettings` and
    `askForKey` (keys a player changes, §11).
  - package:fynkel/testing.dart: `world.run(seconds)`, `world.tap(x, y)`
    and `world.viewPoint(x, y)` for tests (§19).
  - The examples' `lib/measured.dart` is `tool/measured.dart`, and
    package:fynkel_measure is a dev_dependency: move yours, and the tool
    finds it there (it still finds the old place).

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
  - Keys still down when a pause ends, or when the view gets focus back,
    are held again; they stayed up until pressed again.
  - `World.dispose` closes `world.errors`.

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
