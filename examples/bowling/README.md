# bowling

Ten-pin bowling on [fynkel_physics](../../packages/fynkel_physics): the
example for physics (made in stage 5,
[docs/stage-5.md](../../docs/stage-5.md) step 9). The ball waits on the
foul line: drag sideways to place it, and flick up to bowl it, as fast as
the flick and leaning as it leans (or the arrow keys and space). Ten
frames, scored as bowling is. The lane, the ball and the pins are their
real size and weight.

The game is `lib/bowling.dart`, and each part shows one way of working:

- `BowlingGame`, the root, racks the pins, bowls the ball (`roll`), and,
  in a routine, waits for the ball to reach the pit, then for the pins to
  settle, then counts them. It steps physics 100 times a second
  (`time.fixedDelta`): a rolling ball turns faster than physics turns a
  body at 50.
- The `Alley` is boxes with `BoxCollider`s: the lane (waxed, so the ball
  slides on it), the gutters, the walls and the pit. A `Gutter` and the
  `Pit` are triggers (`with Triggers`): the ball rolling into them tells
  the game.
- The `Bowler`, a behaviour, is the input: `onDragStart` tells a drag
  sideways (placing, with a raycast onto the lane) from one up (aiming,
  with a line on the lane), and `onSwipe` bowls. The finger's trail is
  drawn by the HUD.
- The `Ball` is a `Rigidbody` with continuous collision detection, set
  rolling as it starts (`linearVelocity`, `angularVelocity`), and
  interpolated: the camera follows it smoothly, and it's drawn between
  physics' steps, so it doesn't jitter.
- A `Pin` is a mesh made in code (`Mesh.custom`), a pin's profile turned
  about its axis, with a `MeshCollider` that is its hull, so it stands on
  its flat foot. It's down once it tips past 45°.
- `lib/score.dart` keeps the score, strikes and spares and the tenth
  frame's extra rolls, apart from the game: a `ChangeNotifier` that
  `lib/hud.dart` shows as a score sheet. The HUD also draws the finger's
  trail and, until the first ball, how to bowl; it ignores touches.
- The `ChaseCamera` follows the ball down the lane and stops short of the
  pins.

```sh
flutter run -d macos
flutter run -d <iphone>        # flutter devices lists them
flutter run -d emulator-5554   # Android 11 or later, with the NDK installed
```

## Measuring

As the garden's ([../garden/README.md](../garden/README.md)): the tool
runs `lib/measured.dart`, the same game and HUD in `MeasuredApp`. Its
lifecycle check first rolls a ball (`changed`), then destroys and re-adds
the whole game, shows a new view, and resizes it.

```sh
caffeinate -du dart run fynkel_measure:measure rate --seconds 60 [--fullscreen]
caffeinate -du dart run fynkel_measure:measure lifecycle [--cycles 10]
```
