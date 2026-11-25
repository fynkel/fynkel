---
title: "Your first game: Roll"
description: Roll a ball around a walled floor and take five gems. Objects, components, physics, input, a camera that keeps out of walls, a Flutter HUD and a test.
---

You'll make **Roll**: a ball you roll with the keys, five gems to take, and
a camera that follows the ball without going through the walls. It takes
about half an hour, and it touches most of what a Fynkel game is made of.
The finished game is [`examples/roll`](https://github.com/mooumari/fynkel/tree/main/examples/roll),
and the code below is that game's, file by file.

![Roll: an orange ball on a stone floor between walls and pillars, blue gems around it](/tutorials/roll-start.png)

## 1. A new app

You need Flutter and Rust; [Install and first game](/start/install/) says
how. Make the app beside your clone of Fynkel:

```sh
flutter create --platforms macos,ios,android --empty roll
cd roll
```

Add Fynkel's packages to `pubspec.yaml`. Physics is a package of its own,
and Roll needs it for the ball:

```yaml
dependencies:
  flutter:
    sdk: flutter
  fynkel:
    path: ../fynkel/packages/fynkel
  fynkel_flutter:
    path: ../fynkel/packages/fynkel_flutter
  fynkel_physics:
    path: ../fynkel/packages/fynkel_physics

flutter:
  uses-material-design: true
```

(For Android, also do the two small changes in
[§1 of the guide](/guide/01-a-new-game/): `minSdk = 30` and a
`FynkelActivity`.)

## 2. The level: objects with components

Everything in a Fynkel game is a **`GameObject`**: it has a `Transform`
(where it is, how it's turned, how big) and **components** that give it
its parts. A `MeshRenderer` draws a mesh in a material; a `BoxCollider`
makes it solid to physics. The collider takes the object's scale, so one
`BoxCollider()` fits a box of any size.

`level()` makes the floor, four walls and two pillars as children of one
object:

<!-- include: examples/roll/lib/level.dart -->

## 3. The ball: a rigidbody pushed by the keys

A **`Rigidbody`** puts an object under physics: gravity pulls the ball
down, the floor holds it up, and it rolls when pushed. `Ball` is a
`GameObject` subclass, so it can have code of its own, as a Unity
`MonoBehaviour` would:

- `onFixedUpdate` runs at physics' own steady rate, the place to push a body.
- `input.axis2('move')` is W, A, S, D and the arrows (and a gamepad's
  stick) as one value from −1 to 1 each way.
- The push goes the way the **camera** looks, flattened onto the floor,
  so "up" always rolls away from you however the camera has turned.
- `interpolation` smooths what's drawn between physics steps.

<!-- include: examples/roll/lib/ball.dart -->

## 4. Gems: triggers

A collider with `isTrigger: true` isn't solid: things pass through it,
and it says so. Mixing **`Triggers`** into the object gives it
`onTriggerEnter`, called when another collider comes in. The gem checks
it's the ball, tells the game, and destroys itself. `onUpdate` runs every
frame; `time.delta` is the frame's seconds, so the gem spins at the same
speed at any frame rate.

<!-- include: examples/roll/lib/gem.dart -->

## 5. The game: putting it together

The game's **root** is one more `GameObject`, whose `onLoad` adds the
rest:

- **A sun.** A new world already has a daylight sky, which lights the
  scene; the sun adds direct light and shadows.
- **The camera.** A `Camera`, an **`Orbit`** that circles the ball
  (drag to turn it, scroll to zoom), and **`CameraCollision`**, which
  brings the camera in front of a wall or pillar that would come between
  it and the ball.
- **`gemsLeft`**, a Flutter `ValueNotifier`, which the HUD will listen to.

<!-- include: examples/roll/lib/game.dart -->

## 6. The HUD: Flutter over the game

The HUD is an ordinary Flutter widget. It listens to `gemsLeft` and
redraws when it changes:

<!-- include: examples/roll/lib/hud.dart -->

**`FynkelApp`** runs the game full screen with the HUD over it:

<!-- include: examples/roll/lib/main.dart -->

Run it:

```sh
flutter run -d macos
```

The first run builds the engine, which takes a minute or two; later runs
start in seconds. Roll the ball with W, A, S, D, drag to turn the camera,
and roll behind a pillar to watch the camera come in front of it.

## 7. A test

A Fynkel game runs in `flutter test` with no window, frame by frame:
`world.run(seconds)` runs that many seconds of frames, and
`world.input.simulate` presses keys. These tests check that the ball
lands, that W rolls it away from the camera, and that a gem counts. The
last one saves the picture at the top of this page:

<!-- include: examples/roll/test/roll_test.dart -->

```sh
flutter test
```

## Where next

- Make it yours: a timer, a second level, a jump (`body.addForce(Vec3(0, 5, 0), mode: ForceMode.impulse)`
  in `onUpdate` when `input.action('jump').wasPressed`).
- [The guide](/guide/) goes through each part properly, from
  [objects](/guide/03-objects/) to [physics](/guide/16-physics-colliders-rigidbodies-queries-and-callbacks/)
  and [testing](/guide/19-testing-a-game/).
- If you know Unity, [Coming from Unity](/guide/22-from-unity/) maps one to the
  other.
