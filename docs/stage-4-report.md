# Stage 4 report: a solid base for writing games

Step 10 of [the stage 4 plan](stage-4.md). Steps 1 to 9 and the polish
after them are merged, the last as 8184a88e. The details of each step,
and how each number is taken, are in [stage-4.md §4 and
§6](stage-4.md#4-steps); the numbers here are step 9's, from the profile
build (AOT) with package:fynkel_measure's tool.

## The result

Fynkel now works the way Unity works, in Dart. An object is a subclass
of `GameObject` for what it is, with behaviours on it for what it does;
it gets Unity's lifecycle (`onLoad`, `onEnable`, `onStart`, `onUpdate`,
`onLateUpdate`, `onDisable`, `onDestroy`) and reads `time.delta`. It
waits with routines (plain `async` code in game time), tells others
with signals that stop by themselves when the listener goes, and moves
with tweens. Input comes as fingers, gestures, keys and pointer handlers
on objects; cameras are virtual cameras with follow, look-at and orbit
rigs, blends and shake; layers keep effects out of taps and cameras; and
an error in one object's code goes to `world.errors` while the game goes
on.

`examples/arena` is the third game, written with all of it: waves of
enemies to dash through, `Health` and `Flash` shared by the player and
the enemies, a routine for the waves, signals for the score and deaths,
tweens for every hit and pickup, a follow camera that shakes and zooms,
a pause, and a Flutter HUD. 462 lines of game and 87 of HUD. hello, the
garden and coins moved to the new way (the garden 138 → 136 lines, coins
284 → 278).

[`docs/writing-a-game.md`](writing-a-game.md) teaches all of it from
hello up. Its 19 pieces of code compile, and its test passes
(`tools/check_guide.py`); a new app made by its first section's steps
builds and runs on the Mac.

| Each game, 60 s (Note10+: 30 s) | Mac (M3 Max) | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---|---|---|
| Display | 120 Hz, window 1600 × 1200 | 120 Hz, 1320 × 2868 | 60 Hz, 1080 × 2241 |
| hello, garden, arena: fresh fps (median) | **120** | **120** (min 118 – 120) | **59.9** (min 56.9 – 58.9) |
| coins: fresh fps (median) | **120** | **120** | **57.9** (min 50.9) |
| Dart allocation per frame: hello / garden / coins / arena | 6.8 / 15.5 / 46.7 / 26.6 KB | 20.2 / 14.6 / 44.8 / 20.1 KB | – |

**Two misses, both accepted:**

- **coins on the Note10+: 57.9 fps over 30 s, 56.9 over 60 s**, against
  stage 3's 58.9. The plan asked for all four "as well as before". Its
  300 motes were already at this phone's limit in stage 3 (59.9 with
  them moving but hidden); stage 4's frame does a little more for each
  object (the late-update pass, the error guard).
- **The loop on the Note10+**: 10,000 objects each moved by a behaviour
  take 3.5 – 3.75 ms a frame (stage 3: 2.8 ms for objects moving
  themselves). On the Mac and the iPhone the same is 1.02 and 0.88 ms.
  Most of the extra is the second walk for `onLateUpdate`, which the
  Note10+'s smaller caches feel more. A game of hundreds of objects
  doesn't see it.

**Memory**, destroying and re-adding the arena and starting new rounds,
against Flutter alone (`--baseline`):

- The engine's GPU memory is flat through every cycle on all three
  devices (Metal 167.3 MB on the iPhone, Graphics 143.6 MB on the
  Note10+).
- The totals grow about as much as Flutter alone's: on the iPhone +4.2 to
  +6.3 MB over 10 cycles against +2.6 to +5.9; on the Note10+ +5.3 to
  +7.8 over 15 against +2.6 to +7.8. Its native memory rises 35 → 42 MB
  over the first cycles and then holds, as in stage 3.
- On the Mac, a new round steps up 8 MB once and then adds 2.2 MB over
  16 cycles, with the GPU and the Dart heap flat.

**What each feature costs** (`measure bench`, median a frame):

| | Mac | Note10+ |
|---|---:|---:|
| 1,000 routines waiting | 0.01 ms | 0.03 ms |
| 1,000 routines going on every frame | 0.30 ms | 0.85 ms |
| 1,000 tweens moving objects | 0.09 ms | 0.25 ms |

Every bar the plan set for these was met (0.5 ms for waiting routines
and 1 ms for tweens, on the Note10+), and a frame without input,
camera rigs or debug lines allocates nothing (tested).

## What stage 4 built

