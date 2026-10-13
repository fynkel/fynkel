# Stage 6 report: the review

Step 8 of [the stage 6 plan](stage-6.md). Steps 1 to 7 are merged, with
step 6's last fix, PR 15 (measured again).

## The result

Stage 6 stepped back from features and asked three things of the whole:
is it good, is it understandable, does it scale. Seven agents wrote
games from the guide alone, an eighth wrote a 41-file game, every public
name was reviewed, the architecture was reviewed, and a world of 10,000
objects ran on each device. That found 35 findings, 8 API decisions and
21 architecture items. **Fourteen PRs fixed all but six of them**, and
the six are waiting for stage 7 or later on purpose.

The test that counts is step 7: two new agents wrote the menus game and
the shooter again on the fixed framework. **None of step 1's 13 findings
from those two games came back**, and nothing blocked them. What they
hit instead is smaller (findings 36–43). Both said, in their own words,
that the parts stage 6 added were the easy parts: "Pause just works with
Flutter dialogs", "§14 'Bullets' read as if written for exactly this
brief".

## What stage 6 did

| Step | What | Where |
|---|---|---|
| 1 | Four fresh agents, four games (menus, shooter, toy, walker) | [findings](stage-6-findings.md) A–F |
| 2 | Every public name in a table; 8 decisions (A1–A8), all as recommended | [stage-6-api.md](stage-6-api.md) |
| 3 | The architecture: 21 items (R1–R21), and an audio package sketched | [stage-6-architecture.md](stage-6-architecture.md) |
| 4 | Outpost, a 41-file tower defense, written the same way | [findings](stage-6-findings.md) G |
| 5 | 10,000 objects, 30 minutes on each phone, errors everywhere, hot reload under physics | [stage-6-scale.md](stage-6-scale.md) |
| 6 | The fixes, 15 PRs (#69–#83 and #88) | [stage-6-fixes.md](stage-6-fixes.md) |
| 7 | Two new agents, briefs 1 and 2 again | [findings](stage-6-findings.md) H |

## Before and after

| Found | Fixed | Left, on purpose |
|---|---|---|
| Findings F1–F35 | 32 | F6 (the two `Color`s) and F33 (a widget at a world point) go to stage 7; F11 was answered by step 4 |
| API decisions A1–A8, and B, C | all | |
| Architecture R1–R21 | 17 | R10's app pause goes to stage 7; R12 (lights and shadows), R13 (one GPU device per view), R14 (texture memory), R15 (the runtime's crate) wait for stage 7 or the look |

What changed most, in plain words:

- **The game can't silently stop any more.** A throw in a frame, a
  dispose in the middle of an update, a lost GPU, a stale `Physics`:
  each now has a defined, tested ending (PRs 1–3).
- **Flutter and the game work together.** `FynkelApp` is a real
  `MaterialApp`; `world.paused`; Flutter buttons and sticks drive the
  game's input; keys the game doesn't use reach Flutter's shortcuts;
  signals are streams (PRs 3 and 10).
- **Unity's meanings.** `position` is the world's, angles are radians
  with `.deg`, `isPressed`, `startRoutine`/`waitUntil`, `gameObject`,
  `RaycastHit` (PRs 5 and 6).
- **Scale.** The hierarchy lives in Dart, physics follows only what
  moved, culling, a lighter sphere, pooling that allocates nothing (PRs
  4, 7, 8, 9).
- **Packages.** `extend.dart` version 2: their own asset kinds, ordering,
  component removals, taps on colliders, the app going to the background
  (PR 12a).
