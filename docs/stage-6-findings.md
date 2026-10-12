# Stage 6 findings

What the review finds, in one list, step by step. Step 1's come from four
fresh agents (2026-10-12), each given only the guide and the packages'
public code, each writing a game from a brief:

| Brief | Lines (game / tests) | Tests | Builds | Their time |
|---|---|---|---|---|
| menus: title, settings pop-up, pause, game over | 548 / 212 | 10 | ✅ | ~1 h |
| shooter: waves, pooling, many bullets | 733 / 312 | 17 | ✅ | ~1.5 h* |
| toy: physics sandbox, drag, joints, basket | 638 / 390 | 20 | ✅ | ~1 h* |
| walker: a level, a controller, a joystick | 669 / 354 | 19 | ✅ | ~1.5 h* |

\* Their guess at a person's time, from the costs they logged.

All four games build, analyze clean and pass their tests; most of their
code compiled the first time. Their logs are kept in `/tmp/s6/*/LOG.md`
(not in the repository). **What they all liked**: the headless tests (a
`World` run frame by frame, physics and fingers included), the Unity
names working as guessed, routines and signals, and the HUD as plain
Flutter.

Each finding: who hit it, whether I confirmed it in the code, how much it
hurt (**blocker**, **confusing**, **minor**), and a first idea for the fix.

## A. Flutter and the game (most of this goes to stage 7)

1. **`FynkelApp`'s overlay can't hold Material widgets.** It is a bare
   `WidgetsApp` with a `builder` and no navigator, so there is no
   `Overlay`, `Material` or `Theme`: a `Slider` in the HUD fails with "No
   Overlay widget found", and the guide's HUD route leads there. (toy;
   walker wondered too.) Confirmed. **Blocker.** Fix: `FynkelApp` gives
   the overlay a real app (a navigator, and Material if the game wants
   it, with a `theme`), or the guide sends HUDs with Material widgets to
   `MaterialApp` + `FynkelGame` + `FynkelView`.
