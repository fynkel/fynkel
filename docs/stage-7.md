# Stage 7 plan: Flutter and the game

Status: accepted 2026-10-13, with the recommended answers to §3. Scope
and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 6 is
done ([report](stage-6-report.md)).

Stage 6 made the framework good to write games with. A game is more than
its world, though: it has a title screen, settings, pop-ups that pause
it, a HUD, health bars over its enemies, the app's colours, and sometimes
a second view (a minimap). In Fynkel all of that is Flutter, and today
the two meet at one place: `FynkelApp`'s `overlay`. Stage 7 makes them
meet everywhere a game needs them to.

**The goal.** A game that feels like an app: a main menu, settings
(colours, controls), a game that pauses under every dialog, a HUD and
labels that follow things in the world, the app's theme in the game, and
a minimap, all written simply, all in the guide, all tested.

**The road after it**, so there is a finish line: stage 8 is the look
(shadows, point and spot lights, transparency, animated characters),
stage 9 is shipping (a build outside this repository, a licence,
versions, CI that isn't this Mac). After stage 9, a small real game can
be written and shipped with Fynkel: that is version 1.

## 1. What there is today

- `FynkelApp(Game.new, overlay: ...)`: a `MaterialApp` with the game and
  a loose overlay on a transparent `Material`, themes, a navigator key,
  app shortcuts.
- `FynkelGame`: the game in any widget tree; `FynkelView`: the view of
  a world, which feeds it input, the app's lifecycle
  (`world.applicationPaused`) and claims only the keys the game uses.
- `world.paused` stops time and input; a `Signal` and a `Listenable`
  bridge each way (`Signal.stream`, `Listenable.changed`).
- Each view makes its own GPU device (architecture R13).

What's missing, from stage 6: one colour for both (F6), widgets at a
world point (F33), `world.paused` to listen to (38), `game.world` in
`FynkelGame`'s builder (37), reaching the game from a widget (34),
several views (R13), and the review's item 9.

## 2. Phases

Each phase is one PR or a few. Each adds its lab scene ("the basics"
first), its guide section, and its tests; each PR is explained in plain
words. A phase is done when its PRs are merged with CI green.

### Phase 1: groundwork

The small things the other phases stand on.

- `game.world` there from the start in `FynkelGame`'s builder (37), and
  `FynkelGame.of(context)` to reach the game from any widget below it
  (34).
- `world.paused` to listen to, as a `Listenable` (38), so a pause panel
  rebuilds by itself.
- A `Pool`'s objects to walk (39); `bindAxis2('aim')` and
  `bindAction('fire')` with no keys, for one only Flutter controls feed,
  and `hasAxis2`/`hasAction` to ask (40; a name never bound still throws,
  so a typo is loud);
  a picture out of `HeadlessView` (41); the guide's papercuts (42, 43).

*Done when* a pause panel and a HUD beside the view (not only in the
overlay) are written without a workaround, and step 7's minor findings
are closed.

### Phase 2: one look for the app and the game

- Fynkel keeps its names (`Color`, `Material`, `Transform`: §3.1).
  `package:fynkel_flutter` adds conversions both ways (a Flutter colour
  to a Fynkel one and back), and the guide shows the import with a
  prefix for files that use both.
- **The game follows the app's theme**: a game can read the app's
  `ThemeData` (its colour scheme, light or dark) and hear when it
  changes, so switching the app to dark changes the game's sky, light
  and colours too.

*Done when* a palette is written once and used by the theme and the
scene, and the lab's scene switches light and dark from a toggle.

### Phase 3: widgets in the world

- A widget at a point in the world (F33): health bars, name tags, damage
  numbers, "press E" hints. It follows an object or a point, is placed
  by the camera each frame, hides behind the camera and off the screen,
  can shrink with distance, and stays smooth with a hundred of them.
- Taps on such a widget reach the widget, not the game.

*Done when* the arena's enemies have Flutter health bars that follow
them, measured on the Note10+ with 100 of them.

### Phase 4: menus, pop-ups and navigation

- A title screen → the game → pause → settings → back, with Flutter's
  navigator. Every dialog and page over the game pauses it and gives the
  game no taps or keys; closing the last one resumes it. The Android back
  button opens the pause menu, and from it leaves.
- The game behind a menu **stays alive, paused** (§3.2); a `restart`
  makes it again when the game asks.
- **Rebinding controls** in a settings page, so the review's held
  `InputAction` on rebind is fixed here, with a test.

*Done when* a game with all of these is written from the guide with no
workaround, and its widget tests drive it.

### Phase 5: several views

- One GPU device per process, and one set of resources per world shared
  by its views (R13), measured first on the Note10+ (view creation, hot
  restart).
- Two views of one world update it once a frame: a minimap, a
  picture-in-picture, split screen; each with its own camera
  (`CameraView`).
- A widget that reads physics doesn't wait for its step: decided here
  (a copy that never waits, or the rule documented as it is).

*Done when* the lab shows a minimap over a game and a split screen,
holding stage 6's frame rates on the three devices.

Done so far (PRs #95, #96):

- One device for the app, one set of meshes and textures per world.
  Creating a view on the iPhone: 54–64 ms → 34–54 ms on screen; the Mac
  and the first view unchanged.
- `FynkelView(camera:, input:, worldWidgets:)`. The world updates once
  a frame however many views show it, and pointers aim from the camera
  of the view that takes input (`Input.camera`).
- Physics read by a widget: **the rule stays as documented**. Views and
  world widgets read the objects' transforms, which never wait for the
  step (a lab test checks that the step is still running after each
  frame with both). A widget that reads a body as it builds does wait,
  so the guide says to read it in `onUpdate` and hand the widget the
  number. A copy that never waits would be a step old, and give a
  second answer to "where is it".
- The lab's minimap and split screen: 120 fps in every view on the Mac.
  On the iPhone, the minimap holds 120 and the split screen holds 120
  with short dips to 107–118.
- The Note10+ (2026-10-14): the minimap and split screen hold its 60 fps
  in every view, and the arena's 100 bars hold 60 too (the report).

### Phase 6: the leftovers

- Camera shake that doesn't drift a camera moved during it: the shake
  is added when the frame is drawn, not written into the camera.
- `near ≥ far` reported, not drawn as nothing (checked when the camera
  draws, so setting `near` then `far` stays possible).
- The extract's work for every object before culling (the scale
  example at 10,000 objects says how much it's worth).

*Done when* each has a test, and the scale numbers are no worse.

Done (PR #97):

- **Shake** goes to the engine as an offset (`fy_camera_shake_set`, ABI
  20) and moves the camera only where it's drawn; the camera's transform
  is the game's alone. `world.cameras.shakeOf(camera)` says how far, and
  world widgets move with it. A camera moved by `position += step` during
  a shake ends exactly at the sum of its steps (the old code was 0.18 off
  after one second).
- **near ≥ far**: the extract refuses the frame, naming the camera, and
  the view's `frame()` throws an `ArgumentError`; a `FynkelView` reports
  it once a second and the game goes on. Checked only when drawn, so
  setting `near` then `far` works.
- **The extract's work for every object: left as it is.** The probe
  (`extract_probe`, release, Mac) takes 0.23 ms for 10,000 objects in
  trees and 0.11 ms when the camera draws none of them, so culling in the
  extract could save 0.11 ms at the very most, of an 8.3 ms frame, and
  would add the sphere test to the UI thread, the busy one on the
  Note10+. Its new row ("nothing drawn") keeps the number measured.
- Scale, 10,000 objects, Mac, 60 s: `main` 112.1 fps median (min 65.5),
  this PR 120.0 (min 27.2: one window); two earlier runs 120.0/44.9 and
  119.0/40.6. The spread is the Mac's, not the change's.

### Phase 7: a real game, and the report

- A new example that uses all of it: a title screen, settings (theme,
  controls), a HUD, health bars, pause, a minimap. Measured on the Mac,
  iPhone and Note10+.
- A fresh agent writes a game from the menus brief again, from the guide
  only, to check the guide.
- You play it.
- The stage 7 report.

Done so far (PR #98):

- **Sheepdog** (`examples/sheepdog`): herd eight sheep into the pen
  before the clock runs out, keeping the wolves off them. It has a title screen; a game page with a
  HUD, a Bark button and a minimap; a pause menu (button, Escape, P,
  back); and settings for light/dark/system, the minimap, and every key
  rebound. Each sheep has a name tag and each wolf a bar for its nerve
  (`WorldWidget`s), and a pop-up ends the round with its stars. Its
  colours come from one palette: the light theme is a day in the field,
  the dark one a night. 29 tests: 20 that play the game frame by frame,
  9 that click through the app.
- **Your first play** found it worked but wasn't fun: hard and unfair.
  The sheep bolted alone from the dog, the wolves were faster than the
  sheep (made so for a test), took three barks, didn't fear the dog, and
  came in endless waves, the first after 4 s. Reworked:
  - The flock keeps together and follows its neighbours. Near the dog it
    walks away at a pace set by how near the dog is, and settles when it
    backs off. A wider gate (9 m), and sheep at it are drawn in.
  - Wolves are slower than a running sheep but lunge when close, and a
    frightened flock scatters (otherwise the wolves herded it home). A
    wolf keeps away from the dog and goes round it; near the dog its
    nerve wears down. One bark sends it off.
  - Rounds have levels: level 1 has no wolves; level n has 2(n−1), up to
    8, one at a time. The first comes after 15 quiet seconds, and each
    is announced by a red mark (seen on the minimap too) 3 s before it
    arrives. A clock (90 s, then 120 s) ends the round, and stars count
    the sheep home: one for 4, two for 6, three for 8. A star opens the
    next level.
  - Two tests check that it's fun, or at least fair. A scripted dog that
    only walks behind the flock brings all eight home in every round
    tried (levels 1–5, 8 seeds each); the test asks for six or more,
    levels 1–4. With no dog, the wolves take sheep in every round, and
    most level-3 rounds end with no star.
- Measured with its HUD and minimap, 60 s, nobody playing: **Mac 120 fps**
  (min 119, no slow frame); **iPhone 120 fps** (min 114, no slow frame,
  267 MB). After the rework, at level 3 with its wolves: **Mac 120 fps**
  (min 90.2 in one window, no slow frame, 401 MB); **iPhone 120 fps**
  (min 119, no slow frame, 268 MB); **Note10+ 60 fps**, its screen's
  rate (min 57.9, 117 MB).
- What it found: a second `FynkelView(root:)` of a root another view had
  added threw "already added" outside a `FynkelGame`. Now it shows that
  world and leaves it to the view that made it (a test). The first
  measure missed the minimap because of it, and was taken again.
- **The fresh agent** wrote the menus brief again from the guide alone
  (Starfall, 672 / 190 lines, 6 tests, analyzes, tests and builds). It hit
  no blocker, one confusing thing and five minor ones (mostly its own
  slips). The confusing one: the guide sent it to the arena for the "press
  a key" dialog, which it couldn't read. The guide now has that dialog and
  a controls row (§15). Pause under every page, the theme in the game,
  the minimap and the world widgets all worked first time, it says, and
  "the §15 snippets composed into the whole brief almost line for line".
  Its log is kept outside the repository, in
  `~/Documents/fynkel-s7/menus-LOG.md`.
- **The report:** [stage-7-report.md](stage-7-report.md).

## 3. Decisions (2026-10-13)

1. **The name clash.** Fynkel keeps `Color`, `Material` and
   `Transform` (Unity's names); `fynkel_flutter` gives conversions, and a
   file using both imports one with a prefix. *Recommended, accepted.*
   (The other way, renaming Fynkel's, would change every game and the
   guide.)
2. **The game behind a menu** stays alive and paused, as Unity's does;
   `restart` makes it again when wanted. *Recommended, accepted.*

## 4. Checklist

- [x] Phase 1: groundwork (PR #90)
- [x] Phase 2: one look for the app and the game (PR #91)
- [x] Phase 3: widgets in the world (PR #92)
- [x] Phase 4: menus, pop-ups and navigation (PRs #93, #94)
- [x] Phase 5: several views (PRs #95, #96)
- [x] Phase 6: the leftovers (PR #97)
- [x] Phase 7: a real game, and the report (PR #98)
