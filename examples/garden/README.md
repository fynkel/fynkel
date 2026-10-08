# garden

The stage 2 acceptance app ([docs/stage-2.md](../../docs/stage-2.md)): a
character in a garden. Drag to go around it, scroll (or two fingers on a
trackpad) to zoom, and tap the character to turn its head. The game is
`lib/garden.dart`; `lib/main.dart` is the Flutter side, which also times
the loads. The models are Kenney's ([assets/CREDITS.md](assets/CREDITS.md)).

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d emulator-5554   # Android 11 or later, with the NDK installed
```

The top line shows what the view did in the last second, as in hello;
the bottom line what the loads took: reading the asset, loading it (the
helper isolate, then the handles), spawning, and the time until the first
frame on screen has it.

## Measuring

As hello's, with package:fynkel_measure's tool, from this directory. Keep
the display awake while it runs.

```sh
flutter build macos --profile
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--fullscreen]
caffeinate -du dart run fynkel_measure:measure lifecycle [--cycles 10]
caffeinate -du dart run fynkel_measure:measure bench
```

`lifecycle` unloads the character and loads it again, freeing everything
it brought each time, with the memory after each. `--ios <device>`,
`--android <device>` and `--baseline` work as for hello. `bench` runs
`lib/bench.dart`'s timings in the app (`world.update` with 10,000
objects in trees, the world's extract with 10,000 objects in trees, raycasts over 10,000 cubes, the garden's tap) and
prints them. The loads' times are printed at the end of each run.
