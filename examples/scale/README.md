# scale

Stage 6's scale check ([docs/stage-6.md](../../docs/stage-6.md), step 5): a
world of 10,000 objects of 50 kinds, busy the way a game is. Groups of ten
(a block and nine parts) stand in a square, and each group is one of 50
kinds: a mesh, a colour and one of five ways to behave (still, spin, bob,
beat to a signal, or blink as a pool does). The camera turns slowly around
them. `lib/main.dart` says more.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d <android>       # Android 11 or later, 64-bit
```

Its settings are `--dart-define`s:

- `OBJECTS`, how many objects there are (10000);
- `CHURN`, how many groups a second are destroyed and made anew (0), for
  the long runs;
- `ONLY=still|spin|bob|beat|blink`, one behaviour for every kind;
- `SHAPE=cube`, cubes in place of the spheres;
- `AWAY=true`, the camera turned away: everything is still drawn, but no
  pixel is filled.

## Measuring

package:fynkel_measure's tool runs the profile build of `lib/measured.dart`,
the same world in `MeasuredApp`. Run it from this directory, and keep the
display awake while it runs. The tool's `--define` passes the settings above;
`--define FRAME_TIMES=true` also prints, once a second, how long each
`world.update` took.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--define OBJECTS=2000]
caffeinate -du dart run fynkel_measure:measure bench
```

On a phone, add `--ios <device>` or `--android <device>`. The numbers are in
docs/stage-6-scale.md.
