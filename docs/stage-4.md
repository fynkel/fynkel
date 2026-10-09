# Stage 4 plan: a solid base for writing games

Status: accepted 2026-10-09, with the recommended answer to each question in §5. Scope and
rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 3 is done
([report](stage-3-report.md)).

Stage 3 settled how a game is written, but only as far as two small
games needed. Writing them showed what's still missing before animation,
physics or audio can be built on top: behaviours that can be put on any
object, a lifecycle and a clock like Unity's, a way to wait over time,
input that says much more than "tapped here", cameras that move well,
and tweens. Every game needs these, and each later system (animation,
physics, audio) will use them.

Stage 4 makes Fynkel work **the way Unity works**, in Dart: objects with
behaviours on them, `time.delta`, coroutines, input events, virtual
cameras and tweens. Where Dart lets it be simpler or safer than Unity, it
is. Stage 4 ends when a third game, a small arena, is written with all of
it, the garden and coins are moved to it, and all three run as well as
before on all three devices.

What a game looks like at the end:

```dart
class Enemy extends GameObject {
  Enemy(Vec3 at) : super(
    transform: Transform(position: at),
    components: [Health(30), Chase(speed: 2), Flash()],
    children: [Body()],
  );
}

class Health extends Behaviour {
  Health(this.max) : hp = max;
  final int max;
  int hp;
  final died = Signal<void>();

  void hurt(int damage) {
    hp -= damage;
    get<Flash>()?.play();                  // another behaviour on the same object
    if (hp <= 0) died.emit(null);
  }
}

class Chase extends Behaviour {
  Chase({required this.speed});
  final double speed;

  @override
  void onUpdate() {                        // no dt: time.delta, as in Unity
    final player = world.first<Player>();
    if (player == null) return;
    position = position.moveTowards(player.position, speed * time.delta);
  }
}

class Arena extends GameObject {
  @override
  void onStart() {
    start(() async {                       // a coroutine, stopped with the arena
      for (var wave = 1; ; wave++) {
        for (var i = 0; i < wave * 3; i++) {
          final enemy = add(Enemy(spawnPoint()));
          on(enemy.get<Health>()!.died, (_) => score.value++);
          await wait(0.4);
        }
        await until(() => world.all<Enemy>().isEmpty);
        await wait(2);
      }
    });
    on(input.onSwipe, (swipe) => world.first<Player>()?.dash(swipe.direction));
  }
}

class Flash extends Behaviour {
  Future<void> play() => punchScale(0.2, 0.15);   // a tween, stopped with the object
}
```

Not in stage 4: animation, physics (and so a fixed time step), audio,
saving, an editor, scene files, gamepads, networking. Scenes stay code
(your call, 2026-10-09).

## 1. Unity's way, and Fynkel's

What a Unity developer does, and how stage 4 lets them do it in Fynkel:

| In Unity | In Fynkel after stage 4 |
|---|---|
| A GameObject has components; MonoBehaviours add behaviour, any number, added and removed at any time. | The same: `addComponent(Health(100))`, `get<Health>()`, `removeComponent(health)`. A subclass of `GameObject` is still the prefab. |
| `GetComponent<Light>().intensity = 2` changes the light. | The engine's components (Camera, Light, MeshRenderer) are components like any other, reached the same way. They become objects you change in place (§5). |
| Awake, OnEnable, Start, Update, LateUpdate, OnDisable, OnDestroy. | onLoad (async, where an object gets its assets), onEnable, onStart, onUpdate, onLateUpdate, onDisable, onDestroy. FixedUpdate comes with physics. |
| `Time.deltaTime`, `Time.timeScale`, `Time.time`, `Time.unscaledDeltaTime`, `Time.frameCount`. | `time.delta`, `time.scale`, `time.sinceStart`, `time.unscaledDelta`, `time.frame`, on every object and behaviour. |
| Coroutines: `yield return new WaitForSeconds(1)`. They can't return a value, catch errors, or clean up. | Routines: plain `async` code. `await wait(1)`, `await until(...)`, `await race([...])`. They return values, `try/finally` cleans up, and they stop with their object. |
| C# events and UnityEvents; forgetting to unsubscribe calls into destroyed objects. | `Signal<T>`, and `on(signal, handler)`, which ends by itself when the listener is destroyed. |
| `Input.GetMouseButtonDown(0)`, `Input.GetKey(...)`, `Input.touches`; Lean Touch for fingers and gestures. | Both: polling (`input.wasPressed(MouseButton.left)`) and events in the style of Lean Touch (`on(input.onDragStart, ...)`), with a `Finger` that carries everything about the touch. Objects also get `onPointerDown`, `onDrag` and the rest, like Unity's event interfaces. |
| Cinemachine: virtual cameras, priorities, blends, follow, orbit, shake. | Built in, the same ideas. |
| DOTween or LeanTween. | Built in: `tween` for any value, helpers such as `moveTo` and `colorTo`, eases, sequences. |
| Layers, for physics, rays and cameras. | Layers for rays, input and cameras. Tags aren't needed: `world.all<T>()` finds by type. |
| `Debug.DrawLine`, Gizmos. | `debug.line`, `debug.sphere` and the like, drawn by the engine. |
| An exception in one script is logged; the game goes on. | The same: it goes to `world.errors`, and the game goes on. |
| Scenes and prefabs are files made in the editor. | Code: a level is a class, a prefab is a class. No editor in stage 4. |

