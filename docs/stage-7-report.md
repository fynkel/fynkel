# Stage 7 report: Flutter and the game

The last part of phase 7 of [the stage 7 plan](stage-7.md). Phases 1 to 6
are merged (#90–#97); phase 7 is PR #98.

## The result

Stage 7's goal was a game that feels like an app. That means a main menu,
settings for colours and controls, and a game that pauses under every
dialog. It also means a HUD and labels that follow things in the world,
the app's theme in the game, and a minimap. All of it had to be simple
to write, in the guide, and tested. **That is done.** Sheepdog
(`examples/sheepdog`) has all of it in five short files of plain Fynkel
and plain Flutter, with no pause code and no workaround:

- `game.dart`: the game;
- `app.dart`: the app around it;
- `hud.dart`: what's over it;
- `labels.dart`: the widgets in the world;
- `palette.dart`: one colour the themes grow from.

It holds 120 fps on the Mac and the iPhone with its HUD, labels and
minimap.

Two checks matter most here.

**A fresh agent rebuilt the menus brief from the guide alone.** In stage
6 this brief was the one with the most friction. This time it hit no
blocker and one confusing thing, now fixed. In its words, pause under
every page, the theme in the game, the minimap and the world widgets
"all worked first time", and "the §15 snippets composed into the whole
brief almost line for line".

**You played Sheepdog.** The app side worked. The game itself was hard
and unfair, and that was the game's rules, not the framework. One cause
was mine: I made the wolves faster than the sheep so that a test could
pass. The rework needed nothing from the framework. Flocking, wolves that
fear the dog, levels, a clock, stars and "fun-check" tests all went into
the same five files.

## What stage 7 did

| Phase | What | PR |
|---|---|---|
| 1 | Groundwork: `game.world` from the start, pause to listen to, pools to walk, bindings only Flutter feeds, `HeadlessView.png()` | #90 |
| 2 | One look: colours both ways (`toFynkel`/`toFlutter`); `world.theme`, `onThemeChanged`, `isDark` | #91 |
| 3 | `WorldWidget`: a widget at a point in the world | #92 |
| 4 | Menus pause the game (`pauseWhenCovered`); rebinding controls, the held key fixed | #93, #94 |
| 5 | One GPU device per app, one asset set per world; several views of one world (`FynkelView(camera:, input:, worldWidgets:)`) | #95, #96 |
| 6 | Shake as an engine offset (ABI 20); near ≥ far reported; the extract left as it is, measured | #97 |
| 7 | Sheepdog; the fresh agent; your play and the rework; this report | #98 |

Each phase added its lab scenes: beside the game, the theme, widgets in
the world, menus, rebinding, and the minimap and split screen. The guide has a new §15
(Flutter and the game) and stage 7 notes in §21.

The tests grew with the work. fynkel_flutter went from 43 tests to 65,
the lab has 76 and the arena 16. Sheepdog has 29 (20 that play the game,
9 that click through the app) and is in CI.

## Stage 6's carry-overs

| From stage 6 | Now |
|---|---|
| F6: one colour for the theme and the game | Done (phase 2). The names stay; conversions both ways, and a prefixed import in the guide |
| F33: a widget at a world point | Done (phase 3) |
| R10: the app's pause | Done (phase 4): a page or dialog over the game pauses it, and closing the last one goes on |
| 37: `game.world` in the builder; 34: `FynkelGame.of` | Done (phase 1) |
| 38: `world.paused` to listen to | Done (phase 1) |
| 39–43: pools, unbound axes, a picture from `HeadlessView`, the guide's papercuts | Done (phase 1) |
| R13: one GPU device; two views updating a world once | Done (phase 5). On the iPhone, creating a view went from 54–64 ms to 34–54 ms |
| A widget reading physics waits for its step | Decided (phase 5): the rule stays, documented. Views and world widgets never wait |
| The review's 9: shake drift, a held action on rebind, two views, near ≥ far | All done (phases 4, 5, 6) |
| The extract's work before culling | Measured and left (phase 6): 0.11 ms at most, of 8.3 |

Nothing from stage 6 is left open in stage 7's scope.

## Measured

| | Mac | iPhone | Note10+ |
|---|---|---|---|
| Phase 3: 100 enemies with health bars | 120 fps | 120 fps | 60 fps (min 57.9; the same without bars) |
| Phase 5: a minimap over a game | 120 fps | 120 fps | 60 fps in both views (the minimap's min 58.9) |
| Phase 5: split screen | 120 fps | 120, with short dips to 107–118 | 60 fps in both views (min 59.9) |
| Phase 7: Sheepdog, level 3, HUD and minimap | 120 fps (min 90.2 in one second, no slow frame) | 120 fps (min 119) | 60 fps (min 57.9) |

Each was measured for 60 s, alone, under `caffeinate`. The arena's Dart
garbage is 40–44 KB a frame without bars and 81–102 KB with 100 of them.
Sheepdog's is 17 KB (Mac) and 43 KB (iPhone).

**The Note10+** was measured on 2026-10-14, after #98. Its screen
shows 60 frames a second, so 60 is its full rate, and every row holds
it. It does drop a frame now and then: 1.6–3.4% of frames take longer than
16.7 ms to draw, in Sheepdog and in the arena alike, with bars or
without. So that comes from the phone, not from the world widgets or a
second view. In the arena the 100 bars change the frame rate by nothing;
they add Dart garbage (41 KB a frame without them, 118 KB with them).
Sheepdog makes 46 KB a frame and uses 106–117 MB. Its first frame is on
screen 1.5 s after the app starts, as is the arena's (1.3 s); the
iPhone's is about 0.1 s. The lab's views were read from its own rates
(`LAB_RATES`), 30 one-second windows each.

## What the real game found

Writing one game with everything is what found these:

- **A bug.** A second `FynkelView(root:)` of a root that another view had
  already added threw "already added" outside a `FynkelGame`. That is the
  minimap in fynkel_measure's overlay. Fixed in #98, with a test. Now it
  shows that world and leaves it to the view that made it.
- **A gotcha.** `flutter create --empty` leaves out
  `uses-material-design`, so every icon shows as "?". This is worth a
  line in the guide's "a new game" steps.
- **A test idea worth keeping.** "Is it fair" can be a test: a scripted
  player that only does the obvious thing should win, and doing nothing
  should lose. Sheepdog's two "is fun" tests found a real flaw that I
  didn't see by reading the code. The wolves were herding the flock home
  better than no dog at all. This could become a short guide section.
- **Tuning is still yours.** The scripted dog brings all eight home even
  at level 5, so the higher levels may be too easy. That is game design,
  for when you play it more, not stage 7 work.

## What goes to stage 8

Stage 8 is the look, as planned: shadows, point and spot lights,
transparency, animated characters (R12, R14). From stage 7, only:

- the `uses-material-design` line and, if you want it, the "is it fair"
  test pattern in the guide.

## Read

Phase 7 is done when you have read this.

- [x] Read
- [x] The Note10+ numbers in (phases 3, 5, 7)

Then **stage 7 is done**.
