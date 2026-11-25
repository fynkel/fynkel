---
title: Install and first game
description: What you need to run Fynkel, and the first game running in four commands.
---

## What you need

- **A Mac** with **Flutter 3.47** or later
  ([install Flutter](https://docs.flutter.dev/get-started)).
- **Xcode**, for macOS and iOS.
- **Android Studio**, for Android.

Games run on **macOS 12, iOS 15 and Android 11 (64-bit)** or later. Windows,
Linux and the web aren't supported yet.

## Your first game

```sh
dart pub global activate fynkel_cli
fynkel create my_game
cd my_game
flutter run -d macos
```

A cube turns in a window: click it, and it hops. The game is in `lib/main.dart`: change the cube's
colour, save, and press `R` in the terminal to see it.

- **The first build** for each platform downloads the engine, about 7 MB.
  Later builds need no network.
- **`fynkel: command not found`?** Add `$HOME/.pub-cache/bin` to your PATH.
- **Something missing?** `fynkel doctor` checks your Mac and the game in the
  folder you run it in, and says what to do.

## On a phone

Connect it, find its name with `flutter devices`, and run
`flutter run -d <name>`. An iPhone needs a signing team once: open
`ios/Runner.xcworkspace` in Xcode, and choose yours under Signing &
Capabilities. For real speed, run with `--profile` or `--release`.

## Physics and sound

They are packages of their own. Add them when you make the game:

```sh
fynkel create my_game --physics --audio
```

or later, with `flutter pub add fynkel_physics` and
`flutter pub add fynkel_audio`.

## Next

- [Your first game: Roll](/tutorials/roll/): a small whole game in half an
  hour.
- [How a game is made](/basics/hello/), and the guide from there, one idea at
  a time.
- [Recipes](/recipes/moving/): short pieces of code for what most games need.
- [Coming from Unity](/start/from-unity/).
- [What works today](/start/what-works/), and what doesn't yet.
- [Adding Fynkel to an app](/start/existing-app/) you have.
- Something went wrong? [Troubleshooting](/start/troubleshooting/).
