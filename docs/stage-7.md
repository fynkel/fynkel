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

### Phase 6: the leftovers

- Camera shake that doesn't drift a camera moved during it: the shake
  is added when the frame is drawn, not written into the camera.
- `near ≥ far` reported, not drawn as nothing (checked when the camera
  draws, so setting `near` then `far` stays possible).
- The extract's work for every object before culling (the scale
  example at 10,000 objects says how much it's worth).

*Done when* each has a test, and the scale numbers are no worse.

### Phase 7: a real game, and the report

- A new example that uses all of it: a title screen, settings (theme,
  controls), a HUD, health bars, pause, a minimap. Measured on the Mac,
  iPhone and Note10+.
- A fresh agent writes a game from the menus brief again, from the guide
  only, to check the guide.
- You play it.
- The stage 7 report.

## 3. Decisions (2026-10-13)

1. **The name clash.** Fynkel keeps `Color`, `Material` and
   `Transform` (Unity's names); `fynkel_flutter` gives conversions, and a
   file using both imports one with a prefix. *Recommended, accepted.*
   (The other way, renaming Fynkel's, would change every game and the
   guide.)
2. **The game behind a menu** stays alive and paused, as Unity's does;
   `restart` makes it again when wanted. *Recommended, accepted.*

## 4. Checklist

- [ ] Phase 1: groundwork
- [ ] Phase 2: one look for the app and the game
- [ ] Phase 3: widgets in the world
- [ ] Phase 4: menus, pop-ups and navigation
- [ ] Phase 5: several views
- [ ] Phase 6: the leftovers
- [ ] Phase 7: a real game, and the report
