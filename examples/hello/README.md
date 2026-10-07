# hello

The stage 1 acceptance app ([docs/stage-1.md](../../docs/stage-1.md)): a
lit, spinning cube, written in Dart. The game is `lib/game.dart`, which
imports only `package:fynkel`; `lib/main.dart` is the Flutter side.

```sh
flutter run -d macos
```

The top-left line shows what the view did in the last second: `fresh` is
the new frames you saw, `ticks` the frame clock, then the engine's counts.

## Measuring

`tool/measure.dart` runs the profile build and prints the numbers recorded
in docs/stage-1.md §7. Keep the display awake while it runs.

```sh
flutter build macos --profile
caffeinate -du dart tool/measure.dart rate --seconds 60 [--fullscreen]
caffeinate -du dart tool/measure.dart lifecycle [--cycles 10]
```
