# sheepdog

Stage 7's game ([docs/stage-7.md](../../docs/stage-7.md), phase 7; its animals since
stage 9): a game
that feels like an app. You're the dog; bring the eight sheep into the pen
at the back of the field, through its gate, before the clock runs out.

- Move with W, A, S, D or the arrow keys, or drag anywhere as a stick.
- The flock keeps together, and walks away from you: slowly when you're
  at the edge of its reach, at a run when you're close. Walk behind it to
  drive it where you want; back off and it settles. Sheep at the gate
  are drawn in.
- Space, a tap, or the Bark button barks: the sheep near you run, and a
  wolf near you runs off.
- Level 1 has no wolves. From level 2 they come one at a time, the first
  after 15 quiet seconds, each announced by a red mark on its edge of the
  field (and the minimap) a few seconds before. A wolf is slower than a
  running sheep, but lunges when it gets close, and a frightened flock
  scatters. It keeps away from the dog and goes round; stay near it and
  its nerve (the bar over it) runs out, and it leaves.
- Stars at the end: one for four sheep home, two for six, three for all
  eight. A star or more opens the next level.
- Escape, P, the pause button or Android's back pauses.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d <android>       # Android 11 or later, 64-bit
```

Each file shows one side of it:

- `lib/game.dart` is the game, plain Fynkel. The flock, the wolves and the
  dog are objects with `onUpdate`; a wolf's nerve is `Health`, a
  behaviour with signals; the wolves' arrivals are a routine; the round
  tells the HUD through `ValueNotifier`s (the counts, the level, the clock,
  a wolf coming), and the end through `result`. `wear` takes the game's
  colours from the app's theme, and again when it changes: the light theme
  is a day in the field, the dark one a night.
  The dog and the sheep are animated models, a pug and a sheep from
  Quaternius's Farm Animals (CC0, [assets/CREDITS.md](assets/CREDITS.md)):
  each one's legs are a blend of its clips (idle, slow walk, walk, run) by
  its speed, and a sheep hops into the pen. The wolves are still shapes.
- `lib/palette.dart` is the palette, written once: one colour the themes
  grow from.
- `lib/app.dart` is the app around it, plain Flutter: a title screen, the
  game's page, a pause menu, a settings page (light or dark, the minimap,
  the keys) and the pop-up at the end of a round, with its stars. Every page and dialog
  over the game pauses it, and closing the last one goes on, with no pause
  code.
- `lib/hud.dart` is what's over the game: the counts, the level and the
  clock, a warning while a wolf's mark is out, the pause and bark buttons, and the minimap, a second `FynkelView` of the same world from a
  camera high above it.
- `lib/labels.dart` has the widgets in the world: each sheep's name, and
  each wolf's nerve, placed by `WorldWidget`s.

`test/game_test.dart` plays the game without Flutter, a frame at a time
(`SHEEPDOG_SHOT=<dir>` saves pictures of the day and the night there).
Two of its tests check that it's fun, or at least fair: a dog that only
walks behind the flock brings six or more home at every level, and with
no dog the wolves win.
`test/app_test.dart` clicks through the app.

## Measuring

package:fynkel_measure's tool runs the profile build of `tool/measured.dart`:
the game at level 3 with its HUD and minimap, nobody playing.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--ios <device>]
caffeinate -du dart run fynkel_measure:measure lifecycle --cycles 5
```
