// Fynkel Code Only: the game behind the app and the tests — a small level
// in the Playground's style made from Dart alone (ADR-0026). The project
// names no startup scene: `loadProject` opens it empty, and the game puts
// its places in the world one of two ways, the same specs either way
// (lib/game/level.dart):
//
// - [BuildMode.live]: `world.build` of the specs, with the materials made
//   at runtime (`world.createMaterial`); nothing is read from a document.
// - [BuildMode.scenes]: the scene documents tool/scenes.dart wrote from
//   those specs (`package:fynkel/authoring.dart`), cooked like any scene
//   and loaded with `scenes.transition`, as a game made in the editor is.
//
// Menu → Level → Results → Menu or Level again. `GameState.screen` follows
// the places, so the app's overlay switches on it.

import 'package:fynkel/fynkel.dart';
import 'package:pickups/pickups.dart' show PickupSink;

import '../generated/assets.g.dart';
import '../generated/packages.g.dart';
import 'coin_kind.dart';
import 'flow.dart';
import 'game_state.dart';
import 'level.dart';

export 'flow.dart';

/// How the game puts its places in the world.
enum BuildMode {
  /// `world.build` of the specs at runtime.
  live,

  /// The scene documents tool/scenes.dart wrote, loaded by id.
  scenes,
}

/// [BuildMode.live]: the previous place destroyed, the next one built.
class LiveFlow extends Flow {
  LiveFlow(super.world) : look = Look((_, material) => world.createMaterial(material));

  /// The materials, made once for the world's life.
  final Look look;

  /// The kinds of coin, made in code: the same values tool/scenes.dart
  /// writes as data assets.
  final kinds = CoinKinds();
  final List<GameObject> _built = [];

  @override
  void show(Screen screen) {
    for (final o in _built) {
      if (o.isAlive) o.destroy();
    }
    _built
      ..clear()
      ..addAll(
        switch (screen) {
          Screen.menu => menu(look),
          Screen.playing => level(
              look,
              kinds,
              coinAt: (name, at, kind) => coin(look, kind, name: name, transform: Transform(position: at)),
            ),
          Screen.results => results(look),
        }.map(world.build),
      );
    state.screen.value = screen;
  }
}

/// [BuildMode.scenes]: the written scene loaded at the start of the next
/// frame; the screen follows when it is ready.
class SceneFlow extends Flow {
  SceneFlow(super.world) {
    world.scenes.addListener(_onScene);
  }

  static final _places = {
    Screen.menu: Assets.scenes.menu,
    Screen.playing: Assets.scenes.level,
    Screen.results: Assets.scenes.results,
  };

  @override
  void show(Screen screen) => world.scenes.transition(_places[screen]!);

  void _onScene(SceneState s) {
    if (s != SceneState.ready) return;
    final current = world.scenes.current;
    for (final MapEntry(key: screen, value: ref) in _places.entries) {
      if (ref.asset == current) state.screen.value = screen;
    }
  }
}

class Game {
  Game._(this.world, this.report, this.mode);

  final World world;

  /// What `World.loadProject` found (no scene: the project names none).
  final LoadReport report;
  final BuildMode mode;

  /// Opens the project at [projectDir] from its cooked cache ([cacheDir],
  /// default `<projectDir>/.fynkel/cache`) and puts the Menu up, built
  /// [mode]'s way. Throws when the cache is missing or stale (run
  /// `fynkel cook`).
  static Game load(
    String projectDir, {
    BuildMode mode = BuildMode.live,
    String? cacheDir,
    String? settingsPath,
    NativeLibrary? library,
  }) {
    final world = World.create(library: library);
    registerPackages(world.registry);
    final state = GameState();
    world.context
      ..install(state)
      ..install<PickupSink>(state);
    final report = world.loadProject(projectDir, cacheDir: cacheDir);
    if (settingsPath != null) world.useSettings(SettingsStore.open(settingsPath, world: world));
    final flow = switch (mode) {
      BuildMode.live => LiveFlow(world),
      BuildMode.scenes => SceneFlow(world),
    };
    world.context.install<Flow>(flow);
    final game = Game._(world, report, mode);
    if (mode == BuildMode.scenes) {
      // Now rather than at the next frame, so the Menu is there at once.
      world.loadScene(Assets.scenes.menu);
    } else {
      flow.toMenu();
    }
    return game;
  }

  SettingsService get settings => world.context.settings;
  GameState get state => world.context.service<GameState>();
  Flow get flow => world.context.service<Flow>();

  /// The Menu's Play and the Results' Play again.
  void play() => flow.play();

  /// Give up: the Results with what was collected.
  void finish() => flow.finish();

  void toMenu() => flow.toMenu();

  /// One host frame of [dtSeconds] with its raw [input] events.
  FramePlan frame(double dtSeconds, {List<InputEvent> input = const []}) => world.frame(dtSeconds, input: input);

  void dispose() {
    state.dispose();
    world.dispose();
  }
}