- **Trust.** The allocation-test flake found and fixed at its root, a
  soak test, damaged-file fuzzing, Miri; every public member documented,
  one error type for one mistake (PRs 13–14). A README and a guide a
  newcomer can start from (#84).

| Brief | Step 1 | Step 7 |
|---|---|---|
| menus | 6 findings, 3 confusing | 1 confusing (37), 3 minor (41–43) |
| shooter | 8 findings, 3 confusing | 1 confusing (36), 5 minor (38–41, 43) |
| Blockers in either | 0 (step 1's blocker, F1, came from the toy) | 0 |

## What the fresh review found (2026-10-13)

After PR 14, a full review of the engine (six agents: the Dart core, the
Rust engine, physics, Flutter and tooling, a newcomer writing a game, and
project health) found real bugs no step had hit. The ones confirmed in
the code:

1. **Changing the hierarchy in `onDestroy` or `onDisable` splits Dart
   from the engine.** `Player.onDestroy() { weapon.parent = null; }`
   leaves the weapon drawn but forgotten by Dart; the next
   `world.raycast` throws. (`setParent` doesn't look at `_destroyed`.)
2. **`world.debug.line()` after `world.dispose()` crashes the app**:
   `Debug._release` frees the buffer but keeps its capacity.
3. **The show/hide refresh skips a sibling** when a child's `onDisable`
   moves it, so a hidden object keeps updating.
4. **A character can't stand on the world's first collider**: its key is
   0, which also means "no ground", so `ground` is null and platforms
   don't carry it.
5. **A zero scale gives a NaN rotation** in the world transforms physics
   reads, and for a camera or sun under it.
6. **A second `MovedReader` can miss moves** (it doesn't raise the log's
   read mark), which bites the moment two packages read it.
7. **Physics setters keep a bad value after throwing** (`rb.mass = 0`).
8. **Listening or starting a routine in a behaviour's `onDestroy`**
   outlives the behaviour.
9. **Camera shake drifts** a camera moved during the shake; **a held
   `InputAction` breaks on rebind**; **two views of one world** run it
   twice a frame; **near ≥ far** draws nothing, silently.
10. **A 500-byte damaged `.glb` can ask for gigabytes**: the loader caps
    image sizes but not vertex counts.

And two cheap wins: the MSAA colour and depth targets aren't marked
transient (about 140 MB of GPU memory on the Note10+ at full size), and
the extract does work for every object on the UI thread before culling.

**My recommendation: one more PR before stage 6 closes**, "the review's
bugs": 1–8 and 10, finding 36 (the guide's bullets recipe), the transient
targets. They are bugs, small and local, and stage 6 is where bugs get
fixed. 9's items and the extract go with it if they stay small.

## What goes to stage 7

Stage 7 is Flutter and the game: menus, pop-ups, themes, several views.
It starts with:

- **F6**: one colour for the theme and the game (a `Color` bridge, and
  the name clash: `Color`, `Material` and `Transform` are Flutter's names
  too).
- **F33**: a widget at a world point (health bars, labels).
- **The app's pause** (R10's part), and **`world.paused` to listen to**
  (finding 38).
- **`game.world` there from the start** in `FynkelGame`'s builder
  (finding 37), so widgets beside the view can reach the input.
- **R13**: one GPU device per process, not per view, before several
  views; and **two views of one world** updating it once.
- **A UI reading physics waits for its step**: now documented; stage 7
  decides whether a widget gets a copy that never waits.
- **The review's 9 and the extract**, each a choice more than a fix:
  camera shake drifting a camera moved during it (a shake can't tell
  "moved to" from "moved by"), a held `InputAction` on rebind, near ≥
  far (checking it would refuse setting `near` then `far`), and the
  extract's work for every object before culling.
- The small ones from step 7 that stage 7's work touches: a `Pool`'s
  objects to walk (39), unbound axes (40), a picture out of
  `HeadlessView` (41).

## Later

- **The look**: shadows, point and spot lights, more than one sun,
  transparency, skinned and animated models, compressed textures (R12,
  R14).
- **Shipping**: a build that works outside this repository (the hook
  finds the crates at `../../`), or prebuilt binaries; a licence and the
  Rust dependencies' notices; versions and a changelog; Android builds
  from a non-Mac host; a CI runner that isn't this Mac.
- **Speed on the Note10+ at 10,000 objects**: 27–36 fps after PR 15, its
  GPU the limit. The UI thread's next places to look are the loop that
  calls lifecycle methods objects don't use, and the world-pointer
  getters. Step 5's dense sphere at 10,000 swings between 17 and 36 fps
  on its Mali GPU: for a GPU profiler.

## Measured again

PR 15 ran the step 5 checks again after the fixes. In full:
[stage-6-scale.md, step 6](stage-6-scale.md#step-6-measured-again).

| 10,000 objects | Step 5 | After stage 6 |
| --- | --- | --- |
| Mac | 120 fps | 120 fps |
| iPhone | 44 fps | **120 fps** |
| Note10+ | 34.9 fps | 27–36 fps shown, the game updating 52–56 times a second |

- It found two slowdowns the fixes had brought in, and fixed them. Each
  object's world position was worked out again whenever anything in the
  world moved (11 MB of Dart garbage a frame with 9,000 parts bobbing;
  now 1.15 MB). And on Android the game's update waited for the GPU,
  because Flutter waits for each frame on the thread Dart runs on; our
  render thread now waits first. That took the Note10+ from 12.7 fps to
  27–36 at 10,000 objects, and from 23 to 54.9 at 1,000.
- The Note10+'s UI thread, profiled with Dart names: half of it is
  `World.update`, mostly the loop calling each object's lifecycle
  methods; a third is the view's frame, mostly the engine's extract.
- The Mac's memory per object isn't the GPU's part, which stays flat.
  What grows follows what is drawn, not time: no leak.
- Android's performance hints aren't supported on the Note10+.

## Read

Step 8 is done when you have read this.

- [x] Read
- [x] The review's bugs: one more PR in stage 6 ([#87](https://github.com/mooumari/fynkel/pull/87):
  1–8, 10, finding 36, the transient targets; 9 and the extract to stage 7)
- [x] PR 15's numbers in

Then **stage 6 is done**.
