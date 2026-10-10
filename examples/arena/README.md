# arena

The stage 4 game ([docs/stage-4.md](../../docs/stage-4.md), step 9): a
top-down arena. Move with W, A, S, D or the arrow keys, or drag anywhere
as a stick. Swipe, tap where to go, or press space to dash: a dash goes
through enemies and hits them. The enemies come in waves, one more each
wave; some leave a heart. Escape, P or the button pauses; pinch or the
mouse wheel zooms.

The game is `lib/arena.dart`, and each part shows one way of working:

- `Health` and `Flash` are behaviours, shared by the player and the
  enemies: `Health` counts hits and has signals (`hurt`, `healed`,
  `died`); `Flash` listens to `hurt` and tweens the object's colour and
  scale.
- `Round._waves` is a routine: `wait`, add the enemies one by one, then
  `until` they're all gone, and again. Destroying the round stops it.
- The round listens to the player's and each enemy's signals for the
  score, the hearts, the camera shake and the end.
- Tweens do the rest of the motion: an enemy grows in (`scaleTo`) and
  shrinks away, a dash is a `moveTo`, sparks fly out (`moveBy`), a heart
  bobs for ever (`loops: -1, yoyo: true`).
- `ArenaCamera` is a `Follow` and a `LookAt`; a pinch changes the
  follow's offset, and a hit shakes it (`world.cameras.shake`).
- Pausing sets `time.scale` to 0: everything that moves uses
  `time.delta`, so everything stops.
- The sparks are on the `effects` layer, and `input.layers` leaves it
  out, so a tap goes through them to the floor.
- `ArenaGame` keeps what the HUD shows in `ValueNotifier`s, and
  `lib/hud.dart` shows them over the view in plain Flutter.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d emulator-5554   # Android 11 or later, with the NDK installed
```

## Measuring

As the garden's ([../garden/README.md](../garden/README.md)): the tool
runs `lib/measured.dart`, the same game and HUD in `MeasuredApp`. Its
lifecycle check first starts a new round again and again (`changed`),
then destroys and re-adds the whole game, shows a new view, and resizes
it.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--fullscreen]
caffeinate -du dart run fynkel_measure:measure lifecycle [--cycles 10]
```
