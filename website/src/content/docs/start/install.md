---
title: Install and first game
description: What you need to run Fynkel, and the first game running in a few commands.
---

## What you need

- **Flutter 3.47** or later.
- **Rust**, from [rustup](https://rustup.rs). The engine is built from
  source the first time you run a game: that first build takes a few
  minutes (later ones only rebuild what changed; `flutter run -v` shows
  cargo working). The repository's `rust-toolchain.toml` picks the Rust
  version and its targets, and rustup installs them on the first build.
  Rustup 1.28.0 doesn't: run `rustup self update` first.
- For **iOS**, Xcode. For **Android**, the Android NDK, and Android 11
  (API 30) or later on a 64-bit device.

Fynkel runs on **macOS, iOS and Android**. Windows, Linux and the web
aren't supported yet.

## Run an example

Fynkel isn't on pub.dev yet: you use it from a clone of the repository.

```sh
git clone https://github.com/mooumari/fynkel.git
cd fynkel/examples/hello
flutter run -d macos
```

A cube turns in a window. The engine's build lands in `target/` in the
clone (about 750 MB) and every game made from that clone shares it, so the
next game starts in seconds.

## Your own game

Make a Flutter app beside the clone and point it at Fynkel's packages with
`path:` dependencies. [§1 of the guide](/guide/01-a-new-game/) shows the
`pubspec.yaml` and the two small changes Android needs, and
[§2](/guide/02-hello/) the first game.

Or start with the tutorial, [Your first game: Roll](/tutorials/roll/),
which makes a small whole game in half an hour.