## 2. What's new, by package

```
crates/
  fynkel_world/     layers (a mask on rays and on what a camera draws); debug lines for one frame
  fynkel_render/    an orthographic camera; a line pass for debug drawing
  fynkel_ffi/       the above (ABI 12, 13, …)
packages/
  fynkel/           Component and Behaviour; onEnable, onDisable, onLateUpdate; onUpdate() without dt;
                    Time; routines; Signal and on(); errors that don't stop the game;
                    input/ (fingers, gestures, events, polling, keys, axes) with no dependency on objects;
                    motion/ (tween, eases, sequences, helpers, Mathf-style math);
                    cameras (virtual cameras, follow, orbit, look-at, blends, shake);
                    layers; debug drawing
  fynkel_flutter/   feeds the input module Flutter's raw events, each with its time
examples/
  arena/            NEW: the third game, using all of it
  hello/, garden/, coins/   moved to the new API
```

As before, the loop lives in Dart and the engine never calls into Dart.
Rust gains only layers, an orthographic camera and debug lines.
package:fynkel still has no Flutter in it, so input, routines and tweens
are all tested in `dart test`, frame by frame, with made-up events.

## 3. Design decisions

### 3.1 Components and behaviours

- **A `Component` is something on an object.** A `Behaviour` is a component with code: it gets the lifecycle (§3.2) and has `enabled`. The engine's components (Transform, Camera, Light, MeshRenderer) are components too.
- **Any number of behaviours on an object**, of different types or the same: `addComponent(b)` returns `b`; `get<T>()` gives the first of type `T` or null, `getAll<T>()` all of them, `has<T>()`, `removeComponent(b)`. `get<T>()` finds by subtype, so `get<Weapon>()` finds a `Sword`, as `GetComponent` does.
- **From a behaviour, the object is near**: `gameObject`, and the shortcuts Unity has (`transform`, `position`, `get<T>()`, `world`, `time`, `input`, `assets`, `start`, `on`). A behaviour is on one object for its life; it can't move to another.
- **Building**: `GameObject(name:, transform:, components: [...], children: [...])`, so a prefab can be written as one expression, as a Flutter widget tree is. A subclass passes the same to `super`, or adds them in its constructor or `onLoad`.
- **Finding by type includes behaviours**: `world.all<Health>()`, `world.first<Player>()`. Behaviours are kept in the same per-type lists as objects, so it doesn't walk the tree.
- **Subclassing `GameObject` stays**, for things that are one thing (a `Level`, a `Player`). A behaviour is for what's shared (`Health`, `Spins`). The mixins of stage 3 still work, but behaviours replace them in the examples.
- **A behaviour added to a live object** runs its `onLoad`; the object doesn't hide again for it (it's already there). One added before its object is added loads with it, as children do.
- *Settled in step 1:*
  - **`addComponent` and `removeComponent`, not `addBehaviour`.** Once the engine's components became components like the rest (§5, 2), one name had to cover both: `addComponent(Light.sun())` and `addComponent(Health(100))`. `add` still adds children, `get<T>`, `getAll<T>` and `has<T>` stay, and the constructor takes `components:` and `children:`.
  - **`Transform` isn't a component.** Every object has one, as in Unity, so it stays the value `transform` reads and sets (stage 3's step 3b), and `remove<Transform>()` is gone.
  - **The engine's components are changed in place**, and each change goes to the engine at once, checked there: `get<Camera>()!.fovY = 500` throws the engine's error and leaves the old value. The old `camera`, `light` and `meshRenderer` getters and setters are gone, and a missing component is `null`, not an error.
  - **`get<T>` takes any type**, so interfaces work (`get<Damageable>()`), and so do `world.all<T>` and `world.first<T>`, which now find the components the game put on as well as objects.
  - **A model's parts** get their engine components read once, the first time they're asked for, so the same instances come back. A behaviour put on one puts that part in the update order.
  - **Taps reach behaviours** that are loaded and enabled, right after their object's own `onTap` and before the parent's, as objects get taps before their first start.
  - **A behaviour's load** counts toward its object's while the object is loading, and a failure fails the object's load. Once the object has appeared, a failure removes only the behaviour and goes to `world.errors`.
  - **A removed component can't be put on again**, and a removed behaviour gets no more calls, even later in the frame it was removed in.
  - **`getInParent<T>` and `getInChildren<T>`** (Unity's `GetComponentInParent` and `GetComponentInChildren`) look at the object first, then up or down.
  - `onUpdate` still takes `dt`; step 2 removes it.

### 3.2 The lifecycle and its order

For each object and each of its behaviours, in the order they were added:

1. **onLoad** (async, once): get assets, add children. The object appears when everything in it has loaded (stage 3).
2. **onEnable**: each time it becomes active in the hierarchy (when it appears, when it or a parent is turned back on, when a behaviour's `enabled` becomes true).
3. **onStart** (once): at the start of the first frame it's active, before its first update.
4. Each frame: **onUpdate**, then **onLateUpdate** after every object's onUpdate (and after the routines and tweens of §3.4 and §3.7), so a camera follows where things ended up.
5. **onDisable**: each time it stops being active (turned off, a parent turned off, `enabled = false`, or before onDestroy).
6. **onDestroy** (once).

A frame, in `world.update`: **input** (events and the object handlers of §3.6) → **starts** → **updates** → **routines** (each one waiting for this frame goes on, §3.4) → **tweens** → **late updates** → the view copies the world. Fixed, documented, and tested, as stage 3's order is.

### 3.3 Time

- **`onUpdate()` takes no `dt`**: `time.delta` is the frame's time, as in Unity. Fewer parameters to pass along, and a helper anywhere in an object's code reads the same clock.
- **`time` belongs to the world** and is on every object and behaviour: `delta`, `unscaledDelta`, `scale` (0 pauses), `sinceStart`, `unscaledSinceStart`, `frame`, `maxDelta` (a long hitch, or a return from the background, counts as at most this, so nothing jumps through walls; Unity's `maximumDeltaTime`).
- **Not a global `Time` class.** Fynkel can run more than one world at once (two views), and code after an `await` may run while another world's frame is set. `time` on the object is always its own world's. It reads the same as Unity's.
- **Unscaled things** (a pause menu's animation, a UI tween) say so: `wait(1, unscaled: true)`, `tween(..., unscaled: true)`.

### 3.4 Routines (coroutines)

- **`start(() async { ... })`** on any object or behaviour runs a routine and returns a `Routine`: `stop()`, `isRunning`, and `done`, a future of its result.
- **Waiting is awaiting**, and everything waits in game time:
  - `await nextFrame`, `await wait(seconds)`, `await until(() => test)`, `await whileTrue(() => test)`;
  - `await race([a, b])` (the first to finish; the rest are stopped), `await all([a, b])`, `await x.timeout(seconds)`;
  - any `Future` (an asset load, a tween, a signal's `next`, another routine's `done`).
- **Better than Unity's, without being harder:**
  - routines return values: `final choice = await askPlayer();`;
  - errors are caught with `try`, and reported to `world.errors` if not;
  - `try/finally` runs when a routine is stopped, so it can clean up;
  - routines started inside a routine stop with it;
  - a routine stops by itself when its object or behaviour is destroyed, and pauses while it's inactive (unlike Unity, which stops it for good);
  - `time.scale` slows its waits; `unscaled: true` doesn't.
- **When the code after an `await` runs**: in the frame's routines step (§3.2), in a fixed order, not at some later point of Dart's event loop. Each routine runs in a zone that takes Dart's continuations and runs them at that step. A test fixes that a routine's code never runs after its object is destroyed, even after an outside future it was waiting for completes.

### 3.5 Signals, references and singletons

- **`Signal<T>`**: `emit(value)`, `listen(handler)` (returns a subscription to cancel), `await signal.next`. Plain Dart, usable anywhere.
- **`on(signal, handler)`** on objects and behaviours: the subscription ends by itself when the object or behaviour is destroyed. The usual way to listen, so destroyed objects never hear anything.
- **References stay plain Dart fields**, checked by the compiler. A field that points at a destroyed object says so: `isDestroyed`, and anything that needs the engine throws a clear `StateError` ("Enemy#12 was destroyed"), instead of Unity's "fake null".
- **Singletons**: `world.first<Score>()`, or `world.only<Score>()` which throws if there isn't exactly one. No `static instance`, so two worlds and tests never share one.
- **Showing state in Flutter** stays `ValueNotifier` (stage 3).

### 3.6 Input

Its own module (`package:fynkel`'s `input`), which knows nothing about objects: it turns raw events into fingers, gestures, events and polled state. The world then adds what needs the scene (which object is under a finger). It's tested in `dart test` with made-up events and times.

- **A `Finger`** (Lean Touch's name) is one touch, mouse or pen, from down to up:
  - `id`, `kind` (touch, mouse, pen), `button` (left, right, middle), `pressure`;
  - `position`, `startPosition`, `delta` (since last frame), `totalDelta`, `velocity`;
  - `age` (seconds since down), `tapCount` (2 for a double tap), `isDown`, `isTap` (still within the tap's slop and time);
  - `startedOver` and `over`: the object and world point under it at the start and now (cast once, when asked).
  - The mouse is a finger too, even with no button down (for hover).
- **Events** to listen to (Signals, so `on(input.onTap, ...)`):
  - `onFingerDown`, `onFingerUp`, `onFingerHeld`, `onFingerMove`, `onHover`;
  - `onTap` (with `tapCount`), `onLongPress`;
  - `onDragStart`, `onDrag`, `onDragEnd` (with the finger, so start and current positions and speed);
  - `onSwipe` (direction, speed, distance);
  - `onPinch` and `onTwist` (two or more fingers: scale, angle, centre, also from a trackpad), `onScroll`;
  - `onKeyDown`, `onKeyUp`, with modifiers.
- **On objects**, as Unity's `IPointerDownHandler` and the rest: `onPointerDown`, `onPointerUp`, `onPointerEnter`, `onPointerExit`, `onClick` (stage 3's `onTap`), `onLongPress`, `onDragStart`, `onDrag`, `onDragEnd`, on objects and behaviours. They bubble up to the parents until one returns true, as taps do now. An object that takes a drag's start gets the rest of that drag, even when the finger leaves it.
- **Polling**, as Unity's `Input` class: `input.isDown(x)`, `input.wasPressed(x)`, `input.wasReleased(x)` for keys and mouse buttons; `input.mousePosition`, `input.fingers`, `input.scroll`.
- **Keys** are Fynkel's own `Key` values (`Key.space`, `Key.a`), filled from Flutter's by fynkel_flutter, so package:fynkel needs no Flutter.
- **Axes and actions**: `input.axis2('move')` reads WASD and the arrow keys as a `Vec2` by default, and `input.action('jump')` gives a named button bound to keys and mouse buttons, with `isDown`, `wasPressed`, `wasReleased`. Gamepads join later, through the same names.
- **Settings**: the tap's slop and time, the double tap's interval, the long press's time, the swipe's speed, in `input.settings`, with sensible defaults.
- **Flutter UI on top**: pointers taken by a widget over the view (a HUD button) never reach the game, which is what a game wants. A widget can let them through, as Flutter already allows.
- **Allocation**: no allocation in a frame without input, and a fixed small cost per event.

### 3.7 Motion

Built into package:fynkel, its own module: one small core and short helpers on top.

- **The core**: `tween(from, to, seconds, ease:, onUpdate:)` for any value with a lerp. `double`, `Vec2`, `Vec3`, `Quat` (by slerp) and `Color` are built in; another type passes its own `lerp`. A new tween is one line.
- **Eases**: the usual set (`Ease.linear`, `inQuad` … `inOutBack`, `outBounce`, `outElastic`), and any `double Function(double)`, so Flutter's `Curves.x.transform` works too.
- **Control**: `delay`, `loops` (with `yoyo`), `speed` instead of seconds, `unscaled`, `onComplete`; `sequence([...])` and `together([...])`; `stop()`, `complete()`.
- **Every tween is a future and an owned thing**: `await transform.moveTo(...)` in a routine, and it stops with the object or behaviour that made it, as routines do.
- **Helpers**: `moveTo`, `moveBy`, `rotateTo`, `scaleTo`, `lookAtOver`, `colorTo`, `fadeTo`, `punchScale`, `shake`, on objects and materials.
- **Moving things by hand**: `position`, `rotation`, `scale` and `worldPosition` read and set directly on objects and behaviours (still one `Transform` value underneath); `forward`, `right`, `up`; `lookAt(point)`; `transformPoint` and `inverseTransformPoint`. And Unity's Mathf: `moveTowards`, `smoothDamp`, `lerp`, `inverseLerp`, `clamp`, `repeat`, `pingPong`, `randomInSphere`, `randomOnCircle`.
- **Names that don't clash with Flutter's** (`Tween`, `Curve`, `Animation`), since games import both: `tween()`, `Ease`, and `Motion` for a running tween (§5).

### 3.8 Cameras

As Cinemachine: the view has one real camera, and **virtual cameras** decide where it is.

- **A `VirtualCamera`** behaviour has a `priority`; the highest one that's active drives the view's camera. Changing priority, or turning one on or off, **blends** to the new one over a time with an ease (per pair, or a default), or cuts.
- **Rigs** (behaviours on the virtual camera's object):
  - `Follow(target, offset:, damping:, lookAhead:)`;
  - `Orbit(target, distance:, …)`, driven by drag, pinch and scroll from §3.6 by default, with limits;
  - `LookAt(target, damping:)`;
  - none, for a fixed camera.
- **Shake**: `camera.shake(strength, seconds)`, added on top, unscaled if asked.
- **Lens**: field of view, near and far, and **orthographic** with a size (an engine change).
- **Between screen and world**: `screenToRay(point)`, `screenToPlane(point, plane)`, `worldToScreen(point)`.
- All of this is Dart over the engine's one camera; only the orthographic projection is new in Rust.

### 3.9 Layers, debug drawing, errors, hot reload

- **Layers**: each object has one of 32 (`layer`), and rays, input and cameras take a mask. The engine keeps the layer, so rays and drawing skip what they don't want without asking Dart.
- **Debug drawing**: `debug.line(a, b, color)`, `debug.ray`, `debug.box`, `debug.sphere`, `debug.path`, for one frame or for some seconds, drawn by the engine on top of the scene. Off in release builds unless asked.
- **Errors don't stop the game**: an exception from any lifecycle call, routine, handler or tween is reported to `world.errors` with its object, and the rest of the frame goes on. If nothing listens, it's an uncaught error, so tests fail (as load errors already do).
- **Hot reload**: check what Flutter's hot reload does to a running game (changed methods take effect at once; changed fields don't apply to live objects) and write it down, with a test game.

## 4. Steps

Each step is built, tested, and merged on its own, in this order (each
builds on the ones before). Each step's tests fail first where that makes
sense. hello, the garden and coins keep working at every step; they move
fully to the new API in step 9.

**Step 1: components and behaviours.** §3.1: `Component`, `Behaviour`,
`addComponent`, `get`, `getAll`, `has`, `removeComponent`, `components:` and
`children:` in the constructor, finding behaviours by type. The engine's
components become objects you change in place (§5). *Done when* tests show behaviours get the same
lifecycle as objects, in the order added, `get<T>` finds by subtype,
`world.all<T>` finds behaviours and forgets destroyed ones, and the step
3b bench (10,000 objects, now each with one moving behaviour) stays
within 10 % of stage 3's on all three devices.
✅ on the Mac and the iPhone; the Note10+ is still to measure. 17 tests in
`test/component_test.dart`, and every earlier test moved to the new API
(121 in package:fynkel, 25 in package:fynkel_flutter, the examples' 8);
17 mutants, all killed. A behaviour moving its object costs what an object
moving itself does (§6).

**Step 2: the lifecycle and time.** §3.2, §3.3: onEnable, onDisable,
onLateUpdate, `onUpdate()` without `dt`, `time`, errors that don't stop
the game. *Done when* tests fix the whole order of §3.2 (including turning
parents and behaviours on and off, and destroying in each step), `time`
follows `scale` and `maxDelta`, and an exception in one object's update
reaches `world.errors` while the others update.

**Step 3: routines.** §3.4. *Done when* tests show waits in game time
(scaled and not), `race` and `all`, return values, `finally` on stop,
routines stopping with their object and pausing while it's inactive,
nested routines, and that no routine code runs after its object is
destroyed, even when an outside future completes later. 1,000 waiting
routines cost less than 0.5 ms a frame on the Note10+.

**Step 4: signals.** §3.5: `Signal`, `on`, `next`, `world.only`, clear
errors from destroyed objects. *Done when* a destroyed listener is never
called and holds nothing (a test with the garbage collector), and
`await signal.next` works in routines.

**Step 5: input.** §3.6, with fynkel_flutter's raw events. *Done when*
tests drive each event and gesture from made-up events (taps and double
taps, long press, drags and their capture, swipes, pinch and twist, the
mouse's buttons and hover, keys, axes and actions, the order against
the rest of the frame), objects' handlers bubble, a frame without input
allocates nothing, and you have tried it on the Mac, the iPhone and the
Note10+ with a small test scene.

**Step 6: motion.** §3.7. *Done when* tests show each ease's ends and
shape, sequences, loops and yoyo, unscaled tweens, stopping with the
owner, awaiting in routines, and the transform helpers against hand-worked
values; 1,000 running tweens cost less than 1 ms a frame on the Note10+.

**Step 7: cameras.** §3.8, and the orthographic camera in the engine.
*Done when* tests show priorities, blends and cuts, each rig against
hand-worked values, shake's ends, and screen and world conversions both
ways (a golden for the orthographic camera), and the garden's and coins'
cameras are rigs.

**Step 8: layers and debug drawing.** §3.9, in the engine and in Dart.
*Done when* goldens show a layer hidden from a camera and debug lines
drawn, rays and input skip masked layers, and a frame with no debug
drawing costs what it did.

**Step 9: the arena, and the others moved over.** `examples/arena`: a
small top-down game. You move with WASD or by dragging, aim and dash by
swiping, and enemies come in waves. It has behaviours shared by the
player and enemies (`Health`, `Flash`), routines for the waves, signals
for the score and deaths, tweens for hits and pickups, a follow camera
that shakes and zooms with a pinch, a pause that sets `time.scale` to 0,
layers so taps skip effects, and a Flutter HUD. hello, the garden and
coins move to behaviours, `time` and rigs; hot reload is checked and
written down (§3.9). *Done when* all four run at the display rate on all
three devices, the lifecycle memory checks pass against `--baseline` as
in stage 3, and you have played the arena on each.

**Step 10: the stage 4 check.** A short report, as in stages 1 to 3, and a
guide, `docs/writing-a-game.md`, that teaches all of this from hello up.
*Done when* both are committed and you have read the guide.

## 5. Decisions (2026-10-09)

You took the first answer, the recommended one, in each.

1. **Names** (changed in step 1 to `addComponent` / `removeComponent`, §3.1): `addBehaviour` / `get<T>` / `getAll<T>` / `remove`, since `add`
   already adds children; or Unity's `addComponent` / `getComponent`.
2. **The engine's components**: they become objects you change in place
   (`get<Light>()!.intensity = 2`, as in Unity, each change written to the
   engine at once); or they stay values you replace (`light = Light.sun(...)`,
   as now).
3. **`time` on each object**, the world's; or a global `Time` class as in
   Unity, which would be wrong with two worlds and after an `await` (§3.3).
4. **Motion names**: `tween()`, `Ease`, and a `Motion` type for a running
   tween, so nothing clashes with Flutter's `Tween` and `Curve`; or
   `Tween` and `Curve`, which a game would have to hide from Flutter's
   import.
5. **One stage**, with input and cameras in it; or two, with input and
   cameras as stage 5.
6. **The third game**: the arena of step 9; or another you'd rather have.

## 6. Measurements

### Step 1: behaviours (`measure bench`, profile builds, 2026-10-09)

`world.update` with 10,000 objects in 100 trees of 100, median (slowest)
of 300 steady frames, each time with the frame's moves sent; main
measured the same day, the same way:

| 10,000 objects | Mac (M3 Max) | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---:|---:|---:|
| moving themselves (`rotate`), main | 0.81 (1.42) ms | 0.76 (2.48) ms | – |
| moving themselves, step 1 | 0.85 (8.38) ms | 0.77 (2.53) ms | – |
| **each moved by a behaviour**, step 1 | **0.85** (1.46) ms | **0.79** (1.16) ms | – |
| with nothing to do, main / step 1 | 0.02 / 0.03 ms | 0.02 / 0.03 ms | – |

A behaviour costs its object 0.04 ms more per 10,000 on the Mac and 0.03
on the iPhone (5 % and 4 %), the call and the walk over the object's
components. Objects without components pay nothing measurable.
