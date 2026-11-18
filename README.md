# Fynkel

A code-first 3D game framework for Flutter. You write the game in Dart,
the way Unity games are written (objects, components, behaviours,
`onUpdate`), and a Rust engine (wgpu) draws it into a Flutter view. There
are no scene files, prefabs or editor: the game is code.

```dart
import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Hello.new));

class Hello extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 1.5, 4), rotation: Quat.lookRotation(Vec3(0, -1.5, -4))),
        components: [Camera()],
      ),
    );
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.3))),
        components: [Light.sun()],
      ),
    );
    add(Cube());
  }
}

class Cube extends GameObject {
  Cube() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]);

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta);
  }
}
```

## Start here

- **[Writing a game](docs/writing-a-game.md)**, the guide: from a new app
  to physics, Flutter menus and tests, with a page for Unity users (§21).
- **The examples**, in [`examples/`](examples), simplest first:

  | Example | What it is |
  | --- | --- |
  | [pocket_hole](examples/pocket_hole) | A portrait block-collecting game with touch controls, progression and mobile projects |
  | [hello](examples/hello) | One turning cube: the code above |
  | [garden](examples/garden) | A character in a garden, loaded from models; an orbit camera |
  | [coins](examples/coins) | Walk about picking up coins, level after level; triggers and a Flutter HUD |
  | [arena](examples/arena) | A small top-down game: waves, health, signals, tweens, pause |
  | [sheepdog](examples/sheepdog) | A whole game that feels like an app: a title screen, pause menu, settings, rebindable keys, levels and a minimap |
  | [bowling](examples/bowling) | Ten-pin bowling on physics: rigidbodies, a mesh collider, gestures |
  | [lab](examples/lab) | One scene per feature, with buttons and sliders to try it by hand |
  | [scale](examples/scale) | Not a lesson: 10,000 objects, to measure speed |

## What you need

- Flutter 3.47 or later.
- Rust, from [rustup](https://rustup.rs). The engine is built from source:
  the **first build takes a few minutes** (later ones only rebuild what
  changed; `flutter run -v` shows cargo working). `rust-toolchain.toml`
  picks the version and its targets, and rustup installs them itself on
  the first build. Except rustup 1.28.0, which doesn't: `rustup self
  update`, or run `rustup toolchain install` in this repository once. A
  toolchain installed earlier without a target needs `rustup target add`
  (the build says which). The build uses the crate versions in
  `Cargo.lock` (`--locked`), never newer ones.
- For Android, the Android NDK, and Android 11 (API 30) or later on a
  64-bit device.

It runs on **macOS, iOS and Android**. Windows, Linux and the web aren't
supported. Fynkel isn't on pub.dev yet: use it from a clone of this
repository, with `path:` dependencies (guide §1).

## The packages

| Package | What it is |
| --- | --- |
| [`fynkel`](packages/fynkel) | The framework: objects, components, the frame, input, motion, cameras. Plain Dart, with no window: a game runs frame by frame in `flutter test` (guide §18) |
| [`fynkel_flutter`](packages/fynkel_flutter) | `FynkelApp`, `FynkelGame` and `FynkelView`: the game in a Flutter app |
| [`fynkel_physics`](packages/fynkel_physics) | Physics (Rapier), as Unity's: rigidbodies, colliders, triggers, joints, a character controller. Optional |
| [`fynkel_audio`](packages/fynkel_audio) | Sound, as Unity's: clips, sources, the listener, music and groups. Optional |
| [`fynkel_measure`](packages/fynkel_measure) | The tool that measures frame rate and memory on devices |

## For people working on Fynkel

`crates/` is the Rust engine; `docs/` keeps the plans and reports of each
stage (`stage-*.md`) and the decisions (`docs/decisions`). Fynkel 1, with
the editor and the asset pipeline, is archived as tag `fynkel-1-final` and
branch `fynkel-1`.

## Licence

Fynkel is under the [MIT licence](LICENSE). The art and sounds in the
examples and tests are other people's, each under the licence in the
`CREDITS.md` beside it (CC0, CC BY 4.0 or Apache-2.0).
