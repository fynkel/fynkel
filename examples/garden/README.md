# garden

The stage 2 acceptance app ([docs/stage-2.md](../../docs/stage-2.md)),
rewritten as objects in stage 3: a character in a garden. Drag to go
around it, scroll (or two fingers on a trackpad) to zoom, and tap the
character to turn its head. The models are Kenney's
([assets/CREDITS.md](assets/CREDITS.md)).

The game is `lib/garden.dart`: `Garden`, the root, adds an `OrbitCamera`,
the lights, the `Ground`, the `Props` and the `Character`, and each of
those loads what it needs from `assets/`. `lib/main.dart` is
`runApp(FynkelApp(Garden()))`.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d emulator-5554   # Android 11 or later, with the NDK installed
```

## Measuring

As hello's, with package:fynkel_measure's tool, from this directory. It
builds and runs `lib/measured.dart`, the same game in `MeasuredApp`. Keep
the display awake while it runs.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--fullscreen]
caffeinate -du dart run fynkel_measure:measure lifecycle [--cycles 10]
caffeinate -du dart run fynkel_measure:measure bench
```

`lifecycle` destroys the whole garden and adds a new one, shows a new
view, and resizes it, with the memory after each. `--ios <device>`,
`--android <device>` and `--baseline` work as for hello. `bench` runs
package:fynkel_measure's timings in the app (`world.update` with 10,000
objects in trees, the world's extract with 10,000 objects in trees,
raycasts over 10,000 cubes) and the garden's tap, and prints them. How
long the garden took to load and to reach the screen is printed at the
end of each run.
