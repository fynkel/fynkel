# hello

The smallest game: a lit, turning cube, written in Dart, and the code of
[the guide's §2](../../docs/writing-a-game.md#2-hello). `lib/main.dart` is a
root object (`Hello`) with a camera, a sun and a cube that turns itself,
shown by `runApp(FynkelApp(Hello.new))`. It began as stage 1's app
([docs/stage-1.md](../../docs/stage-1.md)).

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d emulator-5554   # Android 11 or later, with the NDK installed
flutter build apk
```

On Android the engine builds for 64-bit devices only (`arm64-v8a` and
`x86_64`). `flutter build apk` builds 32-bit arm too, without the engine:
on such a device the game stops at once with an error that says so.

## Measuring

package:fynkel_measure's tool (`packages/fynkel_measure/bin/measure.dart`)
builds and runs the profile build of `tool/measured.dart`, the same game
wrapped in `MeasuredApp`, and prints the numbers recorded in
docs/stage-1.md §7. Run it from this directory, and keep the display awake
while it runs. The top-left line shows what the view did in the last
second: `fresh` is the new frames you saw, `ticks` the frame clock, then
the engine's counts.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--fullscreen]
caffeinate -du dart run fynkel_measure:measure lifecycle [--cycles 10]
```

On a phone or the emulator, add `--ios <device>` or `--android <device>`;
the tool starts the profile build there itself. `lifecycle` destroys the
game's root and adds a new one, shows a new view, and resizes it, with the
memory after each. `--baseline` runs either check with a spinning Flutter
box in place of the engine's view.
