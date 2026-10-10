# Stage 4 plan: a solid base for writing games

Status: accepted 2026-10-09, with the recommended answer to each question in §5; steps 1–9
done, step 10 waits for you to read the guide ([report](stage-4-report.md)). Scope and
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
- **Subclassing `GameObject` stays**, for what an object is (a `Level`, a `Player`). A behaviour is for what it does that others could do too (`Health`, `Spins`); see *Settled after step 9*. The mixins of stage 3 still work, but behaviours replace them in the examples.
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
- *Settled in step 2 (§3.2 and §3.3):*
  - **onEnable comes as soon as an object appears**, in the same `world.add` when nothing loads, with everything below it, parents first, each object before its behaviours. An object still loading gets it when its whole tree appears, never part of it. onStart still waits for the next frame.
  - **What changes "shown"** calls onEnable or onDisable at once, in the setter: `active`, `parent` (moving below a hidden object, or out from below it), a behaviour's `enabled`, its load finishing on a shown object, `removeComponent`, and `destroy`. A behaviour still loading isn't enabled by its object being turned on; it's enabled when its load is done.
  - **Destroying** gives each object, children first, onDisable if it was enabled, then its enabled behaviours' onDisable, then onDestroy, then its behaviours' onDestroy. An object that destroys itself in any call (onEnable, onStart, onUpdate, onLateUpdate, onDisable) gets nothing more, nor do its behaviours and children, and the frame goes on for the others.
  - **onLateUpdate** goes to the objects updated this frame, after all updates; routines and tweens will come between them in steps 3 and 6.
  - **`world.update(dt)` still takes the frame's real time** (the view passes it); `dt` must be 0 or more. `time.delta` is it clamped to `maxDelta` (1/3 s by default, as Unity's), times `scale`; a 5 s hitch counts as 1/3 s. `unscaledDelta` is clamped too, not scaled. `frame` counts updates from 0.
  - **Errors in any lifecycle call or `onTap`** go to `world.errors` with the object (a behaviour's go with its object), and the frame goes on; a throwing `onTap` counts as not taking the tap. Calls each frame are guarded inline, so a frame still makes no closures.

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

- *Settled in step 3:*
  - **`start` is on objects and behaviours**, and the `Routine` it returns is itself a future: `final choice = await start(askPlayer);`, `race([wait(3), start(fight)])`. `done` gives the same future; `stop()`, `isRunning` and `isStopped` are on it.
  - **The waits are top-level functions**, `nextFrame`, `wait`, `until`, `whileTrue`, so a helper function called from a routine can wait too. They throw a `StateError` outside a routine. `race` and `all` work anywhere.
  - **Every wait ends in a later frame**: one made in frame F (in an update, or in the routines step itself) ends in F + 1 at the earliest, and `wait(s)` counts from there; `wait(0)` is the next frame.
  - **Code after one of these waits runs in the routines step**, in the order the waits were made, with whatever it schedules (an `await` on a finished future, `await null`) before the next routine goes on. **Code after any other future** (an asset load, a Completer) runs when that future completes, as anywhere in Dart, not deferred to the routines step: a zone can't hold back a continuation without breaking what `.then` returns. Its next wait puts it back in step.
  - **Nothing of a stopped routine runs again**, even when an outside future it awaited completes later: its zone runs none of its callbacks but those that unwind it. Waiting on one of the world's waits or on another routine, it unwinds through its `finally` blocks at once; waiting on another future, it's dropped without them.
  - **It stops** when it's stopped, when the routine it was started in is stopped, when its behaviour is removed, or when its object is destroyed (after the object's onDisable, before its onDestroy, so its `finally` can still read it). A routine that ends normally lets those started in it go on, as Unity's do.
  - **It pauses** while its object is hidden, or its behaviour isn't enabled: its waits don't count down or end, and it goes on where it was. (Unity stops them for good.)
  - **`RoutineStopped`** is what a stopped routine throws at its wait and fails with when awaited. Catching it doesn't keep the routine going: its next wait throws it at once.
  - **Errors** a routine doesn't catch go to `world.errors`, unless something awaits it (another routine, `race`, `all`), which gets the error instead.
  - **`race` stops the losing routines and drops the losing waits** given to it as they are; a derived future such as `wait(3).then(...)` can't be stopped, and just ends into nothing.
  - **No game-time `x.timeout(seconds)`**: `Future.timeout` is Dart's own, in real time, and a routine is a `Future`. `race([x, wait(seconds)])` does it.

