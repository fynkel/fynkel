# Stage 3 report: how a game is written

Step 7 of [the stage 3 plan](stage-3.md). Measured on 2026-10-09 on each
step's branch, the last merged as ae0ef8f9, with package:fynkel_measure's
tool on the profile build (AOT). The details of each step, and how each
number is taken, are in [stage-3.md §6](stage-3.md#6-measurements).

## The result

A game is now plain Dart classes: a `GameObject` subclass says what an
object is made of in its constructor, loads its own assets by name in
`onLoad`, and acts in `onStart`, `onUpdate`, `onTap` and `onDestroy`.
Levels, players and managers are objects like any other, and
`runApp(FynkelApp(Garden()))` is the whole app.

`examples/garden` is rewritten this way: five classes, 111 lines of game
code (from 146), and nothing about measuring. `examples/coins` is the
second game: two levels and the change between them, a player who
outlives them and carries a different model in each, coins found by type,
a score shown by a Flutter HUD, two mixins on one class, a logic-only
child, and a manager moving 300 objects.

| 60 s of each game | Mac (M3 Max) | iPhone 16 Pro Max | Galaxy Note10+ |
|---|---|---|---|
| Display | 120 Hz, window 1600 × 1200 | 120 Hz, 1320 × 2868 | 60 Hz, 1080 × 2241 |
| garden: fresh fps (median, min) | **120**, 120 | **120**, 120 | **59.9**, 57.9 |
| coins: fresh fps (median, min) | **120**, 120 | **120**, 118 | **58.9**, 54.9 |
| Dropped frames, garden / coins | 0 / 0 | 0 / 1 | 0 / 0 |
| Dart allocation per frame, garden / coins | 14.5 / 47.9 KB | 13.3 / 44.1 KB | 19.2 / 46.1 KB |

**Two misses, both accepted:**

- **The garden is 111 lines, not 60.** The rest would come only from
  packing code in ways the formatter undoes (one line per item of a broken
  list, one per closing bracket). You accepted it as it is.
- **coins runs at 58.9 fps on the Note10+, 1 fps short.** What's left is
  drawing its 300 motes: with the swarm moving but hidden it runs at 59.9.
  The Note10+ is at its limit at full resolution with either game.

**Memory** (destroying and re-adding the whole game, and changing coins'
level back and forth, against Flutter alone with `--baseline`):

- The engine's GPU memory comes back exactly at every destroy and level
  change, in both games, on all three devices.
- On the Mac and the iPhone the totals grow no more than Flutter alone's.
- On the Note10+ they grow more for the first 15 – 20 cycles, then as
  much. A probe pinned it down: about 20 MB of live native allocations
  over the first 25 cycles, then none; nothing at all without a view. It's
  the GPU side's own pools (the Vulkan driver's or wgpu's), the bounded
  growth stage 2 found and you accepted then.

**Loading:** the garden is on screen 140 ms after it starts loading on the
Mac, 110 ms on the iPhone and 1.25 s on the Note10+ (its first view's
0.84 s included). A new garden in the same view takes 25, 37 – 68 and
71 – 89 ms. Changing coins' level, models and all, is on screen in
20 – 23 ms on the Mac and 26 – 40 ms on the iPhone, and the old level
stays on screen until the new one is there.

**The loop's cost** (`measure bench`, 10,000 objects in trees, median):

| 10,000 objects | Mac | iPhone | Galaxy Note10+ |
|---|---:|---:|---:|
| The loop with nothing to do | 0.03 ms | 0.03 ms | 0.07 ms |
| Reading their transforms | 0.05 ms | 0.05 ms | 0.14 ms |
| Each moving itself (`rotate`) | 0.80 ms | 0.76 ms | **2.76 ms** |

Against a budget of 1 ms: met on the Mac and the iPhone; the Note10+'s
is mostly Dart's own arithmetic and the engine's lookups, both slower
there per object.

## What stage 3 built

- In `fynkel_world`: the active flag (an inactive object's subtree isn't
  drawn or hit, and its cameras and lights don't count). In `fynkel_ffi`: the flag (ABI 9), the view's
  camera (`fy_camera_active`, ABI 10), and a frame's moves in one call
  (`fy_transform_set_many`, ABI 11).
- In `package:fynkel`: `GameObject` you can extend, with `add`, `destroy`,
  `onLoad`, `onStart`, `onUpdate`, `onTap` (bubbling up) and `onDestroy`;
  the same instance back from `find`, `children`, `parent` and rays;
  `world.all<T>`, `first<T>`, `ancestor<T>`; `world.update(dt, input)`
  with `Input` and `Tap` in plain Dart; transforms kept in Dart; `Assets`
  by name, shared, owned by who asked and freed with the last of them;
  `preload`, `own`, `loaded`, `world.errors`.
- In `package:fynkel_flutter`: `FynkelView(root:)`, `FynkelApp`,
  `bundleSource`; Flutter's events fill `Input`.
- In `package:fynkel_measure`: `MeasuredApp`, which measures any game from
  a separate entry point (`lib/measured.dart`), with a lifecycle check of
  any root, `overlay:` for its HUD and `change:` for a change it makes
  again and again.
- hello (one class, a turning cube), the garden rewritten, and coins.

## What was found on the way

- Moving 10,000 objects one engine call each took 1.5 ms on the Mac and
  6.1 ms on the Note10+. Keeping transforms in Dart and sending a frame's
  moves in one call brought it to 0.8 and 2.8 ms, and reading to nothing.
- Visiting only objects that override `onUpdate` can't work with
  `@mustCallSuper`, so every object is visited; the loop costs 0.03 ms
  for 10,000.
- A tap must be cast from the camera the view draws from, which only the
  engine knows, hence `fy_camera_active`.
- Each helper isolate left its scratch behind: about 1.7 KB of native
  memory a model or texture load, now freed (`inHelper`). About 3 KB a
  load is left, which is the Dart VM's own cost of an isolate.
- The built-in sphere (2,304 triangles) as 300 motes took the Note10+ to
  29 fps; small cubes are enough.
- The measuring again: the wait loops found their own command line, and
  10 cycles on the Note10+ weren't enough to see its growth stop, so it
  took 30.

## Still open

- **The Note10+ at full resolution**: coins is 1 fps short and both games
  have many slow raster frames. A lower render scale, as the iPhone used
  in Fynkel 1, would give room; nothing in stage 3 needed it.
- **Moving many objects**: coins' swarm allocates a `Transform` per mote
  per frame (three times the garden's Dart memory a frame), and 10,000
  objects moving themselves take 2.8 ms on the Note10+. A way to move many
  at once, in Dart or in Rust behind the same manager, waits for a game
  that needs it.
- **Destroying a large tree** walks it to forget each Dart object: 1.1 ms
  for 10,000. A single call returning a subtree's ids would remove most of
  it.
- **Not in stage 3, by plan**: animation, physics, audio, saving, several
  behaviours on one object, object input beyond taps, scene files.
- **From stage 2**: the view makes its GPU device on the UI isolate
  (0.84 s on the Note10+ at startup); the native memory finalizers don't
  tell the garbage collector their size (`externalSize`). From stage 1: a
  surface Flutter closes under the engine; the Flutter leak report; 64-bit
  Android only.

## Seen on each device

Step 7 is done when you have played the garden and coins on each device
yourself. Confirmed on 2026-10-09 ("they look good"):

- [x] Mac
- [x] iPhone 16 Pro Max
- [x] Galaxy Note10+

**Stage 3 is done.**
