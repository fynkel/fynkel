# Fynkel

Code-first 3D games for Flutter. You write the game in Dart, the way Unity
games are written (objects, components, behaviours, `onUpdate`), and a Rust
engine (wgpu) draws it. There are no scene files, prefabs or editor: the game
is code.

This package is the framework itself: objects, components, the frame, input,
motion, cameras, rendering. It is plain Dart, and a game runs frame by frame in
`flutter test`, without a window. To show one in an app, add
[fynkel_flutter](https://pub.dev/packages/fynkel_flutter).

## Start

```sh
dart pub global activate fynkel_cli
fynkel create my_game
cd my_game
flutter run -d macos
```

The guide is at [fynkel.com](https://fynkel.com).

## Where it runs

macOS 12, iOS 15 and Android 11 (64-bit) or later. Games are built on a Mac.
The first build for each platform downloads the engine, about 7 MB, and checks
it against the hash in this package; no Rust is needed.
