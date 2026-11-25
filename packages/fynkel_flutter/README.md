# fynkel_flutter

Shows a [Fynkel](https://pub.dev/packages/fynkel) game in a Flutter app:
`FynkelApp` is a whole app that is a game, `FynkelGame` and `FynkelView` put one
among other widgets. Flutter makes the game's menus and HUD.

## Start

```sh
dart pub global activate fynkel_cli
fynkel create my_game
cd my_game
flutter run -d macos
```

`fynkel create` sets the app up. To add Fynkel to an app you have, run
`flutter pub add fynkel fynkel_flutter`, set Android's `minSdk` to 30, make its
`MainActivity` a `FynkelActivity`, and let `fynkel doctor` check it. The guide
is at [fynkel.com](https://fynkel.com).

## Where it runs

macOS 12, iOS 15 and Android 11 (64-bit) or later. Games are built on a Mac.