### 3.5 Signals, references and singletons

- **`Signal<T>`**: `emit(value)`, `listen(handler)` (returns a subscription to cancel), `await signal.next`. Plain Dart, usable anywhere.
- **`on(signal, handler)`** on objects and behaviours: the subscription ends by itself when the object or behaviour is destroyed. The usual way to listen, so destroyed objects never hear anything.
- **References stay plain Dart fields**, checked by the compiler. A field that points at a destroyed object says so: `isDestroyed`, and anything that needs the engine throws a clear `StateError` ("Enemy#12 was destroyed"), instead of Unity's "fake null".
- **Singletons**: `world.first<Score>()`, or `world.only<Score>()` which throws if there isn't exactly one. No `static instance`, so two worlds and tests never share one.
- **Showing state in Flutter** stays `ValueNotifier` (stage 3).

- *Settled in step 4:*
  - **`emit` calls every listener at once**, in the order they started listening. One added while it emits hears the next value; one cancelled before its turn doesn't hear this one. Emitting from a handler is fine.
  - **Errors don't stop the others.** A handler added with `on` that throws goes to `world.errors` with its object, as an update's would. One added with `listen` doesn't stop the rest either; once all have heard, `emit` throws the first such error.
  - **`on` stops listening with the routines**: when the object is destroyed (after its `onDisable`, before its `onDestroy`), when the behaviour is removed, and when the world is disposed. A hidden object still hears, as in Unity.
  - **A listener holds its subscriptions weakly**; the signal holds them. So a long-lived listener (the arena's `on(enemy.died, ...)`) doesn't keep the signals of dead enemies, and a destroyed listener is held by nothing. Tests check both with the garbage collector.
  - **`next` in a routine is one of its waits**: the code after it runs in the routines step, in the frame after it started waiting at the earliest; it holds while the routine is paused; stopping the routine unwinds it through its `finally`; and `race([died.next, wait(5)])` drops it, with its subscription, when the timeout wins. Outside a routine it's a plain future.
  - **`world.only<T>()`** counts objects and components of `T` and its subclasses, as `all` does.
  - **A destroyed object** can still be read in its own `onDisable` and `onDestroy`. After that, everything that needs the engine throws a `StateError` saying what: "Enemy#12 was destroyed, so its transform can't be read". A removed component, or one whose object was destroyed, refuses changes the same way, instead of doing nothing.
  - **Fixed along the way:** a destroyed object stayed in the list of transforms to send until a view drew a frame, so a world without a view kept every destroyed object that had moved. It's now dropped at the next update.

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
- **Keys** are Fynkel's own `KeyCode` values (`KeyCode.space`, `KeyCode.a`), filled from Flutter's by fynkel_flutter, so package:fynkel needs no Flutter.
- **Axes and actions**: `input.axis2('move')` reads WASD and the arrow keys as a `Vec2` by default, and `input.action('jump')` gives a named button bound to keys and mouse buttons, with `isDown`, `wasPressed`, `wasReleased`. Gamepads join later, through the same names.
- **Settings**: the tap's slop and time, the double tap's interval, the long press's time, the swipe's speed, in `input.settings`, with sensible defaults.
- **Flutter UI on top**: pointers taken by a widget over the view (a HUD button) never reach the game, which is what a game wants. A widget can let them through, as Flutter already allows.
- **Allocation**: no allocation in a frame without input, and a fixed small cost per event.

- *Settled in step 5:*
  - **`KeyCode`, not `Key`**: Flutter's widgets have a `Key`, and a game's file imports both. Keys are where they are, not what they type (Flutter's physical keys), so WASD stays WASD on an AZERTY keyboard, as Unity's `KeyCode` does.
  - **Events wait for the next frame.** The view feeds them as they come; the world handles them in order at the start of its update, before starts, updates and routines, and each takes that frame's time. So a finger's `age` and the long press count in game time steps, unscaled: a frame counts for at most a third of a second, as everywhere.
  - **The mouse is one finger** for as long as it's over the view, pressed or not, so it hovers, enters and exits. A touch is a finger from down to up, and enters what it lands on just before its down. Every mouse button taps; `finger.button` says which. A touch holds `MouseButton.left` for polling, so `wasPressed(MouseButton.left)` works on phones too.
  - **A double tap is two taps**: `onTap` fires with `tapCount` 1, then 2. Waiting to see if a second comes would make every single tap late.
  - **A swipe is a drag that ends fast**: at least `swipeSpeed` (800 px/s) over the last tenth of a second when the finger lifts. A slow drag that ends still is only a drag. A finger that moved past the slop never long presses.
  - **On objects:** `onPointerUp` goes only to what took the down, wherever the finger is, as in Unity; `onDrag` and `onDragEnd` go to what took the drag's start. Exits run from the part up, enters from the top down. An object's enabled behaviours come right after it, before its parent.
  - **Errors** from a `listen` handler or an object's handler go to `world.errors`, and the rest still hear; standalone `Input` (for tests) rethrows.
  - **No allocation, tested**: `test/alloc_test.dart` runs 50 objects with behaviours, a resting mouse and listeners for 50,000 frames and compares the VM's allocation profile with a run of none. Making it pass also took the allocations out of the world's own idle frame (iterators in the transform tidy-up, the starts list).

### 3.7 Motion

Built into package:fynkel, its own module: one small core and short helpers on top.

- **The core**: `tween(from, to, seconds, ease:, onUpdate:)` for any value with a lerp. `double`, `Vec2`, `Vec3`, `Quat` (by slerp) and `Color` are built in; another type passes its own `lerp`. A new tween is one line.
- **Eases**: the usual set (`Ease.linear`, `inQuad` … `inOutBack`, `outBounce`, `outElastic`), and any `double Function(double)`, so Flutter's `Curves.x.transform` works too.
- **Control**: `delay`, `loops` (with `yoyo`), `speed` instead of seconds, `unscaled`, `onComplete`; `sequence([...])` and `together([...])`; `stop()`, `complete()`.
- **Every tween is a future and an owned thing**: `await transform.moveTo(...)` in a routine, and it stops with the object or behaviour that made it, as routines do.
- **Helpers**: `moveTo`, `moveBy`, `rotateTo`, `scaleTo`, `lookAtOver`, `colorTo`, `fadeTo`, `punchScale`, `shake`, on objects and materials.
- **Moving things by hand**: `position`, `rotation`, `scale` and `worldPosition` read and set directly on objects and behaviours (still one `Transform` value underneath); `forward`, `right`, `up`; `lookAt(point)`; `transformPoint` and `inverseTransformPoint`. And Unity's Mathf: `moveTowards`, `smoothDamp`, `lerp`, `inverseLerp`, `clamp`, `repeat`, `pingPong`, `randomInSphere`, `randomOnCircle`.
- **Names that don't clash with Flutter's** (`Tween`, `Curve`, `Animation`), since games import both: `tween()`, `Ease`, and `Motion` for a running tween (§5).

- *Settled in step 6:*
  - **`seconds:` or `speed:`, named**: every tween and helper takes one of the two (`moveTo(p, seconds: 0.5)`, `moveTo(p, speed: 4)`), rather than seconds in its place, as Dart can't have both. Speed works for numbers, `Vec2`, `Vec3` (units a second) and `Quat` (radians a second).
  - **Where they run**: in the routines step, before the waits end, in the order they were made; a motion made in a frame moves from the next, as a `wait` counts. So late updates (a camera) see where the tweens put things.
  - **A motion is a future that never fails**: it completes when it ends, however it ends (done, stopped, with its owner); `isCompleted` and `isStopped` say which. Awaited in a routine it's one of the routine's waits, so the code after it runs in the routines step and stopping the routine unwinds it; `race` stops a motion that loses.
  - **Helpers read where they start when they begin** (after their delay), so `sequence([moveTo(a), moveTo(b)])` goes to a, then from a to b. A group takes motions made in the same frame, none looping for ever. A group that loops without a yoyo begins its parts again from where they are (a patrol); one with a yoyo plays them back, the last first. A tween that loops starts over from the same value.
  - **The default ease is `Ease.outQuad`**, DOTween's.
  - **`position`, `rotation`, `scale` are the local ones**, as `transform`'s; `worldPosition` and `forward`, `right`, `up` are the world's, as in Unity. A game's own field with one of these names clashes (hello's `Sphere` had `var up = true`, now `goingUp`); the helpers never read them, so an override can't change what they move.
  - **`rotateBy(axis, angle)`** joined the helpers: a spinning coin needs more than half a turn, which `rotateTo` can't say. **`fadeTo` waits**: the engine draws nothing see-through yet. `colorTo` colours the renderer's material, or the one given.
  - **Mathf**: `lerp`, `inverseLerp`, `clamp01` (Dart's `clamp` does the rest), `moveTowards`, `repeat`, `pingPong`, `randomInSphere`, `randomOnCircle`; `Vec2`/`Vec3.lerp` and `moveTowards`, `Quat.slerp`, `Color.lerp`. `SmoothDamp` and `SmoothDamp3` are small classes holding their velocity, as Dart has no `ref`.

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

- *Settled in step 7:*
  - **The world is the brain**: `world.cameras` holds the virtual cameras, the blends and the shakes; there is no brain component to add. `world.camera` is the object the view draws from. With no virtual camera the camera is the game's, left where it is.
  - **A sixth step of the frame**, after the late updates: the rigs move their objects (`Follow` and `Orbit`, then `LookAt`, so a LookAt aims from where a Follow put it this frame), then the live virtual camera's pose and lens go to the camera, then the shakes. A frame without cameras allocates nothing.
  - **The live one** is the enabled one with the highest priority; of equals, the one enabled or given its priority last (Cinemachine's). The first one cuts. `setBlend(blend, from:, to:)` picks the most exact pair (both, then to, then from), else `defaultBlend` (1 s, `Ease.inOutQuad`); `Blend.cut`. A blend cut short blends on from where it got to. `unscaled` keeps blends going while paused; `onLiveChanged` is a signal.
  - **Damping is Cinemachine's**: 99 % of the way in `damping` seconds, whatever the frame rate.
  - **Shake is `world.cameras.shake(strength, seconds:)`**, not on an object: it shakes the view's camera on top of whatever moves it (a virtual camera, a rig, a tween, the game), across the view, fading as (1 − p)², and ends exactly where the camera would be. `stopShaking`, `isShaking`, `seed`. Step 6's `GameObject.shake` stays, for shaking anything.
  - **`size` is half the view's height**, Unity's `orthographicSize`; `Camera.orthographic` and `size` are engine fields (ABI 12).
  - **`rayThrough` is now `screenToRay`**, beside `screenToPlane(point, plane)` and `worldToScreen(point)` (the view's size by default, null behind a perspective camera); `Plane(normal, point)`, `Plane.ground`.
  - **The lab's own `Follow` became `Trail`**, as package:fynkel now has a `Follow`.

### 3.9 Layers, debug drawing, errors, hot reload

- **Layers**: each object has one of 32 (`layer`), and rays, input and cameras take a mask. The engine keeps the layer, so rays and drawing skip what they don't want without asking Dart.
- **Debug drawing**: `debug.line(a, b, color)`, `debug.ray`, `debug.box`, `debug.sphere`, `debug.path`, for one frame or for some seconds, drawn by the engine on top of the scene. Off in release builds unless asked.
- **Errors don't stop the game**: an exception from any lifecycle call, routine, handler or tween is reported to `world.errors` with its object, and the rest of the frame goes on. If nothing listens, it's an uncaught error, so tests fail (as load errors already do).
- **Hot reload**: check what Flutter's hot reload does to a running game (changed methods take effect at once; changed fields don't apply to live objects) and write it down, with a test game.

- *Settled in step 8:*
  - **A layer is an int, 0 to 31**, on the object (`GameObject.layer`, or `GameObject(layer: 3)`), 0 by default; a child keeps its own, as in Unity. Name layers with constants (`const enemies = 8`); there are no layer names in the engine. Out of range is a `RangeError`.
  - **`LayerMask`** is a value: `LayerMask.all`, `none`, `of([1, 3])`, `except([2])`, `plus`, `minus`, `|`, `&`, `contains`. No layer is special: Unity's layer 2, "Ignore Raycast", has no counterpart, so a default ray hits every layer.
  - **The masks**: `Camera.cullingMask` (an engine field, ABI 13) leaves objects undrawn, and lights still light every layer; `world.raycast(ray, layers:)` goes through what's left out; a finger touches only what the camera draws and `world.input.layers` has (Unity's `eventMask`).
  - **`world.debug`** (`debug` in an object or a behaviour): `line`, `ray` (as long as its direction, as Unity's `DrawRay`), `box` (12 edges, turned by `rotation`), `sphere` (three circles), `path` (`closed:`), each with `color` and `seconds`, and `clear`. A line lasts a frame, or `seconds` of unscaled time, so they pass while paused. `enabled` is off in a release build unless turned on, and turning it off takes the lines away.
  - **The seventh step of the frame**: the lines go to the engine in one call at the end of `update`; a frame with none after a frame with none makes no call. The engine draws them after the scene, through it (no depth test), one pixel wide, blended by their alpha.
  - **Errors**: every call that runs game code already reported to `world.errors` (steps 2 to 7); nothing new was needed.
- *Settled in step 9:*
  - **Hot reload** (tried on macOS with `examples/arena/lib/reload_probe.dart`): a changed method, in an object or a behaviour, runs from the next frame. A changed field initializer doesn't apply to objects that exist; a field added to a class gets its initializer on existing objects when first read. `onLoad` and `onStart` don't run again, so what they made stays as it was. A routine that's running goes on in its old code: its loop keeps the body it started with until it ends. Hot restart makes the game again from `main`, the engine with it; four in a row, no crash and nothing left over.
  - **Resources live while something reaches them** (rough edge 2 of the stage review): a world holds its meshes, materials and textures weakly, and lets one go at the end of the update after it's garbage-collected (materials first). A model's copy keeps its model. `dispose` still frees sooner. `own()` is gone. Freeing an asset texture first takes out the materials that use it and that no object draws with, so a destroyed level's textures go at once; one a drawn material still uses goes with that material.
  - **Moving by hand** (rough edge 1): `position`, `rotation`, `scale` and `worldPosition` already had setters (step 6); `rotate` and `translate` now work in behaviours too. The examples use them instead of `transform.copyWith`.
- *Settled after step 9, from the review of what writing the arena showed:*
  - **A subclass is what an object is; a behaviour is what it does.** Both stay. A `Player`, an `Enemy` or a `Level` is a subclass of `GameObject`: its state, its parts, a constructor, and typed results from `find`, `children`, `ancestor` and `raycast`. Code another kind of object could use too (`Health`, `Spin`, `Flash`), or that comes and goes while the game runs, is a behaviour. The test: if a subclass's `onUpdate` would make sense on another kind of object, it belongs in a behaviour. Behaviours only (Unity's way) would give up the typed objects; subclasses only (Flame's) would give up adding and removing code at runtime. The rule is in the `GameObject` and `Behaviour` docs; every class in the examples already follows it.
  - **A destroyed object's components are still found**: `get`, `getAll` and `has` give what it had, to read (they can't be changed), so `Health get health => get<Health>()!` works on a dead enemy. One taken off with `removeComponent` is gone. Behaviours (all components) have `ancestor`, `getInParent` and `getInChildren`, as objects do.
  - **Starting over after a hot reload**: `FynkelGame(make: ArenaGame.new, builder: ...)` makes the root and builds what shows it; `restart()` builds it all again around a new root, in a new world, with the code as it is now. F5 restarts in debug builds; `restartOnReload: true` makes every hot reload restart, for phones. `FynkelApp` takes a constructor and an `overlay` (`FynkelApp(ArenaGame.new, overlay: (context, game) => Hud(game))`).
  - **A `Transform` is a position, a rotation and a scale, nothing else**
    (after step 10, your call): its `lookAt:` and `forward:` shortcuts are
    gone. They gave three ways to say a rotation, checked only at run time,
    and `lookAt:` took a point in the parent's space while
    `GameObject.lookAt` takes one in the world's. As in Unity, rotations
    are made on `Quat`: `lookRotation`, `axisAngle`, and the new
    `Quat.euler(pitch, yaw, roll)` (Unity's order: roll, pitch, then yaw;
    radians, as everywhere). `Transform` is now `const`.
  - **world.dart is split**: `World` keeps its state and what the game calls; the work behind it is in `world_loop.dart`, `world_objects.dart` and `world_resources.dart` (private extensions), and `GameObject` is in `game_object.dart`.

## 4. Steps

Each step is built, tested, and merged on its own, in this order (each
builds on the ones before). Each step's tests fail first where that makes
sense. hello, the garden and coins keep working at every step; they move
fully to the new API in step 9.

Each step also adds its scene(s) to `examples/lab`, the feature lab, so
the feature is tried by hand on a device, not only in tests. Steps 1 to 3
got theirs after the fact (Components, Lifecycle, Time, Routines). From
step 4 on, a step's *done when* includes its lab scene, tried on the
iPhone or the simulator.

**Step 1: components and behaviours.** §3.1: `Component`, `Behaviour`,
`addComponent`, `get`, `getAll`, `has`, `removeComponent`, `components:` and
`children:` in the constructor, finding behaviours by type. The engine's
components become objects you change in place (§5). *Done when* tests show behaviours get the same
lifecycle as objects, in the order added, `get<T>` finds by subtype,
`world.all<T>` finds behaviours and forgets destroyed ones, and the step
3b bench (10,000 objects, now each with one moving behaviour) stays
within 10 % of stage 3's on all three devices.
✅ on all three devices (the Note10+ measured with step 3, §6). 17 tests in
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
✅ 22 tests in `test/lifecycle_test.dart` (146 in package:fynkel in all,
25 in package:fynkel_flutter); every `onUpdate(double dt)` in the
examples, tests and bench now reads `time.delta`. 26 mutants, all killed
(one more was equivalent, and its code removed). The late-update pass
costs 0.03–0.12 ms per 10,000 objects (§6).

**Step 3: routines.** §3.4. *Done when* tests show waits in game time
(scaled and not), `race` and `all`, return values, `finally` on stop,
routines stopping with their object and pausing while it's inactive,
nested routines, and that no routine code runs after its object is
destroyed, even when an outside future completes later. 1,000 waiting
routines cost less than 0.5 ms a frame on the Note10+.
✅ 21 tests in `test/routine_test.dart` (166 in package:fynkel in all);
21 mutants, all killed. 1,000 waiting routines cost 0.03 ms a frame on
the Note10+, and 1,000 going on every frame 0.85 ms (§6). The code after
an outside future isn't moved into the routines step (§3.4, settled).

**Step 4: signals.** §3.5: `Signal`, `on`, `next`, `world.only`, clear
errors from destroyed objects. *Done when* a destroyed listener is never
called and holds nothing (a test with the garbage collector), and
`await signal.next` works in routines.
✅ 26 tests in `test/signal_test.dart` (193 in package:fynkel in all); 10
mutants, all killed (one only after a test for two objects of the same
class). The lab's Signals scene shows each part by hand: enemies' `died`,
a scoreboard found with `world.only`, a ghost that listens with `listen`
and still hears once destroyed next to one using `on`, which doesn't, a
door whose routine awaits a plate's `next`, and the error from a dead
enemy.

**Step 5: input.** §3.6, with fynkel_flutter's raw events. *Done when*
tests drive each event and gesture from made-up events (taps and double
taps, long press, drags and their capture, swipes, pinch and twist, the
mouse's buttons and hover, keys, axes and actions, the order against
the rest of the frame), objects' handlers bubble, a frame without input
allocates nothing, and you have tried it on the Mac, the iPhone and the
Note10+ with a small test scene.
✅ 34 tests in `test/input_test.dart`, 15 in `test/pointer_test.dart` and
the allocation test (243 in package:fynkel in all), 11 in fynkel_flutter's
`test/input_test.dart`; 12 mutants, all killed (one only after its test
held the finger well past the long press time). The lab's two scenes,
Input: fingers and keys, and Input on objects, were tried on the iOS
simulator (taps, double taps, long press, drags, swipes, bubbling); pinch,
keys and hover wait for the three devices.

**Step 6: motion.** §3.7. *Done when* tests show each ease's ends and
shape, sequences, loops and yoyo, unscaled tweens, stopping with the
owner, awaiting in routines, and the transform helpers against hand-worked
values; 1,000 running tweens cost less than 1 ms a frame on the Note10+.
✅ 6 tests in `test/ease_test.dart`, 42 in `test/motion_test.dart` and 13
in `test/moving_test.dart` (304 in package:fynkel in all); 20 mutants:
15 killed at once, 4 more after a test each (a moveBy loop going on from
where it was, a yoyo group's later parts waiting their turn, a part's
`onComplete` each time it ends, SmoothDamp never passing its target), one
equivalent. 1,000 tweens: median 0.25 ms a frame on the Note10+ (§6). The
lab's two scenes, Motion: eases and Motion: tweens and sequences, were
tried on the iOS simulator (eases, complete, the patrol, the door awaited
in a routine, the race, the shake); the crate's click and the three
devices wait for you.

**Step 7: cameras.** §3.8, and the orthographic camera in the engine.
*Done when* tests show priorities, blends and cuts, each rig against
hand-worked values, shake's ends, and screen and world conversions both
ways (a golden for the orthographic camera), and the garden's and coins'
cameras are rigs.
✅ 30 tests in `test/camera_test.dart` (334 in package:fynkel in all), and
a Rust golden, `tests/golden/orthographic.png`, which the Dart test renders
too; 12 mutants of the camera code: 9 killed at once, 3 more after a test
each (a blend to a camera beating one from the last, the shake's fade, a
virtual camera on the view's camera itself). The garden's camera is an
`Orbit`, coins' a `Follow` and a `LookAt`. Two lab scenes, Cameras: the
basics and Cameras: rigs. Found on the way: in Flutter's debug VM a
`Pointer.asTypedList` view in `_sendTransforms` once had a bad data
pointer and crashed the process (the lab's tests, the Motion scene then a
camera scene; never in AOT or plain Dart); it now writes through the
pointers.

**Step 8: layers and debug drawing.** §3.9, in the engine and in Dart.
*Done when* goldens show a layer hidden from a camera and debug lines
drawn, rays and input skip masked layers, and a frame with no debug
drawing costs what it did.
✅ Rust: layer tests in `fynkel_world`, the extract's culling mask and lines,
`fy_layer_set`/`get`, `fy_raycast`'s mask and `fy_debug_lines_set` through
the C interface, and two goldens: `lines.png` (lines over the cube and
through it) and the cube golden again with a wall on layer 3 left out of
the camera's mask. Dart: 13 tests in `test/layers_test.dart` and 8 in
`test/debug_test.dart`, which draw the same goldens and a new
`debug_shapes.png`, and `test/debug_alloc_test.dart`: drawing a line, a
box and a sphere from ten objects every frame allocates nothing but their
`Vec3`s; 10 mutants of the new Dart code: 8 killed at once, 2 more after a
test each (a line gone the very frame its time is up, a refused culling
mask undone). A frame without debug drawing does no new work: no call into the
engine, and in the renderer two empty checks (not measured on the devices).
Found on the way: on main, a world of plain objects without a mouse over
the view boxes one double and makes one list a frame in the JIT (the alloc
test's scene, with a resting mouse, makes none); not looked into. Lab scene:
Layers and debug: the basics, tried on the iOS 18.4 simulator.

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
✅ Played on all three devices (2026-10-10, "ok all good"). `examples/arena` (9 tests, in CI),
coins on `Spin`, `Bob` and `Collector` behaviours, the examples on the
position setters, `rotate` and `translate` in behaviours, resources freed
by the GC with `own()` gone (`test/collect_test.dart`, 5 tests, the
model keep-alive checked by removing it), hot reload written down (§3.9).
hello was already on `time`, the garden on its `Orbit` rig, coins on a
`Follow` and a `LookAt`; the user's Sphere in hello is left as it is.
**Mac** (profile, 1600×1200 window): all four median 120 fps, no slow
Flutter frames (hello dips to 52 in one window of 30: its Sphere prints
every frame). Dart allocation a frame: hello 6.8 KB, garden 15.5, coins
46.7 (stage 3: 47.9), arena 26.6. Arena lifecycle, 20 cycles: a new round
(`changed`) steps 8 MB once at cycle 4 and then adds 2.2 MB over 16
cycles, GPU and Dart heap flat; destroyed / added +3.4 MB over 20, GPU
flat. Baseline, 10 cycles: changed +0.7, destroyed +2.4, added +2.4 MB.
**iPhone 16 Pro Max** (wireless): all four median 120 fps (min 118 –
120), no slow Flutter frames; Dart a frame: hello 20.2 KB, garden 14.6,
coins 44.8, arena 20.1. Arena lifecycle, 10 cycles, against the
baseline: changed +4.2 MB (+2.6), destroyed +6.3 (+5.9), added +5.2
(+5.4), created +4.9 (+3.3); Metal flat at 167.3 MB.
**Note10+** (1080 × 2241, 30 s): arena, hello and the garden 59.9 fps
(min 56.9 – 58.9); coins 57.9 (min 50.9), and 56.9 over 60 s against
stage 3's 58.9: its 300 motes, at the phone's limit as in stage 3.
Arena lifecycle, 15 cycles, against the baseline: changed +7.6 MB
(+5.9), destroyed +7.8 (+7.6), added +7.4 (+7.8), created +5.3 (+2.6);
Graphics flat at 143.6 MB; Native rises 35.4 → 42.4 MB over the first
cycles, then holds (42.4 → 42.2 from destroyed 8 to 15), as in stage 3.
One run never got its VM service after a 54 s install; run again, it
passed.

**Step 10: the stage 4 check.** A short report, as in stages 1 to 3, and a
guide, `docs/writing-a-game.md`, that teaches all of this from hello up.
*Done when* both are committed and you have read the guide.
Written: [the report](stage-4-report.md) and [the guide](writing-a-game.md),
whose code `tools/check_guide.py` compiles and tests in a throwaway app,
in CI too. To do: your read.

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

### Step 2: the lifecycle (`measure bench`, profile builds, 2026-10-09)

The same bench; main (step 1) measured again the same day, the same way:

| 10,000 objects | Mac (M3 Max) | iPhone 16 Pro Max |
|---|---:|---:|
| moving themselves, main / step 2 | 0.85 / **0.92** ms | 0.77 / **0.81** ms |
| each moved by a behaviour, main / step 2 | 0.90 / **1.02** ms | 0.79 / **0.88** ms |
| with nothing to do, main / step 2 | 0.03 / **0.06** ms | 0.03 / **0.05** ms |
| step 2 without the try/catch (Mac only) | 0.90 / 0.98 / 0.06 ms | – |

Most of the cost is the second walk, for onLateUpdate: every object
(and each behaviour) gets the call, as every script with a LateUpdate
does in Unity. The try/catch that keeps one object's error from stopping
the frame costs 0.02–0.04 ms per 10,000. At 10,000 objects that's at most
0.12 ms, under 1 % of a 60 fps frame; a game with hundreds of objects
won't see it. Kept as it is: skipping objects that don't override
onLateUpdate would need each class to say so.

On the Note10+ (measured with step 3, which adds nothing to a frame
without routines; main before step 1 and step 1 measured the same day):

| 10,000 objects, Note10+ | before step 1 | step 1 | steps 2–3 |
|---|---:|---:|---:|
| moving themselves | 2.88 ms | 2.92 ms | 2.98 / 3.02 ms |
| each moved by a behaviour | – | 3.19 ms | 3.75 / 3.50 ms |
| reading their transforms | 0.18 ms | 0.20 ms | 0.49 / 0.47 ms |
| with nothing to do | 0.08 ms | 0.09 ms | 0.14 / 0.15 ms |

(Two runs of steps 2–3.) Step 1 holds stage 3's numbers (stage 3 missed
its 1 ms budget on this phone, at 2.8 ms). Step 2's second walk, for
onLateUpdate, costs the Note10+ 0.05–0.5 ms per 10,000: more than the
Mac's and the iPhone's, and more for objects whose update touched
memory, as its caches are smaller. Without the try/catch the numbers are
the same (2.98, 3.50, 0.52, 0.14 ms), so it isn't that.

### Step 3: routines (`measure bench`, profile builds, 2026-10-09)

`world.update` with 1,000 routines on one object, median (slowest) of
300 steady frames:

| 1,000 routines | Mac (M3 Max) | Galaxy Note10+ |
|---|---:|---:|
| **waiting** (`await wait(1000)`) | 0.01 (0.01) ms | **0.03** (0.70) ms |
| each going on every frame (`await nextFrame`) | 0.30 (2.28) ms | 0.85 (9.40) ms |
| none | 0.00 ms | 0.00 ms |

Waiting is what the plan budgets (0.5 ms on the Note10+): 0.03 ms. A
routine going on costs about 0.3 µs on the Mac and 0.85 µs on the
Note10+ each time: a wait made, ended and awaited. The slowest frames
are the garbage collector's.

### Step 6: motion (`measure bench`, profile builds, 2026-10-10)

`world.update` with 1,000 motions, median (slowest) of 300 steady frames:

| 1,000 motions | Mac (M3 Max) | Galaxy Note10+ |
|---|---:|---:|
| **moving 1,000 objects** (`moveTo`, yoyo, for ever) | 0.09 (1.17) ms | **0.25** (6.26) ms |
| of a double (`tween`) | 0.02 (0.41) ms | 0.05 (2.78) ms |
| none | 0.00 ms | 0.00 ms |

The bar is 1 ms on the Note10+: 0.25 ms, of which most is setting 1,000
transforms (a double alone is 0.05 ms). The slowest frames are the
garbage collector's, as with routines.
