---
title: Troubleshooting
description: What goes wrong most, and what to do about it.
---

Start with `fynkel doctor`, in the game's folder: it checks your Mac and the
game, and says what to do. If what you see isn't here, [open an
issue](https://github.com/fynkel/fynkel/issues).

## Installing and building

### `fynkel: command not found`

Dart's commands aren't on your PATH. Add this to `~/.zshrc`, and open a new
terminal:

```sh
export PATH="$PATH:$HOME/.pub-cache/bin"
```

### `fynkel` doesn't know `create`, or asks for `--root`

Another program called `fynkel` is first on the PATH. `which fynkel` says
which; the one from `dart pub global activate fynkel_cli` is in
`~/.pub-cache/bin`. Put that first, or run `dart pub global run
fynkel_cli:fynkel create my_game`.

### Errors about named parameters that look right

Fynkel needs Dart 3.13, and an older `dart` found first on the PATH (a
Homebrew one) reports errors that aren't there. Run `flutter analyze` and
`flutter test`, or put Flutter's `bin` folder first on the PATH.

### `minSdkVersion 24 cannot be smaller than version 30`

Fynkel needs Android 11. Set `minSdk = 30` in
`android/app/build.gradle.kts` ([Adding Fynkel to an
app](/start/existing-app/#android-11-or-later)). `fynkel create` does it for
you.

### The engine doesn't download

The first build for each platform gets the engine from GitHub. Behind a
proxy set `HTTPS_PROXY`; where GitHub is blocked, [use a
mirror](/start/existing-app/#where-github-is-blocked).

### An error about 32-bit Android

Fynkel runs on 64-bit devices only. Build the bundle for those: [Android
release](/release/android/).

## Nothing on screen

### The view is one flat colour

- **Is there a camera?** Something has to `add` a `GameObject` with a
  `Camera()`.
- **Does it look the right way?** A camera looks down its −Z. The easy way is
  `Transform.lookAt(eye, target)`.
- **Is the object where you think?** `debug.sphere(point, 0.5)` draws a
  marker there ([Layers and debug drawing](/graphics/layers-and-debug/)).
- **Is it still loading?** An object appears when its `onLoad` has finished,
  with everything added in it.

### The game stands still on a Mac

macOS stops drawing a window that's covered by others. Bring it to the
front: it hasn't hung.

### A file isn't found

Every folder of files is listed in `pubspec.yaml`, and Flutter doesn't look
in folders below the ones listed:

```yaml
flutter:
  assets:
    - assets/
    - assets/models/
```

After changing it, stop the app and run it again: a hot reload isn't enough.

### Icons show as boxes

Add `uses-material-design: true` under `flutter:` in `pubspec.yaml`.

### On a phone, what's painted under the game doesn't show

There the game is drawn under Flutter's own picture. [On a
phone](/flutter/hud/#on-a-phone) says what that changes, and
`directToScreen: false` what to do.

## In the game

### My change doesn't show after a hot reload

A reload changes methods; it doesn't run `onLoad` again. Press **F5** to
start the game over with the new code ([Hot reload](/making/hot-reload/)).

### `Color`, `Material` or `Transform` is "defined in two libraries"

Flutter and Fynkel both have one. Keep widgets and game objects in files of
their own, or hide Flutter's:

```text
import 'package:flutter/material.dart' hide Color, Material, Transform;
```

### `wait(...)` throws

It works only in a routine: start one with `startRoutine(() async { … })`
([Routines](/basics/routines/)).

### Things fall through the floor

- The floor needs a collider: a `MeshRenderer` is only a picture.
- A small, fast body needs `CollisionDetectionMode.continuous`.
- A body moved by setting its `position` teleports: use forces, or
  `movePosition` ([Rigidbodies](/physics/rigidbodies/#moving-bodies)).

### `onTriggerEnter` is never called

The class needs `with Triggers`, one of the colliders `isTrigger: true`, and
one side a rigidbody (kinematic will do) or a character controller
([Triggers and collisions](/physics/callbacks/)).

### There's no sound

- The iPhone's silent switch mutes games ([Playing
  sounds](/sound/playing/#the-silent-switch-calls-and-the-mic)).
- A sound 3D in the world needs a camera near it to hear it.
- `audio.outputError` says whether the device refused.

### It's slow

Run it with `flutter run --profile`: a debug build runs Dart several times
slower, and never on battery saver. Then [Measuring](/making/measuring/).

## In tests

### `pumpAndSettle` never ends

A game draws a frame forever, so nothing settles. Pump a number of frames
instead ([Testing the whole app](/making/testing-the-app/)).
