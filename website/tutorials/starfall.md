---
title: "A phone game: Starfall"
description: A portrait game for phones. Catch falling stars with your finger, keep away from rocks. Touch, pools, particles, routines, Flutter screens and a saved best score.
---

Stars and rocks fall through the night. Hold your finger on the screen and
the basket goes to it: catch a star for a point, miss one and lose a life,
catch a rock and lose one too. After three, the game is over, and the best
score is kept for next time. The finished game is
[`examples/starfall`](https://github.com/fynkel/fynkel/tree/main/examples/starfall).

![Starfall: a blue basket on dark ground under a night sky with a moon, golden stars falling](/tutorials/starfall.png)

This tutorial goes faster than [Roll](/tutorials/roll/): it takes the
objects, components and `onUpdate` from there as known, and spends its time
on what a phone game needs.

## 1. A portrait app

Make the app as in Roll, with `fynkel`, `fynkel_flutter` and, to keep the
best score, `shared_preferences`. Starfall needs no physics: things fall
by their own code, and a catch is a distance check.

```yaml
dependencies:
  flutter:
    sdk: flutter
  fynkel:
    path: ../fynkel/packages/fynkel
  fynkel_flutter:
    path: ../fynkel/packages/fynkel_flutter
  shared_preferences: ^2.5.3
```

To keep it upright, list only `UIInterfaceOrientationPortrait` under
`UISupportedInterfaceOrientations` in `ios/Runner/Info.plist`, and add
`android:screenOrientation="portrait"` to the activity in
`android/app/src/main/AndroidManifest.xml`. On a Mac, a phone-shaped
window is handy for trying it: in `macos/Runner/MainFlutterWindow.swift`,
set `windowFrame.size = NSSize(width: 420, height: 760)` before the
window takes it.

## 2. The night

The scenery is plain objects that never move: a dark hill, a moon, and
ninety far stars. Objects that don't move cost next to nothing a frame.
`world.sky = null` turns off the daylight sky, so the camera's clear
colour is the night, lit only by the game's own lights.

<!-- include: examples/starfall/lib/night.dart -->

## 3. The basket: following a finger

`input.fingers` are the touches on the screen now, and the mouse; one
that `isPressed` is down. **`screenPointToPlane`** turns the point under
a finger into a point in the world, where the ray through it meets a
plane: here the upright plane the stars fall in, so the basket goes to
the same `x` as the finger. A, D and the arrow keys move it too
(`input.axis2('move')`), for playing on a Mac.

<!-- include: examples/starfall/lib/basket.dart -->

## 4. Falling things, from a pool

A long game drops hundreds of stars. Making a new object for each, and
destroying it after, works, but a **`Pool`** is the habit worth having
for anything made and thrown away often: shots, enemies, effects.
`pool.get(reset)` hands out a hidden object (or makes one when none is
waiting) after `reset` has set it up, and `release()` hides it again for
the next `get`.

A `Falling` is a star or a rock: the same object either way, with its
material and size set in `launch`, which the game calls in `get`'s
`reset`.

<!-- include: examples/starfall/lib/falling.dart -->

## 5. Sparks that go back by themselves

A catch makes a puff of sparks: a **`ParticleSystem`** with one `Burst`,
not looping. `StopAction.disable` turns its object off when the last
spark is gone, and one from a pool goes back to it then, so the game
never has to release it. `simulationSpace: ParticleSpace.world` leaves
sparks where they were made.

<!-- include: examples/starfall/lib/sparkle.dart -->

## 6. The game

The game object holds the state Flutter shows, as `ValueNotifier`s:
`phase` (the title, playing, or over), `score`, `lives` and `best`.

- **`start`** gives back anything still falling, resets the numbers and
  starts the drops.
- **`_drop` is a routine**: `startRoutine` runs it alongside the frames,
  and `await wait(seconds)` waits that long in game time. It drops
  something, waits less as the score grows, and stops when the game is
  over. A loop in a routine reads as what it does, with no timer fields.
- **`onUpdate`** checks each falling thing against the basket. It walks
  the pool's handed-out objects with `activeAt` from the end, so it can
  release them as it goes, and makes no list each frame.
- **`punchScale`** gives the basket a little bounce on a catch: a tween
  that springs back by itself.

<!-- include: examples/starfall/lib/game.dart -->

## 7. Screens in Flutter

The title, the score and lives, and the game over panel are Flutter
widgets over the game. One `ValueListenableBuilder` on `phase` picks
which; the HUD rebuilds only when the score or lives change. The buttons
call the game's `start` directly: the overlay has the game.

<!-- include: examples/starfall/lib/screens.dart -->

## 8. Keeping the best score

One number to keep needs no more than `shared_preferences`. `main` reads
it before the app starts, hands it to the game, and saves it whenever the
game's `best` changes:

<!-- include: examples/starfall/lib/main.dart -->

(For a save with more in it, a level and what was found, write a file:
[§18 of the guide](/guide/18-hot-reload-and-starting-over/) shows how.)

Run it on a phone or the Mac:

```sh
flutter run
```

## 9. Tests

The tests run the game with no window, in a phone-sized view, so a
finger's point on the screen lands where it would on a phone.
`input.simulate.fingerDown` and `fingerMove` play the finger. The last
test but one plays a minute and checks the pools stopped growing: every
star, rock and spark came back.

<!-- include: examples/starfall/test/starfall_test.dart -->

## Where next

- Make it yours: a star worth five that falls faster, a shield that saves
  one life, a sound on each catch ([§13b](/guide/13b-sound/)).
- [Particles](/guide/13c-particles/) has much more: trails, colour by
  speed, collisions.
- [Speed](/guide/20-speed/) says what to measure on a phone, and how.
