# Fynkel Engine

A 3D game engine for Flutter: the runtime is Rust (`hecs` world, `wgpu`
renderer, Rapier physics, its own audio mixer) built as a library the host
drives; gameplay is Dart (`package:fynkel`: behaviours, typed native
components, input actions, scenes, HUD models) drawn into a Flutter app
through `package:fynkel_flutter`'s `FynkelGameView`; authoring is strict
JSON documents cooked by the `fynkel` CLI. macOS and iOS for 1.0.
Pre-release: nothing is published yet and the license (ADR-0005) is still
open — `docs/status.md` is the honest state of things, `docs/blueprint.md`
the plan, `docs/decisions/` the binding decisions, `docs/README.md` the
index.

Requirements: Rust (`rustup`), Flutter (its `dart` on `PATH`), Xcode with
the macOS and iOS platforms; versions in `docs/toolchain.md`.

```sh
cargo install --path apps/cli        # the `fynkel` CLI, usable from any directory (tests find it on PATH, or through FYNKEL_CLI)

cd examples/reference_game           # the Tier 1 reference game (its README)
fynkel validate                      # documents, references, lock
fynkel test --headless               # the startup scene on the Rust host, no window
fynkel test --headless --scene assets/scenes/playground.scene.json --input test/collect_all.input.json --steps 1400
fynkel render --scene assets/scenes/playground.scene.json --out /tmp/playground.png --size 1280x720 --steps 40
flutter test                         # the Dart side against the real runtime (cooks first)
fynkel run                           # validate, cook, flutter run on this Mac; -d <iphone> for a device
fynkel editor .                      # the project editor: the game compiled in, hot reload on save
fynkel editor                        # the editor's launcher: a game project you open there opens in its project editor

fynkel create my_game && cd my_game  # a new project: the starter, with AGENTS.md as the tour
```

The whole tree's green (`cargo fmt/clippy/test`, both `codegen --check`
pipelines, every Dart package, the starter, the showcase, the captures) is
`docs/status.md` §5; `docs/how.md` has the stress scene. Layout: `crates/`
the runtime, `apps/cli` and `apps/editor`, `packages/` the Dart SDK,
`hosts/` the standalone and Flutter hosts, `platform/apple` the viewport,
`examples/` the showcase and the reference game, `benchmarks/` the
captures and their results.
