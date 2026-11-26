---
title: "Adding Fynkel to an app"
description: "Into a Flutter app you have, Android set-up, mirrors, and a clone."
---

In a Flutter app you have already, run:

```sh
flutter pub add fynkel fynkel_flutter
```

Then make the two changes for Android below. (`fynkel create` makes a new
game with both made.) Icons show as boxes unless `pubspec.yaml` has
`uses-material-design: true` under `flutter:`.

`fynkel doctor`, run in the app's folder, says what is missing.

## Android 11 or later

Without this the build stops at "minSdkVersion 24 cannot be smaller than
version 30 declared in library [:fynkel_flutter]". In
`android/app/build.gradle.kts`, `flutter create` wrote `minSdk =
flutter.minSdkVersion`; make it 30:

```kotlin
// my_game/android/app/build.gradle.kts
android {
    defaultConfig {
        minSdk = 30 // was flutter.minSdkVersion
    }
}
```

## The Android activity

Make the game's activity a `FynkelActivity`. Then each
`FynkelView` draws straight to the screen, behind Flutter's view, and
Flutter never waits for the game's frames: on a Galaxy Note10+ that took
Sheepdog from 51 to 59 fps ([the report](https://github.com/fynkel/fynkel/blob/main/docs/android-surface-report.md)).
`flutter create` wrote a `FlutterActivity`; change it:

```kotlin
// my_game/android/app/src/main/kotlin/<your package>/MainActivity.kt
import dev.fynkel.fynkel_flutter.FynkelActivity

class MainActivity : FynkelActivity()
```

A game whose activity stays a `FlutterActivity` runs too, with its views
drawn as textures Flutter shows. What this changes for widgets around
the game is in [A HUD over the game](/flutter/hud/#on-a-phone).

## iOS and macOS

On iOS nothing is needed: each `FynkelView` draws straight to the
screen, on a layer under Flutter's view, which the plugin makes
see-through itself. Flutter then never copies the game's frames, which
keeps an iPhone cool ([the report](https://github.com/fynkel/fynkel/blob/main/docs/ios-surface-report.md)).

## Where GitHub is blocked

Behind a wall that keeps GitHub out, the engine can come from a mirror
of your own (set `HTTPS_PROXY` for a proxy). Each package that has a
library asks it, and still checks what it gets:

```yaml
# my_game/pubspec.yaml
hooks:
  user_defines:
    fynkel:
      mirror: https://example.com/fynkel/v0.1.1/
```

## With a clone of the repository

To change Fynkel itself, `fynkel create
my_game --path <the clone>` makes a game on it. The engine is then built
from source, with Rust ([rustup](https://rustup.rs)), the first time and
whenever it changes: a few minutes, with nothing shown but with `flutter
run -v`, and about 750 MB in the clone's `target/`. The README says what
rustup needs.