- **Components and behaviours** (step 1): `addComponent`, `get<T>` (by
  subtype or interface), `getAll`, `has`, `removeComponent`,
  `getInParent`, `getInChildren`; `components:` and `children:` in the
  constructor; the engine's `Camera`, `Light` and `MeshRenderer` changed
  in place; `world.all<T>` finds behaviours too.
- **The lifecycle and time** (step 2): `onEnable`, `onDisable`,
  `onLateUpdate`, `onUpdate()` without `dt`, `time` with `scale` and
  `maxDelta`, and errors that go to `world.errors` while the frame goes
  on.
- **Routines** (step 3): `start`, `wait`, `until`, `whileTrue`,
  `nextFrame`, `race`, `all`; they return values, run their `finally`
  when stopped, stop with their object and pause while it's hidden.
- **Signals** (step 4): `Signal`, `on`, `next`, `world.only`, and clear
  errors from destroyed objects.
- **Input** (step 5): fingers, taps, double taps, long presses, drags,
  swipes, pinch and twist, scroll, keys, axes and actions, and pointer
  handlers on objects and behaviours that bubble up.
- **Motion** (step 6): `tween`, 31 eases, `sequence`, `together`, loops
  and yoyo, the helpers (`moveTo` … `shake`), the transform shortcuts
  (`position`, `rotate`, `lookAt` …) and Unity's Mathf.
- **Cameras** (step 7): virtual cameras with priorities and blends,
  `Follow`, `LookAt`, `Orbit`, shake, the orthographic camera (ABI 12),
  `screenToRay`, `screenToPlane`, `worldToScreen`.
- **Layers and debug drawing** (step 8): a layer on each object, culling
  masks, masked rays and input (ABI 13); `debug.line`, `ray`, `box`,
  `sphere`, `path`, drawn by the engine.
- **The arena** (step 9), and after it: resources freed when nothing
  reaches them (`own()` is gone), a destroyed object's components still
  readable, `FynkelGame` with F5 and `restartOnReload`, `FynkelApp(X.new,
  overlay:)`, the subclass-or-behaviour rule written down, and
  `world.dart` split in five files.
- **The lab** (`examples/lab`): a scene for each feature, 14 in all, each
  with a "the basics" one where the feature has many parts.
- **Tests**: 362 in package:fynkel, 27 in
  package:fynkel_flutter, 36 in the examples; each step's new code was
  checked with mutants (128 in all, every one killed or shown
  equivalent).

## What was found on the way

- **A boxed double every frame since step 7**, in the input step's pinch
  check, even with no finger down. It showed only in some JIT runs, so CI
  found it and the Mac didn't; `--no-background-compilation` reproduces
  it every time. Fixed before the arena merged (#40).
- **CI's format check had failed since step 8**, so CI ran none of the
  tests after it until #40. Both slips were small; the gap was not.
- **The Mac's GPU driver** holds on to upload memory for up to about 18 s
  after a texture is freed, so the texture-memory test waits 30 s.
- **Flutter's debug VM** once gave a bad pointer for a
  `Pointer.asTypedList` view of the frame's transforms and crashed; the
  hot paths now write through the pointers.
- **The iOS 26.5 simulators crash on this Mac** (an empty dyld cache);
  the lab is tried on an iOS 18.4 simulator.
- **Writing the arena** showed the rough edges fixed after step 9: no
  setters for `position` in behaviours, resources a game had to `own()`,
  `get<Health>()` failing on a dead enemy, and no way to see a changed
  `onLoad` without restarting the app.

## Still open

- **The Note10+ at full resolution**, from stage 3: coins is about 2 fps
  short. A lower render scale would give room.
- **The loop's cost on the Note10+**: see the second miss.
- **Step 8's other allocation**: a world of plain objects with no mouse
  over the view also made one list a frame in the JIT. The boxed double
  was found and fixed; the list wasn't checked again.
- **Not in stage 4, by plan**: physics and collisions (and with them a
  fixed time step), audio, animation of models, point and spot lights
  and shadows, saving, gamepads, scene files, an editor.
- **From earlier stages**: the view makes its GPU device on the UI
  isolate (0.84 s on the Note10+ at startup); the native memory
  finalizers don't tell the garbage collector their size; a surface
  Flutter closes under the engine; the Flutter leak report; 64-bit
  Android only.

## Seen on each device

Step 9 is done when you have played the arena (and the others) on each
device. Confirmed on 2026-10-10 ("ok all good"):

- [x] Mac
- [x] iPhone 16 Pro Max
- [x] Galaxy Note10+

Step 10 is done when you have read [the guide](writing-a-game.md).
Closed on 2026-10-10 ("now stage 4 is closed"), with the two misses
above accepted:

- [x] Read

**Stage 4 is done.**
