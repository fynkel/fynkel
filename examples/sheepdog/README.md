# sheepdog

Stage 7's game ([docs/stage-7.md](../../docs/stage-7.md), phase 7): a game
that feels like an app. You're the dog; bring the eight sheep into the pen
at the back of the field, through its gate, before the wolves take them.

- Move with W, A, S, D or the arrow keys, or drag anywhere as a stick.
- Space, a tap, or the Bark button barks: the sheep near you run from it,
  and a wolf near you is hurt and pushed back. Three barks send a wolf off.
- The sheep run from you and from the wolves, and the wolves are a little
  faster than they are: step between them.
- Escape, P, the pause button or Android's back pauses.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d <android>       # Android 11 or later, 64-bit
```

Each file shows one side of it:

- `lib/game.dart` is the game, plain Fynkel. The flock, the wolves and the
  dog are objects with `onUpdate`; `Health` is a behaviour with signals;
  the waves are a routine; the round tells the HUD through
  `ValueNotifier`s, and the end through `result`. `wear` takes the game's
  colours from the app's theme, and again when it changes: the light theme
  is a day in the field, the dark one a night.
- `lib/palette.dart` is the palette, written once: one colour the themes
  grow from.
- `lib/app.dart` is the app around it, plain Flutter: a title screen, the
  game's page, a pause menu, a settings page (light or dark, the minimap,
  the keys) and the pop-up at the end of a round. Every page and dialog
  over the game pauses it, and closing the last one goes on, with no pause
  code.
- `lib/hud.dart` is what's over the game: the counts, the pause and bark
  buttons, and the minimap, a second `FynkelView` of the same world from a
  camera high above it.
- `lib/labels.dart` has the widgets in the world: each sheep's name, and
  each wolf's health bar, placed by `WorldWidget`s.

`test/game_test.dart` plays the game without Flutter, a frame at a time
(`SHEEPDOG_SHOT=<dir>` saves pictures of the day and the night there);
`test/app_test.dart` clicks through the app.

## Measuring

package:fynkel_measure's tool runs the profile build of `lib/measured.dart`:
the game with its HUD and minimap, nobody playing.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--ios <device>]
caffeinate -du dart run fynkel_measure:measure lifecycle --cycles 5
```