2. **No pause.** `time.scale = 0` stops time, but taps still reach objects
   (each handler checks a flag), and nothing ties a pause menu, the app
   going to the background, or a dialog to the game. (menus) **Confusing.**
   Fix: a first-class pause (time and the game's input), and a recipe.
3. **Flutter controls can't feed the game's input.** Axes and actions take
   only keys; a Flutter joystick or button writes fields on the game,
   and the player reads two paths. (walker) **Confusing.** Fix: an axis
   and an action can be set from outside (`input.setAxis2`, `press`), and
   perhaps an on-screen stick widget, as Unity's on-screen controls.
4. **Keys go to the view, not to Flutter's shortcuts.** An Escape-to-pause
   shortcut has to live in the game and come back through a notifier.
   (menus) **Confusing.** Fix: decide which keys Flutter sees first, and
   write it down.
5. **The game tells the UI things only through `ValueNotifier`s.** "Game
   over, open a dialog" is a notifier a widget listens to; a `Signal` has
   no `Stream` or `Listenable` side. (menus) **Minor.**
6. **Two `Color`s, no bridge.** A palette shared by the theme and the
   scene is written twice. (menus, walker) **Minor.** Fix: conversions in
   `fynkel_flutter`, and themes passed into the game (stage 7).
7. **Leaving the page or `restart()` runs no `onDestroy`/`onDisable`**,
   and `isDestroyed` stays false; the guide says leaving "destroys the
   game". Clean-up and saving in `onDestroy` never run. (menus) Confirmed
   (`World.dispose` stops listeners and waits, and frees the engine,
   without destroying the objects). **Confusing.** Fix: dispose destroys
   the root first.

## B. Pooling and many objects

8. **No pooling help.** No helper, no guide text; and what a hidden object
   keeps has to be found out: its routines pause and later resume stale,
   and `world.all<T>()` still counts it, so §9's own wave pattern never
   ends once you pool. `onEnable` as the reset point works well. (shooter)
   **Confusing.** Fix: a small pool, and a guide section; perhaps
   `world.all` with `includeHidden`.
9. **A query costs every collider after anything moved.** After any
   transform is set, the next query walks every collider to see which
   moved; a shooter casting and moving 500 bullets among 300 enemies paid
   4.7 ms a frame, 2.6 when it cast first and moved after. (shooter)
   Confirmed (`Physics._followMoved` checks each collider's stamp). **Confusing**
   (silent). Fix: the core keeps a list of moved objects, so a query
   looks only at those (stage 5's plan had it).
10. **No word on bullets**: a trigger and a body on each, or casts from
    one manager? (shooter) **Minor.** Fix: a guide recipe with costs.
11. **Where the line is between objects that do things and data moved by a
    manager**, and how a 50-file game passes things around
    (`world.only<T>()` everywhere feels like singletons). (shooter)
    Goes to step 4 (the bigger game).

## C. Materials and meshes

12. **No per-object tint.** The guide says share materials, and
    `colorTo` changes the shared one, so a hit flash flashes everyone.
    (shooter) **Confusing.** Fix: a per-renderer colour (Unity's
    `MaterialPropertyBlock`), or `Material.copy()` with the guide saying
    when.
13. **No `Mesh.capsule()`** (or cylinder) to match `CapsuleCollider` and
    the character controller. (toy, walker) **Minor.**

## D. Physics

14. **Changing gravity leaves sleeping bodies in the air**, and waking
    them throws for a body not yet in the world, with no way to ask, and an
    error that names the wrong reason. (toy) **Confusing.** Fix: setting
    `gravity` wakes every body; `wakeUp` on a body not yet in the world
    does nothing, as Unity's.
15. **An error from a destroyed object's rigidbody** says "not enabled, or
    its object isn't shown", not "destroyed". (toy) **Minor.**
16. **A joint's `anchor` and scale**: "in the object's own space" doesn't
    say whether scale applies. (toy) **Confusing.** Fix: say it, in the
    doc and the guide.
17. **No drag-a-body helper**: every physics game writes one (the toy's
    took 70 lines, the lab's chain has one). (toy) **Minor.** Fix: a
    small `DragRigidbody` behaviour, or a guide recipe.
18. **Trigger stay stops when a kinematic body sleeps.** As Unity's, and
    documented, but it caused a bug. (shooter) **Minor.** Fix: say so
    where it bites, or keep kinematic bodies moved by hand awake.
19. **Calling the game from Flutter callbacks is safe** (physics waits for
    its step), but only a private doc comment says so. (toy) **Minor.**
    Fix: one line in the guide.

## E. The frame and the API

20. **Moving platforms**: tweens run after the updates, so a platform
    moved with `moveTo` is a frame behind its rider; the character
    controller doesn't carry it or say what it stands on; order is the
    order objects were added, with no way to set it. (walker)
    **Confusing.** Fix: a ground object and its motion on the controller;
    look at where tweens run.
21. **Common words are taken on every object**: `start`, `on`, `time`,
    `wait`, `until`, `all`… A field named `start` doesn't compile. (walker)
    **Minor.** Goes to step 2 (the API review).
22. **`Follow`'s offset is in the world only**, not behind a turning
    target. (walker) **Minor.**
23. **An error in `onLoad` destroys the object**, which §6's "errors don't
    stop the game" doesn't say. (toy) **Minor.** Fix: say it.

## F. Tests and the guide

24. **Widget tests of the whole app work** (the real view, real taps), and
    the guide doesn't say so; nor that `pumpAndSettle` never settles with
    a view on screen, that taps need `input.resize` first, or how to tap
    an object (`worldToScreen` + `fingerDown`). (menus, toy, shooter)
    **Minor.** Fix: a part of §17.
25. **One agent's widget test of `FynkelApp` never ticked the game**,
    where three others' did. (walker) Not yet understood: to reproduce
    in step 6.
26. **Does a game error fail a plain test by itself?** Yes, but §17
    doesn't say; `world.errors` is a stream, not a list. (walker)
    **Minor.**
27. **No recipe for "rebuild the level in place"** (reset without a new
    world). (toy) **Minor.**
28. **Moving a kinematic trigger body by hand**: `position =` or
    `movePosition`? (shooter) **Minor.** Fix: a line in §14.

## G. Step 4: the bigger game

A fifth fresh agent wrote **Outpost** (2026-10-12), a tower-defense game
on the same rules as step 1's: two levels from text maps, four enemy kinds
(a splitter among them), three towers with upgrades and selling, waves
from data, pooled enemies and shots, and a Flutter title screen, build
menu, tower panel, HUD, pause and win/lose pop-ups with saved best
results. **41 files and 2,204 lines in `lib/`; 41 tests in 801 lines** (39
pass, 2 skipped as the repro of finding 29), headless and widget tests,
one of them a bot that wins both levels. It builds and analyzes clean; its log
is in `/tmp/s6/outpost/LOG.md`. I built it in release and it starts on the
Mac, the iPhone and the Note10+ (Android needed finding 35 first).

**How it grew**: one root object, `OutpostGame`, makes everything in its
constructor (so the HUD can bind before the game loads) and hands each
manager (`Board`, `Enemies`, `Projectiles`, `Towers`, `WaveManager`) the
others it needs. Objects find each other through references passed in:
the game never calls `world.only` or `world.all`, and it never felt like
globals (finding 11's worry). Subclasses are what things are (enemy,
tower, shot, tile, the managers); behaviours what they do (`Health`,
`Slow`, `PathWalker` for walkers and flyers alike, `Shooter`,
`Economy`); kinds are const data with no engine import. The guide's rule
"a subclass is, a behaviour does" helped. What got harder with the file
count: constructors with many references (`Towers` takes 7; next time it
would pass one context object), and import cycles between `core/` and the
feature folders (Dart allows them, so nothing warned).

29. **Awaiting a routine that ends at once hangs in a headless test.**
    `await start(() async {})` (or `all([...])` of such routines) never
    resumes when the outer routine was started outside the frame's routine
    step (from `onStart`, a signal or the test) and the frames are driven
    by a plain loop of `world.update`: the inner routine's end is handed
    to Dart's own microtask queue, which a synchronous test never drains.
    In a running app the queue drains between frames, so a game doesn't
    hang, but the guide's tests are synchronous loops, and the agent lost
    25 minutes to it. Confirmed (with `await Future.delayed(Duration.zero)`
    between frames it passes). **Confusing**, and silent. Fix: a routine's
    microtasks always go to the world's own queue, drained in the frame.
30. **Pooling, again** (finding 8): the agent wrote a pool, a reset step
    (not `onEnable`: it takes no arguments, and a new object isn't loaded
    when taken), "subscribe once" on first use, its own "in play" list
    because `world.all` counts hidden objects, and a generation number,
    after a dying splitter came back from the pool as its own child and a
    shot nearly hit the new enemy. **Confusing.** Fix: the pool of B
    (finding 8) gives versioned handles, or the guide says how.
31. **No word on wiring a bigger game**: a root that holds managers,
    references passed in, a context object, data-driven variants (a
    "prefab is a subclass" doesn't fit a runner and a tank of one class).
    **Minor** (the agent found a good way alone). Fix: a guide section
    from Outpost's shape.
32. **`on(...)` takes only `Signal`s**, so game code reacting to a
    `ValueNotifier` the HUD also reads uses a raw `addListener` nothing
    removes (finding 5, the other way round). **Minor.** Fix: `on` takes a
    `Listenable` too, and `Signal` gains a `Listenable` side.
33. **Health bars over enemies** are 3D cubes that don't face the camera:
    no billboard, and no widget placed at a world point. **Minor.** Stage 7
    (a widget at a world point).
34. **Reaching the game from a widget test of the real view** takes
    `tester.state<FynkelViewState>(...).world.all<OutpostGame>()`. **Minor.**
    Fix: `FynkelGame.of(context).game`, or a finder (with finding 24).
35. **An Android build needs Android 11 (`minSdk = 30`)**, and the guide
    doesn't say so: a new app's first `flutter build apk` fails in the
    manifest merge (`minSdkVersion 24 cannot be smaller than version 30
    declared in library [:fynkel_flutter]`). Found when I built Outpost
    for the Note10+; the agent built only for the Mac. **Minor**, but a
    first-run wall. Fix: one line in §1.

Seen again, already listed: `onDestroy` not run when the page goes (7),
`fovY` in degrees while the guide says radians (A1), the `Color` clash
(handled by keeping game and widget files apart, as the guide says), and
pause by `time.scale` with taps blocked only because the pause overlay is
opaque (2). And what it liked best: whole systems tested headless in about
a second, `FynkelGame` and `FynkelView` inside its own `MaterialApp` with
`restart()` for retry, and taps reaching objects with no physics, passed
up to the root, through a layer mask.
