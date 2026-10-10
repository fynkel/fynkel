# coins

The second stage 3 game ([docs/stage-3.md](../../docs/stage-3.md) §3.7,
step 6): walk about picking up coins. Tap (or click) anywhere to walk
there. The last coin of a level takes you to the next, a meadow and then
a field at dusk, and round again. The models are Kenney's
([assets/CREDITS.md](assets/CREDITS.md)).

The game is `lib/coins.dart`, and each part shows one way of working:

- `CoinsGame`, the root, keeps the `Player` and the `FollowCamera` and
  changes the `Level` under them (`nextLevel`): the player outlives every
  level.
- A `Level` adds its light, ground, props, coins and `Swarm`, and hands
  the player what it carries here (`player.carry`). A tap on anything in
  it bubbles up to the level, which walks the player there.
- A `Coin` spins and bobs through two behaviours, `Spin` and `Bob`.
- The player's `Collector` is a behaviour, kept apart from the walking:
  it finds the coins by type (`world.all<Coin>()`), picks up those near,
  and asks for the next level after the last.
- The player's `score` is a `ValueNotifier`, which `lib/hud.dart` shows
  over the view with a `ValueListenableBuilder`. `lib/main.dart` is one
  line: `FynkelApp(CoinsGame.new, overlay: (context, game) => Hud(...))`.
- A `Swarm` moves 300 plain objects, motes with only a mesh, in its own
  `onUpdate`.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d emulator-5554   # Android 11 or later, with the NDK installed
```

## Measuring

As the garden's ([../garden/README.md](../garden/README.md)): the tool
runs `lib/measured.dart`, the same game and HUD in `MeasuredApp`. Its
lifecycle check first changes the level back and forth (`changed`), then
destroys and re-adds the whole game, shows a new view, and resizes it.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--fullscreen]
caffeinate -du dart run fynkel_measure:measure lifecycle [--cycles 10]
```
