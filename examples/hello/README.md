# hello

The stage 1 acceptance app ([docs/stage-1.md](../../docs/stage-1.md)): a
lit, spinning cube, written in Dart. The game is `lib/game.dart`, which
imports only `package:fynkel`; `lib/main.dart` is the Flutter side.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d emulator-5554   # Android 11 or later, with the NDK installed
flutter build apk --target-platform android-arm64,android-x64
```

On Android the engine builds for 64-bit devices only (`arm64-v8a` and
`x86_64`). `flutter build apk` adds 32-bit arm unless told otherwise.

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

On a phone or the emulator, add `--ios <device>` or `--android <device>`;
the tool starts the profile build there itself. `--baseline` runs either
check with a spinning Flutter box in place of the engine's view.
